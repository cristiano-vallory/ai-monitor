//! Host operations are explicit, loopback-only, bounded and never shell-expanded.
use crate::{client::Error, confirmation::Confirmation, docker::local_command, model::Config};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};
pub const WARNING: &str = "Ações no serviço podem interromper agentes conectados. Confira o destino e salve backup antes de parar/atualizar. Nenhuma ação local é executada automaticamente. Logs podem conter dados privados. Recuperação offline fornece um roteiro; não restaura nem exclui bancos automaticamente.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Docker,
    LaunchAgent,
    Systemd,
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
                ("context", crate::platform::DOCKER_CONTEXT_LABEL),
                ("container", "Nome ou ID do contêiner ai-memory"),
            ],
            Backend::LaunchAgent => vec![(
                "plist",
                "Caminho absoluto do LaunchAgent ai-memory (.plist)",
            )],
            Backend::Systemd => vec![(
                "unit",
                "Caminho absoluto da unit ai-memory.service (systemd --user)",
            )],
        };
        if self.kind == Kind::Update {
            v.push(("backup", "Backup privado já salvo (.tar.gz)"));
            match self.backend {
                Backend::Docker => v.extend([
                    ("compose", "Arquivo Compose absoluto já configurado"),
                    ("service", "Nome exato do serviço no Compose"),
                ]),
                Backend::LaunchAgent | Backend::Systemd => v.extend([
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
            if ["plist", "unit", "backup", "compose", "replacement", "data"].contains(&key)
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
        if self.backend == Backend::Systemd
            && Path::new(self.s("unit"))
                .file_name()
                .and_then(|n| n.to_str())
                != Some("ai-memory.service")
        {
            return Err(Error::Invalid(
                "A unit precisa se chamar exatamente ai-memory.service.".into(),
            ));
        }
        if self.kind == Kind::Update
            && matches!(self.backend, Backend::LaunchAgent | Backend::Systemd)
            && (self.s("sha256").len() != 64
                || !self.s("sha256").bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(Error::Invalid(
                "Informe o SHA-256 completo do novo binário.".into(),
            ));
        }
        Ok(())
    }
    /// O alvo: o contêiner (Docker) ou "ai-memory" (LaunchAgent, systemd).
    pub fn target(&self) -> &str {
        if self.backend == Backend::Docker {
            self.s("container")
        } else {
            "ai-memory"
        }
    }
    /// Nível de confirmação (`None` = leitura). Sem curinga: uma operação
    /// nova não compila sem ser classificada.
    pub fn confirmation(&self) -> Option<Confirmation> {
        match self.kind {
            Kind::Status | Kind::Logs | Kind::Recovery => None,
            Kind::Start | Kind::Restart => Some(Confirmation::Click),
            Kind::Stop | Kind::Update => Some(Confirmation::Type(self.target().into())),
        }
    }
    /// O título da folha e o verbo do botão final, escritos à mão; `None` =
    /// leitura. Sem curinga: uma operação nova não compila sem eles.
    pub fn question(&self) -> Option<(String, &'static str)> {
        let target = self.target();
        Some(match self.kind {
            Kind::Status | Kind::Logs | Kind::Recovery => return None,
            Kind::Start => (format!("Iniciar o serviço “{target}”?"), "Iniciar"),
            Kind::Stop => (format!("Parar o serviço “{target}”?"), "Parar"),
            Kind::Restart => (format!("Reiniciar o serviço “{target}”?"), "Reiniciar"),
            Kind::Update => (format!("Atualizar o serviço “{target}”?"), "Atualizar"),
        })
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
                        "Configure o caminho absoluto do Docker em Ajustes.".into(),
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
            Backend::Systemd => self.inspect_systemd(),
        }
    }
    #[cfg(target_os = "linux")]
    fn inspect_systemd(&self) -> Result<Value, Error> {
        regular(self.s("unit"))?;
        let out = run(
            "/usr/bin/systemctl",
            &[
                "--user",
                "show",
                "ai-memory.service",
                "--no-pager",
                "-p",
                "LoadState",
                "-p",
                "FragmentPath",
                "-p",
                "DropInPaths",
                "-p",
                "NeedDaemonReload",
                "-p",
                "ExecStart",
                "-p",
                "ActiveState",
                "-p",
                "SubState",
                "-p",
                "MainPID",
                "-p",
                "ExecStartPre",
                "-p",
                "ExecStartPost",
                "-p",
                "ExecReload",
                "-p",
                "ExecStop",
                "-p",
                "ExecStopPost",
            ],
            false,
            6,
        )?;
        let show = parse_systemd_show(&out, self.s("unit"))?;
        regular(&show.binary)?;
        let dropin_sha256: Vec<Value> = show
            .dropins
            .iter()
            .map(|d| {
                json!({
                    "path": d,
                    "sha256": hash(d).map(Value::String).unwrap_or(Value::Null),
                })
            })
            .collect();
        Ok(json!({
            "unit": self.s("unit"),
            "fragment": show.fragment,
            "dropins": show.dropins,
            "dropin_sha256": dropin_sha256,
            "exec_start": show.exec_start,
            "active_state": show.active_state,
            "sub_state": show.sub_state,
            "main_pid": show.main_pid,
            "extra_commands": show.extra_commands,
            "binary": show.binary,
            "unit_sha256": hash(self.s("unit"))?,
            "binary_sha256": hash(&show.binary)?,
        }))
    }
    #[cfg(not(target_os = "linux"))]
    fn inspect_systemd(&self) -> Result<Value, Error> {
        Err(Error::Invalid("systemd existe somente no Linux.".into()))
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
        #[cfg(not(target_os = "macos"))]
        if self.backend == Backend::LaunchAgent {
            return Err(Error::Invalid(
                "LaunchAgent existe somente no macOS. No Linux, use o backend systemd (usuário)."
                    .into(),
            ));
        }
        #[cfg(not(target_os = "linux"))]
        if self.backend == Backend::Systemd {
            return Err(Error::Invalid("systemd existe somente no Linux.".into()));
        }
        self.validate()?;
        let url = reqwest::Url::parse(&c.endpoint).map_err(|_| Error::Format)?;
        if !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) {
            return Err(Error::Invalid(format!(
                "Controle local desabilitado para conexão remota. Selecione explicitamente o servidor deste {}.",
                crate::platform::HOST
            )));
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
            m.remove("active_state");
            m.remove("sub_state");
            m.remove("main_pid");
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
                Backend::Systemd => {
                    regular(self.s("replacement"))?;
                    let digest = hash(self.s("replacement"))?;
                    if !digest.eq_ignore_ascii_case(self.s("sha256")) {
                        return Err(Error::Invalid(
                            "SHA-256 do novo binário não confere.".into(),
                        ));
                    }
                    if !systemd_update_allowed(state["active_state"].as_str().unwrap_or("")) {
                        return Err(Error::Invalid("Pare o serviço pelo app antes de substituir o binário. Ele ficará parado até você iniciar novamente.".into()));
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
                        return launch_agent_logs(self.s("plist"));
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
            Backend::Systemd => match self.kind {
                Kind::Logs => {
                    let raw = run(
                        "/usr/bin/journalctl",
                        &[
                            "--user",
                            "-u",
                            "ai-memory.service",
                            "-n",
                            "300",
                            "--no-pager",
                            "-o",
                            "short-iso",
                        ],
                        true,
                        15,
                    )?;
                    return Ok(
                        json!({"logs":tail(&raw),"notice":"Últimas 300 linhas do journal do usuário; até 100 KiB. Logs podem conter dados privados."}),
                    );
                }
                Kind::Start => {
                    run(
                        "/usr/bin/systemctl",
                        &["--user", "start", "ai-memory.service"],
                        false,
                        60,
                    )?;
                }
                Kind::Stop => {
                    run(
                        "/usr/bin/systemctl",
                        &["--user", "stop", "ai-memory.service"],
                        false,
                        60,
                    )?;
                }
                Kind::Restart => {
                    run(
                        "/usr/bin/systemctl",
                        &["--user", "restart", "ai-memory.service"],
                        false,
                        90,
                    )?;
                }
                Kind::Update => {
                    let ps = run("/usr/bin/ps", &["-e", "-o", "comm="], false, 6)?;
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
            },
        }
        Ok(
            json!({"command_completed":true,"next":"Consulte o estado e a saúde do servidor para verificar o resultado. A resposta do comando não prova que o serviço está saudável."}),
        )
    }
}
fn run(binary: &str, args: &[&str], logs: bool, secs: u64) -> Result<String, Error> {
    local_command(binary, args, Duration::from_secs(secs), logs).map_err(Error::Invalid)
}
#[cfg(target_os = "linux")]
pub struct SystemdShow {
    pub fragment: String,
    pub dropins: Vec<String>,
    pub exec_start: String,
    pub binary: String,
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: String,
    pub extra_commands: Vec<String>,
}
#[cfg(target_os = "linux")]
fn split_words(s: &str) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut has_word = false;
    for ch in s.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !in_quotes => escaped = true,
            '"' => {
                in_quotes = !in_quotes;
                has_word = true;
            }
            ' ' | '\t' if !in_quotes => {
                if has_word || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has_word = false;
                }
            }
            _ => {
                cur.push(ch);
                has_word = true;
            }
        }
    }
    if escaped || in_quotes {
        return Err(Error::Format);
    }
    if has_word || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}
