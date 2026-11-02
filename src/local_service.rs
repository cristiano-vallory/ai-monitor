//! Host operations are explicit, loopback-only, bounded and never shell-expanded.
use crate::{client::Error, docker::local_command, model::Config};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};
pub const WARNING: &str = "Ações no serviço podem interromper agentes conectados. Confira o destino e salve backup antes de parar/atualizar. Nenhuma ação local é executada automaticamente. Logs podem conter dados privados. Recuperação offline fornece um roteiro; não restaura nem exclui bancos automaticamente.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Docker,
    LaunchAgent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Status,
    Logs,
    Start,
    Stop,
    Restart,
    Update,
    Recovery,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub backend: Backend,
    pub kind: Kind,
    pub values: BTreeMap<String, String>,
    binding: Option<Value>,
}
impl Operation {
    pub fn new(backend: Backend, kind: Kind) -> Self {
        Self {
            backend,
            kind,
            values: BTreeMap::new(),
            binding: None,
        }
    }
    pub fn read(&self) -> bool {
        matches!(self.kind, Kind::Status | Kind::Logs | Kind::Recovery)
    }
    pub fn label(&self) -> &'static str {
        match self.kind {
            Kind::Status => "Consultar serviço local",
            Kind::Logs => "Ler logs privados",
            Kind::Start => "Iniciar serviço local",
            Kind::Stop => "Parar serviço local",
            Kind::Restart => "Reiniciar serviço local",
            Kind::Update => "Atualizar serviço local",
            Kind::Recovery => "Roteiro de recuperação offline",
        }
    }
    pub fn fields(&self) -> Vec<(&'static str, &'static str)> {
        let mut v = match self.backend {
            Backend::Docker => vec![
                ("context", "Contexto Docker local (ex.: desktop-linux)"),
                ("container", "Nome ou ID do contêiner ai-memory"),
            ],
            Backend::LaunchAgent => vec![(
                "plist",
                "Caminho absoluto do LaunchAgent ai-memory (.plist)",
            )],
        };
        if self.kind == Kind::Update {
            v.push(("backup", "Backup privado já salvo (.tar.gz)"));
            match self.backend {
                Backend::Docker => v.extend([
                    ("compose", "Arquivo Compose absoluto já configurado"),
                    ("service", "Nome exato do serviço no Compose"),
                ]),
                Backend::LaunchAgent => v.extend([
                    (
                        "replacement",
                        "Novo binário ai-memory, obtido de fonte confiável",
                    ),
                    ("sha256", "SHA-256 esperado do novo binário"),
                ]),
            }
        }
        if self.kind == Kind::Recovery {
            v.extend([
                ("data", "Diretório absoluto dos dados"),
                ("backup", "Backup privado para recuperação"),
            ]);
        }
        v
    }
    fn s(&self, k: &str) -> &str {
        self.values.get(k).map(|v| v.trim()).unwrap_or("")
    }
    pub fn validate(&self) -> Result<(), Error> {
        for (key, _) in self.fields() {
            let v = self.s(key);
            if v.is_empty()
                || v.starts_with('-')
                || v.len() > 4096
                || v.chars().any(char::is_control)
            {
                return Err(Error::Invalid(
                    "Preencha todos os campos locais com valores explícitos válidos.".into(),
                ));
            }
            if ["plist", "backup", "compose", "replacement", "data"].contains(&key)
                && !Path::new(v).is_absolute()
            {
                return Err(Error::Invalid(
                    "Use caminhos absolutos para arquivos locais.".into(),
                ));
            }
        }
        if self.backend == Backend::Docker
            && ["context", "container", "service"].iter().any(|k| {
                self.values.contains_key(*k)
                    && !self
                        .s(k)
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            })
        {
            return Err(Error::Invalid("Identificador Docker inválido.".into()));
        }
        if self.kind == Kind::Update
            && self.backend == Backend::LaunchAgent
            && (self.s("sha256").len() != 64
                || !self.s("sha256").bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(Error::Invalid(
                "Informe o SHA-256 completo do novo binário.".into(),
            ));
        }
        Ok(())
    }
    pub fn confirmation(&self) -> String {
        format!(
            "{} {}",
            match self.kind {
                Kind::Start => "INICIAR",
                Kind::Stop => "PARAR",
                Kind::Restart => "REINICIAR",
                Kind::Update => "ATUALIZAR",
                _ => "CONSULTAR",
            },
            if self.backend == Backend::Docker {
                self.s("container")
            } else {
                "ai-memory"
            }
        )
    }
    pub fn bind(&mut self, result: &Value) -> Result<(), Error> {
        self.binding = Some(result.get("binding").cloned().ok_or(Error::Format)?);
        Ok(())
    }
    fn docker(&self, c: &Config, args: &[&str], logs: bool, seconds: u64) -> Result<String, Error> {
        let mut a = vec!["--context", self.s("context")];
        a.extend(args);
        run(&c.docker_binary, &a, logs, seconds)
    }
    fn inspect(&self, c: &Config) -> Result<Value, Error> {
        match self.backend {
            Backend::Docker => {
                if !Path::new(&c.docker_binary).is_absolute() {
                    return Err(Error::Invalid(
                        "Configure o caminho absoluto do Docker em Conexão.".into(),
                    ));
                }
                let host = self.docker(
                    c,
                    &[
                        "context",
                        "inspect",
                        self.s("context"),
                        "--format",
                        "{{.Endpoints.docker.Host}}",
                    ],
                    false,
                    6,
                )?;
                if !host.trim().starts_with("unix:///") {
                    return Err(Error::Invalid("Controle local exige contexto Docker com socket Unix; contexto remoto recusado.".into()));
                }
                let raw=self.docker(c,&["container","inspect","--format",r#"{"id":{{json .Id}},"image":{{json .Image}},"state":{{json .State.Status}},"service":{{json (index .Config.Labels "com.docker.compose.service")}},"compose":{{json (index .Config.Labels "com.docker.compose.project.config_files")}},"project":{{json (index .Config.Labels "com.docker.compose.project")}}}"#,"--",self.s("container")],false,6)?;
                let v: Value = serde_json::from_str(raw.trim()).map_err(|_| Error::Format)?;
                if !v["id"]
                    .as_str()
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    return Err(Error::Format);
                }
                Ok(json!({"host":host.trim(),"container":v}))
            }
            Backend::LaunchAgent => {
                regular(self.s("plist"))?;
                let label = extract(self.s("plist"), "Label")?;
                if label != "com.github.akitaonrails.ai-memory" {
                    return Err(Error::Invalid(
                        "Use o LaunchAgent oficial com Label com.github.akitaonrails.ai-memory."
                            .into(),
                    ));
                }
                let binary = extract(self.s("plist"), "ProgramArguments.0")?;
                if Path::new(&binary).file_name().and_then(|x| x.to_str()) != Some("ai-memory")
                    || extract(self.s("plist"), "ProgramArguments.1")? != "serve"
                {
                    return Err(Error::Invalid(
                        "LaunchAgent deve executar o binário ai-memory serve diretamente.".into(),
                    ));
                }
                regular(&binary)?;
                let uid = run("/usr/bin/id", &["-u"], false, 6)?.trim().to_owned();
                if !uid.bytes().all(|b| b.is_ascii_digit()) || uid.is_empty() {
                    return Err(Error::Format);
                }
                let domain = format!("gui/{uid}");
                run("/bin/launchctl", &["print", &domain], false, 6)?;
                let target = format!("{domain}/{label}");
                let loaded = run("/bin/launchctl", &["print", &target], false, 6).is_ok();
                Ok(
                    json!({"domain":domain,"target":target,"loaded":loaded,"binary":binary,"plist_sha256":hash(self.s("plist"))?,"binary_sha256":hash(&binary)?}),
                )
            }
        }
    }
    fn compose_digest(&self, c: &Config, project: &str) -> Result<String, Error> {
        use sha2::{Digest, Sha256};
        // Resolved config can include secrets. Only its digest leaves this function.
        let config = self.docker(
            c,
            &[
                "compose",
                "--project-name",
                project,
                "--file",
                self.s("compose"),
                "config",
                "--format",
                "json",
            ],
            false,
            30,
        )?;
        let parsed: Value = serde_json::from_str(&config).map_err(|_| Error::Format)?;
        if !parsed["services"][self.s("service")]["image"].is_string() {
            return Err(Error::Invalid("Atualização exige serviço Compose com image explícita; build local não é executado pelo monitor.".into()));
        }
        Ok(format!("{:x}", Sha256::digest(config.as_bytes())))
    }
    pub fn execute(
        &self,
        c: &Config,
        preview: bool,
        cancelled: impl Fn() -> bool,
    ) -> Result<Value, Error> {
        self.validate()?;
        let url = reqwest::Url::parse(&c.endpoint).map_err(|_| Error::Format)?;
        if !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) {
            return Err(Error::Invalid("Controle local desabilitado para conexão remota. Selecione explicitamente o servidor deste Mac.".into()));
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let state = self.inspect(c)?;
        let mut binding = state.clone();
        if let Some(m) = binding["container"].as_object_mut() {
            m.remove("state");
        }
        if let Some(m) = binding.as_object_mut() {
            m.remove("loaded");
        }
        if self.kind == Kind::Update {
            backup(self.s("backup"))?;
            binding["backup_sha256"] = json!(hash(self.s("backup"))?);
            match self.backend {
                Backend::Docker => {
                    regular(self.s("compose"))?;
                    if state["container"]["service"] != self.s("service")
                        || state["container"]["compose"] != self.s("compose")
                    {
                        return Err(Error::Invalid("Compose/serviço não corresponde ao contêiner. Atualização automática exige um único arquivo Compose registrado no contêiner; stacks com overrides precisam de atualização pelo operador.".into()));
                    }
                    binding["compose_sha256"] = json!(hash(self.s("compose"))?);
                    let project = state["container"]["project"]
                        .as_str()
                        .filter(|s| !s.is_empty() && !s.starts_with('-'))
                        .ok_or(Error::Format)?;
                    binding["resolved_compose_sha256"] = json!(self.compose_digest(c, project)?);
                }
                Backend::LaunchAgent => {
                    regular(self.s("replacement"))?;
                    let digest = hash(self.s("replacement"))?;
                    if !digest.eq_ignore_ascii_case(self.s("sha256")) {
                        return Err(Error::Invalid(
                            "SHA-256 do novo binário não confere.".into(),
                        ));
                    }
                    if state["loaded"] != false {
                        return Err(Error::Invalid("Pare o LaunchAgent pelo app antes de substituir o binário. Ele ficará parado até você iniciar novamente.".into()));
                    }
                    binding["replacement_sha256"] = json!(digest);
                }
            }
        }
        if preview {
            return Ok(
                json!({"binding":binding,"state":state,"operation":self.label(),"parameters":self.values,"warning":WARNING}),
            );
        }
        if !self.read() && self.binding.as_ref() != Some(&binding) {
            return Err(Error::Conflict);
        }
        if cancelled() {
            return Err(Error::Cancelled);
        }
        if self.kind == Kind::Recovery {
            return Ok(
                json!({"state":state,"data":self.s("data"),"backup":self.s("backup"),"steps":["1. Salve backup privado completo; confira arquivo, origem, versão e pepper das chaves.","2. Pare o serviço pelo app e confirme que nenhum outro ai-memory usa o mesmo diretório/volume.","3. Em terminal de manutenção com a versão compatível: ai-memory --data-dir <diretório> restore --from <backup> --force. Preserve config.toml; revise a cópia restaurada antes de adotá-la. Não use --with-config sem revisão.","4. Para reindex: somente banco vazio e servidor parado. Preserve uma cópia do banco original; reindex não recupera sessões, observações, handoffs, usuários ou auditoria. Execute ai-memory --data-dir <diretório> reindex.","5. Docker: use um contêiner de manutenção isolado com a mesma versão, configuração e volume, sem servidor concorrente. Não remova volumes.","6. Inicie o serviço e verifique saúde, contagens, autenticação e sessões. Se falhar, mantenha o backup e investigue antes de repetir."],"execution":"Roteiro apenas; nenhum restore/reindex foi executado."}),
            );
        }
        if self.kind == Kind::Status {
            return Ok(state);
        }
        match self.backend {
            Backend::Docker => {
                let id = state["container"]["id"].as_str().ok_or(Error::Format)?;
                match self.kind {
                    Kind::Logs => {
                        let raw = self.docker(
                            c,
                            &["logs", "--tail", "300", "--timestamps", id],
                            true,
                            15,
                        )?;
                        return Ok(
                            json!({"logs":tail(&raw),"notice":"Últimas 300 linhas de stdout/stderr; streams concatenados, sem garantia de ordem temporal; até 100 KiB."}),
                        );
                    }
                    Kind::Start => {
                        self.docker(c, &["start", id], false, 60)?;
                    }
                    Kind::Stop => {
                        self.docker(c, &["stop", "--time", "30", id], false, 60)?;
                    }
                    Kind::Restart => {
                        self.docker(c, &["restart", "--time", "30", id], false, 90)?;
                    }
                    Kind::Update => {
                        let project = state["container"]["project"]
                            .as_str()
                            .filter(|s| !s.is_empty() && !s.starts_with('-'))
                            .ok_or(Error::Format)?;
                        self.docker(
                            c,
                            &[
                                "compose",
                                "--project-name",
                                project,
                                "--file",
                                self.s("compose"),
                                "pull",
                                self.s("service"),
                            ],
                            false,
                            300,
                        )?;
                        if cancelled() {
                            return Err(Error::Cancelled);
                        }
                        if hash(self.s("compose"))? != binding["compose_sha256"]
                            || self.compose_digest(c, project)?
                                != binding["resolved_compose_sha256"]
                        {
                            return Err(Error::Conflict);
                        }
                        self.docker(
                            c,
                            &[
                                "compose",
                                "--project-name",
                                project,
                                "--file",
                                self.s("compose"),
                                "up",
                                "-d",
                                "--no-deps",
                                "--no-build",
                                "--pull",
                                "never",
                                self.s("service"),
                            ],
                            false,
                            180,
                        )?;
                    }
                    _ => {}
                }
            }
            Backend::LaunchAgent => {
                let target = state["target"].as_str().ok_or(Error::Format)?;
                let domain = state["domain"].as_str().ok_or(Error::Format)?;
                match self.kind {
                    Kind::Logs => {
                        let mut logs = Vec::new();
                        for key in ["StandardOutPath", "StandardErrorPath"] {
                            let path = extract(self.s("plist"), key)?;
                            regular(&path)?;
                            let raw = run("/usr/bin/tail", &["-n", "150", &path], false, 6)?;
                            logs.push(json!({"stream":key,"text":tail(&raw)}));
                        }
                        return Ok(json!({"logs":logs}));
                    }
                    Kind::Start => {
                        run(
                            "/bin/launchctl",
                            &["bootstrap", domain, self.s("plist")],
                            false,
                            30,
                        )?;
                    }
                    Kind::Stop => {
                        run("/bin/launchctl", &["bootout", target], false, 30)?;
                    }
                    Kind::Restart => {
                        run("/bin/launchctl", &["kickstart", "-k", target], false, 30)?;
                    }
                    Kind::Update => {
                        let ps = run("/bin/ps", &["-axo", "comm="], false, 6)?;
                        if ps.lines().any(|s| {
                            Path::new(s.trim()).file_name().and_then(|x| x.to_str())
                                == Some("ai-memory")
                        }) {
                            return Err(Error::Invalid("Ainda existe processo ai-memory ativo. Pare todas as instâncias antes da troca.".into()));
                        }
                        return replace(
                            state["binary"].as_str().ok_or(Error::Format)?,
                            self.s("replacement"),
                            self.s("sha256"),
                        );
                    }
                    _ => {}
                }
            }
        }
        Ok(
            json!({"command_completed":true,"next":"Consulte o estado e a saúde do servidor para verificar o resultado. A resposta do comando não prova que o serviço está saudável."}),
        )
    }
}
fn run(binary: &str, args: &[&str], logs: bool, secs: u64) -> Result<String, Error> {
    local_command(binary, args, Duration::from_secs(secs), logs).map_err(Error::Invalid)
}
fn regular(path: &str) -> Result<(), Error> {
    let m = std::fs::symlink_metadata(path)
        .map_err(|_| Error::Invalid("Arquivo local ausente ou inacessível.".into()))?;
    if !Path::new(path).is_absolute() || !m.is_file() || m.file_type().is_symlink() {
        return Err(Error::Invalid(
            "Use arquivo regular absoluto; links simbólicos não são aceitos para manutenção."
                .into(),
        ));
    }
    Ok(())
}
fn extract(path: &str, key: &str) -> Result<String, Error> {
    run(
        "/usr/bin/plutil",
        &["-extract", key, "raw", "-o", "-", path],
        false,
        6,
    )
    .map(|s| s.trim().to_owned())
}
fn hash(path: &str) -> Result<String, Error> {
    regular(path)?;
    let s = run("/usr/bin/shasum", &["-a", "256", path], false, 300)?;
    let s = s.split_whitespace().next().unwrap_or("");
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Format);
    }
    Ok(s.into())
}
fn backup(path: &str) -> Result<(), Error> {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    regular(path)?;
    let mut f = std::fs::File::open(path).map_err(|_| Error::Unavailable)?;
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic).map_err(|_| Error::Format)?;
    let m = f.metadata().map_err(|_| Error::Unavailable)?;
    if magic != [0x1f, 0x8b] || m.len() < 3 || m.permissions().mode() & 0o077 != 0 {
        return Err(Error::Invalid("Selecione um backup gzip privado (0600). A presença do arquivo não comprova sua restauração ou origem; confirme-as antes de prosseguir.".into()));
    }
    Ok(())
}
fn tail(s: &str) -> &str {
    let mut start = s.len().saturating_sub(100 * 1024);
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}
fn replace(binary: &str, replacement: &str, expected: &str) -> Result<Value, Error> {
    use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};
    // Same-directory staging keeps the final replacement atomic; backup is never overwritten.
    regular(binary)?;
    regular(replacement)?;
    let suffix = chrono::Utc::now()
        .timestamp_nanos_opt()
        .ok_or(Error::Unavailable)?;
    let old = format!("{binary}.backup-{suffix}");
    let stage = format!("{binary}.new-{suffix}");
    let copy_private = |from: &str, to: &str| -> Result<(), Error> {
        let mut src = std::fs::File::open(from).map_err(|_| Error::Unavailable)?;
        let mut dest = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o700)
            .open(to)
            .map_err(|_| {
                Error::Invalid(
                    "Não foi possível criar backup/staging ao lado do binário. Confira permissões."
                        .into(),
                )
            })?;
        std::io::copy(&mut src, &mut dest).map_err(|_| Error::Unavailable)?;
        dest.flush().map_err(|_| Error::Unavailable)?;
        dest.sync_all().map_err(|_| Error::Unavailable)
    };
    copy_private(binary, &old)?;
    let result = (|| {
        copy_private(replacement, &stage)?;
        if !hash(&stage)?.eq_ignore_ascii_case(expected) {
            return Err(Error::Conflict);
        }
        std::fs::rename(&stage, binary).map_err(|_| Error::Unavailable)?;
        Ok(
            json!({"binary_replaced":true,"previous_binary":old,"next":"Serviço permanece parado. Inicie explicitamente e confira a saúde; o binário anterior foi preservado para rollback."}),
        )
    })();
    let _ = std::fs::remove_file(&stage);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_host_is_refused_before_subprocess() {
        let mut op = Operation::new(Backend::Docker, Kind::Stop);
        op.values.insert("context".into(), "local".into());
        op.values.insert("container".into(), "ai-memory".into());
        let cfg = Config {
            endpoint: "https://example.test".into(),
            docker_binary: "/missing".into(),
            ..Default::default()
        };
        assert!(
            matches!(op.execute(&cfg,true,||false),Err(Error::Invalid(s))if s.contains("remota"))
        );
    }
    #[test]
    fn local_identifiers_cannot_inject_options_or_shell() {
        let mut op = Operation::new(Backend::Docker, Kind::Start);
        op.values.insert("context".into(), "local".into());
        for bad in ["--help", "x; rm", "$(touch x)", "x\ny"] {
            op.values.insert("container".into(), bad.into());
            assert!(op.validate().is_err());
        }
    }
    #[test]
    fn utf8_log_tail_is_bounded() {
        let s = "á".repeat(100_000);
        let t = tail(&s);
        assert!(t.len() <= 100 * 1024);
        assert!(t.is_char_boundary(0));
    }
    #[test]
    fn docker_preview_binds_container_and_refuses_remote_or_changed_target() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "monitor-docker-control-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir(&dir).unwrap();
        let script = dir.join("docker");
        std::fs::write(&script,r#"#!/bin/sh
root=$(/usr/bin/dirname "$0")
shift 2
case "$1" in
 context) if [ -e "$root/remote" ]; then echo tcp://remote:2375; else echo unix:///synthetic.sock; fi ;;
 container) /bin/cat "$root/state" ;;
 stop) printf '%s\n' "$@" >> "$root/actions" ;;
 *) exit 1 ;;
