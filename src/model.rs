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
    pub theme: Appearance,
}

/// Aparência escolhida em Ajustes. Um valor desconhecido ou de tipo errado
/// no disco vira `System` sem derrubar a leitura do resto da configuração.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}
impl<'de> Deserialize<'de> for Appearance {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(
            match serde_json::Value::deserialize(deserializer)?.as_str() {
                Some("light") => Self::Light,
                Some("dark") => Self::Dark,
                _ => Self::System,
            },
        )
    }
}

#[cfg(target_os = "macos")]
fn docker_binary_candidates() -> &'static [&'static str] {
    &[
        "/usr/local/bin/docker",
        "/opt/homebrew/bin/docker",
        "/Applications/Docker.app/Contents/Resources/bin/docker",
    ]
}
#[cfg(target_os = "linux")]
fn docker_binary_candidates() -> &'static [&'static str] {
    &["/usr/bin/docker", "/usr/local/bin/docker"]
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn docker_binary_candidates() -> &'static [&'static str] {
    &["/usr/local/bin/docker", "/usr/bin/docker"]
}

#[cfg(target_os = "macos")]
fn config_path(home: Option<std::ffi::OsString>, _xdg: Option<std::ffi::OsString>) -> PathBuf {
    PathBuf::from(home.unwrap_or_default())
        .join("Library/Application Support/AI Monitor/config.json")
}
#[cfg(not(target_os = "macos"))]
fn config_path(home: Option<std::ffi::OsString>, xdg: Option<std::ffi::OsString>) -> PathBuf {
    let xdg = xdg
        .filter(|v| !v.is_empty() && PathBuf::from(v).is_absolute())
        .map(PathBuf::from);
    xdg.unwrap_or_else(|| PathBuf::from(home.unwrap_or_default()).join(".config"))
        .join("ai-monitor/config.json")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:49374".into(),
            container: "ai-memory".into(),
            docker_binary: docker_binary_candidates()
                .iter()
                .copied()
                .find(|p| std::path::Path::new(p).exists())
                .unwrap_or("docker")
                .into(),
            resource_seconds: 2,
            api_seconds: 60,
            token: String::new(),
            human: None,
            remember_token: false,
            proposal_watch: None,
            theme: Appearance::System,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        config_path(
            std::env::var_os("HOME"),
            std::env::var_os("XDG_CONFIG_HOME"),
        )
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
        let mut normalized = self.clone();
        normalized.normalize();
        write_private(path, &serde_json::to_vec_pretty(&normalized).unwrap())
    }
}

const NO_HOME: &str = "Diretório HOME indisponível para salvar a configuração.";

/// Gravação atômica (tmp + rename) com permissão 0600, para a config inteira
/// e para a troca de tema.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let Some(parent) = path.parent().filter(|_| path.is_absolute()) else {
        return Err(NO_HOME.into());
    };
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
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
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}

pub fn save_theme(theme: Appearance) -> Result<(), String> {
    save_theme_to(&Config::path(), theme)
}

/// Grava só a chave `theme`: os demais bytes do arquivo ficam como estão,
/// inclusive chaves desconhecidas e um endpoint vindo do ambiente nunca vai
/// para o disco por aqui. Arquivo ilegível, JSON inválido ou que não seja
/// objeto não é tocado; o erro volta para a tela e o tema vale só na sessão.
pub fn save_theme_to(path: &std::path::Path, theme: Appearance) -> Result<(), String> {
    use serde_json::Value;
    if !path.is_absolute() {
        return Err(NO_HOME.into());
    }
    let value = serde_json::to_string(&theme).unwrap();
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let fresh = serde_json::json!({ "theme": theme });
            return write_private(path, &serde_json::to_vec_pretty(&fresh).unwrap());
        }
        Err(e) => {
            return Err(format!(
                "Tema aplicado só nesta sessão: não foi possível ler a configuração ({e})."
            ));
        }
    };
    let invalid = || {
        "Tema aplicado só nesta sessão: a configuração em disco não é um objeto JSON válido e não foi alterada."
            .to_owned()
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return Err(invalid());
    };
    let Ok(Value::Object(mut expected)) = serde_json::from_str::<Value>(&text) else {
        return Err(invalid());
    };
    expected.insert("theme".into(), serde_json::to_value(theme).unwrap());
    // A edição é textual; antes de gravar, o resultado tem de ler como o
    // objeto original com o tema trocado.
    match patch_theme(&text, &value) {
        Some(patched)
            if serde_json::from_str::<Value>(&patched).ok() == Some(Value::Object(expected)) =>
        {
            write_private(path, patched.as_bytes())
        }
        _ => Err("Tema aplicado só nesta sessão: não foi possível trocar só o tema sem alterar o resto da configuração; nada foi gravado.".into()),
    }
}

