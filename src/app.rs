use crate::{model::*, monitor::Shared};
use chrono::{DateTime, Local, Utc};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use std::{
    collections::VecDeque,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

const BG: Color32 = Color32::from_rgb(15, 18, 24);
const PANEL: Color32 = Color32::from_rgb(23, 28, 36);
const LINE: Color32 = Color32::from_rgb(43, 51, 63);
const TEXT: Color32 = Color32::from_rgb(229, 235, 242);
const MUTED: Color32 = Color32::from_rgb(139, 153, 171);
const GREEN: Color32 = Color32::from_rgb(102, 224, 184);
const BLUE: Color32 = Color32::from_rgb(116, 164, 255);
const AMBER: Color32 = Color32::from_rgb(244, 190, 103);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Overview,
    Central,
    Management,
    Advanced,
    Sessions,
    Docker,
    Settings,
}
type Sample = ResourceSample;

pub struct MonitorApp {
    #[cfg(target_os = "macos")]
    menu_bar: Option<crate::menu_bar::MenuBar>,
    shared: Arc<Shared>,
    config: Config,
    draft: Config,
    resources: ResourceSnapshot,
    api: ApiSnapshot,
    page: Page,
    docker_tab: usize,
    session_tab: usize,
    handoff_page: usize,
    filter: String,
    scope: String,
    only_open: bool,
    running_only: bool,
    history: VecDeque<Sample>,
    minutes: i64,
    last_sync: Instant,
    settings_message: String,
    selected_container: Option<String>,
    paused: bool,
    central: crate::console::Console,
    health: crate::health::Snapshot,
    management: crate::management_ui::Management,
    authentication: crate::auth::Panel,
    operations: crate::operations_ui::Panel,
}

