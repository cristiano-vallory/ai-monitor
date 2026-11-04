use crate::{
    advanced::{self, Kind, Operation},
    client::Scope,
    confirmation::Confirmation,
    local_service::{self, Backend, Kind as LocalKind},
    maintenance::{Action, Intent, PREVIEW_TTL, Read, Receipt, Work, Worker},
    model::Project,
    monitor::Shared,
    theme,
    widgets::{self, Answer, Button, Segment, Select, Tone, Type},
};
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

/// A legenda das credenciais, no pé da página.
const LEGEND: &str = "Operações sob demanda. Credenciais geradas e logs ficam somente na memória e são removidos da tela após 5 minutos ou ao limpar/trocar de operação.";

/// Os grupos do catálogo, na ordem do seletor: o prefixo de `Kind::label()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Projects,
    Workspaces,
    Sessions,
    Profile,
    Users,
    Permissions,
    Keys,
    Messages,
    Workstreams,
}
const GROUPS: [Group; 9] = [
    Group::Projects,
    Group::Workspaces,
    Group::Sessions,
    Group::Profile,
    Group::Users,
    Group::Permissions,
    Group::Keys,
    Group::Messages,
    Group::Workstreams,
];
impl Group {
    fn label(self) -> &'static str {
        match self {
            Self::Projects => "Projetos",
            Self::Workspaces => "Workspaces",
            Self::Sessions => "Sessões",
            Self::Profile => "Perfil",
            Self::Users => "Usuários",
            Self::Permissions => "Permissões",
            Self::Keys => "Chaves",
            Self::Messages => "Mensagens",
            Self::Workstreams => "Workstreams",
        }
    }
    /// O grupo de cada operação. Sem curinga: uma operação nova não compila
    /// sem grupo.
    fn of(kind: Kind) -> Self {
        match kind {
            Kind::Projects
            | Kind::RenameProject
            | Kind::MoveProject
            | Kind::MergeProject
            | Kind::PurgeProject => Self::Projects,
            Kind::RenameWorkspace | Kind::MergeWorkspace | Kind::DeleteWorkspace => {
                Self::Workspaces
            }
            Kind::MoveSession => Self::Sessions,
            Kind::ProfileStatus
            | Kind::ProfileList
            | Kind::ProfileReview
            | Kind::ProfileApply
            | Kind::ProfileRebuild => Self::Profile,
            Kind::Users
            | Kind::CreateUser
            | Kind::EditUser
            | Kind::DisableUser
            | Kind::EnableUser
            | Kind::ResetPassword => Self::Users,
            Kind::Grants
            | Kind::Grant
            | Kind::RevokeGrant
            | Kind::ProjectGrants
            | Kind::ProjectAccess => Self::Permissions,
            Kind::Keys | Kind::CreateKey | Kind::RotateKey | Kind::RevokeKey => Self::Keys,
            Kind::Inbox | Kind::Outbox | Kind::SendMessage | Kind::CancelMessage => Self::Messages,
            Kind::Workstreams | Kind::Run | Kind::Events => Self::Workstreams,
        }
    }
    /// As operações do grupo, na ordem de `advanced::ALL`.
    fn kinds(self) -> impl Iterator<Item = Kind> {
        advanced::ALL
            .iter()
            .copied()
            .filter(move |&kind| Self::of(kind) == self)
    }
}
/// O nome da operação no grupo: o que vem depois de " · " em `label()`.
fn name(kind: Kind) -> &'static str {
    let label = kind.label();
    label.split_once(" · ").map_or(label, |(_, name)| name)
}
/// As operações do serviço local, na ordem do seletor.
const LOCAL: [LocalKind; 7] = [
    LocalKind::Status,
    LocalKind::Logs,
    LocalKind::Start,
    LocalKind::Stop,
    LocalKind::Restart,
    LocalKind::Update,
    LocalKind::Recovery,
];

