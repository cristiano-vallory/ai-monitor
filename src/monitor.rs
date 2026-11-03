use crate::{api, docker, model::*};
use chrono::Utc;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

pub struct Shared {
    pub config: RwLock<Config>,
    pub resources: Mutex<ResourceSnapshot>,
    pub history: Mutex<VecDeque<ResourceSample>>,
    pub api: Mutex<ApiSnapshot>,
    pub health: Mutex<crate::health::Snapshot>,
    pub generation: AtomicU64,
    pub navigation: AtomicU8,
    pub stop: Arc<AtomicBool>,
    /// Pedidos de "Atualizar agora" (`refresh_health`), em contagem.
    pub health_asked: AtomicU64,
    /// Até que pedido a última coleta de saúde publicada atende. Gravado sob
    /// o lock de `health`, junto com a coleta.
    pub health_served: AtomicU64,
}

impl Shared {
    pub fn unstarted(config: Config) -> Arc<Self> {
        Arc::new(Self {
            config: RwLock::new(config),
            resources: Mutex::new(ResourceSnapshot::default()),
            history: Mutex::new(VecDeque::new()),
            api: Mutex::new(ApiSnapshot::default()),
            health: Mutex::new(crate::health::Snapshot::default()),
            generation: AtomicU64::new(0),
            navigation: AtomicU8::new(0),
            stop: Arc::new(AtomicBool::new(false)),
            health_asked: AtomicU64::new(0),
            health_served: AtomicU64::new(0),
        })
    }
    pub fn start(config: Config) -> Arc<Self> {
        let shared = Self::unstarted(config);
        let resources = shared.clone();
        thread::spawn(move || {
            let mut system = System::new();
            system.refresh_cpu_usage();
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_cpu(),
            );
            thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            let mut images = Vec::new();
            let mut images_at = None;
            let mut image_ids = std::collections::HashMap::new();
            let mut next_inventory = Instant::now();
            let mut generation = 0;
            let mut inventory_error = None;
            loop {
                if resources.stop.load(Ordering::Relaxed) {
                    break;
                }
                let started = Instant::now();
                let (current, config) = {
                    let config = resources.config.read().unwrap();
                    (resources.generation.load(Ordering::Acquire), config.clone())
                };
                if current != generation {
                    images.clear();
                    images_at = None;
                    image_ids.clear();
                    inventory_error = None;
                    next_inventory = Instant::now();
                    generation = current;
                }
                let mut snapshot = sample_resources(&config, &mut system);
                if snapshot.docker_error.is_none() {
                    let inventory_changed = snapshot.containers.len() != image_ids.len()
                        || snapshot
                            .containers
                            .iter()
                            .any(|c| !image_ids.contains_key(&c.id));
                    if should_poll_inventory(
                        Instant::now(),
                        next_inventory,
                        inventory_changed,
                        inventory_error.is_some(),
                    ) {
                        match docker::images(&config.docker_binary, &mut snapshot.containers) {
                            Ok(new_images) => {
                                images = new_images;
                                images_at = Some(Utc::now());
                                inventory_error = None;
                                image_ids = snapshot
                                    .containers
                                    .iter()
                                    .map(|c| (c.id.clone(), c.image_id.clone()))
                                    .collect();
                            }
                            Err(e) => inventory_error = Some(e),
                        }
                        next_inventory = Instant::now() + Duration::from_secs(15);
                    }
                    snapshot.images = images.clone();
                    snapshot.images_at = images_at;
                    snapshot.inventory_error = inventory_error.clone();
                    refresh_running_images(
                        &mut snapshot.containers,
                        &mut snapshot.images,
                        &image_ids,
                    );
                }
                {
                    let mut target = resources.resources.lock().unwrap();
                    if resources.generation.load(Ordering::Acquire) == current {
                        if let Some(at) = snapshot.at {
                            let mut history = resources.history.lock().unwrap();
                            history.push_back(ResourceSample {
                                at,
                                ai: snapshot.ai_usage(&config.container),
                                docker: snapshot.docker_total(),
                            });
                            while history
                                .front()
                                .is_some_and(|s| (at - s.at).num_seconds() > 600)
                            {
                                history.pop_front();
                            }
                        }
                        *target = snapshot;
                    }
                }
                wait(
                    &resources,
                    current,
                    started,
                    config.resource_seconds,
                    || false,
                );
            }
        });
        let server = shared.clone();
        thread::spawn(move || {
            loop {
                if server.stop.load(Ordering::Relaxed) {
                    break;
                }
                let (generation, config) = {
                    let config = server.config.read().unwrap();
                    (server.generation.load(Ordering::Acquire), config.clone())
                };
                let snapshot = api::collect_until(&config, || {
                    server.stop.load(Ordering::Relaxed)
                        || server.generation.load(Ordering::Acquire) != generation
                });
                {
                    let mut target = server.api.lock().unwrap();
                    if server.generation.load(Ordering::Acquire) == generation {
                        *target = snapshot;
                    }
                }
                wait(
                    &server,
                    generation,
                    Instant::now(),
                    config.api_seconds,
                    || false,
                );
            }
        });
        let health = shared.clone();
        thread::spawn(move || {
            health_loop(&health, |config, cancelled| {
                crate::health::collect(config, cancelled)
            })
        });
        shared
    }
    /// "Atualizar agora": pede uma coleta de saúde fora do ciclo. O coletor
    /// acorda e coleta uma vez; os pedidos feitos até o início dessa coleta
    /// são todos atendidos por ela.
    pub fn refresh_health(&self) {
        self.health_asked.fetch_add(1, Ordering::AcqRel);
    }
    pub fn apply(&self, config: Config) {
        let mut target = self.config.write().unwrap();
        *target = config;
        self.generation.fetch_add(1, Ordering::AcqRel);
        *self.resources.lock().unwrap() = ResourceSnapshot::default();
        self.history.lock().unwrap().clear();
        *self.api.lock().unwrap() = ApiSnapshot::default();
        *self.health.lock().unwrap() = crate::health::Snapshot::default();
    }
}

