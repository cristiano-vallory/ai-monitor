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
    #[serde(skip)]
    pub human: Option<crate::auth::Session>,
    pub remember_token: bool,
    pub proposal_watch: Option<crate::client::Scope>,
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
            human: None,
            remember_token: false,
            proposal_watch: None,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Library/Application Support/AI Monitor/config.json")
    }
    pub fn normalize(&mut self) {
        self.endpoint = self.endpoint.trim().into();
        self.token = self.token.trim().into();
        self.container = self.container.trim().into();
        self.docker_binary = self.docker_binary.trim().into();
    }
    pub fn from_sources(
        disk: Option<&[u8]>,
        env_url: Option<String>,
        env_token: Option<String>,
    ) -> Self {
        let token = env_token.unwrap_or_default().trim().to_owned();
        let disk_cfg: Option<Self> = disk.and_then(|b| serde_json::from_slice(b).ok());
        for base in [disk_cfg, Some(Self::default())].into_iter().flatten() {
            if let Some(url) = &env_url {
                let mut candidate = base.clone();
                candidate.endpoint = url.clone();
                candidate.token = token.clone();
                candidate.normalize();
                if candidate.validate().is_ok() {
                    return candidate;
                }
            }
            let mut candidate = base;
            candidate.token = token.clone();
            candidate.normalize();
            if candidate.validate().is_ok() {
                return candidate;
            }
        }
        Self {
            token,
            ..Self::default()
        }
    }
    pub fn load_from(
        disk: Option<&[u8]>,
        env_url: Option<String>,
        env_token: Option<std::ffi::OsString>,
        keychain: impl FnOnce(&str) -> Option<String>,
    ) -> Self {
        let explicit = env_token.is_some();
        let mut config =
            Self::from_sources(disk, env_url, env_token.and_then(|t| t.into_string().ok()));
        if !explicit {
            let endpoint = config.endpoint.clone();
            config.apply_keychain_token(|| keychain(&endpoint));
        }
        config
    }
    pub fn load() -> Self {
        Self::load_from(
            std::fs::read(Self::path()).ok().as_deref(),
            std::env::var("AI_MEMORY_SERVER_URL").ok(),
            std::env::var_os("AI_MEMORY_AUTH_TOKEN"),
            |e| crate::keychain::load(e).ok().flatten(),
        )
    }
    pub fn apply_keychain_token(&mut self, load: impl FnOnce() -> Option<String>) {
        if self.remember_token
            && self.token.is_empty()
            && let Some(token) = load()
        {
            let token = token.trim().to_owned();
            if !token.is_empty() {
                self.token = token;
                if self.validate().is_err() {
                    self.token.clear();
                }
            }
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Some(scope) = &self.proposal_watch {
            scope.validate().map_err(|e| e.to_string())?;
        }
        let endpoint = self.endpoint.trim();
        let token = self.token.trim();
        let url = reqwest::Url::parse(endpoint).map_err(|_| "URL do servidor inválida.")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Use uma URL HTTP(S), sem credenciais, query ou fragmento.".into());
        }
        if (!token.is_empty() || self.human.is_some())
            && url.scheme() == "http"
            && !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        {
            return Err("Use HTTPS para enviar um token a um servidor remoto.".into());
        }
        if self.container.trim().is_empty()
            || self.container.trim().starts_with('-')
            || self.docker_binary.trim().is_empty()
            || self.docker_binary.trim().starts_with('-')
        {
            return Err("Informe um contêiner e um executável do Docker válidos.".into());
        }
        if !(1..=60).contains(&self.resource_seconds) || !(5..=300).contains(&self.api_seconds) {
            return Err("Recursos: 1–60 s. API: 5–300 s.".into());
        }
        Ok(())
    }
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        let Some(parent) = path.parent().filter(|_| path.is_absolute()) else {
            return Err("Diretório HOME indisponível para salvar a configuração.".into());
        };
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        let mut normalized = self.clone();
        normalized.normalize();
        let bytes = serde_json::to_vec_pretty(&normalized).unwrap();
        #[cfg(unix)]
        {
            use std::{
                io::Write,
                os::unix::fs::{OpenOptionsExt, PermissionsExt},
            };
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&tmp)
                .map_err(|e| e.to_string())?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
        }
        #[cfg(not(unix))]
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
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
            && let Some(c) = self
                .containers
                .iter()
                .find(|c| c.name == name && c.state == "running")
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

    #[test]
    fn exited_container_falls_back_to_native_ai_memory_while_running_container_takes_precedence() {
        let mut s = ResourceSnapshot {
            at: Some(Utc::now()),
            containers: vec![Container {
                name: "ai-memory".into(),
                state: "exited".into(),
                usage: None,
                ..Default::default()
            }],
            processes: vec![HostProcess {
                pid: 42,
                name: "ai-memory".into(),
                group: "ai-memory".into(),
                usage: Usage {
                    cpu: 42.5,
                    memory: 2048,
                    tasks: Some(4),
                    ..Default::default()
                },
            }],
            ..Default::default()
        };
        let fallback = s
            .ai_usage("ai-memory")
            .expect("exited container must not block native ai-memory fallback");
        assert_eq!(fallback.cpu, 42.5);
        assert_eq!(fallback.memory, 2048);
        assert_eq!(fallback.tasks, Some(4));

        // When the container is actually running, it takes precedence over host processes.
        s.containers[0].state = "running".into();
        assert!(s.ai_usage("ai-memory").is_none());
        s.containers[0].usage = Some(Usage {
            cpu: 110.0,
            memory: 4096,
            tasks: Some(9),
            ..Default::default()
        });
        assert_eq!(s.ai_usage("ai-memory").unwrap().cpu, 110.0);
    }

    #[test]
    fn config_preserves_env_overrides_when_disk_is_invalid_and_trims_fields() {
        // Invalid resource_seconds on disk must not wipe out valid env URL and token.
        let loaded = Config::from_sources(
            Some(br#"{"resource_seconds":0,"endpoint":"http://remote.example.com"}"#),
            Some("  https://example.com  ".into()),
            Some("  synthetic-env-token  ".into()),
        );
        assert_eq!(loaded.endpoint, "https://example.com");
        assert_eq!(loaded.token, "synthetic-env-token");
        assert_eq!(loaded.resource_seconds, 2);

        // Remote HTTP on disk (valid without token) must fall back to loopback default
        // without dropping the environment bearer token.
        let loaded = Config::from_sources(
            Some(br#"{"endpoint":"http://remote.example.com:49374"}"#),
            None,
            Some("synthetic-env-token".into()),
        );
        assert_eq!(loaded.endpoint, "http://127.0.0.1:49374");
        assert_eq!(loaded.token, "synthetic-env-token");

        // Invalid endpoint on disk overridden by valid AI_MEMORY_SERVER_URL preserves other valid disk settings.
        let loaded = Config::from_sources(
            Some(br#"{"endpoint":"not-a-url","container":"custom-ai","resource_seconds":5}"#),
            Some("https://example.com".into()),
            Some("synthetic-env-token".into()),
        );
        assert_eq!(loaded.endpoint, "https://example.com");
        assert_eq!(loaded.container, "custom-ai");
        assert_eq!(loaded.resource_seconds, 5);
        assert_eq!(loaded.token, "synthetic-env-token");
    }

    #[test]
    #[cfg(unix)]
    fn config_save_restricts_permissions_and_rejects_relative_path() {
        use std::os::unix::fs::PermissionsExt;
        assert!(
            Config::default()
                .save_to(std::path::Path::new("relative/config.json"))
                .is_err()
        );
        let dir = std::env::temp_dir().join(format!("ai-monitor-test-{}", std::process::id()));
        let path = dir.join("config.json");
        let _ = std::fs::remove_dir_all(&dir);
        Config::default().save_to(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn keychain_token_is_refused_for_insecure_remote_http_and_normalized() {
        let mut remote_http = Config {
            endpoint: "http://remote.example.com:49374".into(),
            remember_token: true,
            ..Default::default()
        };
        remote_http.apply_keychain_token(|| Some("  synthetic-keychain-token  ".into()));
        assert!(
            remote_http.token.is_empty(),
            "keychain token must not be attached to plaintext remote HTTP endpoint"
        );
        assert!(remote_http.validate().is_ok());

        let mut remote_https = Config {
            endpoint: "https://remote.example.com:49374".into(),
            remember_token: true,
            ..Default::default()
        };
        remote_https.apply_keychain_token(|| Some("  synthetic-keychain-token  ".into()));
        assert_eq!(remote_https.token, "synthetic-keychain-token");
        assert!(remote_https.validate().is_ok());
    }

    #[test]
    fn keychain_is_only_read_when_remember_is_enabled_and_no_env_token() {
        use std::cell::Cell;
        let calls = Cell::new(0u32);
        let keychain = |_: &str| {
            calls.set(calls.get() + 1);
            Some("  synthetic-keychain-token  ".to_owned())
        };

        let off_disk = serde_json::to_vec(&Config {
            endpoint: "http://localhost:49374".into(),
            remember_token: false,
            ..Default::default()
        })
        .unwrap();
        let cfg = Config::load_from(Some(&off_disk), None, None, keychain);
        assert_eq!(
            calls.get(),
            0,
            "remember_token=false must not read Keychain"
        );
        assert!(cfg.token.is_empty());

        let on_disk = serde_json::to_vec(&Config {
            endpoint: "http://localhost:49374".into(),
            remember_token: true,
            ..Default::default()
        })
        .unwrap();
        let cfg = Config::load_from(Some(&on_disk), None, None, keychain);
        assert_eq!(calls.get(), 1, "remember_token=true reads Keychain once");
        assert_eq!(cfg.token, "synthetic-keychain-token");

        let cfg = Config::load_from(
            Some(&on_disk),
            None,
            Some(std::ffi::OsString::from("")),
            keychain,
        );
        assert_eq!(
            calls.get(),
            1,
            "explicit env token (even empty) skips Keychain"
        );
        assert!(cfg.token.is_empty());

        use std::os::unix::ffi::OsStringExt;
        let cfg = Config::load_from(
            Some(&on_disk),
            None,
            Some(std::ffi::OsString::from_vec(vec![0x66, 0xff])),
            keychain,
        );
        assert_eq!(
            calls.get(),
            1,
            "non-UTF-8 env token still counts as explicit"
        );
        assert!(cfg.token.is_empty());
    }
}
