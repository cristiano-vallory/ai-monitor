use crate::{management_ui::Section, model::*, monitor::Shared, theme};
use chrono::{DateTime, Local, Utc};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use std::{
    collections::VecDeque,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

enum KeychainChange<'a> {
    Save { endpoint: &'a str, token: &'a str },
    Forget(&'a str),
    Keep,
}
fn server_version<'a>(api: &'a ApiSnapshot, health: &'a crate::health::Snapshot) -> &'a str {
    if !api.version.is_empty() {
        return &api.version;
    }
    health
        .status
        .as_ref()
        .and_then(|s| s["version"].as_str())
        .unwrap_or("")
}
fn keychain_change<'a>(current: &'a Config, draft: &'a Config) -> KeychainChange<'a> {
    if draft.remember_token {
        KeychainChange::Save {
            endpoint: &draft.endpoint,
            token: &draft.token,
        }
    } else if current.remember_token {
        KeychainChange::Forget(&current.endpoint)
    } else {
        KeychainChange::Keep
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Page {
    Overview,
    Sessions,
    Docker,
    Health,
    Explore,
    Review,
    Maintenance,
    Admin,
    Settings,
}
impl Page {
    /// Nome da página na lateral.
    fn label(self) -> &'static str {
        match self {
            Page::Overview => "Visão geral",
            Page::Sessions => "Sessões e handoffs",
            Page::Docker => "Docker",
            Page::Health => "Saúde",
            Page::Explore => "Explorar",
            Page::Review => "Revisão",
            Page::Maintenance => "Manutenção e backup",
            Page::Admin => "Administração",
            Page::Settings => "Ajustes",
        }
    }
    /// Título na barra superior.
    fn title(self) -> &'static str {
        match self {
            Page::Health => "Saúde do ai-memory",
            page => page.label(),
        }
    }
    /// A página que mostra esta parte da `Management`.
    fn of(section: Section) -> Self {
        match section {
            Section::Review => Page::Review,
            Section::Maintenance => Page::Maintenance,
        }
    }
}
type Sample = ResourceSample;

/// Tamanho inicial e mínimo da janela, em pontos (macOS e Linux).
pub const WINDOW_SIZE: [f32; 2] = [1320., 860.];
pub const MIN_WINDOW_SIZE: [f32; 2] = [720., 560.];
/// Abaixo desta largura de janela a lateral fica compacta (só ícones).
const COMPACT_BELOW: f32 = 1000.;
const SIDEBAR_WIDTH: f32 = 232.;
/// No macOS os botões da janela vão até x = 70, acima da lateral compacta.
#[cfg(target_os = "macos")]
const COMPACT_SIDEBAR_WIDTH: f32 = 78.;
#[cfg(not(target_os = "macos"))]
const COMPACT_SIDEBAR_WIDTH: f32 = 64.;
fn compact(width: f32) -> bool {
    width < COMPACT_BELOW
}

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
    /// Último `SetTheme` enviado à janela nativa; `None` = nada enviado ao
    /// contexto atual.
    window_theme: Option<egui::SystemTheme>,
    /// `Shared::health_served` lido junto com `health`: até que pedido de
    /// "Atualizar agora" a saúde na tela atende.
    health_served: u64,
}

