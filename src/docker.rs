use crate::model::{Container, DockerImage, Usage};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_OUTPUT: usize = 16 * 1024 * 1024;

pub fn command(binary: &str, args: &[&str]) -> Result<String, String> {
    command_with_timeout(binary, args, Duration::from_secs(6))
}

fn command_with_timeout(binary: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(binary);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .args(args)
        // The ai-memory bearer belongs only to HTTP requests, never Docker children.
        .env_remove("AI_MEMORY_AUTH_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Docker CLI não encontrado. Confira o caminho em Conexão.".to_owned())?;
    // Drain both pipes while the command runs, avoiding pipe-buffer deadlocks.
    fn drain(mut stream: impl Read) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 8192];
        while let Ok(n) = stream.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let remaining = MAX_OUTPUT.saturating_sub(bytes.len());
            bytes.extend_from_slice(&chunk[..n.min(remaining)]);
        }
        bytes
    }
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || drain(stdout));
    let err = thread::spawn(move || drain(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(25)),
            _ => {
                #[cfg(unix)]
                {
                    unsafe extern "C" {
                        fn kill(pid: i32, signal: i32) -> i32;
                    }
                    // SAFETY: this group belongs exclusively to the child we just spawned.
                    // Kill descendants too, so inherited stdout pipes cannot block join().
                    unsafe {
                        kill(-(child.id() as i32), 9);
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let output = out.join().unwrap_or_default();
    let _ = err.join();
    match status {
        None => Err("Docker excedeu o limite de 6 s; nova tentativa automática.".into()),
        Some(s) if !s.success() => {
            Err("Comando Docker falhou. Verifique o Docker Desktop e o contexto ativo.".into())
        }
        _ if output.len() >= MAX_OUTPUT => Err("Resposta Docker excedeu 16 MiB.".into()),
        _ => String::from_utf8(output).map_err(|_| "Resposta Docker inválida.".into()),
    }
}

pub fn json_lines(raw: &str) -> Result<Vec<Value>, String> {
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|_| "Resposta JSON do Docker inválida.".into())
        })
        .collect()
}

fn field<'a>(v: &'a Value, name: &str) -> &'a str {
    v[name].as_str().unwrap_or("")
}

pub fn parse_size(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let split = raw
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(raw.len());
    let n: f64 = raw[..split].parse().ok()?;
    let multiplier = match raw[split..].trim().to_ascii_lowercase().as_str() {
        "b" | "" => 1.,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "kib" => 1024.,
        "mib" => 1048576.,
        "gib" => 1073741824.,
        "tib" => 1099511627776.,
        _ => return None,
    };
    if !n.is_finite() || n < 0. {
        return None;
    }
    Some((n * multiplier).round() as u64)
}

pub fn parse_usage(v: &Value) -> Option<Usage> {
    let cpu = field(v, "CPUPerc")
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()?;
    if !cpu.is_finite() || cpu < 0. {
        return None;
    }
    let (used, limit) = field(v, "MemUsage").split_once('/')?;
    Some(Usage {
        cpu,
        memory: parse_size(used)?,
        limit: parse_size(limit)?,
        tasks: field(v, "PIDs").parse().ok(),
    })
}

