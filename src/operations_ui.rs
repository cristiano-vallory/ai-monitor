use crate::{
    advanced::{self, Operation},
    client::Scope,
    local_service::{self, Backend, Kind as LocalKind},
    maintenance::{Action, Intent, Read, Receipt, Work, Worker},
    model::Project,
    monitor::Shared,
};
use eframe::egui::{self, Color32};
use serde_json::Value;
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
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
        if self
            .result_at
            .is_some_and(|at| at.elapsed() > Duration::from_secs(300))
        {
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
                ui.selectable_value(&mut self.local_tab, true, "Serviço deste Mac");
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
            ui.colored_label(Color32::from_rgb(244, 190, 103), error);
        }
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        if let Some(receipt) = self.receipt.clone() {
            ui.separator();
            ui.heading("Revisar e confirmar");
            ui.strong(receipt.action.label());
            ui.label(receipt.action.warning());
            let expected = match &receipt.action.intent {
                Intent::Advanced(op) => op.confirmation(),
                Intent::Local(op) => op.confirmation(),
                _ => return,
            };
            ui.label(format!("Revise a prévia abaixo e digite: {expected}"));
            ui.add(egui::TextEdit::singleline(&mut self.confirm).char_limit(1024));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.busy && self.confirm == expected,
                        egui::Button::new("Confirmar operação"),
                    )
                    .clicked()
                {
                    self.submit(shared, Work::Apply(receipt));
                }
                if ui.button("Cancelar prévia").clicked() {
                    self.clear();
                }
            });
        }
        if let Some(value) = self.result.clone() {
            ui.separator();
            ui.heading(if self.receipt.is_some() {
                "Prévia de impacto"
            } else {
                "Resultado"
            });
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
            ui.selectable_value(&mut self.local.backend, Backend::LaunchAgent, "LaunchAgent");
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
        ui.small("Configure explicitamente o serviço. Docker usa contexto com socket Unix e fixa o ID revisado. LaunchAgent aceita o plist oficial; atualização nativa exige serviço parado, binário novo e SHA-256. Campos não são gravados em disco.");
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