impl MonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = PANEL;
        style.visuals.extreme_bg_color = BG;
        style.visuals.faint_bg_color = Color32::from_rgb(27, 32, 41);
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.selection.bg_fill = Color32::from_rgb(39, 70, 65);
        style.visuals.selection.stroke = Stroke::new(1.0_f32, GREEN);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(33, 40, 51);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(33, 40, 51);
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
        style.spacing.item_spacing = Vec2::new(10., 10.);
        style.spacing.button_padding = Vec2::new(12., 8.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.));
        cc.egui_ctx.set_style(style);
        let shared = Shared::start(config.clone());
        #[cfg(target_os = "macos")]
        let menu_bar = crate::menu_bar::MenuBar::new(
            cc.egui_ctx.clone(),
            shared.clone(),
            std::env::args().any(|a| a == "--show-dashboard"),
        );
        let central = crate::console::Console::new(shared.clone());
        let management = crate::management_ui::Management::new(shared.clone());
        let operations = crate::operations_ui::Panel::new(shared.clone());
        Self {
            management,
            operations,
            authentication: crate::auth::Panel::new(),
            central,
            health: Default::default(),
            #[cfg(target_os = "macos")]
            menu_bar,
            shared,
            draft: config.clone(),
            config,
            resources: Default::default(),
            api: Default::default(),
            page: Page::Overview,
            docker_tab: 0,
            session_tab: 0,
            handoff_page: 0,
            filter: String::new(),
            scope: String::new(),
            only_open: true,
            running_only: false,
            history: VecDeque::new(),
            minutes: 5,
            last_sync: Instant::now() - Duration::from_secs(1),
            settings_message: String::new(),
            selected_container: None,
            paused: false,
        }
    }

    fn sync(&mut self) {
        if self.paused || self.last_sync.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_sync = Instant::now();
        self.health = self.shared.health.lock().unwrap().clone();
        let mut resources_changed = false;
        {
            let incoming = self.shared.resources.lock().unwrap();
            if incoming.at != self.resources.at {
                self.resources = incoming.clone();
                resources_changed = true;
            }
        }
        {
            let incoming = self.shared.api.lock().unwrap();
            if incoming.at != self.api.at || incoming.error != self.api.error {
                self.api = incoming.clone();
            }
        }
        if resources_changed || self.history.is_empty() {
            self.history = self.shared.history.lock().unwrap().clone();
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(198.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(19, 23, 30))
                    .inner_margin(20),
            )
            .show(ctx, |ui| {
                ui.add_space(12.);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(30.), egui::Sense::hover());
                    ui.painter().rect_filled(r, 8., GREEN);
                    let pts = [
                        (4., 16.),
                        (10., 16.),
                        (13., 7.),
                        (18., 24.),
                        (22., 13.),
                        (27., 13.),
                    ]
                    .map(|(x, y)| r.min + Vec2::new(x, y));
                    ui.painter()
                        .add(egui::Shape::line(pts.to_vec(), Stroke::new(2.0_f32, BG)));
                    ui.label(RichText::new("AI Monitor").size(21.).strong());
                });
                ui.add_space(6.);
                ui.label(RichText::new("LOCAL OBSERVABILITY").size(9.5).color(MUTED));
                ui.add_space(38.);
                ui.label(RichText::new("MONITORAMENTO").size(10.).color(MUTED));
                ui.add_space(8.);
                for (page, label) in [
                    (Page::Overview, "Visão geral"),
                    (Page::Central, "Central ai-memory"),
                    (Page::Management, "Administrar memória"),
                    (Page::Advanced, "Administração avançada"),
                    (Page::Sessions, "Sessões e handoffs"),
                    (Page::Docker, "Docker"),
                    (Page::Settings, "Conexão"),
                ] {
                    let selected = self.page == page;
                    let button = egui::Button::new(RichText::new(label).color(if selected {
                        GREEN
                    } else {
                        MUTED
                    }))
                    .fill(if selected {
                        Color32::from_rgb(30, 54, 49)
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::NONE)
                    .corner_radius(7.);
                    if ui.add_sized([158., 42.], button).clicked() {
                        self.page = page;
                        self.filter.clear();
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION"), "  ·  Rust / macOS"))
                            .size(11.)
                            .color(MUTED),
                    );
                    ui.add_space(8.);
                    ui.separator();
                    ui.add_space(6.);
                    status(
                        ui,
                        "Docker",
                        self.resources.at.is_some() && self.resources.docker_error.is_none(),
                    );
                    status(
                        ui,
                        "ai-memory API",
                        self.api.at.is_some() && self.api.error.is_none(),
                    );
                    ui.add_space(6.);
                    ui.label(RichText::new("FONTES CONECTADAS").size(10.).color(MUTED));
                });
            });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let (title, subtitle) = match self.page {
            Page::Overview => (
                "Visão geral",
                "Recursos, sessões e continuidade do seu ambiente.",
            ),
            Page::Sessions => (
                "Sessões e handoffs",
                "Acompanhe sessões e trabalho aguardando continuidade.",
            ),
            Page::Docker => (
                "Docker",
                "Consumo, contêineres e imagens do contexto Docker ativo.",
            ),
            Page::Advanced => (
                "Administração avançada",
                "Projetos, identidade, mensagens e operação local.",
            ),
            Page::Settings => ("Conexão", "Ajuste as fontes e a frequência de atualização."),
            Page::Management => (
                "Administrar memória",
                "Revisão, manutenção e recuperação do conhecimento.",
            ),
            Page::Central => (
                "Central ai-memory",
                "Saúde, sessões e conhecimento do servidor conectado.",
            ),
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading(RichText::new(title).size(29.).strong());
                ui.label(RichText::new(subtitle).color(MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(if self.paused {
                        "Retomar"
                    } else {
                        "Pausar tela"
                    })
                    .clicked()
                {
                    self.paused = !self.paused;
                    if !self.paused {
                        self.last_sync = Instant::now() - Duration::from_secs(1);
                    }
                }
                badge(
                    ui,
                    if self.paused { "PAUSADO" } else { "AO VIVO" },
                    if self.paused { AMBER } else { GREEN },
                );
            });
        });
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Recursos {}  ·  API {}",
                    age(self.resources.at),
                    age(self.api.at)
                ))
                .small()
                .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} s / {} s",
                        self.config.resource_seconds, self.config.api_seconds
                    ))
                    .small()
                    .color(MUTED),
                );
            });
        });
        ui.add_space(8.);
        if let Some(e) = &self.resources.docker_error {
            notice(ui, e, AMBER);
        }
        if let Some(e) = &self.api.error {
            notice(ui, e, AMBER);
        }
        if !self.api.errors.is_empty() {
            notice(
                ui,
                &format!(
                    "Dados parciais: {} aviso(s). Consulte Conexão.",
                    self.api.errors.len()
                ),
                AMBER,
            );
        }
        if let Some(at) = self.api.at
            && (Utc::now() - at).num_seconds() > self.config.api_seconds as i64 * 3
            && !self.paused
        {
            notice(
                ui,
                "A atualização da API está atrasada. Os horários indicam a última coleta concluída.",
                AMBER,
            );
        }
    }

    fn ai_cards(&self, ui: &mut egui::Ui) {
        let usage = self.resources.ai_usage(&self.config.container);
        ui.columns(4, |cols| {
            metric(
                &mut cols[0],
                "CPU · AI-MEMORY",
                usage.as_ref().map(|u| format!("{:.1}%", u.cpu)),
                "100% = um núcleo",
                GREEN,
            );
            metric(
                &mut cols[1],
                "MEMÓRIA · AI-MEMORY",
                usage.as_ref().map(|u| bytes(u.memory)),
                "Uso atual do serviço",
                BLUE,
            );
            metric(
                &mut cols[2],
                "THREADS / TAREFAS",
                usage.as_ref().and_then(|u| u.tasks.map(number_u64)),
                "Processos + threads Linux",
                TEXT,
            );
            metric(
                &mut cols[3],
                "HANDOFFS ABERTOS",
                self.api.at.map(|_| {
                    format!(
                        "{}{}",
                        number(self.api.handoff_total.unwrap_or(self.api.handoffs.len())),
                        if self.api.handoff_total.is_some() {
                            ""
                        } else {
                            "+"
                        }
                    )
                }),
                "Visíveis para esta conexão",
                AMBER,
            );
        });
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        self.ai_cards(ui);
        ui.add_space(14.);
        ui.horizontal(|ui| {
            section(ui, "Histórico de recursos");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for n in [10, 5, 1] {
                    ui.selectable_value(&mut self.minutes, n, format!("{n} min"));
                }
            });
        });
        let interval = self.config.resource_seconds;
        ui.columns(2, |cols| {
            chart(
                &mut cols[0],
                "CPU",
                &self.history,
                self.minutes,
                false,
                interval,
            );
            chart(
                &mut cols[1],
                "Memória",
                &self.history,
                self.minutes,
                true,
                interval,
            );
        });
        ui.add_space(14.);
        let total = self.resources.docker_total();
        ui.columns(4, |cols| {
            metric(
                &mut cols[0],
                "CPU · CONTÊINERES",
                total.as_ref().map(|u| format!("{:.1}%", u.cpu)),
                "Soma dos contêineres ativos",
                BLUE,
            );
            metric(
                &mut cols[1],
                "MEMÓRIA · CONTÊINERES",
                total.as_ref().map(|u| bytes(u.memory)),
                "Inclui o ai-memory",
                BLUE,
            );
            metric(
                &mut cols[2],
                "CONTÊINERES ATIVOS",
                self.resources
                    .at
                    .filter(|_| self.resources.docker_error.is_none())
                    .map(|_| {
                        format!(
                            "{} / {}",
                            self.resources
                                .containers
                                .iter()
                                .filter(|c| c.state == "running")
                                .count(),
                            self.resources.containers.len()
                        )
                    }),
                "Em execução / existentes",
                GREEN,
            );
            metric(
                &mut cols[3],
                "SESSÕES ABERTAS",
                self.api.open_session_links().map(number_u64),
                "Vínculos por projeto (API)",
                TEXT,
            );
        });
        ui.add_space(14.);
        ui.horizontal(|ui| {
            section(ui, "Contêineres em execução");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Ver todos").clicked() {
                    self.page = Page::Docker;
                }
            });
        });
        let running: Vec<_> = self
            .resources
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .collect();
        let prev_selected = self.selected_container.clone();
        container_table(ui, &running, &mut self.selected_container, 180.);
        if self.selected_container != prev_selected && self.selected_container.is_some() {
            self.page = Page::Docker;
            self.docker_tab = 0;
        }
        ui.add_space(16.);
        panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("ESTE MAC").small().color(MUTED));
                ui.label(format!("CPU {:.1}%", self.resources.host_cpu));
                ui.separator();
                ui.label(format!(
                    "Memória {} / {}",
                    bytes(self.resources.host_memory),
                    bytes(self.resources.host_total)
                ));
                ui.separator();
                ui.label(format!("{} projetos", self.api.projects.len()));
            });
        });
    }

    fn sessions(&mut self, ui: &mut egui::Ui) {
        let previous_filter = (self.filter.clone(), self.scope.clone());
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.session_tab,
                0,
                format!(
                    "Handoffs abertos ({})",
                    self.api.handoff_total.map(number).unwrap_or_else(
                        || if self.api.at.is_some() {
                            format!("{}+", number(self.api.handoffs.len()))
                        } else {
                            "—".into()
                        }
                    )
                ),
            );
            ui.selectable_value(&mut self.session_tab, 1, "Sessões");
            ui.selectable_value(&mut self.session_tab, 2, "Projetos");
        });
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Buscar projeto, agente ou ID…")
                    .desired_width(280.),
            );
            egui::ComboBox::from_id_salt("scope")
                .selected_text(if self.scope.is_empty() {
                    "Todos os projetos"
                } else {
                    &self.scope
                })
                .width(240.)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.scope, String::new(), "Todos os projetos");
                    for p in &self.api.projects {
                        let scope = format!("{} / {}", p.workspace_name, p.project_name);
                        ui.selectable_value(&mut self.scope, scope.clone(), scope);
                    }
                });
            if self.session_tab == 1 {
                ui.checkbox(&mut self.only_open, "Só abertas");
            }
        });
        ui.add_space(8.);
        if previous_filter != (self.filter.clone(), self.scope.clone()) {
            self.handoff_page = 0;
        }
        let needle = self.filter.to_lowercase();
        if self.session_tab == 0 {
            ui.label(
                RichText::new(
                    "Handoffs permanecem abertos até serem aceitos. Esta tela apenas consulta.",
                )
                .small()
                .color(MUTED),
            );
            if !self.api.warnings.is_empty() {
                ui.label(
                    RichText::new(handoff_warning_message(
                        self.api.handoffs.len(),
                        self.api.handoff_total,
                    ))
                    .color(AMBER),
                );
            }
            let hs: Vec<_> = self
                .api
                .handoffs
                .iter()
                .filter(|h| handoff_matches(h, &self.scope, &needle))
                .collect();
            if hs.is_empty() {
                empty(
                    ui,
                    "Nenhum handoff aberto",
                    if self.api.at.is_some() {
                        "Nenhum resultado visível para os filtros e as permissões atuais."
                    } else {
                        "Aguardando a consulta ao ai-memory."
                    },
                );
            }
            let pages = hs.len().div_ceil(20).max(1);
            self.handoff_page = self.handoff_page.min(pages - 1);
            if !hs.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} encontrados · página {} de {}",
                            number(hs.len()),
                            self.handoff_page + 1,
                            pages
                        ))
                        .small()
                        .color(MUTED),
                    );
                    if ui
                        .add_enabled(self.handoff_page > 0, egui::Button::new("Anterior"))
                        .clicked()
                    {
                        self.handoff_page -= 1;
                    }
                    if ui
                        .add_enabled(self.handoff_page + 1 < pages, egui::Button::new("Próxima"))
                        .clicked()
                    {
                        self.handoff_page += 1;
                    }
                });
            }
            for h in hs.into_iter().skip(self.handoff_page * 20).take(20) {
                panel().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        badge(ui, "ABERTO", AMBER);
                        ui.label(RichText::new(&h.scope).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(date(&h.at)).small().color(MUTED));
                        });
                    });
                    ui.label(
                        RichText::new(format!("{} · {}", h.agent, h.id))
                            .small()
                            .color(MUTED),
                    );
                    if h.redacted {
                        ui.label("Resumo restrito pelas permissões do servidor.");
                    } else {
                        ui.label(h.summary.as_deref().unwrap_or("Sem resumo."));
                    }
                    if let Some(steps) = &h.next_steps
                        && !steps.is_empty()
                    {
                        egui::CollapsingHeader::new("Próximos passos")
                            .id_salt(&h.id)
                            .show(ui, |ui| {
                                for step in steps {
                                    ui.label(format!("• {step}"));
                                }
                            });
                    }
                    if ui.small_button("Copiar ID").clicked() {
                        ui.ctx().copy_text(h.id.clone());
                    }
                });
                ui.add_space(4.);
            }
        } else if self.session_tab == 1 {
            ui.label(RichText::new("Últimas 100 sessões por projeto. Aberta = sem encerramento registrado; não comprova atividade atual. Vínculos entre projetos podem se repetir.").small().color(MUTED));
            let ss: Vec<_> = self
                .api
                .sessions
                .iter()
                .filter(|s| {
                    (!self.only_open || s.ended_at.is_none())
                        && (self.scope.is_empty() || s.scope == self.scope)
                        && format!("{} {} {}", s.scope, s.agent_kind, s.session_id)
                            .to_lowercase()
                            .contains(&needle)
                })
                .collect();
            ui.label(
                RichText::new(format!("{} registros", number(ss.len())))
                    .small()
                    .color(MUTED),
            );
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(100.))
                .column(Column::remainder().at_least(180.))
                .column(Column::initial(90.))
                .column(Column::initial(90.))
                .column(Column::initial(140.))
                .column(Column::initial(75.))
                .header(30., |mut h| {
                    for title in [
                        "ESTADO",
                        "PROJETO / SESSÃO",
                        "AGENTE",
                        "EVENTOS",
                        "INÍCIO",
                        "ID",
                    ] {
                        h.col(|ui| {
                            table_head(ui, title);
                        });
                    }
                })
                .body(|body| {
                    body.rows(54., ss.len(), |mut row| {
                        let s = ss[row.index()];
                        row.col(|ui| {
                            badge(
                                ui,
                                if s.ended_at.is_none() {
                                    "ABERTA"
                                } else {
                                    "ENCERRADA"
                                },
                                if s.ended_at.is_none() { GREEN } else { MUTED },
                            );
                        });
                        row.col(|ui| {
                            ui.vertical(|ui| {
                                ui.label(short_project(&s.scope)).on_hover_text(&s.scope);
                                ui.label(
                                    RichText::new(short_id(&s.session_id)).small().color(MUTED),
                                )
                                .on_hover_text(s.cwd.as_deref().unwrap_or(""));
                            });
                        });
                        row.col(|ui| {
                            ui.label(&s.agent_kind);
                        });
                        row.col(|ui| {
                            ui.label(number_u64(s.observation_count));
                        });
                        row.col(|ui| {
                            ui.label(date(&s.started_at));
                        });
                        row.col(|ui| {
                            if ui.small_button("Copiar").clicked() {
                                ui.ctx().copy_text(s.session_id.clone());
                            }
                        });
                    });
                });
        } else {
            ui.label(RichText::new("Atividade = último evento ou atualização informado pela API. Contagens por projeto podem compartilhar sessões.").small().color(MUTED));
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::remainder().at_least(200.))
                .column(Column::initial(70.))
                .column(Column::initial(80.))
                .column(Column::initial(100.))
                .column(Column::initial(150.))
                .header(30., |mut h| {
                    for title in ["PROJETO", "PÁGINAS", "ABERTAS", "EVENTOS", "ATIVIDADE"] {
                        h.col(|ui| {
                            table_head(ui, title);
                        });
                    }
                })
                .body(|mut body| {
                    for p in self.api.projects.iter().filter(|p| {
                        let scope = format!("{} / {}", p.workspace_name, p.project_name);
                        (self.scope.is_empty() || self.scope == scope)
                            && scope.to_lowercase().contains(&needle)
                    }) {
                        body.row(36., |mut row| {
                            row.col(|ui| {
                                ui.label(&p.project_name).on_hover_text(&p.workspace_name);
                            });
                            row.col(|ui| {
                                ui.label(number_u64(p.page_count));
                            });
                            row.col(|ui| {
                                ui.label(
                                    p.open_session_count.map(number_u64).unwrap_or("—".into()),
                                );
                            });
                            row.col(|ui| {
                                ui.label(p.observation_count.map(number_u64).unwrap_or("—".into()));
                            });
                            row.col(|ui| {
                                ui.label(
                                    p.last_activity.as_deref().map(date).unwrap_or("—".into()),
                                );
                            });
                        });
                    }
                });
        }
    }

    fn docker(&mut self, ui: &mut egui::Ui) {
        let total = self.resources.docker_total();
        ui.columns(4, |cols| {
            metric(
                &mut cols[0],
                "CPU · CONTÊINERES",
                total.as_ref().map(|u| format!("{:.1}%", u.cpu)),
                "100% = um núcleo",
                BLUE,
            );
            metric(
                &mut cols[1],
                "MEMÓRIA · CONTÊINERES",
                total.as_ref().map(|u| bytes(u.memory)),
                "Cache inativo excluído pelo Docker",
                BLUE,
            );
            metric(
                &mut cols[2],
                "THREADS / TAREFAS",
                total.as_ref().and_then(|u| u.tasks.map(number_u64)),
                "Processos + threads Linux",
                TEXT,
            );
            metric(
                &mut cols[3],
                "IMAGENS LOCAIS",
                self.resources
                    .images_at
                    .map(|_| number(self.resources.images.len())),
                "IDs únicos; tags agrupadas",
                GREEN,
            );
        });
        ui.add_space(16.);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.docker_tab, 0, "Contêineres");
            ui.selectable_value(&mut self.docker_tab, 1, "Imagens");
            ui.selectable_value(&mut self.docker_tab, 2, "Processos no Mac");
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Buscar nome, imagem ou ID…")
                    .desired_width(300.),
            );
            if self.docker_tab == 0 {
                ui.checkbox(&mut self.running_only, "Só em execução");
            }
        });
        ui.add_space(6.);
        let needle = self.filter.to_lowercase();
        if self.docker_tab == 0 {
            let cs: Vec<_> = self
                .resources
                .containers
                .iter()
                .filter(|c| {
                    (!self.running_only || c.state == "running")
                        && format!("{} {} {}", c.name, c.image, c.id)
                            .to_lowercase()
                            .contains(&needle)
                })
                .collect();
            container_table(ui, &cs, &mut self.selected_container, 420.);
            if let Some(c) = cs
                .iter()
                .copied()
                .find(|c| Some(&c.id) == self.selected_container.as_ref())
            {
                ui.add_space(10.);
                panel().show(ui, |ui| {
                    section(ui, &c.name);
                    ui.label(RichText::new(&c.image).color(MUTED));
                    ui.label(format!("Status: {}", c.status));
                    ui.label(format!(
                        "Portas: {}",
                        if c.ports.is_empty() { "—" } else { &c.ports }
                    ));
                    ui.label(format!(
                        "Rede acumulada: {}  ·  Disco acumulado: {}",
                        c.net_io, c.block_io
                    ));
                    if let Some(u) = &c.usage {
                        ui.label(format!(
                            "Limite de memória do contêiner: {}",
                            bytes(u.limit)
                        ));
                    }
                    if c.name == self.config.container {
                        ui.separator();
                        ui.label(RichText::new("Processos dentro do ai-memory").strong());
                        if let Some(e) = &self.resources.ai_process_error {
                            ui.label(e);
                        }
                        for (pid, name, threads) in &self.resources.ai_processes {
                            ui.label(format!("PID {pid}    {name}    {threads} threads"));
                        }
                    }
                    if ui.small_button("Copiar ID").clicked() {
                        ui.ctx().copy_text(c.id.clone());
                    }
                });
            }
        } else if self.docker_tab == 1 {
            if let Some(e) = &self.resources.inventory_error {
                notice(ui, e, AMBER);
            }
            ui.label(
                RichText::new(format!(
                    "Inventário {} · imagens associadas a contêineres parados continuam em uso.",
                    age(self.resources.images_at)
                ))
                .small()
                .color(MUTED),
            );
            ui.label(RichText::new("Tamanho lógico por imagem; camadas compartilhadas impedem somar estes valores como espaço real em disco.").small().color(MUTED));
            let images: Vec<_> = self
                .resources
                .images
                .iter()
                .filter(|i| {
                    format!("{} {}", i.tags.join(" "), i.id)
                        .to_lowercase()
                        .contains(&needle)
                })
                .collect();
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .column(Column::remainder().at_least(220.))
                .column(Column::initial(100.))
                .column(Column::initial(120.))
                .column(Column::initial(200.))
                .header(30., |mut h| {
                    for title in ["IMAGEM / TAGS", "TAMANHO", "USO", "CONTÊINERES"] {
                        h.col(|ui| {
                            table_head(ui, title);
                        });
                    }
                })
                .body(|body| {
                    body.rows(54., images.len(), |mut row| {
                        let i = images[row.index()];
                        row.col(|ui| {
                            ui.vertical(|ui| {
                                ui.label(i.tags.first().map(String::as_str).unwrap_or("<sem tag>"))
                                    .on_hover_text(i.tags.join("\n"));
                                ui.label(RichText::new(short_id(&i.id)).small().color(MUTED));
                            });
                        });
                        row.col(|ui| {
                            ui.label(bytes(i.size));
                        });
                        row.col(|ui| {
                            badge(
                                ui,
                                if i.running > 0 {
                                    "EM EXECUÇÃO"
                                } else if !i.containers.is_empty() {
                                    "ASSOCIADA"
                                } else {
                                    "SEM VÍNCULO"
                                },
                                if i.running > 0 { GREEN } else { MUTED },
                            );
                        });
                        row.col(|ui| {
                            ui.label(if i.containers.is_empty() {
                                "—".into()
                            } else {
                                i.containers.join(", ")
                            });
                        });
                    });
                });
        } else {
            ui.label(RichText::new("Processos do Docker Desktop e clientes ai-memory no macOS. RSS pode compartilhar páginas; não some ao consumo dos contêineres.").small().color(MUTED));
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::initial(80.))
                .column(Column::remainder())
                .column(Column::initial(90.))
                .column(Column::initial(110.))
                .column(Column::initial(90.))
                .header(30., |mut h| {
                    for title in ["PID", "PROCESSO", "CPU", "MEMÓRIA RSS", "THREADS"] {
                        h.col(|ui| {
                            table_head(ui, title);
                        });
                    }
                })
                .body(|mut body| {
                    for p in self
                        .resources
                        .processes
                        .iter()
                        .filter(|p| process_matches(p, &needle))
                    {
                        body.row(36., |mut row| {
                            row.col(|ui| {
                                ui.label(p.pid.to_string());
                            });
                            row.col(|ui| {
                                ui.label(&p.name).on_hover_text(&p.group);
                            });
                            row.col(|ui| {
                                ui.label(format!("{:.1}%", p.usage.cpu));
                            });
                            row.col(|ui| {
                                ui.label(bytes(p.usage.memory));
                            });
                            row.col(|ui| {
                                ui.label(p.usage.tasks.map(number_u64).unwrap_or("—".into()));
                            });
                        });
                    }
                });
        }
    }

    fn reconnect(&mut self, config: Config) {
        self.config = config.clone();
        self.draft = config.clone();
        self.shared.apply(config);
        self.central.reset();
        self.management.reset();
        self.operations.reset();
        self.health = Default::default();
        self.history.clear();
        self.resources = Default::default();
        self.api = Default::default();
        self.scope.clear();
        self.selected_container = None;
        self.handoff_page = 0;
        self.paused = false;
        self.last_sync = Instant::now() - Duration::from_secs(1);
    }
    fn settings(&mut self, ui: &mut egui::Ui) {
        panel().show(ui,|ui| {
            section(ui,"Fontes de dados");
            egui::Grid::new("config").num_columns(2).spacing([28.,16.]).show(ui,|ui| {
                ui.label("Servidor ai-memory");if ui.add(egui::TextEdit::singleline(&mut self.draft.endpoint).desired_width(400.)).changed(){self.draft.token.clear();self.draft.human=None;self.draft.proposal_watch=None;self.settings_message="Servidor alterado. Informe o token desse endereço ou use login humano após conectar.".into();}ui.end_row();
                ui.label("Token Bearer (opcional)");ui.add(egui::TextEdit::singleline(&mut self.draft.token).password(true).desired_width(400.));ui.end_row();
                ui.label("Contêiner ai-memory");ui.add(egui::TextEdit::singleline(&mut self.draft.container).desired_width(400.));ui.end_row();
                ui.label("Executável Docker");ui.add(egui::TextEdit::singleline(&mut self.draft.docker_binary).desired_width(400.));ui.end_row();
                ui.label("Atualizar recursos");ui.add(egui::DragValue::new(&mut self.draft.resource_seconds).range(1..=60).suffix(" s"));ui.end_row();
                ui.label("Atualizar sessões / handoffs");ui.add(egui::DragValue::new(&mut self.draft.api_seconds).range(5..=300).suffix(" s"));ui.end_row();
            });
            ui.label("Propostas na barra de menus (um projeto, até 200 pendentes)");
            egui::ComboBox::from_id_salt("watch-proposals").width(460.).selected_text(self.draft.proposal_watch.as_ref().map(crate::client::Scope::label).unwrap_or("Desativado".into())).show_ui(ui,|ui|{
                if ui.selectable_value(&mut self.draft.proposal_watch,None,"Desativado").clicked(){ui.close();}
                for p in &self.api.projects {let scope=crate::client::Scope{workspace:p.workspace_name.clone(),project:p.project_name.clone()};let label=scope.label();if ui.selectable_value(&mut self.draft.proposal_watch,Some(scope),label).clicked(){ui.close();}}
            });
            ui.add_space(12.);ui.checkbox(&mut self.draft.remember_token,"Guardar o Bearer no Keychain para este servidor (opcional)");
            ui.label(RichText::new("Sem essa opção, o token fica somente em memória. Senhas e sessões humanas nunca são persistidas.").small().color(MUTED));
            if ui.add_enabled(!self.management.busy&&!self.authentication.busy&&!self.operations.busy,egui::Button::new("Carregar Bearer do Keychain deste endereço")).clicked(){
                self.draft.normalize();
                match crate::keychain::load(&self.draft.endpoint){
                    Ok(Some(token))=>{self.draft.token=token.trim().into();self.settings_message="Token carregado no campo. Salve para conectar.".into();},
                    Ok(None)=>self.settings_message="Nenhum token guardado para esse endereço.".into(),
                    Err(e)=>self.settings_message=e
                }
            }

            ui.horizontal(|ui| {
                if ui.add_enabled(!self.management.busy&&!self.authentication.busy&&!self.operations.busy, egui::Button::new(RichText::new("Salvar e conectar").color(GREEN))).clicked() {
                    self.draft.normalize();
                    match self.draft.validate().and_then(|_|{
                        if self.draft.remember_token {
                            crate::keychain::save(&self.draft.endpoint,&self.draft.token)?;
                        } else if self.config.remember_token {
                            crate::keychain::forget(&self.config.endpoint)?;
                        }
                        self.draft.save()
                    }) {
                        Ok(())=> {self.reconnect(self.draft.clone());self.authentication.clear();self.settings_message="Configuração salva. Reconectando…".into();},
                        Err(e)=>self.settings_message=e,
                    }
                }
                if ui.button("Restaurar campos padrão").clicked(){self.draft=Config::default();self.settings_message="Campos restaurados ao padrão. Clique em Salvar e conectar para aplicar.".into();}
            });
            if !self.settings_message.is_empty(){ui.label(&self.settings_message);}
        });
        ui.add_space(12.);
        panel().show(ui, |ui| {
            self.authentication.show(
                ui,
                &self.shared,
                !self.management.busy && !self.operations.busy,
            )
        });
        ui.add_space(12.);
        panel().show(ui,|ui| {
            section(ui,"Diagnóstico da conexão");ui.label(format!("ai-memory {}",if self.api.version.is_empty(){"—"}else{&self.api.version}));
            ui.label(format!("Recursos: {} · API: {}",age(self.resources.at),age(self.api.at)));
            ui.label("As consultas respeitam o usuário do token. Handoffs de outros usuários podem não estar visíveis.");
            if self.api.errors.is_empty()&&self.api.error.is_none(){ui.label(RichText::new(if self.api.at.is_some(){"Última coleta da API concluída sem erros."}else{"Conectando…"}).color(GREEN));}
            for e in &self.api.errors{ui.label(RichText::new(e).color(AMBER));}
            if let Some(e)=&self.resources.inventory_error{ui.label(RichText::new(e).color(AMBER));}
        });
    }
}

