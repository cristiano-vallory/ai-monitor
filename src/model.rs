use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub endpoint: String,
    pub container: String,
    pub docker_binary: String,
    pub resource_seconds: u64,
    pub api_seconds: u64,
    #[serde(skip)]
    pub token: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:49374".into(),
            container: "ai-memory".into(),
            docker_binary: [
                "/usr/local/bin/docker",
                "/opt/homebrew/bin/docker",
                "/Applications/Docker.app/Contents/Resources/bin/docker",
            ]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .unwrap_or("docker")
            .into(),
            resource_seconds: 2,
            api_seconds: 10,
            token: String::new(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Library/Application Support/AI Monitor/config.json")
    }
    pub fn load() -> Self {
        let mut config: Self = std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if let Ok(url) = std::env::var("AI_MEMORY_SERVER_URL") {
            config.endpoint = url;
        }
        config.token = std::env::var("AI_MEMORY_AUTH_TOKEN").unwrap_or_default();
        if config.validate().is_err() {
            return Self::default();
        }
        config
    }
    pub fn validate(&self) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.endpoint).map_err(|_| "URL do servidor inválida.")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Use uma URL HTTP(S), sem credenciais, query ou fragmento.".into());
        }
        if !self.token.is_empty()
            && url.scheme() == "http"
            && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        {
            return Err("Use HTTPS para enviar um token a um servidor remoto.".into());
        }
        if self.container.trim().is_empty() || self.docker_binary.trim().is_empty() {
            return Err("Informe o contêiner e o executável do Docker.".into());
        }
        if !(1..=60).contains(&self.resource_seconds) || !(5..=300).contains(&self.api_seconds) {
            return Err("Recursos: 1–60 s. API: 5–300 s.".into());
        }
        Ok(())
    }
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap())
            .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, path).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Usage {
    pub cpu: f64,
    pub memory: u64,
    pub limit: u64,
    pub tasks: Option<u64>,
}

#[derive(Clone)]
pub struct ResourceSample {
    pub at: DateTime<Utc>,
    pub ai: Option<Usage>,
    pub docker: Option<Usage>,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub image_id: String,
    pub state: String,
    pub status: String,
    pub ports: String,
    pub usage: Option<Usage>,
    pub net_io: String,
    pub block_io: String,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct DockerImage {
    pub id: String,
    pub tags: Vec<String>,
    pub size: u64,
    pub containers: Vec<String>,
    pub running: usize,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct HostProcess {
    pub pid: u32,
    pub name: String,
    pub group: String,
    pub usage: Usage,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct ResourceSnapshot {
    pub at: Option<DateTime<Utc>>,
    pub containers: Vec<Container>,
    pub images: Vec<DockerImage>,
    pub docker_error: Option<String>,
    pub inventory_error: Option<String>,
    pub images_at: Option<DateTime<Utc>>,
    pub host_cpu: f32,
    pub host_memory: u64,
    pub host_total: u64,
    pub processes: Vec<HostProcess>,
    pub ai_processes: Vec<(String, String, u64)>,
    pub ai_process_error: Option<String>,
}

impl ResourceSnapshot {
    pub fn docker_total(&self) -> Option<Usage> {
        if self.docker_error.is_some() || self.at.is_none() {
            return None;
        }
        let mut total = Usage {
            tasks: Some(0),
            ..Default::default()
        };
        for c in self.containers.iter().filter(|c| c.state == "running") {
            let u = c.usage.as_ref()?;
            total.cpu += u.cpu;
            total.memory += u.memory;
            total.tasks = total.tasks.zip(u.tasks).map(|(a, b)| a + b);
        }
        Some(total)
    }
    pub fn ai_usage(&self, name: &str) -> Option<Usage> {
        if self.docker_error.is_none()
            && let Some(c) = self.containers.iter().find(|c| c.name == name)
        {
            return c.usage.clone();
        }
        let native: Vec<_> = self
            .processes
            .iter()
            .filter(|p| p.group == "ai-memory")
            .collect();
        if native.is_empty() {
            return None;
        }
        Some(Usage {
            cpu: native.iter().map(|p| p.usage.cpu).sum(),
            memory: native.iter().map(|p| p.usage.memory).sum(),
            tasks: native
                .iter()
                .map(|p| p.usage.tasks)
                .try_fold(0, |a, b| b.map(|v| a + v)),
            ..Default::default()
        })
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Project {
    pub workspace_name: String,
    pub project_name: String,
    #[serde(default)]
    pub page_count: u64,
    #[serde(default)]
    pub session_count: Option<u64>,
    #[serde(default)]
    pub observation_count: Option<u64>,
    #[serde(default)]
    pub open_session_count: Option<u64>,
    pub last_activity: Option<String>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Session {
    pub session_id: String,
    pub agent_kind: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub observation_count: u64,
    #[serde(default)]
    pub consolidation: Option<serde_json::Value>,
    #[serde(default)]
    pub usage: Option<serde_json::Value>,
    #[serde(skip_deserializing)]
    pub scope: String,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Handoff {
    pub id: String,
    pub agent: String,
    pub at: String,
    pub state: String,
    pub summary: Option<String>,
    #[serde(default)]
    pub redacted: bool,
    #[serde(default)]
    pub next_steps: Option<Vec<String>>,
    #[serde(skip_deserializing)]
    pub scope: String,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct ApiSnapshot {
    pub at: Option<DateTime<Utc>>,
    pub version: String,
    pub projects: Vec<Project>,
    pub sessions: Vec<Session>,
    pub handoffs: Vec<Handoff>,
    pub handoff_total: Option<usize>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub error: Option<String>,
}
impl ApiSnapshot {
    pub fn open_session_links(&self) -> Option<u64> {
        self.at?;
        self.projects
            .iter()
            .try_fold(0u64, |a, p| p.open_session_count.map(|n| a + n))
    }
    pub fn open_sessions(&self) -> usize {
        self.sessions
            .iter()
            .filter(|s| s.ended_at.is_none())
            .map(|s| &s.session_id)
            .collect::<HashSet<_>>()
            .len()
    }
}

pub fn bytes(n: u64) -> String {
    let (n, suffix) = if n >= 1 << 30 {
        (n as f64 / (1u64 << 30) as f64, "GiB")
    } else if n >= 1 << 20 {
        (n as f64 / (1u64 << 20) as f64, "MiB")
    } else if n >= 1 << 10 {
        (n as f64 / 1024., "KiB")
    } else {
        return format!("{n} B");
    };
    format!("{n:.1} {suffix}")
}

pub fn number(n: usize) -> String {
    let s = n.to_string();
    s.chars()
        .enumerate()
        .fold(String::new(), |mut out, (i, c)| {
            if i > 0 && (s.len() - i).is_multiple_of(3) {
                out.push('.');
            }
            out.push(c);
            out
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_stats_and_disconnection_are_not_zero_usage() {
        let mut s = ResourceSnapshot {
            at: Some(Utc::now()),
            containers: vec![Container {
                state: "running".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(s.docker_total().is_none());
        s.containers[0].usage = Some(Usage {
            cpu: 250.,
            memory: 1024,
            tasks: Some(12),
            ..Default::default()
        });
        assert_eq!(s.docker_total().unwrap().cpu, 250.);
        s.docker_error = Some("Offline".into());
        assert!(s.docker_total().is_none());
    }
}