pub fn containers(binary: &str) -> Result<Vec<Container>, String> {
    let list = json_lines(&command(
        binary,
        &["ps", "-a", "--no-trunc", "--format", "{{json .}}"],
    )?)?;
    let stats = json_lines(&command(
        binary,
        &[
            "stats",
            "--no-stream",
            "--no-trunc",
            "--format",
            "{{json .}}",
        ],
    )?)?;
    let mut containers: Vec<_> = list
        .iter()
        .map(|v| {
            let stat = stats.iter().find(|s| field(s, "ID") == field(v, "ID"));
            Container {
                id: field(v, "ID").into(),
                name: field(v, "Names").into(),
                image: field(v, "Image").into(),
                state: field(v, "State").into(),
                status: field(v, "Status").into(),
                ports: field(v, "Ports").into(),
                usage: stat.and_then(parse_usage),
                net_io: stat.map(|s| field(s, "NetIO").into()).unwrap_or_default(),
                block_io: stat.map(|s| field(s, "BlockIO").into()).unwrap_or_default(),
                ..Default::default()
            }
        })
        .collect();
    containers.sort_by(|a, b| {
        (b.state == "running")
            .cmp(&(a.state == "running"))
            .then_with(|| {
                b.usage
                    .as_ref()
                    .map(|u| u.cpu)
                    .unwrap_or(0.)
                    .total_cmp(&a.usage.as_ref().map(|u| u.cpu).unwrap_or(0.))
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(containers)
}

pub fn images(binary: &str, containers: &mut [Container]) -> Result<Vec<DockerImage>, String> {
    if !containers.is_empty() {
        let mut args = vec!["container", "inspect", "--format", "{{.Id}} {{.Image}}"];
        args.extend(containers.iter().map(|c| c.id.as_str()));
        let raw = command(binary, &args)?;
        for line in raw.lines() {
            let mut fields = line.split_whitespace();
            if let (Some(id), Some(image)) = (fields.next(), fields.next())
                && let Some(c) = containers.iter_mut().find(|c| c.id == id)
            {
                c.image_id = image.into();
            }
        }
    }
    let rows = json_lines(&command(
        binary,
        &["image", "ls", "--no-trunc", "--format", "{{json .}}"],
    )?)?;
    Ok(group_images(&rows, containers))
}

pub fn group_images(rows: &[Value], containers: &[Container]) -> Vec<DockerImage> {
    let mut images = BTreeMap::<String, DockerImage>::new();
    for row in rows {
        let id = field(row, "ID").to_string();
        let image = images.entry(id.clone()).or_insert_with(|| {
            let matched: Vec<_> = containers.iter().filter(|c| c.image_id == id).collect();
            DockerImage {
                id,
                size: parse_size(field(row, "Size")).unwrap_or(0),
                containers: matched.iter().map(|c| c.name.clone()).collect(),
                running: matched.iter().filter(|c| c.state == "running").count(),
                ..Default::default()
            }
        });
        let tag = format!("{}:{}", field(row, "Repository"), field(row, "Tag"));
        if !image.tags.contains(&tag) {
            image.tags.push(tag);
        }
    }
    let mut result: Vec<_> = images.into_values().collect();
    result.sort_by(|a, b| {
        b.running
            .cmp(&a.running)
            .then(b.containers.len().cmp(&a.containers.len()))
            .then(b.size.cmp(&a.size))
    });
    result
}

pub fn process_threads(
    binary: &str,
    container: &str,
) -> Result<Vec<(String, String, u64)>, String> {
    let raw = command(binary, &["top", container, "-eo", "pid,comm,nlwp"])?;
    let rows: Vec<_> = raw
        .lines()
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<_> = line.split_whitespace().collect();
            if columns.len() < 3 {
                return None;
            }
            Some((
                columns[0].into(),
                columns[1..columns.len() - 1].join(" "),
                columns.last()?.parse().ok()?,
            ))
        })
        .collect();
    if rows.is_empty() {
        return Err("Detalhamento de threads indisponível neste contêiner.".into());
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    #[cfg(unix)]
    fn docker_children_do_not_inherit_ai_memory_bearer() {
        const CHILD: &str = "AI_MONITOR_TEST_ENV_CHILD";
        if std::env::var_os(CHILD).is_some() {
            // A real subprocess checks inheritance without mutating this test runner's environment.
            assert!(command("/usr/bin/printenv", &["AI_MEMORY_AUTH_TOKEN"]).is_err());
            assert_eq!(command("/usr/bin/printenv", &[CHILD]).unwrap().trim(), "1");
            return;
        }
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "docker::tests::docker_children_do_not_inherit_ai_memory_bearer",
            ])
            .env(CHILD, "1")
            .env("AI_MEMORY_AUTH_TOKEN", "synthetic-test-bearer")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "Bearer isolation subprocess failed"
        );
    }
    #[test]
    #[cfg(unix)]
    fn timeout_reaps_the_command_and_its_pipe_holding_children() {
        let started = Instant::now();
        let result = command_with_timeout(
            "/bin/sh",
            &["-c", "sleep 20 & wait"],
            Duration::from_millis(80),
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn sizes_and_unavailable_values() {
        assert_eq!(parse_size("277.4MiB"), Some(290874982));
        assert_eq!(parse_size("1.5 GB"), Some(1500000000));
        assert_eq!(parse_size("--"), None);
        assert_eq!(parse_size("NaN"), None);
        assert!(parse_usage(&json!({"CPUPerc":"--", "MemUsage":"0B / 0B"})).is_none());
        let u = parse_usage(&json!({"CPUPerc":"235.14%", "MemUsage":"1GiB / 8GiB", "PIDs":"45"}))
            .unwrap();
        assert_eq!(u.cpu, 235.14);
        assert_eq!(u.tasks, Some(45));
    }
    #[test]
    fn image_aliases_are_not_double_counted_and_stopped_containers_still_use_images() {
        let rows = vec![
            json!({"ID":"sha256:abc","Repository":"app","Tag":"latest","Size":"1GB"}),
            json!({"ID":"sha256:abc","Repository":"app","Tag":"v1","Size":"1GB"}),
        ];
        let cs = vec![Container {
            name: "stopped".into(),
            image_id: "sha256:abc".into(),
            state: "exited".into(),
            ..Default::default()
        }];
        let result = group_images(&rows, &cs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tags.len(), 2);
        assert_eq!(result[0].containers, vec!["stopped"]);
        assert_eq!(result[0].running, 0);
    }
}