pub fn systemd_update_allowed(active_state: &str) -> bool {
    matches!(active_state, "inactive" | "failed")
}
#[cfg(target_os = "linux")]
pub fn parse_systemd_show(text: &str, unit: &str) -> Result<SystemdShow, Error> {
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            fields.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let get = |key: &str| fields.get(key).cloned().unwrap_or_default();
    match get("LoadState").as_str() {
        "loaded" => {}
        other => {
            return Err(Error::Invalid(format!(
                "A unit ai-memory.service não está carregada no gerenciador do usuário (LoadState={other}). Recarregue com: systemctl --user daemon-reload"
            )));
        }
    }
    if get("FragmentPath") != unit {
        return Err(Error::Invalid(
            "A unit carregada não corresponde ao arquivo informado (FragmentPath difere).".into(),
        ));
    }
    if get("NeedDaemonReload") == "yes" {
        return Err(Error::Invalid(
            "A unit mudou desde o último carregamento. Execute: systemctl --user daemon-reload"
                .into(),
        ));
    }
    let exec = get("ExecStart");
    if exec.is_empty() {
        return Err(Error::Invalid(
            "A unit ai-memory.service não define ExecStart.".into(),
        ));
    }
    // Cada comando aparece como um grupo { ... } na saída de systemctl show.
    let groups = exec.matches("{ path=").count();
    if groups != 1 {
        return Err(Error::Invalid(
            "A unit ai-memory.service precisa definir exatamente um comando ExecStart.".into(),
        ));
    }
    // systemd executa `path=`, não argv[0]; os dois precisam coincidir para
    // que o binário validado seja o binário executado (sem prefixo @).
    let path_field = exec
        .split("path=")
        .nth(1)
        .and_then(|rest| rest.split(" ; ").next())
        .and_then(|rest| rest.split(' ').next())
        .ok_or(Error::Format)?;
    let argv_field = exec
        .split("argv[]=")
        .nth(1)
        .and_then(|rest| rest.split(" ; ").next())
        .ok_or(Error::Format)?;
    let argv = split_words(argv_field)?;
    let binary = path_field.to_owned();
    let argv0 = argv.first().cloned().ok_or(Error::Format)?;
    if !Path::new(&binary).is_absolute()
        || Path::new(&binary).file_name().and_then(|n| n.to_str()) != Some("ai-memory")
    {
        return Err(Error::Invalid(
            "A unit deve executar um binário ai-memory em caminho absoluto.".into(),
        ));
    }
    if binary != argv0 {
        return Err(Error::Invalid(
            "A unit deve executar diretamente o binário ai-memory (path e argv[0] diferem).".into(),
        ));
    }
    // Subcomando = primeira palavra que não é opção nem valor de --data-dir/--config.
    let mut i = 1;
    let mut subcommand = None;
    while i < argv.len() {
        let w = &argv[i];
        if w == "--data-dir" || w == "--config" {
            i += 2;
            continue;
        }
        if w.starts_with("--data-dir=") || w.starts_with("--config=") {
            i += 1;
            continue;
        }
        if w.starts_with('-') {
            i += 1;
            continue;
        }
        subcommand = Some(w.clone());
        break;
    }
    if subcommand.as_deref() != Some("serve") {
        return Err(Error::Invalid(
            "A unit ai-memory.service deve executar o subcomando serve.".into(),
        ));
    }
    Ok(SystemdShow {
        fragment: get("FragmentPath"),
        dropins: get("DropInPaths")
            .split_whitespace()
            .map(|d| d.to_owned())
            .collect(),
        exec_start: exec,
        extra_commands: [
            get("ExecStartPre"),
            get("ExecStartPost"),
            get("ExecReload"),
            get("ExecStop"),
            get("ExecStopPost"),
        ]
        .into_iter()
        .filter(|c| !c.is_empty())
        .collect(),
        binary,
        active_state: get("ActiveState"),
        sub_state: get("SubState"),
        main_pid: get("MainPID"),
    })
}
#[cfg(unix)]
fn open_nofollow(path: &str) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}
fn open_regular(path: &str) -> Result<std::fs::File, Error> {
    open_regular_with(path, open_nofollow)
}
fn open_regular_with(
    path: &str,
    open: impl FnOnce(&str) -> std::io::Result<std::fs::File>,
) -> Result<std::fs::File, Error> {
    let m = std::fs::symlink_metadata(path)
        .map_err(|_| Error::Invalid("Arquivo local ausente ou inacessível.".into()))?;
    if !Path::new(path).is_absolute() || !m.is_file() || m.file_type().is_symlink() {
        return Err(Error::Invalid(
            "Use arquivo regular absoluto; links simbólicos não são aceitos para manutenção."
                .into(),
        ));
    }
    let file = open(path)
        .map_err(|_| Error::Invalid("Arquivo local ausente ou link simbólico recusado.".into()))?;
    let fm = file.metadata().map_err(|_| Error::Unavailable)?;
    if !fm.is_file() {
        return Err(Error::Invalid(
            "Use arquivo regular absoluto; links simbólicos não são aceitos para manutenção."
                .into(),
        ));
    }
    // Ancestor aliases (e.g. macOS /var) are valid, but a path swap between
    // validation and open must not substitute another regular file.
    use std::os::unix::fs::MetadataExt;
    if (m.dev(), m.ino()) != (fm.dev(), fm.ino()) {
        return Err(Error::Conflict);
    }
    Ok(file)
}
fn regular(path: &str) -> Result<(), Error> {
    open_regular(path).map(|_| ())
}
fn extract(path: &str, key: &str) -> Result<String, Error> {
    run(
        "/usr/bin/plutil",
        &["-extract", key, "raw", "-o", "-", "--", path],
        false,
        6,
    )
    .map(|s| s.trim().to_owned())
}
fn hash(path: &str) -> Result<String, Error> {
    hash_within(path, Duration::from_secs(300))
}
fn hash_within(path: &str, limit: Duration) -> Result<String, Error> {
    use sha2::{Digest, Sha256};
    let mut file = open_regular(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let started = Instant::now();
    loop {
        if started.elapsed() >= limit {
            return Err(Error::Invalid(
                "Cálculo do SHA-256 excedeu o tempo limite; confira o arquivo antes de tentar novamente.".into(),
            ));
        }
        match std::io::Read::read(&mut file, &mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Error::Unavailable),
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn backup(path: &str) -> Result<(), Error> {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    let mut f = open_regular(path)?;
    let mut magic = [0u8; 2];
    f.read_exact(&mut magic).map_err(|_| Error::Format)?;
    let m = f.metadata().map_err(|_| Error::Unavailable)?;
    if magic != [0x1f, 0x8b] || m.len() < 3 || m.permissions().mode() & 0o077 != 0 {
        return Err(Error::Invalid("Selecione um backup gzip privado (0600). A presença do arquivo não comprova sua restauração ou origem; confirme-as antes de prosseguir.".into()));
    }
    Ok(())
}
fn launch_agent_logs(plist: &str) -> Result<Value, Error> {
    launch_agent_logs_with(plist, open_regular)
}
fn launch_agent_logs_with(
    plist: &str,
    open: impl Fn(&str) -> Result<std::fs::File, Error>,
) -> Result<Value, Error> {
    let mut logs = Vec::new();
    let mut unavailable = Vec::new();
    for key in ["StandardOutPath", "StandardErrorPath"] {
        let Ok(path) = extract(plist, key) else {
            unavailable.push(json!({"stream":key,"reason":"não configurado ou ilegível no plist"}));
            continue;
        };
        if !Path::new(&path).is_absolute() {
            return Err(Error::Invalid(
                "Caminho de log relativo no plist; use caminho absoluto.".into(),
            ));
        }
        if matches!(
            std::fs::symlink_metadata(&path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound
        ) {
            unavailable.push(json!({"stream":key,"reason":"arquivo ainda não criado"}));
            continue;
        }
        let raw = log_tail(open(&path)?)?;
        logs.push(json!({"stream":key,"text":raw}));
    }
    if logs.is_empty() {
        let detail = unavailable
            .iter()
            .map(|u| {
                format!(
                    "{}: {}",
                    u["stream"].as_str().unwrap_or(""),
                    u["reason"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(Error::Invalid(format!(
            "Nenhum arquivo de log regular configurado ou disponível para este LaunchAgent. {detail}"
        )));
    }
    Ok(json!({"logs":logs,"unavailable":unavailable}))
}
fn log_tail(mut file: impl std::io::Read + std::io::Seek) -> Result<String, Error> {
    use std::io::{Read, SeekFrom};
    // Snapshot the end offset: rotations keep the opened inode, and appends do
    // not extend this read. Three extra bytes preserve a UTF-8 boundary before
    // the final byte cap. Even a huge single line only reads this small window.
    let end = file
        .seek(SeekFrom::End(0))
        .map_err(|_| Error::Unavailable)?;
    let start = end.saturating_sub(100 * 1024 + 3);
    file.seek(SeekFrom::Start(start))
        .map_err(|_| Error::Unavailable)?;
    let mut bytes = Vec::new();
    file.take(end - start)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Unavailable)?;
    let scan_end = bytes
        .len()
        .saturating_sub(usize::from(bytes.last() == Some(&b'\n')));
    let mut breaks = 0;
    let mut first = 0;
    for i in (0..scan_end).rev() {
        if bytes[i] == b'\n' {
            breaks += 1;
            if breaks == 150 {
                first = i + 1;
                break;
            }
        }
    }
    let text = String::from_utf8_lossy(&bytes[first..]);
    Ok(tail(&text).to_owned())
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
    let same_file = {
        use std::os::unix::fs::MetadataExt;
        let a = open_regular(binary)?
            .metadata()
            .map_err(|_| Error::Unavailable)?;
        let b = open_regular(replacement)?
            .metadata()
            .map_err(|_| Error::Unavailable)?;
        (a.dev(), a.ino()) == (b.dev(), b.ino())
    };
    if same_file {
        return Err(Error::Invalid(
            "O novo binário deve estar em um caminho diferente do binário instalado.".into(),
        ));
    }
    let suffix = chrono::Utc::now()
        .timestamp_nanos_opt()
        .ok_or(Error::Unavailable)?;
    let old = format!("{binary}.backup-{suffix}");
    let stage = format!("{binary}.new-{suffix}");
    let copy_private = |from: &str, to: &str| -> Result<(), Error> {
        let mut src = open_regular(from)?;
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

    #[test]
    fn replace_rejects_identical_source_and_target_path() {
        let dir = std::env::temp_dir().join(format!(
            "monitor-local-same-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir(&dir).unwrap();
        let bin = dir.join("ai-memory");
        std::fs::write(&bin, "binary").unwrap();
        let digest = hash(bin.to_str().unwrap()).unwrap();
        assert_eq!(
            digest,
            "9a3a45d01531a20e89ac6ae10b0b0beb0492acd7216a368aa062d1a5fecaf9cd"
        );
        #[cfg(unix)]
        {
            let link = dir.join("symlink-bin");
            std::os::unix::fs::symlink(&bin, &link).unwrap();
            assert!(hash(link.to_str().unwrap()).is_err());
        }
        std::fs::hard_link(&bin, dir.join("hardlink")).unwrap();
        for alias in [
            bin.to_str().unwrap().to_owned(),
            dir.join("hardlink").to_str().unwrap().to_owned(),
            std::fs::canonicalize(&bin)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned(),
        ] {
            let res = replace(bin.to_str().unwrap(), &alias, &digest);
            assert!(
                res.is_err(),
                "replace must refuse alias of its own file: {alias}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(&bin).unwrap(),
            "binary",
            "refused replace must leave the binary untouched"
        );
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".backup-") || n.contains(".new-"))
            .collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            leftovers.is_empty(),
            "refused replace must not leave backups/staging: {leftovers:?}"
        );
    }

    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "monitor-{tag}-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        dir
    }
    #[cfg(target_os = "macos")]
    fn plist_with(entries: &[(&str, &str)]) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = unique_temp_dir("logs");
        let pairs: String = entries
            .iter()
            .map(|(k, v)| format!("<key>{k}</key><string>{v}</string>"))
            .collect();
        let plist = dir.join("agent.plist");
        std::fs::write(
            &plist,
            format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>{pairs}</dict></plist>"),
        )
        .unwrap();
        (dir, plist)
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn launch_agent_logs_reports_missing_streams_without_failing() {
        let dir = unique_temp_dir("logs");
        let out = dir.join("out.log");
        std::fs::write(&out, "out-line\n").unwrap();
        let err = dir.join("missing-err.log");
        let plist = dir.join("agent.plist");
        std::fs::write(
            &plist,
            format!(
                "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string></dict></plist>",
                out.display(),
                err.display()
            ),
        )
        .unwrap();
        let v = launch_agent_logs(plist.to_str().unwrap()).unwrap();
        assert_eq!(v["logs"].as_array().unwrap().len(), 1);
        assert!(
            v["logs"][0]["text"].as_str().unwrap().contains("out-line"),
            "log tail must include the file contents"
        );
        let un = &v["unavailable"].as_array().unwrap()[0];
        assert_eq!(un["stream"], "StandardErrorPath");
        assert_eq!(un["reason"], "arquivo ainda não criado");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn launch_agent_logs_rejects_dangling_symlink() {
        let dir = unique_temp_dir("logs");
        let out = dir.join("out.log");
        std::fs::write(&out, "out-line\n").unwrap();
        let dangling = dir.join("dangling-err.log");
        std::os::unix::fs::symlink(dir.join("gone"), &dangling).unwrap();
        let plist = dir.join("agent.plist");
        std::fs::write(
            &plist,
            format!(
                "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string></dict></plist>",
                out.display(),
                dangling.display()
            ),
        )
        .unwrap();
        assert!(
            launch_agent_logs(plist.to_str().unwrap()).is_err(),
            "dangling symlink must not be silently reported as not-created"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn launch_agent_logs_error_lists_each_stream_reason() {
        let (dir, plist) = plist_with(&[]);
        let Err(Error::Invalid(msg)) = launch_agent_logs(plist.to_str().unwrap()) else {
            panic!("missing streams must error");
        };
        let _ = std::fs::remove_dir_all(&dir);
        assert!(msg.contains("StandardOutPath"), "got: {msg}");
        assert!(msg.contains("StandardErrorPath"), "got: {msg}");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn launch_agent_logs_rejects_relative_log_path() {
        let (dir, plist) = plist_with(&[("StandardOutPath", "relative.log")]);
        let Err(Error::Invalid(msg)) = launch_agent_logs(plist.to_str().unwrap()) else {
            panic!("relative path must error");
        };
        let _ = std::fs::remove_dir_all(&dir);
        assert!(msg.contains("absoluto"), "got: {msg}");
    }

    #[test]
    fn open_nofollow_refuses_symlinks_and_does_not_block_on_fifo() {
        let dir = unique_temp_dir("nofollow");
        let target = dir.join("target.txt");
        std::fs::write(&target, "x").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(
            open_nofollow(link.to_str().unwrap()).is_err(),
            "O_NOFOLLOW must refuse opening through a symlink"
        );

        let fifo = dir.join("pipe");
        std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        let fifo_path = fifo.to_str().unwrap().to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(open_nofollow(&fifo_path).is_ok());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(2)).is_ok(),
            "open_nofollow must not block on a FIFO (needs O_NONBLOCK)"
        );
        assert!(
            open_regular(fifo.to_str().unwrap()).is_err(),
            "FIFO is not a regular file and must be refused"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_refuses_replaced_ancestor_but_accepts_stable_directory_alias() {
        use std::io::Read;
        let dir = unique_temp_dir("ancestor-race");
        let live = dir.join("live");
        let saved = dir.join("saved");
        let other = dir.join("other");
        std::fs::create_dir(&live).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::write(live.join("log"), "approved original").unwrap();
        std::fs::write(other.join("log"), "unapproved replacement").unwrap();
        let path = live.join("log");
        let result = open_regular_with(path.to_str().unwrap(), |p| {
            std::fs::rename(&live, &saved).unwrap();
            std::os::unix::fs::symlink(&other, &live).unwrap();
            open_nofollow(p)
        });
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "unapproved replacement"
        );
        assert!(
            matches!(result, Err(Error::Conflict)),
            "opening must reject changed file identity"
        );
        let mut content = String::new();
        open_regular(path.to_str().unwrap())
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(
            content, "unapproved replacement",
            "stable ancestor aliases remain supported"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn log_read_keeps_opened_file_when_path_is_replaced() {
        let dir = unique_temp_dir("log-rotation");
        let out = dir.join("out.log");
        std::fs::write(&out, "approved original\n").unwrap();
        let (plist_dir, plist) = plist_with(&[("StandardOutPath", out.to_str().unwrap())]);
        let value = launch_agent_logs_with(plist.to_str().unwrap(), |path| {
            let file = open_regular(path)?;
            std::fs::rename(&out, dir.join("rotated.log")).unwrap();
            std::fs::write(&out, "unapproved replacement\n").unwrap();
            Ok(file)
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "unapproved replacement\n"
        );
        assert_eq!(
            value["logs"][0]["text"], "approved original\n",
            "tail must consume the opened descriptor, never reopen the path"
        );
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(plist_dir).unwrap();
    }

    #[test]
    fn log_tail_has_exact_line_and_utf8_bounds() {
        use std::io::Cursor;
        assert_eq!(log_tail(Cursor::new(b"")).unwrap(), "");
        for count in [1usize, 149, 150, 151, 300] {
            for terminated in [false, true] {
                let lines: Vec<_> = (0..count).map(|n| format!("line-{n}")).collect();
                let ending = if terminated { "\n" } else { "" };
                let input = format!("{}{ending}", lines.join("\n"));
                let expected = format!("{}{ending}", lines[count.saturating_sub(150)..].join("\n"));
                assert_eq!(
                    log_tail(Cursor::new(input)).unwrap(),
                    expected,
                    "{count} lines, terminated={terminated}"
                );
            }
        }
        assert_eq!(
            log_tail(Cursor::new("\n".repeat(151))).unwrap(),
            "\n".repeat(150)
        );
        for text in ["á", "€", "😀"] {
            let input = text.repeat(150_000);
            let actual = log_tail(Cursor::new(input.as_bytes())).unwrap();
            assert_eq!(actual, tail(&input), "UTF-8 cap for {text}");
            assert!(!actual.contains('�'), "valid UTF-8 must stay valid");
        }
        let invalid = vec![0xff; 150_000];
        let actual = log_tail(Cursor::new(invalid)).unwrap();
        assert!(actual.len() <= 100 * 1024);
        assert!(actual.chars().all(|c| c == '�'));
    }

    #[test]
    fn log_tail_bounds_io_and_does_not_follow_appends() {
        use std::io::{Cursor, Read, Seek, SeekFrom};
        struct GrowingLog {
            file: Cursor<Vec<u8>>,
            read: usize,
        }
        impl Read for GrowingLog {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let count = self.file.read(out)?;
                self.read += count;
                Ok(count)
            }
        }
        impl Seek for GrowingLog {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
                let result = self.file.seek(pos)?;
                if matches!(pos, SeekFrom::End(0)) {
                    self.file
                        .get_mut()
                        .extend_from_slice(b"unapproved append\n");
                }
                Ok(result)
            }
        }
        let original = format!("{}original end\n", "x".repeat(2_000_000));
        let mut file = GrowingLog {
            file: Cursor::new(original.clone().into_bytes()),
            read: 0,
        };
        let actual = log_tail(&mut file).unwrap();
        assert_eq!(
            actual,
            tail(&original),
            "appends after the observed end must not join this result"
        );
        assert_eq!(
            file.read,
            100 * 1024 + 3,
            "read only the fixed tail window, not the entire file"
        );
        assert!(file.file.get_ref().ends_with(b"unapproved append\n"));
    }

    #[test]
    fn hash_within_honors_a_zero_deadline() {
        let dir = unique_temp_dir("hashlimit");
        let file = dir.join("nonempty");
        std::fs::write(&file, "data").unwrap();
        let Err(Error::Invalid(msg)) = hash_within(file.to_str().unwrap(), Duration::ZERO) else {
            panic!("zero deadline must abort hashing a non-empty file");
        };
        assert!(
            msg.contains("tempo limite"),
            "deadline error must say the hash timed out, got: {msg}"
        );
        let Ok(digest) = hash_within(file.to_str().unwrap(), Duration::from_secs(60)) else {
            panic!("generous deadline must hash normally");
        };
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            digest,
            "3a6eb0790f39ac87c94f3856b2dd2c5d110e6811602261a9a923d3bb23adc8b7"
        );
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn systemd_is_refused_on_non_linux_before_any_command() {
        let cfg = Config::default();
        for preview in [true, false] {
            let mut op = Operation::new(Backend::Systemd, Kind::Status);
            op.values
                .insert("unit".into(), "/tmp/ai-memory.service".into());
            let Err(Error::Invalid(msg)) = op.execute(&cfg, preview, || false) else {
                panic!("Systemd must be refused on this platform");
            };
            assert!(
                msg.contains("somente no Linux"),
                "preview={preview}, got: {msg}"
            );
        }
    }

    #[cfg(target_os = "linux")]
    fn show(lines: &[(&str, &str)]) -> String {
        lines
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[cfg(target_os = "linux")]
    fn good_show() -> String {
        show(&[
            ("LoadState", "loaded"),
            (
                "FragmentPath",
                "/home/u/.config/systemd/user/ai-memory.service",
            ),
            ("DropInPaths", ""),
            ("NeedDaemonReload", "no"),
            (
                "ExecStart",
                "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory --data-dir /home/u/.local/share/ai-memory --config /home/u/.config/ai-memory/config.toml serve --transport http --enable-web ; ignore_errors=no ; start_time=[n/a] ; pid=0 ; code=(null) ; status=0/0 }",
            ),
            ("ActiveState", "inactive"),
            ("SubState", "dead"),
            ("MainPID", "0"),
        ])
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn systemd_parser_accepts_official_renderings() {
        for exec in [
            "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory --data-dir /home/u/d --config /home/u/c serve --transport http --enable-web ; pid=0 }",
            "{ path=/usr/bin/ai-memory ; argv[]=\"/usr/bin/ai-memory\" \"--data-dir\" \"/home/u/d\" \"--config\" \"/home/u/c\" \"serve\" \"--transport\" \"http\" \"--enable-web\" ; pid=0 }",
            "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory --data-dir=/home/u/d --config=/home/u/c serve ; pid=0 }",
            "{ path=/opt/ai-memory ; argv[]=/opt/ai-memory --verbose serve ; pid=0 }",
        ] {
            let text = show(&[
                ("LoadState", "loaded"),
                ("FragmentPath", "/u/ai-memory.service"),
                ("NeedDaemonReload", "no"),
                ("ExecStart", exec),
                ("ActiveState", "active"),
                ("SubState", "running"),
                ("MainPID", "42"),
            ]);
            let parsed = parse_systemd_show(&text, "/u/ai-memory.service").unwrap();
            assert_eq!(parsed.active_state, "active");
            assert_eq!(parsed.main_pid, "42");
            assert!(parsed.binary.ends_with("/ai-memory"), "{exec}");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn systemd_parser_refuses_bad_units() {
        let base = [
            ("LoadState", "loaded"),
            ("FragmentPath", "/u/ai-memory.service"),
            ("NeedDaemonReload", "no"),
        ];
        let exec = |e: &str| {
            show(
                &base
                    .iter()
                    .cloned()
                    .chain([
                        ("ExecStart", e),
                        ("ActiveState", "inactive"),
                        ("SubState", "dead"),
                        ("MainPID", "0"),
                    ])
                    .collect::<Vec<_>>(),
            )
        };
        for (text, why) in [
            (
                exec("{ path=/usr/bin/ai-memory ; argv[]=ai-memory serve ; }"),
                "relative argv0",
            ),
            (
                exec("{ path=/usr/bin/other ; argv[]=/usr/bin/other serve ; }"),
                "wrong basename",
            ),
            (
                exec("{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory ; }"),
                "no subcommand",
            ),
            (
                exec("{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory reindex ; }"),
                "wrong subcommand",
            ),
            (exec(""), "empty ExecStart"),
            (
                exec(
                    "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory serve ; } { path=/bin/true ; argv[]=/bin/true ; }",
                ),
                "two commands",
            ),
            (
                exec(
                    "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory \"unterminated serve ; }",
                ),
                "unbalanced quote",
            ),
            (
                exec("{ path=/usr/bin/other ; argv[]=/home/u/ai-memory serve ; }"),
                "@-prefix: path is not ai-memory while argv[0] is",
            ),
            (
                exec("{ path=/usr/bin/ai-memory ; argv[]=/home/u/ai-memory serve ; }"),
                "path differs from argv[0] even with matching basename",
            ),
        ] {
            assert!(
                parse_systemd_show(&text, "/u/ai-memory.service").is_err(),
                "must refuse: {why}"
            );
        }
        for (fields, why) in [
            (
                vec![
                    ("LoadState", "not-found"),
                    ("FragmentPath", "/u/ai-memory.service"),
                    ("NeedDaemonReload", "no"),
                    (
                        "ExecStart",
                        "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory serve ; }",
                    ),
                ],
                "not-found",
            ),
            (
                vec![
                    ("LoadState", "masked"),
                    ("FragmentPath", "/u/ai-memory.service"),
                    ("NeedDaemonReload", "no"),
                    (
                        "ExecStart",
                        "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory serve ; }",
                    ),
                ],
                "masked",
            ),
            (
                vec![
                    ("LoadState", "loaded"),
                    ("FragmentPath", "/other/ai-memory.service"),
                    ("NeedDaemonReload", "no"),
                    (
                        "ExecStart",
                        "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory serve ; }",
                    ),
                ],
                "fragment mismatch",
            ),
            (
                vec![
                    ("LoadState", "loaded"),
                    ("FragmentPath", "/u/ai-memory.service"),
                    ("NeedDaemonReload", "yes"),
                    (
                        "ExecStart",
                        "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory serve ; }",
                    ),
                ],
                "daemon-reload needed",
            ),
        ] {
            let text = show(&fields.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>());
            assert!(
                parse_systemd_show(&text, "/u/ai-memory.service").is_err(),
                "must refuse: {why}"
            );
        }
        let parsed = parse_systemd_show(
            &good_show(),
            "/home/u/.config/systemd/user/ai-memory.service",
        )
        .unwrap();
        assert_eq!(parsed.sub_state, "dead");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn systemd_extra_commands_are_parsed_or_empty() {
        let parsed = parse_systemd_show(
            &show(&[
                ("LoadState", "loaded"),
                ("FragmentPath", "/u/ai-memory.service"),
                ("NeedDaemonReload", "no"),
                ("ActiveState", "inactive"),
                (
                    "ExecStart",
                    "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory serve ; }",
                ),
                (
                    "ExecStartPre",
                    "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory check ; }",
                ),
                (
                    "ExecStop",
                    "{ path=/usr/bin/kill ; argv[]=/usr/bin/kill ai-memory ; }",
                ),
            ]),
            "/u/ai-memory.service",
        )
        .unwrap();
        assert_eq!(
            parsed.extra_commands.len(),
            2,
            "{:?}",
            parsed.extra_commands
        );
        assert!(parsed.extra_commands[0].contains("check"));
        assert!(parsed.extra_commands[1].contains("kill"));
        let parsed = parse_systemd_show(
            &show(&[
                ("LoadState", "loaded"),
                ("FragmentPath", "/u/ai-memory.service"),
                ("NeedDaemonReload", "no"),
                ("ActiveState", "inactive"),
                (
                    "ExecStart",
                    "{ path=/usr/bin/ai-memory ; argv[]=/usr/bin/ai-memory serve ; }",
                ),
            ]),
            "/u/ai-memory.service",
        )
        .unwrap();
        assert_eq!(parsed.extra_commands.len(), 0);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn systemd_update_only_when_inactive_or_failed() {
        // ActiveState domain; only inactive|failed may bind an update.
        for (state, allowed) in [
            ("inactive", true),
            ("failed", true),
            ("active", false),
            ("reloading", false),
            ("activating", false),
            ("deactivating", false),
            ("", false),
        ] {
            assert_eq!(systemd_update_allowed(state), allowed, "{state}");
        }
    }

    #[test]
    fn systemd_unit_must_be_absolute_and_named_ai_memory_service() {
        let mut op = Operation::new(Backend::Systemd, Kind::Status);
        for bad in [
            "relative.service",
            "/tmp/other.service",
            "ai-memory.service",
        ] {
            op.values.insert("unit".into(), bad.into());
            assert!(op.validate().is_err(), "must refuse unit={bad}");
        }
        op.values.insert(
            "unit".into(),
            "/home/u/.config/systemd/user/ai-memory.service".into(),
        );
        assert!(op.validate().is_ok());
    }

    #[test]
    #[ignore = "requer um gerenciador systemd --user real (linger ativo, barramento do usuário)"]
    #[cfg(target_os = "linux")]
    fn systemd_backend_full_lifecycle_against_real_user_manager() {
        use std::os::unix::fs::PermissionsExt;
        let home = std::env::var("HOME").expect("HOME");
        let dir = unique_temp_dir("systemd-it");
        let bin = dir.join("ai-memory");
        std::fs::write(&bin, "#!/bin/sh\necho fake-ai-memory-v1\nexec sleep 3600\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        let unit_dir = std::path::Path::new(&home).join(".config/systemd/user");
        std::fs::create_dir_all(&unit_dir).unwrap();
        let unit = unit_dir.join("ai-memory.service");
        std::fs::write(
            &unit,
            format!(
                "[Unit]\nDescription=ai-memory test\n[Service]\nExecStart={} --data-dir={}/data --config={}/config.toml serve --transport http --enable-web\n[Install]\nWantedBy=default.target\n",
                bin.display(),
                dir.display(),
                dir.display()
            ),
        )
        .unwrap();
        let cleanup = |dir: &std::path::Path, unit: &std::path::Path| {
            let _ = std::process::Command::new("/usr/bin/systemctl")
                .args(["--user", "stop", "ai-memory.service"])
                .status();
            let _ = std::fs::remove_file(unit);
            let _ = std::fs::remove_dir_all(dir);
            let _ = std::process::Command::new("/usr/bin/systemctl")
                .args(["--user", "daemon-reload"])
                .status();
        };
        std::process::Command::new("/usr/bin/systemctl")
            .args(["--user", "daemon-reload"])
            .status()
            .unwrap();
        // Aufräumen vorab: ein abgebrochener Lauf könnte den Dienst aktiv lassen.
        let _ = std::process::Command::new("/usr/bin/systemctl")
            .args(["--user", "stop", "ai-memory.service"])
            .status();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let cfg = Config::default();
        let op = |kind| {
            let mut op = Operation::new(Backend::Systemd, kind);
            op.values
                .insert("unit".into(), unit.to_str().unwrap().into());
            op
        };
        let run = |kind, preview| op(kind).execute(&cfg, preview, || false);
        let run_bound = |kind| {
            let mut o = op(kind);
            let preview = o.execute(&cfg, true, || false).unwrap();
            o.bind(&preview).unwrap();
            o.execute(&cfg, false, || false)
        };
        // Status enquanto parado.
        let state = run(Kind::Status, false).unwrap();
        assert_eq!(state["active_state"], "inactive", "{state}");
        // Start com preview + binding.
        run_bound(Kind::Start).unwrap();
        for _ in 0..50 {
            if run(Kind::Status, false).unwrap()["active_state"] == "active" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let state = run(Kind::Status, false).unwrap();
        assert_eq!(state["active_state"], "active", "{state}");
        // Logs devem conter a linha do binário falso.
        let logs = run(Kind::Logs, false).unwrap();
        assert!(
            logs["logs"].as_str().unwrap().contains("fake-ai-memory-v1"),
            "{logs}"
        );
        run_bound(Kind::Restart).unwrap();
        // Update recusado com o serviço ativo.
        let update_refused = op(Kind::Update);
        assert!(update_refused.execute(&cfg, true, || false).is_err());
        run_bound(Kind::Stop).unwrap();
        for _ in 0..50 {
            if run(Kind::Status, false).unwrap()["active_state"] == "inactive" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // Update com binário novo + SHA-256 + backup gzip privado.
        let replacement = dir.join("ai-memory-new");
        std::fs::write(
            &replacement,
            "#!/bin/sh\necho fake-ai-memory-v2\nexec sleep 3600\n",
        )
        .unwrap();
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o700)).unwrap();
        let backup_path = dir.join("backup.tar.gz");
        std::fs::write(&backup_path, b"\x1f\x8bbackup").unwrap();
        std::fs::set_permissions(&backup_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let digest = hash(replacement.to_str().unwrap()).unwrap();
        let mut update = op(Kind::Update);
        update
            .values
            .insert("backup".into(), backup_path.to_str().unwrap().into());
        update
            .values
            .insert("replacement".into(), replacement.to_str().unwrap().into());
        update.values.insert("sha256".into(), digest);
        let preview = update.execute(&cfg, true, || false).unwrap();
        update.bind(&preview).unwrap();
        let result = update.execute(&cfg, false, || false).unwrap();
        assert_eq!(result["binary_replaced"], true, "{result}");
        assert!(
            std::fs::read_to_string(&bin).unwrap().contains("v2"),
            "installed binary must be the replacement"
        );
        // Start com o binário novo funciona.
        run_bound(Kind::Start).unwrap();
        for _ in 0..50 {
            if run(Kind::Status, false).unwrap()["active_state"] == "active" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let logs = run(Kind::Logs, false).unwrap();
        assert!(
            logs["logs"].as_str().unwrap().contains("fake-ai-memory-v2"),
            "{logs}"
        );
        cleanup(&dir, &unit);
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn launch_agent_is_refused_on_non_macos_before_any_command() {
        let cfg = Config::default();
        for preview in [true, false] {
            let mut op = Operation::new(Backend::LaunchAgent, Kind::Status);
            op.values.insert("plist".into(), "/tmp/agent.plist".into());
            let Err(Error::Invalid(msg)) = op.execute(&cfg, preview, || false) else {
                panic!("LaunchAgent must be refused on this platform");
            };
            assert!(
                msg.contains("somente no macOS"),
                "preview={preview}, got: {msg}"
            );
        }
    }
}