pub struct Panel {
    worker: Worker,
    pub busy: bool,
    ticket: u64,
    op: Operation,
    local: local_service::Operation,
    result: Option<Value>,
    receipt: Option<Receipt>,
    error: Option<String>,
    notice: String,
    confirm: String,
    reveal: bool,
    result_at: Option<Instant>,
    local_tab: bool,
}
impl Panel {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            worker: Worker::new(shared),
            busy: false,
            ticket: 0,
            op: Operation::new(advanced::Kind::Projects),
            local: local_service::Operation::new(Backend::Docker, LocalKind::Status),
            result: None,
            receipt: None,
            error: None,
            notice: String::new(),
            confirm: String::new(),
            reveal: false,
            result_at: None,
            local_tab: false,
        }
    }
    fn clear(&mut self) {
        self.result = None;
        self.receipt = None;
        self.error = None;
        self.notice.clear();
        self.confirm.clear();
        self.reveal = false;
        self.result_at = None;
    }
    pub fn reset(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.clear();
        self.op = Operation::new(advanced::Kind::Projects);
        self.local = local_service::Operation::new(Backend::Docker, LocalKind::Status);
    }
    fn submit(&mut self, shared: &Shared, work: Work) {
        if self.busy {
            return;
        }
        self.ticket = self.ticket.wrapping_add(1);
        self.clear();
        match self.worker.submit(self.ticket, shared, work) {
            Ok(()) => self.busy = true,
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub fn poll(&mut self, shared: &Shared) {
        while let Ok(reply) = self.worker.replies.try_recv() {
            self.busy = false;
            if reply.ticket != self.ticket
                || reply.generation != shared.generation.load(Ordering::Acquire)
            {
                continue;
            }
            match reply.result {
                Ok(value) => {
                    self.result = Some(value);
                    self.receipt = reply.receipt;
                    self.result_at = Some(Instant::now());
                    self.notice = if reply.applying {
                        "Resposta recebida. Consulte novamente o estado para verificar a operação."
                    } else {
                        "Consulta concluída. Revise a prévia e os parâmetros."
                    }
                    .into();
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    if reply.applying {
                        self.notice="Pode ter sido processado mesmo sem resposta. Verifique o estado antes de repetir. Nenhuma repetição automática.".into();
                    }
                }
            }
        }
        // A prévia vence pela idade do recibo, como a contagem da folha:
        // a folha fecha e o formulário destrava no mesmo frame.
        if self.receipt.as_ref().is_some_and(Receipt::expired) {
            self.clear();
            self.notice = "A prévia expirou após 5 minutos. Prepare a operação novamente.".into();
        } else if self.result_at.is_some_and(|at| at.elapsed() > PREVIEW_TTL) {
            self.clear();
            self.notice =
                "Resultado removido da tela após 5 minutos. Consulte novamente se necessário."
                    .into();
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, shared: &Arc<Shared>, projects: &[Project]) {
        self.poll(shared);
        let previous = self.local_tab;
        let local = format!("Serviço deste {}", crate::platform::HOST);
        widgets::toolbar(
            ui,
            |ui| {
                // Travado também sob a folha: o véu só engole cliques, e o
                // teclado (Tab e Enter) trocaria a aba, descartando a prévia.
                ui.add_enabled_ui(!self.busy && self.receipt.is_none(), |ui| {
                    widgets::segmented(
                        ui,
                        &mut self.local_tab,
                        &[
                            Segment {
                                value: false,
                                label: "Servidor · administração",
                                count: None,
                            },
                            Segment {
                                value: true,
                                label: &local,
                                count: None,
                            },
                        ],
                    );
                });
            },
            |_| {},
        );
        if previous != self.local_tab {
            self.clear();
        }
        ui.add_enabled_ui(!self.busy && self.receipt.is_none(), |ui| {
            widgets::card(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                if self.local_tab {
                    self.local_form(ui, shared);
                } else {
                    self.remote_form(ui, shared, projects);
                }
            });
        });
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    "Operação em andamento. Aguarde antes de trocar a conexão ou iniciar outra ação.",
                );
            });
        }
        if let Some(error) = &self.error {
            widgets::strip(ui, Tone::Bad, error, "", None);
        }
        if !self.notice.is_empty() {
            widgets::caption(ui, &self.notice);
        }
        if let Some(receipt) = self.receipt.clone() {
            let action = &receipt.action;
            let typed = match action.confirmation() {
                Some(Confirmation::Type(target)) => Some(target),
                // Leitura não gera recibo.
                Some(Confirmation::Click) | None => None,
            };
            let subtitle = if self.local_tab {
                local.clone()
            } else {
                "Servidor · administração".into()
            };
            // Leitura não gera recibo.
            let (title, verb) = action.question().unwrap_or_default();
            let result = &self.result;
            let answer = widgets::confirm_sheet(
                ui,
                &widgets::Confirm {
                    title: &title,
                    subtitle: &subtitle,
                    facts: &[
                        widgets::Fact::new("Operação", action.label()),
                        widgets::Fact::new("Alvo", action.target()),
                    ],
                    preview: "Prévia de impacto",
                    warning: action.warning(),
                    typed: typed.as_deref(),
                    verb,
                    left: receipt.left(),
                    ready: !self.busy,
                },
                &mut self.confirm,
                // Sem "Revelar credenciais" na folha: a prévia vai oculta.
                |ui| {
                    if let Some(value) = result {
                        crate::console::show_values(ui, &advanced::concealed(value), 0);
                    }
                },
            );
            match answer {
                Some(Answer::Confirm) => self.submit(shared, Work::Apply(receipt)),
                Some(Answer::Cancel) => self.clear(),
                None => {}
            }
        }
        // Com recibo, a prévia está na folha.
        if self.receipt.is_none()
            && let Some(value) = self.result.clone()
        {
            widgets::card(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(28.);
                    ui.label(RichText::new("Resultado").font(Type::Section.font()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let clear = Button::secondary("Limpar resultado privado").raised();
                        if ui.add(clear).clicked() {
                            self.clear();
                        }
                        widgets::switch(
                            ui,
                            &mut self.reveal,
                            "Revelar credenciais neste resultado",
                        );
                    });
                });
                if self.result.is_some() {
                    crate::console::show_values(
                        ui,
                        &if self.reveal {
                            value
                        } else {
                            advanced::concealed(&value)
                        },
                        0,
                    );
                }
            });
        }
        widgets::caption(ui, LEGEND);
    }
    fn remote_form(&mut self, ui: &mut egui::Ui, shared: &Shared, projects: &[Project]) {
        let old = self.op.kind;
        let mut kind = old;
        widgets::row(ui, |ui| {
            let group = Group::of(kind);
            let select = Select::new("remote-group", "Grupo", group.label())
                .min_width(150.)
                .raised();
            select.show(ui, |ui| {
                for other in GROUPS {
                    if widgets::option(ui, other == group, other.label()).clicked()
                        && other != group
                    {
                        // Outro grupo: a primeira operação dele.
                        kind = other.kinds().next().unwrap_or(kind);
                    }
                }
            });
            let select = Select::new("remote-operation", "Operação", name(kind))
                .min_width(240.)
                .raised();
            select.show(ui, |ui| {
                for each in Group::of(kind).kinds() {
                    if widgets::option(ui, each == kind, name(each)).clicked() {
                        kind = each;
                    }
                }
            });
        });
        if old != kind {
            self.clear();
            self.op = Operation::new(kind);
        }
        if self.op.values.contains_key("workspace") {
            let select = Select::new(
                "remote-project",
                "Projeto conhecido",
                "Preencher a partir de um projeto conhecido…",
            )
            .min_width(300.)
            .raised();
            select.show(ui, |ui| {
                for p in projects {
                    let label = format!("{} / {}", p.workspace_name, p.project_name);
                    if widgets::option(ui, false, &label).clicked() {
                        self.op
                            .values
                            .insert("workspace".into(), p.workspace_name.clone());
                        if self.op.values.contains_key("project") {
                            self.op
                                .values
                                .insert("project".into(), p.project_name.clone());
                        }
                    }
                }
            });
        }
        let width = ui.available_width().min(560.);
        for field in self.op.kind.fields() {
            ui.push_id(field.key, |ui| {
                ui.spacing_mut().item_spacing.y = widgets::space::XS;
                let label = ui.label(RichText::new(field.label).font(Type::Body.font()));
                let value = self.op.values.entry(field.key.into()).or_default();
                if !field.choices.is_empty() {
                    choice(ui, value, field.choices);
                } else if field.key == "body" {
                    text_area(ui, value, ui.available_width(), 8000).labelled_by(label.id);
                } else {
                    // Opcional como no Settings.dc.html: a dica do campo.
                    let hint = if field.optional { "Opcional" } else { "" };
                    widgets::text_field(ui, value, width, hint, false, 1024).labelled_by(label.id);
                }
            });
        }
        let read = self.op.kind.read();
        warning(ui, read, self.op.kind.warning());
        if ui.add(Button::primary(action(read))).clicked() {
            let op = self.op.clone();
            let work = if op.kind.read() {
                Work::Read(Read::Advanced(op))
            } else {
                Work::Preview(Action {
                    scope: Scope::default(),
                    intent: Intent::Advanced(op),
                })
            };
            self.submit(shared, work);
        }
    }
    fn local_form(&mut self, ui: &mut egui::Ui, shared: &Shared) {
        let old = (self.local.backend, self.local.kind);
        let mut backends = vec![Segment {
            value: Backend::Docker,
            label: "Docker",
            count: None,
        }];
        #[cfg(target_os = "macos")]
        backends.push(Segment {
            value: Backend::LaunchAgent,
            label: "LaunchAgent",
            count: None,
        });
        #[cfg(target_os = "linux")]
        backends.push(Segment {
            value: Backend::Systemd,
            label: "systemd (usuário)",
            count: None,
        });
        widgets::row(ui, |ui| {
            widgets::segmented(ui, &mut self.local.backend, &backends);
            let label = self.local.label();
            let (backend, kind) = (self.local.backend, &mut self.local.kind);
            let select = Select::new("local-operation", "Operação", label)
                .min_width(240.)
                .raised();
            select.show(ui, |ui| {
                for each in LOCAL {
                    let label = local_service::Operation::new(backend, each).label();
                    if widgets::option(ui, *kind == each, label).clicked() {
                        *kind = each;
                    }
                }
            });
        });
        if old != (self.local.backend, self.local.kind) {
            self.clear();
        }
        widgets::caption(ui, crate::platform::LOCAL_SERVICE_HINT);
        if self.local.kind == LocalKind::Update {
            widgets::caption(
                ui,
                "Backup é obrigatório; o app verifica arquivo gzip privado, sem garantir sua restaurabilidade. Docker usa a imagem definida no Compose. O modo nativo mantém cópia do binário anterior e permanece parado até iniciar explicitamente.",
            );
        }
        let width = ui.available_width().min(650.);
        for (key, label) in self.local.fields() {
            ui.push_id(key, |ui| {
                ui.spacing_mut().item_spacing.y = widgets::space::XS;
                let label = ui.label(RichText::new(label).font(Type::Body.font()));
                let value = self.local.values.entry(key.into()).or_default();
                widgets::text_field(ui, value, width, "", false, 4096).labelled_by(label.id);
            });
        }
        let read = self.local.read();
        warning(ui, read, local_service::WARNING);
        if ui.add(Button::primary(action(read))).clicked() {
            let op = self.local.clone();
            let work = if op.read() {
                Work::Read(Read::Local(op))
            } else {
                Work::Preview(Action {
                    scope: Scope::default(),
                    intent: Intent::Local(op),
                })
            };
            self.submit(shared, work);
        }
    }
}

