//! Fixed administrative catalog, verified against ai-memory v3.3.1.
//! No arbitrary route, command or JSON editor is exposed by the UI.
use crate::client::{Connection, Error};
use reqwest::Method;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Projects,
    RenameProject,
    MoveProject,
    MergeProject,
    PurgeProject,
    RenameWorkspace,
    MergeWorkspace,
    DeleteWorkspace,
    MoveSession,
    ProfileStatus,
    ProfileList,
    ProfileReview,
    ProfileApply,
    ProfileRebuild,
    Users,
    CreateUser,
    EditUser,
    DisableUser,
    EnableUser,
    ResetPassword,
    Grants,
    Grant,
    RevokeGrant,
    ProjectGrants,
    ProjectAccess,
    Keys,
    CreateKey,
    RotateKey,
    RevokeKey,
    Inbox,
    Outbox,
    SendMessage,
    CancelMessage,
    Workstreams,
    Run,
    Events,
}
use Kind::*;
pub const ALL: &[Kind] = &[
    Projects,
    RenameProject,
    MoveProject,
    MergeProject,
    PurgeProject,
    RenameWorkspace,
    MergeWorkspace,
    DeleteWorkspace,
    MoveSession,
    ProfileStatus,
    ProfileList,
    ProfileReview,
    ProfileApply,
    ProfileRebuild,
    Users,
    CreateUser,
    EditUser,
    DisableUser,
    EnableUser,
    ResetPassword,
    Grants,
    Grant,
    RevokeGrant,
    ProjectGrants,
    ProjectAccess,
    Keys,
    CreateKey,
    RotateKey,
    RevokeKey,
    Inbox,
    Outbox,
    SendMessage,
    CancelMessage,
    Workstreams,
    Run,
    Events,
];
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub optional: bool,
    pub choices: &'static [&'static str],
}
fn f(key: &'static str, label: &'static str) -> Field {
    Field {
        key,
        label,
        optional: false,
        choices: &[],
    }
}
fn optional(key: &'static str, label: &'static str) -> Field {
    Field {
        optional: true,
        ..f(key, label)
    }
}
fn choice(key: &'static str, label: &'static str, choices: &'static [&'static str]) -> Field {
    Field {
        choices,
        ..f(key, label)
    }
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Projects => "Projetos · inventário",
            RenameProject => "Projetos · renomear",
            MoveProject => "Projetos · mover entre workspaces",
            MergeProject => "Projetos · mesclar",
            PurgeProject => "Projetos · excluir",
            RenameWorkspace => "Workspaces · renomear",
            MergeWorkspace => "Workspaces · mesclar",
            DeleteWorkspace => "Workspaces · excluir",
            MoveSession => "Sessões · mover uma sessão",
            ProfileStatus => "Perfil · estado",
            ProfileList => "Perfil · entradas",
            ProfileReview => "Perfil · revisar",
            ProfileApply => "Perfil · instruções geradas",
            ProfileRebuild => "Perfil · reconstruir",
            Users => "Usuários · listar",
            CreateUser => "Usuários · criar",
            EditUser => "Usuários · editar",
            DisableUser => "Usuários · desabilitar login",
            EnableUser => "Usuários · habilitar login",
            ResetPassword => "Usuários · redefinir senha",
            Grants => "Permissões · grants do usuário",
            Grant => "Permissões · conceder",
            RevokeGrant => "Permissões · revogar",
            ProjectGrants => "Permissões · acesso ao projeto",
            ProjectAccess => "Permissões · alterar acesso ao projeto",
            Keys => "Chaves · listar",
            CreateKey => "Chaves · emitir",
            RotateKey => "Chaves · rotacionar",
            RevokeKey => "Chaves · revogar",
            Inbox => "Mensagens · recebidas",
            Outbox => "Mensagens · enviadas",
            SendMessage => "Mensagens · enviar",
            CancelMessage => "Mensagens · cancelar por ID",
            Workstreams => "Workstreams · recentes no checkout",
            Run => "Workstreams · estado da execução",
            Events => "Workstreams · eventos",
        }
    }
    pub fn read(self) -> bool {
        matches!(
            self,
            Projects
                | ProfileStatus
                | ProfileList
                | ProfileReview
                | ProfileApply
                | Users
                | Grants
                | ProjectGrants
                | Keys
                | Inbox
                | Outbox
                | Workstreams
                | Run
                | Events
        )
    }
    pub fn fields(self) -> Vec<Field> {
        let scope = || vec![f("workspace", "Workspace"), f("project", "Projeto")];
        let user = || vec![f("username", "Usuário")];
        match self {
            Projects | Users | Keys | ProfileRebuild => vec![],
            RenameProject | MergeProject => {
                let mut v = scope();
                v.push(f("to", "Projeto de destino / novo nome"));
                v
            }
            MoveProject => {
                let mut v = scope();
                v.extend([
                    f("to_workspace", "Workspace de destino"),
                    choice(
                        "allow_lossy_merge",
                        "Permitir mesclar em destino existente, perdendo histórico?",
                        &["false", "true"],
                    ),
                ]);
                v
            }
            PurgeProject | ProjectGrants | Inbox | Outbox => scope(),
            RenameWorkspace | MergeWorkspace => vec![
                f("workspace", "Workspace de origem"),
                f("to", "Workspace de destino / novo nome"),
            ],
            DeleteWorkspace => vec![f("workspace", "Workspace a excluir com todo o conteúdo")],
            MoveSession => vec![
                f("session_id", "ID exato da sessão"),
                f("workspace", "Workspace de destino"),
                f("project", "Projeto de destino existente"),
                choice("pages", "Páginas da sessão", &["move", "regenerate"]),
            ],
            ProfileStatus | ProfileList | ProfileReview | ProfileApply => vec![
                f("workspace", "Workspace do perfil"),
                optional("user", "Usuário do perfil privado (opcional)"),
            ],
            CreateUser | EditUser => vec![
                f("username", "Usuário"),
                optional("name", "Nome (vazio: não alterar)"),
                optional("email", "E-mail (vazio: não alterar)"),
                choice("role", "Papel humano", &["user", "root"]),
            ],
            DisableUser | EnableUser | ResetPassword | Grants => user(),
            Grant | RevokeGrant => {
                let mut v = user();
                v.extend(scope());
                if self == Grant {
                    v.push(choice("level", "Permissão", &["read", "write"]));
                }
                v
            }
            ProjectAccess => {
                let mut v = scope();
                v.push(choice("mode", "Acesso", &["restricted", "open"]));
                v
            }
            CreateKey => vec![
                f("username", "Usuário proprietário"),
                f("label", "Identificação da chave"),
            ],
            RotateKey | RevokeKey => vec![f("id", "ID exato da credencial")],
            SendMessage => {
                let mut v = scope();
                v.extend([
                    f("to_workspace", "Workspace destinatário"),
                    f("to_project", "Projeto destinatário"),
                    optional("subject", "Assunto (até 200 caracteres)"),
                    choice(
                        "nature",
                        "Natureza",
                        &["unknown", "measured", "reported", "inferred"],
                    ),
                    optional(
                        "evidence",
                        "Evidência (obrigatória para measured; até 1000 caracteres)",
                    ),
                    f("body", "Mensagem (até 8000 caracteres)"),
                ]);
                v
            }
            CancelMessage => {
                let mut v = scope();
                v.push(f("message_id", "ID exato da mensagem enviada"));
                v
            }
            Workstreams => {
                let mut v = scope();
                v.extend([
                    f("repo_fingerprint", "Fingerprint do repositório"),
                    f("worktree_fingerprint", "Fingerprint do checkout"),
                    f("offset", "Deslocamento (50 por página; 0, 50, 100…)"),
                ]);
                v
            }
            Run => vec![f("run_id", "ID exato da execução")],
            Events => vec![
                f("workstream_id", "ID exato do workstream"),
                optional("q", "Busca nos eventos (até 100 resultados)"),
            ],
        }
    }
    pub fn warning(self) -> &'static str {
        match self {
            MoveProject | MergeWorkspace => {
                "Mover para um destino existente pode perder histórico no processo de copiar e excluir. Mesclar workspace pode aplicar parte dos projetos antes de falhar. Conflitos de páginas bloqueiam a operação; leases ativos não são forçados. Faça backup antes. A prévia por inventário não é uma simulação transacional."
            }
            PurgeProject | DeleteWorkspace => {
                "Exclusão irreversível de dados e vínculos, inclusive referências cruzadas. Backups e histórico Git podem preservar conteúdo; não equivale a apagar fisicamente todos os vestígios. Faça backup antes."
            }
            MergeProject => {
                "Mescla histórico e vínculos e remove a origem. Revise conflitos, backup e sessões ativas antes de confirmar."
            }
            DisableUser => {
                "Desabilita o login humano. Chaves de máquina do usuário continuam válidas; revogue-as separadamente quando necessário."
            }
            EditUser | ProjectAccess | Grant | RevokeGrant => {
                "Altera permissões. Root concede administração pela sessão humana. Modo open pode ampliar acesso; restricted pode retirar acesso. Revise os grants atuais."
            }
            CreateKey | RotateKey | ResetPassword | CreateUser => {
                "O segredo será mostrado uma única vez pelo servidor. O resultado fica apenas em memória, oculto até revelar. Rotação e redefinição invalidam credenciais anteriores conforme as regras do servidor. Guarde a nova credencial antes de sair."
            }
            ProfileRebuild => {
                "Reconstrói o perfil segundo a configuração efetiva do servidor; pode abranger toda a instalação e consumir provider. Não altera o modo de compartilhamento configurado."
            }
            ProfileApply => {
                "Consulta as instruções geradas pelo perfil. Não grava arquivos de instruções nos seus projetos."
            }
            SendMessage => {
                "Enfileira uma mensagem para o projeto destinatário. Isso não inicia um agente nem comprova que a tarefa foi concluída."
            }
            CancelMessage => {
                "Cancela somente o ID informado. A lista de mensagens nunca consome a inbox."
            }
            Workstreams | Run | Events => {
                "Consulta exige Bearer. Não renova leases, não aceita contexto e não marca mensagens como lidas. Recentes exige os fingerprints reais do checkout; eventos retornam até 100 itens por busca."
            }
            MoveSession => {
                "Move a sessão exata. Não cria projeto de destino nem força sessão ativa. A opção regenerate pode exigir reconsolidação posterior."
            }
            _ => {
                "A prévia mostra o estado disponível e os parâmetros exatos. Não bloqueia alterações concorrentes; a API revalida permissões e conflitos ao executar."
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub kind: Kind,
    pub values: BTreeMap<String, String>,
}
impl Operation {
    pub fn new(kind: Kind) -> Self {
        let mut values = BTreeMap::new();
        for f in kind.fields() {
            values.insert(
                f.key.into(),
                f.choices
                    .first()
                    .copied()
                    .unwrap_or(if f.key == "offset" { "0" } else { "" })
                    .into(),
            );
        }
        Self { kind, values }
    }
    fn s(&self, k: &str) -> &str {
        self.values.get(k).map(|s| s.trim()).unwrap_or("")
    }
    pub fn validate(&self) -> Result<(), Error> {
        for f in self.kind.fields() {
            let s = self.s(f.key);
            let limit = match f.key {
                "body" => 8000,
                "evidence" => 1000,
                "subject" => 200,
                _ => 512,
            };
            if (!f.optional && s.is_empty())
                || s.chars().count() > limit
                || s.chars()
                    .any(|c| c.is_control() && !(f.key == "body" && matches!(c, '\n' | '\t')))
                || (!f.choices.is_empty() && !f.choices.contains(&s))
                || matches!(s, "." | "..")
            {
                return Err(Error::Invalid(format!("Campo inválido: {}.", f.label)));
            }
        }
        if self.kind == Workstreams && self.s("offset").parse::<usize>().is_err() {
            return Err(Error::Invalid(
                "Deslocamento deve ser um inteiro não negativo.".into(),
            ));
        }
        if self.kind == SendMessage
            && self.s("nature") == "measured"
            && self.s("evidence").is_empty()
        {
            return Err(Error::Invalid(
                "Mensagens measured exigem evidência.".into(),
            ));
        }
        if matches!(self.kind, RenameProject | MergeProject) && self.s("project") == self.s("to")
            || matches!(self.kind, RenameWorkspace | MergeWorkspace)
                && self.s("workspace") == self.s("to")
            || self.kind == MoveProject && self.s("workspace") == self.s("to_workspace")
        {
            return Err(Error::Invalid(
                "Origem e destino devem ser diferentes.".into(),
            ));
        }
        Ok(())
    }
    pub fn confirmation(&self) -> String {
        let target = [
            "message_id",
            "session_id",
            "id",
            "username",
            "project",
            "workspace",
        ]
        .iter()
        .map(|k| self.s(k))
        .find(|v| !v.is_empty())
        .unwrap_or("GLOBAL");
        format!("CONFIRMAR {target}")
    }
    fn scope(&self) -> Value {
        json!({"workspace":self.s("workspace"),"project":self.s("project")})
    }
    fn query(&self, keys: &[&'static str]) -> Vec<(&'static str, String)> {
        keys.iter()
            .filter(|k| !self.s(k).is_empty())
            .map(|k| (*k, self.s(k).into()))
            .collect()
    }
    pub fn execute(&self, c: &Connection, preview: bool) -> Result<Value, Error> {
        self.validate()?;
        let k = self.kind;
        if preview && !k.read() {
            if matches!(k, MergeProject | MoveSession | PurgeProject) {
                let v = self.send(c, true)?;
                return Ok(
                    json!({"preview_kind":"Simulação do servidor","request":self.body(false),"report":v}),
                );
            }
            // delete-workspace's old servers ignore dry_run and can delete empty workspaces.
            // Never POST it during preview; use GET inventory on every server version.
            let state = match k {
                CreateUser | EditUser | EnableUser | DisableUser | ResetPassword | CreateKey => {
                    c.get::<Value>(&["admin", "users"], &[])?
                }
                RotateKey | RevokeKey => c.get(&["admin", "api-credentials"], &[])?,
                Grant | RevokeGrant => {
                    c.get(&["admin", "users", self.s("username"), "grants"], &[])?
                }
                ProjectAccess => c.get(
                    &["admin", "projects", "grants"],
                    &self.query(&["workspace", "project"]),
                )?,
                ProfileRebuild => c.get(&["admin", "profile", "status"], &[])?,
                CancelMessage => {
                    let mut q = self.query(&["workspace", "project"]);
                    q.extend([("box", "outbox".into()), ("limit", "200".into())]);
                    c.get(&["admin", "messages"], &q)?
                }
                _ => c.get(&["api", "v1", "projects"], &[])?,
            };
            return Ok(
                json!({"preview_kind":"Consulta de estado; sem simulação transacional", "warning":k.warning(),"request":self.body(false),"current_state":state}),
            );
        }
        if !k.read() {
            return self.send(c, false);
        }
        let q = self.query(&["workspace", "project"]);
        match k {
            Projects=>c.get(&["api","v1","projects"],&[]),
            Users=>c.get(&["admin","users"],&[]),
            Keys=>c.get(&["admin","api-credentials"],&[]),
            Grants=>c.get(&["admin","users",self.s("username"),"grants"],&[]),
            ProjectGrants=>c.get(&["admin","projects","grants"],&q),
            ProfileStatus|ProfileList|ProfileReview|ProfileApply=>c.get(&["admin","profile",match k{ProfileStatus=>"status",ProfileList=>"list",ProfileReview=>"review",_=>"apply"}],&self.query(&["workspace","user"])),
            Inbox|Outbox=>{let mut q=q;q.extend([("box",if k==Inbox{"inbox"}else{"outbox"}.into()),("limit","200".into())]);c.get(&["admin","messages"],&q)},
            Workstreams=>c.post(&["workstream","recent"],&[],&json!({"workspace":self.s("workspace"),"project":self.s("project"),"repo_fingerprint":self.s("repo_fingerprint"),"worktree_fingerprint":self.s("worktree_fingerprint"),"limit":50,"offset":self.s("offset").parse::<usize>().map_err(|_|Error::Format)?})),
            Run=>c.get(&["workstream","runs",self.s("run_id")],&[]),
            Events=>{let mut q=self.query(&["q"]);q.push(("limit","100".into()));c.get(&["workstream",self.s("workstream_id"),"events"],&q)},
            _=>Err(Error::Invalid("Operação inválida.".into())),
        }
    }
    fn body(&self, preview: bool) -> Value {
        let mut b = self.scope();
        match self.kind {
            RenameProject => {
                json!({"workspace":self.s("workspace"),"from":self.s("project"),"to":self.s("to")})
            }
            MergeProject => {
                json!({"workspace":self.s("workspace"),"from":self.s("project"),"into":self.s("to"),"confirm":!preview})
            }
            MoveProject => {
                json!({"from_workspace":self.s("workspace"),"project":self.s("project"),"to_workspace":self.s("to_workspace"),"confirm":true,"force":false,"allow_lossy_merge":self.s("allow_lossy_merge")=="true","on_conflict":"block"})
            }
            RenameWorkspace | MergeWorkspace => {
                json!({"from":self.s("workspace"),"to":self.s("to"),"confirm":true,"force":false,"on_conflict":"block"})
            }
            DeleteWorkspace => {
                json!({"workspace":self.s("workspace"),"dry_run":false,"force":true,"compact":false})
            }
            PurgeProject => {
                b["confirm"] = json!(!preview);
                b["dry_run"] = json!(preview);
                b["force"] = json!(false);
                b["compact"] = json!(false);
                b
            }
            MoveSession => {
                b["session_id"] = json!(self.s("session_id"));
                b["pages"] = json!(self.s("pages"));
                b["confirm"] = json!(!preview);
                b["force"] = json!(false);
                b["create"] = json!(false);
                b
            }
            CreateUser | EditUser => {
                let mut b = json!({"role":self.s("role")});
                if self.kind == CreateUser {
                    b["username"] = json!(self.s("username"));
                }
                for k in ["name", "email"] {
                    if !self.s(k).is_empty() {
                        b[k] = json!(self.s(k));
                    }
                }
                b
            }
            Grant => {
                b["level"] = json!(self.s("level"));
                b
            }
            RevokeGrant => b,
            ProjectAccess => {
                b["mode"] = json!(self.s("mode"));
                b
            }
            CreateKey => json!({"username":self.s("username"),"label":self.s("label")}),
            SendMessage => {
                let mut b = json!({"from_workspace":self.s("workspace"),"from_project":self.s("project"),"to_workspace":self.s("to_workspace"),"to_project":self.s("to_project"),"nature":self.s("nature"),"body":self.s("body")});
                for k in ["subject", "evidence"] {
                    if !self.s(k).is_empty() {
                        b[k] = json!(self.s(k));
                    }
                }
                b
            }
            CancelMessage => {
                b["message_id"] = json!(self.s("message_id"));
                b
            }
            _ => json!({}),
        }
    }
    fn send(&self, c: &Connection, preview: bool) -> Result<Value, Error> {
        let mut method = Method::POST;
        let route: Vec<&str> = match self.kind {
            RenameProject => vec!["rename-project"],
            MoveProject => vec!["move-project"],
            MergeProject => vec!["merge-project"],
            PurgeProject => vec!["purge-project"],
            RenameWorkspace => vec!["rename-workspace"],
            MergeWorkspace => vec!["merge-workspace"],
            DeleteWorkspace => vec!["delete-workspace"],
            MoveSession => vec!["move-session"],
            ProfileRebuild => vec!["profile", "rebuild"],
            CreateUser => vec!["human-users"],
            EditUser => {
                method = Method::PATCH;
                vec!["users", self.s("username")]
            }
            DisableUser | EnableUser | ResetPassword | Grant | RevokeGrant => vec![
                "users",
                self.s("username"),
                match self.kind {
                    DisableUser => "disable",
                    EnableUser => "enable",
                    ResetPassword => "reset-password",
                    Grant => "grant",
                    _ => "revoke",
                },
            ],
            ProjectAccess => vec!["projects", "access"],
            CreateKey => vec!["api-credentials"],
            RotateKey | RevokeKey => vec![
                "api-credentials",
                self.s("id"),
                if self.kind == RotateKey {
                    "rotate"
                } else {
                    "revoke"
                },
            ],
            SendMessage | CancelMessage => vec![
                "messages",
                if self.kind == SendMessage {
                    "send"
                } else {
                    "cancel"
                },
            ],
            _ => return Err(Error::Invalid("Operação inválida.".into())),
        };
        let mut path = vec!["admin"];
        path.extend(route);
        c.json(method, &path, &[], &self.body(preview))
    }
}
/// Hide generated credentials recursively. Never persist administrative results.
pub fn concealed(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let lower = k.to_ascii_lowercase();
                    let secret_key = (lower.contains("password")
                        || (lower.contains("token") && !lower.ends_with("_tokens"))
                        || lower.contains("secret")
                        || lower.contains("csrf")
                        || lower.contains("cookie")
                        || matches!(lower.as_str(), "api_key" | "raw_key" | "private_key"))
                        && !v.is_null()
                        && !v.is_boolean()
                        && !v.is_number();
                    (
                        k.clone(),
                        if secret_key {
                            json!("[oculto]")
                        } else {
                            concealed(v)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(rows) => Value::Array(rows.iter().map(concealed).collect()),
        Value::String(s) if s.starts_with("aim_") => json!("[oculto]"),
        _ => value.clone(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Mock, Response};
    fn op(kind: Kind) -> Operation {
        let mut op = Operation::new(kind);
        for f in kind.fields() {
            if f.choices.is_empty() && f.key != "offset" {
                op.values
                    .insert(f.key.into(), format!("synthetic-{}", f.key));
            }
        }
        op
    }
    #[test]
    fn every_catalog_entry_validates_and_preview_never_applies() {
        for &kind in ALL {
            let op = op(kind);
            op.validate().unwrap();
            let mock = Mock::new(|_| Response::json(200, json!({})));
            op.execute(&Connection::new(&mock.config()).unwrap(), true)
                .unwrap();
            for r in mock.requests() {
                if r.method != "GET" {
                    assert_eq!(r.method, "POST");
                    if kind == Workstreams {
                        assert_eq!(r.path, "/workstream/recent");
                    } else {
                        assert!([MergeProject, MoveSession, PurgeProject].contains(&kind));
                        assert_eq!(r.body["confirm"], false);
                    }
                }
            }
        }
    }
    #[test]
    fn old_workspace_delete_preview_never_posts_even_for_empty_inventory() {
        let mock = Mock::new(|_| Response::json(200, json!([])));
        op(DeleteWorkspace)
            .execute(&Connection::new(&mock.config()).unwrap(), true)
            .unwrap();
        assert!(mock.requests().iter().all(|r| r.method == "GET"));
    }
    #[test]
    fn cancel_requires_one_exact_id_and_reads_never_consume() {
        let mock = Mock::new(|_| Response::json(200, json!({})));
        let c = Connection::new(&mock.config()).unwrap();
        let mut cancel = op(CancelMessage);
        cancel.values.insert("message_id".into(), " ".into());
        assert!(cancel.execute(&c, false).is_err());
        assert!(mock.requests().is_empty());
        op(Inbox).execute(&c, false).unwrap();
        assert_eq!(mock.requests()[0].method, "GET");
        assert!(mock.requests()[0].path.contains("box=inbox"));
    }
    #[test]
    fn mutation_contracts_and_secret_masking() {
        let mock = Mock::new(|_| Response::json(200, json!({})));
        let c = Connection::new(&mock.config()).unwrap();
        op(EditUser).execute(&c, false).unwrap();
        op(MoveSession).execute(&c, false).unwrap();
        op(PurgeProject).execute(&c, false).unwrap();
        let r = mock.requests();
        assert_eq!(r[0].method, "PATCH");
        assert_eq!(r[1].body["confirm"], true);
        assert_eq!(r[1].body["force"], false);
        assert_eq!(r[1].body["create"], false);
        assert_eq!(r[2].body["dry_run"], false);
        let v = concealed(
            &json!({"token":"private","nested":[{"temporary_password":"private"}],"id":"public"}),
        );
        assert!(!v.to_string().contains("private"));
        assert_eq!(v["id"], "public");
    }
    #[test]
    fn measured_message_needs_evidence_and_self_move_is_refused() {
        let mut o = op(SendMessage);
        o.values.insert("nature".into(), "measured".into());
        o.values.insert("evidence".into(), String::new());
        assert!(o.validate().is_err());
        let mut o = op(MoveProject);
        o.values
            .insert("to_workspace".into(), o.s("workspace").into());
        assert!(o.validate().is_err());
    }

    #[test]
    fn concealed_masks_compound_secret_keys_and_aim_prefixed_values_and_omits_empty_optionals() {
        let masked = concealed(&json!({
            "api_token": "secret-1",
            "raw_token": "secret-2",
            "secret_key": "secret-3",
            "api_key": "secret-4",
            "credential": "aim_live_secret_value",
            "input_tokens": 42,
            "id": "public-id"
        }));
        let s = masked.to_string();
        for leaked in [
            "secret-1",
            "secret-2",
            "secret-3",
            "secret-4",
            "aim_live_secret_value",
        ] {
            assert!(
                !s.contains(leaked),
                "leaked secret in concealed(): {leaked}"
            );
        }
        assert_eq!(masked["input_tokens"], 42);
        assert_eq!(masked["id"], "public-id");

        let mut msg = Operation::new(SendMessage);
        msg.values.insert("workspace".into(), "w1".into());
        msg.values.insert("project".into(), "p1".into());
        msg.values.insert("to_workspace".into(), "w2".into());
        msg.values.insert("to_project".into(), "p2".into());
        msg.values.insert("body".into(), "hello".into());
        let body = msg.body(false);
        assert!(body.get("subject").is_none());
        assert!(body.get("evidence").is_none());
    }
}