impl MonitorApp {
    /// Aplica o tema resolvido a `ctx` e à janela nativa (barra de título;
    /// decorações CSD no Linux). Cada um só é enviado quando muda.
    pub(crate) fn apply_theme(&mut self, ctx: &egui::Context) {
        let platform = theme::platform_theme();
        let wanted = theme::resolve(self.config.theme, platform);
        if ctx.options(|o| o.theme_preference) != wanted {
            ctx.set_theme(wanted);
        }
        let window = theme::window_theme(self.config.theme, platform);
        if self.window_theme != Some(window) {
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(window));
            self.window_theme = Some(window);
        }
    }
    /// Janela nova no Linux: o contexto é outro e ainda não recebeu nada.
    #[cfg(target_os = "linux")]
    pub(crate) fn attach(&mut self, ctx: &egui::Context) {
        self.window_theme = None;
        self.apply_theme(ctx);
    }
    /// Troca de aparência: vale na hora, grava só o tema (`save_theme`) sem
    /// reconectar e não mexe no rascunho do usuário além do próprio tema.
    fn choose_theme(
        &mut self,
        choice: Appearance,
        save: impl FnOnce(Appearance) -> Result<(), String>,
    ) {
        self.config.theme = choice;
        self.draft.theme = choice;
        self.shared.config.write().unwrap().theme = choice;
        if let Err(e) = save(choice) {
            self.settings_message = e;
        }
    }
    fn restore_defaults(&mut self) {
        self.draft = Config {
            theme: self.config.theme,
            ..Config::default()
        };
        self.settings_message =
            "Campos restaurados ao padrão. Clique em Salvar e conectar para aplicar.".into();
    }
    fn build(config: Config, shared: Arc<Shared>) -> Self {
        let central = crate::console::Console::new(shared.clone());
        let management = crate::management_ui::Management::new(shared.clone());
        let operations = crate::operations_ui::Panel::new(shared.clone());
        Self {
            management,
            operations,
            window_theme: None,
            authentication: crate::auth::Panel::new(),
            central,
            health: Default::default(),
            health_served: 0,
            #[cfg(target_os = "macos")]
            menu_bar: None,
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
    #[cfg(target_os = "macos")]
    pub fn new(cc: &eframe::CreationContext<'_>, config: Config) -> Self {
        theme::install(&cc.egui_ctx);
        let shared = Shared::start(config.clone());
        let mut app = Self::build(config, shared.clone());
        app.apply_theme(&cc.egui_ctx);
        app.menu_bar = crate::menu_bar::MenuBar::new(
            cc.egui_ctx.clone(),
            shared,
            std::env::args().any(|a| a == "--show-dashboard"),
        );
        app
    }
    /// Construtor Linux: reutiliza o Shared já iniciado (a janela é criada e
    /// destruída sob demanda; o app e os coletores vivem no processo). Fontes,
    /// estilos e tema de cada janela são aplicados por linux::prepare_window.
    #[cfg(target_os = "linux")]
    pub fn for_shared(shared: Arc<Shared>) -> Self {
        let config = shared.config.read().unwrap().clone();
        Self::build(config, shared)
    }

    fn sync(&mut self) {
        if self.paused || self.last_sync.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_sync = Instant::now();
        {
            // O coletor grava as duas sob este lock.
            let health = self.shared.health.lock().unwrap();
            self.health = health.clone();
            self.health_served = self.shared.health_served.load(Ordering::Acquire);
        }
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

    fn sidebar(&mut self, ctx: &egui::Context, compact: bool) {
        let p = theme::palette(&ctx.style().visuals);
        egui::SidePanel::left("sidebar")
            .exact_width(if compact {
                COMPACT_SIDEBAR_WIDTH
            } else {
                SIDEBAR_WIDTH
            })
            .resizable(false)
            .frame(egui::Frame::new().fill(p.side).inner_margin(egui::Margin {
                left: 10,
                right: 10,
                top: 0,
                bottom: 12,
            }))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0., 1.);
                let (top, strip) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), 52.),
                    egui::Sense::hover(),
                );
                // macOS: a faixa inteira, sob os botões da janela, arrasta a janela.
                #[cfg(target_os = "macos")]
                drag_window(&ui.interact(
                    top.expand2(Vec2::new(10., 0.)),
                    strip.id.with("drag"),
                    egui::Sense::click_and_drag(),
                ));
                #[cfg(not(target_os = "macos"))]
                {
                    strip.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "AI Monitor")
                    });
                    let logo = if compact {
                        egui::Rect::from_center_size(top.center(), Vec2::splat(22.))
                    } else {
                        egui::Rect::from_min_size(
                            egui::pos2(top.left() + 10., top.center().y - 11.),
                            Vec2::splat(22.),
                        )
                    };
                    let painter = ui.painter();
                    painter.rect_filled(logo, 6., p.acc);
                    let scale = logo.width() / 30.;
                    let pts = [
                        (4., 16.),
                        (10., 16.),
                        (13., 7.),
                        (18., 24.),
                        (22., 13.),
                        (27., 13.),
                    ]
                    .map(|(x, y)| logo.min + Vec2::new(x, y) * scale);
                    painter.add(egui::Shape::line(
                        pts.to_vec(),
                        Stroke::new(2.0 * scale, p.on_acc),
                    ));
                    if !compact {
                        painter.text(
                            egui::pos2(logo.right() + 8., logo.center().y),
                            egui::Align2::LEFT_CENTER,
                            "AI Monitor",
                            egui::FontId::new(13., theme::semibold()),
                            p.text,
                        );
                    }
                }
                let badge = handoff_badge(&self.api);
                let pending = review_badge(&self.health, &self.config, Utc::now());
                for (title, pages) in [
                    (
                        "Monitorar",
                        &[Page::Overview, Page::Sessions, Page::Docker][..],
                    ),
                    (
                        "ai-memory",
                        &[
                            Page::Health,
                            Page::Explore,
                            Page::Review,
                            Page::Maintenance,
                            Page::Admin,
                        ][..],
                    ),
                ] {
                    if !compact {
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: 10,
                                right: 10,
                                top: 8,
                                bottom: 4,
                            })
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(title)
                                        .size(11.)
                                        .family(theme::semibold())
                                        .color(p.text2),
                                );
                            });
                    }
                    for &page in pages {
                        let aside = match page {
                            Page::Sessions => badge.clone().map(Aside::Count),
                            Page::Review => pending.clone().map(Aside::Warning),
                            _ => None,
                        };
                        if nav_item(ui, page, self.page == page, compact, aside).clicked() {
                            self.page = page;
                            self.filter.clear();
                        }
                    }
                    ui.add_space(10.);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let settings = nav_item(
                        ui,
                        Page::Settings,
                        self.page == Page::Settings,
                        compact,
                        Some(Aside::Shortcut(crate::platform::SETTINGS_SHORTCUT)),
                    );
                    if settings.clicked() {
                        self.page = Page::Settings;
                        self.filter.clear();
                    }
                    // Fontes, de baixo para cima, entre bordas de 10 px e a linha.
                    ui.add_space(16.);
                    source(
                        ui,
                        "Docker",
                        "Docker",
                        self.resources.at.is_some(),
                        self.resources.docker_error.as_deref(),
                        compact,
                    );
                    ui.add_space(5.);
                    let version = server_version(&self.api, &self.health);
                    source(
                        ui,
                        "ai-memory",
                        &if version.is_empty() {
                            "ai-memory API".to_owned()
                        } else {
                            format!("ai-memory {version}")
                        },
                        self.api.at.is_some(),
                        self.api.error.as_deref(),
                        compact,
                    );
                    ui.add_space(10.);
                    let (line, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 1.),
                        egui::Sense::hover(),
                    );
                    ui.painter().hline(
                        line.x_range(),
                        line.center().y,
                        Stroke::new(1.0_f32, p.line),
                    );
                });
            });
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        let p = theme::palette(&ctx.style().visuals);
        egui::TopBottomPanel::top("top-bar")
            .exact_height(52.)
            .frame(egui::Frame::new().fill(p.bg))
            .show(ctx, |ui| {
                let bar = ui.max_rect();
                // macOS: o fundo arrasta a janela. Vem antes dos widgets, que
                // ficam por cima dele.
                #[cfg(target_os = "macos")]
                drag_window(&ui.interact(bar, ui.id().with("drag"), egui::Sense::click_and_drag()));
                let inner = egui::Rect::from_x_y_ranges(
                    bar.left() + 24.0..=bar.right() - 20.,
                    bar.y_range(),
                );
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 12.;
                        if pause_button(ui, self.paused).clicked() {
                            self.paused = !self.paused;
                            if !self.paused {
                                self.last_sync = Instant::now() - Duration::from_secs(1);
                            }
                        }
                        freshness_chip(
                            ui,
                            freshness(
                                Utc::now(),
                                self.resources.at,
                                self.api.at,
                                self.config.resource_seconds,
                                self.config.api_seconds,
                                self.paused,
                            ),
                        );
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(self.page.title())
                                        .size(15.)
                                        .family(theme::semibold()),
                                )
                                .selectable(false)
                                .truncate(),
                            );
                        });
                    },
                );
            });
    }

    fn status_bar(&self, ctx: &egui::Context) {
        let p = theme::palette(&ctx.style().visuals);
        egui::TopBottomPanel::bottom("status-bar")
            .exact_height(28.)
            .frame(
                egui::Frame::new()
                    .fill(p.bg)
                    .inner_margin(egui::Margin::symmetric(24, 0)),
            )
            .show(ctx, |ui| {
                let small = |text: String| RichText::new(text).size(11.).color(p.text2);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 14.;
                    ui.add(
                        egui::Label::new(small(format!(
                            "Recursos a cada {} s · API a cada {} s · v{}",
                            self.config.resource_seconds,
                            self.config.api_seconds,
                            env!("CARGO_PKG_VERSION")
                        )))
                        .extend(),
                    );
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(small(host_status(&self.resources, &self.api)))
                                .truncate(),
                        );
                    });
                });
            });
    }

    fn notices(&self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.visuals());
        if let Some(e) = &self.resources.docker_error {
            notice(ui, e, p.warn);
        }
        if let Some(e) = &self.api.error {
            notice(ui, e, p.warn);
        }
        if !self.api.errors.is_empty() {
            notice(
                ui,
                &format!(
                    "Dados parciais: {} aviso(s). Consulte Ajustes.",
                    self.api.errors.len()
                ),
                p.warn,
            );
        }
    }

    fn ai_cards(&self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.visuals());
        let usage = self.resources.ai_usage(&self.config.container);
        ui.columns(4, |cols| {
            metric(
                &mut cols[0],
                "CPU · AI-MEMORY",
                usage.as_ref().map(|u| format!("{}%", decimal(u.cpu, 1))),
                "100% = um núcleo",
                p.acc,
            );
            metric(
                &mut cols[1],
                "MEMÓRIA · AI-MEMORY",
                usage.as_ref().map(|u| bytes(u.memory)),
                "Uso atual do serviço",
                p.series2,
            );
            metric(
                &mut cols[2],
                "THREADS / TAREFAS",
                usage.as_ref().and_then(|u| u.tasks.map(number_u64)),
                "Processos + threads Linux",
                p.text,
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
                p.warn,
            );
        });
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.visuals());
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
                total.as_ref().map(|u| format!("{}%", decimal(u.cpu, 1))),
                "Soma dos contêineres ativos",
                p.series2,
            );
            metric(
                &mut cols[1],
                "MEMÓRIA · CONTÊINERES",
                total.as_ref().map(|u| bytes(u.memory)),
                "Inclui o ai-memory",
                p.series2,
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
                p.acc,
            );
            metric(
                &mut cols[3],
                "SESSÕES ABERTAS",
                self.api.open_session_links().map(number_u64),
                "Vínculos por projeto (API)",
                p.text,
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
        panel(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("ESTE MAC").small().color(p.text2));
                ui.label(format!(
                    "CPU {}%",
                    decimal(self.resources.host_cpu.into(), 1)
                ));
                ui.separator();
                ui.label(format!(
                    "Memória {} / {}",
                    bytes(self.resources.host_memory),
                    bytes(self.resources.host_total)
                ));
                ui.separator();
                ui.label(format!("{} projetos", number(self.api.projects.len())));
            });
        });
    }

    fn sessions(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui.visuals());
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
                .color(p.text2),
            );
            if !self.api.warnings.is_empty() {
                ui.label(
                    RichText::new(handoff_warning_message(
                        self.api.handoffs.len(),
                        self.api.handoff_total,
                    ))
                    .color(p.warn),
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
                        .color(p.text2),
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
                panel(ui).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        badge(ui, "ABERTO", p.warn);
                        ui.label(RichText::new(&h.scope).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(date(&h.at)).small().color(p.text2));
                        });
                    });
                    ui.label(
                        RichText::new(format!("{} · {}", h.agent, h.id))
                            .small()
                            .color(p.text2),
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
            ui.label(RichText::new("Últimas 100 sessões por projeto. Aberta = sem encerramento registrado; não comprova atividade atual. Vínculos entre projetos podem se repetir.").small().color(p.text2));
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
                    .color(p.text2),
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
                                if s.ended_at.is_none() { p.acc } else { p.text2 },
                            );
                        });
                        row.col(|ui| {
                            ui.vertical(|ui| {
                                ui.label(short_project(&s.scope)).on_hover_text(&s.scope);
                                ui.label(
                                    RichText::new(short_id(&s.session_id))
                                        .small()
                                        .color(p.text2),
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
            ui.label(RichText::new("Atividade = último evento ou atualização informado pela API. Contagens por projeto podem compartilhar sessões.").small().color(p.text2));
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
        let p = theme::palette(ui.visuals());
        let total = self.resources.docker_total();
        ui.columns(4, |cols| {
            metric(
                &mut cols[0],
                "CPU · CONTÊINERES",
                total.as_ref().map(|u| format!("{}%", decimal(u.cpu, 1))),
                "100% = um núcleo",
                p.series2,
            );
            metric(
                &mut cols[1],
                "MEMÓRIA · CONTÊINERES",
                total.as_ref().map(|u| bytes(u.memory)),
                "Cache inativo excluído pelo Docker",
                p.series2,
            );
            metric(
                &mut cols[2],
                "THREADS / TAREFAS",
                total.as_ref().and_then(|u| u.tasks.map(number_u64)),
                "Processos + threads Linux",
                p.text,
            );
            metric(
                &mut cols[3],
                "IMAGENS LOCAIS",
                self.resources
                    .images_at
                    .map(|_| number(self.resources.images.len())),
                "IDs únicos; tags agrupadas",
                p.acc,
            );
        });
        ui.add_space(16.);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.docker_tab, 0, "Contêineres");
            ui.selectable_value(&mut self.docker_tab, 1, "Imagens");
            ui.selectable_value(
                &mut self.docker_tab,
                2,
                format!("Processos no {}", crate::platform::HOST),
            );
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
                panel(ui).show(ui, |ui| {
                    section(ui, &c.name);
                    ui.label(RichText::new(&c.image).color(p.text2));
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
                            ui.label(format!(
                                "PID {pid}    {name}    {} threads",
                                number_u64(*threads)
                            ));
                        }
                    }
                    if ui.small_button("Copiar ID").clicked() {
                        ui.ctx().copy_text(c.id.clone());
                    }
                });
            }
        } else if self.docker_tab == 1 {
            if let Some(e) = &self.resources.inventory_error {
                notice(ui, e, p.warn);
            }
            ui.label(
                RichText::new(format!(
                    "Inventário {} · imagens associadas a contêineres parados continuam em uso.",
                    age(self.resources.images_at)
                ))
                .small()
                .color(p.text2),
            );
            ui.label(RichText::new("Tamanho lógico por imagem; camadas compartilhadas impedem somar estes valores como espaço real em disco.").small().color(p.text2));
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
                                ui.label(RichText::new(short_id(&i.id)).small().color(p.text2));
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
                                if i.running > 0 { p.acc } else { p.text2 },
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
            ui.label(
                RichText::new(crate::platform::PROCESSES_CAPTION)
                    .small()
                    .color(p.text2),
            );
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
                                ui.label(format!("{}%", decimal(p.usage.cpu, 1)));
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

    fn reconnect(&mut self, mut config: Config) {
        // O tema é trocado à parte (choose_theme); uma config montada antes da
        // troca, como a do login humano em andamento, não pode revertê-lo.
        config.theme = self.config.theme;
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
        let p = theme::palette(ui.visuals());
        panel(ui).show(ui, |ui| {
            section(ui, "Aparência");
            let mut choice = self.config.theme;
            ui.horizontal(|ui| {
                for (value, label) in [
                    (Appearance::System, "Sistema"),
                    (Appearance::Light, "Claro"),
                    (Appearance::Dark, "Escuro"),
                ] {
                    ui.selectable_value(&mut choice, value, label);
                }
            });
            if choice != self.config.theme {
                self.choose_theme(choice, save_theme);
            }
        });
        ui.add_space(12.);
        panel(ui).show(ui,|ui| {
            section(ui,"Fontes de dados");
            egui::Grid::new("config").num_columns(2).spacing([28.,16.]).show(ui,|ui| {
                ui.label("Servidor ai-memory");if ui.add(egui::TextEdit::singleline(&mut self.draft.endpoint).desired_width(400.)).changed(){self.draft.token.clear();self.draft.human=None;self.draft.proposal_watch=None;self.settings_message="Servidor alterado. Informe o token desse endereço ou use login humano após conectar.".into();}ui.end_row();
                ui.label("Token Bearer (opcional)");ui.add(egui::TextEdit::singleline(&mut self.draft.token).password(true).desired_width(400.));ui.end_row();
                ui.label("Contêiner ai-memory");ui.add(egui::TextEdit::singleline(&mut self.draft.container).desired_width(400.));ui.end_row();
                ui.label("Executável Docker");ui.add(egui::TextEdit::singleline(&mut self.draft.docker_binary).desired_width(400.));ui.end_row();
                ui.label("Atualizar recursos");ui.add(egui::DragValue::new(&mut self.draft.resource_seconds).range(1..=60).suffix(" s"));ui.end_row();
                ui.label("Atualizar sessões / handoffs");ui.add(egui::DragValue::new(&mut self.draft.api_seconds).range(5..=300).suffix(" s"));ui.end_row();
            });
            ui.label(crate::platform::PROPOSAL_WATCH_LABEL);
            egui::ComboBox::from_id_salt("watch-proposals").width(460.).selected_text(self.draft.proposal_watch.as_ref().map(crate::client::Scope::label).unwrap_or("Desativado".into())).show_ui(ui,|ui|{
                if ui.selectable_value(&mut self.draft.proposal_watch,None,"Desativado").clicked(){ui.close();}
                for p in &self.api.projects {let scope=crate::client::Scope{workspace:p.workspace_name.clone(),project:p.project_name.clone()};let label=scope.label();if ui.selectable_value(&mut self.draft.proposal_watch,Some(scope),label).clicked(){ui.close();}}
            });
            ui.add_space(12.);ui.checkbox(&mut self.draft.remember_token,format!("Guardar o Bearer no {} para este servidor (opcional)",crate::platform::SECRET_STORE));
            ui.label(RichText::new("Sem essa opção, o token fica somente em memória. Senhas e sessões humanas nunca são persistidas.").small().color(p.text2));
            if ui.add_enabled(!self.management.busy&&!self.authentication.busy&&!self.operations.busy,egui::Button::new(format!("Carregar Bearer do {} deste endereço",crate::platform::SECRET_STORE))).clicked(){
                self.draft.normalize();
                match crate::keychain::load(&self.draft.endpoint){
                    Ok(Some(token))=>{self.draft.token=token.trim().into();self.settings_message="Token carregado no campo. Salve para conectar.".into();},
                    Ok(None)=>self.settings_message="Nenhum token guardado para esse endereço.".into(),
                    Err(e)=>self.settings_message=e
                }
            }

            ui.horizontal(|ui| {
                if ui.add_enabled(!self.management.busy&&!self.authentication.busy&&!self.operations.busy, egui::Button::new(RichText::new("Salvar e conectar").color(p.acc))).clicked() {
                    self.draft.normalize();
                    match self.draft.validate().and_then(|_|{
                        match keychain_change(&self.config,&self.draft) {
                            KeychainChange::Save{endpoint,token}=>crate::keychain::save(endpoint,token)?,
                            KeychainChange::Forget(endpoint)=>crate::keychain::forget(endpoint)?,
                            KeychainChange::Keep=>{}
                        }
                        self.draft.save()
                    }) {
                        Ok(())=> {self.reconnect(self.draft.clone());self.authentication.clear();self.settings_message="Configuração salva. Reconectando…".into();},
                        Err(e)=>self.settings_message=e,
                    }
                }
                if ui.button("Restaurar campos padrão").clicked(){self.restore_defaults();}
            });
            if !self.settings_message.is_empty(){ui.label(&self.settings_message);}
        });
        ui.add_space(12.);
        panel(ui).show(ui, |ui| {
            self.authentication.show(
                ui,
                &self.shared,
                !self.management.busy && !self.operations.busy,
            )
        });
        ui.add_space(12.);
        panel(ui).show(ui,|ui| {
            section(ui,"Diagnóstico da conexão");ui.label(format!("ai-memory {}",{let v=server_version(&self.api,&self.health);if v.is_empty(){"—"}else{v}}));
            ui.label(format!("Recursos: {} · API: {}",age(self.resources.at),age(self.api.at)));
            ui.label("As consultas respeitam o usuário do token. Handoffs de outros usuários podem não estar visíveis.");
            if self.api.errors.is_empty()&&self.api.error.is_none(){ui.label(RichText::new(if self.api.at.is_some(){"Última coleta da API concluída sem erros."}else{"Conectando…"}).color(p.acc));}
            for e in &self.api.errors{ui.label(RichText::new(e).color(p.warn));}
            if let Some(e)=&self.resources.inventory_error{ui.label(RichText::new(e).color(p.warn));}
        });
    }
    /// Destino de `shared.navigation` (barra de menus e bandeja): 1 = Saúde,
    /// 2 = Revisão, 3 = Administração; o resto não navega.
    fn navigate(&mut self, code: u8) {
        self.page = match code {
            1 => Page::Health,
            2 => Page::Review,
            3 => Page::Admin,
            _ => return,
        };
    }
    /// Um frame da janela: lateral, barras e a página. Sem coleta nem
    /// navegação externa, para rodar sem janela nos testes.
    fn ui(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.page = Page::Settings;
        }
        self.sidebar(ctx, compact(ctx.content_rect().width()));
        self.top_bar(ctx);
        self.status_bar(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::palette(&ctx.style().visuals).bg)
                    .inner_margin(egui::Margin::symmetric(24, 20)),
            )
            .show(ctx, |ui| {
                self.notices(ui);
                // Na largura mínima, tabelas e linhas de Sessões e Docker
                // passam da área: rolam na horizontal em vez de cortar.
                egui::ScrollArea::both()
                    .id_salt("main-scroll")
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        match self.page {
                            Page::Overview => self.overview(ui),
                            Page::Review | Page::Maintenance => {
                                let section = if self.page == Page::Review {
                                    Section::Review
                                } else {
                                    Section::Maintenance
                                };
                                let go = ui
                                    .add_enabled_ui(
                                        !self.authentication.busy && !self.operations.busy,
                                        |ui| {
                                            self.management.show(
                                                ui,
                                                &self.shared,
                                                &self.api.projects,
                                                section,
                                            )
                                        },
                                    )
                                    .inner;
                                if let Some(section) = go {
                                    self.page = Page::of(section);
                                }
                            }
                            Page::Admin => {
                                ui.add_enabled_ui(
                                    !self.authentication.busy && !self.management.busy,
                                    |ui| {
                                        self.operations.show(ui, &self.shared, &self.api.projects);
                                    },
                                );
                            }
                            Page::Health => crate::health_ui::show(
                                ui,
                                &self.shared,
                                &self.health,
                                self.shared.health_asked.load(Ordering::Acquire)
                                    > self.health_served,
                                self.paused,
                                Utc::now(),
                            ),
                            Page::Explore => {
                                self.central.show(ui, &self.shared, &self.api.projects)
                            }
                            Page::Sessions => self.sessions(ui),
                            Page::Docker => self.docker(ui),
                            Page::Settings => self.settings(ui),
                        }
                    });
            });
    }
}

