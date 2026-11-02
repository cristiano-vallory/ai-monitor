use crate::{api, docker, model::*};
use chrono::Utc;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
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
    pub stop: Arc<AtomicBool>,
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
            stop: Arc::new(AtomicBool::new(false)),
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
                wait(&resources, current, started, config.resource_seconds);
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
                wait(&server, generation, Instant::now(), config.api_seconds);
            }
        });
        let health = shared.clone();
        thread::spawn(move || {
            loop {
                if health.stop.load(Ordering::Relaxed) {
                    break;
                }
                let (generation, config) = {
                    let config = health.config.read().unwrap();
                    (health.generation.load(Ordering::Acquire), config.clone())
                };
                let snapshot = crate::health::collect(&config, || {
                    health.stop.load(Ordering::Relaxed)
                        || health.generation.load(Ordering::Acquire) != generation
                });
                {
                    let mut target = health.health.lock().unwrap();
                    if health.generation.load(Ordering::Acquire) == generation {
                        *target = snapshot;
                    }
                }
                wait(&health, generation, Instant::now(), crate::health::INTERVAL);
            }
        });
        shared
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

fn wait(shared: &Shared, generation: u64, started: Instant, seconds: u64) {
    while started.elapsed() < Duration::from_secs(seconds)
        && !shared.stop.load(Ordering::Relaxed)
        && shared.generation.load(Ordering::Acquire) == generation
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
        let group = if name == "ai-memory" {
            "ai-memory"
        } else if name.starts_with("com.docker.") || exe.contains("/Docker.app/") {
            "Docker Desktop"
        } else {
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
#[cfg(not(target_os = "macos"))]
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
mod tests {
    use super::*;

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
}
