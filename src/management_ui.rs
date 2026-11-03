use crate::{
    client::Scope,
    console,
    maintenance::{Action, Intent, Read, Receipt, Task, Work, Worker},
    model::Project,
    monitor::Shared,
    widgets::{self, Tone},
};
use eframe::egui;
use serde_json::Value;
use std::sync::{Arc, atomic::Ordering};

/// As duas páginas que compartilham este painel (e o seu estado): Revisão
/// (Propostas, Handoffs) e Manutenção e backup (Manutenção, Backup e
/// restauração).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Section {
    Review,
    Maintenance,
}
impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Section::Review => "Revisão",
            Section::Maintenance => "Manutenção e backup",
        }
    }
    /// Abas da página; a primeira é a que abre.
    fn tabs(self) -> [(u8, &'static str); 2] {
        match self {
            Section::Review => [(0, "Propostas"), (1, "Handoffs")],
            Section::Maintenance => [(2, "Manutenção"), (3, "Backup e restauração")],
        }
    }
    /// A legenda sem projeto escolhido; backup e checkpoints são da
    /// Manutenção e backup.
    fn pick_project(self) -> &'static str {
        match self {
            Section::Review => "Selecione o projeto para continuar.",
            Section::Maintenance => {
                "Selecione o projeto para continuar. Backup e checkpoints indicam explicitamente quando abrangem toda a instalação."
            }
        }
    }
    fn of(tab: u8) -> Self {
        if tab < 2 {
            Section::Review
        } else {
            Section::Maintenance
        }
    }
}

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
    /// A prévia venceu e `poll` a descartou; só resta o aviso em `notice`.
    /// Prende o painel como um resultado na tela: sai com um clique.
    expired: bool,
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
            expired: false,
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
        self.expired = false;
    }
    /// Troca de aba ou de projeto: descarta prévia, resultado e os campos.
    fn discard(&mut self) {
        self.clear();
        self.identifier.clear();
        self.reason.clear();
        self.path.clear();
        self.version.clear();
        self.session.clear();
    }
    /// O que a troca de aba descartaria e o usuário ainda não viu ou
    /// decidiu: operação em andamento, resultado ou erro na tela, ou o aviso
    /// de que a prévia expirou. Uma prévia aguardando confirmação sempre vem
    /// com o seu resultado.
    fn holds(&self) -> bool {
        self.busy || self.result.is_some() || self.error.is_some() || self.expired
    }
    /// `section` aberta com o painel preso à outra página: avisa, oferece ir
    /// até lá e, sem operação em voo, descartar. Nada muda sem um clique.
    /// Devolve a página pedida pelo "Ir para".
    fn elsewhere(&mut self, ui: &mut egui::Ui, section: Section) -> Option<Section> {
        let owner = Section::of(self.tab);
        let tab = owner
            .tabs()
            .into_iter()
            .find(|&(id, _)| id == self.tab)
            .map_or("", |(_, label)| label);
        let place = format!("{} · {tab}", owner.label());
        let (bold, text) = if self.busy {
            (
                format!("Operação em andamento em {place}."),
                "Aguarde a resposta lá; esta página usa o mesmo painel.",
            )
        } else if self.receipt.is_some() {
            (
                format!("Prévia aguardando confirmação em {place}."),
                "Confirme-a lá ou descarte-a para usar esta página.",
            )
        } else if self.expired {
            (
                format!("Prévia expirada em {place}."),
                "Veja o aviso lá ou descarte-o para usar esta página.",
            )
        } else {
            (
                format!("Resultado ainda na tela em {place}."),
                "Confira-o lá ou descarte-o para usar esta página.",
            )
        };
        let go = widgets::strip(
            ui,
            Tone::Warn,
            &bold,
            text,
            Some(&format!("Ir para {}", owner.label())),
        )
        .is_some_and(|r| r.clicked());
        if !self.busy
            && ui
                .add(widgets::Button::secondary(&format!(
                    "Descartar e abrir {}",
                    section.label()
                )))
                .clicked()
        {
            self.discard();
            self.tab = section.tabs()[0].0;
        }
        go.then_some(owner)
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
            self.expired = true;
        }
    }
    /// A página `section`. Se o painel está em aba da outra página, troca
    /// para a primeira aba desta, a menos que isso descarte algo (`holds`):
    /// aí mostra o aviso e mantém a aba. Devolve a página pedida pelo aviso.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        shared: &Arc<Shared>,
        projects: &[Project],
        section: Section,
    ) -> Option<Section> {
        self.poll(shared);
        if Section::of(self.tab) != section {
            if self.holds() {
                return self.elsewhere(ui, section);
            }
            self.tab = section.tabs()[0].0;
            self.discard();
        }
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
                for (id, label) in section.tabs() {
                    ui.selectable_value(&mut self.tab, id, label);
                }
            });
        });
        if before != (self.scope.clone(), self.tab) {
            self.discard();
        }
        let Some(scope) = self.scope.clone() else {
            ui.small(section.pick_project());
            return None;
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
            ui.colored_label(crate::theme::palette(ui.visuals()).bad, e);
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
            ui.colored_label(
                crate::theme::palette(ui.visuals()).warn,
                receipt.action.warning(),
            );
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
        None
    }
    /// "Gerenciar em Revisão →" (Sessões e handoffs): a aba Handoffs, a menos
    /// que a troca descarte algo (`holds`); aí a aba fica e a página Revisão
    /// mostra o que prende o painel.
    pub fn open_handoffs(&mut self) {
        if self.tab != 1 && !self.holds() {
            self.tab = 1;
            self.discard();
        }
    }
    #[cfg(test)]
    pub(crate) fn tab(&self) -> u8 {
        self.tab
    }
    #[cfg(test)]
    pub(crate) fn set_tab(&mut self, tab: u8) {
        self.tab = tab;
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
    use crate::{
        test_support::{Mock, Response},
        widgets::tests::{click, context, node, nodes, rect_of, run},
    };
    use egui::accesskit::Role;
    use std::{
        sync::{Mutex, mpsc},
        time::{Duration, Instant},
    };

    fn scope() -> Scope {
        Scope {
            workspace: "fixture".into(),
            project: "demo".into(),
        }
    }
    fn receipt() -> Receipt {
        Receipt {
            action: Action {
                scope: scope(),
                intent: Intent::Checkpoint {
                    message: "test".into(),
                },
            },
            generation: 0,
            at: std::time::Instant::now(),
        }
    }
    /// O recibo de uma prévia pedida há `secs` segundos.
    fn aged(secs: u64) -> Receipt {
        Receipt {
            at: Instant::now() - Duration::from_secs(secs),
            ..receipt()
        }
    }
    /// Espera a resposta do trabalhador (até 3 s).
    fn settle(m: &mut Management, shared: &Shared) {
        let until = Instant::now() + Duration::from_secs(3);
        while m.busy {
            assert!(Instant::now() < until, "the worker never answered");
            std::thread::sleep(Duration::from_millis(2));
            m.poll(shared);
        }
    }

    #[test]
    fn each_page_shows_only_its_tabs_and_opens_on_its_first_one() {
        for (tab, section, want) in [
            (0, Section::Review, 0),
            (1, Section::Review, 1),
            (2, Section::Review, 0),
            (3, Section::Review, 0),
            (0, Section::Maintenance, 2),
            (1, Section::Maintenance, 2),
            (2, Section::Maintenance, 2),
            (3, Section::Maintenance, 3),
        ] {
            let shared = Shared::unstarted(crate::model::Config::default());
            let mut m = Management::new(shared.clone());
            m.tab = tab;
            m.identifier = "draft".into();
            let ctx = context(egui::Theme::Dark);
            let out = run(&ctx, vec![], |ui| {
                assert_eq!(m.show(ui, &shared, &[], section), None);
            });
            assert_eq!(m.tab, want, "tab {tab} on {section:?}");
            assert_eq!(
                m.identifier.is_empty(),
                tab != want,
                "a tab switch discards the fields, staying keeps them"
            );
            let (mine, theirs) = match section {
                Section::Review => (
                    ["Propostas", "Handoffs"],
                    ["Manutenção", "Backup e restauração"],
                ),
                Section::Maintenance => (
                    ["Manutenção", "Backup e restauração"],
                    ["Propostas", "Handoffs"],
                ),
            };
            for label in mine {
                node(&out, Role::Button, label);
            }
            for label in theirs {
                assert!(
                    nodes(&out, Role::Button, label).is_empty(),
                    "{label} on {section:?}"
                );
            }
        }
    }

    #[test]
    fn the_caption_without_a_project_is_the_one_of_each_page() {
        let review = "Selecione o projeto para continuar.";
        let maintenance = "Selecione o projeto para continuar. Backup e checkpoints indicam explicitamente quando abrangem toda a instalação.";
        for section in [Section::Review, Section::Maintenance] {
            // Uma página nova tem de entrar nesta tabela.
            let (mine, theirs) = match section {
                Section::Review => (review, maintenance),
                Section::Maintenance => (maintenance, review),
            };
            let shared = Shared::unstarted(crate::model::Config::default());
            let mut m = Management::new(shared.clone());
            let ctx = context(egui::Theme::Light);
            let out = run(&ctx, vec![], |ui| {
                m.show(ui, &shared, &[], section);
            });
            node(&out, Role::Label, mine);
            assert!(
                nodes(&out, Role::Label, theirs).is_empty(),
                "{section:?}: not the caption of the other page"
            );
            // Com o projeto escolhido, a legenda sai.
            m.scope = Some(scope());
            let out = run(&ctx, vec![], |ui| {
                m.show(ui, &shared, &[], section);
            });
            assert!(nodes(&out, Role::Label, mine).is_empty(), "{section:?}");
            node(&out, Role::Button, section.tabs()[0].1);
        }
    }

    #[test]
    fn the_other_page_never_discards_what_holds_the_panel() {
        // (estado que prende o painel em Revisão · Propostas, aviso, pode descartar?)
        for (case, warning, discard) in [
            (
                "busy",
                "Operação em andamento em Revisão · Propostas. Aguarde a resposta lá; esta página usa o mesmo painel.",
                false,
            ),
            (
                "receipt",
                "Prévia aguardando confirmação em Revisão · Propostas. Confirme-a lá ou descarte-a para usar esta página.",
                true,
            ),
            (
                "result",
                "Resultado ainda na tela em Revisão · Propostas. Confira-o lá ou descarte-o para usar esta página.",
                true,
            ),
            (
                "error",
                "Resultado ainda na tela em Revisão · Propostas. Confira-o lá ou descarte-o para usar esta página.",
                true,
            ),
            (
                "expired",
                "Prévia expirada em Revisão · Propostas. Veja o aviso lá ou descarte-o para usar esta página.",
                true,
            ),
        ] {
            let shared = Shared::unstarted(crate::model::Config::default());
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            match case {
                "busy" => m.busy = true,
                "receipt" => {
                    m.receipt = Some(receipt());
                    m.result = Some(serde_json::json!({"preview": true}));
                }
                "result" => m.result = Some(serde_json::json!([])),
                "expired" => {
                    m.receipt = Some(aged(301));
                    m.result = Some(serde_json::json!({"preview": true}));
                    // O app chama `poll` a cada frame, em qualquer página.
                    m.poll(&shared);
                    assert!(m.receipt.is_none() && m.result.is_none(), "poll expired it");
                }
                _ => m.error = Some("Recusado.".into()),
            }
            let ctx = context(egui::Theme::Light);
            let mut went = None;
            let mut add = |ui: &mut egui::Ui| {
                if let Some(section) = m.show(ui, &shared, &[], Section::Maintenance) {
                    went = Some(section);
                }
            };
            let out = run(&ctx, vec![], &mut add);
            node(&out, Role::Label, warning);
            assert!(
                nodes(&out, Role::Button, "Manutenção").is_empty(),
                "{case}: the Maintenance form stays closed"
            );
            let discard_button = "Descartar e abrir Manutenção e backup";
            assert_eq!(
                nodes(&out, Role::Button, discard_button).len(),
                usize::from(discard),
                "{case}"
            );
            let go = rect_of(&node(&out, Role::Button, "Ir para Revisão"));
            let discard_rect = nodes(&out, Role::Button, discard_button)
                .first()
                .map(rect_of);
            let discard_at = discard_rect.map(|r| r.center());
            // O botão vem abaixo da faixa inteira, não sobre ela.
            let strip = rect_of(&node(&out, Role::Label, warning));
            if let Some(button) = discard_rect {
                assert!(
                    button.top() > strip.bottom(),
                    "{case}: {button:?} under {strip:?}"
                );
            }
            click(&ctx, go.center(), &mut add);
            drop(add);
            assert_eq!(went, Some(Section::Review), "{case}");
            assert_eq!(m.tab, 0, "{case}: the tab is kept");
            assert!(m.holds(), "{case}: nothing was discarded");
            if let Some(at) = discard_at {
                let mut add = |ui: &mut egui::Ui| {
                    m.show(ui, &shared, &[], Section::Maintenance);
                };
                click(&ctx, at, &mut add);
                let out = run(&ctx, vec![], &mut add);
                drop(add);
                assert_eq!(m.tab, 2, "{case}: discarding opens Maintenance");
                assert!(!m.holds(), "{case}");
                node(&out, Role::Button, "Manutenção");
            }
        }
    }

    #[test]
    fn a_preview_that_expires_on_the_other_page_waits_for_a_click() {
        let shared = Shared::unstarted(crate::model::Config::default());
        let waiting = "Prévia aguardando confirmação em Revisão · Propostas. Confirme-a lá ou descarte-a para usar esta página.";
        let expired = "Prévia expirada em Revisão · Propostas. Veja o aviso lá ou descarte-o para usar esta página.";
        let notice = "A prévia expirou após 5 minutos. Prepare a operação novamente.";
        for go in [true, false] {
            let mut m = Management::new(shared.clone());
            m.scope = Some(scope());
            m.identifier = "draft-id".into();
            m.reason = "draft reason".into();
            m.receipt = Some(receipt());
            m.result = Some(serde_json::json!({"preview": true}));
            let ctx = context(egui::Theme::Dark);
            // Em Manutenção e backup, a prévia de Revisão ainda vale.
            let out = run(&ctx, vec![], |ui| {
                m.show(ui, &shared, &[], Section::Maintenance);
            });
            node(&out, Role::Label, waiting);
            // Passam 5 minutos; o app chama `poll` a cada frame.
            m.receipt.as_mut().unwrap().at = Instant::now() - Duration::from_secs(301);
            m.poll(&shared);
            assert!(m.receipt.is_none(), "go {go}: poll expired the preview");
            assert_eq!(m.notice, notice, "go {go}");
            // O frame seguinte da outra página: a faixa, sem trocar de aba.
            let out = run(&ctx, vec![], |ui| {
                m.show(ui, &shared, &[], Section::Maintenance);
            });
            node(&out, Role::Label, expired);
            assert!(
                nodes(&out, Role::Label, waiting).is_empty(),
                "go {go}: not the strip of a live preview"
            );
            assert!(
                nodes(&out, Role::Button, "Manutenção").is_empty(),
                "go {go}: the Maintenance form stays closed"
            );
            assert_eq!(m.tab, 0, "go {go}: the tab is kept");
            assert_eq!(
                (m.identifier.as_str(), m.reason.as_str(), m.notice.as_str()),
                ("draft-id", "draft reason", notice),
                "go {go}: nothing was discarded"
            );
            if go {
                let at = rect_of(&node(&out, Role::Button, "Ir para Revisão")).center();
                let mut went = None;
                click(&ctx, at, |ui| {
                    if let Some(section) = m.show(ui, &shared, &[], Section::Maintenance) {
                        went = Some(section);
                    }
                });
                assert_eq!(went, Some(Section::Review));
                // Em Revisão, o aviso e os campos digitados estão lá.
                let out = run(&ctx, vec![], |ui| {
                    assert_eq!(m.show(ui, &shared, &[], Section::Review), None);
                });
                assert_eq!(m.tab, 0);
                node(&out, Role::Label, notice);
                assert_eq!(nodes(&out, Role::TextInput, "draft-id").len(), 1);
                assert_eq!(nodes(&out, Role::TextInput, "draft reason").len(), 1);
                assert!(
                    nodes(&out, Role::Label, expired).is_empty(),
                    "on its own page the notice is the usual one"
                );
            } else {
                let at = rect_of(&node(
                    &out,
                    Role::Button,
                    "Descartar e abrir Manutenção e backup",
                ))
                .center();
                click(&ctx, at, |ui| {
                    m.show(ui, &shared, &[], Section::Maintenance);
                });
                let out = run(&ctx, vec![], |ui| {
                    m.show(ui, &shared, &[], Section::Maintenance);
                });
                assert_eq!(m.tab, 2, "discarding opens Maintenance");
                assert_eq!(
                    (m.identifier.as_str(), m.reason.as_str(), m.notice.as_str()),
                    ("", "", ""),
                    "the click discarded it"
                );
                assert!(!m.holds());
                node(&out, Role::Button, "Manutenção");
                assert!(nodes(&out, Role::Label, expired).is_empty());
            }
        }
    }

    #[test]
    fn an_operation_answered_while_on_the_other_page_stays_with_its_own_page() {
        // O servidor sintético segura a resposta até o teste liberar.
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let mock = Mock::new(move |r| {
            if r.path.starts_with("/admin/pending-writes") {
                let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(5));
                Response::json(
                    200,
                    serde_json::json!([{"id": "p-1", "title": "Proposta sintética", "status": "pending"}]),
                )
            } else {
                Response::json(404, serde_json::json!({}))
            }
        });
        let shared = Shared::unstarted(mock.config());
        let mut m = Management::new(shared.clone());
        m.scope = Some(scope());
        m.begin(
            &shared,
            Work::Read(Read::Proposals {
                scope: scope(),
                status: "pending".into(),
            }),
        );
        assert!(m.busy, "precondition: the request is in flight");
        let ctx = context(egui::Theme::Dark);
        // O usuário abre Manutenção e backup com a consulta em voo.
        let out = run(&ctx, vec![], |ui| {
            m.show(ui, &shared, &[], Section::Maintenance);
        });
        node(
            &out,
            Role::Label,
            "Operação em andamento em Revisão · Propostas. Aguarde a resposta lá; esta página usa o mesmo painel.",
        );
        assert_eq!(m.tab, 0);
        release.send(()).unwrap();
        settle(&mut m, &shared);
        assert!(m.result.is_some(), "the answer was accepted");
        let out = run(&ctx, vec![], |ui| {
            m.show(ui, &shared, &[], Section::Maintenance);
        });
        assert_eq!(m.tab, 0, "the answer does not move to Maintenance");
        node(
            &out,
            Role::Label,
            "Resultado ainda na tela em Revisão · Propostas. Confira-o lá ou descarte-o para usar esta página.",
        );
        assert!(nodes(&out, Role::Label, "Proposta sintética").is_empty());
        // De volta a Revisão, o resultado está lá, na aba dele.
        let out = run(&ctx, vec![], |ui| {
            m.show(ui, &shared, &[], Section::Review);
        });
        node(&out, Role::Label, "Proposta sintética");
        assert_eq!(m.tab, 0);
        drop(release);
        drop(mock);
    }
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