/// Coletor de saúde: coleta, publica se a conexão é a mesma e espera o
/// intervalo, ou até um pedido de "Atualizar agora" ainda não atendido. A
/// config é relida a cada coleta, então um pedido seguido de reconexão
/// coleta com a config nova.
pub(crate) fn health_loop(
    shared: &Shared,
    collect: impl Fn(&Config, &dyn Fn() -> bool) -> crate::health::Snapshot,
) {
    let due = || {
        shared.health_asked.load(Ordering::Acquire) > shared.health_served.load(Ordering::Acquire)
    };
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        let (generation, config, asked) = {
            let config = shared.config.read().unwrap();
            (
                shared.generation.load(Ordering::Acquire),
                config.clone(),
                shared.health_asked.load(Ordering::Acquire),
            )
        };
        let snapshot = collect(&config, &|| {
            shared.stop.load(Ordering::Relaxed)
                || shared.generation.load(Ordering::Acquire) != generation
        });
        {
            let mut target = shared.health.lock().unwrap();
            if shared.generation.load(Ordering::Acquire) == generation {
                *target = snapshot;
                shared.health_served.store(asked, Ordering::Release);
            }
        }
        wait(
            shared,
            generation,
            Instant::now(),
            crate::health::INTERVAL,
            due,
        );
    }
}

fn wait(shared: &Shared, generation: u64, started: Instant, seconds: u64, wake: impl Fn() -> bool) {
    while started.elapsed() < Duration::from_secs(seconds)
        && !shared.stop.load(Ordering::Relaxed)
        && shared.generation.load(Ordering::Acquire) == generation
        && !wake()
    {
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn sample_resources(config: &Config, system: &mut System) -> ResourceSnapshot {
    system.refresh_cpu_usage();
    system.refresh_memory();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_exe(UpdateKind::OnlyIfNotSet),
    );
    let mut result = ResourceSnapshot {
        host_cpu: system.global_cpu_usage(),
        host_memory: system.used_memory(),
        host_total: system.total_memory(),
        ..Default::default()
    };
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_string();
        let exe = process
            .exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Processes inside containers are visible on the host; only the names
        // that depend on cgroup to classify pay the /proc/<pid>/cgroup read.
        #[cfg(target_os = "linux")]
        let cgroup = needs_cgroup(&name)
            .then(|| std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok())
            .flatten();
        #[cfg(not(target_os = "linux"))]
        let cgroup: Option<String> = None;
        let Some(group) = classify(&name, &exe, cgroup.as_deref()) else {
            continue;
        };
        result.processes.push(HostProcess {
            pid: pid.as_u32(),
            name,
            group: group.into(),
            usage: Usage {
                cpu: process.cpu_usage() as f64,
                memory: process.memory(),
                tasks: thread_count(pid.as_u32()),
                ..Default::default()
            },
        });
    }
    result
        .processes
        .sort_by(|a, b| b.usage.cpu.total_cmp(&a.usage.cpu));
    match docker::containers(&config.docker_binary) {
        Ok(cs) => result.containers = cs,
        Err(e) => result.docker_error = Some(e),
    }
    if result
        .containers
        .iter()
        .any(|c| c.name == config.container && c.state == "running")
    {
        match docker::process_threads(&config.docker_binary, &config.container) {
            Ok(ps) => result.ai_processes = ps,
            Err(e) => result.ai_process_error = Some(e),
        }
    }
    result.at = Some(Utc::now());
    result
}

