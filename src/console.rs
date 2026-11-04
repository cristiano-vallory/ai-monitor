//! Native read-only inspector. Text from memory is displayed as data, never executed.
use crate::{
    client::Scope,
    model::Project,
    monitor::Shared,
    queries::{Query, Worker},
    sessions_ui::dated,
    theme,
    widgets::{self, Button, Fact, Segment, Select, Tone, Type, space},
};
use chrono::{Local, Utc};
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::sync::{Arc, atomic::Ordering};

/// Os tipos do filtro de eventos ("" = todos).
const KINDS: [&str; 10] = [
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
];

#[derive(Default, Clone, Copy, PartialEq)]
enum Tab {
    #[default]
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
    pub(crate) fn poll(&mut self, shared: &Shared) {
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
    pub fn show(&mut self, ui: &mut egui::Ui, shared: &Arc<Shared>, projects: &[Project]) {
        self.poll(shared);
        let (tab, old) = (self.tab, self.scope.clone());
        self.toolbar(ui, projects);
        if tab != self.tab {
            self.clear_result();
        }
        if old != self.scope {
            self.clear_result();
            self.observation_search.clear();
            self.kind.clear();
        }
        let Some(scope) = self.scope.clone() else {
            widgets::empty(
                ui,
                "Nenhum projeto selecionado",
                "Escolha o projeto cujos dados você deseja consultar.",
            );
            return;
        };
        ui.add_enabled_ui(!self.busy, |ui| match self.tab {
            Tab::Sessions => self.session_controls(ui, shared, &scope),
            Tab::Knowledge => self.knowledge_controls(ui, shared, &scope),
        });
        if let Some(e) = &self.error {
            widgets::strip(ui, Tone::Bad, "Falha na consulta.", e, None);
        }
        if self.busy {
            widgets::empty(
                ui,
                "Consultando…",
                "Os dados aparecem aqui quando o servidor responder.",
            );
            ui.vertical_centered(|ui| ui.spinner());
        } else if let Some((query, data)) = self.result.clone() {
            if let Some(at) = self.at {
                widgets::caption(
                    ui,
                    &format!(
                        "Consulta: {} · conteúdo restrito às permissões da conexão",
                        at.with_timezone(&Local).format("%H:%M:%S")
                    ),
                );
            }
            self.render(ui, shared, &query, &data);
        } else if self.error.is_none() {
            widgets::empty(
                ui,
                "Nada carregado",
                "Use os controles acima para carregar os dados.",
            );
        }
    }
    /// O seletor de projeto, que alarga até o que sobra na linha, e as abas.
    fn toolbar(&mut self, ui: &mut egui::Ui, projects: &[Project]) {
        let (scope, tab) = (&mut self.scope, &mut self.tab);
        let text = scope
            .as_ref()
            .map_or_else(|| "Selecione um projeto".to_owned(), Scope::label);
        let tabs = [
            Segment {
                value: Tab::Sessions,
                label: "Sessões detalhadas",
                count: None,
            },
            Segment {
                value: Tab::Knowledge,
                label: "Conhecimento",
                count: None,
            },
        ];
        let room = ui.available_width() - widgets::segmented_width(ui, tab, &tabs) - space::S;
        widgets::toolbar(
            ui,
            |ui| {
                ui.allocate_ui(egui::vec2(room.max(200.), 26.), |ui| {
                    let select = Select::new("explore-scope", "Projeto", &text).min_width(200.);
                    select.show(ui, |ui| {
                        for p in projects {
                            let s = Scope {
                                workspace: p.workspace_name.clone(),
                                project: p.project_name.clone(),
                            };
                            let label = s.label();
                            if widgets::option(ui, scope.as_ref() == Some(&s), &label).clicked() {
                                *scope = Some(s);
                            }
                        }
                    });
                });
                widgets::segmented(ui, tab, &tabs);
            },
            |_| {},
        );
    }
    fn session_controls(&mut self, ui: &mut egui::Ui, shared: &Shared, scope: &Scope) {
        widgets::row(ui, |ui| {
            widgets::checkbox(ui, &mut self.include_open, "Incluir abertas");
            if ui.add(Button::secondary("Carregar sessões")).clicked() {
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
                if ui.add(Button::secondary(label)).clicked() {
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
        widgets::caption(
            ui,
            "Sessão aberta não comprova um processo ativo. Consultar esta tela não aceita handoffs.",
        );
    }
    fn knowledge_controls(&mut self, ui: &mut egui::Ui, shared: &Shared, scope: &Scope) {
        widgets::row(ui, |ui| {
            for (label, query) in [
                ("Páginas", Query::Pages(scope.clone())),
                ("Resumo do projeto", Query::Briefing(scope.clone())),
                ("Relações do projeto", Query::Graph(scope.clone())),
                ("Relações entre projetos", Query::CrossGraph),
            ] {
                if ui.add(Button::secondary(label)).clicked() {
                    self.begin(shared, query);
                }
            }
        });
        widgets::row(ui, |ui| {
            widgets::text_field(
                ui,
                &mut self.search,
                280.,
                "Pesquisar conhecimento…",
                false,
                1024,
            );
            widgets::checkbox(ui, &mut self.global, "Todos os projetos autorizados");
            if ui.add(Button::secondary("Buscar")).clicked() {
                self.begin(
                    shared,
                    Query::Search {
                        scope: (!self.global).then(|| scope.clone()),
                        text: self.search.clone(),
                    },
                );
            }
        });
        widgets::caption(
            ui,
            "Pesquisa com escopo pode usar embeddings/reranking do servidor. As páginas são exibidas como texto histórico.",
        );
    }
    fn render(&mut self, ui: &mut egui::Ui, shared: &Shared, query: &Query, data: &Value) {
        let p = theme::palette(ui.visuals());
        // As datas como na Revisão ("03/11 18:25 (há 2 h)"); a do servidor
        // vai à dica.
        let now = Utc::now();
        match query {
            Query::Sessions {
                scope,
                offset,
                include_open,
            } => {
                let rows = array(&data["sessions"]);
                self.pager(ui, shared, *offset, rows.len(), None, false, |n| {
                    Query::Sessions {
                        scope: scope.clone(),
                        offset: n,
                        include_open: *include_open,
                    }
                });
                if rows.is_empty() {
                    widgets::empty(
                        ui,
                        "Nenhuma sessão nesta página",
                        "Volte à página anterior ou inclua as sessões abertas.",
                    );
                }
                for row in rows {
                    card(ui, "", |ui| {
                        ui.horizontal(|ui| {
                            ui.set_min_height(24.);
                            ui.label(
                                RichText::new(text(row, "agent_kind")).font(Type::Section.font()),
                            );
                            let open = row["ended_at"].is_null();
                            ui.label(
                                RichText::new(if open { "Aberta" } else { "Encerrada" })
                                    .size(12.)
                                    .color(if open { p.acc } else { p.text2 }),
                            );
                            let started = text(row, "started_at");
                            widgets::caption(ui, &dated(started, now)).on_hover_text(started);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let events = Button::secondary("Ver eventos").small().raised();
                                    if ui.add_enabled(!self.busy, events).clicked() {
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
                                },
                            );
                        });
                        ui.label(
                            RichText::new(text(row, "session_id"))
                                .font(Type::Mono.font())
                                .color(p.text2),
                        );
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
                text: filter,
                kind: filter_kind,
            } => {
                let rows = array(&data["observations"]);
                card(ui, &format!("Eventos da sessão {session}"), |ui| {
                    show_usage(ui, &data["session"]["usage"]);
                    ui.add_enabled_ui(!self.busy, |ui| {
                        widgets::row(ui, |ui| {
                            widgets::text_field(
                                ui,
                                &mut self.observation_search,
                                240.,
                                "Pesquisar eventos…",
                                false,
                                1024,
                            );
                            let shown = if self.kind.is_empty() {
                                "Todos os tipos".to_owned()
                            } else {
                                self.kind.clone()
                            };
                            let kind = &mut self.kind;
                            let select = Select::new("observation-kind", "Tipo de evento", &shown)
                                .min_width(160.)
                                .raised();
                            select.show(ui, |ui| {
                                for k in KINDS {
                                    let label = if k.is_empty() { "Todos os tipos" } else { k };
                                    if widgets::option(ui, kind.as_str() == k, label).clicked() {
                                        *kind = k.into();
                                    }
                                }
                            });
                            let apply = Button::secondary("Aplicar filtro").raised();
                            if ui.add(apply).clicked() {
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
                    self.pager(
                        ui,
                        shared,
                        *offset,
                        rows.len(),
                        data["total"].as_u64(),
                        true,
                        |n| Query::Observations {
                            scope: scope.clone(),
                            session: session.clone(),
                            offset: n,
                            text: filter.clone(),
                            kind: filter_kind.clone(),
                        },
                    );
                });
                if rows.is_empty() {
                    widgets::empty(
                        ui,
                        "Nenhum evento nesta página",
                        "Ajuste o filtro ou volte à página anterior.",
                    );
                } else {
                    widgets::card(ui).inner_margin(0).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 0.;
                        let mut seen = std::collections::HashSet::new();
                        for (idx, row) in rows.iter().enumerate() {
                            if idx > 0 {
                                let (line, _) = ui.allocate_exact_size(
                                    egui::vec2(ui.available_width(), 1.),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(line, 0., p.line);
                            }
                            let created = text(row, "created_at");
                            widgets::disclosure_row_hinted(
                                ui,
                                row_key(&mut seen, row["id"].as_str(), idx),
                                &format!("{} · {}", text(row, "kind"), text(row, "title")),
                                &dated(created, now),
                                Some(created),
                                |ui| {
                                    ui.reset_style();
                                    ui.add(
                                        egui::Label::new(text(row, "body")).wrap().selectable(true),
                                    );
                                },
                            );
                        }
                    });
                }
                if data["elided_other_scope"].as_u64().is_some_and(|n| n > 0) {
                    widgets::caption(
                        ui,
                        "Esta sessão também deixou eventos em outros projetos; apenas este escopo é exibido.",
                    );
                }
            }
            Query::Pages(scope) => card(ui, "Páginas", |ui| {
                self.page_list(ui, shared, scope, array(data))
            }),
            Query::Search { scope, .. } => {
                widgets::caption(
                    ui,
                    "Até 100 resultados. Refine a pesquisa para reduzir o conjunto.",
                );
                let rows = array(data);
                if rows.is_empty() {
                    widgets::empty(
                        ui,
                        "Nenhum resultado",
                        "Refine a pesquisa ou inclua todos os projetos autorizados.",
                    );
                } else {
                    card(ui, "Resultados", |ui| {
                        for row in rows {
                            let s = scope.clone().unwrap_or_else(|| Scope {
                                workspace: text(row, "workspace").into(),
                                project: text(row, "project").into(),
                            });
                            self.page_list(ui, shared, &s, std::slice::from_ref(row));
                            widgets::caption(
                                ui,
                                &text(row, "snippet")
                                    .replace("<mark>", "")
                                    .replace("</mark>", ""),
                            );
                        }
                    });
                }
            }
            Query::Page { scope, path } => {
                card(ui, "", |ui| {
                    ui.label(RichText::new(text(data, "title")).font(Type::Detail.font()));
                    widgets::caption(
                        ui,
                        &format!("{} · {} · versão {}", scope.label(), path, text(data, "id")),
                    );
                    let history = Button::secondary("Histórico e comparação").raised();
                    if ui.add_enabled(!self.busy, history).clicked() {
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
                        widgets::strip(
                            ui,
                            Tone::Warn,
                            "Prévia limitada a 100 KiB;",
                            "o conteúdo no servidor permanece completo.",
                            None,
                        );
                    }
                    ui.add(
                        egui::Label::new(bounded_text(text(data, "body_markdown"), 100_000))
                            .wrap()
                            .selectable(true),
                    );
                });
                for (key, label) in [("links", "Referências"), ("backlinks", "Referenciada por")] {
                    card(ui, label, |ui| {
                        ui.push_id(key, |ui| {
                            self.page_list(ui, shared, scope, array(&data[key]))
                        });
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
                let versions = array(&data["versions"]);
                card(ui, &format!("Histórico: {path}"), |ui| {
                    widgets::row(ui, |ui| {
                        for (id, label, selected) in [
                            ("history-from", "Comparar de", &mut self.from),
                            ("history-to", "Até", &mut self.to),
                        ] {
                            let shown = if selected.is_empty() {
                                label.to_owned()
                            } else {
                                selected.clone()
                            };
                            let select = Select::new(id, label, &shown).min_width(160.).raised();
                            select.show(ui, |ui| {
                                if widgets::option(ui, selected.is_empty(), "Automático").clicked()
                                {
                                    selected.clear();
                                }
                                for row in versions {
                                    let id = text(row, "id");
                                    let created = text(row, "created_at");
                                    let version = format!("{} · {id}", dated(created, now));
                                    if widgets::option(ui, selected.as_str() == id, &version)
                                        .on_hover_text(created)
                                        .clicked()
                                    {
                                        *selected = id.into();
                                    }
                                }
                            });
                        }
                        let compare = Button::secondary("Comparar").raised();
                        if ui.add_enabled(!self.busy, compare).clicked() {
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
                        versions.len(),
                        data["total"].as_u64(),
                        true,
                        |n| Query::History {
                            scope: scope.clone(),
                            path: path.clone(),
                            offset: n,
                            from: from.clone(),
                            to: to.clone(),
                        },
                    );
                    for row in versions {
                        let created = text(row, "created_at");
                        ui.label(format!(
                            "{} · {} · {} caracteres",
                            dated(created, now),
                            text(row, "id"),
                            number(&row["body_chars"])
                        ))
                        .on_hover_text(created);
                    }
                });
                let diff = &data["diff"];
                card(
                    ui,
                    &format!("Comparação: {}", text(diff, "status")),
                    |ui| {
                        if diff["truncated"].as_bool() == Some(true)
                            || diff["status"] == "too_large"
                        {
                            widgets::strip(
                                ui,
                                Tone::Warn,
                                "Comparação parcial ou grande demais.",
                                "Consulte versões menores.",
                                None,
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
                                        .color(
                                            match text(line, "op") {
                                                "+" => p.acc,
                                                "-" => p.warn,
                                                _ => ui.visuals().text_color(),
                                            },
                                        ),
                                    )
                                    .wrap()
                                    .selectable(true),
                                );
                            }
                        }
                    },
                );
            }
            Query::Briefing(scope) => {
                let last = text(data, "last_observation_at");
                card(ui, "Resumo do projeto", |ui| {
                    widgets::facts(
                        ui,
                        &[
                            Fact::new("Páginas atuais", number(&data["counts"]["pages_latest"])),
                            Fact::new("Sessões", number(&data["counts"]["sessions"])),
                            Fact::new("Eventos", number(&data["counts"]["observations"])),
                            Fact::new("Handoffs pendentes", number(&data["pending_handoff_count"])),
                            Fact::new("Último evento", dated(last, now)).hint(last),
                        ],
                    );
                    for (key, title) in [
                        ("activity_7d", "Últimos 7 dias"),
                        ("activity_30d", "Últimos 30 dias"),
                    ] {
                        ui.label(RichText::new(title).font(Type::Section.font()));
                        widgets::facts(
                            ui,
                            &[
                                Fact::new("Sessões", number(&data[key]["sessions"])),
                                Fact::new("Eventos", number(&data[key]["observations"])),
                                Fact::new(
                                    "Páginas atualizadas",
                                    number(&data[key]["pages_updated"]),
                                ),
                            ],
                        );
                    }
                });
                for (key, label) in [("rules", "Regras"), ("recent_pages", "Páginas recentes")] {
                    card(ui, label, |ui| {
                        ui.push_id(key, |ui| {
                            self.page_list(ui, shared, scope, array(&data[key]))
                        });
                    });
                }
            }
            Query::Graph(scope) => card(ui, "Relações do projeto", |ui| {
                if data["truncated"] == true {
                    widgets::strip(
                        ui,
                        Tone::Warn,
                        "Grafo parcial:",
                        "o servidor limita a 120 páginas.",
                        None,
                    );
                }
                draw_graph(ui, data);
                self.page_list(ui, shared, scope, array(&data["nodes"]));
            }),
            Query::CrossGraph => card(ui, "Relações entre projetos autorizados", |ui| {
                let edges = array(&data["edges"]);
                if edges.is_empty() {
                    widgets::caption(ui, "Nenhuma relação.");
                }
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
            }),
            Query::Handoffs { .. } => {
                widgets::caption(
                    ui,
                    "Lista limitada a 200 registros; nenhum handoff é consumido.",
                );
                let rows = array(&data["handoffs"]);
                if rows.is_empty() {
                    widgets::empty(
                        ui,
                        "Nenhum handoff neste estado",
                        "Os handoffs do projeto neste estado aparecem aqui.",
                    );
                }
                for row in rows {
                    card(
                        ui,
                        &format!("{} · {}", text(row, "state"), text(row, "agent")),
                        |ui| {
                            let at = text(row, "at");
                            widgets::caption(
                                ui,
                                &format!("{} · {}", dated(at, now), text(row, "id")),
                            )
                            .on_hover_text(at);
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
                        },
                    );
                }
            }
        }
    }
    fn page_list(&mut self, ui: &mut egui::Ui, shared: &Shared, scope: &Scope, rows: &[Value]) {
        let p = theme::palette(ui.visuals());
        if rows.is_empty() {
            widgets::caption(ui, "Nenhuma página.");
        }
        let range = local_page(ui, rows.len());
        for row in &rows[range] {
            let target = related_scope(scope, row);
            ui.horizontal_wrapped(|ui| {
                // Com seleção de texto, o egui anuncia o link como Label.
                ui.style_mut().interaction.selectable_labels = false;
                let link = egui::Link::new(RichText::new(text(row, "title")).color(p.acc));
                if ui.add_enabled(!self.busy, link).clicked() {
                    self.begin(
                        shared,
                        Query::Page {
                            scope: target.clone(),
                            path: text(row, "path").into(),
                        },
                    );
                }
                widgets::caption(ui, &format!("{} · {}", target.label(), text(row, "path")));
            });
        }
    }
    /// A paginação do servidor, de 50 em 50; `raised` dentro de um cartão.
    #[allow(clippy::too_many_arguments)]
    fn pager(
        &mut self,
        ui: &mut egui::Ui,
        shared: &Shared,
        offset: usize,
        count: usize,
        total: Option<u64>,
        raised: bool,
        query: impl Fn(usize) -> Query,
    ) {
        let more = total
            .map(|n| offset.saturating_add(count) < n as usize)
            .unwrap_or(count == 50);
        let label = range_label(offset, count, total.map(|n| n as usize));
        let (back, next) = pager_row(
            ui,
            &label,
            !self.busy && offset > 0,
            !self.busy && more,
            raised,
        );
        if back {
            self.begin(shared, query(offset.saturating_sub(50)));
        }
        if next {
            self.begin(shared, query(offset.saturating_add(50)));
        }
    }
}
/// Um cartão da largura toda, com o título (Seção) quando há um.
fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    widgets::card(ui)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if !title.is_empty() {
                ui.label(RichText::new(title).font(Type::Section.font()));
            }
            add(ui)
        })
        .inner
}
/// "N–M de T" e os botões Secundários Anterior e Próxima (`raised` dentro de
/// um cartão: no escuro, o bg é o do campo); devolve os cliques.
fn pager_row(ui: &mut egui::Ui, label: &str, back: bool, next: bool, raised: bool) -> (bool, bool) {
    let button = |text| {
        let button = Button::secondary(text).small();
        if raised { button.raised() } else { button }
    };
    ui.horizontal(|ui| {
        ui.set_min_height(24.);
        widgets::caption(ui, label);
        let back = ui.add_enabled(back, button("Anterior")).clicked();
        let next = ui.add_enabled(next, button("Próxima")).clicked();
        (back, next)
    })
    .inner
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
/// "N–M de T" (M = N + `count` − 1), ou "N–M" sem o total; sem registros,
/// "0 de T", ou "Nenhum registro" sem o total.
fn range_label(offset: usize, count: usize, total: Option<usize>) -> String {
    let number = crate::model::number;
    let span = format!("{}–{}", number(offset + 1), number(offset + count));
    match (count, total) {
        (0, None) => "Nenhum registro".into(),
        (0, Some(total)) => format!("0 de {}", number(total)),
        (_, Some(total)) => format!("{span} de {}", number(total)),
        (_, None) => span,
    }
}
/// O deslocamento da página local (de 100 em 100), guardado na memória do
/// egui e preso à última página. Com mais de 100, `nav` mostra a navegação e
/// devolve o deslocamento novo.
fn local_offset(
    ui: &mut egui::Ui,
    total: usize,
    nav: impl FnOnce(&mut egui::Ui, usize) -> usize,
) -> std::ops::Range<usize> {
    let id = ui.id().with("local-page");
    let mut offset = ui.data(|d| d.get_temp::<usize>(id).unwrap_or(0));
    offset = offset.min(total.saturating_sub(1) / 100 * 100);
    if total > 100 {
        offset = nav(ui, offset);
    }
    ui.data_mut(|d| d.insert_temp(id, offset));
    offset..(offset + 100).min(total)
}
/// A página local das listas de Explorar e de `show_values`, dentro de
/// cartões: "N–M de T" e Anterior/Próxima.
fn local_page(ui: &mut egui::Ui, total: usize) -> std::ops::Range<usize> {
    local_offset(ui, total, |ui, offset| {
        let label = range_label(offset, (total - offset).min(100), Some(total));
        match pager_row(ui, &label, offset > 0, offset + 100 < total, true) {
            (true, _) => offset.saturating_sub(100),
            (_, true) => offset + 100,
            _ => offset,
        }
    })
}
pub(crate) fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub(crate) fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("—")
}
pub(crate) fn row_key(
    seen: &mut std::collections::HashSet<String>,
    id: Option<&str>,
    idx: usize,
) -> (String, Option<usize>) {
    match id {
        Some(id) if !id.is_empty() && seen.insert(id.to_owned()) => (id.to_owned(), None),
        _ => (id.unwrap_or("").to_owned(), Some(idx)),
    }
}
pub(crate) fn number(v: &Value) -> String {
    v.as_u64()
        .map(|n| crate::model::number(n as usize))
        .unwrap_or("—".into())
}
fn show_usage(ui: &mut egui::Ui, usage: &Value) {
    if usage.is_null() {
        widgets::caption(ui, "Tokens: não reportados pelo cliente.");
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
    widgets::caption(
        ui,
        "Totais reportados da sessão; não são valores de faturamento nem rateio por projeto/modelo.",
    );
}
pub(crate) fn count_grid(ui: &mut egui::Ui, id: &str, data: &Value, fields: &[(&str, &str)]) {
    egui::Grid::new(id).num_columns(2).show(ui, |ui| {
        for (key, label) in fields {
            ui.label(*label);
            ui.strong(number(&data[*key]));
            ui.end_row();
        }
    });
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
                ui.colored_label(theme::palette(ui.visuals()).warn, "Objeto parcial: somente os primeiros 200 campos. Consulte a fonte completa antes de decidir.");
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
    let p = theme::palette(ui.visuals());
    let nodes = array(&data["nodes"]);
    if nodes.is_empty() {
        widgets::caption(ui, "Nenhuma relação disponível.");
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
            // text2: as ligações se veem (≥ 3:1) sobre o cartão; a `line`
            // ficaria em 1,25:1.
            ui.painter().line_segment(
                [positions[a], positions[b]],
                egui::Stroke::new(1.0_f32, p.text2),
            );
        }
    }
    for (i, pos) in positions.iter().enumerate() {
        ui.painter().circle_filled(*pos, 5., p.acc);
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
    widgets::caption(
        ui,
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
        // Com as fontes do app: os títulos usam a família SemiBold.
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let ctx = crate::widgets::tests::context(theme);
            for query in &queries {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| inspector.render(ui, &shared, query, &json!({})));
                });
            }
        }
        inspector.result = Some((Query::CrossGraph, json!({"edges":[]})));
        inspector.search = "private fixture".into();
        inspector.reset();
        assert!(inspector.result.is_none());
        assert!(inspector.search.is_empty());
    }

    #[test]
    fn explore_has_detailed_sessions_and_knowledge_and_opens_on_sessions() {
        use crate::widgets::tests::{context, node, nodes, run};
        use egui::accesskit::Role;
        let shared = Shared::unstarted(crate::model::Config::default());
        let mut console = Console::new(shared.clone());
        console.scope = Some(Scope {
            workspace: "fixture".into(),
            project: "demo".into(),
        });
        let ctx = context(egui::Theme::Dark);
        let out = run(&ctx, vec![], |ui| console.show(ui, &shared, &[]));
        for tab in ["Sessões detalhadas", "Conhecimento"] {
            node(&out, Role::Button, tab);
        }
        // Abre em Sessões detalhadas: os controles dela, não os de Conhecimento.
        node(&out, Role::Button, "Carregar sessões");
        assert!(nodes(&out, Role::Button, "Páginas").is_empty());
        assert!(
            nodes(&out, Role::Button, "Saúde").is_empty(),
            "Saúde is its own page now"
        );
    }

    #[test]
    fn counts_group_thousands() {
        assert_eq!(number(&json!(1234567)), "1.234.567");
        assert_eq!(number(&json!(999)), "999");
        assert_eq!(number(&json!(0)), "0");
        for missing in [json!(null), json!(-1), json!(1.5), json!("12")] {
            assert_eq!(number(&missing), "—", "{missing}");
        }
    }

    #[test]
    fn pager_and_range_labels_group_thousands() {
        // A paginação do servidor: "N–M de T", ou "N–M" sem o total.
        assert_eq!(range_label(0, 50, None), "1–50");
        assert_eq!(range_label(0, 0, Some(0)), "0 de 0");
        assert_eq!(range_label(0, 0, None), "Nenhum registro");
        assert_eq!(
            range_label(49_950, 50, Some(1_234_567)),
            "49.951–50.000 de 1.234.567"
        );
        // A página local (de 100), a mesma de antes.
        assert_eq!(range_label(0, 100, Some(5_000)), "1–100 de 5.000");
        assert_eq!(range_label(1_200, 34, Some(1_234)), "1.201–1.234 de 1.234");
    }

    #[test]
    fn row_key_is_stable_for_unique_ids_and_disambiguates_duplicates() {
        let mut seen = std::collections::HashSet::new();
        assert_eq!(row_key(&mut seen, Some("a"), 0), ("a".into(), None));
        assert_eq!(row_key(&mut seen, Some("b"), 1), ("b".into(), None));

        let mut seen = std::collections::HashSet::new();
        assert_eq!(row_key(&mut seen, Some("b"), 0), ("b".into(), None));
        assert_eq!(row_key(&mut seen, Some("a"), 1), ("a".into(), None));

        let mut seen = std::collections::HashSet::new();
        assert_eq!(row_key(&mut seen, Some("a"), 0), ("a".into(), None));
        assert_eq!(row_key(&mut seen, Some("a"), 1), ("a".into(), Some(1)));

        let mut seen = std::collections::HashSet::new();
        assert_eq!(row_key(&mut seen, None, 0), ("".into(), Some(0)));
        assert_eq!(row_key(&mut seen, Some(""), 3), ("".into(), Some(3)));
    }

    use crate::{
        test_support::{Mock, Response},
        widgets::tests::{
            THEMES, click_sized, context, node, nodes, rect_of, rects, run_sized, texts,
        },
    };
    use egui::accesskit::{Role, Toggled};

    /// Alto o bastante para a lista inteira ficar à vista.
    const TALL: [f32; 2] = [1000., 6000.];
    fn fixture() -> Scope {
        Scope {
            workspace: "fixture".into(),
            project: "demo".into(),
        }
    }
    /// Explorar num servidor sintético; `chosen`: com o projeto escolhido.
    fn explore(chosen: bool) -> (Mock, Arc<Shared>, Console) {
        let mock = Mock::new(|_| Response::json(200, json!({"sessions": []})));
        let shared = Shared::unstarted(mock.config());
        let mut console = Console::new(shared.clone());
        if chosen {
            console.scope = Some(fixture());
        }
        (mock, shared, console)
    }
    /// Dois frames: o segundo já com os tamanhos medidos no primeiro.
    fn frames(ctx: &egui::Context, add: &mut impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
        run_sized(ctx, TALL, vec![], &mut *add);
        run_sized(ctx, TALL, vec![], &mut *add)
    }
    /// Clica no nó `role` `text` do frame atual.
    fn press(ctx: &egui::Context, add: &mut impl FnMut(&mut egui::Ui), role: Role, text: &str) {
        let out = frames(ctx, add);
        let at = rect_of(&node(&out, role, text)).center();
        click_sized(ctx, TALL, at, &mut *add);
    }
    /// Espera a resposta do worker.
    fn settle(console: &mut Console, shared: &Shared) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while console.busy {
            assert!(
                std::time::Instant::now() < until,
                "the worker never answered"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
            console.poll(shared);
        }
    }
    fn toggled(out: &egui::FullOutput, text: &str) -> Option<Toggled> {
        node(out, Role::Button, text).toggled()
    }

    #[test]
    fn the_tabs_expose_their_selection_and_switching_clears_the_result() {
        for (theme, _) in THEMES {
            let (_mock, shared, mut console) = explore(true);
            console.result = Some((Query::CrossGraph, json!({"edges": []})));
            console.at = Some(Utc::now());
            let ctx = context(theme);
            let mut add = |ui: &mut egui::Ui| console.show(ui, &shared, &[]);
            let out = frames(&ctx, &mut add);
            assert_eq!(
                (
                    toggled(&out, "Sessões detalhadas"),
                    toggled(&out, "Conhecimento")
                ),
                (Some(Toggled::True), Some(Toggled::False)),
                "{theme:?}"
            );
            node(&out, Role::Label, "Relações entre projetos autorizados");
            press(&ctx, &mut add, Role::Button, "Conhecimento");
            let out = frames(&ctx, &mut add);
            assert_eq!(
                (
                    toggled(&out, "Sessões detalhadas"),
                    toggled(&out, "Conhecimento")
                ),
                (Some(Toggled::False), Some(Toggled::True)),
                "{theme:?}"
            );
            node(&out, Role::Button, "Páginas");
            assert!(nodes(&out, Role::Button, "Carregar sessões").is_empty());
            assert!(nodes(&out, Role::Label, "Relações entre projetos autorizados").is_empty());
            node(&out, Role::Label, "Nada carregado");
            drop(add);
            assert!(console.tab == Tab::Knowledge);
            assert!(
                console.result.is_none() && console.at.is_none(),
                "{theme:?}"
            );
        }
    }

    #[test]
    fn the_project_selector_lists_the_projects_and_a_new_pick_clears_the_result() {
        let projects = ["demo", "outro"].map(|name| Project {
            workspace_name: "fixture".into(),
            project_name: name.into(),
            ..Default::default()
        });
        let (_mock, shared, mut console) = explore(true);
        console.result = Some((Query::CrossGraph, json!({"edges": []})));
        console.observation_search = "busca".into();
        console.kind = "stop".into();
        let ctx = context(egui::Theme::Light);
        let mut add = |ui: &mut egui::Ui| console.show(ui, &shared, &projects);
        let out = frames(&ctx, &mut add);
        let combo = node(&out, Role::ComboBox, "Projeto");
        assert_eq!(combo.value(), Some("fixture / demo"));
        let at = rect_of(&combo);
        assert_eq!(at.height(), 26.);
        click_sized(&ctx, TALL, at.center(), &mut add);
        let out = frames(&ctx, &mut add);
        assert_eq!(toggled(&out, "fixture / demo"), Some(Toggled::True));
        assert_eq!(toggled(&out, "fixture / outro"), Some(Toggled::False));
        let other = rect_of(&node(&out, Role::Button, "fixture / outro"));
        assert!(
            other.top() > at.bottom(),
            "the list opens below the selector"
        );
        click_sized(&ctx, TALL, other.center(), &mut add);
        let out = frames(&ctx, &mut add);
        assert_eq!(
            node(&out, Role::ComboBox, "Projeto").value(),
            Some("fixture / outro")
        );
        drop(add);
        assert_eq!(
            console.scope.as_ref().map(|s| s.project.as_str()),
            Some("outro")
        );
        assert!(console.result.is_none());
        assert_eq!(
            (console.observation_search.as_str(), console.kind.as_str()),
            ("", "")
        );
    }

    #[test]
    fn the_server_pager_at_its_limits() {
        // (deslocamento, linhas, total, rótulo, Anterior?, Próxima?)
        let cases = [
            (0, 0, Some(0), "0 de 0", false, false),
            (0, 1, Some(1), "1–1 de 1", false, false),
            (0, 50, Some(120), "1–50 de 120", false, true),
            (50, 50, Some(100), "51–100 de 100", true, false),
            (100, 20, Some(120), "101–120 de 120", true, false),
            // Sessões não informam o total: página cheia sugere mais.
            (0, 50, None, "1–50", false, true),
            (50, 49, None, "51–99", true, false),
            (0, 0, None, "Nenhum registro", false, false),
        ];
        for (offset, count, total, label, back, next) in cases {
            let case = format!("{offset} + {count} of {total:?}");
            let (mock, shared, mut console) = explore(true);
            let query = match total {
                Some(total) => {
                    let rows: Vec<Value> = (0..count)
                        .map(|i| json!({"id": format!("e{i}"), "kind": "stop", "title": "t"}))
                        .collect();
                    let data = json!({"observations": rows, "total": total});
                    let query = Query::Observations {
                        scope: fixture(),
                        session: "s1".into(),
                        offset,
                        text: String::new(),
                        kind: String::new(),
                    };
                    console.result = Some((query, data));
                    "observations"
                }
                None => {
                    let rows: Vec<Value> = (0..count)
                        .map(|i| json!({"session_id": format!("s{i}"), "agent_kind": "codex"}))
                        .collect();
                    let query = Query::Sessions {
                        scope: fixture(),
                        offset,
                        include_open: true,
                    };
                    console.result = Some((query, json!({"sessions": rows})));
                    "sessions?"
                }
            };
            let ctx = context(egui::Theme::Dark);
            let mut add = |ui: &mut egui::Ui| console.show(ui, &shared, &[]);
            let out = frames(&ctx, &mut add);
            node(&out, Role::Label, label);
            assert_eq!(
                (
                    !node(&out, Role::Button, "Anterior").is_disabled(),
                    !node(&out, Role::Button, "Próxima").is_disabled()
                ),
                (back, next),
                "{case}"
            );
            // O botão habilitado pede a página vizinha ao servidor.
            let (button, to) = match (back, next) {
                (_, true) => ("Próxima", offset + 50),
                (true, false) => ("Anterior", offset - 50),
                (false, false) => continue,
            };
            press(&ctx, &mut add, Role::Button, button);
            drop(add);
            settle(&mut console, &shared);
            let requests = mock.requests();
            assert_eq!(requests.len(), 1, "{case}");
            assert!(
                requests[0].path.contains(query)
                    && requests[0].path.contains(&format!("offset={to}&")),
                "{case}: {}",
                requests[0].path
            );
        }
    }

    #[test]
    fn the_local_pager_at_its_limits() {
        // (páginas, rótulo, Anterior?, Próxima?, links, a última página e os links dela)
        for (total, label, back, next, links, last) in [
            (0, None, false, false, 0, None),
            (1, None, false, false, 1, None),
            (100, None, false, false, 100, None),
            (
                101,
                Some("1–100 de 101"),
                false,
                true,
                100,
                Some(("101–101 de 101", 1)),
            ),
            (
                200,
                Some("1–100 de 200"),
                false,
                true,
                100,
                Some(("101–200 de 200", 100)),
            ),
        ] {
            let rows: Vec<Value> = (0..total)
                .map(|i| json!({"title": format!("Página {i}"), "path": format!("p{i}.md")}))
                .collect();
            let (_mock, shared, mut console) = explore(true);
            console.tab = Tab::Knowledge;
            console.result = Some((Query::Pages(fixture()), json!(rows)));
            let ctx = context(egui::Theme::Light);
            let mut add = |ui: &mut egui::Ui| console.show(ui, &shared, &[]);
            let out = frames(&ctx, &mut add);
            let shown = |out: &egui::FullOutput| {
                let update = out.platform_output.accesskit_update.as_ref().unwrap();
                update
                    .nodes
                    .iter()
                    .filter(|(_, n)| n.role() == Role::Link)
                    .count()
            };
            assert_eq!(shown(&out), links, "{total}");
            assert_eq!(
                nodes(&out, Role::Label, "Nenhuma página.").len(),
                usize::from(total == 0),
                "{total}"
            );
            let Some(label) = label else {
                assert!(nodes(&out, Role::Button, "Próxima").is_empty(), "{total}");
                continue;
            };
            node(&out, Role::Label, label);
            assert_eq!(
                (
                    !node(&out, Role::Button, "Anterior").is_disabled(),
                    !node(&out, Role::Button, "Próxima").is_disabled()
                ),
                (back, next),
                "{total}"
            );
            // A última página, e de volta à primeira.
            let (last, last_links) = last.unwrap();
            press(&ctx, &mut add, Role::Button, "Próxima");
            let out = frames(&ctx, &mut add);
            node(&out, Role::Label, last);
            assert_eq!(shown(&out), last_links, "{total}");
            node(&out, Role::Link, "Página 100");
            assert_eq!(
                (
                    !node(&out, Role::Button, "Anterior").is_disabled(),
                    !node(&out, Role::Button, "Próxima").is_disabled()
                ),
                (true, false),
                "{total}: the last page"
            );
            press(&ctx, &mut add, Role::Button, "Anterior");
            let out = frames(&ctx, &mut add);
            node(&out, Role::Label, label);
            node(&out, Role::Link, "Página 0");
        }
    }

    #[test]
    fn show_values_pages_long_lists_with_the_explore_pager_raised_on_its_card() {
        // Saúde, Ajustes, Administração e Revisão mostram os dados num cartão.
        let data = json!((0..250).map(|i| format!("item {i}")).collect::<Vec<_>>());
        for (theme, p) in THEMES {
            let ctx = context(theme);
            let mut add = |ui: &mut egui::Ui| {
                widgets::card(ui).show(ui, |ui| show_values(ui, &data, 0));
            };
            let fills = |out: &egui::FullOutput, button: &str| -> Vec<egui::Color32> {
                let at = rect_of(&node(out, Role::Button, button));
                rects(out)
                    .into_iter()
                    .filter(|r| r.rect == at)
                    .map(|r| r.fill)
                    .collect()
            };
            let out = frames(&ctx, &mut add);
            node(&out, Role::Label, "1–100 de 250");
            node(&out, Role::Label, "item 99");
            assert!(nodes(&out, Role::Label, "item 100").is_empty(), "{theme:?}");
            assert!(
                node(&out, Role::Button, "Anterior").is_disabled(),
                "{theme:?}"
            );
            assert!(
                !node(&out, Role::Button, "Próxima").is_disabled(),
                "{theme:?}"
            );
            assert_eq!(fills(&out, "Próxima"), [p.raised], "{theme:?}");
            press(&ctx, &mut add, Role::Button, "Próxima");
            // O ponteiro sai: sem o surf2 de quem está sob ele.
            run_sized(&ctx, TALL, vec![egui::Event::PointerGone], &mut add);
            let out = frames(&ctx, &mut add);
            node(&out, Role::Label, "101–200 de 250");
            node(&out, Role::Label, "item 100");
            assert!(nodes(&out, Role::Label, "item 99").is_empty(), "{theme:?}");
            for button in ["Anterior", "Próxima"] {
                assert!(!node(&out, Role::Button, button).is_disabled(), "{theme:?}");
                let fills = fills(&out, button);
                assert_eq!(fills, [p.raised], "{theme:?} {button}");
                assert!(
                    !fills.contains(&p.field),
                    "{theme:?} {button}: like a field"
                );
            }
        }
    }

    #[test]
    fn the_graph_draws_its_links_at_3_to_1_on_its_card() {
        use crate::theme::tests::contrast;
        for (theme, p) in THEMES {
            let (_mock, shared, mut console) = explore(true);
            console.tab = Tab::Knowledge;
            let data = json!({
                "nodes": [{"path": "a.md", "title": "A"}, {"path": "b.md", "title": "B"}],
                "edges": [{"from_path": "a.md", "to_path": "b.md"}],
            });
            console.result = Some((Query::Graph(fixture()), data));
            let ctx = context(theme);
            let out = frames(&ctx, &mut |ui: &mut egui::Ui| {
                console.show(ui, &shared, &[])
            });
            node(&out, Role::Label, "Relações do projeto");
            let links: Vec<egui::Color32> = crate::widgets::tests::shapes(&out)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::LineSegment { stroke, .. } => Some(stroke.color),
                    _ => None,
                })
                .collect();
            assert_eq!(links.len(), 1, "{theme:?}: one link");
            let ratio = contrast(links[0], p.surf);
            assert!(
                ratio >= 3.,
                "{theme:?}: the link on the card is {ratio:.2}:1"
            );
            node(&out, Role::Link, "A");
            node(&out, Role::Link, "B");
        }
    }

    #[test]
    fn empty_loading_and_error_states_say_what_is_going_on() {
        for (theme, p) in THEMES {
            let (_mock, shared, mut console) = explore(false);
            let ctx = context(theme);
            let show = |console: &mut Console| {
                frames(&ctx, &mut |ui: &mut egui::Ui| {
                    console.show(ui, &shared, &[])
                })
            };
            let out = show(&mut console);
            node(&out, Role::Label, "Nenhum projeto selecionado");
            node(
                &out,
                Role::Label,
                "Escolha o projeto cujos dados você deseja consultar.",
            );
            assert!(nodes(&out, Role::Button, "Carregar sessões").is_empty());
            console.scope = Some(fixture());
            let out = show(&mut console);
            node(&out, Role::Label, "Nada carregado");
            assert!(!node(&out, Role::Button, "Carregar sessões").is_disabled());
            console.busy = true;
            let out = show(&mut console);
            node(&out, Role::Label, "Consultando…");
            assert!(nodes(&out, Role::Label, "Nada carregado").is_empty());
            assert!(node(&out, Role::Button, "Carregar sessões").is_disabled());
            console.busy = false;
            console.error = Some("Servidor respondeu HTTP 500.".into());
            let out = show(&mut console);
            let strip = rect_of(&node(
                &out,
                Role::Label,
                "Falha na consulta. Servidor respondeu HTTP 500.",
            ));
            assert!(
                rects(&out)
                    .iter()
                    .any(|r| r.rect == strip && r.fill == widgets::tone(p.bad, 0.08)),
                "{theme:?}: a Bad strip"
            );
            assert!(nodes(&out, Role::Label, "Nada carregado").is_empty());
            console.error = None;
            // Listas vazias: a mensagem de cada consulta.
            for (query, data, message) in [
                (
                    Query::Sessions {
                        scope: fixture(),
                        offset: 0,
                        include_open: true,
                    },
                    json!({"sessions": []}),
                    "Nenhuma sessão nesta página",
                ),
                (
                    Query::Observations {
                        scope: fixture(),
                        session: "s1".into(),
                        offset: 0,
                        text: String::new(),
                        kind: String::new(),
                    },
                    json!({"observations": [], "total": 0}),
                    "Nenhum evento nesta página",
                ),
                (
                    Query::Search {
                        scope: Some(fixture()),
                        text: "nada".into(),
                    },
                    json!([]),
                    "Nenhum resultado",
                ),
                (
                    Query::Handoffs {
                        scope: fixture(),
                        state: "open".into(),
                    },
                    json!({"handoffs": []}),
                    "Nenhum handoff neste estado",
                ),
            ] {
                console.result = Some((query, data));
                let out = show(&mut console);
                node(&out, Role::Label, message);
            }
        }
    }

    #[test]
    fn the_session_filters_send_what_they_show() {
        for include in [true, false] {
            let (mock, shared, mut console) = explore(true);
            let ctx = context(egui::Theme::Dark);
            {
                let mut add = |ui: &mut egui::Ui| console.show(ui, &shared, &[]);
                let out = frames(&ctx, &mut add);
                let check = node(&out, Role::CheckBox, "Incluir abertas");
                assert_eq!(check.toggled(), Some(Toggled::True), "on by default");
                if !include {
                    press(&ctx, &mut add, Role::CheckBox, "Incluir abertas");
                    let out = frames(&ctx, &mut add);
                    assert_eq!(
                        node(&out, Role::CheckBox, "Incluir abertas").toggled(),
                        Some(Toggled::False)
                    );
                }
                press(&ctx, &mut add, Role::Button, "Carregar sessões");
            }
            settle(&mut console, &shared);
            let requests = mock.requests();
            assert_eq!(requests.len(), 1, "{include}");
            assert!(
                requests[0]
                    .path
                    .contains(&format!("offset=0&include_open={include}")),
                "{include}: {}",
                requests[0].path
            );
        }
    }

    #[test]
    fn dates_read_like_the_review_and_keep_the_server_text_in_the_tooltip() {
        // Há 2 h e 5 min, no formato do servidor (microssegundos, Z).
        let raw = (Utc::now() - chrono::Duration::minutes(125))
            .format("%Y-%m-%dT%H:%M:%S%.6fZ")
            .to_string();
        let shown = format!("{} (há 2 h)", crate::app::date(&raw));
        // (consulta, resposta, texto que leva a data)
        let cases = [
            (
                Query::Sessions {
                    scope: fixture(),
                    offset: 0,
                    include_open: true,
                },
                json!({"sessions": [{"agent_kind": "claude-code", "started_at": raw, "ended_at": raw, "session_id": "s1"}]}),
                shown.clone(),
            ),
            (
                Query::Handoffs {
                    scope: fixture(),
                    state: "open".into(),
                },
                json!({"handoffs": [{"state": "open", "agent": "codex", "at": raw, "id": "h1", "summary": "resumo"}]}),
                format!("{shown} · h1"),
            ),
            (
                Query::History {
                    scope: fixture(),
                    path: "a.md".into(),
                    offset: 0,
                    from: String::new(),
                    to: String::new(),
                },
                json!({"versions": [{"id": "v1", "created_at": raw, "body_chars": 12}], "total": 1, "diff": {}}),
                format!("{shown} · v1 · 12 caracteres"),
            ),
            (
                Query::Observations {
                    scope: fixture(),
                    session: "s1".into(),
                    offset: 0,
                    text: String::new(),
                    kind: String::new(),
                },
                json!({"observations": [{"id": "e1", "kind": "stop", "title": "fim", "created_at": raw, "body": "corpo"}], "total": 1}),
                shown.clone(),
            ),
            (
                Query::Briefing(fixture()),
                json!({"counts": {}, "last_observation_at": raw}),
                shown.clone(),
            ),
        ];
        for (query, data, text) in cases {
            let (_mock, shared, mut console) = explore(true);
            console.result = Some((query, data));
            let ctx = context(egui::Theme::Dark);
            let mut add = |ui: &mut egui::Ui| console.show(ui, &shared, &[]);
            let out = frames(&ctx, &mut add);
            assert!(
                nodes(&out, Role::Label, &raw).is_empty()
                    && !texts(&out).iter().any(|(t, _)| t.contains(&raw)),
                "{text}: the server's text is not shown"
            );
            // A dica, com o ponteiro parado sobre a data pintada (0,5 s; cada
            // frame avança 1/60 s).
            let painted: Vec<egui::Rect> = crate::widgets::tests::shapes(&out)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) if t.galley.text() == text => {
                        Some(t.galley.rect.translate(t.pos.to_vec2()))
                    }
                    _ => None,
                })
                .collect();
            let [painted] = painted[..] else {
                panic!("{text}: painted once, got {painted:?}");
            };
            let at = painted.center();
            let mut out = run_sized(&ctx, TALL, vec![egui::Event::PointerMoved(at)], &mut add);
            for _ in 0..45 {
                out = run_sized(&ctx, TALL, vec![], &mut add);
            }
            assert!(
                texts(&out).iter().any(|(t, _)| *t == raw),
                "{text}: the tooltip has the server's text"
            );
        }
    }

    #[test]
    fn the_secondaries_on_the_cards_are_raised_and_never_the_field_color() {
        // (consulta, resposta, o secundário do cartão)
        let cases = [
            (
                Query::Sessions {
                    scope: fixture(),
                    offset: 0,
                    include_open: true,
                },
                json!({"sessions": [{"agent_kind": "claude-code", "session_id": "s1"}]}),
                "Ver eventos",
            ),
            (
                Query::Observations {
                    scope: fixture(),
                    session: "s1".into(),
                    offset: 0,
                    text: String::new(),
                    kind: String::new(),
                },
                json!({"observations": [], "total": 0}),
                "Aplicar filtro",
            ),
            (
                Query::Page {
                    scope: fixture(),
                    path: "a.md".into(),
                },
                json!({"title": "A", "id": "v1", "body_markdown": "corpo"}),
                "Histórico e comparação",
            ),
            (
                Query::History {
                    scope: fixture(),
                    path: "a.md".into(),
                    offset: 0,
                    from: String::new(),
                    to: String::new(),
                },
                json!({"versions": [], "total": 0, "diff": {}}),
                "Comparar",
            ),
        ];
        for (theme, p) in THEMES {
            for (query, data, button) in cases.clone() {
                let (_mock, shared, mut console) = explore(true);
                console.result = Some((query, data));
                let ctx = context(theme);
                let out = frames(&ctx, &mut |ui: &mut egui::Ui| {
                    console.show(ui, &shared, &[])
                });
                let at = rect_of(&node(&out, Role::Button, button));
                let fills: Vec<egui::Color32> = rects(&out)
                    .into_iter()
                    .filter(|r| r.rect == at)
                    .map(|r| r.fill)
                    .collect();
                assert_eq!(fills, [p.raised], "{theme:?} {button}");
                assert_ne!(p.raised, p.field, "{theme:?} {button}: like a field");
            }
        }
    }

    #[test]
    fn the_pagers_on_the_cards_are_raised_and_the_sessions_one_is_not() {
        let rows = |n: usize, row: fn(usize) -> Value| -> Vec<Value> { (0..n).map(row).collect() };
        let observations = rows(50, |i| json!({"id": format!("e{i}"), "kind": "stop"}));
        let versions = rows(50, |i| json!({"id": format!("v{i}"), "body_chars": 1}));
        let sessions = rows(
            50,
            |i| json!({"session_id": format!("s{i}"), "agent_kind": "codex"}),
        );
        let pages = rows(
            250,
            |i| json!({"title": format!("Página {i}"), "path": "p.md"}),
        );
        let edges = rows(
            250,
            |i| json!({"from_path": format!("a{i}.md"), "to_path": "b.md"}),
        );
        // (caso, consulta, resposta, página local?, num cartão?)
        let cases = [
            (
                "observations",
                Query::Observations {
                    scope: fixture(),
                    session: "s1".into(),
                    offset: 50,
                    text: String::new(),
                    kind: String::new(),
                },
                json!({"observations": observations, "total": 500}),
                false,
                true,
            ),
            (
                "history",
                Query::History {
                    scope: fixture(),
                    path: "a.md".into(),
                    offset: 50,
                    from: String::new(),
                    to: String::new(),
                },
                json!({"versions": versions, "total": 500, "diff": {}}),
                false,
                true,
            ),
            ("pages", Query::Pages(fixture()), json!(pages), true, true),
            (
                "cross graph",
                Query::CrossGraph,
                json!({"edges": edges}),
                true,
                true,
            ),
            // A das sessões fica sobre o fundo da página, fora de cartão.
            (
                "sessions",
                Query::Sessions {
                    scope: fixture(),
                    offset: 50,
                    include_open: true,
                },
                json!({"sessions": sessions}),
                false,
                false,
            ),
        ];
        for (theme, p) in THEMES {
            for (case, query, data, local, on_card) in cases.clone() {
                let (_mock, shared, mut console) = explore(true);
                console.result = Some((query, data));
                let ctx = context(theme);
                let mut add = |ui: &mut egui::Ui| console.show(ui, &shared, &[]);
                // A página local abre na primeira: na segunda, as duas valem.
                if local {
                    press(&ctx, &mut add, Role::Button, "Próxima");
                }
                // O ponteiro sai: sem o surf2 de quem está sob ele.
                run_sized(&ctx, TALL, vec![egui::Event::PointerGone], &mut add);
                let out = frames(&ctx, &mut add);
                if local {
                    node(&out, Role::Label, "101–200 de 250");
                }
                let want = if on_card { p.raised } else { p.surf };
                for button in ["Anterior", "Próxima"] {
                    let found = node(&out, Role::Button, button);
                    assert!(!found.is_disabled(), "{theme:?} {case} {button}");
                    let at = rect_of(&found);
                    let fills: Vec<egui::Color32> = rects(&out)
                        .into_iter()
                        .filter(|r| r.rect == at)
                        .map(|r| r.fill)
                        .collect();
                    assert_eq!(fills, [want], "{theme:?} {case} {button}");
                    if on_card {
                        assert!(
                            !fills.contains(&p.field),
                            "{theme:?} {case} {button}: like a field"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_selects_on_the_cards_are_raised_and_the_project_one_is_not() {
        // (consulta, resposta, os seletores no cartão dela)
        let cases = [
            (
                Query::Observations {
                    scope: fixture(),
                    session: "s1".into(),
                    offset: 0,
                    text: String::new(),
                    kind: String::new(),
                },
                json!({"observations": [{"id": "e1", "kind": "stop"}], "total": 1}),
                &["Tipo de evento"][..],
            ),
            (
                Query::History {
                    scope: fixture(),
                    path: "a.md".into(),
                    offset: 0,
                    from: String::new(),
                    to: String::new(),
                },
                json!({"versions": [{"id": "v1", "body_chars": 1}], "total": 1, "diff": {}}),
                &["Comparar de", "Até"][..],
            ),
        ];
        for (theme, p) in THEMES {
            for (query, data, on_card) in cases.clone() {
                let (_mock, shared, mut console) = explore(true);
                console.result = Some((query, data));
                let ctx = context(theme);
                let mut add = |ui: &mut egui::Ui| console.show(ui, &shared, &[]);
                // O ponteiro sai: sem o surf2 de quem está sob ele.
                run_sized(&ctx, TALL, vec![egui::Event::PointerGone], &mut add);
                let out = frames(&ctx, &mut add);
                let fill = |label: &str| -> Vec<egui::Color32> {
                    let at = rect_of(&node(&out, Role::ComboBox, label));
                    rects(&out)
                        .into_iter()
                        .filter(|r| r.rect == at)
                        .map(|r| r.fill)
                        .collect()
                };
                for label in on_card {
                    let case = format!("{theme:?} {label}");
                    assert_eq!(fill(label), [p.raised], "{case}: on its card");
                    assert_ne!(fill(label), [p.field], "{case}: like a field");
                }
                // O do projeto fica na linha de ferramentas, sobre o fundo.
                assert_eq!(fill("Projeto"), [p.surf], "{theme:?} Projeto");
            }
        }
    }

    #[test]
    fn the_project_selector_widens_up_to_the_tabs() {
        // (projeto, cabe inteiro?)
        for (project, whole) in [
            (
                "acme-servicos-portal-coleta-zero-carbono-v2-e-mais-um-tanto".to_owned(),
                true,
            ),
            ("um-projeto".repeat(30), false),
        ] {
            let (_mock, shared, mut console) = explore(true);
            console.scope = Some(Scope {
                workspace: "fixture".into(),
                project,
            });
            let label = console.scope.as_ref().unwrap().label();
            let ctx = context(egui::Theme::Light);
            let out = frames(&ctx, &mut |ui: &mut egui::Ui| {
                console.show(ui, &shared, &[])
            });
            let select = rect_of(&node(&out, Role::ComboBox, "Projeto"));
            let tab = rect_of(&node(&out, Role::Button, "Conhecimento"));
            let elided: Vec<bool> = crate::widgets::tests::shapes(&out)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t) if t.galley.text() == label => Some(t.galley.elided),
                    _ => None,
                })
                .collect();
            let case = format!("{} chars: {select:?} {tab:?}", label.len());
            assert_eq!(elided, [!whole], "{case}");
            assert!(select.width() > 260., "{case}");
            // As abas seguem na linha, dentro da largura (1000 − 2 × 8).
            assert!(tab.top() < select.bottom(), "{case}: the tabs wrapped");
            assert!(
                tab.right() <= TALL[0] - 8. + 0.5,
                "{case}: the tabs overflow"
            );
        }
    }

    #[test]
    fn explore_controls_have_line_borders_and_4_5_to_1_text_in_both_themes() {
        use crate::widgets::tests::audit;
        for (theme, p) in THEMES {
            for tab in [Tab::Sessions, Tab::Knowledge] {
                let (_mock, shared, mut console) = explore(true);
                console.tab = tab;
                console.search = "texto".into();
                console.result = Some(if tab == Tab::Sessions {
                    // A paginação no meio (as duas setas valem) e o filtro de eventos.
                    let rows: Vec<Value> = (0..50)
                        .map(|i| json!({"id": format!("e{i}"), "kind": "stop", "title": "t"}))
                        .collect();
                    (
                        Query::Observations {
                            scope: fixture(),
                            session: "s1".into(),
                            offset: 50,
                            text: String::new(),
                            kind: String::new(),
                        },
                        json!({"observations": rows, "total": 500}),
                    )
                } else {
                    let rows: Vec<Value> = (0..150)
                        .map(|i| json!({"title": format!("Página {i}"), "path": "p.md"}))
                        .collect();
                    (Query::Pages(fixture()), json!(rows))
                });
                let ctx = context(theme);
                let out = frames(&ctx, &mut |ui: &mut egui::Ui| {
                    console.show(ui, &shared, &[])
                });
                let audit = audit(&out, p);
                let case = format!(
                    "{theme:?} {}",
                    if tab == Tab::Sessions {
                        "sessions"
                    } else {
                        "knowledge"
                    }
                );
                assert!(audit.failures.is_empty(), "{case}: {:#?}", audit.failures);
                let mut want = vec![
                    "ComboBox Projeto",
                    "Button Sessões detalhadas",
                    "Button Próxima",
                ];
                if tab == Tab::Sessions {
                    want.extend([
                        "CheckBox Incluir abertas",
                        "Button Carregar sessões",
                        "Button Handoffs abertos",
                        "Button Anterior",
                        "ComboBox Tipo de evento",
                        "Button Aplicar filtro",
                    ]);
                } else {
                    want.extend([
                        "CheckBox Todos os projetos autorizados",
                        "Button Páginas",
                        "Button Buscar",
                        "TextInput texto",
                    ]);
                }
                for name in want {
                    assert!(
                        audit.checked.iter().any(|c| c == name),
                        "{case}: {name} was measured, {:#?}",
                        audit.checked
                    );
                }
                assert!(
                    audit.checked.iter().any(|c| c.starts_with("segmented at ")),
                    "{case}: the tabs"
                );
            }
        }
    }
}
