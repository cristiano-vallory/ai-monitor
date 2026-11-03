use crate::{
    advanced::{self, Operation},
    client::Scope,
    local_service::{self, Backend, Kind as LocalKind},
    maintenance::{Action, Confirmation, Intent, PREVIEW_TTL, Read, Receipt, Work, Worker},
    model::Project,
    monitor::Shared,
    widgets::{self, Answer},
};
use eframe::egui;
use serde_json::Value;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

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
        if self.receipt.as_ref().is_some_and(|r| r.left().is_zero()) {
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
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.local_tab, false, "Servidor · administração");
                ui.selectable_value(
                    &mut self.local_tab,
                    true,
                    format!("Serviço deste {}", crate::platform::HOST),
                );
            });
        });
        if previous != self.local_tab {
            self.clear();
        }
        ui.label("Operações sob demanda. Credenciais geradas e logs ficam somente na memória e são removidos da tela após 5 minutos ou ao limpar/trocar de operação.");
        ui.add_enabled_ui(!self.busy && self.receipt.is_none(), |ui| {
            if self.local_tab {
                self.local_form(ui, shared);
            } else {
                self.remote_form(ui, shared, projects);
            }
        });
        if self.busy {
            ui.spinner();
            ui.label(
                "Operação em andamento. Aguarde antes de trocar a conexão ou iniciar outra ação.",
            );
        }
        if let Some(error) = &self.error {
            ui.colored_label(crate::theme::palette(ui.visuals()).bad, error);
        }
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        if let Some(receipt) = self.receipt.clone() {
            let action = &receipt.action;
            let typed = match action.confirmation() {
                Some(Confirmation::Type(target)) => Some(target),
                // Leitura não gera recibo.
                Some(Confirmation::Click) | None => None,
            };
            let subtitle = if self.local_tab {
                format!("Serviço deste {}", crate::platform::HOST)
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
            ui.separator();
            ui.heading("Resultado");
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.reveal, "Revelar credenciais neste resultado");
                if ui.button("Limpar resultado privado").clicked() {
                    self.clear();
                }
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
        }
    }
    fn remote_form(&mut self, ui: &mut egui::Ui, shared: &Shared, projects: &[Project]) {
        let old = self.op.kind;
        egui::ComboBox::from_id_salt("advanced-operation")
            .width(460.)
            .selected_text(self.op.kind.label())
            .show_ui(ui, |ui| {
                for &kind in advanced::ALL {
                    if ui
                        .selectable_value(&mut self.op.kind, kind, kind.label())
                        .clicked()
                    {
                        ui.close();
                    }
                }
            });
        if old != self.op.kind {
            self.clear();
            self.op = Operation::new(self.op.kind);
        }
        ui.label(self.op.kind.warning());
        if self.op.values.contains_key("workspace") {
            egui::ComboBox::from_id_salt("advanced-preset")
                .width(460.)
                .selected_text("Preencher a partir de um projeto conhecido…")
                .show_ui(ui, |ui| {
                    for p in projects {
                        if ui
                            .selectable_label(
                                false,
                                format!("{} / {}", p.workspace_name, p.project_name),
                            )
                            .clicked()
                        {
                            self.op
                                .values
                                .insert("workspace".into(), p.workspace_name.clone());
                            if self.op.values.contains_key("project") {
                                self.op
                                    .values
                                    .insert("project".into(), p.project_name.clone());
                            }
                            ui.close();
                        }
                    }
                });
        }
        for field in self.op.kind.fields() {
            ui.push_id(field.key, |ui| {
                ui.label(field.label);
                let value = self.op.values.entry(field.key.into()).or_default();
                if !field.choices.is_empty() {
                    egui::ComboBox::from_id_salt("choice")
                        .selected_text(value.as_str())
                        .show_ui(ui, |ui| {
                            for c in field.choices {
                                if ui.selectable_value(value, (*c).into(), *c).clicked() {
                                    ui.close();
                                }
                            }
                        });
                } else if field.key == "body" {
                    ui.add(
                        egui::TextEdit::multiline(value)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY)
                            .char_limit(8000),
                    );
                } else {
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .desired_width(550.)
                            .char_limit(1024),
                    );
                }
            });
        }
        if ui
            .button(if self.op.kind.read() {
                "Consultar"
            } else {
                "Preparar prévia"
            })
            .clicked()
        {
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
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.local.backend, Backend::Docker, "Docker");
            #[cfg(target_os = "macos")]
            ui.selectable_value(&mut self.local.backend, Backend::LaunchAgent, "LaunchAgent");
            #[cfg(target_os = "linux")]
            ui.selectable_value(
                &mut self.local.backend,
                Backend::Systemd,
                "systemd (usuário)",
            );
        });
        egui::ComboBox::from_id_salt("local-operation")
            .width(460.)
            .selected_text(self.local.label())
            .show_ui(ui, |ui| {
                for kind in [
                    LocalKind::Status,
                    LocalKind::Logs,
                    LocalKind::Start,
                    LocalKind::Stop,
                    LocalKind::Restart,
                    LocalKind::Update,
                    LocalKind::Recovery,
                ] {
                    let label = local_service::Operation::new(self.local.backend, kind).label();
                    if ui
                        .selectable_value(&mut self.local.kind, kind, label)
                        .clicked()
                    {
                        ui.close();
                    }
                }
            });
        if old != (self.local.backend, self.local.kind) {
            self.clear();
        }
        ui.label(local_service::WARNING);
        ui.small(crate::platform::LOCAL_SERVICE_HINT);
        if self.local.kind == LocalKind::Update {
            ui.label("Backup é obrigatório; o app verifica arquivo gzip privado, sem garantir sua restaurabilidade. Docker usa a imagem definida no Compose. O modo nativo mantém cópia do binário anterior e permanece parado até iniciar explicitamente.");
        }
        for (key, label) in self.local.fields() {
            ui.label(label);
            ui.push_id(key, |ui| {
                ui.add(
                    egui::TextEdit::singleline(self.local.values.entry(key.into()).or_default())
                        .desired_width(650.)
                        .char_limit(4096),
                );
            });
        }
        if ui
            .button(if self.local.read() {
                "Consultar serviço"
            } else {
                "Preparar operação local"
            })
            .clicked()
        {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        test_support::{Mock, Response},
        widgets::tests::{click_sized, context, node, nodes, rect_of, run_sized},
    };
    use egui::accesskit::Role;
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
}