impl eframe::App for MonitorApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if let Some(config) = self.authentication.poll(&self.shared) {
            self.reconnect(config);
        }
        self.central.poll(&self.shared);
        self.management.poll(&self.shared);
        self.operations.poll(&self.shared);
        ctx.request_repaint_after(Duration::from_secs(1));
        match self.shared.navigation.swap(0, Ordering::AcqRel) {
            1 => self.page = Page::Central,
            2 => self.page = Page::Management,
            3 => self.page = Page::Advanced,
            _ => {}
        }
        #[cfg(target_os = "macos")]
        if let Some(menu_bar) = &self.menu_bar {
            if ctx.input(|i| i.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                menu_bar.hide();
            }
            if !menu_bar.is_visible() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                return;
            }
        }
        self.sync();
        ctx.request_repaint_after(Duration::from_millis(500));
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(26))
            .show(ctx, |ui| {
                self.header(ui);
                egui::ScrollArea::vertical()
                    .id_salt("main-scroll")
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        match self.page {
                            Page::Overview => self.overview(ui),
                            Page::Management => {
                                ui.add_enabled_ui(
                                    !self.authentication.busy && !self.operations.busy,
                                    |ui| self.management.show(ui, &self.shared, &self.api.projects),
                                );
                            }
                            Page::Advanced => {
                                ui.add_enabled_ui(
                                    !self.authentication.busy && !self.management.busy,
                                    |ui| {
                                        self.operations.show(ui, &self.shared, &self.api.projects);
                                    },
                                );
                            }
                            Page::Central => self.central.show(
                                ui,
                                &self.shared,
                                &self.api.projects,
                                &self.health,
                            ),
                            Page::Sessions => self.sessions(ui),
                            Page::Docker => self.docker(ui),
                            Page::Settings => self.settings(ui),
                        }
                    });
            });
    }
}
impl Drop for MonitorApp {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, LINE))
        .corner_radius(10.)
        .inner_margin(16)
}
fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(17.).strong());
}
fn table_head(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(10.5).color(MUTED));
}
fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::proportional(10.0), color);
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + Vec2::new(14.0, 8.0), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    ui.painter()
        .rect_filled(rect, 4.0, color.gamma_multiply(0.12));
    ui.painter()
        .galley(rect.min + Vec2::new(7.0, 4.0), galley, color);
}
fn status(ui: &mut egui::Ui, text: &str, ok: bool) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 14.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 3.0, if ok { GREEN } else { MUTED });
        ui.label(RichText::new(text).size(12.0).color(MUTED));
    });
}
fn notice(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.08))
        .inner_margin(10)
        .corner_radius(6.)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(text).small().color(color));
        });
    ui.add_space(4.);
}
fn empty(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.add_space(30.);
        ui.vertical_centered(|ui| {
            ui.heading(title);
            ui.label(RichText::new(subtitle).color(MUTED));
        });
        ui.add_space(30.);
    });
}
fn metric(ui: &mut egui::Ui, title: &str, value: Option<String>, hint: &str, color: Color32) {
    panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(title).size(10.5).color(MUTED));
        ui.add_space(5.);
        ui.label(
            RichText::new(value.unwrap_or("—".into()))
                .size(29.)
                .color(color),
        );
        ui.label(RichText::new(hint).size(11.).color(MUTED));
    });
}
fn number_u64(n: u64) -> String {
    number(n as usize)
}
fn age(at: Option<DateTime<Utc>>) -> String {
    at.map(|a| format!("há {} s", (Utc::now() - a).num_seconds().max(0)))
        .unwrap_or("aguardando".into())
}
fn date(at: &str) -> String {
    DateTime::parse_from_rfc3339(at)
        .map(|d| d.with_timezone(&Local).format("%d/%m %H:%M").to_string())
        .unwrap_or_else(|_| "—".into())
}
fn short_id(id: &str) -> String {
    id.chars().take(18).collect()
}
fn short_project(s: &str) -> &str {
    s.split(" / ").last().unwrap_or(s)
}