impl eframe::App for MonitorApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.apply_theme(ctx);
        if let Some(config) = self.authentication.poll(&self.shared) {
            self.reconnect(config);
        }
        self.central.poll(&self.shared);
        self.management.poll(&self.shared);
        self.operations.poll(&self.shared);
        ctx.request_repaint_after(Duration::from_secs(1));
        self.navigate(self.shared.navigation.swap(0, Ordering::AcqRel));
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
        self.ui(ctx);
    }
}
impl Drop for MonitorApp {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

fn panel(ui: &egui::Ui) -> egui::Frame {
    let p = theme::palette(ui.visuals());
    egui::Frame::new()
        .fill(p.surf)
        .stroke(Stroke::new(1.0_f32, p.line))
        .corner_radius(10.)
        .inner_margin(16)
}
fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(17.)
            .strong()
            .family(theme::semibold()),
    );
}
fn table_head(ui: &mut egui::Ui, text: &str) {
    let p = theme::palette(ui.visuals());
    ui.label(RichText::new(text).size(10.5).color(p.text2));
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
/// Texto em uma linha só, com reticências se passar de `width`.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(0.));
    ui.painter().layout_job(job)
}
/// O que fica à direita do rótulo de um item da lateral.
enum Aside {
    Count(String),
    /// Contador que pede atenção (warn).
    Warning(String),
    Shortcut(&'static str),
}
/// Item da lateral: ícone, rótulo numa linha só e, à direita, o contador ou o
/// atalho. No compacto, só o ícone; o rótulo vai ao tooltip no compacto e
/// sempre que for cortado.
fn nav_item(
    ui: &mut egui::Ui,
    page: Page,
    selected: bool,
    compact: bool,
    aside: Option<Aside>,
) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let label = page.label();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
    });
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 7., p.sel);
    } else if response.hovered() {
        painter.rect_filled(rect, 7., p.surf2.gamma_multiply(0.5));
    }
    let color = if selected { p.acc } else { p.text };
    let icon = if compact {
        egui::Rect::from_center_size(rect.center(), Vec2::splat(16.))
    } else {
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + 10., rect.center().y - 8.),
            Vec2::splat(16.),
        )
    };
    paint_icon(painter, icon, page, color);
    if compact {
        return response.on_hover_text(label);
    }
    let mut right = rect.right() - 10.;
    match aside {
        Some(Aside::Count(count)) => {
            right = count_pill(painter, right, rect, count, p.line, p.text2)
        }
        Some(Aside::Warning(count)) => {
            right = count_pill(
                painter,
                right,
                rect,
                count,
                crate::widgets::tone(p.warn, 0.16),
                p.warn,
            )
        }
        Some(Aside::Shortcut(keys)) => {
            let galley =
                painter.layout_no_wrap(keys.to_owned(), egui::FontId::proportional(11.), p.text2);
            let at = egui::pos2(
                right - galley.size().x,
                rect.center().y - galley.size().y / 2.,
            );
            right = at.x - 8.;
            painter.galley(at, galley, p.text2);
        }
        None => {}
    }
    let family = if selected {
        theme::semibold()
    } else {
        egui::FontFamily::Proportional
    };
    let left = icon.right() + 10.;
    let galley = one_line(
        ui,
        label,
        egui::FontId::new(13., family),
        color,
        right - left,
    );
    let elided = galley.elided;
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.),
        galley,
        color,
    );
    if elided {
        response.on_hover_text(label)
    } else {
        response
    }
}
/// Pílula de contador encostada em `right`; devolve onde o rótulo termina.
fn count_pill(
    painter: &egui::Painter,
    right: f32,
    row: egui::Rect,
    count: String,
    fill: Color32,
    color: Color32,
) -> f32 {
    let galley = painter.layout_no_wrap(count, egui::FontId::new(11., theme::semibold()), color);
    let size = Vec2::new((galley.size().x + 12.).max(18.), galley.size().y + 2.);
    let pill = egui::Rect::from_min_size(
        egui::pos2(right - size.x, row.center().y - size.y / 2.),
        size,
    );
    painter.rect_filled(pill, 9., fill);
    painter.galley(pill.center() - galley.size() / 2., galley, color);
    pill.left() - 8.
}
/// Ícones de 16 px da lateral, a partir dos paths de 24 unidades do artboard
/// (Sidebar.dc.html), em traço de 1,8 na mesma escala.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, page: Page, color: Color32) {
    let scale = rect.width() / 24.;
    let at = |(x, y): (f32, f32)| rect.min + Vec2::new(x, y) * scale;
    let stroke = Stroke::new(1.8 * scale, color);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().copied().map(at).collect(),
            stroke,
        ));
    };
    let closed = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::closed_line(
            points.iter().copied().map(at).collect(),
            stroke,
        ));
    };
    match page {
        Page::Overview => {
            for (x, y) in [(4., 4.), (13., 4.), (4., 13.), (13., 13.)] {
                closed(&[(x, y), (x + 7., y), (x + 7., y + 7.), (x, y + 7.)]);
            }
        }
        Page::Sessions => {
            painter.circle_stroke(at((12., 12.)), 9. * scale, stroke);
            line(&[(12., 7.), (12., 12.), (15., 14.)]);
        }
        Page::Docker => {
            closed(&[(3., 7.5), (12., 3.), (21., 7.5), (12., 12.)]);
            line(&[(3., 7.5), (3., 16.5), (12., 21.), (21., 16.5), (21., 7.5)]);
            line(&[(12., 12.), (12., 21.)]);
        }
        Page::Health => line(&[
            (2., 13.),
            (6., 13.),
            (8.5, 7.),
            (12.5, 18.),
            (15.5, 11.),
            (22., 11.),
        ]),
        Page::Explore => {
            painter.circle_stroke(at((11., 11.)), 7. * scale, stroke);
            line(&[(20., 20.), (16., 16.)]);
        }
        Page::Review => {
            for y in [6., 12., 18.] {
                line(&[(10., y), (20., y)]);
                line(&[(4., y), (5.2, y + 1.2), (7.5, y - 1.)]);
            }
        }
        Page::Maintenance => {
            closed(&[(4., 8.), (20., 8.), (20., 20.), (4., 20.)]);
            closed(&[(3., 4.), (21., 4.), (21., 8.), (3., 8.)]);
            line(&[(10., 12.), (14., 12.)]);
        }
        Page::Admin => {
            // Escudo: as duas curvas do path amostradas em quartos.
            closed(&[
                (12., 3.),
                (20., 6.),
                (20., 12.),
                (19.4, 15.2),
                (17.7, 17.85),
                (15.2, 19.8),
                (12., 21.),
                (8.8, 19.8),
                (6.3, 17.85),
                (4.6, 15.2),
                (4., 12.),
                (4., 6.),
            ]);
            line(&[(9.5, 12.), (11.5, 14.), (15., 10.5)]);
        }
        Page::Settings => {
            for segment in [
                [(4., 6.), (13., 6.)],
                [(17., 6.), (20., 6.)],
                [(15., 4.), (15., 8.)],
                [(4., 12.), (7., 12.)],
                [(11., 12.), (20., 12.)],
                [(9., 10.), (9., 14.)],
                [(4., 18.), (15., 18.)],
                [(19., 18.), (20., 18.)],
                [(17., 16.), (17., 20.)],
            ] {
                line(&segment);
            }
        }
    }
}
/// Uma fonte de dados no rodapé da lateral: ponto de estado e `label`. O
/// tooltip e o AccessKit dizem "`name`: estado" (conectado, o erro ou a
/// espera). No compacto, só o ponto.
fn source(
    ui: &mut egui::Ui,
    name: &str,
    label: &str,
    collected: bool,
    error: Option<&str>,
    compact: bool,
) {
    let p = theme::palette(ui.visuals());
    let (color, state) = match (error, collected) {
        (Some(e), _) => (p.bad, e),
        (None, true) => (p.acc, "conectado"),
        (None, false) => (p.text2, "aguardando primeira coleta"),
    };
    let state = format!("{name}: {state}");
    let font = egui::FontId::proportional(12.);
    let height = ui.fonts_mut(|f| f.row_height(&font));
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &state));
    let dot = if compact {
        rect.center()
    } else {
        egui::pos2(rect.left() + 3.5, rect.center().y)
    };
    ui.painter().circle_filled(dot, 3.5, color);
    if !compact {
        let left = dot.x + 3.5 + 8.;
        let galley = one_line(ui, label, font, p.text2, rect.right() - left);
        ui.painter().galley(
            egui::pos2(left, rect.center().y - galley.size().y / 2.),
            galley,
            p.text2,
        );
    }
    response.on_hover_text(state);
}
/// Chip de atualização da barra superior.
fn freshness_chip(ui: &mut egui::Ui, state: Freshness) {
    let p = theme::palette(ui.visuals());
    let (fill, color) = match state {
        Freshness::Paused => (p.surf2, p.text),
        Freshness::Connecting => (p.surf2, p.text2),
        Freshness::Live(_) => (p.sel, p.acc),
        Freshness::ApiLate(_) | Freshness::ResourcesLate(_) | Freshness::BothLate(_) => {
            (p.warn.gamma_multiply(0.14), p.warn)
        }
    };
    let font = egui::FontId::proportional(12.);
    let mut job = egui::text::LayoutJob::default();
    if let Freshness::Live(age) = state {
        job.append(
            "Ao vivo",
            0.,
            egui::TextFormat::simple(egui::FontId::new(12., theme::semibold()), p.acc),
        );
        job.append(
            &format!(" · atualizado {}", ago(age)),
            0.,
            egui::TextFormat::simple(font, p.text2),
        );
    } else {
        job.append(&state.text(), 0., egui::TextFormat::simple(font, color));
    }
    let galley = ui.painter().layout_job(job);
    let mark = if state == Freshness::Paused { 10. } else { 7. };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(10. + mark + 8. + galley.size().x + 10., 28.),
        egui::Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, state.text()));
    let painter = ui.painter();
    painter.rect_filled(rect, 14., fill);
    let mark = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 10., rect.center().y - mark / 2.),
        Vec2::splat(mark),
    );
    if state == Freshness::Paused {
        pause_icon(painter, mark, color, 3.);
    } else {
        painter.circle_filled(mark.center(), 3.5, color);
    }
    painter.galley(
        egui::pos2(mark.right() + 8., rect.center().y - galley.size().y / 2.),
        galley,
        color,
    );
}
/// Duas barras verticais (viewBox 24, `M9 5v14M15 5v14`).
fn pause_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32, width: f32) {
    let scale = rect.width() / 24.;
    for x in [9., 15.] {
        painter.line_segment(
            [
                rect.min + Vec2::new(x, 5.) * scale,
                rect.min + Vec2::new(x, 19.) * scale,
            ],
            Stroke::new(width * scale, color),
        );
    }
}
/// Botão de pausa da barra superior. Também sente arrasto: senão o arrasto
/// do fundo da barra (macOS) começaria por baixo dele.
fn pause_button(ui: &mut egui::Ui, paused: bool) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let label = if paused {
        "Retomar atualização da tela"
    } else {
        "Pausar atualização da tela"
    };
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(30., 28.), egui::Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let painter = ui.painter();
    painter.rect(
        rect,
        6.,
        if response.hovered() { p.surf2 } else { p.surf },
        Stroke::new(1.0_f32, p.line),
        egui::StrokeKind::Inside,
    );
    let icon = egui::Rect::from_center_size(rect.center(), Vec2::splat(14.));
    if paused {
        let scale = icon.width() / 24.;
        painter.add(egui::Shape::closed_line(
            [(7., 5.), (18., 12.), (7., 19.)]
                .map(|(x, y)| icon.min + Vec2::new(x, y) * scale)
                .to_vec(),
            Stroke::new(2. * scale, p.text2),
        ));
    } else {
        pause_icon(painter, icon, p.text2, 2.);
    }
    response.on_hover_text(label)
}
/// macOS, sem barra de título: arrastar com o botão principal move a janela
/// e o duplo clique maximiza ou restaura.
#[cfg(target_os = "macos")]
fn drag_window(response: &egui::Response) {
    let ctx = &response.ctx;
    if response.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if response.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
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
    let p = theme::palette(ui.visuals());
    panel(ui).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.add_space(30.);
        ui.vertical_centered(|ui| {
            ui.heading(title);
            ui.label(RichText::new(subtitle).color(p.text2));
        });
        ui.add_space(30.);
    });
}
fn metric(ui: &mut egui::Ui, title: &str, value: Option<String>, hint: &str, color: Color32) {
    let p = theme::palette(ui.visuals());
    panel(ui).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(title).size(10.5).color(p.text2));
        ui.add_space(5.);
        ui.label(
            RichText::new(value.unwrap_or("—".into()))
                .size(29.)
                .color(color),
        );
        ui.label(RichText::new(hint).size(11.).color(p.text2));
    });
}
fn number_u64(n: u64) -> String {
    number(n as usize)
}
fn age(at: Option<DateTime<Utc>>) -> String {
    at.map(|a| ago((Utc::now() - a).num_seconds()))
        .unwrap_or("aguardando".into())
}
/// Idade de uma coleta em texto; negativa (relógio ajustado) conta como 0.
pub(crate) fn ago(secs: i64) -> String {
    let secs = secs.max(0) as usize;
    if secs < 60 {
        format!("há {} s", number(secs))
    } else if secs < 3600 {
        format!("há {} min", number(secs / 60))
    } else {
        format!("há {} h", number(secs / 3600))
    }
}

