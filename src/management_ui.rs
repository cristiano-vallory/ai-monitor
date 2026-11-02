use crate::{
    client::Scope,
    console,
    maintenance::{Action, Intent, Read, Receipt, Task, Work, Worker},
    model::Project,
    monitor::Shared,
};
use eframe::egui::{self, Color32};
use serde_json::Value;
use std::sync::{Arc, atomic::Ordering};

pub struct Management {
    worker: Worker,
    pub busy: bool,
    ticket: u64,
    scope: Option<Scope>,
    tab: u8,
    result: Option<Value>,
    receipt: Option<Receipt>,
    error: Option<String>,
    notice: String,
    confirm: String,
    status: String,
    identifier: String,
    reason: String,
    task: Task,
    reembed: bool,
    days: u32,
    session: String,
    path: String,
    signal: String,
    version: String,
    git: bool,
    message: String,
    destination: String,
    okf: bool,
}
impl Management {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            worker: Worker::new(shared),
            busy: false,
            ticket: 0,
            scope: None,
            tab: 0,
            result: None,
            receipt: None,
            error: None,
            notice: String::new(),
            confirm: String::new(),
            status: "pending".into(),
            identifier: String::new(),
            reason: String::new(),
            task: Task::Lint,
            reembed: false,
            days: 7,
            session: String::new(),
            path: String::new(),
            signal: "helpful".into(),
            version: String::new(),
            git: false,
            message: "Checkpoint solicitado pelo AI Monitor".into(),
            destination: String::new(),
            okf: false,
        }
    }
    pub fn reset(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.scope = None;
        self.clear();
        self.identifier.clear();
        self.reason.clear();
        self.session.clear();
        self.path.clear();
        self.version.clear();
        self.destination.clear();
    }
    fn clear(&mut self) {
        self.receipt = None;
        self.result = None;
        self.error = None;
        self.confirm.clear();
        self.notice.clear();
    }
    fn begin(&mut self, shared: &Shared, work: Work) {
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
    fn preview(&mut self, shared: &Shared, scope: &Scope, intent: Intent) {
        self.begin(
            shared,
            Work::Preview(Action {
                scope: scope.clone(),
                intent,
            }),
        );
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
                Ok(v) => {
                    self.result = Some(v);
                    self.receipt = reply.receipt;
                    self.notice=if reply.applying {"Resposta recebida. Confira os detalhes abaixo e atualize a listagem para verificar o estado final."}else{"Consulta concluída; revise os dados abaixo."}.into();
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    if reply.applying {
                        self.notice="A solicitação pode ter sido processada mesmo sem uma resposta válida. Confira o estado no servidor antes de tentar novamente. Não houve repetição automática.".into();
                    }
                }
            }
        }
        if self
            .receipt
            .as_ref()
            .is_some_and(|r| r.at.elapsed() > std::time::Duration::from_secs(300))
        {
            self.clear();
            self.notice = "A prévia expirou após 5 minutos. Prepare a operação novamente.".into();
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, shared: &Arc<Shared>, projects: &[Project]) {
        self.poll(shared);
        ui.label("Revise o escopo e a prévia antes de confirmar. Operações dependem das permissões e recursos habilitados no servidor.");
        let before = (self.scope.clone(), self.tab);
        ui.add_enabled_ui(!self.busy, |ui| {
            egui::ComboBox::from_id_salt("management-scope")
                .width(420.)
                .selected_text(
                    self.scope
                        .as_ref()
                        .map(Scope::label)
                        .unwrap_or("Selecione um projeto".into()),
                )
                .show_ui(ui, |ui| {
                    for p in projects {
                        let s = Scope {
                            workspace: p.workspace_name.clone(),
                            project: p.project_name.clone(),
                        };
                        let label = s.label();
                        if ui
                            .selectable_value(&mut self.scope, Some(s), label)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
            ui.horizontal_wrapped(|ui| {
                for (id, label) in [
                    (0, "Propostas"),
                    (1, "Handoffs"),
                    (2, "Manutenção"),
                    (3, "Backup e restauração"),
                ] {
                    ui.selectable_value(&mut self.tab, id, label);
                }
            });
        });
        if before != (self.scope.clone(), self.tab) {
            self.clear();
            self.identifier.clear();
            self.reason.clear();
            self.path.clear();
            self.version.clear();
            self.session.clear();
        }
        let Some(scope) = self.scope.clone() else {
            ui.small("Selecione o projeto para continuar. Backup e checkpoints indicam explicitamente quando abrangem toda a instalação.");
            return;
        };
        ui.separator();
        ui.add_enabled_ui(!self.busy && self.receipt.is_none(), |ui| match self.tab {
            0 => self.proposals(ui, shared, &scope),
            1 => self.handoffs(ui, shared, &scope),
            2 => self.maintenance(ui, shared, &scope),
            _ => self.backups(ui, shared, &scope),
        });
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Operação em andamento. Aguarde a resposta antes de trocar a conexão.");
            });
        }
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(244, 190, 103), e);
        }
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        // The receipt is immutable: changing tabs/scope cancels it; fields are locked until cancel.
        if let Some(receipt) = self.receipt.clone() {
            ui.separator();
            ui.heading("Revisar e confirmar");
            ui.strong(receipt.action.label());
            ui.label(format!("Projeto: {}", receipt.action.scope.label()));
            ui.colored_label(Color32::from_rgb(244, 190, 103), receipt.action.warning());
            if let Intent::Reject { reason, .. } | Intent::Feedback { reason, .. } =
                &receipt.action.intent
            {
                ui.label(format!("Motivo: {reason}"));
            }
            let global = matches!(
                receipt.action.intent,
                Intent::Checkpoint { .. } | Intent::Download { okf: false, .. }
            );
            let expected = if let Intent::Consolidate { session } = &receipt.action.intent {
                ui.strong("O projeto selecionado é apenas contexto: esta operação é identificada pela sessão global.");
                format!("SESSAO {session}")
            } else if global {
                "GLOBAL".into()
            } else {
                receipt.action.scope.label()
            };
            ui.label(format!("Leia a prévia abaixo e digite {expected} para confirmar. Prévia válida por 5 minutos."));
            ui.add(egui::TextEdit::singleline(&mut self.confirm).char_limit(512));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.busy && self.confirm == expected,
                        egui::Button::new("Confirmar execução"),
                    )
                    .clicked()
                {
                    self.begin(shared, Work::Apply(receipt));
                }
                if ui.button("Cancelar e editar").clicked() {
                    self.clear();
                }
            });
        }
        if let Some(result) = self.result.clone() {
            ui.separator();
            ui.heading(if self.receipt.is_some() {
                "Prévia / evidências"
            } else {
                "Resultado"
            });
            if self.tab == 0 && result.is_array() {
                ui.small("Até 200 propostas; filtre pelo estado. Use o ID exato para revisar uma proposta fora da lista.");
                let mut seen = std::collections::HashSet::new();
                for (idx, row) in result.as_array().unwrap().iter().take(200).enumerate() {
                    ui.push_id(
                        crate::console::row_key(&mut seen, row["id"].as_str(), idx),
                        |ui| {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.strong(row["title"].as_str().unwrap_or("Proposta"));
                                ui.label(format!(
                                    "{} · {}",
                                    str_field(row, "status"),
                                    str_field(row, "target_path")
                                ));
                                ui.small(str_field(row, "id"));
                                if ui
                                    .add_enabled(
                                        !self.busy && self.receipt.is_none(),
                                        egui::Button::new("Selecionar ID"),
                                    )
                                    .clicked()
                                {
                                    self.identifier = str_field(row, "id").into();
                                }
                            });
                        },
                    );
                }
            } else {
                console::show_values(ui, &result, 0);
            }
        }
    }
    fn proposals(&mut self, ui: &mut egui::Ui, shared: &Shared, s: &Scope) {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("proposal-state")
                .selected_text(&self.status)
                .show_ui(ui, |ui| {
                    for status in ["pending", "approved", "rejected", "conflict", "failed"] {
                        ui.selectable_value(&mut self.status, status.into(), status);
                    }
                });
            if ui.button("Carregar propostas").clicked() {
                self.begin(
                    shared,
                    Work::Read(Read::Proposals {
                        scope: s.clone(),
                        status: self.status.clone(),
                    }),
                );
            }
        });
        field(ui, "ID da proposta", &mut self.identifier, 128);
        field(ui, "Motivo da rejeição", &mut self.reason, 500);
        ui.horizontal_wrapped(|ui| {
            if ui.button("Ver detalhes").clicked() {
                self.begin(
                    shared,
                    Work::Read(Read::Proposal {
                        scope: s.clone(),
                        id: self.identifier.trim().into(),
                    }),
                );
            }
            if ui.button("Revisar aprovação").clicked() {
                self.preview(
                    shared,
                    s,
                    Intent::Approve {
                        id: self.identifier.trim().into(),
                    },
                );
            }
            if ui.button("Revisar rejeição").clicked() {
                self.preview(
                    shared,
                    s,
                    Intent::Reject {
                        id: self.identifier.trim().into(),
                        reason: self.reason.clone(),
                    },
                );
            }
        });
        ui.small("A prévia carrega evidências e comparação do servidor. A fila depende da política de aprovação configurada no ai-memory.");
    }
    fn handoffs(&mut self, ui: &mut egui::Ui, shared: &Shared, s: &Scope) {
        if ui.button("Carregar histórico sem consumir").clicked() {
            self.begin(shared, Work::Read(Read::Handoffs(s.clone())));
        }
        field(ui, "ID do handoff", &mut self.identifier, 128);
        if ui.button("Revisar cancelamento por ID").clicked() {
            self.preview(
                shared,
                s,
                Intent::CancelHandoff {
                    id: self.identifier.trim().into(),
                },
            );
        }
        ui.horizontal(|ui| {
            ui.label("Idade mínima para expirar");
            ui.add(
                egui::DragValue::new(&mut self.days)
                    .range(0..=36500)
                    .suffix(" dias"),
            );
        });
        ui.small("Zero dias abrange todos os abertos de sua propriedade ou compartilhados neste projeto.");
        if ui.button("Revisar expiração por idade").clicked() {
            self.preview(shared, s, Intent::ExpireHandoffs { days: self.days });
        }
    }
    fn maintenance(&mut self, ui: &mut egui::Ui, shared: &Shared, s: &Scope) {
        egui::ComboBox::from_id_salt("maintenance-task")
            .selected_text(self.task.label())
            .show_ui(ui, |ui| {
                for task in [Task::Lint, Task::Curator, Task::Sweep, Task::Embeddings] {
                    ui.selectable_value(&mut self.task, task, task.label());
                }
            });
        if self.task == Task::Embeddings {
            ui.checkbox(
                &mut self.reembed,
                "Regenerar também embeddings já existentes",
            );
        }
        if ui.button("Preparar prévia sem aplicar").clicked() {
            self.preview(
                shared,
                s,
                Intent::Maintain {
                    task: self.task,
                    reembed: self.reembed,
                },
            );
        }
        ui.separator();
        ui.strong("Reconsolidação de sessão");
        field(ui, "ID exato da sessão", &mut self.session, 128);
        if ui.button("Preparar prévia de reconsolidação").clicked() {
            self.preview(
                shared,
                s,
                Intent::Consolidate {
                    session: self.session.trim().into(),
                },
            );
        }
        ui.separator();
        ui.strong("Feedback de conhecimento");
        field(ui, "Caminho da página", &mut self.path, 4096);
        egui::ComboBox::from_id_salt("feedback-signal")
            .selected_text(&self.signal)
            .show_ui(ui, |ui| {
                for (value, label) in [
                    ("helpful", "Útil"),
                    ("not_helpful", "Não ajudou"),
                    ("stale", "Desatualizada"),
                    ("wrong", "Incorreta"),
                ] {
                    ui.selectable_value(&mut self.signal, value.into(), label);
                }
            });
        field(ui, "Motivo", &mut self.reason, 500);
        if ui.button("Revisar feedback").clicked() {
            self.preview(
                shared,
                s,
                Intent::Feedback {
                    path: self.path.trim().into(),
                    signal: self.signal.clone(),
                    reason: self.reason.clone(),
                },
            );
        }
    }
    fn backups(&mut self, ui: &mut egui::Ui, shared: &Shared, s: &Scope) {
        ui.horizontal_wrapped(|ui| {
            ui.radio_value(&mut self.okf, false, "Backup global");
            ui.radio_value(&mut self.okf, true, "Exportar projeto (OKF)");
        });
        field(
            ui,
            "Arquivo de destino (caminho absoluto)",
            &mut self.destination,
            4096,
        );
        if ui.button("Revisar download privado").clicked() {
            self.preview(
                shared,
                s,
                Intent::Download {
                    destination: self.destination.trim().into(),
                    okf: self.okf,
                },
            );
        }
        ui.separator();
        ui.strong("Checkpoints da wiki");
        if ui.button("Listar últimos 100 checkpoints").clicked() {
            self.begin(shared, Work::Read(Read::Checkpoints));
        }
        field(ui, "Descrição do checkpoint", &mut self.message, 500);
        if ui.button("Revisar criação de checkpoint").clicked() {
            self.preview(
                shared,
                s,
                Intent::Checkpoint {
                    message: self.message.clone(),
                },
            );
        }
        ui.separator();
        ui.strong("Restaurar uma página");
        field(ui, "Caminho da página", &mut self.path, 4096);
        ui.checkbox(
            &mut self.git,
            "Usar checkpoint Git (desmarcado: ID da versão da página)",
        );
        field(
            ui,
            "ID completo da versão ou checkpoint",
            &mut self.version,
            128,
        );
        if ui.button("Preparar restauração").clicked() {
            self.preview(
                shared,
                s,
                Intent::Restore {
                    path: self.path.trim().into(),
                    version: self.version.trim().into(),
                    git: self.git,
                },
            );
        }
    }
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String, limit: usize) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(560.)
            .char_limit(limit),
    );
}
fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("—")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_connection_discards_preview_and_private_form_state() {
        let shared = Shared::unstarted(crate::model::Config::default());
        let mut ui = Management::new(shared.clone());
        ui.receipt = Some(Receipt {
            action: Action {
                scope: Scope {
                    workspace: "fixture".into(),
                    project: "demo".into(),
                },
                intent: Intent::Checkpoint {
                    message: "test".into(),
                },
            },
            generation: 0,
            at: std::time::Instant::now(),
        });
        ui.confirm = "GLOBAL".into();
        ui.reason = "private fixture".into();
        ui.identifier = "old-id".into();
        ui.result = Some(serde_json::json!({"private":"fixture"}));
        ui.reset();
        assert!(ui.receipt.is_none());
        assert!(ui.confirm.is_empty());
        assert!(ui.reason.is_empty());
        assert!(ui.identifier.is_empty());
        assert!(ui.result.is_none());
    }

    #[test]
    fn expired_preview_receipt_is_cleared_during_poll() {
        let shared = Shared::unstarted(crate::model::Config::default());
        let mut ui = Management::new(shared.clone());
        let receipt = |age: u64| Receipt {
            action: Action {
                scope: Scope {
                    workspace: "fixture".into(),
                    project: "demo".into(),
                },
                intent: Intent::Checkpoint {
                    message: "test".into(),
                },
            },
            generation: 0,
            at: std::time::Instant::now() - std::time::Duration::from_secs(age),
        };
        ui.receipt = Some(receipt(301));
        ui.result = Some(serde_json::json!({"preview": true}));
        ui.error = Some("stale error".into());
        ui.confirm = "GLOBAL".into();
        ui.poll(&shared);
        assert!(
            ui.receipt.is_none(),
            "expired receipt must be cleared during poll so form fields unlock"
        );
        assert!(
            ui.result.is_none(),
            "expired preview result must be discarded, not kept on screen"
        );
        assert!(ui.error.is_none(), "expiry must clear stale errors");
        assert!(ui.confirm.is_empty());
        assert!(
            ui.notice.contains("expirou"),
            "expiry notice expected, got {:?}",
            ui.notice
        );

        ui.receipt = Some(receipt(299));
        ui.result = Some(serde_json::json!({"preview": true}));
        ui.poll(&shared);
        assert!(
            ui.receipt.is_some() && ui.result.is_some(),
            "fresh preview must be kept"
        );
    }
}