fn container_table(
    ui: &mut egui::Ui,
    cs: &[&Container],
    selected: &mut Option<String>,
    height: f32,
) {
    if cs.is_empty() {
        empty(
            ui,
            "Nenhum contêiner nesta lista",
            "Os dados aparecem quando o Docker está disponível e há contêineres compatíveis com o filtro.",
        );
        return;
    }
    TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .max_scroll_height(height)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::remainder().at_least(150.))
        .column(Column::initial(120.))
        .column(Column::initial(85.))
        .column(Column::initial(110.))
        .column(Column::initial(85.))
        .header(30., |mut h| {
            for title in ["CONTÊINER", "ESTADO", "CPU", "MEMÓRIA", "TAREFAS"] {
                h.col(|ui| {
                    table_head(ui, title);
                });
            }
        })
        .body(|body| {
            body.rows(52., cs.len(), |mut row| {
                let c = cs[row.index()];
                row.col(|ui| {
                    ui.vertical(|ui| {
                        if ui
                            .selectable_label(
                                selected.as_ref() == Some(&c.id),
                                RichText::new(&c.name).strong(),
                            )
                            .clicked()
                        {
                            *selected = Some(c.id.clone());
                        }
                        ui.label(RichText::new(&c.image).size(11.).color(MUTED))
                            .on_hover_text(&c.image);
                    });
                });
                row.col(|ui| {
                    badge(
                        ui,
                        if c.state == "running" {
                            "EM EXECUÇÃO"
                        } else if c.state == "exited" {
                            "PARADO"
                        } else {
                            &c.state
                        },
                        if c.state == "running" { GREEN } else { MUTED },
                    );
                });
                row.col(|ui| {
                    ui.label(
                        c.usage
                            .as_ref()
                            .map(|u| format!("{:.1}%", u.cpu))
                            .unwrap_or("—".into()),
                    );
                });
                row.col(|ui| {
                    ui.label(
                        c.usage
                            .as_ref()
                            .map(|u| bytes(u.memory))
                            .unwrap_or("—".into()),
                    );
                });
                row.col(|ui| {
                    ui.label(
                        c.usage
                            .as_ref()
                            .and_then(|u| u.tasks.map(number_u64))
                            .unwrap_or("—".into()),
                    );
                });
            });
        });
}