esac
"#).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let state = |id: &str| json!({"id":id.repeat(64),"image":"sha256:synthetic","state":"running","service":"ai-memory","compose":"/synthetic.yaml","project":"synthetic"});
        std::fs::write(dir.join("state"), state("a").to_string()).unwrap();
        let config = Config {
            docker_binary: script.to_str().unwrap().into(),
            ..Config::default()
        };
        let mut op = Operation::new(Backend::Docker, Kind::Stop);
        op.values.insert("context".into(), "synthetic".into());
        op.values.insert("container".into(), "ai-memory".into());
        assert_eq!(op.execute(&config, false, || false), Err(Error::Conflict));
        let review = op.execute(&config, true, || false).unwrap();
        op.bind(&review).unwrap();
        assert!(!dir.join("actions").exists());
        std::fs::write(dir.join("state"), state("b").to_string()).unwrap();
        assert_eq!(op.execute(&config, false, || false), Err(Error::Conflict));
        std::fs::write(dir.join("state"), state("a").to_string()).unwrap();
        std::fs::write(dir.join("remote"), "").unwrap();
        assert!(op.execute(&config, false, || false).is_err());
        assert!(!dir.join("actions").exists());
        std::fs::remove_file(dir.join("remote")).unwrap();
        op.execute(&config, false, || false).unwrap();
        let actions = std::fs::read_to_string(dir.join("actions")).unwrap();
        assert_eq!(actions, format!("stop\n--time\n30\n{}\n", "a".repeat(64)));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn replacement_keeps_private_backup_and_checks_staged_digest() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "monitor-local-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir(&dir).unwrap();
        let old = dir.join("ai-memory");
        let new = dir.join("candidate");
        std::fs::write(&old, "old binary").unwrap();
        std::fs::write(&new, "new binary").unwrap();
        let result = replace(
            old.to_str().unwrap(),
            new.to_str().unwrap(),
            &"0".repeat(64),
        );
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "old binary");
        let result = replace(
            old.to_str().unwrap(),
            new.to_str().unwrap(),
            &hash(new.to_str().unwrap()).unwrap(),
        )
        .unwrap();
        let backup = result["previous_binary"].as_str().unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "old binary");
        assert_eq!(
            std::fs::metadata(backup).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(std::fs::read_to_string(old).unwrap(), "new binary");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