#[cfg(not(target_os = "linux"))]
fn classify(name: &str, exe: &str, _cgroup: Option<&str>) -> Option<&'static str> {
    if name == "ai-memory" {
        Some("ai-memory")
    } else if name.starts_with("com.docker.") || exe.contains("/Docker.app/") {
        Some("Docker Desktop")
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn needs_cgroup(name: &str) -> bool {
    name == "ai-memory" || is_engine(name)
}

#[cfg(target_os = "linux")]
fn is_engine(name: &str) -> bool {
    matches!(
        name,
        "dockerd" | "containerd" | "docker-proxy" | "rootlesskit"
    ) || name.starts_with("containerd-shim")
}

#[cfg(target_os = "linux")]
fn classify(name: &str, _exe: &str, cgroup: Option<&str>) -> Option<&'static str> {
    // sysinfo name() is the kernel comm, truncated to 15 bytes; every prefix
    // below is short enough to survive that truncation.
    if name.starts_with("com.docker.") {
        return Some("Docker Desktop");
    }
    if is_engine(name) {
        // Rodando dentro de contêiner, esses nomes não são o engine do host.
        return if containerized(cgroup) {
            None
        } else {
            Some("Docker Engine")
        };
    }
    if name == "ai-memory" && !containerized(cgroup) {
        return Some("ai-memory");
    }
    None
}