/// Troca o valor de todo membro `"theme"` do objeto de topo, ou insere um no
/// fim, mexendo só nesses bytes. Supõe JSON já validado; `None` se não for.
fn patch_theme(text: &str, value: &str) -> Option<String> {
    let b = text.as_bytes();
    let skip_ws = |mut i: usize| {
        while b.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        i
    };
    let string_end = |mut i: usize| -> Option<usize> {
        i += 1;
        loop {
            match *b.get(i)? {
                b'"' => return Some(i + 1),
                b'\\' => i += 2,
                _ => i += 1,
            }
        }
    };
    let value_end = |mut i: usize| -> Option<usize> {
        match *b.get(i)? {
            b'"' => string_end(i),
            b'{' | b'[' => {
                let mut depth = 0usize;
                loop {
                    match *b.get(i)? {
                        b'"' => {
                            i = string_end(i)?;
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth = depth.checked_sub(1)?;
                            if depth == 0 {
                                return Some(i + 1);
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            _ => {
                while b
                    .get(i)
                    .is_some_and(|c| !matches!(c, b',' | b'}' | b']') && !c.is_ascii_whitespace())
                {
                    i += 1;
                }
                Some(i)
            }
        }
    };
    let open = skip_ws(0);
    if b.get(open) != Some(&b'{') {
        return None;
    }
    let mut i = open + 1;
    let mut spans = Vec::new();
    let mut last_value_end = None;
    loop {
        i = skip_ws(i);
        match *b.get(i)? {
            b'}' => break,
            b'"' => {}
            _ => return None,
        }
        let key_end = string_end(i)?;
        let key: String = serde_json::from_str(text.get(i..key_end)?).ok()?;
        i = skip_ws(key_end);
        if b.get(i) != Some(&b':') {
            return None;
        }
        let start = skip_ws(i + 1);
        i = value_end(start)?;
        if key == "theme" {
            spans.push((start, i));
        }
        last_value_end = Some(i);
        i = skip_ws(i);
        if b.get(i) == Some(&b',') {
            i += 1;
        }
    }
    let mut out = text.to_owned();
    if spans.is_empty() {
        match last_value_end {
            Some(end) => out.insert_str(end, &format!(",\n  \"theme\": {value}")),
            None => out.insert_str(open + 1, &format!("\"theme\": {value}")),
        }
    } else {
        for (start, end) in spans.into_iter().rev() {
            out.replace_range(start..end, value);
        }
    }
    Some(out)
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
        return format!("{} B", number(n as usize));
    };
    format!("{} {suffix}", decimal(n, 1))
}

pub fn number(n: usize) -> String {
    group(&n.to_string())
}

/// Decimal em pt-BR: vírgula decimal e ponto de milhar. O arredondamento é
/// feito antes de agrupar, então 999,95 com uma casa vira `1.000,0`.
pub fn decimal(x: f64, places: usize) -> String {
    if !x.is_finite() {
        return "—".into();
    }
    let fixed = format!("{:.*}", places, x.abs());
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
    // Sem "-0,0": o sinal só aparece se sobrar algum dígito diferente de zero.
    let negative = x < 0. && fixed.bytes().any(|b| matches!(b, b'1'..=b'9'));
    let mut out = String::from(if negative { "-" } else { "" });
    out.push_str(&group(int));
    if !frac.is_empty() {
        out.push(',');
        out.push_str(frac);
    }
    out
}

/// Como `decimal`, com até `max_places` casas e sem zeros à direita: inteiros
/// saem sem vírgula (`1.000`), frações curtas (`1.234,5`).
pub fn decimal_up_to(x: f64, max_places: usize) -> String {
    let s = decimal(x, max_places);
    match s.split_once(',') {
        Some((int, frac)) => match frac.trim_end_matches('0') {
            "" => int.to_owned(),
            frac => format!("{int},{frac}"),
        },
        None => s,
    }
}

/// Valor de métrica em pt-BR: até 3 casas a partir de 1; abaixo de 1, 3
/// algarismos significativos; nunca mostra um valor não nulo como zero.
pub fn metric_value(x: f64) -> String {
    if !x.is_finite() {
        return "—".into();
    }
    let a = x.abs();
    if a == 0. {
        return "0".into();
    }
    if a >= 1. {
        return decimal_up_to(x, 3);
    }
    if a >= 1e-9 {
        // 3 algarismos significativos: 2 casas além da primeira não nula.
        return decimal_up_to(x, (2. - a.log10().floor()) as usize);
    }
    // Abaixo de 1e-9 a forma decimal passaria de 11 casas: usa a científica.
    let sci = format!("{x:.2e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let mantissa = mantissa.replace('.', ",");
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches(',');
    format!("{mantissa}e{exponent}")
}

fn group(digits: &str) -> String {
    digits
        .chars()
        .enumerate()
        .fold(String::new(), |mut out, (i, c)| {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
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
    fn api_interval_defaults_to_60_s_and_respects_saved_values() {
        assert_eq!(
            Config::default().api_seconds,
            60,
            "default API interval must be 60 s"
        );
        // `container` diferente do padrão prova que a config do disco foi
        // carregada, e não descartada em favor do padrão.
        let loaded = Config::from_sources(Some(br#"{"container":"custom-ai"}"#), None, None);
        assert_eq!(loaded.container, "custom-ai");
        assert_eq!(
            loaded.api_seconds, 60,
            "missing key falls back to the default"
        );
        let loaded = Config::from_sources(
            Some(br#"{"container":"custom-ai","api_seconds":10}"#),
            None,
            None,
        );
        assert_eq!(loaded.container, "custom-ai");
        assert_eq!(
            loaded.api_seconds, 10,
            "a saved choice is kept, no migration"
        );
        for edge in [5, 300] {
            let disk = format!(r#"{{"container":"custom-ai","api_seconds":{edge}}}"#);
            let loaded = Config::from_sources(Some(disk.as_bytes()), None, None);
            assert_eq!(loaded.api_seconds, edge, "limit {edge} is accepted");
        }
        // Fora de 5–300 a config do disco inteira é descartada (mesmo caminho
        // do resource_seconds inválido): vale o padrão, inclusive o contêiner.
        for invalid in [0, 4, 301] {
            let disk = format!(r#"{{"container":"custom-ai","api_seconds":{invalid}}}"#);
            let loaded = Config::from_sources(Some(disk.as_bytes()), None, None);
            assert_eq!(loaded.api_seconds, 60, "api_seconds={invalid}");
            assert_eq!(loaded.container, "ai-memory", "api_seconds={invalid}");
        }
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
    #[test]
    #[cfg(target_os = "linux")]
    fn linux_config_path_prefers_absolute_xdg_and_falls_back_to_home() {
        use std::ffi::OsString;
        let home = || Some(OsString::from("/home/user"));
        assert_eq!(
            config_path(home(), Some(OsString::from("/xdg"))),
            PathBuf::from("/xdg/ai-monitor/config.json")
        );
        for xdg in [
            Some(OsString::from("relative/xdg")),
            Some(OsString::from("")),
            None,
        ] {
            assert_eq!(
                config_path(home(), xdg.clone()),
                PathBuf::from("/home/user/.config/ai-monitor/config.json"),
                "xdg={xdg:?} must fall back to HOME/.config"
            );
        }
        let path = config_path(None, None);
        assert!(
            !path.is_absolute(),
            "HOME and XDG unset must produce a relative path: {path:?}"
        );
        let Err(msg) = Config::default().save_to(&path) else {
            panic!("relative path must be refused");
        };
        assert!(msg.contains("HOME"), "got: {msg}");
        // An absolute XDG still wins even without HOME.
        assert_eq!(
            config_path(None, Some(OsString::from("/xdg"))),
            PathBuf::from("/xdg/ai-monitor/config.json")
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_config_path_ignores_xdg() {
        use std::ffi::OsString;
        assert_eq!(
            config_path(
                Some(OsString::from("/Users/u")),
                Some(OsString::from("/xdg"))
            ),
            PathBuf::from("/Users/u/Library/Application Support/AI Monitor/config.json")
        );
    }

    // Campos fora do padrão: se a config do disco cair inteira no default,
    // endpoint, contêiner e intervalos denunciam.
    const CUSTOM: &str = r#""endpoint":"http://192.168.0.10:49374","container":"custom-ai","resource_seconds":7,"api_seconds":42"#;
    fn assert_custom_fields_kept(loaded: &Config, case: &str) {
        assert_eq!(loaded.endpoint, "http://192.168.0.10:49374", "{case}");
        assert_eq!(loaded.container, "custom-ai", "{case}");
        assert_eq!(loaded.resource_seconds, 7, "{case}");
        assert_eq!(loaded.api_seconds, 42, "{case}");
    }

    #[test]
    fn theme_defaults_to_system_and_each_choice_roundtrips() {
        assert_eq!(Config::default().theme, Appearance::System);
        let old = format!("{{{CUSTOM}}}");
        let loaded = Config::from_sources(Some(old.as_bytes()), None, None);
        assert_custom_fields_kept(&loaded, "config without theme");
        assert_eq!(loaded.theme, Appearance::System, "missing field → System");

        for (theme, wire) in [
            (Appearance::System, "system"),
            (Appearance::Light, "light"),
            (Appearance::Dark, "dark"),
        ] {
            // Um membro novo do enum tem de aparecer nesta matriz.
            match theme {
                Appearance::System | Appearance::Light | Appearance::Dark => {}
            }
            let config = Config {
                theme,
                ..Config::from_sources(Some(old.as_bytes()), None, None)
            };
            let json = serde_json::to_value(&config).unwrap();
            assert_eq!(json["theme"], wire);
            let bytes = serde_json::to_vec(&config).unwrap();
            let loaded = Config::from_sources(Some(&bytes), None, None);
            assert_custom_fields_kept(&loaded, wire);
            assert_eq!(loaded.theme, theme, "{wire} must roundtrip");
        }
    }

    #[test]
    fn invalid_theme_becomes_system_without_discarding_the_rest_of_the_config() {
        for bad in [
            r#""purple""#,
            "42",
            "null",
            "true",
            "[]",
            r#"{"a":1}"#,
            r#""Dark""#,
        ] {
            let disk = format!(r#"{{{CUSTOM},"theme":{bad}}}"#);
            let loaded = Config::from_sources(Some(disk.as_bytes()), None, None);
            assert_custom_fields_kept(&loaded, bad);
            assert_eq!(loaded.theme, Appearance::System, "theme={bad}");
        }
        // Valor válido ao lado dos mesmos campos: prova que o teste acima não
        // passa só porque o tema nunca é lido.
        let disk = format!(r#"{{{CUSTOM},"theme":"light"}}"#);
        let loaded = Config::from_sources(Some(disk.as_bytes()), None, None);
        assert_eq!(loaded.theme, Appearance::Light);
    }

    #[test]
    fn decimal_uses_pt_br_separators_rounding_carry_and_sign() {
        for (x, places, want) in [
            (0., 1, "0,0"),
            (0., 0, "0"),
            (0.04, 1, "0,0"),
            (0.05, 1, "0,1"),
            (159.24, 1, "159,2"),
            (999.95, 1, "1.000,0"),
            (999.5, 0, "1.000"),
            (1234.5, 1, "1.234,5"),
            (-1234.5, 1, "-1.234,5"),
            (-0.04, 1, "0,0"),
            (-0.0, 1, "0,0"),
            (1e12, 1, "1.000.000.000.000,0"),
            (1234.567, 2, "1.234,57"),
            (f64::NAN, 1, "—"),
            (f64::INFINITY, 1, "—"),
            (f64::NEG_INFINITY, 0, "—"),
        ] {
            assert_eq!(decimal(x, places), want, "decimal({x}, {places})");
        }
    }

    #[test]
    fn bytes_and_number_group_thousands_in_pt_br() {
        for (n, want) in [
            (0, "0 B"),
            (1023, "1.023 B"),
            (1024, "1,0 KiB"),
            (3 << 29, "1,5 GiB"),
            (u64::MAX, "17.179.869.184,0 GiB"),
        ] {
            assert_eq!(bytes(n), want, "bytes({n})");
        }
        for (n, want) in [
            (0, "0"),
            (999, "999"),
            (1000, "1.000"),
            (1_000_000, "1.000.000"),
            (usize::MAX, "18.446.744.073.709.551.615"),
        ] {
            assert_eq!(number(n), want, "number({n})");
        }
    }

    #[test]
    fn decimal_up_to_trims_zeros_and_groups_integers() {
        for (x, want) in [
            (0., "0"),
            (1000., "1.000"),
            (1234.5, "1.234,5"),
            (0.1 + 0.2, "0,3"),
            (1e-7, "0"),
            (-1e-7, "0"),
            (-1234.5, "-1.234,5"),
            (0.125, "0,125"),
            (2.0005, "2,001"),
            (999.9996, "1.000"),
            (1e21, "1.000.000.000.000.000.000.000"),
            (f64::NAN, "—"),
            (f64::INFINITY, "—"),
            (f64::NEG_INFINITY, "—"),
        ] {
            assert_eq!(decimal_up_to(x, 3), want, "decimal_up_to({x:e}, 3)");
        }
    }

    #[test]
    fn metric_value_table_never_shows_a_nonzero_value_as_zero() {
        for (x, want) in [
            (0., "0"),
            (-0., "0"),
            (1., "1"),
            (1234.5, "1.234,5"),
            (-1234.5, "-1.234,5"),
            (1e21, "1.000.000.000.000.000.000.000"),
            (0.5, "0,5"),
            (0.1 + 0.2, "0,3"),
            (0.123456, "0,123"),
            (0.0012345, "0,00123"),
            (0.001, "0,001"),
            (0.0003, "0,0003"),
            (-0.00042, "-0,00042"),
            (1e-7, "0,0000001"),
            (1e-9, "0,000000001"),
            (0.99996, "1"),
            (0.0009996, "0,001"),
            (9.99e-10, "9,99e-10"),
            (1.5e-12, "1,5e-12"),
            (-1.5e-12, "-1,5e-12"),
            (f64::MIN_POSITIVE, "2,23e-308"),
            // Menor subnormal (4,9406…e-324): `{:.2e}` dá 4,94.
            (5e-324, "4,94e-324"),
            (f64::NAN, "—"),
            (f64::INFINITY, "—"),
            (f64::NEG_INFINITY, "—"),
        ] {
            assert_eq!(metric_value(x), want, "metric_value({x:e})");
        }
    }

    #[test]
    fn metric_value_invariants_hold_for_every_decade() {
        let parse = |s: String| s.parse::<f64>().unwrap();
        let mut samples = Vec::new();
        for k in -323..=20 {
            samples.push(parse(format!("1e{k}")));
        }
        for k in -300..=-1 {
            samples.push(parse(format!("3e{k}")));
            samples.push(parse(format!("9.99e{k}")));
        }
        // Entrada que arredonda para zero no parse não provaria nada.
        assert!(samples.iter().all(|x| *x > 0.));
        assert_eq!(samples.len(), 344 + 2 * 300);
        for x in samples.into_iter().flat_map(|x| [x, -x]) {
            let out = metric_value(x);
            // Algum dígito 1-9 na mantissa descarta "0", "-0" e "0,0…".
            let mantissa = out.split('e').next().unwrap();
            assert!(
                mantissa.bytes().any(|b| matches!(b, b'1'..=b'9')),
                "metric_value({x:e}) = {out:?} parece zero"
            );
            if x.abs() < 1. {
                assert!(
                    !out.contains('.'),
                    "metric_value({x:e}) = {out:?} agrupa milhar abaixo de 1"
                );
            }
            assert_eq!(
                out.contains('e'),
                x.abs() < 1e-9,
                "metric_value({x:e}) = {out:?}: notação científica só abaixo de 1e-9"
            );
        }
    }

    /// Diretório temporário só deste teste; apagado ao sair.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("ai-monitor-theme-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // Formatação fora do padrão do serde_json (espaços, 2.50, \u00e9, chave
    // desconhecida e ordem própria): qualquer regravação via Value a mudaria.
    const HAND_EDITED: &str = "{\n  \"endpoint\" : \"http://192.168.0.10:49374\",\n  \"future_key\": {\"nested\": [1, 2.50, \"\\u00e9\"]},\n  \"theme\": \"light\",\n  \"api_seconds\": 42\n}\n";

    #[test]
    fn save_theme_replaces_only_the_theme_bytes() {
        let dir = TempDir::new("replace");
        let path = dir.0.join("config.json");
        std::fs::write(&path, HAND_EDITED).unwrap();
        save_theme_to(&path, Appearance::Dark).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            HAND_EDITED.replace("\"theme\": \"light\"", "\"theme\": \"dark\"")
        );
    }

    #[test]
    fn save_theme_inserts_the_key_and_keeps_every_other_byte() {
        let dir = TempDir::new("insert");
        let path = dir.0.join("config.json");
        let without = HAND_EDITED.replace("  \"theme\": \"light\",\n", "");
        std::fs::write(&path, &without).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        save_theme_to(&path, Appearance::Light).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            without.replace(
                "\"api_seconds\": 42\n}",
                "\"api_seconds\": 42,\n  \"theme\": \"light\"\n}"
            )
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "same permissions as save_to");
        }
        let loaded = Config::from_sources(Some(&std::fs::read(&path).unwrap()), None, None);
        assert_eq!(loaded.endpoint, "http://192.168.0.10:49374");
        assert_eq!(loaded.api_seconds, 42);
        assert_eq!(loaded.theme, Appearance::Light);

        for (seed, want) in [
            ("{}", "{\"theme\": \"dark\"}"),
            (" { } ", " {\"theme\": \"dark\" } "),
            (
                r#"{"theme":"light","x":1,"theme":"system"}"#,
                r#"{"theme":"dark","x":1,"theme":"dark"}"#,
            ),
            (
                r#"{"a":"}\"{","theme":null}"#,
                r#"{"a":"}\"{","theme":"dark"}"#,
            ),
        ] {
            std::fs::write(&path, seed).unwrap();
            save_theme_to(&path, Appearance::Dark).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), want, "seed {seed}");
        }
    }

    #[test]
    fn save_theme_creates_a_missing_file_with_only_the_theme() {
        let dir = TempDir::new("missing");
        let path = dir.0.join("nested/config.json");
        save_theme_to(&path, Appearance::Dark).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\n  \"theme\": \"dark\"\n}"
        );
        assert!(
            save_theme_to(
                std::path::Path::new("relative/config.json"),
                Appearance::Dark
            )
            .is_err()
        );
        assert!(!std::path::Path::new("relative/config.json").exists());
    }

    #[test]
    fn save_theme_never_overwrites_an_unreadable_invalid_or_non_object_file() {
        let dir = TempDir::new("invalid");
        let path = dir.0.join("config.json");
        for seed in [
            &b"[]"[..],
            b"\"x\"",
            b"42",
            b"null",
            b"",
            b"{\"endpoint\": ",
            b"{\"a\":1} trailing",
            b"\xEF\xBB\xBF{\"a\":1}",
            b"{\"a\":\"\xff\"}",
        ] {
            std::fs::write(&path, seed).unwrap();
            let result = save_theme_to(&path, Appearance::Dark);
            assert_eq!(
                std::fs::read(&path).unwrap(),
                seed,
                "file must stay intact: {}",
                String::from_utf8_lossy(seed)
            );
            let err = result.unwrap_err();
            assert!(err.contains("só nesta sessão"), "{err}");
        }
        // Um diretório no lugar do arquivo: ilegível como texto.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let err = save_theme_to(&path, Appearance::Dark).unwrap_err();
        assert!(err.contains("não foi possível ler"), "{err}");
        assert!(path.is_dir());
    }
}