fn chart(
    ui: &mut egui::Ui,
    title: &str,
    history: &VecDeque<Sample>,
    minutes: i64,
    memory: bool,
    resource_seconds: u64,
) {
    panel().show(ui, |ui| {
        ui.horizontal(|ui| {
            section(ui, title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("Docker").small().color(BLUE));
                ui.label(RichText::new("ai-memory").small().color(GREEN));
            });
        });
        let end = history.back().map(|s| s.at).unwrap_or_else(Utc::now);
        let start = end - chrono::Duration::minutes(minutes);
        let value = |u: &Usage| {
            if memory {
                u.memory as f64 / (1u64 << 20) as f64
            } else {
                u.cpu
            }
        };
        let max = history
            .iter()
            .filter(|s| s.at >= start)
            .flat_map(|s| [s.ai.as_ref(), s.docker.as_ref()])
            .flatten()
            .map(value)
            .fold(if memory { 64. } else { 100. }, f64::max)
            * 1.15;
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 152.), egui::Sense::hover());
        let plot = rect.shrink2(Vec2::new(0., 12.));
        let plot = egui::Rect::from_min_max(plot.min + Vec2::new(46., 0.), plot.max);
        let painter = ui.painter();
        for i in 0..3 {
            let frac = i as f32 / 2.;
            let y = plot.bottom() - plot.height() * frac;
            painter.line_segment(
                [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
                Stroke::new(1.0_f32, LINE),
            );
            painter.text(
                egui::pos2(plot.left() - 8., y),
                egui::Align2::RIGHT_CENTER,
                format!("{:.0}{}", max * frac as f64, if memory { "M" } else { "%" }),
                egui::FontId::proportional(10.),
                MUTED,
            );
        }
        let plot_painter = painter.with_clip_rect(plot.expand(4.));
        for (ai, color) in [(false, BLUE), (true, GREEN)] {
            let (lines, dots) =
                chart_segments(history, minutes, max, memory, ai, resource_seconds, plot);
            for pts in lines {
                plot_painter.add(egui::Shape::line(pts, Stroke::new(2.0_f32, color)));
            }
            for pt in dots {
                plot_painter.circle_filled(pt, 3., color);
            }
        }
        if history.is_empty() {
            painter.text(
                plot.center(),
                egui::Align2::CENTER_CENTER,
                "Aguardando amostras…",
                egui::FontId::proportional(13.),
                MUTED,
            );
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("−{minutes} min"))
                    .small()
                    .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("agora").small().color(MUTED));
            });
        });
    });
}