#[cfg(any(target_os = "linux", test))]
fn containerized(cgroup: Option<&str>) -> bool {
    // hidepid esconde /proc de processos alheios por inteiro; um cgroup visível
    // mas ilegível é um caso raro — contar como host é a escolha conservadora
    // para a aba de processos.
    let Some(cgroup) = cgroup else {
        return false;
    };
    cgroup.lines().any(|line| {
        let Some(path) = line.splitn(3, ':').nth(2) else {
            return false;
        };
        path.split('/').any(|segment| {
            if segment.starts_with("kubepods") {
                return true;
            }
            let mut id = segment;
            for prefix in ["docker-", "libpod-", "cri-containerd-", "crio-"] {
                if let Some(rest) = id.strip_prefix(prefix) {
                    id = rest;
                    break;
                }
            }
            let id = id.strip_suffix(".scope").unwrap_or(id);
            id.len() == 64
                && id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    })
}

#[cfg(any(target_os = "linux", test))]
fn parse_threads(status: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Threads:"))
        .and_then(|value| value.trim().parse().ok())
}

#[cfg(target_os = "linux")]
fn thread_count(pid: u32) -> Option<u64> {
    parse_threads(&std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?)
}

#[cfg(target_os = "macos")]
fn thread_count(pid: u32) -> Option<u64> {
    // Layout of proc_taskinfo in the macOS SDK's sys/proc_info.h.
    #[repr(C)]
    #[derive(Default)]
    struct TaskInfo {
        times: [u64; 6],
        counters: [i32; 12],
    }
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut std::ffi::c_void,
            size: i32,
        ) -> i32;
    }
    let mut info = TaskInfo::default();
    let size = std::mem::size_of::<TaskInfo>() as i32;
    // SAFETY: buffer is aligned, initialized and has exactly the SDK struct size.
    let read = unsafe { proc_pidinfo(pid as i32, 4, 0, (&mut info as *mut TaskInfo).cast(), size) };
    (read == size && info.counters[9] >= 0).then_some(info.counters[9] as u64)
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn thread_count(_: u32) -> Option<u64> {
    None
}

pub fn should_poll_inventory(
    now: Instant,
    next_inventory: Instant,
    inventory_changed: bool,
    has_error: bool,
) -> bool {
    now >= next_inventory || (inventory_changed && !has_error)
}

pub fn refresh_running_images(
    containers: &mut [Container],
    images: &mut [DockerImage],
    image_ids: &std::collections::HashMap<String, String>,
) {
    for c in containers.iter_mut() {
        c.image_id = image_ids.get(&c.id).cloned().unwrap_or_default();
    }
    for image in images.iter_mut() {
        image.running = containers
            .iter()
            .filter(|c| c.image_id == image.id && c.state == "running")
            .count();
    }
    docker::sort_images(images);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, mpsc};

    /// O coletor de saúde rodando numa thread, com uma coleta falsa (sem
    /// rede): cada coleta avisa em `calls` com o endpoint da config usada e
    /// espera `release` antes de devolver. A coleta devolvida traz o endpoint
    /// em `status`, mesmo se cancelada: quem descarta é o laço.
    pub(crate) struct HealthProbe {
        pub shared: Arc<Shared>,
        pub calls: mpsc::Receiver<String>,
        release: Option<mpsc::Sender<()>>,
        pub most_in_flight: Arc<AtomicUsize>,
        thread: Option<thread::JoinHandle<()>>,
    }
    impl HealthProbe {
        pub(crate) fn start(shared: Arc<Shared>) -> Self {
            let (calls_tx, calls) = mpsc::channel();
            let (release, gate) = mpsc::channel::<()>();
            let in_flight = Arc::new(AtomicUsize::new(0));
            let most_in_flight = Arc::new(AtomicUsize::new(0));
            let (now, most) = (in_flight.clone(), most_in_flight.clone());
            let looping = shared.clone();
            let thread = thread::spawn(move || {
                health_loop(&looping, |config, _cancelled| {
                    let running = now.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(running, Ordering::SeqCst);
                    let _ = calls_tx.send(config.endpoint.clone());
                    let _ = gate.recv();
                    now.fetch_sub(1, Ordering::SeqCst);
                    crate::health::Snapshot {
                        at: Some(Utc::now()),
                        status: Some(serde_json::json!({ "endpoint": config.endpoint })),
                        ..Default::default()
                    }
                })
            });
            Self {
                shared,
                calls,
                release: Some(release),
                most_in_flight,
                thread: Some(thread),
            }
        }
        /// A próxima coleta (bem antes dos 30 s do ciclo).
        pub(crate) fn next(&self) -> String {
            self.calls
                .recv_timeout(Duration::from_secs(3))
                .expect("the collector must collect now, not after the 30 s cycle")
        }
        /// Nenhuma coleta nova por 400 ms (o laço reavalia a cada 100 ms).
        pub(crate) fn quiet(&self) -> bool {
            matches!(
                self.calls.recv_timeout(Duration::from_millis(400)),
                Err(mpsc::RecvTimeoutError::Timeout)
            )
        }
        pub(crate) fn release(&self) {
            self.release.as_ref().unwrap().send(()).unwrap();
        }
        /// Espera a publicação que atende `asked` (até 3 s).
        pub(crate) fn served(&self, asked: u64) {
            let until = Instant::now() + Duration::from_secs(3);
            while self.shared.health_served.load(Ordering::Acquire) < asked {
                assert!(Instant::now() < until, "request {asked} was never served");
                thread::sleep(Duration::from_millis(2));
            }
        }
        pub(crate) fn published(&self) -> Option<String> {
            self.shared
                .health
                .lock()
                .unwrap()
                .status
                .as_ref()
                .and_then(|s| s["endpoint"].as_str().map(str::to_owned))
        }
    }
    impl Drop for HealthProbe {
        fn drop(&mut self) {
            self.shared.stop.store(true, Ordering::Relaxed);
            self.release.take();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
    fn endpoint(url: &str) -> Config {
        Config {
            endpoint: url.into(),
            ..Default::default()
        }
    }

    #[test]
    fn refresh_wakes_the_waiting_collector_once_even_after_a_double_click() {
        let probe = HealthProbe::start(Shared::unstarted(endpoint("http://old.example")));
        assert_eq!(probe.next(), "http://old.example", "startup collection");
        probe.release();
        probe.served(0);
        while probe.published().is_none() {
            thread::sleep(Duration::from_millis(2));
        }
        assert!(probe.quiet(), "precondition: waiting for the 30 s cycle");
        let asked_at = Instant::now();
        probe.shared.refresh_health();
        probe.shared.refresh_health();
        assert_eq!(probe.next(), "http://old.example");
        assert!(
            asked_at.elapsed() < Duration::from_secs(1),
            "the collector must wake right away, not at the end of the interval: {:?}",
            asked_at.elapsed()
        );
        probe.release();
        probe.served(2);
        assert!(probe.quiet(), "a double click is ONE extra collection");
        assert_eq!(probe.most_in_flight.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn refresh_during_a_collection_runs_one_more_after_it_never_in_parallel() {
        let probe = HealthProbe::start(Shared::unstarted(endpoint("http://old.example")));
        assert_eq!(
            probe.next(),
            "http://old.example",
            "cycle collection in flight"
        );
        probe.shared.refresh_health();
        probe.shared.refresh_health();
        assert!(probe.quiet(), "no second collection while one is in flight");
        probe.release();
        assert_eq!(probe.next(), "http://old.example", "the pending request");
        assert_eq!(
            probe.published().as_deref(),
            Some("http://old.example"),
            "the first collection was published before the second started"
        );
        assert_eq!(
            probe.shared.health_served.load(Ordering::Acquire),
            0,
            "the collection that started before the click does not serve it"
        );
        probe.release();
        probe.served(2);
        assert!(probe.quiet(), "both clicks were served by one collection");
        assert_eq!(
            probe.most_in_flight.load(Ordering::SeqCst),
            1,
            "never in parallel"
        );
    }

    #[test]
    fn refresh_followed_by_reconnect_never_collects_or_publishes_the_old_config() {
        let probe = HealthProbe::start(Shared::unstarted(endpoint("http://old.example")));
        assert_eq!(
            probe.next(),
            "http://old.example",
            "in flight with the old config"
        );
        probe.shared.refresh_health();
        probe.shared.apply(endpoint("http://new.example"));
        probe.release();
        assert_eq!(
            probe.next(),
            "http://new.example",
            "the request is served with the config read after the reconnection"
        );
        assert_eq!(
            probe.published(),
            None,
            "the old collection finished after the reconnection: discarded, not published"
        );
        assert_eq!(probe.shared.health_served.load(Ordering::Acquire), 0);
        probe.release();
        probe.served(1);
        assert_eq!(probe.published().as_deref(), Some("http://new.example"));
        assert!(
            probe.quiet(),
            "the reconnection and the request share one collection"
        );
    }

    #[test]
    fn refreshing_running_state_between_inventory_polls_resorts_images_and_clears_uncommitted_ids()
    {
        let mut containers = vec![
            Container {
                id: "c1".into(),
                name: "one".into(),
                state: "exited".into(),
                ..Default::default()
            },
            Container {
                id: "c2".into(),
                name: "two".into(),
                state: "running".into(),
                image_id: "sha256:uncommitted".into(),
                ..Default::default()
            },
        ];
        let mut images = vec![
            DockerImage {
                id: "sha256:img-a".into(),
                size: 100,
                containers: vec!["one".into()],
                running: 1,
                ..Default::default()
            },
            DockerImage {
                id: "sha256:img-b".into(),
                size: 200,
                containers: vec!["two".into()],
                running: 0,
                ..Default::default()
            },
        ];
        let image_ids = std::collections::HashMap::from([
            ("c1".into(), "sha256:img-a".into()),
            ("c2".into(), "sha256:img-b".into()),
        ]);
        refresh_running_images(&mut containers, &mut images, &image_ids);
        assert_eq!(containers[1].image_id, "sha256:img-b");
        assert_eq!(
            images[0].id, "sha256:img-b",
            "newly running image must sort ahead of stopped image between 15s polls"
        );
        assert_eq!(images[0].running, 1);
        assert_eq!(images[1].running, 0);

        let now = Instant::now();
        let future = now + Duration::from_secs(15);
        assert!(should_poll_inventory(now, future, true, false));
        assert!(
            !should_poll_inventory(now, future, true, true),
            "inventory error must enforce backoff until next_inventory"
        );
        assert!(should_poll_inventory(future, future, true, true));
    }

    #[test]
    fn parse_threads_reads_first_threads_line_only() {
        assert_eq!(
            parse_threads("Name:\tai-memory\nThreads:\t42\nPid:\t1\n"),
            Some(42)
        );
        assert_eq!(parse_threads("Threads:  7 \nThreads: 99\n"), Some(7));
        assert_eq!(parse_threads("Threads:\t\t5\n"), Some(5));
        assert_eq!(parse_threads(""), None);
        assert_eq!(parse_threads("Pid:\t1\n"), None);
        assert_eq!(parse_threads("Threads: abc\n"), None);
        assert_eq!(parse_threads("Threads: 18446744073709551616\n"), None);
        // A malformed first line wins: a later valid line must not be used.
        assert_eq!(parse_threads("Threads: abc\nThreads: 5\n"), None);
        assert_eq!(
            parse_threads("Threads: 18446744073709551615\n"),
            Some(u64::MAX)
        );
    }

    const H64: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn containerized_detects_container_runtimes_in_cgroup_paths() {
        let ok = [
            format!("0::/system.slice/docker-{H64}.scope"),
            format!("0::/docker/{H64}"),
            format!(
                "0::/user.slice/user-1000.slice/user@1000.service/user.slice/docker-{H64}.scope"
            ),
            format!("0::/user.slice/libpod-{H64}.scope"),
            format!("0::/kubepods.slice/kubepods-burstable.slice/cri-containerd-{H64}.scope"),
            // cgroup v1: several lines, any container path is enough.
            format!("1:name=systemd:/user.slice\n0::/docker/{H64}"),
        ];
        for cgroup in ok {
            assert!(containerized(Some(&cgroup)), "container: {cgroup}");
        }
        let host = [
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/ai-memory.service".into(),
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-Hyprland-kitty-1234.scope".into(),
            "0::/".to_owned(),
            "0::/init.scope".to_owned(),
            String::new(),
            // Boundaries: 63/65 hex chars and uppercase are not container IDs.
            format!("0::/docker/{}", &H64[..63]),
            format!("0::/docker/{H64}x"),
            format!("0::/docker/{}", H64.to_uppercase()),
        ];
        for cgroup in host {
            assert!(!containerized(Some(&cgroup)), "host: {cgroup}");
        }
        assert!(!containerized(None), "unreadable cgroup means host");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn needs_cgroup_covers_whole_domain() {
        for name in [
            "ai-memory",
            "dockerd",
            "containerd",
            "docker-proxy",
            "rootlesskit",
            "containerd-shim",
            "containerd-shim-ru", // comm truncado a 15 bytes
        ] {
            assert!(needs_cgroup(name), "{name} must read cgroup");
        }
        for name in ["docker", "com.docker.dev-envs", "firefox", "shim"] {
            assert!(!needs_cgroup(name), "{name} must not read cgroup");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_classify_engine_names_respect_container_boundary() {
        let inside = "0::/docker/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let host = "0::/system.slice/containerd.service";
        assert_eq!(classify("dockerd", "/usr/bin/dockerd", Some(inside)), None);
        assert_eq!(
            classify("dockerd", "/usr/bin/dockerd", Some(host)),
            Some("Docker Engine")
        );
        assert_eq!(
            classify("containerd-shim", "/usr/bin/shim", Some(host)),
            Some("Docker Engine")
        );
        assert_eq!(
            classify("containerd-shim", "/usr/bin/shim", Some(inside)),
            None
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_classify_covers_the_whole_process_domain() {
        let container = format!("0::/system.slice/docker-{H64}.scope");
        assert_eq!(classify("ai-memory", "", None), Some("ai-memory"));
        assert_eq!(
            classify("ai-memory", "", Some(&container)),
            None,
            "containerized ai-memory must not count as a host process"
        );
        for name in [
            "dockerd",
            "containerd",
            "docker-proxy",
            "rootlesskit",
            "containerd-shim-runc-v2",
            "containerd-shim",
        ] {
            assert_eq!(classify(name, "", None), Some("Docker Engine"), "{name}");
        }
        for name in ["com.docker.backend", "com.docker.back"] {
            assert_eq!(classify(name, "", None), Some("Docker Desktop"), "{name}");
        }
        assert_eq!(classify("docker", "", None), None, "docker CLI is a client");
        assert_eq!(classify("ai-memory-x", "", None), None);
        assert_eq!(classify("kitty", "", None), None);
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn classify_keeps_the_macos_rule() {
        assert_eq!(classify("ai-memory", "", None), Some("ai-memory"));
        assert_eq!(
            classify("com.docker.backend", "", None),
            Some("Docker Desktop")
        );
        assert_eq!(
            classify("helper", "/Applications/Docker.app/Contents/x", None),
            Some("Docker Desktop")
        );
        assert_eq!(classify("dockerd", "", None), None);
        assert_eq!(classify("kitty", "", None), None);
    }
}