/// Estado do chip de atualização da barra superior. A idade é a da fonte mais
/// nova (ao vivo) ou da mais velha entre as atrasadas, em segundos.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Freshness {
    Paused,
    Connecting,
    Live(i64),
    ApiLate(i64),
    ResourcesLate(i64),
    BothLate(i64),
}
impl Freshness {
    fn text(self) -> String {
        match self {
            Freshness::Paused => "Pausado".into(),
            Freshness::Connecting => "Conectando…".into(),
            Freshness::Live(age) => format!("Ao vivo · atualizado {}", ago(age)),
            Freshness::ApiLate(age) => format!("API atrasada · {}", ago(age)),
            Freshness::ResourcesLate(age) => format!("Recursos atrasados · {}", ago(age)),
            Freshness::BothLate(age) => format!("Atrasado · {}", ago(age)),
        }
    }
}
/// Uma fonte atrasa quando a última coleta tem mais de três intervalos; só
/// conta a fonte que já coletou. Pausado vence qualquer outro estado.
fn freshness(
    now: DateTime<Utc>,
    resources_at: Option<DateTime<Utc>>,
    api_at: Option<DateTime<Utc>>,
    resource_seconds: u64,
    api_seconds: u64,
    paused: bool,
) -> Freshness {
    if paused {
        return Freshness::Paused;
    }
    let age = |at: Option<DateTime<Utc>>| at.map(|at| (now - at).num_seconds().max(0));
    let late = |age: Option<i64>, seconds: u64| age.filter(|&age| age > seconds as i64 * 3);
    let (resources, api) = (age(resources_at), age(api_at));
    match (
        late(resources, resource_seconds),
        late(api, api_seconds),
        resources.into_iter().chain(api).min(),
    ) {
        (Some(r), Some(a), _) => Freshness::BothLate(r.max(a)),
        (Some(r), None, _) => Freshness::ResourcesLate(r),
        (None, Some(a), _) => Freshness::ApiLate(a),
        (None, None, Some(youngest)) => Freshness::Live(youngest),
        (None, None, None) => Freshness::Connecting,
    }
}

/// Contador do item Sessões e handoffs: só com a API coletada e handoffs
/// abertos; sem o total do servidor, a lista pode estar cortada ("+").
fn handoff_badge(api: &ApiSnapshot) -> Option<String> {
    api.at?;
    let open = api.handoff_total.unwrap_or(api.handoffs.len());
    (open > 0).then(|| {
        format!(
            "{}{}",
            number(open),
            if api.handoff_total.is_some() { "" } else { "+" }
        )
    })
}

/// Contador do item Revisão: propostas pendentes do projeto vigiado, com a
/// mesma leitura e o mesmo texto da barra de menus e da bandeja; só quando há
/// alguma.
fn review_badge(
    health: &crate::health::Snapshot,
    config: &Config,
    now: DateTime<Utc>,
) -> Option<String> {
    let watch = config.proposal_watch.as_ref()?;
    health
        .watched_pending(watch, now)
        .filter(|&n| n > 0)
        .map(crate::health::pending_text)
}

