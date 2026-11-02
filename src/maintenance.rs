//! Explicit administrative intents. Every mutation follows a bound review receipt.
use crate::{
    client::{Connection, Error, Scope, check_status, page_segments},
    model::Config,
    monitor::Shared,
    queries::Query,
};
use reqwest::Method;
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{Read as IoRead, Write},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Lint,
    Curator,
    Sweep,
    Embeddings,
}
impl Task {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lint => "Auditar memória",
            Self::Curator => "Gerar proposta de curadoria",
            Self::Sweep => "Aplicar retenção",
            Self::Embeddings => "Gerar embeddings",
        }
    }
    fn route(self) -> &'static str {
        match self {
            Self::Lint => "lint",
            Self::Curator => "curator",
            Self::Sweep => "forget-sweep",
            Self::Embeddings => "embed",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Intent {
    Approve {
        id: String,
    },
    Reject {
        id: String,
        reason: String,
    },
    CancelHandoff {
        id: String,
    },
    ExpireHandoffs {
        days: u32,
    },
    Maintain {
        task: Task,
        reembed: bool,
    },
    Consolidate {
        session: String,
    },
    Feedback {
        path: String,
        signal: String,
        reason: String,
    },
    Checkpoint {
        message: String,
    },
    Restore {
        path: String,
        version: String,
        git: bool,
    },
    Download {
        destination: PathBuf,
        okf: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub scope: Scope,
    pub intent: Intent,
}
impl Action {
    pub fn label(&self) -> String {
        match &self.intent {
            Intent::Approve { id } => format!("Aprovar proposta {id}"),
            Intent::Reject { id, .. } => format!("Rejeitar proposta {id}"),
            Intent::CancelHandoff { id } => format!("Cancelar handoff {id}"),
            Intent::ExpireHandoffs { days } => {
                format!("Expirar handoffs com pelo menos {days} dias")
            }
            Intent::Maintain { task, .. } => task.label().into(),
            Intent::Consolidate { session } => format!("Reconsolidar sessão {session}"),
            Intent::Feedback { path, signal, .. } => format!("Avaliar {path}: {signal}"),
            Intent::Checkpoint { .. } => "Criar checkpoint global da wiki".into(),
            Intent::Restore { path, version, .. } => {
                format!("Restaurar {path} a partir de {version}")
            }
            Intent::Download { okf, .. } => if *okf {
                "Exportar projeto em OKF"
            } else {
                "Salvar backup de toda a instalação"
            }
            .into(),
        }
    }
    pub fn warning(&self) -> &'static str {
        match self.intent {
            Intent::Approve { .. } => {
                "Aplica a proposta à wiki. O servidor verifica conflitos com a página atual."
            }
            Intent::Reject { .. } => "Registra a rejeição e o motivo na memória do projeto.",
            Intent::CancelHandoff { .. } | Intent::ExpireHandoffs { .. } => {
                "O handoff deixa de estar disponível para retomada. A prévia de expiração é limitada a 200 itens; o servidor reavalia todos no momento da ação."
            }
            Intent::Maintain {
                task: Task::Sweep, ..
            } => {
                "Pode remover ou compactar memórias e eventos conforme a retenção do servidor. Faça backup antes. A prévia não bloqueia mudanças concorrentes."
            }
            Intent::Maintain {
                task: Task::Embeddings,
                ..
            } => {
                "Pode consumir o provider de embeddings e gerar cobrança. A prévia somente conta páginas."
            }
            Intent::Maintain {
                task: Task::Curator,
                ..
            } => {
                "Cria uma proposta de relatório para revisão; não aprova automaticamente alterações."
            }
            Intent::Maintain {
                task: Task::Lint, ..
            } => "Grava o relatório de auditoria. Esta opção usa regras sem chamar o LLM.",
            Intent::Consolidate { .. } => {
                "Chama o LLM e grava memória; pode gerar cobrança. O destino é escolhido pelo servidor a partir da sessão e pode ser outro projeto. O projeto selecionado não limita essa chamada. A prévia de admissão mostra o caminho, não o texto final."
            }
            Intent::Feedback { .. } => {
                "Registra feedback sobre a versão atual e pode afetar sua relevância nas buscas."
            }
            Intent::Checkpoint { .. } => {
                "O checkpoint inclui alterações da wiki inteira, não somente deste projeto. Não substitui o backup do banco."
            }
            Intent::Restore { .. } => {
                "Cria uma nova versão com conteúdo antigo. A API não oferece bloqueio por versão atual nesta operação; evite edições concorrentes. Checkpoint Git não oferece prévia do corpo por HTTP."
            }
            Intent::Download { okf: false, .. } => {
                "Backup global: contém banco, wiki/histórico e config.toml, que pode conter segredos. O arquivo será privado (0600), sem sobrescrever existentes. Nunca publique este backup."
            }
            Intent::Download { okf: true, .. } => {
                "Exporta o conhecimento do projeto. O arquivo pode conter dados privados e será salvo com permissão 0600. Não é um backup completo do banco."
            }
        }
    }
    fn validate(&self) -> Result<(), Error> {
        self.scope.validate()?;
        let required = |s: &str| {
            if s.trim().is_empty() || s.len() > 4096 || s.chars().any(char::is_control) {
                Err(Error::Invalid(
                    "Informe um identificador válido e explícito.".into(),
                ))
            } else {
                Ok(())
            }
        };
        match &self.intent {
            Intent::Approve { id } | Intent::Reject { id, .. } | Intent::CancelHandoff { id } => {
                required(id)?
            }
            Intent::Consolidate { session } => required(session)?,
            Intent::Feedback {
                path,
                signal,
                reason,
            } => {
                page_segments(path)?;
                if !["helpful", "not_helpful", "stale", "wrong"].contains(&signal.as_str())
                    || reason.chars().count() > 500
                {
                    return Err(Error::Invalid(
                        "Feedback inválido ou motivo acima de 500 caracteres.".into(),
                    ));
                }
            }
            Intent::Restore { path, version, .. } => {
                page_segments(path)?;
                required(version)?;
            }
            Intent::Checkpoint { message } => required(message)?,
            Intent::Download { destination, .. }
                if !destination.is_absolute()
                    || destination.file_name().is_none()
                    || destination.exists() =>
            {
                return Err(Error::Invalid("Escolha um arquivo novo com caminho absoluto; arquivos existentes não são sobrescritos.".into()));
            }
            _ => {}
        }
        if let Intent::Reject { reason, .. } = &self.intent
            && reason.chars().count() > 500
        {
            return Err(Error::Invalid("Motivo limitado a 500 caracteres.".into()));
        }
        Ok(())
    }
    fn scope_query(&self) -> Vec<(&'static str, String)> {
        vec![
            ("workspace", self.scope.workspace.clone()),
            ("project", self.scope.project.clone()),
        ]
    }
    fn body(&self) -> Value {
        json!({"workspace":self.scope.workspace,"project":self.scope.project})
    }
    pub fn preview(
        &self,
        client: &Connection,
        cancelled: impl Fn() -> bool,
    ) -> Result<Value, Error> {
        self.validate()?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        match &self.intent {
            Intent::Approve { id } | Intent::Reject { id, .. } => {
                let detail: Value =
                    client.get(&["admin", "pending-writes", id], &self.scope_query())?;
                if detail["summary"]["id"] != *id {
                    return Err(Error::Format);
                }
                if detail["summary"]["status"] != "pending" {
                    return Err(Error::Conflict);
                }
                if cancelled() {
                    return Err(Error::Cancelled);
                }
                let diff: Value = client.get(
                    &["admin", "pending-writes", id, "diff"],
                    &self.scope_query(),
                )?;
                if !diff["diff"].is_string() || diff["proposal_id"] != *id {
                    return Err(Error::Format);
                }
                Ok(json!({"proposal":detail,"comparison":diff}))
            }
            Intent::Maintain { task, reembed } => self.maintenance(client, *task, *reembed, true),
            Intent::Consolidate { session } => client.tool(
                "memory_consolidate",
                &json!({"session_id":session,"dry_run":true,"multi_page":false}),
                cancelled,
            ),
            Intent::CancelHandoff { id } => {
                let v = Query::Handoffs {
                    scope: self.scope.clone(),
                    state: "open".into(),
                }
                .execute(client)?;
                let found = v["handoffs"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|r| r["id"] == *id))
                    .ok_or(Error::Missing)?;
                Ok(found.clone())
            }
            Intent::ExpireHandoffs { days } => {
                let v = Query::Handoffs {
                    scope: self.scope.clone(),
                    state: "open".into(),
                }
                .execute(client)?;
                Ok(
                    json!({"older_than_days":days,"limited_open_handoffs":v["handoffs"],"notice":"Lista não é uma prévia transacional; até 200 registros visíveis."}),
                )
            }
            Intent::Feedback { path, .. } => Query::Page {
                scope: self.scope.clone(),
                path: path.clone(),
            }
            .execute(client),
            Intent::Restore {
                path,
                version,
                git: false,
            } => {
                let current = Query::Page {
                    scope: self.scope.clone(),
                    path: path.clone(),
                }
                .execute(client)?;
                if cancelled() {
                    return Err(Error::Cancelled);
                }
                Query::History {
                    scope: self.scope.clone(),
                    path: path.clone(),
                    offset: 0,
                    from: current["id"].as_str().ok_or(Error::Format)?.into(),
                    to: version.clone(),
                }
                .execute(client)
            }
            Intent::Restore {
                version, git: true, ..
            } => {
                let checkpoints: Value =
                    client.get(&["admin", "checkpoints"], &[("limit", "100".into())])?;
                let found = checkpoints
                    .as_array()
                    .and_then(|rows| rows.iter().find(|r| r["oid"] == *version))
                    .ok_or(Error::Missing)?;
                Ok(
                    json!({"checkpoint":found,"notice":"Revisão validada na lista dos últimos 100 checkpoints; sem prévia do corpo."}),
                )
            }
            Intent::Checkpoint { message } => Ok(json!({"scope":"Toda a wiki","message":message})),
            Intent::Download { destination, okf } => Ok(
                json!({"scope":if *okf {self.scope.label()} else {"Toda a instalação".into()},"destination":destination,"permissions":"0600","overwrite":false}),
            ),
        }
    }
    fn maintenance(
        &self,
        c: &Connection,
        task: Task,
        reembed: bool,
        preview: bool,
    ) -> Result<Value, Error> {
        let mut body = self.body();
        body["dry_run"] = json!(preview);
        match task {
            Task::Lint => body["no_llm"] = json!(true),
            Task::Curator => body["stage"] = json!(!preview),
            Task::Embeddings => {
                body["reembed"] = json!(reembed);
                body["all_projects"] = json!(false);
            }
            Task::Sweep => {}
        }
        c.post(&["admin", task.route()], &[], &body)
    }
    fn execute(&self, c: &Connection, cancelled: impl Fn() -> bool) -> Result<Value, Error> {
        self.validate()?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut body = self.body();
        match &self.intent {
            Intent::Approve { id } => c.post(
                &["admin", "pending-writes", id, "approve"],
                &self.scope_query(),
                &json!({}),
            ),
            Intent::Reject { id, reason } => c.post(
                &["admin", "pending-writes", id, "reject"],
                &self.scope_query(),
                &json!({"reason":reason}),
            ),
            Intent::CancelHandoff { id } => {
                body["handoff_id"] = json!(id);
                body["any_owner"] = json!(false);
                c.tool("memory_handoff_cancel", &body, cancelled)
            }
            Intent::ExpireHandoffs { days } => {
                body["confirm"] = json!(true);
                body["older_than_days"] = json!(days);
                c.post(&["admin", "handoffs", "expire"], &[], &body)
            }
            Intent::Maintain { task, reembed } => self.maintenance(c, *task, *reembed, false),
            Intent::Consolidate { session } => c.tool(
                "memory_consolidate",
                &json!({"session_id":session,"dry_run":false,"multi_page":false}),
                cancelled,
            ),
            Intent::Feedback {
                path,
                signal,
                reason,
            } => {
                body["path"] = json!(path);
                body["signal"] = json!(signal);
                body["reason"] = json!(reason);
                c.tool("memory_feedback", &body, cancelled)
            }
            Intent::Checkpoint { message } => {
                c.post(&["admin", "commit"], &[], &json!({"message":message}))
            }
            Intent::Restore { path, version, git } => {
                body["path"] = json!(path);
                body[if *git { "rev" } else { "page_id" }] = json!(version);
                c.post(&["admin", "restore-page"], &[], &body)
            }
            Intent::Download { destination, okf } => {
                download(c, destination, *okf, &self.scope_query(), cancelled)
            }
        }
    }
}
fn download(
    c: &Connection,
    path: &std::path::Path,
    okf: bool,
    query: &[(&str, String)],
    cancelled: impl Fn() -> bool,
) -> Result<Value, Error> {
    use std::os::unix::fs::OpenOptionsExt;
    const LIMIT: u64 = 2 * 1024 * 1024 * 1024;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options.open(path).map_err(|_| {
        Error::Invalid(
            "Não foi possível criar o arquivo privado; confira o destino e as permissões.".into(),
        )
    })?;
    let result = (|| {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut response = c
            .request_builder(
                Method::POST,
                &["admin", if okf { "export-okf" } else { "backup" }],
                if okf { query } else { &[] },
            )?
            .timeout(Duration::from_secs(300))
            .send()
            .map_err(|_| Error::Unavailable)?;
        check_status(&response)?;
        if response.content_length().is_some_and(|n| n > LIMIT) {
            return Err(Error::Invalid(
                "Arquivo excedeu o limite de download de 2 GiB.".into(),
            ));
        }
        let mut bytes = 0u64;
        let mut buffer = [0u8; 65536];
        let mut signature: Vec<u8> = Vec::new();
        loop {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let n = response.read(&mut buffer).map_err(|_| Error::Unavailable)?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            if bytes > LIMIT {
                return Err(Error::Invalid(
                    "Arquivo excedeu o limite de download de 2 GiB.".into(),
                ));
            }
            signature.extend(
                buffer[..n]
                    .iter()
                    .take(2usize.saturating_sub(signature.len())),
            );
            file.write_all(&buffer[..n])
                .map_err(|_| Error::Invalid("Falha ao gravar o arquivo privado.".into()))?;
        }
        if signature != [0x1f, 0x8b] {
            return Err(Error::Format);
        }
        file.sync_all()
            .map_err(|_| Error::Invalid("Falha ao confirmar a gravação em disco.".into()))?;
        Ok(json!({"saved":true,"bytes":bytes,"file":path,"permissions":"0600"}))
    })();
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[derive(Clone)]
pub enum Read {
    Proposal { scope: Scope, id: String },
    Proposals { scope: Scope, status: String },
    Handoffs(Scope),
    Checkpoints,
}
impl Read {
    fn execute(&self, c: &Connection) -> Result<Value, Error> {
        match self {
            Self::Proposal { scope, id } => {
                scope.validate()?;
                let v: Value = c.get(
                    &["admin", "pending-writes", id],
                    &[
                        ("workspace", scope.workspace.clone()),
                        ("project", scope.project.clone()),
                    ],
                )?;
                if v["summary"]["id"] != *id {
                    return Err(Error::Format);
                }
                Ok(v)
            }
            Self::Proposals { scope, status } => {
                scope.validate()?;
                c.get(
                    &["admin", "pending-writes"],
                    &[
                        ("workspace", scope.workspace.clone()),
                        ("project", scope.project.clone()),
                        ("status", status.clone()),
                        ("limit", "200".into()),
                    ],
                )
            }
            Self::Handoffs(scope) => Query::Handoffs {
                scope: scope.clone(),
                state: String::new(),
            }
            .execute(c),
            Self::Checkpoints => c.get(&["admin", "checkpoints"], &[("limit", "100".into())]),
        }
    }
}
#[derive(Clone)]
pub struct Receipt {
    pub action: Action,
    pub generation: u64,
    pub at: Instant,
}
impl Receipt {
    fn validate(&self, shared: &Shared) -> Result<(), Error> {
        if shared.generation.load(Ordering::Acquire) != self.generation {
            return Err(Error::Cancelled);
        }
        if self.at.elapsed() > Duration::from_secs(300) {
            return Err(Error::Invalid(
                "A prévia expirou. Prepare a operação novamente.".into(),
            ));
        }
        self.action.validate()
    }
}
pub enum Work {
    Read(Read),
    Preview(Action),
    Apply(Receipt),
}
struct Job {
    work: Work,
    config: Config,
    generation: u64,
    ticket: u64,
}
pub struct Reply {
    pub ticket: u64,
    pub generation: u64,
    pub receipt: Option<Receipt>,
    pub applying: bool,
    pub result: Result<Value, Error>,
}
pub struct Worker {
    sender: SyncSender<Job>,
    pub replies: Receiver<Reply>,
    stop: Arc<AtomicBool>,
}
impl Worker {
    pub fn new(shared: Arc<Shared>) -> Self {
        let (sender, jobs) = mpsc::sync_channel::<Job>(1);
        let (responses, replies) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            loop {
                if stopped.load(Ordering::Relaxed) || shared.stop.load(Ordering::Relaxed) {
                    break;
                }
                let job = match jobs.recv_timeout(Duration::from_millis(100)) {
                    Ok(j) => j,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(_) => break,
                };
                let cancelled = || {
                    stopped.load(Ordering::Relaxed)
                        || shared.stop.load(Ordering::Relaxed)
                        || shared.generation.load(Ordering::Acquire) != job.generation
                };
                let applying = matches!(job.work, Work::Apply(_));
                let mut receipt = None;
                let result = (|| {
                    if cancelled() {
                        return Err(Error::Cancelled);
                    }
                    let client = Connection::new(&job.config)?;
                    match job.work {
                        Work::Read(read) => read.execute(&client),
                        Work::Preview(action) => {
                            let v = action.preview(&client, cancelled)?;
                            if cancelled() {
                                return Err(Error::Cancelled);
                            }
                            receipt = Some(Receipt {
                                action,
                                generation: job.generation,
                                at: Instant::now(),
                            });
                            Ok(v)
                        }
                        Work::Apply(review) => {
                            review.validate(&shared)?;
                            review.action.execute(&client, cancelled)
                        }
                    }
                })();
                if responses
                    .send(Reply {
                        ticket: job.ticket,
                        generation: job.generation,
                        receipt,
                        applying,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender,
            replies,
            stop,
        }
    }
    pub fn submit(&self, ticket: u64, shared: &Shared, work: Work) -> Result<(), Error> {
        if let Work::Apply(receipt) = &work {
            receipt.validate(shared)?;
        }
        let config = shared.config.read().unwrap();
        self.sender
            .try_send(Job {
                work,
                config: config.clone(),
                generation: shared.generation.load(Ordering::Acquire),
                ticket,
            })
            .map_err(|_| Error::Invalid("Aguarde a operação atual.".into()))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Mock, Response};
    fn scope() -> Scope {
        Scope {
            workspace: "synthetic team".into(),
            project: "demo".into(),
        }
    }
    fn action(intent: Intent) -> Action {
        Action {
            scope: scope(),
            intent,
        }
    }
    #[test]
    fn every_maintenance_preview_is_explicitly_dry_and_scoped() {
        let mock = Mock::new(|_| Response::json(200, json!({"dry_run":true})));
        let c = Connection::new(&mock.config()).unwrap();
        for task in [Task::Lint, Task::Curator, Task::Sweep, Task::Embeddings] {
            action(Intent::Maintain {
                task,
                reembed: true,
            })
            .preview(&c, || false)
            .unwrap();
        }
        let requests = mock.requests();
        assert_eq!(requests.len(), 4);
        for request in &requests {
            assert_eq!(request.method, "POST");
            assert_eq!(request.body["dry_run"], true);
            assert_eq!(request.body["workspace"], "synthetic team");
            assert_eq!(request.body["project"], "demo");
        }
        assert_eq!(requests[0].body["no_llm"], true);
        assert_eq!(requests[1].body["stage"], false);
        assert_eq!(requests[3].body["all_projects"], false);
    }
    #[test]
    fn proposal_review_requires_pending_status_and_loads_scoped_diff() {
        let mock = Mock::new(|r| {
            if r.path.contains("/diff") {
                Response::json(
                    200,
                    json!({"diff":"before -> after","proposal_id":"proposal-1"}),
                )
            } else {
                Response::json(
                    200,
                    json!({"summary":{"id":"proposal-1","status":"pending"},"evidence_json":["synthetic"]}),
                )
            }
        });
        let a = action(Intent::Approve {
            id: "proposal-1".into(),
        });
        let c = Connection::new(&mock.config()).unwrap();
        let result = a.preview(&c, || false).unwrap();
        assert!(result["comparison"]["diff"].is_string());
        for r in mock.requests() {
            assert_eq!(r.method, "GET");
            assert!(r.path.contains("workspace=synthetic+team&project=demo"));
            assert!(!r.path.contains("approve"));
        }
        a.execute(&c, || false).unwrap();
        assert!(
            mock.requests()
                .last()
                .unwrap()
                .path
                .contains("/approve?workspace=")
        );
        let decided = Mock::new(|_| {
            Response::json(
                200,
                json!({"summary":{"status":"approved","id":"proposal-1"}}),
            )
        });
        assert_eq!(
            a.preview(&Connection::new(&decided.config()).unwrap(), || false),
            Err(Error::Conflict)
        );
        assert_eq!(decided.requests().len(), 1);
    }
    #[test]
    fn expired_and_previous_connection_receipts_never_dispatch() {
        let mock = Mock::new(|_| Response::json(200, json!({})));
        let shared = Shared::unstarted(mock.config());
        let worker = Worker::new(shared.clone());
        let a = action(Intent::ExpireHandoffs { days: 7 });
        let receipt = Receipt {
            action: a,
            generation: 0,
            at: Instant::now() - Duration::from_secs(301),
        };
        assert!(
            worker
                .submit(1, &shared, Work::Apply(receipt.clone()))
                .is_err()
        );
        shared.apply(mock.config());
        let receipt = Receipt {
            at: Instant::now(),
            ..receipt
        };
        assert_eq!(
            worker.submit(2, &shared, Work::Apply(receipt)),
            Err(Error::Cancelled)
        );
        assert!(mock.requests().is_empty());
    }
    #[test]
    fn failed_mutation_is_sent_once_and_not_retried() {
        let mock = Mock::new(|_| Response::json(503, json!({"error":"synthetic-private"})));
        let shared = Shared::unstarted(mock.config());
        let worker = Worker::new(shared.clone());
        worker
            .submit(
                1,
                &shared,
                Work::Apply(Receipt {
                    action: action(Intent::ExpireHandoffs { days: 14 }),
                    generation: 0,
                    at: Instant::now(),
                }),
            )
            .unwrap();
        let reply = worker.replies.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(reply.applying);
        assert_eq!(reply.result, Err(Error::Http(503)));
        assert!(reply.receipt.is_none());
        let r = mock.requests();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].body["confirm"], true);
        assert_eq!(r[0].body["older_than_days"], 14);
    }
    #[test]
    fn private_download_refuses_overwrite_and_removes_failed_partial_file() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "ai-monitor-backup-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("private.tar.gz");
        let mock = Mock::new(|_| Response {
            status: 200,
            body: vec![0x1f, 0x8b, 0, 1],
            headers: vec![],
        });
        let c = Connection::new(&mock.config()).unwrap();
        let a = action(Intent::Download {
            destination: path.clone(),
            okf: false,
        });
        a.preview(&c, || false).unwrap();
        assert!(mock.requests().is_empty());
        a.execute(&c, || false).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(a.execute(&c, || false).is_err());
        assert_eq!(mock.requests().len(), 1);
        let bad = Mock::new(|_| Response::json(200, json!({"not":"archive"})));
        let partial = root.join("partial.tar.gz");
        assert_eq!(
            action(Intent::Download {
                destination: partial.clone(),
                okf: true
            })
            .execute(&Connection::new(&bad.config()).unwrap(), || false),
            Err(Error::Format)
        );
        assert!(!partial.exists());
        assert_eq!(std::fs::read(&path).unwrap(), [0x1f, 0x8b, 0, 1]);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn restore_has_one_source_and_requires_explicit_session_id() {
        let mock = Mock::new(|_| Response::json(200, json!({"page_id":"restored"})));
        let c = Connection::new(&mock.config()).unwrap();
        action(Intent::Restore {
            path: "notes/a.md".into(),
            version: "v1".into(),
            git: false,
        })
        .execute(&c, || false)
        .unwrap();
        let body = &mock.requests()[0].body;
        assert_eq!(body["page_id"], "v1");
        assert!(body.get("rev").is_none());
        assert!(
            action(Intent::Consolidate {
                session: " ".into()
            })
            .preview(&c, || false)
            .is_err()
        );
        assert_eq!(mock.requests().len(), 1);
    }
    #[test]
    fn restore_preview_compares_current_version_to_requested_version() {
        let mock = Mock::new(|r| {
            if r.path.contains("/history/") {
                Response::json(
                    200,
                    json!({"workspace":"synthetic team","project":"demo","path":"notes/a.md","total":2,"versions":[],"diff":{}}),
                )
            } else {
                Response::json(
                    200,
                    json!({"workspace":"synthetic team","project":"demo","path":"notes/a.md","id":"current-v2","body_markdown":"current","links":[],"backlinks":[]}),
                )
            }
        });
        action(Intent::Restore {
            path: "notes/a.md".into(),
            version: "old-v1".into(),
            git: false,
        })
        .preview(&Connection::new(&mock.config()).unwrap(), || false)
        .unwrap();
        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].path.contains("from=current-v2&to=old-v1"));
        assert!(requests.iter().all(|r| r.method == "GET"));
    }
}