/// O botão da operação: "Consultar" na leitura, "Prévia…" (abre a folha)
/// na escrita.
fn action(read: bool) -> &'static str {
    if read { "Consultar" } else { "Prévia…" }
}

/// O aviso da operação, logo antes do botão: Warn antes da prévia de uma
/// escrita; Info numa leitura, que não tem prévia.
fn warning(ui: &mut egui::Ui, read: bool, text: &str) {
    if read {
        widgets::strip(ui, Tone::Info, "Observação.", text, None);
    } else {
        widgets::strip(ui, Tone::Warn, "Atenção.", text, None);
    }
}

/// Um campo de opções fixas: um segmentado.
fn choice(ui: &mut egui::Ui, value: &mut String, choices: &'static [&'static str]) {
    let mut at = choices.iter().position(|c| *c == value.as_str());
    let items: Vec<Segment<'_, Option<usize>>> = choices
        .iter()
        .enumerate()
        .map(|(i, label)| Segment {
            value: Some(i),
            label,
            count: None,
        })
        .collect();
    if widgets::segmented(ui, &mut at, &items)
        && let Some(i) = at
    {
        *value = choices[i].into();
    }
}

/// O campo da mensagem: o `text_field` em 5 linhas (fundo field, raio 5,
/// texto 12, borda line, acc com foco).
fn text_area(ui: &mut egui::Ui, text: &mut String, width: f32, limit: usize) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(
        egui::TextEdit::multiline(text)
            .frame(false)
            .font(egui::FontId::proportional(12.))
            .margin(egui::Margin::symmetric(8, 6))
            .desired_width(width - 16.)
            .desired_rows(5)
            .char_limit(limit),
    );
    let border = if response.has_focus() { p.acc } else { p.line };
    ui.painter().set(
        background,
        egui::epaint::RectShape::new(
            response.rect,
            5.,
            p.field,
            egui::Stroke::new(1.0_f32, border),
            egui::StrokeKind::Inside,
        ),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_support::{Mock, Response},
        widgets::tests::{
            THEMES, click_sized, context, node, nodes, rect_of, rects, run_sized, tab_and_enter,
        },
    };
    use egui::accesskit::{Role, Toggled};
    use std::time::Duration;

    const WINDOW: [f32; 2] = [1320., 860.];
    /// O painel com a prévia de `op` e uma credencial na resposta, num
    /// servidor sintético.
    fn previewing(intent: Intent, local_tab: bool) -> (Mock, Arc<Shared>, Panel) {
        let mock = Mock::new(|_| Response::json(200, serde_json::json!({"ok": true})));
        let shared = Shared::unstarted(mock.config());
        let mut panel = Panel::new(shared.clone());
        panel.local_tab = local_tab;
        panel.receipt = Some(Receipt {
            action: Action {
                scope: Scope::default(),
                intent,
            },
            generation: 0,
            at: Instant::now(),
        });
        panel.result = Some(serde_json::json!({
            "project": "demo",
            "token": "synthetic-private-token",
        }));
        panel.result_at = Some(Instant::now());
        (mock, shared, panel)
    }
    fn purge() -> Intent {
        let mut op = Operation::new(advanced::Kind::PurgeProject);
        op.values.insert("workspace".into(), "fixture".into());
        op.values.insert("project".into(), "demo".into());
        Intent::Advanced(op)
    }
    /// Dois frames: o primeiro da folha só mede (sizing pass).
    fn frames(ctx: &egui::Context, add: &mut impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        run_sized(ctx, WINDOW, vec![], &mut *add);
        run_sized(ctx, WINDOW, vec![], &mut *add)
    }

    #[test]
    fn a_destructive_preview_asks_for_the_target_and_keeps_secrets_hidden() {
        for (typed, enabled) in [
            ("", false),
            ("dem", false),
            ("demo ", false),
            ("CONFIRMAR demo", false),
            ("demo", true),
        ] {
            let (mock, shared, mut panel) = previewing(purge(), false);
            panel.confirm = typed.into();
            let ctx = context(egui::Theme::Dark);
            let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
            let out = frames(&ctx, &mut add);
            for text in [
                "Excluir o projeto “demo”?",
                "Servidor · administração",
                "Operação: Projetos · excluir",
                "Alvo: demo",
                "Prévia de impacto",
                "Digite demo para confirmar",
                "project: demo",
            ] {
                node(&out, Role::Label, text);
            }
            let update = out.platform_output.accesskit_update.as_ref().unwrap();
            assert!(
                !update.nodes.iter().any(|(_, n)| [n.label(), n.value()]
                    .into_iter()
                    .flatten()
                    .any(|t| t.contains("synthetic-private-token"))),
                "{typed:?}: the credential stays hidden in the sheet"
            );
            assert!(nodes(&out, Role::Label, "Resultado").is_empty());
            let confirm = node(&out, Role::Button, "Excluir");
            assert_eq!(confirm.is_disabled(), !enabled, "{typed:?}");
            click_sized(&ctx, WINDOW, rect_of(&confirm).center(), &mut add);
            drop(add);
            assert_eq!(panel.busy, enabled, "{typed:?}");
            assert_eq!(panel.receipt.is_some(), !enabled, "{typed:?}");
            if enabled {
                let until = Instant::now() + Duration::from_secs(3);
                while panel.busy {
                    assert!(Instant::now() < until, "the worker never answered");
                    std::thread::sleep(Duration::from_millis(2));
                    panel.poll(&shared);
                }
            }
            assert_eq!(mock.requests().len(), usize::from(enabled), "{typed:?}");
        }
    }

    #[test]
    fn a_click_operation_and_a_local_start_confirm_without_typing() {
        let mut op = Operation::new(advanced::Kind::CreateUser);
        op.values.insert("username".into(), "ana".into());
        let mut local = local_service::Operation::new(Backend::Docker, LocalKind::Start);
        local.values.insert("container".into(), "ai-memory".into());
        for (intent, local_tab, verb, subtitle) in [
            (
                Intent::Advanced(op),
                false,
                "Criar",
                "Servidor · administração".to_owned(),
            ),
            (
                Intent::Local(local),
                true,
                "Iniciar",
                format!("Serviço deste {}", crate::platform::HOST),
            ),
        ] {
            let (_mock, shared, mut panel) = previewing(intent, local_tab);
            let ctx = context(egui::Theme::Light);
            let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
            node(&out, Role::Label, &subtitle);
            assert!(!node(&out, Role::Button, verb).is_disabled(), "{verb}");
        }
    }

    #[test]
    fn the_veil_keeps_the_tab_under_the_sheet_from_discarding_the_preview() {
        let local = format!("Serviço deste {}", crate::platform::HOST);
        for open in [true, false] {
            let (_mock, shared, mut panel) = previewing(purge(), false);
            if !open {
                panel.receipt = None;
            }
            let ctx = context(egui::Theme::Dark);
            let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
            let out = frames(&ctx, &mut add);
            let tab = rect_of(&node(&out, Role::Button, &local));
            if open {
                let title = rect_of(&node(&out, Role::Label, "Excluir o projeto “demo”?"));
                assert!(
                    tab.right() < title.left(),
                    "precondition: the tab is outside the sheet"
                );
            }
            click_sized(&ctx, WINDOW, tab.center(), &mut add);
            drop(add);
            assert_eq!(panel.local_tab, !open, "sheet open: {open}");
            assert_eq!(panel.receipt.is_some(), open, "sheet open: {open}");
        }
    }

    #[test]
    fn the_keyboard_never_reaches_the_tab_under_the_sheet() {
        let local = format!("Serviço deste {}", crate::platform::HOST);
        for open in [true, false] {
            let (_mock, shared, mut panel) = previewing(purge(), false);
            if !open {
                panel.receipt = None;
            }
            let ctx = context(egui::Theme::Dark);
            let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
            frames(&ctx, &mut add);
            // Tab até a aba do serviço local e Enter, como o teclado faria.
            let seen = tab_and_enter(&ctx, WINDOW, &mut add, |l| l == local, 12);
            drop(add);
            let case = format!("sheet open: {open}, focus went through {seen:?}");
            assert_eq!(
                panel.receipt.is_some(),
                open,
                "{case}: Tab + Enter on the tab discarded the preview"
            );
            assert_eq!(panel.local_tab, !open, "{case}");
            assert_eq!(seen.contains(&local), !open, "{case}");
            if open {
                // O teclado andou: só não chega ao que está sob o véu.
                assert!(seen.iter().any(|l| l == "Cancelar"), "{case}");
            }
        }
    }

    #[test]
    fn cancel_and_escape_close_the_sheet_and_clear_the_preview_and_the_text() {
        for way in ["button", "escape"] {
            let (mock, shared, mut panel) = previewing(purge(), false);
            panel.confirm = "dem".into();
            panel.op.values.insert("draft".into(), "kept".into());
            let ctx = context(egui::Theme::Light);
            let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
            let out = frames(&ctx, &mut add);
            match way {
                "button" => {
                    let at = rect_of(&node(&out, Role::Button, "Cancelar")).center();
                    click_sized(&ctx, WINDOW, at, &mut add);
                }
                _ => {
                    let key = |pressed| egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Default::default(),
                    };
                    run_sized(&ctx, WINDOW, vec![key(true), key(false)], &mut add);
                }
            }
            let out = run_sized(&ctx, WINDOW, vec![], &mut add);
            drop(add);
            assert!(
                panel.receipt.is_none() && panel.result.is_none() && panel.result_at.is_none(),
                "{way}"
            );
            assert_eq!(panel.confirm, "", "{way}");
            assert_eq!(
                panel.op.values.get("draft").map(String::as_str),
                Some("kept"),
                "{way}: the form keeps its fields"
            );
            assert!(nodes(&out, Role::Button, "Excluir").is_empty(), "{way}");
            assert!(nodes(&out, Role::Label, "Resultado").is_empty(), "{way}");
            assert!(mock.requests().is_empty(), "{way}: nothing was sent");
        }
    }

    #[test]
    fn the_preview_expires_by_the_receipt_age_closing_the_sheet_and_unlocking_the_form_in_one_frame()
     {
        let expired = "A prévia expirou após 5 minutos. Prepare a operação novamente.";
        let removed =
            "Resultado removido da tela após 5 minutos. Consulte novamente se necessário.";
        // Um aviso anterior: fica se nada vence, é trocado se vence.
        let before = "aviso anterior";
        // (idade do recibo, idade do resultado, recibo fica?, aviso)
        for (receipt_age, result_age, kept, notice) in [
            (Some(299), 0, true, before),
            // O resultado chegou depois do recibo: só a idade do recibo vence.
            (Some(300), 0, false, expired),
            (Some(301), 0, false, expired),
            (Some(301), 301, false, expired),
            // Sem prévia, o resultado privado sai pela idade dele.
            (None, 299, false, before),
            (None, 301, false, removed),
        ] {
            let (_mock, shared, mut panel) = previewing(purge(), false);
            match receipt_age {
                Some(age) => {
                    panel.receipt.as_mut().unwrap().at = Instant::now() - Duration::from_secs(age)
                }
                None => panel.receipt = None,
            }
            panel.result_at = Some(Instant::now() - Duration::from_secs(result_age));
            panel.notice = before.into();
            let case = format!("receipt {receipt_age:?}, result {result_age} s");
            let ctx = context(egui::Theme::Dark);
            // Um frame só: o `poll` do começo de `show` decide.
            let out = run_sized(&ctx, WINDOW, vec![], |ui| panel.show(ui, &shared, &[]));
            assert_eq!(panel.receipt.is_some(), kept, "{case}");
            assert_eq!(panel.notice, notice, "{case}");
            node(&out, Role::Label, notice);
            assert_eq!(panel.result.is_some(), notice == before, "{case}");
            assert_eq!(
                node(&out, Role::Button, "Consultar").is_disabled(),
                kept,
                "{case}: the form is locked only while the preview lives"
            );
            assert_eq!(
                nodes(&out, Role::Button, "Cancelar").len(),
                usize::from(kept),
                "{case}: the sheet is open only while the preview lives"
            );
        }
    }

    #[test]
    fn a_pasted_confirmation_stops_at_512_characters() {
        let (_mock, shared, mut panel) = previewing(purge(), false);
        let ctx = context(egui::Theme::Light);
        let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
        frames(&ctx, &mut add);
        run_sized(
            &ctx,
            WINDOW,
            vec![egui::Event::Paste("é".repeat(600))],
            &mut add,
        );
        drop(add);
        assert_eq!(panel.confirm.chars().count(), 512);
        assert_eq!(panel.confirm, "é".repeat(512));
    }

    #[test]
    fn the_result_after_applying_stays_outside_with_reveal_and_clear() {
        let (_mock, shared, mut panel) = previewing(purge(), false);
        panel.receipt = None;
        let ctx = context(egui::Theme::Dark);
        let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
        node(&out, Role::Label, "Resultado");
        node(&out, Role::CheckBox, "Revelar credenciais neste resultado");
        node(&out, Role::Button, "Limpar resultado privado");
        assert!(nodes(&out, Role::Label, "Prévia de impacto").is_empty());
        assert!(nodes(&out, Role::Button, "Cancelar").is_empty(), "no sheet");
    }
    #[test]
    fn the_result_clear_button_is_raised_and_never_the_field_color() {
        for (theme, p) in THEMES {
            let (_mock, shared, mut panel) = previewing(purge(), false);
            panel.receipt = None;
            let ctx = context(theme);
            let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
            let at = rect_of(&node(&out, Role::Button, "Limpar resultado privado"));
            let fills: Vec<egui::Color32> = rects(&out)
                .into_iter()
                .filter(|r| r.rect == at)
                .map(|r| r.fill)
                .collect();
            assert_eq!(fills, [p.raised], "{theme:?}");
            assert_ne!(p.raised, p.field, "{theme:?}: like a field");
        }
    }
    #[test]
    fn every_select_on_the_form_card_is_raised_and_never_the_field_color() {
        for (theme, p) in THEMES {
            // (aba local?, os seletores do cartão do formulário)
            for (local_tab, selects) in [
                (false, &["Grupo", "Operação", "Projeto conhecido"][..]),
                (true, &["Operação"][..]),
            ] {
                let (_mock, shared, mut panel) = idle();
                // Com "workspace": o seletor de projeto conhecido aparece.
                panel.op = Operation::new(Kind::SendMessage);
                panel.local_tab = local_tab;
                let ctx = context(theme);
                let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
                // O ponteiro sai: sem o surf2 de quem está sob ele.
                run_sized(&ctx, WINDOW, vec![egui::Event::PointerGone], &mut add);
                let out = frames(&ctx, &mut add);
                for label in selects {
                    let at = rect_of(&node(&out, Role::ComboBox, label));
                    let fills: Vec<egui::Color32> = rects(&out)
                        .into_iter()
                        .filter(|r| r.rect == at)
                        .map(|r| r.fill)
                        .collect();
                    let case = format!("{theme:?} local {local_tab} {label}");
                    assert_eq!(fills, [p.raised], "{case}");
                    assert_ne!(fills, [p.field], "{case}: like a field");
                }
            }
        }
    }
    #[test]
    fn private_result_expires_without_rendering_a_window() {
        let shared = Shared::unstarted(crate::model::Config::default());
        let mut panel = Panel::new(shared.clone());
        panel.result = Some(serde_json::json!({"token":"synthetic-private"}));
        panel.result_at = Some(Instant::now() - Duration::from_secs(301));
        panel.reveal = true;
        panel.poll(&shared);
        assert!(panel.result.is_none());
        assert!(panel.receipt.is_none());
        assert!(!panel.reveal);
        panel
            .op
            .values
            .insert("body".into(), "synthetic message".into());
        panel.reset();
        assert!(!panel.op.values.contains_key("body"));
    }

    /// Um painel parado, sem prévia nem resultado, num servidor sintético.
    fn idle() -> (Mock, Arc<Shared>, Panel) {
        let mock = Mock::new(|_| Response::json(200, serde_json::json!({"ok": true})));
        let shared = Shared::unstarted(mock.config());
        let panel = Panel::new(shared.clone());
        (mock, shared, panel)
    }
    /// O popup aberto sob o seletor `combo` lista `options` nesta ordem,
    /// abaixo dele, com só `current` marcada.
    fn listed(out: &egui::FullOutput, combo: &str, options: &[&str], current: &str) {
        let mut last = rect_of(&node(out, Role::ComboBox, combo)).bottom() - 0.5;
        for option in options {
            let item = node(out, Role::Button, option);
            let selected = if *option == current {
                Toggled::True
            } else {
                Toggled::False
            };
            assert_eq!(item.toggled(), Some(selected), "{combo}: {option}");
            let top = rect_of(&item).top();
            assert!(top > last, "{combo}: {option} below the previous one");
            last = top;
        }
    }
    /// Abre o seletor `combo` e devolve o frame com o popup à vista.
    fn open(
        ctx: &egui::Context,
        add: &mut impl FnMut(&mut egui::Ui),
        combo: &str,
    ) -> egui::FullOutput {
        let out = frames(ctx, add);
        let at = rect_of(&node(&out, Role::ComboBox, combo)).center();
        click_sized(ctx, WINDOW, at, &mut *add);
        // O popup novo mede num frame e aparece no seguinte.
        frames(ctx, add)
    }
    /// Abre o seletor `combo` e clica na opção `option`.
    fn pick(ctx: &egui::Context, add: &mut impl FnMut(&mut egui::Ui), combo: &str, option: &str) {
        let out = open(ctx, add, combo);
        let at = rect_of(&node(&out, Role::Button, option)).center();
        click_sized(ctx, WINDOW, at, &mut *add);
    }
    /// Clica no nó `role` `text` do frame atual.
    fn press(ctx: &egui::Context, add: &mut impl FnMut(&mut egui::Ui), role: Role, text: &str) {
        let out = frames(ctx, add);
        let at = rect_of(&node(&out, role, text)).center();
        click_sized(ctx, WINDOW, at, &mut *add);
    }
    /// Espera a resposta do worker.
    fn settle(panel: &mut Panel, shared: &Shared) {
        let until = Instant::now() + Duration::from_secs(3);
        while panel.busy {
            assert!(Instant::now() < until, "the worker never answered");
            std::thread::sleep(Duration::from_millis(2));
            panel.poll(shared);
        }
    }
    /// Algum nó da saída fala `secret`.
    fn speaks(out: &egui::FullOutput, secret: &str) -> bool {
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        update.nodes.iter().any(|(_, n)| {
            [n.label(), n.value()]
                .into_iter()
                .flatten()
                .any(|t| t.contains(secret))
        })
    }
    fn local_label() -> String {
        format!("Serviço deste {}", crate::platform::HOST)
    }

    #[test]
    fn every_operation_is_reached_by_its_group_and_then_its_name() {
        // Um membro novo do catálogo quebra esta contagem: confira o grupo dele.
        assert_eq!(advanced::ALL.len(), 36);
        let groups: Vec<&str> = GROUPS.iter().map(|g| g.label()).collect();
        let (_mock, shared, mut panel) = idle();
        let ctx = context(egui::Theme::Dark);
        for &kind in advanced::ALL {
            let group = Group::of(kind);
            let (prefix, rest) = kind.label().split_once(" · ").expect("Grupo · operação");
            assert_eq!(
                prefix,
                group.label(),
                "{kind:?}: the group is the label prefix"
            );
            assert_eq!(name(kind), rest, "{kind:?}");
            {
                let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
                let out = open(&ctx, &mut add, "Grupo");
                let before = node(&out, Role::ComboBox, "Grupo")
                    .value()
                    .unwrap()
                    .to_owned();
                listed(&out, "Grupo", &groups, &before);
                let at = rect_of(&node(&out, Role::Button, group.label())).center();
                click_sized(&ctx, WINDOW, at, &mut add);
                let out = open(&ctx, &mut add, "Operação");
                let names: Vec<&str> = group.kinds().map(name).collect();
                let current = node(&out, Role::ComboBox, "Operação")
                    .value()
                    .unwrap()
                    .to_owned();
                listed(&out, "Operação", &names, &current);
                // Só as do grupo: nenhum nome que seja só de outro grupo.
                for other in advanced::ALL.iter().map(|&k| name(k)) {
                    if !names.contains(&other) {
                        assert!(
                            nodes(&out, Role::Button, other).is_empty(),
                            "{kind:?}: {other}"
                        );
                    }
                }
                let at = rect_of(&node(&out, Role::Button, name(kind))).center();
                click_sized(&ctx, WINDOW, at, &mut add);
                let out = frames(&ctx, &mut add);
                assert_eq!(
                    (
                        node(&out, Role::ComboBox, "Grupo").value(),
                        node(&out, Role::ComboBox, "Operação").value()
                    ),
                    (Some(group.label()), Some(name(kind))),
                    "{kind:?}"
                );
            }
            assert_eq!(panel.op.kind, kind);
        }
        // Os grupos repartem o catálogo: cada um tem operações, nenhuma sobra.
        assert!(GROUPS.iter().all(|g| g.kinds().count() > 0));
        assert_eq!(GROUPS.iter().map(|g| g.kinds().count()).sum::<usize>(), 36);
    }

    #[test]
    fn reads_consult_and_writes_open_a_preview_through_one_primary_button() {
        for (theme, p) in THEMES {
            let (_mock, shared, mut panel) = idle();
            let ctx = context(theme);
            let check = |panel: &mut Panel, read: bool, case: &str| {
                let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
                let (want, other) = if read {
                    ("Consultar", "Prévia…")
                } else {
                    ("Prévia…", "Consultar")
                };
                let button = node(&out, Role::Button, want);
                assert!(!button.is_disabled(), "{theme:?} {case}");
                assert!(
                    nodes(&out, Role::Button, other).is_empty(),
                    "{theme:?} {case}"
                );
                let at = rect_of(&button);
                assert!(
                    rects(&out).iter().any(|r| r.rect == at && r.fill == p.acc),
                    "{theme:?} {case}: {want} is primary"
                );
            };
            for &kind in advanced::ALL {
                panel.op = Operation::new(kind);
                check(&mut panel, kind.read(), &format!("{kind:?}"));
            }
            panel.local_tab = true;
            for kind in LOCAL {
                // Um tipo local novo tem de entrar em LOCAL e aqui.
                let read = match kind {
                    LocalKind::Status | LocalKind::Logs | LocalKind::Recovery => true,
                    LocalKind::Start | LocalKind::Stop | LocalKind::Restart | LocalKind::Update => {
                        false
                    }
                };
                panel.local = local_service::Operation::new(Backend::Docker, kind);
                check(&mut panel, read, &format!("local {kind:?}"));
            }
        }
    }

    #[test]
    fn every_field_is_a_labelled_text_field_or_a_segmented_and_optionals_say_so() {
        let (_mock, shared, mut panel) = idle();
        let ctx = context(egui::Theme::Light);
        for &kind in advanced::ALL {
            panel.op = Operation::new(kind);
            let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
            let update = out.platform_output.accesskit_update.as_ref().unwrap();
            let inputs: Vec<&egui::accesskit::Node> = update
                .nodes
                .iter()
                .map(|(_, n)| n)
                .filter(|n| matches!(n.role(), Role::TextInput | Role::MultilineTextInput))
                .collect();
            let mut texts = 0;
            for field in kind.fields() {
                let case = format!("{kind:?} {}", field.key);
                let (label, _) = update
                    .nodes
                    .iter()
                    .find(|(_, n)| n.role() == Role::Label && n.value() == Some(field.label))
                    .unwrap_or_else(|| panic!("{case}: no label {:?}", field.label));
                let labelled: Vec<_> = inputs
                    .iter()
                    .filter(|n| n.labelled_by().contains(label))
                    .collect();
                if field.choices.is_empty() {
                    let [input] = labelled[..] else {
                        panic!("{case}: one field, got {}", labelled.len())
                    };
                    let role = if field.key == "body" {
                        Role::MultilineTextInput
                    } else {
                        Role::TextInput
                    };
                    assert_eq!(input.role(), role, "{case}");
                    let hint = if field.optional { "Opcional" } else { "" };
                    assert_eq!(input.placeholder(), Some(hint), "{case}");
                    texts += 1;
                } else {
                    assert!(labelled.is_empty(), "{case}: a choice is not a text field");
                    for (i, choice) in field.choices.iter().enumerate() {
                        assert_eq!(
                            node(&out, Role::Button, choice).toggled(),
                            Some(if i == 0 {
                                Toggled::True
                            } else {
                                Toggled::False
                            }),
                            "{case} {choice}"
                        );
                    }
                }
            }
            assert_eq!(inputs.len(), texts, "{kind:?}: no other text field");
        }
    }

    #[test]
    fn clicking_a_choice_stores_it_and_moves_the_selection() {
        let mut choices = 0;
        for &kind in advanced::ALL {
            for field in kind.fields().into_iter().filter(|f| !f.choices.is_empty()) {
                choices += 1;
                let last = field.choices[field.choices.len() - 1];
                let (_mock, shared, mut panel) = idle();
                panel.op = Operation::new(kind);
                let ctx = context(egui::Theme::Dark);
                let out = {
                    let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
                    press(&ctx, &mut add, Role::Button, last);
                    frames(&ctx, &mut add)
                };
                assert_eq!(
                    panel.op.values.get(field.key).map(String::as_str),
                    Some(last),
                    "{kind:?} {}",
                    field.key
                );
                assert_eq!(
                    node(&out, Role::Button, last).toggled(),
                    Some(Toggled::True)
                );
                assert_eq!(
                    node(&out, Role::Button, field.choices[0]).toggled(),
                    Some(Toggled::False)
                );
            }
        }
        // MoveProject, MoveSession, CreateUser, EditUser, Grant, ProjectAccess e SendMessage.
        assert_eq!(choices, 7);
    }

    #[test]
    fn blank_required_fields_are_refused_before_any_request() {
        for &kind in advanced::ALL {
            let (mock, shared, mut panel) = idle();
            panel.op = Operation::new(kind);
            let ctx = context(egui::Theme::Dark);
            let button = if kind.read() {
                "Consultar"
            } else {
                "Prévia…"
            };
            press(
                &ctx,
                &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]),
                Role::Button,
                button,
            );
            settle(&mut panel, &shared);
            // O primeiro obrigatório em branco: opções e o deslocamento vêm preenchidos.
            let blank = kind
                .fields()
                .into_iter()
                .find(|f| !f.optional && f.choices.is_empty() && f.key != "offset");
            match blank {
                Some(field) => {
                    let error = format!("Campo inválido: {}.", field.label);
                    assert_eq!(panel.error.as_deref(), Some(error.as_str()), "{kind:?}");
                    assert!(mock.requests().is_empty(), "{kind:?}: nothing was sent");
                    let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
                    node(&out, Role::Label, &error);
                }
                None => {
                    assert_eq!(panel.error, None, "{kind:?}");
                    assert!(
                        !mock.requests().is_empty(),
                        "{kind:?}: no field to fill, it was sent"
                    );
                }
            }
        }
        for kind in LOCAL {
            let (_mock, shared, mut panel) = idle();
            panel.local_tab = true;
            panel.local = local_service::Operation::new(Backend::Docker, kind);
            let ctx = context(egui::Theme::Dark);
            let button = if panel.local.read() {
                "Consultar"
            } else {
                "Prévia…"
            };
            press(
                &ctx,
                &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]),
                Role::Button,
                button,
            );
            settle(&mut panel, &shared);
            assert_eq!(
                panel.error.as_deref(),
                Some("Preencha todos os campos locais com valores explícitos válidos."),
                "local {kind:?}"
            );
            assert!(
                panel.receipt.is_none() && panel.result.is_none(),
                "local {kind:?}"
            );
        }
    }

    #[test]
    fn changing_operation_group_tab_or_local_operation_clears_the_private_result() {
        let backend = if cfg!(target_os = "macos") {
            "LaunchAgent"
        } else {
            "systemd (usuário)"
        };
        for how in [
            "operation",
            "group",
            "tab",
            "local operation",
            "local backend",
        ] {
            let (_mock, shared, mut panel) = idle();
            panel.op = Operation::new(Kind::SendMessage);
            panel
                .op
                .values
                .insert("body".into(), "rascunho privado".into());
            panel.local_tab = how.starts_with("local");
            panel
                .local
                .values
                .insert("container".into(), "ai-memory".into());
            panel.result = Some(serde_json::json!({"token": "synthetic-private-token"}));
            panel.result_at = Some(Instant::now());
            panel.reveal = true;
            panel.confirm = "demo".into();
            panel.notice = "aviso anterior".into();
            panel.error = Some("erro anterior".into());
            let ctx = context(egui::Theme::Dark);
            let out = {
                let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
                let out = frames(&ctx, &mut add);
                assert!(
                    speaks(&out, "synthetic-private-token"),
                    "{how}: precondition"
                );
                match how {
                    "operation" => pick(&ctx, &mut add, "Operação", "cancelar por ID"),
                    "group" => pick(&ctx, &mut add, "Grupo", "Chaves"),
                    "tab" => press(&ctx, &mut add, Role::Button, &local_label()),
                    "local operation" => pick(&ctx, &mut add, "Operação", "Ler logs privados"),
                    _ => press(&ctx, &mut add, Role::Button, backend),
                }
                frames(&ctx, &mut add)
            };
            assert!(!speaks(&out, "synthetic-private-token"), "{how}");
            assert!(nodes(&out, Role::Label, "Resultado").is_empty(), "{how}");
            assert!(
                panel.result.is_none() && panel.result_at.is_none() && !panel.reveal,
                "{how}: the private result is gone"
            );
            assert_eq!(
                (
                    panel.confirm.as_str(),
                    panel.notice.as_str(),
                    panel.error.as_deref()
                ),
                ("", "", None),
                "{how}"
            );
            match how {
                "operation" | "group" => {
                    let kind = if how == "group" {
                        Kind::Keys
                    } else {
                        Kind::CancelMessage
                    };
                    assert_eq!(panel.op.kind, kind, "{how}");
                    assert_eq!(panel.op, Operation::new(kind), "{how}: the draft is gone");
                }
                // Como antes: a aba e o serviço local limpam só o resultado.
                "tab" => assert_eq!(
                    panel.op.values.get("body").map(String::as_str),
                    Some("rascunho privado")
                ),
                _ => assert_eq!(
                    panel.local.values.get("container").map(String::as_str),
                    Some("ai-memory"),
                    "{how}"
                ),
            }
        }
    }

    #[test]
    fn the_reveal_switch_starts_off_and_shows_then_hides_the_credential() {
        for (theme, _) in THEMES {
            let (_mock, shared, mut panel) = idle();
            panel.result = Some(serde_json::json!({
                "project": "demo",
                "token": "synthetic-private-token",
            }));
            panel.result_at = Some(Instant::now());
            let ctx = context(theme);
            let switch = "Revelar credenciais neste resultado";
            let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
            for (step, revealed) in [(0, false), (1, true), (2, false)] {
                if step > 0 {
                    press(&ctx, &mut add, Role::CheckBox, switch);
                }
                let out = frames(&ctx, &mut add);
                let toggled = if revealed {
                    Toggled::True
                } else {
                    Toggled::False
                };
                let node_switch = node(&out, Role::CheckBox, switch);
                assert_eq!(node_switch.toggled(), Some(toggled), "{theme:?} {step}");
                // Um interruptor (o trilho de 26 × 16), não uma caixa.
                let at = rect_of(&node_switch);
                assert!(
                    rects(&out)
                        .iter()
                        .any(|r| r.rect.size() == egui::vec2(26., 16.) && at.contains_rect(r.rect)),
                    "{theme:?} {step}: a switch"
                );
                assert_eq!(
                    speaks(&out, "synthetic-private-token"),
                    revealed,
                    "{theme:?} {step}"
                );
                node(
                    &out,
                    Role::Label,
                    if revealed {
                        "token: synthetic-private-token"
                    } else {
                        "token: [oculto]"
                    },
                );
                node(&out, Role::Label, "project: demo");
            }
            drop(add);
            assert!(!panel.reveal);
        }
    }

    #[test]
    fn tabs_and_backends_expose_their_selection_to_accesskit() {
        let backend = if cfg!(target_os = "macos") {
            "LaunchAgent"
        } else {
            "systemd (usuário)"
        };
        let (_mock, shared, mut panel) = idle();
        let ctx = context(egui::Theme::Light);
        let local = local_label();
        let mut add = |ui: &mut egui::Ui| panel.show(ui, &shared, &[]);
        let state = |out: &egui::FullOutput, text: &str| node(out, Role::Button, text).toggled();
        let out = frames(&ctx, &mut add);
        assert_eq!(
            (state(&out, "Servidor · administração"), state(&out, &local)),
            (Some(Toggled::True), Some(Toggled::False))
        );
        press(&ctx, &mut add, Role::Button, &local);
        let out = frames(&ctx, &mut add);
        assert_eq!(
            (state(&out, "Servidor · administração"), state(&out, &local)),
            (Some(Toggled::False), Some(Toggled::True))
        );
        assert_eq!(
            (state(&out, "Docker"), state(&out, backend)),
            (Some(Toggled::True), Some(Toggled::False))
        );
        press(&ctx, &mut add, Role::Button, backend);
        let out = frames(&ctx, &mut add);
        assert_eq!(
            (state(&out, "Docker"), state(&out, backend)),
            (Some(Toggled::False), Some(Toggled::True))
        );
        drop(add);
        assert!(panel.local_tab);
        assert_ne!(panel.local.backend, Backend::Docker);
    }

    #[test]
    fn the_operation_warning_sits_in_the_card_right_before_its_button() {
        for (theme, p) in THEMES {
            let (_mock, shared, mut panel) = idle();
            let ctx = context(theme);
            let check = |panel: &mut Panel, read: bool, warning: &str, case: &str| {
                let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
                let (bold, fill) = if read {
                    ("Observação.", widgets::tone(p.info, 0.10))
                } else {
                    ("Atenção.", widgets::tone(p.warn, 0.10))
                };
                let strip = rect_of(&node(&out, Role::Label, &format!("{bold} {warning}")));
                assert!(
                    rects(&out)
                        .iter()
                        .any(|r| r.rect == strip && r.fill == fill),
                    "{theme:?} {case}: {bold} tone"
                );
                let button = rect_of(&node(&out, Role::Button, action(read)));
                let gap = button.top() - strip.bottom();
                assert!(
                    (0. ..=12.).contains(&gap),
                    "{case}: right before the button, {gap}"
                );
                let card = rects(&out)
                    .into_iter()
                    .filter(|r| r.fill == p.surf && r.corner_radius == egui::CornerRadius::same(10))
                    .map(|r| r.rect)
                    .find(|r| r.contains_rect(strip))
                    .unwrap_or_else(|| panic!("{case}: the strip is in a card"));
                assert!(card.contains_rect(button), "{case}: and so is the button");
            };
            for &kind in advanced::ALL {
                panel.op = Operation::new(kind);
                check(
                    &mut panel,
                    kind.read(),
                    kind.warning(),
                    &format!("{kind:?}"),
                );
            }
            panel.local_tab = true;
            for kind in LOCAL {
                panel.local = local_service::Operation::new(Backend::Docker, kind);
                let read = panel.local.read();
                check(
                    &mut panel,
                    read,
                    local_service::WARNING,
                    &format!("local {kind:?}"),
                );
            }
        }
    }

    #[test]
    fn every_control_has_the_line_border_and_4_5_to_1_text_in_both_themes() {
        use crate::widgets::tests::audit;
        for (theme, p) in THEMES {
            for local_tab in [false, true] {
                let (_mock, shared, mut panel) = idle();
                // Seletores, campos (um opcional preenchido), a mensagem, um
                // segmentado de opções e o cartão do resultado.
                panel.op = Operation::new(Kind::SendMessage);
                panel.op.values.insert("subject".into(), "assunto".into());
                panel.local_tab = local_tab;
                panel.result = Some(serde_json::json!({"token": "synthetic"}));
                panel.result_at = Some(Instant::now());
                let ctx = context(theme);
                let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
                let audit = audit(&out, p);
                let case = format!("{theme:?} local {local_tab}");
                assert!(audit.failures.is_empty(), "{case}: {:#?}", audit.failures);
                let mut want = vec![
                    "ComboBox Operação",
                    "Button Servidor · administração",
                    "Button Limpar resultado privado",
                    "CheckBox Revelar credenciais neste resultado",
                ];
                let (fields, frames) = if local_tab {
                    want.extend(["Button Consultar", "Button Docker"]);
                    (2, 2)
                } else {
                    want.extend([
                        "ComboBox Grupo",
                        "ComboBox Projeto conhecido",
                        "Button Prévia…",
                        "Button unknown",
                        "TextInput assunto",
                        "MultilineTextInput ",
                    ]);
                    (7, 2)
                };
                for name in want {
                    assert!(
                        audit.checked.iter().any(|c| c == name),
                        "{case}: {name} was measured, {:#?}",
                        audit.checked
                    );
                }
                let count = |prefix: &str| {
                    audit
                        .checked
                        .iter()
                        .filter(|c| c.starts_with(prefix))
                        .count()
                };
                assert_eq!(
                    count("TextInput ") + count("MultilineTextInput "),
                    fields,
                    "{case}"
                );
                assert_eq!(count("segmented at "), frames, "{case}");
            }
        }
    }

    #[test]
    fn the_credentials_legend_closes_the_page() {
        for with_result in [false, true] {
            let (_mock, shared, mut panel) = idle();
            if with_result {
                panel.result = Some(serde_json::json!({"ok": true}));
                panel.result_at = Some(Instant::now());
            }
            let ctx = context(egui::Theme::Dark);
            let out = frames(&ctx, &mut |ui: &mut egui::Ui| panel.show(ui, &shared, &[]));
            let legend = rect_of(&node(&out, Role::Label, LEGEND));
            let lowest = rects(&out)
                .into_iter()
                .filter(|r| r.corner_radius == egui::CornerRadius::same(10))
                .map(|r| r.rect.bottom())
                .fold(0., f32::max);
            assert!(lowest > 0., "{with_result}: precondition, a card");
            assert!(legend.top() > lowest, "{with_result}: below every card");
            assert_eq!(
                nodes(&out, Role::Label, "Resultado").len(),
                usize::from(with_result)
            );
        }
    }

    #[test]
    fn the_form_fits_a_narrow_window() {
        for local_tab in [false, true] {
            let (_mock, shared, mut panel) = idle();
            panel.op = Operation::new(Kind::SendMessage);
            panel.local_tab = local_tab;
            let ctx = context(egui::Theme::Light);
            let size = [560., 1400.];
            run_sized(&ctx, size, vec![], |ui| panel.show(ui, &shared, &[]));
            let out = run_sized(&ctx, size, vec![], |ui| panel.show(ui, &shared, &[]));
            let update = out.platform_output.accesskit_update.as_ref().unwrap();
            let mut controls = 0;
            for (_, n) in &update.nodes {
                if matches!(
                    n.role(),
                    Role::Button | Role::ComboBox | Role::TextInput | Role::MultilineTextInput
                ) && let Some(b) = n.bounds()
                {
                    controls += 1;
                    assert!(
                        b.x1 <= f64::from(size[0]) - 8. + 0.5,
                        "local {local_tab}: {:?} {:?} ends at {}",
                        n.role(),
                        n.label().or(n.value()),
                        b.x1
                    );
                }
            }
            assert!(controls > 5, "precondition: {controls}");
        }
    }
}