/// Texto à esquerda da barra de status.
fn host_status(resources: &ResourceSnapshot, api: &ApiSnapshot) -> String {
    let host = format!("Este {}", crate::platform::HOST);
    if resources.at.is_none() {
        return host;
    }
    let mut text = format!(
        "{host} · CPU {}% · Memória {} de {}",
        decimal(resources.host_cpu.into(), 0),
        bytes(resources.host_memory),
        bytes(resources.host_total)
    );
    if api.at.is_some() {
        let projects = api.projects.len();
        text.push_str(&format!(
            " · {} {}",
            number(projects),
            if projects == 1 { "projeto" } else { "projetos" }
        ));
    }
    text
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
    let p = theme::palette(ui.visuals());
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
                        ui.label(RichText::new(&c.image).size(11.).color(p.text2))
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
                        if c.state == "running" { p.acc } else { p.text2 },
                    );
                });
                row.col(|ui| {
                    ui.label(
                        c.usage
                            .as_ref()
                            .map(|u| format!("{}%", decimal(u.cpu, 1)))
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
    let p = theme::palette(ui.visuals());
    panel(ui).show(ui, |ui| {
        ui.horizontal(|ui| {
            section(ui, title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("Docker").small().color(p.series2));
                ui.label(RichText::new("ai-memory").small().color(p.acc));
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
                Stroke::new(1.0_f32, p.line),
            );
            painter.text(
                egui::pos2(plot.left() - 8., y),
                egui::Align2::RIGHT_CENTER,
                format!(
                    "{}{}",
                    decimal(max * frac as f64, 0),
                    if memory { "M" } else { "%" }
                ),
                egui::FontId::proportional(10.),
                p.text2,
            );
        }
        let plot_painter = painter.with_clip_rect(plot.expand(4.));
        for (ai, color) in [(false, p.series2), (true, p.acc)] {
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
                p.text2,
            );
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("−{minutes} min"))
                    .small()
                    .color(p.text2),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new("agora").small().color(p.text2));
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

    #[test]
    fn keychain_change_decides_save_forget_keep() {
        let current = Config {
            endpoint: "https://old.example".into(),
            token: "old-token".into(),
            remember_token: true,
            ..Default::default()
        };
        let draft = Config {
            endpoint: "https://new.example".into(),
            token: "draft-token".into(),
            remember_token: true,
            ..Default::default()
        };
        match keychain_change(&current, &draft) {
            KeychainChange::Save { endpoint, token } => {
                assert_eq!(endpoint, "https://new.example");
                assert_eq!(token, "draft-token");
            }
            _ => panic!("remembered draft must save to the draft endpoint"),
        }

        let mut draft = current.clone();
        draft.remember_token = false;
        match keychain_change(&current, &draft) {
            KeychainChange::Forget(endpoint) => assert_eq!(endpoint, "https://old.example"),
            _ => panic!("unchecked remember must forget the connected endpoint"),
        }
        draft.endpoint = "https://new.example".into();
        match keychain_change(&current, &draft) {
            KeychainChange::Forget(endpoint) => assert_eq!(
                endpoint, "https://old.example",
                "forget must target the endpoint that was connected, not the draft"
            ),
            _ => panic!("endpoint change with unchecked remember must still forget"),
        }
        match keychain_change(&current, &Config::default()) {
            KeychainChange::Forget(endpoint) => assert_eq!(endpoint, "https://old.example"),
            _ => panic!("restoring defaults must forget the connected endpoint"),
        }

        let plain = Config::default();
        assert!(matches!(
            keychain_change(&plain, &Config::default()),
            KeychainChange::Keep
        ));
    }

    #[test]
    fn server_version_prefers_api_then_health_then_empty() {
        let api = crate::model::ApiSnapshot {
            version: "9.9.9".into(),
            ..Default::default()
        };
        let health = crate::health::Snapshot {
            status: Some(serde_json::json!({"version":"1.2.3"})),
            ..Default::default()
        };
        assert_eq!(server_version(&api, &health), "9.9.9");
        let api = crate::model::ApiSnapshot::default();
        assert_eq!(server_version(&api, &health), "1.2.3");
        let health = crate::health::Snapshot {
            status: Some(serde_json::json!({"counts":{}})),
            ..Default::default()
        };
        assert_eq!(server_version(&api, &health), "");
        let health = crate::health::Snapshot::default();
        assert_eq!(server_version(&api, &health), "");
    }

    fn app_with_unsaved_draft() -> MonitorApp {
        let config = Config {
            endpoint: "https://connected.example".into(),
            ..Default::default()
        };
        let mut app = MonitorApp::build(config.clone(), Shared::unstarted(config));
        app.draft.endpoint = "https://draft.example".into();
        app.draft.container = "draft-ai".into();
        app
    }

    #[test]
    fn theme_change_applies_and_saves_only_the_theme_without_reconnecting() {
        let mut app = app_with_unsaved_draft();
        let mut saved = None;
        app.choose_theme(Appearance::Light, |theme| {
            saved = Some(theme);
            Ok(())
        });
        assert_eq!(app.config.theme, Appearance::Light);
        assert_eq!(app.draft.theme, Appearance::Light);
        assert_eq!(app.shared.config.read().unwrap().theme, Appearance::Light);
        assert_eq!(
            saved,
            Some(Appearance::Light),
            "theme change must be saved right away"
        );
        assert_eq!(app.config.endpoint, "https://connected.example");
        assert_eq!(app.draft.endpoint, "https://draft.example");
        assert_eq!(app.draft.container, "draft-ai");
        assert_eq!(
            app.shared.generation.load(Ordering::Acquire),
            0,
            "a theme change must not reconnect"
        );
        assert!(app.settings_message.is_empty());

        app.choose_theme(Appearance::Dark, |_| Err("disco cheio".into()));
        assert_eq!(app.settings_message, "disco cheio");
        assert_eq!(
            app.config.theme,
            Appearance::Dark,
            "a failed save keeps the theme for the session"
        );
        assert_eq!(app.shared.config.read().unwrap().theme, Appearance::Dark);
    }

    #[test]
    fn theme_change_never_writes_the_in_memory_endpoint_to_disk() {
        // Endpoint em memória vindo de AI_MEMORY_SERVER_URL, diferente do disco.
        let dir = std::env::temp_dir().join(format!("ai-monitor-app-theme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let disk =
            "{\n  \"endpoint\": \"http://192.168.0.10:49374\",\n  \"container\": \"custom-ai\"\n}";
        std::fs::write(&path, disk).unwrap();
        let mut app = app_with_unsaved_draft();
        app.choose_theme(Appearance::Dark, |theme| save_theme_to(&path, theme));
        let written = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(app.settings_message.is_empty(), "{}", app.settings_message);
        assert_eq!(
            written,
            disk.replace(
                "\"custom-ai\"\n}",
                "\"custom-ai\",\n  \"theme\": \"dark\"\n}"
            )
        );
        assert_eq!(app.config.endpoint, "https://connected.example");
    }

    #[test]
    fn native_window_theme_is_sent_once_per_change() {
        fn set_theme(out: &egui::FullOutput) -> Vec<egui::SystemTheme> {
            out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::ViewportCommand::SetTheme(t) => Some(*t),
                    _ => None,
                })
                .collect()
        }
        let mut app = app_with_unsaved_draft();
        let ctx = egui::Context::default();
        // Enviado antes do primeiro frame (como no CreationContext), chega a ele.
        app.apply_theme(&ctx);
        app.apply_theme(&ctx);
        let out = ctx.run(egui::RawInput::default(), |_| {});
        assert_eq!(
            set_theme(&out),
            vec![theme::window_theme(
                Appearance::System,
                theme::platform_theme()
            )]
        );
        app.choose_theme(Appearance::Light, |_| Ok(()));
        app.apply_theme(&ctx);
        let out = ctx.run(egui::RawInput::default(), |_| {});
        assert_eq!(set_theme(&out), vec![egui::SystemTheme::Light]);
        app.apply_theme(&ctx);
        let out = ctx.run(egui::RawInput::default(), |_| {});
        assert_eq!(set_theme(&out), vec![], "unchanged theme must not resend");
        assert_eq!(
            ctx.options(|o| o.theme_preference),
            egui::ThemePreference::Light
        );
    }

    #[test]
    fn restore_defaults_and_reconnect_keep_the_theme() {
        let mut app = app_with_unsaved_draft();
        app.choose_theme(Appearance::Light, |_| Ok(()));
        app.restore_defaults();
        assert_eq!(app.draft.container, "ai-memory", "the draft was restored");
        assert_eq!(app.draft.endpoint, Config::default().endpoint);
        assert_eq!(
            app.draft.theme,
            Appearance::Light,
            "restore keeps the theme"
        );

        // Config montada antes da troca (ex.: login humano em voo).
        app.reconnect(Config::default());
        assert_eq!(app.config.theme, Appearance::Light);
        assert_eq!(app.draft.theme, Appearance::Light);
        assert_eq!(app.shared.config.read().unwrap().theme, Appearance::Light);
        assert_eq!(app.shared.generation.load(Ordering::Acquire), 1);
    }

    /// Contexto com as fontes e os estilos do app, como numa janela nova.
    fn styled() -> egui::Context {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx
    }
    /// Um frame de `ui` numa janela de `size` pontos.
    fn frame(
        app: &mut MonitorApp,
        ctx: &egui::Context,
        size: [f32; 2],
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                events,
                ..Default::default()
            },
            |ctx| app.ui(ctx),
        )
    }
    fn sidebar_width(ctx: &egui::Context) -> f32 {
        egui::containers::panel::PanelState::load(ctx, egui::Id::new("sidebar"))
            .expect("the sidebar must have been shown")
            .rect
            .width()
    }

    #[test]
    fn compact_below_1000_points_of_window_width() {
        for width in [0., 720., 999.9] {
            assert!(compact(width), "{width} must be compact");
        }
        for width in [1000., 1000.1, 1320.] {
            assert!(!compact(width), "{width} must show the full sidebar");
        }
    }

    #[test]
    fn minimum_window_reaches_the_compact_sidebar_and_the_default_does_not() {
        assert!(
            compact(MIN_WINDOW_SIZE[0]),
            "the minimum window width {} must be below the cut, or compact is unreachable",
            MIN_WINDOW_SIZE[0]
        );
        assert!(
            !compact(WINDOW_SIZE[0]),
            "the default window must open with the full sidebar"
        );
    }

    #[test]
    fn sidebar_is_compact_at_720_and_232_wide_at_1320() {
        // macOS: 78 px, para os botões da janela (até x = 70); Linux: 64 px.
        let compact = if cfg!(target_os = "macos") { 78. } else { 64. };
        let mut app = app_with_unsaved_draft();
        let ctx = styled();
        frame(&mut app, &ctx, [720., 560.], vec![]);
        assert_eq!(
            sidebar_width(&ctx),
            compact,
            "720 × 560 must use this platform's compact sidebar"
        );
        frame(&mut app, &ctx, [1320., 860.], vec![]);
        assert_eq!(
            sidebar_width(&ctx),
            232.,
            "1320 × 860 must use the full sidebar"
        );
        frame(&mut app, &ctx, [720., 560.], vec![]);
        assert_eq!(
            sidebar_width(&ctx),
            compact,
            "shrinking the window compacts it again"
        );
    }

    /// As 9 páginas, na ordem da lateral (Ajustes no rodapé).
    const PAGES: [Page; 9] = [
        Page::Overview,
        Page::Sessions,
        Page::Docker,
        Page::Health,
        Page::Explore,
        Page::Review,
        Page::Maintenance,
        Page::Admin,
        Page::Settings,
    ];
    /// Toda página tem de estar em PAGES: um membro novo quebra este match.
    #[test]
    fn pages_lists_every_page_once() {
        for page in PAGES {
            match page {
                Page::Overview
                | Page::Sessions
                | Page::Docker
                | Page::Health
                | Page::Explore
                | Page::Review
                | Page::Maintenance
                | Page::Admin
                | Page::Settings => {}
            }
            assert_eq!(PAGES.iter().filter(|&&p| p == page).count(), 1, "{page:?}");
        }
        let labels: Vec<_> = PAGES.iter().map(|p| p.label()).collect();
        assert_eq!(
            labels,
            [
                "Visão geral",
                "Sessões e handoffs",
                "Docker",
                "Saúde",
                "Explorar",
                "Revisão",
                "Manutenção e backup",
                "Administração",
                "Ajustes",
            ]
        );
        for page in PAGES {
            let want = if page == Page::Health {
                "Saúde do ai-memory"
            } else {
                page.label()
            };
            assert_eq!(page.title(), want, "{page:?}");
        }
        assert_eq!(Page::of(Section::Review), Page::Review);
        assert_eq!(Page::of(Section::Maintenance), Page::Maintenance);
    }
    /// Propostas pendentes de "ws / proj", vigiado e lido agora.
    fn watching(app: &mut MonitorApp, pending: Option<usize>) {
        let scope = crate::client::Scope {
            workspace: "ws".into(),
            project: "proj".into(),
        };
        app.config.proposal_watch = Some(scope.clone());
        app.health = crate::health::Snapshot {
            at: Some(Utc::now()),
            pending,
            pending_scope: Some(scope),
            ..Default::default()
        };
    }

    /// Textos pintados na lateral, com a marca de corte.
    fn sidebar_texts(ctx: &egui::Context, out: &egui::FullOutput) -> Vec<(String, bool)> {
        let width = sidebar_width(ctx);
        out.shapes
            .iter()
            .filter(|clipped| clipped.clip_rect.max.x <= width + 0.5)
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_owned(), t.galley.elided)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_sidebar_label_fits_at_232_selected_or_not() {
        let at = Some(Utc::now());
        let open = |total, listed| ApiSnapshot {
            at,
            handoff_total: total,
            handoffs: vec![Handoff::default(); listed],
            ..Default::default()
        };
        // "999+" (sem total do servidor) deixa 117,6 px ao rótulo, menos que os
        // 120,9 px (regular) e 124,2 px (semibold) de "Sessões e handoffs":
        // corta, e o nome vai ao tooltip.
        // Revisão leva o contador de propostas: até "200+" cabe com folga.
        for (counter, api, sessions_fit, review) in [
            (None, ApiSnapshot::default(), true, None),
            (Some("999"), open(Some(999), 0), true, Some(3)),
            (Some("999+"), open(None, 999), false, Some(200)),
        ] {
            for selected in PAGES {
                let mut app = app_with_unsaved_draft();
                app.api = api.clone();
                if review.is_some() {
                    watching(&mut app, review);
                }
                app.page = selected;
                let ctx = styled();
                let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
                let texts = sidebar_texts(&ctx, &out);
                for counter in counter
                    .into_iter()
                    .chain(review.map(|n| if n >= 200 { "200+" } else { "3" }))
                {
                    assert!(
                        texts.contains(&(counter.to_owned(), false)),
                        "the {counter} counter must be painted whole"
                    );
                }
                for page in PAGES {
                    let painted: Vec<bool> = texts
                        .iter()
                        .filter(|(text, _)| text == page.label())
                        .map(|&(_, elided)| elided)
                        .collect();
                    assert!(!painted.is_empty(), "{page:?} must be painted");
                    let fits = page != Page::Sessions || sessions_fit;
                    assert_eq!(
                        painted.iter().any(|&elided| elided),
                        !fits,
                        "{page:?} cut = {} at {} px, {selected:?} selected, counter {counter:?}",
                        !fits,
                        sidebar_width(&ctx)
                    );
                }
            }
        }
    }

    #[test]
    fn menu_navigation_opens_health_review_and_admin_and_ignores_the_rest() {
        for code in 0..=u8::MAX {
            let mut app = app_with_unsaved_draft();
            app.page = Page::Docker;
            app.navigate(code);
            let want = match code {
                1 => Page::Health,
                2 => Page::Review,
                3 => Page::Admin,
                _ => Page::Docker,
            };
            assert_eq!(app.page, want, "navigation {code}");
        }
    }

    #[test]
    fn every_page_shows_its_title_and_marks_only_its_item_in_both_themes() {
        use egui::accesskit::{Role, Toggled};
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            for page in PAGES {
                let mut app = app_with_unsaved_draft();
                app.page = page;
                let ctx = styled();
                ctx.set_theme(theme);
                ctx.enable_accesskit();
                let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
                let update = out.platform_output.accesskit_update.as_ref().unwrap();
                // O título da barra superior é o único Label com esse texto
                // fora da lateral (232 px).
                let titles = update
                    .nodes
                    .iter()
                    .filter(|(_, n)| n.role() == Role::Label && n.value() == Some(page.title()))
                    .filter_map(|(_, n)| n.bounds())
                    .filter(|b| b.x0 > 232. && b.y1 <= 52.)
                    .count();
                assert_eq!(titles, 1, "{theme:?} {page:?}: top bar title");
                for other in PAGES {
                    let item = widgets(&out, Role::Button, other.label());
                    assert_eq!(
                        item.len(),
                        1,
                        "{theme:?}: one {other:?} item while on {page:?}"
                    );
                    let toggled = update
                        .nodes
                        .iter()
                        .find(|(_, n)| {
                            n.role() == Role::Button
                                && n.label() == Some(other.label())
                                && n.bounds().is_some_and(|b| b.x1 <= 232.)
                        })
                        .map(|(_, n)| n.toggled());
                    let want = if other == page {
                        Toggled::True
                    } else {
                        Toggled::False
                    };
                    assert_eq!(toggled, Some(Some(want)), "{theme:?} {other:?} on {page:?}");
                }
            }
        }
    }

    /// Lateral na janela mínima: os 8 itens, Ajustes e as duas fontes, todos
    /// dentro da lateral e da janela, na ordem, sem um sobrepor o outro.
    #[test]
    fn sidebar_fits_without_overlap_at_the_minimum_height() {
        use egui::accesskit::Role;
        // Compacta (720) e inteira (1000) na altura mínima de 560.
        for size in [[720., 560.], [1000., 560.]] {
            let mut app = app_with_unsaved_draft();
            let ctx = styled();
            ctx.enable_accesskit();
            let out = frame(&mut app, &ctx, size, vec![]);
            let width = sidebar_width(&ctx);
            let mut rows: Vec<(String, egui::Rect)> = PAGES
                .iter()
                .map(|p| {
                    (
                        p.label().to_owned(),
                        widget_rect(&out, Role::Button, p.label()),
                    )
                })
                .collect();
            for source in [
                "ai-memory: aguardando primeira coleta",
                "Docker: aguardando primeira coleta",
            ] {
                rows.push((source.to_owned(), widget_rect(&out, Role::Label, source)));
            }
            let order = [
                "Visão geral",
                "Sessões e handoffs",
                "Docker",
                "Saúde",
                "Explorar",
                "Revisão",
                "Manutenção e backup",
                "Administração",
                "ai-memory: aguardando primeira coleta",
                "Docker: aguardando primeira coleta",
                "Ajustes",
            ];
            rows.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
            let sorted: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
            assert_eq!(sorted, order, "{size:?}: top-to-bottom order");
            for (name, rect) in &rows {
                assert!(
                    rect.left() >= 0. && rect.right() <= width && rect.top() >= 52.,
                    "{size:?}: {name} {rect:?} leaves the sidebar ({width} px)"
                );
                assert!(
                    rect.bottom() <= size[1] - 12.,
                    "{size:?}: {name} {rect:?} is cut"
                );
            }
            for pair in rows.windows(2) {
                assert!(
                    pair[0].1.bottom() <= pair[1].1.top(),
                    "{size:?}: {} {:?} overlaps {} {:?}",
                    pair[0].0,
                    pair[0].1,
                    pair[1].0,
                    pair[1].1
                );
            }
        }
    }

    #[test]
    fn review_counter_matches_the_menu_bar_and_only_shows_pending_proposals() {
        use crate::menu_bar::MenuReading;
        let now = Utc::now();
        let scope = crate::client::Scope {
            workspace: "ws".into(),
            project: "proj".into(),
        };
        let other = crate::client::Scope {
            workspace: "ws".into(),
            project: "outro".into(),
        };
        let reading = |age: i64, of: &crate::client::Scope, pending| crate::health::Snapshot {
            at: Some(now - chrono::Duration::seconds(age)),
            pending,
            pending_scope: Some(of.clone()),
            ..Default::default()
        };
        // (caso, vigiado?, leitura, contador na lateral, número no menu)
        for (case, watch, health, badge, menu) in [
            (
                "nothing watched",
                false,
                reading(0, &scope, Some(3)),
                None,
                None,
            ),
            (
                "three pending",
                true,
                reading(0, &scope, Some(3)),
                Some("3"),
                Some("3"),
            ),
            (
                "one pending",
                true,
                reading(0, &scope, Some(1)),
                Some("1"),
                Some("1"),
            ),
            (
                "199 pending",
                true,
                reading(0, &scope, Some(199)),
                Some("199"),
                Some("199"),
            ),
            (
                "list limit",
                true,
                reading(0, &scope, Some(200)),
                Some("200+"),
                Some("200+"),
            ),
            (
                "zero is no counter",
                true,
                reading(0, &scope, Some(0)),
                None,
                Some("0"),
            ),
            (
                "denied is unknown",
                true,
                reading(0, &scope, None),
                None,
                Some("—"),
            ),
            (
                "stale is unknown",
                true,
                reading(91, &scope, Some(3)),
                None,
                Some("—"),
            ),
            (
                "another project",
                true,
                reading(0, &other, Some(3)),
                None,
                Some("—"),
            ),
        ] {
            let config = Config {
                proposal_watch: watch.then(|| scope.clone()),
                ..Default::default()
            };
            assert_eq!(
                review_badge(&health, &config, now).as_deref(),
                badge,
                "{case}"
            );
            let title = MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, now)
                .with_context(None, Some(&health), &config, now)
                .title;
            match menu {
                Some(n) => assert!(title.ends_with(&format!(" P{n}")), "{case}: {title}"),
                None => assert!(!title.contains(" P"), "{case}: {title}"),
            }
        }
    }

    #[test]
    fn review_counter_is_painted_in_warn_next_to_its_item() {
        for (theme, p) in [
            (egui::Theme::Dark, &theme::DARK),
            (egui::Theme::Light, &theme::LIGHT),
        ] {
            let mut app = app_with_unsaved_draft();
            watching(&mut app, Some(3));
            let ctx = styled();
            ctx.set_theme(theme);
            let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
            let three: Vec<_> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) if t.galley.text() == "3" => Some(t.fallback_color),
                    _ => None,
                })
                .collect();
            assert_eq!(three, vec![p.warn], "{theme:?}");
            let pill = crate::widgets::tone(p.warn, 0.16);
            assert!(
                out.shapes
                    .iter()
                    .any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == pill)),
                "{theme:?}: warn pill"
            );
        }
    }

    #[test]
    fn opening_review_from_the_menu_picks_proposals_unless_the_tab_is_already_there() {
        // (aba atual, ocupado?, aba depois de abrir Revisão pela navegação 2)
        for (tab, busy, want) in [
            (0, false, 0),
            (1, false, 1),
            (2, false, 0),
            (3, false, 0),
            // Ocupado em Manutenção e backup: a aba fica até terminar.
            (2, true, 2),
            (3, true, 3),
            (1, true, 1),
        ] {
            let mut app = app_with_unsaved_draft();
            app.page = Page::Overview;
            app.management.set_tab(tab);
            app.management.busy = busy;
            app.navigate(2);
            let ctx = styled();
            frame(&mut app, &ctx, [1320., 860.], vec![]);
            assert_eq!(app.page, Page::Review, "tab {tab} busy {busy}");
            assert_eq!(app.management.tab(), want, "tab {tab} busy {busy}");
        }
    }

    /// Clique completo em `pos` (pressiona e solta em dois frames).
    fn click_at(app: &mut MonitorApp, ctx: &egui::Context, size: [f32; 2], pos: egui::Pos2) {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(
            app,
            ctx,
            size,
            vec![egui::Event::PointerMoved(pos), press(true)],
        );
        frame(app, ctx, size, vec![press(false)]);
    }

    #[test]
    fn leaving_review_mid_operation_keeps_its_tab_and_warns_on_the_other_page() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.page = Page::Review;
        app.management.set_tab(1);
        app.management.busy = true;
        let ctx = styled();
        ctx.enable_accesskit();
        let out = frame(&mut app, &ctx, size, vec![]);
        let maintenance = widget_rect(&out, Role::Button, "Manutenção e backup");
        click_at(&mut app, &ctx, size, maintenance.center());
        assert_eq!(app.page, Page::Maintenance, "the click still navigates");
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(app.management.tab(), 1, "the tab of the operation is kept");
        assert!(app.management.busy);
        let warning = "Operação em andamento em Revisão · Handoffs. Aguarde a resposta lá; esta página usa o mesmo painel.";
        assert_eq!(widgets(&out, Role::Label, warning).len(), 1);
        assert!(
            widgets(&out, Role::Button, "Manutenção").is_empty(),
            "the Maintenance tabs stay hidden while Revisão owns the panel"
        );
        let back = widget_rect(&out, Role::Button, "Ir para Revisão");
        click_at(&mut app, &ctx, size, back.center());
        assert_eq!(app.page, Page::Review);
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(
            widgets(&out, Role::Button, "Handoffs").len(),
            1,
            "Revisão shows its tabs"
        );
        assert!(widgets(&out, Role::Label, warning).is_empty());
    }

    #[test]
    fn cross_locks_keep_review_maintenance_and_admin_apart() {
        use egui::accesskit::Role;
        // (página, quem está ocupado, um botão da página, desabilitado?)
        for (page, busy, button, disabled) in [
            (Page::Review, "", "Propostas", false),
            (Page::Review, "auth", "Propostas", true),
            (Page::Review, "operations", "Propostas", true),
            (Page::Maintenance, "", "Manutenção", false),
            (Page::Maintenance, "auth", "Manutenção", true),
            (Page::Maintenance, "operations", "Manutenção", true),
            (Page::Admin, "", "Servidor · administração", false),
            (Page::Admin, "auth", "Servidor · administração", true),
            (Page::Admin, "management", "Servidor · administração", true),
        ] {
            let mut app = app_with_unsaved_draft();
            app.page = page;
            match busy {
                "auth" => app.authentication.busy = true,
                "operations" => app.operations.busy = true,
                "management" => {
                    // Ocupada em Admin não troca de aba: fica onde está.
                    app.management.busy = true;
                }
                _ => {}
            }
            let ctx = styled();
            ctx.enable_accesskit();
            let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
            let update = out.platform_output.accesskit_update.as_ref().unwrap();
            let node = update
                .nodes
                .iter()
                .find(|(_, n)| {
                    n.role() == Role::Button
                        && n.label() == Some(button)
                        && n.bounds().is_some_and(|b| b.x0 > 232.)
                })
                .map(|(_, n)| n.is_disabled());
            assert_eq!(
                node,
                Some(disabled),
                "{page:?} with {busy:?} busy: {button}"
            );
        }
    }

    /// Retângulos dos seis cartões da página Saúde.
    fn health_cards(out: &egui::FullOutput) -> Vec<egui::Rect> {
        crate::health_ui::CARDS
            .iter()
            .map(|title| widget_rect(out, egui::accesskit::Role::Pane, title))
            .collect()
    }
    fn columns(cards: &[egui::Rect]) -> usize {
        let mut lefts: Vec<i32> = cards.iter().map(|r| r.left().round() as i32).collect();
        lefts.sort();
        lefts.dedup();
        lefts.len()
    }

    #[test]
    fn health_grid_takes_its_columns_from_the_width_of_every_frame() {
        let mut app = app_with_unsaved_draft();
        app.page = Page::Health;
        let ctx = styled();
        ctx.enable_accesskit();
        // (janela, colunas, borda direita do conteúdo = janela − 24)
        for (size, want) in [
            ([720., 560.], 2),
            ([1320., 860.], 3),
            ([720., 560.], 2),
            ([1000., 700.], 2),
            ([1320., 860.], 3),
        ] {
            let mut out = frame(&mut app, &ctx, size, vec![]);
            // A grade só lê a largura do frame; um segundo frame não muda nada.
            for _ in 0..2 {
                let cards = health_cards(&out);
                assert_eq!(columns(&cards), want, "{size:?}: {cards:?}");
                let right = cards.iter().map(|r| r.right()).fold(0., f32::max);
                assert!(
                    (right - (size[0] - 24.)).abs() <= 1.,
                    "{size:?}: the grid must span the content width, ends at {right}"
                );
                // Na mesma linha, os cartões têm a mesma altura.
                for row in cards.chunks(want) {
                    assert!(
                        row.iter()
                            .all(|r| (r.height() - row[0].height()).abs() < 0.5),
                        "{size:?}: {row:?}"
                    );
                }
                // A linha da página: o texto e o botão lado a lado, dentro do conteúdo.
                let line = widget_rect(
                    &out,
                    egui::accesskit::Role::Label,
                    "Aguardando a primeira coleta · a cada 30 s, mesmo com a janela fechada",
                );
                let button = widget_rect(&out, egui::accesskit::Role::Button, "Atualizar agora");
                assert!(
                    !line.intersects(button)
                        && button.right() <= size[0] - 24. + 0.5
                        && (button.center().y - line.center().y).abs() < 2.,
                    "{size:?}: {line:?} / {button:?}"
                );
                out = frame(&mut app, &ctx, size, vec![]);
            }
        }
    }

    /// Linux: a janela fecha com um "Atualizar agora" em aberto; o coletor,
    /// que vive no Shared, atende sem janela, e a janela seguinte (outro
    /// contexto egui, o mesmo app) mostra a coleta nova e o botão livre.
    #[test]
    fn refresh_requested_before_the_window_closes_is_served_without_it() {
        use crate::monitor::tests::HealthProbe;
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let config = Config {
            endpoint: "http://synthetic.example".into(),
            ..Default::default()
        };
        let probe = HealthProbe::start(Shared::unstarted(config.clone()));
        assert_eq!(probe.next(), "http://synthetic.example");
        probe.release();
        probe.served(0);
        let mut app = MonitorApp::build(config, probe.shared.clone());
        app.page = Page::Health;
        let sync = |app: &mut MonitorApp| {
            app.last_sync = Instant::now() - Duration::from_secs(1);
            app.sync();
        };
        let until = Instant::now() + Duration::from_secs(3);
        while app.health.at.is_none() {
            assert!(
                Instant::now() < until,
                "the startup collection never reached the app"
            );
            std::thread::sleep(Duration::from_millis(2));
            sync(&mut app);
        }
        let first = app.health.at;
        let window = styled();
        window.enable_accesskit();
        let out = frame(&mut app, &window, size, vec![]);
        let button = widget_rect(&out, Role::Button, "Atualizar agora");
        click_at(&mut app, &window, size, button.center());
        let out = frame(&mut app, &window, size, vec![]);
        let pending = update_node(&out, Role::Button, "Atualizando…");
        assert!(pending.is_disabled(), "the button waits for the collection");
        drop(window);
        assert_eq!(
            probe.next(),
            "http://synthetic.example",
            "collected with no window"
        );
        probe.release();
        probe.served(1);
        sync(&mut app);
        assert!(app.health.at > first, "the new collection reached the app");
        let reopened = styled();
        reopened.enable_accesskit();
        let out = frame(&mut app, &reopened, size, vec![]);
        assert!(!update_node(&out, Role::Button, "Atualizar agora").is_disabled());
        assert!(widgets(&out, Role::Button, "Atualizando…").is_empty());
        assert!(probe.quiet(), "one click, one extra collection");
    }
    fn update_node(
        out: &egui::FullOutput,
        role: egui::accesskit::Role,
        text: &str,
    ) -> egui::accesskit::Node {
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        let found: Vec<_> = update
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.role() == role && (n.label() == Some(text) || n.value() == Some(text))
            })
            .map(|(_, n)| n.clone())
            .collect();
        match &found[..] {
            [n] => n.clone(),
            _ => panic!("expected one {role:?} {text:?}, found {}", found.len()),
        }
    }

    #[test]
    fn freshness_matrix() {
        use Freshness::*;
        let now = Utc::now();
        let at = |secs: i64| Some(now - chrono::Duration::seconds(secs));
        // resource_seconds = 2 (atrasa acima de 6 s); api_seconds = 60 (acima de 180 s).
        for (case, resources, api, paused, want) in [
            ("paused", at(1), at(1), true, Paused),
            (
                "paused wins over both late",
                at(100),
                at(1000),
                true,
                Paused,
            ),
            ("paused wins over connecting", None, None, true, Paused),
            ("connecting", None, None, false, Connecting),
            ("live: youngest source", at(6), at(3), false, Live(3)),
            ("live: youngest is resources", at(2), at(45), false, Live(2)),
            ("resources exactly 3x", at(6), at(1), false, Live(1)),
            ("resources 3x + 1 s", at(7), at(1), false, ResourcesLate(7)),
            ("api exactly 3x", at(1), at(180), false, Live(1)),
            ("api 3x + 1 s", at(1), at(181), false, ApiLate(181)),
            ("only api late", at(2), at(600), false, ApiLate(600)),
            (
                "only resources late",
                at(30),
                at(10),
                false,
                ResourcesLate(30),
            ),
            (
                "both late: api older",
                at(30),
                at(600),
                false,
                BothLate(600),
            ),
            (
                "both late: resources older",
                at(4000),
                at(200),
                false,
                BothLate(4000),
            ),
            ("api absent, resources live", at(3), None, false, Live(3)),
            (
                "api absent, resources late",
                at(10),
                None,
                false,
                ResourcesLate(10),
            ),
            ("resources absent, api live", None, at(30), false, Live(30)),
            (
                "resources absent, api late",
                None,
                at(200),
                false,
                ApiLate(200),
            ),
            ("negative ages count as 0", at(-5), at(-90), false, Live(0)),
            (
                "negative age is never late",
                at(-1000),
                None,
                false,
                Live(0),
            ),
        ] {
            assert_eq!(
                freshness(now, resources, api, 2, 60, paused),
                want,
                "{case}"
            );
        }
    }

    #[test]
    fn freshness_text_for_every_state() {
        use Freshness::*;
        for state in [
            Paused,
            Connecting,
            Live(3),
            ApiLate(181),
            ResourcesLate(7),
            BothLate(3600),
        ] {
            let want = match state {
                Paused => "Pausado",
                Connecting => "Conectando…",
                Live(_) => "Ao vivo · atualizado há 3 s",
                ApiLate(_) => "API atrasada · há 3 min",
                ResourcesLate(_) => "Recursos atrasados · há 7 s",
                BothLate(_) => "Atrasado · há 1 h",
            };
            assert_eq!(state.text(), want, "{state:?}");
        }
    }

    #[test]
    fn ago_switches_units_at_60_s_and_3600_s() {
        for (secs, want) in [
            (-30, "há 0 s"),
            (0, "há 0 s"),
            (59, "há 59 s"),
            (60, "há 1 min"),
            (3599, "há 59 min"),
            (3600, "há 1 h"),
            (7_200_000, "há 2.000 h"),
        ] {
            assert_eq!(ago(secs), want, "{secs} s");
        }
        assert_eq!(
            age(Some(Utc::now() - chrono::Duration::seconds(90))),
            "há 1 min",
            "age() must use the same formatter"
        );
        assert_eq!(age(None), "aguardando");
    }

    #[test]
    fn handoff_badge_only_with_open_handoffs_after_a_collection() {
        let handoffs = vec![Handoff::default(); 3];
        for (case, api, want) in [
            (
                "no collection yet",
                ApiSnapshot {
                    handoff_total: Some(75),
                    handoffs: handoffs.clone(),
                    ..Default::default()
                },
                None,
            ),
            (
                "server total",
                ApiSnapshot {
                    at: Some(Utc::now()),
                    handoff_total: Some(1234),
                    handoffs: handoffs.clone(),
                    ..Default::default()
                },
                Some("1.234"),
            ),
            (
                "list only, may be cut",
                ApiSnapshot {
                    at: Some(Utc::now()),
                    handoffs: handoffs.clone(),
                    ..Default::default()
                },
                Some("3+"),
            ),
            (
                "server total zero",
                ApiSnapshot {
                    at: Some(Utc::now()),
                    handoff_total: Some(0),
                    ..Default::default()
                },
                None,
            ),
            (
                "empty list without total",
                ApiSnapshot {
                    at: Some(Utc::now()),
                    ..Default::default()
                },
                None,
            ),
        ] {
            assert_eq!(handoff_badge(&api).as_deref(), want, "{case}");
        }
    }

    #[test]
    fn host_status_without_and_with_each_collection() {
        let host = format!("Este {}", crate::platform::HOST);
        let mut resources = ResourceSnapshot {
            host_cpu: 23.4,
            host_memory: 3 << 30,
            host_total: 8 << 30,
            ..Default::default()
        };
        let mut api = ApiSnapshot {
            projects: vec![Project::default(); 14],
            ..Default::default()
        };
        assert_eq!(
            host_status(&resources, &api),
            host,
            "no resource collection"
        );
        api.at = Some(Utc::now());
        assert_eq!(
            host_status(&resources, &api),
            host,
            "no resource collection, even with the API"
        );
        resources.at = Some(Utc::now());
        api.at = None;
        assert_eq!(
            host_status(&resources, &api),
            format!("{host} · CPU 23% · Memória 3,0 GiB de 8,0 GiB")
        );
        api.at = Some(Utc::now());
        assert_eq!(
            host_status(&resources, &api),
            format!("{host} · CPU 23% · Memória 3,0 GiB de 8,0 GiB · 14 projetos")
        );
        for (count, want) in [(0, "0 projetos"), (1, "1 projeto"), (2, "2 projetos")] {
            api.projects = vec![Project::default(); count];
            assert_eq!(
                host_status(&resources, &api),
                format!("{host} · CPU 23% · Memória 3,0 GiB de 8,0 GiB · {want}"),
                "{count} projects"
            );
        }
    }

    #[test]
    fn command_comma_opens_settings() {
        let comma = |modifiers| egui::Event::Key {
            key: egui::Key::Comma,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let mut app = app_with_unsaved_draft();
        let ctx = styled();
        assert_eq!(app.page, Page::Overview);
        frame(
            &mut app,
            &ctx,
            [1320., 860.],
            vec![comma(egui::Modifiers::NONE)],
        );
        assert_eq!(app.page, Page::Overview, "a bare comma must not navigate");
        frame(
            &mut app,
            &ctx,
            [1320., 860.],
            vec![comma(egui::Modifiers::COMMAND)],
        );
        assert_eq!(app.page, Page::Settings, "Cmd/Ctrl + , must open Ajustes");
    }

    /// Nós do AccessKit com este papel e texto (o rótulo, ou o valor nos de
    /// papel `Label`), com os retângulos.
    fn widgets(out: &egui::FullOutput, role: egui::accesskit::Role, text: &str) -> Vec<egui::Rect> {
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        update
            .nodes
            .iter()
            .filter(|(_, node)| {
                node.role() == role && (node.label() == Some(text) || node.value() == Some(text))
            })
            .filter_map(|(_, node)| node.bounds())
            .map(|b| {
                egui::Rect::from_min_max(
                    egui::pos2(b.x0 as f32, b.y0 as f32),
                    egui::pos2(b.x1 as f32, b.y1 as f32),
                )
            })
            .collect()
    }
    fn widget_rect(out: &egui::FullOutput, role: egui::accesskit::Role, text: &str) -> egui::Rect {
        match widgets(out, role, text)[..] {
            [rect] => rect,
            ref found => panic!("expected one {role:?} {text:?}, found {}", found.len()),
        }
    }
    /// Repousa o ponteiro em `pos` além do atraso do tooltip (0,5 s; sem
    /// relógio, cada frame avança 1/60 s) e devolve o último frame.
    fn rest_pointer(
        app: &mut MonitorApp,
        ctx: &egui::Context,
        size: [f32; 2],
        pos: egui::Pos2,
    ) -> egui::FullOutput {
        let mut out = frame(app, ctx, size, vec![egui::Event::PointerMoved(pos)]);
        for _ in 0..45 {
            out = frame(app, ctx, size, vec![]);
        }
        out
    }

    #[test]
    fn a_cut_sidebar_label_shows_its_whole_name_on_hover() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        // Sem o total do servidor, o contador "999+" corta "Sessões e handoffs".
        app.api = ApiSnapshot {
            at: Some(Utc::now()),
            handoffs: vec![Handoff::default(); 999],
            ..Default::default()
        };
        let ctx = styled();
        ctx.enable_accesskit();
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(
            sidebar_texts(&ctx, &out).contains(&("Sessões e handoffs".to_owned(), true)),
            "precondition: the label is cut"
        );
        let sessions = widget_rect(&out, Role::Button, "Sessões e handoffs");
        let docker = widget_rect(&out, Role::Button, "Docker");
        let tooltips = |out: &egui::FullOutput, text| widgets(out, Role::Label, text).len();
        let before = tooltips(&out, "Sessões e handoffs");
        let out = rest_pointer(&mut app, &ctx, size, sessions.center());
        assert_eq!(
            tooltips(&out, "Sessões e handoffs"),
            before + 1,
            "hovering a cut label must show the whole name"
        );
        // Um rótulo inteiro não ganha tooltip.
        let before = tooltips(&out, "Docker");
        let out = rest_pointer(&mut app, &ctx, size, docker.center());
        assert_eq!(
            tooltips(&out, "Docker"),
            before,
            "a label that fits needs no tooltip"
        );
    }

    #[test]
    fn source_rows_name_the_source_in_tooltip_and_accesskit() {
        use egui::accesskit::Role;
        let mut app = app_with_unsaved_draft();
        let ctx = styled();
        ctx.enable_accesskit();
        let shows = |out: &egui::FullOutput, text: &str| widgets(out, Role::Label, text).len();
        let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
        for text in [
            "ai-memory: aguardando primeira coleta",
            "Docker: aguardando primeira coleta",
        ] {
            assert_eq!(shows(&out, text), 1, "{text}");
        }
        app.api.at = Some(Utc::now());
        app.api.version = "3.3.1".into();
        app.resources.at = Some(Utc::now());
        app.resources.docker_error = Some("Docker CLI não encontrado.".into());
        let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
        for text in ["ai-memory: conectado", "Docker: Docker CLI não encontrado."] {
            assert_eq!(shows(&out, text), 1, "{text}");
        }
        assert!(
            sidebar_texts(&ctx, &out).contains(&("ai-memory 3.3.1".to_owned(), false)),
            "the row still shows the version"
        );
        // Erro com coleta anterior: vale o erro. No compacto, o tooltip do
        // ponto diz o mesmo que o AccessKit.
        app.api.error = Some("Falha ao conectar.".into());
        let size = [720., 560.];
        let out = frame(&mut app, &ctx, size, vec![]);
        let dot = widget_rect(&out, Role::Label, "ai-memory: Falha ao conectar.");
        let out = rest_pointer(&mut app, &ctx, size, dot.center());
        assert_eq!(
            shows(&out, "ai-memory: Falha ao conectar."),
            2,
            "the compact dot's tooltip must name the source like its AccessKit label"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn macos_drags_the_window_from_empty_bar_areas_but_not_from_the_pause_button() {
        use egui::{Event, PointerButton, ViewportCommand, accesskit::Role};
        fn commands(out: &egui::FullOutput) -> Vec<ViewportCommand> {
            out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .clone()
        }
        let size = [1320., 860.];
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        // Pressiona, arrasta 40 pontos e solta; devolve os comandos dos três frames.
        let drag = |app: &mut MonitorApp, ctx: &egui::Context, from: egui::Pos2| {
            let to = from + Vec2::new(40., 0.);
            [
                vec![Event::PointerMoved(from), press(from, true)],
                vec![Event::PointerMoved(to)],
                vec![press(to, false)],
            ]
            .into_iter()
            .flat_map(|events| commands(&frame(app, ctx, size, events)))
            .filter(|c| *c == ViewportCommand::StartDrag)
            .count()
        };
        let mut app = app_with_unsaved_draft();
        let ctx = styled();
        ctx.enable_accesskit();
        // Primeiro frame: os widgets que os próximos frames vão atingir.
        let out = frame(&mut app, &ctx, size, vec![]);
        let pause = widget_rect(&out, Role::Button, "Pausar atualização da tela");
        let title = widget_rect(&out, Role::Label, "Visão geral");
        let chip = widget_rect(&out, Role::Label, "Conectando…");
        // Entre o título e o chip, e no meio da faixa de 52 px da lateral.
        let empty_bar = egui::pos2((title.right() + chip.left()) / 2., pause.center().y);
        let sidebar_strip = egui::pos2(110., 26.);

        assert_eq!(
            drag(&mut app, &ctx, empty_bar),
            1,
            "empty top bar must drag"
        );
        assert_eq!(
            drag(&mut app, &ctx, sidebar_strip),
            1,
            "sidebar strip must drag"
        );
        assert_eq!(
            drag(&mut app, &ctx, pause.center()),
            0,
            "dragging from the pause button must not move the window"
        );
        assert!(!app.paused, "a drag is not a click");
        // O fundo vem antes: um clique no botão ainda chega a ele.
        frame(
            &mut app,
            &ctx,
            size,
            vec![
                Event::PointerMoved(pause.center()),
                press(pause.center(), true),
            ],
        );
        frame(&mut app, &ctx, size, vec![press(pause.center(), false)]);
        assert!(app.paused, "a click on the pause button must still pause");

        let mut maximize = Vec::new();
        for pressed in [true, false, true, false] {
            maximize.extend(commands(&frame(
                &mut app,
                &ctx,
                size,
                vec![press(empty_bar, pressed)],
            )));
        }
        assert!(
            maximize.contains(&ViewportCommand::Maximized(true)),
            "double click on the empty bar must maximize: {maximize:?}"
        );
    }

    #[test]
    fn main_area_scrolls_sideways_when_a_page_is_wider_than_720() {
        // A linha de filtros da aba Sessões (busca, projeto, "Só abertas")
        // passa da área de conteúdo em 720 pontos, com ou sem dados.
        let label = |out: &egui::FullOutput| {
            out.shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(t) if t.galley.text() == "Só abertas" => {
                        Some(t.visual_bounding_rect())
                    }
                    _ => None,
                })
                .expect("the Sessões tab shows the filter row")
        };
        let mut app = app_with_unsaved_draft();
        app.page = Page::Sessions;
        app.session_tab = 1;
        let ctx = styled();
        let size = [720., 560.];
        let before = label(&frame(&mut app, &ctx, size, vec![]));
        assert!(
            before.right() > 720. - 24.,
            "precondition: the filter row overflows the content area ({})",
            before.right()
        );
        // Ponteiro sobre a linha de filtros, dentro da área visível.
        let pointer = egui::Event::PointerMoved(egui::pos2(400., before.center().y));
        frame(&mut app, &ctx, size, vec![pointer.clone()]);
        frame(
            &mut app,
            &ctx,
            size,
            vec![
                pointer.clone(),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(-300., 0.),
                    modifiers: Default::default(),
                },
            ],
        );
        let mut after = before;
        for _ in 0..30 {
            after = label(&frame(&mut app, &ctx, size, vec![pointer.clone()]));
        }
        assert!(
            after.left() < before.left() - 50.,
            "content cut at 720 must be reachable by scrolling sideways: x {} -> {}",
            before.left(),
            after.left()
        );
    }
}