fn chart_segments(
    history: &VecDeque<Sample>,
    minutes: i64,
    max: f64,
    memory: bool,
    ai: bool,
    resource_seconds: u64,
    plot: egui::Rect,
) -> (Vec<Vec<egui::Pos2>>, Vec<egui::Pos2>) {
    let end = history.back().map(|s| s.at).unwrap_or_else(Utc::now);
    let start = end - chrono::Duration::minutes(minutes);
    let value = |u: &Usage| {
        if memory {
            u.memory as f64 / (1u64 << 20) as f64
        } else {
            u.cpu
        }
    };
    let gap_limit = (resource_seconds.saturating_mul(3).max(65)) as i64;
    let mut lines = Vec::new();
    let mut dots = Vec::new();
    let mut points = Vec::new();
    let mut previous = None;
    for s in history.iter().filter(|s| s.at >= start) {
        let usage = if ai { s.ai.as_ref() } else { s.docker.as_ref() };
        if previous
            .is_some_and(|at: DateTime<Utc>| (s.at - at).num_seconds() > gap_limit || s.at < at)
            || usage.is_none()
        {
            if points.len() > 1 {
                lines.push(std::mem::take(&mut points));
            } else if let Some(p) = points.pop() {
                dots.push(p);
            }
        }
        if let Some(u) = usage {
            let x = (plot.left()
                + plot.width()
                    * ((s.at - start).num_milliseconds() as f32 / (minutes * 60 * 1000) as f32))
                .clamp(plot.left(), plot.right());
            let y = (plot.bottom() - plot.height() * (value(u) / max) as f32)
                .clamp(plot.top(), plot.bottom());
            points.push(egui::pos2(x, y));
        }
        previous = Some(s.at);
    }
    if points.len() > 1 {
        lines.push(points);
    } else if let Some(p) = points.pop() {
        dots.push(p);
    }
    (lines, dots)
}

