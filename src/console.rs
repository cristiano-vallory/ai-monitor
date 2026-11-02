//! Native read-only inspector. Text from memory is displayed as data, never executed.
use crate::{
    client::Scope,
    health,
    model::{Project, bytes},
    monitor::Shared,
    queries::{Query, Worker},
};
use chrono::{Local, Utc};
use eframe::egui::{self, Color32, RichText};
use serde_json::Value;
use std::sync::{Arc, atomic::Ordering};

const ACCENT: Color32 = Color32::from_rgb(102, 224, 184);
const WARN: Color32 = Color32::from_rgb(244, 190, 103);
#[derive(Default, Clone, Copy, PartialEq)]
enum Tab {
    #[default]
    Health,
    Sessions,
    Knowledge,
}
pub struct Console {
    worker: Worker,
    ticket: u64,
    busy: bool,
    tab: Tab,
    scope: Option<Scope>,
    result: Option<(Query, Value)>,
    error: Option<String>,
    at: Option<chrono::DateTime<Utc>>,
    search: String,
    observation_search: String,
    kind: String,
    global: bool,
    include_open: bool,
    from: String,
    to: String,
}
impl Console {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            worker: Worker::new(shared),
            ticket: 0,
            busy: false,
            tab: Tab::default(),
            scope: None,
            result: None,
            error: None,
            at: None,
            search: String::new(),
            observation_search: String::new(),
            kind: String::new(),
            global: false,
            include_open: true,
            from: String::new(),
            to: String::new(),
        }
    }
    pub fn reset(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.scope = None;
        self.result = None;
        self.error = None;
        self.at = None;
        self.search.clear();
        self.observation_search.clear();
        self.kind.clear();
        self.from.clear();
        self.to.clear();
    }
    fn clear_result(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.result = None;
        self.error = None;
        self.at = None;
        self.from.clear();
        self.to.clear();
    }
    fn begin(&mut self, shared: &Shared, query: Query) {
        if self.busy {
            return;
        }
        self.ticket = self.ticket.wrapping_add(1);
        self.error = None;
        match self.worker.submit(self.ticket, shared, query) {
            Ok(()) => {
                self.busy = true;
                self.result = None;
                self.at = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn poll(&mut self, shared: &Shared) {
        while let Ok(reply) = self.worker.replies.try_recv() {
            self.busy = false;
            if reply.ticket != self.ticket
                || reply.generation != shared.generation.load(Ordering::Acquire)
            {
                continue;
            }
            match reply.result {
                Ok(v) => {
                    self.at = Some(Utc::now());
                    self.result = Some((reply.query, v));
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        shared: &Arc<Shared>,
        projects: &[Project],
        snapshot: &health::Snapshot,
    ) {
        self.poll(shared);
        let previous = self.tab;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Health, "Saúde");
            ui.selectable_value(&mut self.tab, Tab::Sessions, "Sessões detalhadas");
            ui.selectable_value(&mut self.tab, Tab::Knowledge, "Conhecimento");
        });
        if previous != self.tab {
            self.clear_result();
        }
        ui.separator();
        if self.tab == Tab::Health {
            show_health(ui, snapshot);
            return;
        }
        let old = self.scope.clone();
        egui::ComboBox::from_id_salt("central-scope")
            .width(440.)
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
                    ui.selectable_value(&mut self.scope, Some(s), label);
                }
            });
        if old != self.scope {
            self.clear_result();
            self.observation_search.clear();
            self.kind.clear();
        }
        let Some(scope) = self.scope.clone() else {
            ui.label("Escolha o projeto cujos dados você deseja consultar.");
            return;
        };
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Consultando…");
            });
        }
        if let Some(e) = &self.error {
            ui.colored_label(WARN, e);
        }
        if let Some(at) = self.at {
            ui.small(format!(
                "Consulta: {} · conteúdo restrito às permissões da conexão",
                at.with_timezone(&Local).format("%H:%M:%S")
            ));
        }
        ui.add_enabled_ui(!self.busy, |ui| match self.tab {
            Tab::Sessions => self.session_controls(ui, shared, &scope),
            Tab::Knowledge => self.knowledge_controls(ui, shared, &scope),
            Tab::Health => {}
        });
        ui.separator();
        if let Some((query, data)) = self.result.clone() {
            self.render(ui, shared, &query, &data);
        } else if !self.busy && self.error.is_none() {
            ui.label("Use os controles acima para carregar os dados.");
        }
    }
    fn session_controls(&mut self, ui: &mut egui::Ui, shared: &Shared, scope: &Scope) {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.include_open, "Incluir abertas");
            if ui.button("Carregar sessões").clicked() {
                self.observation_search.clear();
                self.kind.clear();
                self.begin(
                    shared,
                    Query::Sessions {
                        scope: scope.clone(),
                        offset: 0,
                        include_open: self.include_open,
                    },
                );
            }
            for (state, label) in [
                ("open", "Handoffs abertos"),
                ("accepted", "Aceitos"),
                ("expired", "Expirados"),
            ] {
                if ui.button(label).clicked() {
                    self.begin(
                        shared,
                        Query::Handoffs {
                            scope: scope.clone(),
                            state: state.into(),
                        },
                    );
                }
            }
        });
        ui.small("Sessão aberta não comprova um processo ativo. Consultar esta tela não aceita handoffs.");
    }
    fn knowledge_controls(&mut self, ui: &mut egui::Ui, shared: &Shared, scope: &Scope) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Páginas").clicked() {
                self.begin(shared, Query::Pages(scope.clone()));
            }
            if ui.button("Resumo do projeto").clicked() {
                self.begin(shared, Query::Briefing(scope.clone()));
            }
            if ui.button("Relações do projeto").clicked() {
                self.begin(shared, Query::Graph(scope.clone()));
            }
            if ui.button("Relações entre projetos").clicked() {
                self.begin(shared, Query::CrossGraph);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Pesquisar conhecimento…")
                    .char_limit(1024),
            );
            ui.checkbox(&mut self.global, "Todos os projetos autorizados");
            if ui.button("Buscar").clicked() {
                self.begin(
                    shared,
                    Query::Search {
                        scope: (!self.global).then(|| scope.clone()),
                        text: self.search.clone(),
                    },
                );
            }
        });
        ui.small("Pesquisa com escopo pode usar embeddings/reranking do servidor. As páginas são exibidas como texto histórico.");
    }
    fn render(&mut self, ui: &mut egui::Ui, shared: &Shared, query: &Query, data: &Value) {
        match query {
            Query::Sessions {
                scope,
                offset,
                include_open,
            } => {
                let rows = array(&data["sessions"]);
                self.pager(ui, shared, *offset, rows.len(), None, |n| Query::Sessions {
                    scope: scope.clone(),
                    offset: n,
                    include_open: *include_open,
                });
                if rows.is_empty() {
                    ui.label("Nenhuma sessão nesta página.");
                }
                for row in rows {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(text(row, "agent_kind"));
                            ui.label(if row["ended_at"].is_null() {
                                "Aberta"
                            } else {
                                "Encerrada"
                            });
                            ui.small(text(row, "started_at"));
                            if ui
                                .add_enabled(!self.busy, egui::Button::new("Ver eventos"))
                                .clicked()
                            {
                                self.observation_search.clear();
                                self.kind.clear();
                                self.begin(
                                    shared,
                                    Query::Observations {
                                        scope: scope.clone(),
                                        session: text(row, "session_id").into(),
                                        offset: 0,
                                        text: String::new(),
                                        kind: String::new(),
                                    },
                                );
                            }
                        });
                        ui.small(text(row, "session_id"));
                        ui.label(format!(
                            "{} eventos · consolidação: {} · tentativas: {}",
                            number(&row["observation_count"]),
                            text(&row["consolidation"], "state"),
                            number(&row["consolidation"]["attempts"])
                        ));
                        show_usage(ui, &row["usage"]);
                    });
                }
            }
            Query::Observations {
                scope,
                session,
                offset,
                ..
            } => {
                ui.strong(format!("Eventos da sessão {session}"));
                show_usage(ui, &data["session"]["usage"]);
                ui.add_enabled_ui(!self.busy, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.observation_search)
                                .hint_text("Pesquisar eventos…")
                                .char_limit(1024),
                        );
                        egui::ComboBox::from_id_salt("event-kind")
                            .selected_text(if self.kind.is_empty() {
                                "Todos os tipos"
                            } else {
                                &self.kind
                            })
                            .show_ui(ui, |ui| {
                                for k in [
                                    "",
                                    "session-start",
                                    "user-prompt",
                                    "pre-tool-use",
                                    "post-tool-use",
                                    "stop",
                                    "pre-compact",
                                    "post-compaction",
                                    "session-end",
                                    "other",
                                ] {
                                    ui.selectable_value(
                                        &mut self.kind,
                                        k.into(),
                                        if k.is_empty() { "Todos os tipos" } else { k },
                                    );
                                }
                            });
                        if ui.button("Aplicar filtro").clicked() {
                            self.begin(
                                shared,
                                Query::Observations {
                                    scope: scope.clone(),
                                    session: session.clone(),
                                    offset: 0,
                                    text: self.observation_search.clone(),
                                    kind: self.kind.clone(),
                                },
                            );
                        }
                    });
                });
                if let Query::Observations { text, kind, .. } = query {
                    self.pager(
                        ui,
                        shared,
                        *offset,
                        array(&data["observations"]).len(),
                        data["total"].as_u64(),
                        |n| Query::Observations {
                            scope: scope.clone(),
                            session: session.clone(),
                            offset: n,
                            text: text.clone(),
                            kind: kind.clone(),
                        },
                    );
                }
                for row in array(&data["observations"]) {
                    egui::CollapsingHeader::new(format!(
                        "{} · {} · {}",
                        text(row, "created_at"),
                        text(row, "kind"),
                        text(row, "title")
                    ))
                    .id_salt(text(row, "id"))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(text(row, "body")).wrap().selectable(true));
                    });
                }
                if data["elided_other_scope"].as_u64().is_some_and(|n| n > 0) {
                    ui.small("Esta sessão também deixou eventos em outros projetos; apenas este escopo é exibido.");
                }
            }
            Query::Pages(scope) => self.page_list(ui, shared, scope, array(data)),
            Query::Search { scope, .. } => {
                ui.small("Até 100 resultados. Refine a pesquisa para reduzir o conjunto.");
                for row in array(data) {
                    let s = scope.clone().unwrap_or_else(|| Scope {
                        workspace: text(row, "workspace").into(),
                        project: text(row, "project").into(),
                    });
                    self.page_list(ui, shared, &s, std::slice::from_ref(row));
                    ui.label(
                        text(row, "snippet")
                            .replace("<mark>", "")
                            .replace("</mark>", ""),
                    );
                }
            }
            Query::Page { scope, path } => {
                ui.heading(text(data, "title"));
                ui.small(format!(
                    "{} · {} · versão {}",
                    scope.label(),
                    path,
                    text(data, "id")
                ));
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Histórico e comparação"))
                    .clicked()
                {
                    self.from.clear();
                    self.to.clear();
                    self.begin(
                        shared,
                        Query::History {
                            scope: scope.clone(),
                            path: path.clone(),
                            offset: 0,
                            from: String::new(),
                            to: String::new(),
                        },
                    );
                }
                if text(data, "body_markdown").len() > 100_000 {
                    ui.colored_label(
                        WARN,
                        "Prévia limitada a 100 KiB; o conteúdo no servidor permanece completo.",
                    );
                }
                ui.add(
                    egui::Label::new(bounded_text(text(data, "body_markdown"), 100_000))
                        .wrap()
                        .selectable(true),
                );
                for (key, label) in [("links", "Referências"), ("backlinks", "Referenciada por")] {
                    ui.separator();
                    ui.strong(label);
                    ui.push_id(key, |ui| {
                        self.page_list(ui, shared, scope, array(&data[key]))
                    });
                }
            }
            Query::History {
                scope,
                path,
                offset,
                from,
                to,
            } => {
                ui.heading(format!("Histórico: {path}"));
                ui.horizontal_wrapped(|ui| {
                    for (label, selected) in
                        [("Comparar de", &mut self.from), ("Até", &mut self.to)]
                    {
                        egui::ComboBox::from_id_salt(label)
                            .selected_text(if selected.is_empty() {
                                label
                            } else {
                                selected.as_str()
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(selected, String::new(), "Automático");
                                for row in array(&data["versions"]) {
                                    ui.selectable_value(
                                        selected,
                                        text(row, "id").into(),
                                        format!(
                                            "{} · {}",
                                            text(row, "created_at"),
                                            text(row, "id")
                                        ),
                                    );
                                }
                            });
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Comparar"))
                        .clicked()
                    {
                        self.begin(
                            shared,
                            Query::History {
                                scope: scope.clone(),
                                path: path.clone(),
                                offset: *offset,
                                from: self.from.clone(),
                                to: self.to.clone(),
                            },
                        );
                    }
                });
                self.pager(
                    ui,
                    shared,
                    *offset,
                    array(&data["versions"]).len(),
                    data["total"].as_u64(),
                    |n| Query::History {
                        scope: scope.clone(),
                        path: path.clone(),
                        offset: n,
                        from: from.clone(),
                        to: to.clone(),
                    },
                );
                for row in array(&data["versions"]) {
                    ui.label(format!(
                        "{} · {} · {} caracteres",
                        text(row, "created_at"),
                        text(row, "id"),
                        number(&row["body_chars"])
                    ));
                }
                let diff = &data["diff"];
                ui.separator();
                ui.strong(format!("Comparação: {}", text(diff, "status")));
                if diff["truncated"].as_bool() == Some(true) || diff["status"] == "too_large" {
                    ui.colored_label(
                        WARN,
                        "Comparação parcial ou grande demais. Consulte versões menores.",
                    );
                }
                for hunk in array(&diff["hunks"]) {
                    for line in array(&hunk["lines"]) {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{} {}",
                                    text(line, "op"),
                                    text(line, "text")
                                ))
                                .monospace()
                                .color(match text(line, "op") {
                                    "+" => ACCENT,
                                    "-" => WARN,
                                    _ => ui.visuals().text_color(),
                                }),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    }
                }
            }
            Query::Briefing(scope) => {
                ui.heading("Resumo do projeto");
                count_grid(
                    ui,
                    "briefing-counts",
                    &data["counts"],
                    &[
                        ("pages_latest", "Páginas atuais"),
                        ("sessions", "Sessões"),
                        ("observations", "Eventos"),
                    ],
                );
                ui.label(format!(
                    "Handoffs pendentes: {} · último evento: {}",
                    number(&data["pending_handoff_count"]),
                    text(data, "last_observation_at")
                ));
                for key in ["activity_7d", "activity_30d"] {
                    count_grid(
                        ui,
                        key,
                        &data[key],
                        &[
                            ("sessions", "Sessões"),
                            ("observations", "Eventos"),
                            ("pages_updated", "Páginas atualizadas"),
                        ],
                    );
                }
                for (key, label) in [("rules", "Regras"), ("recent_pages", "Páginas recentes")] {
                    ui.strong(label);
                    ui.push_id(key, |ui| {
                        self.page_list(ui, shared, scope, array(&data[key]))
                    });
                }
            }
            Query::Graph(scope) => {
                if data["truncated"] == true {
                    ui.colored_label(WARN, "Grafo parcial: o servidor limita a 120 páginas.");
                }
                draw_graph(ui, data);
                self.page_list(ui, shared, scope, array(&data["nodes"]));
            }
            Query::CrossGraph => {
                ui.heading("Relações entre projetos autorizados");
                let edges = array(&data["edges"]);
                let range = local_page(ui, edges.len());
                for edge in &edges[range] {
                    ui.label(format!(
                        "{} / {} : {} → {} / {} : {}",
                        text(edge, "from_workspace"),
                        text(edge, "from_project"),
                        text(edge, "from_path"),
                        text(edge, "to_workspace"),
                        text(edge, "to_project"),
                        text(edge, "to_path")
                    ));
                }
            }
            Query::Handoffs { .. } => {
                ui.small("Lista limitada a 200 registros; nenhum handoff é consumido.");
                for row in array(&data["handoffs"]) {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.strong(format!("{} · {}", text(row, "state"), text(row, "agent")));
                        ui.small(format!("{} · {}", text(row, "at"), text(row, "id")));
                        if row["redacted"] == true {
                            ui.label("Resumo restrito pelas permissões.");
                        } else {
                            ui.add(
                                egui::Label::new(text(row, "summary"))
                                    .wrap()
                                    .selectable(true),
                            );
                            for step in array(&row["next_steps"]) {
                                if let Some(step) = step.as_str() {
                                    ui.label(format!("• {step}"));
                                }
                            }
                        }
                    });
                }
            }
        }
    }
    fn page_list(&mut self, ui: &mut egui::Ui, shared: &Shared, scope: &Scope, rows: &[Value]) {
        if rows.is_empty() {
            ui.small("Nenhuma página.");
        }
        let range = local_page(ui, rows.len());
        for row in &rows[range] {
            let target = related_scope(scope, row);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new(text(row, "title")))
                    .clicked()
                {
                    self.begin(
                        shared,
                        Query::Page {
                            scope: target.clone(),
                            path: text(row, "path").into(),
                        },
                    );
                }
                ui.small(format!("{} · {}", target.label(), text(row, "path")));
            });
        }
    }
    fn pager(
        &mut self,
        ui: &mut egui::Ui,
        shared: &Shared,
        offset: usize,
        count: usize,
        total: Option<u64>,
        query: impl Fn(usize) -> Query,
    ) {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "Página {} · {} registros{}",
                offset / 50 + 1,
                count,
                total.map(|n| format!(" de {n}")).unwrap_or_default()
            ));
            if ui
                .add_enabled(!self.busy && offset > 0, egui::Button::new("Anterior"))
                .clicked()
            {
                self.begin(shared, query(offset.saturating_sub(50)));
            }
            let more = total
                .map(|n| offset.saturating_add(count) < n as usize)
                .unwrap_or(count == 50);
            if ui
                .add_enabled(!self.busy && more, egui::Button::new("Próxima"))
                .clicked()
            {
                self.begin(shared, query(offset.saturating_add(50)));
            }
        });
    }
}
fn related_scope(fallback: &Scope, row: &Value) -> Scope {
    match (row["workspace"].as_str(), row["project"].as_str()) {
        (Some(workspace), Some(project)) => Scope {
            workspace: workspace.into(),
            project: project.into(),
        },
        _ => fallback.clone(),
    }
}
fn bounded_text(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
fn local_page(ui: &mut egui::Ui, total: usize) -> std::ops::Range<usize> {
    let id = ui.id().with("local-page");
    let mut offset = ui.data(|d| d.get_temp::<usize>(id).unwrap_or(0));
    offset = offset.min(total.saturating_sub(1) / 100 * 100);
    if total > 100 {
        ui.horizontal(|ui| {
            ui.small(format!(
                "{}–{} de {total}",
                offset + 1,
                (offset + 100).min(total)
            ));
            if ui
                .add_enabled(offset > 0, egui::Button::new("← 100"))
                .clicked()
            {
                offset = offset.saturating_sub(100);
            }
            if ui
                .add_enabled(offset + 100 < total, egui::Button::new("100 →"))
                .clicked()
            {
                offset += 100;
            }
        });
    }
    ui.data_mut(|d| d.insert_temp(id, offset));
    offset..(offset + 100).min(total)
}
fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("—")
}
fn number(v: &Value) -> String {
    v.as_u64().map(|n| n.to_string()).unwrap_or("—".into())
}
fn show_usage(ui: &mut egui::Ui, usage: &Value) {
    if usage.is_null() {
        ui.small("Tokens: não reportados pelo cliente.");
        return;
    }
    ui.label(format!(
        "Tokens · entrada {} · saída {} · cache gravado {} · cache lido {} · modelo {}",
        number(&usage["input_tokens"]),
        number(&usage["output_tokens"]),
        number(&usage["cache_write_tokens"]),
        number(&usage["cache_read_tokens"]),
        text(usage, "model")
    ));
    ui.small("Totais reportados da sessão; não são valores de faturamento nem rateio por projeto/modelo.");
}
fn count_grid(ui: &mut egui::Ui, id: &str, data: &Value, fields: &[(&str, &str)]) {
    egui::Grid::new(id).num_columns(2).show(ui, |ui| {
        for (key, label) in fields {
            ui.label(*label);
            ui.strong(number(&data[*key]));
            ui.end_row();
        }
    });
}
fn show_health(ui: &mut egui::Ui, s: &health::Snapshot) {
    ui.heading("Saúde interna do ai-memory");
    ui.label(s.menu_summary(Utc::now()));
    ui.small("Coleta a cada 30 s após a resposta, mesmo com a janela fechada. Alertas também aparecem como ! na barra.");
    if let Some(at) = s.at {
        ui.small(format!(
            "Última tentativa: {}",
            at.with_timezone(&Local).format("%H:%M:%S")
        ));
    }
    if let Some(i) = &s.identity {
        ui.label(format!(
            "Versão {} · acesso {} · distingue operadores: {}",
            i.version, i.level, i.distinguishes_operators
        ));
    }
    if let Some(v) = &s.status {
        for alert in s.alerts() {
            ui.colored_label(WARN, alert);
        }
        ui.separator();
        ui.strong("Captura e consolidação");
        count_grid(
            ui,
            "health-counts",
            &v["counts"],
            &[
                ("pages_latest", "Páginas atuais"),
                ("sessions", "Sessões"),
                ("observations", "Eventos"),
            ],
        );
        count_grid(
            ui,
            "health-consolidation",
            &v["consolidation"],
            &[
                ("pending", "Consolidações pendentes"),
                ("running", "Em execução"),
                ("completed", "Concluídas"),
                ("failed", "Com falha"),
            ],
        );
        ui.label(format!(
            "Fila de escrita: {} / {}",
            number(&v["write_queue"][0]),
            number(&v["write_queue"][1])
        ));
        ui.separator();
        ui.strong("Providers");
        for role in ["llm", "embedding"] {
            let p = &v["providers"][role];
            ui.label(format!(
                "{role}: {} · {} · {}",
                text(p, "provider"),
                text(p, "model"),
                text(p, "status")
            ));
        }
        egui::CollapsingHeader::new("Uso dos providers por atividade").show(ui, |ui| {
            ui.small("Contadores passivos desde o início do servidor. Estimativas e tokens reportados são medidas distintas.");
            for row in array(&v["providers"]["llm_workloads"]).iter().take(100) {
                ui.push_id(text(row, "workload"), |ui| {
                    ui.strong(text(row, "workload"));
                    count_grid(ui, "workload", row, &[("calls", "Chamadas"), ("errors", "Falhas"), ("reported_input_tokens", "Entrada reportada"), ("reported_output_tokens", "Saída reportada"), ("estimated_input_tokens", "Entrada estimada"), ("estimated_output_tokens", "Saída estimada")]);
                });
            }
        });
        ui.separator();
        ui.strong("Armazenamento e índices");
        for (key, label) in [
            ("database_bytes", "Banco"),
            ("reclaimable_bytes", "Potencial recuperável"),
            ("data_dir_free_bytes", "Disco livre no servidor"),
        ] {
            ui.label(format!(
                "{label}: {}",
                v["storage"][key].as_u64().map(bytes).unwrap_or("—".into())
            ));
        }
        count_grid(
            ui,
            "health-derived",
            &v["derived"],
            &[
                (
                    "latest_pages_missing_embeddings",
                    "Páginas sem embedding compatível",
                ),
                ("embed_failures_unresolved", "Embeddings com falha"),
                ("unresolved_links_from_latest_pages", "Links não resolvidos"),
                ("pages_rows", "Versões de página"),
                ("pages_fts_rows", "Versões no índice textual"),
            ],
        );
        ui.small("Campo ausente = desconhecido. Ausência de embeddings pode ser normal quando o recurso não está configurado.");
    }
    for e in &s.auxiliary_errors {
        ui.colored_label(WARN, e);
    }
    egui::CollapsingHeader::new("Métricas acumuladas desde o início do servidor").show(ui, |ui| {
        for m in &s.metrics {
            ui.horizontal_wrapped(|ui| {
                ui.monospace(&m.series);
                ui.label(m.value.to_string());
            });
        }
        if s.metrics.is_empty() {
            ui.label("Nenhuma métrica disponível.");
        }
    });
    if let Some(features) = &s.features {
        egui::CollapsingHeader::new("Recursos informados pelo servidor").show(ui, |ui| {
            show_values(ui, features, 0);
        });
    }
}
pub(crate) fn show_values(ui: &mut egui::Ui, value: &Value, depth: usize) {
    if depth > 10 {
        ui.small(
            "Conteúdo aninhado além do limite de visualização; consulte a fonte antes de aprovar.",
        );
        return;
    }
    match value {
        Value::Object(map) => {
            if map.len() > 200 {
                ui.colored_label(Color32::from_rgb(244, 190, 103), "Objeto parcial: somente os primeiros 200 campos. Consulte a fonte completa antes de decidir.");
            }
            for (key, v) in map.iter().take(200) {
                if v.is_object() || v.is_array() {
                    ui.push_id(key, |ui| {
                        egui::CollapsingHeader::new(key)
                            .default_open(depth < 2)
                            .show(ui, |ui| show_values(ui, v, depth + 1));
                    });
                } else {
                    let value = v
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string());
                    ui.add(
                        egui::Label::new(format!("{key}: {}", bounded_text(&value, 100_000)))
                            .wrap()
                            .selectable(true),
                    );
                    if value.len() > 100_000 {
                        ui.small("Conteúdo exibido parcialmente (100 KiB).");
                    }
                }
            }
        }
        Value::Array(rows) => {
            let range = local_page(ui, rows.len());
            for i in range {
                ui.push_id(i, |ui| {
                    ui.separator();
                    show_values(ui, &rows[i], depth + 1);
                });
            }
        }
        v => {
            let label = v
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string());
            if label.len() > 100_000 {
                ui.small("Conteúdo exibido parcialmente (100 KiB).");
            }
            ui.add(
                egui::Label::new(bounded_text(&label, 100_000))
                    .wrap()
                    .selectable(true),
            );
        }
    }
}
fn draw_graph(ui: &mut egui::Ui, data: &Value) {
    let nodes = array(&data["nodes"]);
    if nodes.is_empty() {
        ui.label("Nenhuma relação disponível.");
        return;
    }
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(650.), 320.),
        egui::Sense::hover(),
    );
    let positions: Vec<_> = (0..nodes.len())
        .map(|i| {
            let angle = i as f32 / nodes.len() as f32 * std::f32::consts::TAU;
            rect.center() + egui::vec2(angle.cos() * rect.width() * 0.4, angle.sin() * 125.)
        })
        .collect();
    for e in array(&data["edges"]) {
        let from = nodes.iter().position(|n| n["path"] == e["from_path"]);
        let to = nodes.iter().position(|n| n["path"] == e["to_path"]);
        if let (Some(a), Some(b)) = (from, to) {
            ui.painter().line_segment(
                [positions[a], positions[b]],
                egui::Stroke::new(1.0_f32, Color32::from_gray(65)),
            );
        }
    }
    for (i, pos) in positions.iter().enumerate() {
        ui.painter().circle_filled(*pos, 5., ACCENT);
        ui.interact(
            egui::Rect::from_center_size(*pos, egui::vec2(16., 16.)),
            ui.id().with(("node", i)),
            egui::Sense::hover(),
        )
        .on_hover_text(format!(
            "{}\n{}",
            text(&nodes[i], "title"),
            text(&nodes[i], "path")
        ));
    }
    ui.small(
        "Passe o mouse sobre os pontos para identificar páginas. Abra uma página na lista abaixo.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn cross_project_reference_uses_its_actual_coordinates() {
        let current = Scope {
            workspace: "a".into(),
            project: "b".into(),
        };
        let target = related_scope(&current, &json!({"workspace":"other", "project":"repo"}));
        assert_eq!(target.label(), "other / repo");
        assert_eq!(related_scope(&current, &json!({"path":"x.md"})), current);
    }
    #[test]
    fn preview_limit_preserves_unicode_and_does_not_change_source() {
        let source = "é".repeat(60_000);
        assert_eq!(bounded_text(&source, 99_999).len(), 99_998);
        assert_eq!(source.len(), 120_000);
    }
    #[test]
    fn inspector_renders_missing_optional_data_without_panicking() {
        let shared = Shared::unstarted(crate::model::Config::default());
        let mut inspector = Console::new(shared.clone());
        let scope = Scope {
            workspace: "fixture".into(),
            project: "demo".into(),
        };
        let queries = [
            Query::Sessions {
                scope: scope.clone(),
                offset: 0,
                include_open: true,
            },
            Query::Observations {
                scope: scope.clone(),
                session: "s1".into(),
                offset: 0,
                text: String::new(),
                kind: String::new(),
            },
            Query::Pages(scope.clone()),
            Query::Page {
                scope: scope.clone(),
                path: "p.md".into(),
            },
            Query::History {
                scope: scope.clone(),
                path: "p.md".into(),
                offset: 0,
                from: String::new(),
                to: String::new(),
            },
            Query::Briefing(scope.clone()),
            Query::Graph(scope.clone()),
            Query::CrossGraph,
            Query::Handoffs {
                scope,
                state: "open".into(),
            },
        ];
        let ctx = egui::Context::default();
        for query in queries {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| inspector.render(ui, &shared, &query, &json!({})));
            });
        }
        inspector.result = Some((Query::CrossGraph, json!({"edges":[]})));
        inspector.search = "private fixture".into();
        inspector.reset();
        assert!(inspector.result.is_none());
        assert!(inspector.search.is_empty());
    }
}