fn handoff_matches(h: &Handoff, scope: &str, needle: &str) -> bool {
    let summary = if h.redacted {
        ""
    } else {
        h.summary.as_deref().unwrap_or("")
    };
    (scope.is_empty() || h.scope == scope)
        && format!("{} {} {} {}", h.scope, h.agent, h.id, summary)
            .to_lowercase()
            .contains(needle)
}

fn handoff_warning_message(loaded: usize, total: Option<usize>) -> String {
    match total {
        Some(total) => format!(
            "Lista: {} handoffs carregados de {} abertos. Até 200 por projeto.",
            number(loaded),
            number(total)
        ),
        None => format!(
            "Lista: {} handoffs carregados (total aberto indisponível). Até 200 por projeto.",
            number(loaded)
        ),
    }
}

fn process_matches(p: &HostProcess, needle: &str) -> bool {
    format!("{} {} {}", p.pid, p.name, p.group)
        .to_lowercase()
        .contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_preserves_isolated_samples_and_scales_gap_with_resource_interval() {
        let base = Utc::now();
        let u = Some(Usage {
            cpu: 50.0,
            memory: 128 << 20,
            ..Default::default()
        });
        // 1) An isolated valid sample followed by a failed (None) sample must still render as a dot.
        let history = VecDeque::from([
            Sample {
                at: base - chrono::Duration::seconds(10),
                ai: u.clone(),
                docker: None,
            },
            Sample {
                at: base - chrono::Duration::seconds(8),
                ai: None,
                docker: None,
            },
        ]);
        let plot = egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(200., 100.));
        let (lines, dots) = chart_segments(&history, 5, 100.0, false, true, 2, plot);
        assert!(lines.is_empty());
        assert_eq!(
            dots.len(),
            1,
            "isolated sample before a None gap must not be erased"
        );

        // 2) With resource_seconds = 60, samples 66s apart must connect as a line, not blank out.
        let history_60s = VecDeque::from([
            Sample {
                at: base - chrono::Duration::seconds(132),
                ai: u.clone(),
                docker: None,
            },
            Sample {
                at: base - chrono::Duration::seconds(66),
                ai: u.clone(),
                docker: None,
            },
            Sample {
                at: base,
                ai: u,
                docker: None,
            },
        ]);
        let (lines, _) = chart_segments(&history_60s, 5, 100.0, false, true, 60, plot);
        assert_eq!(
            lines.len(),
            1,
            "60s interval samples separated by 66s must form a connected line"
        );
        assert_eq!(lines[0].len(), 3);
    }

    #[test]
    fn redacted_handoff_summary_is_not_searchable_and_short_project_is_neutral() {
        let redacted = Handoff {
            id: "h-1".into(),
            agent: "claude".into(),
            at: "2026-01-01T00:00:00Z".into(),
            state: "open".into(),
            summary: Some("segredo-confidencial-interno".into()),
            redacted: true,
            next_steps: None,
            scope: "ws / proj".into(),
        };
        assert!(
            !handoff_matches(&redacted, "", "segredo-confidencial"),
            "redacted summary must not be searchable via filter"
        );
        let visible = Handoff {
            redacted: false,
            ..redacted
        };
        assert!(handoff_matches(&visible, "", "segredo-confidencial"));
        assert_eq!(
            short_project("default / cristiano-vallory-app"),
            "cristiano-vallory-app"
        );
        assert_eq!(
            handoff_warning_message(200, None),
            "Lista: 200 handoffs carregados (total aberto indisponível). Até 200 por projeto."
        );
        assert_eq!(
            handoff_warning_message(200, Some(350)),
            "Lista: 200 handoffs carregados de 350 abertos. Até 200 por projeto."
        );
        let proc = HostProcess {
            pid: 9876,
            name: "com.docker.backend".into(),
            group: "Docker Desktop".into(),
            usage: Default::default(),
        };
        assert!(process_matches(&proc, "9876"));
        assert!(process_matches(&proc, "docker desktop"));
        assert!(!process_matches(&proc, "ai-memory"));
    }
}
