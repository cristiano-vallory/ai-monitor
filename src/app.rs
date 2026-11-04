use crate::{management_ui::Section, model::*, monitor::Shared, theme};
use chrono::{DateTime, Local, Utc};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
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

/// Da página à borda direita da barra de rolagem dela, na margem de 24 do
/// painel central.
const PAGE_GAP: f32 = 16.;
/// A janela de Ajustes (Settings.dc.html), em pontos.
const SETTINGS_SIZE: [f32; 2] = [780., 620.];
/// macOS: a faixa do título, no cromo, sob os botões da janela (a barra de
/// título é transparente). No Linux, o título é do gerenciador de janelas.
#[cfg(target_os = "macos")]
const SETTINGS_TITLE: f32 = 34.;
#[cfg(not(target_os = "macos"))]
const SETTINGS_TITLE: f32 = 0.;
pub(crate) fn settings_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("settings")
}
/// Abas da janela de Ajustes, na ordem do cromo.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum SettingsTab {
    #[default]
    General,
    Connection,
    Tray,
    Account,
}
impl SettingsTab {
    const ALL: [SettingsTab; 4] = [
        SettingsTab::General,
        SettingsTab::Connection,
        SettingsTab::Tray,
        SettingsTab::Account,
    ];
    /// Rótulo da aba e título da janela.
    fn label(self) -> &'static str {
        match self {
            SettingsTab::General => "Geral",
            SettingsTab::Connection => "Conexão",
            SettingsTab::Tray => crate::platform::TRAY_TAB,
            SettingsTab::Account => "Conta",
        }
    }
}
/// A janela de Ajustes: aberta ou não, a aba, um pedido de trazê-la à frente
/// e o último `SetTheme` enviado a ela (`None` = nada enviado a esta janela).
/// `raise_again`: aberta pela barra ou pela bandeja, que trazem a principal à
/// frente no mesmo frame (o Focus dela é tratado depois do de Ajustes); então
/// Ajustes vem à frente de novo no frame seguinte.
#[derive(Default)]
struct SettingsWindow {
    open: bool,
    tab: SettingsTab,
    raise: bool,
    raise_again: bool,
    theme: Option<egui::SystemTheme>,
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
    sessions: crate::sessions_ui::Sessions,
    docker: crate::docker_ui::Docker,
    history: VecDeque<Sample>,
    overview: crate::overview_ui::Overview,
    last_sync: Instant,
    settings_message: String,
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
    settings: SettingsWindow,
    /// Grava a troca de tema (`save_theme`); os testes trocam para não
    /// escrever no disco.
    save_theme: fn(Appearance) -> Result<(), String>,
    /// Grava a config de "Salvar e conectar" (`Config::save`); idem.
    save_config: fn(&Config) -> Result<(), String>,
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
        // Ajustes fechou junto com a janela anterior.
        self.close_settings();
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
            sessions: Default::default(),
            docker: Default::default(),
            history: VecDeque::new(),
            overview: Default::default(),
            last_sync: Instant::now() - Duration::from_secs(1),
            settings_message: String::new(),
            paused: false,
            settings: SettingsWindow::default(),
            save_theme,
            save_config: Config::save,
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
                        let item = nav_item(
                            ui,
                            page.label(),
                            Some(self.page == page),
                            compact,
                            aside,
                            |painter, rect, color| paint_icon(painter, rect, page, color),
                        );
                        if item.clicked() {
                            self.page = page;
                        }
                    }
                    ui.add_space(10.);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    let settings = nav_item(
                        ui,
                        "Ajustes",
                        None,
                        compact,
                        Some(Aside::Shortcut(crate::platform::SETTINGS_SHORTCUT)),
                        settings_icon,
                    );
                    if settings.clicked() {
                        self.open_settings();
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

    /// Avisos globais, no topo de todas as páginas: Docker e ai-memory fora
    /// (Bad; o do ai-memory abre Ajustes na aba Conexão) e dados parciais da
    /// API (Warn).
    fn notices(&mut self, ui: &mut egui::Ui) {
        use crate::widgets::{Tone, strip};
        if let Some(e) = &self.resources.docker_error {
            strip(ui, Tone::Bad, "Docker indisponível.", e, None);
        }
        if let Some(e) = &self.api.error
            && strip(
                ui,
                Tone::Bad,
                "Sem conexão com o ai-memory.",
                e,
                Some("Abrir Ajustes"),
            )
            .is_some_and(|r| r.clicked())
        {
            self.settings.tab = SettingsTab::Connection;
            self.open_settings();
        }
        if !self.api.errors.is_empty() {
            strip(
                ui,
                Tone::Warn,
                "Dados parciais:",
                &format!("{} aviso(s). Consulte Ajustes.", self.api.errors.len()),
                None,
            );
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
        self.sessions.reset();
        self.docker.reset();
        self.paused = false;
        self.last_sync = Instant::now() - Duration::from_secs(1);
    }
    /// Abre Ajustes, ou a traz à frente se já estiver aberta.
    fn open_settings(&mut self) {
        self.settings.open = true;
        self.settings.raise = true;
    }
    /// Fecha a janela de Ajustes; o rascunho e a aba ficam.
    fn close_settings(&mut self) {
        self.settings = SettingsWindow {
            tab: self.settings.tab,
            ..Default::default()
        };
    }
    /// macOS: a janela principal escondida. Ajustes fecha junto; senão
    /// voltaria com ela.
    #[cfg(target_os = "macos")]
    fn hide(&mut self, ctx: &egui::Context) {
        self.close_settings();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }
    /// A janela de Ajustes, aberta: uma viewport imediata, que só existe nos
    /// frames em que a principal a mostra. Sem janelas nativas (testes), o
    /// egui a embute como `egui::Window`.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings.open {
            return;
        }
        let title = self.settings.tab.label();
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(SETTINGS_SIZE)
            .with_resizable(false)
            .with_app_id("ai-monitor");
        // macOS: o cromo vai até o topo, sob o título e os botões da janela.
        #[cfg(target_os = "macos")]
        let builder = builder
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false);
        ctx.show_viewport_immediate(settings_id(), builder, |ctx, class| {
            if class == egui::ViewportClass::Embedded {
                let mut open = true;
                self.settings.raise = false;
                self.settings.raise_again = false;
                egui::Window::new(title)
                    .id(egui::Id::new("settings-window"))
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                    .frame(egui::Frame::window(&ctx.style()).inner_margin(0))
                    .show(ctx, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(SETTINGS_SIZE.into(), egui::Sense::hover());
                        self.settings_ui(ui, rect, false);
                    });
                if !open {
                    self.close_settings();
                }
                return;
            }
            if std::mem::take(&mut self.settings.raise) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                if std::mem::take(&mut self.settings.raise_again) {
                    self.settings.raise = true;
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            }
            // A barra de título acompanha o tema do app, como a da principal.
            let window = theme::window_theme(self.config.theme, theme::platform_theme());
            if self.settings.theme != Some(window) {
                ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(window));
                self.settings.theme = Some(window);
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let rect = ui.max_rect();
                    self.settings_ui(ui, rect, true);
                });
            // O botão de fechar da janela só fecha a janela: o rascunho fica.
            if ctx.input(|i| i.viewport().close_requested()) {
                self.close_settings();
            }
        });
    }
    /// O conteúdo da janela em `rect`: cromo com as abas, corpo que rola e
    /// rodapé de 52. `native` = janela própria (no macOS, com a faixa do
    /// título, que arrasta a janela).
    fn settings_ui(&mut self, ui: &mut egui::Ui, rect: egui::Rect, native: bool) {
        let p = theme::palette(ui.visuals());
        let title = if native { SETTINGS_TITLE } else { 0. };
        // Abas de 50 com 2 acima (8 sem a faixa do título, como no Linux) e
        // 8 abaixo.
        let above = if title > 0. { 2. } else { 8. };
        let chrome =
            egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), title + above + 50. + 8.));
        let footer =
            egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 52.), rect.max);
        let body = egui::Rect::from_min_max(
            egui::pos2(rect.left(), chrome.bottom()),
            egui::pos2(rect.right(), footer.top()),
        );
        let painter = ui.painter();
        painter.rect_filled(chrome, 0., p.surf2);
        painter.rect_filled(body, 0., p.surf);
        painter.rect_filled(footer, 0., p.surf2);
        let line = Stroke::new(1.0_f32, p.line);
        painter.hline(rect.x_range(), chrome.bottom() - 0.5, line);
        painter.hline(rect.x_range(), footer.top() + 0.5, line);
        // macOS: a faixa do título arrasta a janela (de tamanho fixo: o
        // duplo clique não maximiza, ao contrário da principal).
        #[cfg(target_os = "macos")]
        if native {
            let strip = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), title));
            let strip = ui.interact(strip, ui.id().with("settings-drag"), egui::Sense::drag());
            if strip.drag_started_by(egui::PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
        let tabs = SettingsTab::ALL.len() as f32;
        let left = rect.center().x - (tabs * 96. + (tabs - 1.) * 4.) / 2.;
        for (i, tab) in SettingsTab::ALL.into_iter().enumerate() {
            let at = egui::Rect::from_min_size(
                egui::pos2(left + i as f32 * 100., rect.top() + title + above),
                Vec2::new(96., 50.),
            );
            if settings_tab(ui, at, tab, self.settings.tab == tab).clicked() {
                self.settings.tab = tab;
            }
        }
        // A barra de rolagem à vista (`scroll_bar`), a 8 da borda da janela,
        // na margem de 40 do conteúdo.
        let mut content = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body.with_max_x(body.right() - 8.))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        crate::widgets::scroll_bar(&mut content);
        egui::ScrollArea::vertical()
            .id_salt("settings-body")
            .auto_shrink(false)
            .show(&mut content, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 40,
                        right: 32,
                        top: 22,
                        bottom: 22,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing = Vec2::new(20., 18.);
                        match self.settings.tab {
                            SettingsTab::General => self.general(ui),
                            SettingsTab::Connection => self.connection(ui),
                            SettingsTab::Tray => self.tray(ui),
                            SettingsTab::Account => self.authentication.show(
                                ui,
                                &self.shared,
                                !self.management.busy && !self.operations.busy,
                            ),
                        }
                    });
            });
        let busy = self.management.busy || self.authentication.busy || self.operations.busy;
        let mut bottom = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(footer.shrink2(Vec2::new(20., 0.)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        bottom.spacing_mut().item_spacing.x = 12.;
        if link(&mut bottom, "Restaurar padrões").clicked() {
            self.restore_defaults();
        }
        bottom.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!busy, crate::widgets::Button::primary("Salvar e conectar"))
                .clicked()
            {
                self.save_settings();
            }
            if !self.settings_message.is_empty() {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    message(ui, &self.settings_message);
                });
            }
        });
    }
    /// Geral: aparência, aplicada na hora, e os intervalos de coleta.
    fn general(&mut self, ui: &mut egui::Ui) {
        setting_row(ui, "Aparência:", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.;
                for (value, label) in [
                    (Appearance::System, "Sistema"),
                    (Appearance::Light, "Claro"),
                    (Appearance::Dark, "Escuro"),
                ] {
                    let chosen = self.config.theme == value;
                    if appearance(ui, value, label, chosen).clicked() && !chosen {
                        let save = self.save_theme;
                        self.choose_theme(value, save);
                    }
                }
            });
            crate::widgets::caption(
                ui,
                "“Sistema” acompanha o macOS e, no Linux, a preferência de cor do portal (GNOME, Omarchy).",
            );
        });
        setting_row(ui, "Atualizar a cada:", |ui| {
            interval(
                ui,
                &mut self.draft.resource_seconds,
                1..=60,
                "Intervalo de recursos em segundos",
                "s · CPU e memória",
                "(1–60)",
            );
            interval(
                ui,
                &mut self.draft.api_seconds,
                5..=300,
                "Intervalo de sessões em segundos",
                "s · sessões e handoffs",
                "(5–300)",
            );
        });
    }
    /// Conexão: servidor e token, contêiner e Docker, e o diagnóstico.
    fn connection(&mut self, ui: &mut egui::Ui) {
        use crate::{
            platform::SECRET_STORE,
            widgets::{Button, caption, checkbox, mono_field, text_field},
        };
        let p = theme::palette(ui.visuals());
        let busy = self.management.busy || self.authentication.busy || self.operations.busy;
        setting_row(ui, "Servidor ai-memory:", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.;
                let endpoint = mono_field(ui, &mut self.draft.endpoint, 280.);
                name(ui, &endpoint, "Servidor ai-memory");
                if endpoint.changed() {
                    self.draft.token.clear();
                    self.draft.human = None;
                    self.draft.proposal_watch = None;
                    self.settings_message = "Servidor alterado. Informe o token desse endereço ou use login humano após conectar.".into();
                }
                let version = server_version(&self.api, &self.health);
                let (color, state) = match (&self.api.error, self.api.at) {
                    (Some(e), _) => (p.bad, e.clone()),
                    (None, Some(_)) if version.is_empty() => (p.acc, "Conectado".to_owned()),
                    (None, Some(_)) => (p.acc, format!("Conectado · {version}")),
                    (None, None) => (p.text2, "Aguardando primeira coleta".to_owned()),
                };
                connection_state(ui, color, &state);
            });
        });
        setting_row(ui, "Token:", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.;
                let token = text_field(
                    ui,
                    &mut self.draft.token,
                    280.,
                    "Opcional",
                    true,
                    usize::MAX,
                );
                name(ui, &token, "Token");
                let load = format!("Carregar do {SECRET_STORE}");
                if ui
                    .add_enabled(!busy, Button::secondary(&load).small().raised())
                    .clicked()
                {
                    self.load_token();
                }
            });
        });
        setting_row(ui, "", |ui| {
            checkbox(
                ui,
                &mut self.draft.remember_token,
                &format!("Guardar no {SECRET_STORE} para este servidor"),
            );
            caption(
                ui,
                "Sem essa opção, o token fica somente em memória. Senhas e sessões humanas nunca são persistidas.",
            );
        });
        divider(ui);
        setting_row(ui, "Contêiner ai-memory:", |ui| {
            let container = text_field(ui, &mut self.draft.container, 200., "", false, usize::MAX);
            name(ui, &container, "Contêiner ai-memory");
        });
        setting_row(ui, "Executável Docker:", |ui| {
            let docker = mono_field(ui, &mut self.draft.docker_binary, 220.);
            name(ui, &docker, "Executável Docker");
        });
        divider(ui);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 6.;
            ui.label(
                RichText::new("Diagnóstico da conexão").font(crate::widgets::Type::Section.font()),
            );
            let version = server_version(&self.api, &self.health);
            let small = |text: String| RichText::new(text).size(12.);
            ui.label(small(format!(
                "ai-memory {}",
                if version.is_empty() { "—" } else { version }
            )));
            ui.label(small(format!(
                "Recursos: {} · API: {}",
                age(self.resources.at),
                age(self.api.at)
            )));
            ui.label(small("As consultas respeitam o usuário do token. Handoffs de outros usuários podem não estar visíveis.".into()).color(p.text2));
            if self.api.errors.is_empty() && self.api.error.is_none() {
                ui.label(
                    small(
                        if self.api.at.is_some() {
                            "Última coleta da API concluída sem erros."
                        } else {
                            "Conectando…"
                        }
                        .into(),
                    )
                    .color(p.acc),
                );
            }
            for e in &self.api.errors {
                ui.label(small(e.clone()).color(p.warn));
            }
            if let Some(e) = &self.resources.inventory_error {
                ui.label(small(e.clone()).color(p.warn));
            }
        });
    }
    /// Barra de menus (macOS) ou bandeja (Linux): o modo do item (só no
    /// macOS), o projeto das propostas e o ponto de alerta.
    fn tray(&mut self, ui: &mut egui::Ui) {
        use crate::widgets::{caption, checkbox, option, select};
        #[cfg(target_os = "macos")]
        setting_row(ui, "Mostrar na barra:", |ui| {
            // A prévia mostra o que a barra mostraria agora.
            let reading = crate::menu::MenuReading::from_snapshot(
                &self.resources,
                &self.config.container,
                self.config.resource_seconds,
                Utc::now(),
            );
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 10.;
                for (mode, label) in [
                    (BarMode::Icon, "Só o ícone"),
                    (BarMode::IconCpu, "Ícone + CPU"),
                    (BarMode::Full, "Completo"),
                ] {
                    let preview = crate::menu::bar_title(&reading, mode).text;
                    if bar_mode(ui, label, self.draft.bar_mode == mode, &preview).clicked() {
                        self.draft.bar_mode = mode;
                    }
                }
            });
            caption(
                ui,
                "Em Macs com notch, itens largos somem quando a barra lota. O modo completo fica no menu e na dica.",
            );
        });
        setting_row(ui, "Propostas:", |ui| {
            let selected = self
                .draft
                .proposal_watch
                .as_ref()
                .map(crate::client::Scope::label)
                .unwrap_or("Desativado".into());
            select(ui, "Propostas", &selected, 280., |ui| {
                if option(ui, self.draft.proposal_watch.is_none(), "Desativado").clicked() {
                    self.draft.proposal_watch = None;
                }
                for p in &self.api.projects {
                    let scope = crate::client::Scope {
                        workspace: p.workspace_name.clone(),
                        project: p.project_name.clone(),
                    };
                    let label = scope.label();
                    if option(
                        ui,
                        self.draft.proposal_watch.as_ref() == Some(&scope),
                        &label,
                    )
                    .clicked()
                    {
                        self.draft.proposal_watch = Some(scope);
                    }
                }
            });
            caption(ui, crate::platform::PROPOSAL_WATCH_LABEL);
        });
        setting_row(ui, "Alertas:", |ui| {
            checkbox(
                ui,
                &mut self.draft.alert_dot,
                "Ponto âmbar no ícone quando a saúde tiver alertas",
            );
        });
    }
    /// "Carregar do Keychain/chaveiro": o token guardado para o endereço do
    /// rascunho vai ao campo.
    fn load_token(&mut self) {
        self.draft.normalize();
        match crate::keychain::load(&self.draft.endpoint) {
            Ok(Some(token)) => {
                self.draft.token = token.trim().into();
                self.settings_message = "Token carregado no campo. Salve para conectar.".into();
            }
            Ok(None) => self.settings_message = "Nenhum token guardado para esse endereço.".into(),
            Err(e) => self.settings_message = e,
        }
    }
    /// "Salvar e conectar": valida, grava (o token no Keychain/chaveiro e a
    /// config) e reconecta; com erro, só a mensagem.
    fn save_settings(&mut self) {
        self.draft.normalize();
        match self.draft.validate().and_then(|_| {
            match keychain_change(&self.config, &self.draft) {
                KeychainChange::Save { endpoint, token } => crate::keychain::save(endpoint, token)?,
                KeychainChange::Forget(endpoint) => crate::keychain::forget(endpoint)?,
                KeychainChange::Keep => {}
            }
            (self.save_config)(&self.draft)
        }) {
            Ok(()) => {
                self.reconnect(self.draft.clone());
                self.authentication.clear();
                self.settings_message = "Configuração salva. Reconectando…".into();
            }
            Err(e) => self.settings_message = e,
        }
    }
    /// Destino de `shared.navigation` (barra de menus e bandeja, um
    /// `menu::Destination`): a página, ou Ajustes por cima da que estiver;
    /// fora do domínio, nada.
    fn navigate(&mut self, code: u8) {
        use crate::menu::Destination;
        match Destination::from_code(code) {
            Some(Destination::Health) => self.page = Page::Health,
            Some(Destination::Review) => self.page = Page::Review,
            Some(Destination::Sessions) => self.page = Page::Sessions,
            Some(Destination::Settings) => {
                self.open_settings();
                self.settings.raise_again = true;
            }
            Some(Destination::Panel) | None => {}
        }
    }
    /// Um frame da janela: lateral, barras, a página e Ajustes, se aberta.
    /// Sem coleta nem navegação externa, para rodar sem janela nos testes.
    fn ui(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.open_settings();
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
                // Sessões e Docker controlam a própria rolagem (listas,
                // detalhe, inspetor) e cabem na largura.
                if self.page == Page::Sessions {
                    if let Some((scope, id)) = self.sessions.show(ui, &self.api, Utc::now()) {
                        self.management
                            .open_handoff(&self.api.projects, &scope, &id);
                        self.page = Page::Review;
                    }
                    return;
                }
                if self.page == Page::Docker {
                    self.docker
                        .show(ui, &self.resources, &self.config.container, Utc::now());
                    return;
                }
                // Na largura mínima, o que passar da área rola na horizontal
                // em vez de cortar. A barra fica à vista (`scroll_bar`) na
                // margem direita do painel, a `PAGE_GAP` da página; a página
                // não herda o estilo da barra.
                let rect = ui.available_rect_before_wrap();
                let mut area = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(rect.with_max_x(rect.right() + PAGE_GAP))
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                let style = area.style().clone();
                crate::widgets::scroll_bar(&mut area);
                egui::ScrollArea::both()
                    .id_salt("main-scroll")
                    .show(&mut area, |ui| {
                        ui.set_style(style);
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                right: PAGE_GAP as i8,
                                ..Default::default()
                            })
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                match self.page {
                                    Page::Overview => {
                                        use crate::overview_ui::{Data, Go};
                                        let data = Data {
                                            resources: &self.resources,
                                            history: &self.history,
                                            api: &self.api,
                                            health: &self.health,
                                            config: &self.config,
                                            now: Utc::now(),
                                        };
                                        match self.overview.show(ui, &data) {
                                            Some(Go::Health) => self.page = Page::Health,
                                            Some(Go::Sessions) => self.page = Page::Sessions,
                                            Some(Go::Docker(id)) => {
                                                self.page = Page::Docker;
                                                if let Some(id) = id {
                                                    self.docker.open(&id);
                                                }
                                            }
                                            None => {}
                                        }
                                    }
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
                                                        &self.api,
                                                        section,
                                                        Utc::now(),
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
                                                self.operations.show(
                                                    ui,
                                                    &self.shared,
                                                    &self.api.projects,
                                                );
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
                                    // Mostradas acima, fora desta rolagem.
                                    Page::Sessions | Page::Docker => {}
                                }
                            });
                    });
            });
        self.settings_window(ctx);
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
                self.hide(ctx);
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

/// Uma linha de Ajustes (Settings.dc.html): o rótulo alinhado à direita numa
/// coluna de 170 e os controles à esquerda, a 20 dele, empilhados a 8.
/// `label` vazio = só a coluna (um controle sob o da linha anterior).
pub(crate) fn setting_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 20.;
        ui.allocate_ui_with_layout(
            Vec2::new(170., 26.),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_size(Vec2::new(170., 26.));
                if !label.is_empty() {
                    ui.label(RichText::new(label).size(13.));
                }
            },
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 8.;
            add(ui)
        })
        .inner
    })
    .inner
}
/// Nome acessível de um controle cujo rótulo é o da coluna da esquerda.
pub(crate) fn name(ui: &egui::Ui, control: &egui::Response, label: &str) {
    ui.ctx()
        .accesskit_node_builder(control.id, |node| node.set_label(label));
}
/// Um intervalo em segundos: campo de 56 × 24 (arrasta ou digita; o valor
/// fica em `range`), a unidade e os limites em text2.
fn interval(
    ui: &mut egui::Ui,
    value: &mut u64,
    range: std::ops::RangeInclusive<u64>,
    label: &str,
    unit: &str,
    limits: &str,
) {
    let p = theme::palette(ui.visuals());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.;
        let field = ui
            .scope(|ui| {
                let visuals = ui.visuals_mut();
                visuals.text_edit_bg_color = Some(p.field);
                let widgets = &mut visuals.widgets;
                for state in [
                    &mut widgets.inactive,
                    &mut widgets.hovered,
                    &mut widgets.active,
                ] {
                    state.bg_fill = p.field;
                    state.weak_bg_fill = p.field;
                    state.bg_stroke = Stroke::new(1.0_f32, p.line);
                    state.corner_radius = egui::CornerRadius::same(5);
                    state.expansion = 0.;
                }
                widgets.active.bg_stroke = Stroke::new(1.0_f32, p.acc);
                ui.spacing_mut().interact_size = Vec2::new(56., 24.);
                ui.spacing_mut().button_padding = Vec2::new(6., 0.);
                ui.style_mut().drag_value_text_style = egui::TextStyle::Small;
                ui.add_sized([56., 24.], egui::DragValue::new(value).range(range))
            })
            .inner;
        name(ui, &field, label);
        ui.label(RichText::new(unit).size(13.));
        ui.label(RichText::new(limits).size(12.).color(p.text2));
    });
}
/// Uma miniatura de aparência (88 × 58, as duas metades do tema) com o nome
/// embaixo; a escolhida leva o anel acc de 3 e o nome em SemiBold.
/// AccessKit: `RadioButton` marcado ou não.
fn appearance(ui: &mut egui::Ui, value: Appearance, label: &str, chosen: bool) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let (light, dark) = (&theme::LIGHT, &theme::DARK);
    // (fundo, tinta) de cada metade: o cromo e o corpo de cada tema.
    let halves = match value {
        Appearance::System => [(light.surf2, light.text2), (dark.surf, dark.text2)],
        Appearance::Light => [(light.surf2, light.text2), (light.surf, light.text2)],
        Appearance::Dark => [(dark.side, dark.text2), (dark.surf2, dark.text2)],
    };
    let family = if chosen {
        theme::semibold()
    } else {
        egui::FontFamily::Proportional
    };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::new(12., family), p.text);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(galley.size().x.max(88.), 58. + 6. + galley.size().y),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::RadioButton,
            ui.is_enabled(),
            chosen,
            label,
        )
    });
    let painter = ui.painter();
    let thumb = egui::Rect::from_min_size(
        egui::pos2(rect.center().x - 44., rect.top()),
        Vec2::new(88., 58.),
    );
    for (i, (fill, ink)) in halves.into_iter().enumerate() {
        let half = egui::Rect::from_min_size(
            thumb.min + Vec2::new(44. * i as f32, 0.),
            Vec2::new(44., 58.),
        );
        let (outer, inner) = (8, 0);
        let corners = if i == 0 {
            egui::CornerRadius {
                nw: outer,
                sw: outer,
                ne: inner,
                se: inner,
            }
        } else {
            egui::CornerRadius {
                nw: inner,
                sw: inner,
                ne: outer,
                se: outer,
            }
        };
        painter.rect_filled(half, corners, fill);
        // Três traços de 4, a 5 um do outro, com 8 acima e 6 dos lados.
        let ink = crate::widgets::tone(ink, 0.3);
        for (row, (share, color)) in [(0.7, ink), (0.5, ink), (0.6, p.acc)]
            .into_iter()
            .enumerate()
        {
            let bar = egui::Rect::from_min_size(
                half.min + Vec2::new(6., 8. + row as f32 * 9.),
                Vec2::new((44. - 12.) * share, 4.),
            );
            painter.rect_filled(bar, 2., color);
        }
    }
    let ring = if chosen {
        Stroke::new(3.0_f32, p.acc)
    } else {
        Stroke::new(1.0_f32, p.line)
    };
    painter.rect_stroke(thumb, 8., ring, egui::StrokeKind::Outside);
    painter.galley(
        egui::pos2(rect.center().x - galley.size().x / 2., thumb.bottom() + 6.),
        galley,
        p.text,
    );
    response
}
/// Uma opção de "Mostrar na barra:" (Settings.dc.html): o rádio de 14, o
/// nome em 150 e a prévia do item da barra, o chip `menubar` (#2C2E34 no
/// escuro, #ECECEF no claro: o raised e o side) com o pulso e `preview`.
/// AccessKit: `RadioButton` marcado ou não.
#[cfg(target_os = "macos")]
fn bar_mode(ui: &mut egui::Ui, label: &str, chosen: bool, preview: &str) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let chip_fill = if ui.visuals().dark_mode {
        p.raised
    } else {
        p.side
    };
    let painter = ui.painter();
    let name = painter.layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.), p.text);
    let text = painter.layout_no_wrap(preview.to_owned(), egui::FontId::proportional(12.), p.text);
    let gap = if preview.is_empty() { 0. } else { 5. };
    let chip_width = 8. + 14. + gap + text.size().x + 8.;
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(14. + 10. + 150. + 10. + chip_width, 22.),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::RadioButton,
            ui.is_enabled(),
            chosen,
            label,
        )
    });
    let painter = ui.painter();
    let center = egui::pos2(rect.left() + 7., rect.center().y);
    // Como a caixa: desmarcado com a borda text2 sobre o field; marcado em
    // acc com o miolo on_acc.
    if chosen {
        painter.circle_filled(center, 7., p.acc);
        painter.circle_filled(center, 3., p.on_acc);
    } else {
        painter.circle(center, 6.5, p.field, Stroke::new(1.0_f32, p.text2));
    }
    painter.galley(
        egui::pos2(rect.left() + 24., rect.center().y - name.size().y / 2.),
        name,
        p.text,
    );
    let chip = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 24. + 150. + 10., rect.top()),
        Vec2::new(chip_width, 22.),
    );
    painter.rect_filled(chip, 5., chip_fill);
    let icon = egui::Rect::from_min_size(
        egui::pos2(chip.left() + 8., chip.center().y - 7.),
        Vec2::splat(14.),
    );
    crate::widgets::strokes(
        painter,
        icon,
        &[&crate::menu::PULSE],
        crate::menu::PULSE_WIDTH,
        p.text,
    );
    painter.galley(
        egui::pos2(icon.right() + gap, chip.center().y - text.size().y / 2.),
        text,
        p.text,
    );
    response
}
/// Uma aba do cromo de Ajustes: 96 × 50, ícone de 20 sobre o rótulo de 11;
/// a escolhida em sel/acc. AccessKit: `Tab`, marcada como selecionada.
fn settings_tab(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    tab: SettingsTab,
    selected: bool,
) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let label = tab.label();
    let response = ui.interact(
        rect,
        ui.id().with(("settings-tab", label)),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::accesskit::Role::Tab);
        node.set_selected(selected);
    });
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 8., p.sel);
    } else if response.hovered() {
        painter.rect_filled(rect, 8., crate::widgets::tone(p.line, 0.5));
    }
    let color = if selected { p.acc } else { p.text2 };
    let family = if selected {
        theme::semibold()
    } else {
        egui::FontFamily::Proportional
    };
    let galley = painter.layout_no_wrap(label.to_owned(), egui::FontId::new(11., family), color);
    let top = rect.center().y - (20. + 4. + galley.size().y) / 2.;
    let icon = egui::Rect::from_min_size(egui::pos2(rect.center().x - 10., top), Vec2::splat(20.));
    tab_icon(painter, icon, tab, color);
    painter.galley(
        egui::pos2(rect.center().x - galley.size().x / 2., icon.bottom() + 4.),
        galley,
        color,
    );
    response
}
/// Os ícones das abas (paths de 24 do artboard, traço 1,7). Arcos viram
/// polilinhas; a engrenagem, oito dentes.
fn tab_icon(painter: &egui::Painter, rect: egui::Rect, tab: SettingsTab, color: Color32) {
    use crate::widgets::{dot, strokes};
    let scale = rect.width() / 24.;
    let stroke = Stroke::new(1.7 * scale, color);
    let at = |(x, y): (f32, f32)| rect.min + Vec2::new(x, y) * scale;
    match tab {
        SettingsTab::General => {
            painter.circle_stroke(at((12., 12.)), 3. * scale, stroke);
            let gear = (0..8)
                .flat_map(|k| {
                    let a = k as f32 * 45.;
                    [(7.3, a - 14.), (9.6, a - 9.), (9.6, a + 9.), (7.3, a + 14.)]
                })
                .map(|(r, degrees): (f32, f32)| {
                    let t = degrees.to_radians();
                    at((12. + r * t.cos(), 12. + r * t.sin()))
                })
                .collect();
            painter.add(egui::Shape::closed_line(gear, stroke));
        }
        SettingsTab::Connection => {
            for (a, b, r) in [
                ((2., 9.), (22., 9.), 15.),
                ((5., 12.5), (19., 12.5), 10.),
                ((8.5, 16.), (15.5, 16.), 5.),
            ] {
                strokes(painter, rect, &[&arc(a, b, r)], 1.7, color);
            }
            dot(painter, rect, (12., 19.5), 1.7, color);
        }
        SettingsTab::Tray => strokes(
            painter,
            rect,
            &[
                &[(3., 5.), (21., 5.), (21., 9.), (3., 9.), (3., 5.)],
                &[(14., 9.), (14., 19.), (17., 16.), (19., 20.)],
            ],
            1.7,
            color,
        ),
        SettingsTab::Account => {
            painter.circle_stroke(at((12., 8.)), 4. * scale, stroke);
            strokes(
                painter,
                rect,
                &[&arc((4., 21.), (20., 21.), 8.)],
                1.7,
                color,
            );
        }
    }
}
/// Pontos do arco de raio `r` de `a` a `b` (mesma altura, `a` à esquerda),
/// curvado para cima, como o `a r r 0 0 1` dos paths.
fn arc(a: (f32, f32), b: (f32, f32), r: f32) -> Vec<(f32, f32)> {
    let half = (b.0 - a.0) / 2.;
    let center = (a.0 + half, a.1 + (r * r - half * half).max(0.).sqrt());
    let angle = |(x, y): (f32, f32)| (y - center.1).atan2(x - center.0);
    let (mut from, to) = (angle(a), angle(b));
    if from > to {
        from -= std::f32::consts::TAU;
    }
    (0..=16)
        .map(|i| {
            let t = from + (to - from) * i as f32 / 16.;
            (center.0 + r * t.cos(), center.1 + r * t.sin())
        })
        .collect()
}
/// Um botão em estilo de link: texto 12 em text2 (text sob o ponteiro).
fn link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(12.),
        Color32::PLACEHOLDER,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    let color = if response.hovered() { p.text } else { p.text2 };
    ui.painter().galley(rect.min, galley, color);
    response
}
/// `settings_message` no rodapé: 12, em até duas linhas na largura que
/// sobra; além delas, reticências e o texto inteiro no tooltip.
fn message(ui: &mut egui::Ui, text: &str) {
    let p = theme::palette(ui.visuals());
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        egui::FontId::proportional(12.),
        p.text,
        ui.available_width(),
    );
    job.wrap.max_rows = 2;
    let galley = ui.painter().layout_job(job);
    let elided = galley.elided;
    let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    ui.painter().galley(rect.min, galley, p.text);
    if elided {
        response.on_hover_text(text);
    }
}
/// A divisória de largura inteira do corpo de Ajustes.
fn divider(ui: &mut egui::Ui) {
    let p = theme::palette(ui.visuals());
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0_f32, p.line),
    );
}
/// O estado da conexão ao lado do servidor: ponto e texto na cor do estado,
/// numa linha (cortado, o texto inteiro vai ao tooltip). AccessKit: `Label`.
fn connection_state(ui: &mut egui::Ui, color: Color32, text: &str) {
    let galley = crate::widgets::one_line(
        ui,
        text,
        egui::FontId::proportional(12.),
        color,
        ui.available_width() - 13.,
    );
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(13. + galley.size().x, 26.), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    let painter = ui.painter();
    painter.circle_filled(egui::pos2(rect.left() + 3.5, rect.center().y), 3.5, color);
    let elided = galley.elided;
    painter.galley(
        egui::pos2(rect.left() + 13., rect.center().y - galley.size().y / 2.),
        galley,
        color,
    );
    if elided {
        response.on_hover_text(text);
    }
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
/// sempre que for cortado. `selected: None` = um item que não é página
/// (Ajustes, que abre a janela própria): botão, nunca marcado.
fn nav_item(
    ui: &mut egui::Ui,
    label: &str,
    selected: Option<bool>,
    compact: bool,
    aside: Option<Aside>,
    paint: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let p = theme::palette(ui.visuals());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.), egui::Sense::click());
    response.widget_info(|| match selected {
        Some(selected) => {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label)
        }
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label),
    });
    let selected = selected == Some(true);
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
    paint(painter, icon, color);
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
    let galley = crate::widgets::one_line(
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
    }
}
/// O ícone de Ajustes (controles deslizantes), no traço dos da lateral.
fn settings_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    crate::widgets::strokes(
        painter,
        rect,
        &[
            &[(4., 6.), (13., 6.)],
            &[(17., 6.), (20., 6.)],
            &[(15., 4.), (15., 8.)],
            &[(4., 12.), (7., 12.)],
            &[(11., 12.), (20., 12.)],
            &[(9., 10.), (9., 14.)],
            &[(4., 18.), (15., 18.)],
            &[(19., 18.), (20., 18.)],
            &[(17., 16.), (17., 20.)],
        ],
        1.8,
        color,
    );
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
        let galley = crate::widgets::one_line(ui, label, font, p.text2, rect.right() - left);
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
pub(crate) fn date(at: &str) -> String {
    DateTime::parse_from_rfc3339(at)
        .map(|d| d.with_timezone(&Local).format("%d/%m %H:%M").to_string())
        .unwrap_or_else(|_| "—".into())
}
pub(crate) fn short_id(id: &str) -> String {
    id.chars().take(18).collect()
}
pub(crate) fn short_project(s: &str) -> &str {
    s.split(" / ").last().unwrap_or(s)
}

pub(crate) fn handoff_matches(h: &Handoff, scope: &str, needle: &str) -> bool {
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

pub(crate) fn handoff_warning_message(loaded: usize, total: Option<usize>) -> String {
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

pub(crate) fn process_matches(p: &HostProcess, needle: &str) -> bool {
    format!("{} {} {}", p.pid, p.name, p.group)
        .to_lowercase()
        .contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Nenhum teste grava o tema nem a config no disco de quem roda.
        app.save_theme = |_| Ok(());
        app.save_config = |_| Ok(());
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

    /// As 8 páginas, na ordem da lateral. Ajustes, no rodapé, abre a janela
    /// própria e não é página.
    const PAGES: [Page; 8] = [
        Page::Overview,
        Page::Sessions,
        Page::Docker,
        Page::Health,
        Page::Explore,
        Page::Review,
        Page::Maintenance,
        Page::Admin,
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
                | Page::Admin => {}
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
                // As páginas e Ajustes, no rodapé, com o atalho ao lado.
                for label in PAGES.iter().map(|p| p.label()).chain(["Ajustes"]) {
                    let painted: Vec<bool> = texts
                        .iter()
                        .filter(|(text, _)| text == label)
                        .map(|&(_, elided)| elided)
                        .collect();
                    assert!(!painted.is_empty(), "{label} must be painted");
                    let fits = label != Page::Sessions.label() || sessions_fit;
                    assert_eq!(
                        painted.iter().any(|&elided| elided),
                        !fits,
                        "{label} cut = {} at {} px, {selected:?} selected, counter {counter:?}",
                        !fits,
                        sidebar_width(&ctx)
                    );
                }
                assert!(
                    texts.contains(&(crate::platform::SETTINGS_SHORTCUT.to_owned(), false)),
                    "the shortcut is painted whole next to Ajustes"
                );
            }
        }
    }

    #[test]
    fn menu_navigation_opens_each_page_or_settings_and_ignores_the_rest() {
        for code in 0..=u8::MAX {
            let mut app = app_with_unsaved_draft();
            app.page = Page::Docker;
            app.navigate(code);
            // (página, Ajustes aberta) de cada código; o resto não mexe.
            let want = match code {
                1 => (Page::Health, false),
                2 => (Page::Review, false),
                3 => (Page::Sessions, false),
                4 => (Page::Docker, true),
                _ => (Page::Docker, false),
            };
            assert_eq!((app.page, app.settings.open), want, "navigation {code}");
            assert_eq!(app.settings.raise, want.1, "navigation {code}: raised");
        }
    }

    #[test]
    fn the_settings_code_opens_settings_over_the_page_in_the_next_update() {
        use crate::menu::Destination;
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.page = Page::Health;
        let ctx = styled();
        let _native = Native::install(&ctx);
        let mut frame_ = eframe::Frame::_new_kittest();
        let mut update = |app: &mut MonitorApp| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                    ..Default::default()
                },
                |ctx| eframe::App::update(app, ctx, &mut frame_),
            )
        };
        let out = update(&mut app);
        assert!(settings_viewport(&out).is_none(), "precondition: closed");
        use egui::ViewportCommand::Focus;
        let root = |out: &egui::FullOutput| out.viewport_output[&egui::ViewportId::ROOT].clone();
        // Janela nova do Linux: só o código, sem Focus da principal antes.
        // O segundo Focus de Ajustes vem no frame seguinte, já, e não no tick
        // de 500 ms.
        app.shared
            .navigation
            .store(Destination::Settings.code(), Ordering::Release);
        let out = update(&mut app);
        assert!(commands(&out).contains(&Focus));
        assert_eq!(
            root(&out).repaint_delay,
            Duration::ZERO,
            "the next frame now"
        );
        let out = update(&mut app);
        assert!(commands(&out).contains(&Focus), "again in the next frame");
        app.close_settings();
        let out = update(&mut app);
        assert!(
            settings_viewport(&out).is_none(),
            "precondition: closed again"
        );
        // O que a barra (macOS) ou a bandeja (Linux) faz ao clicar Ajustes…:
        // grava o código e traz a principal à frente (`show_dashboard`,
        // `FocusAndNavigate`), com o Focus dela no mesmo frame.
        app.shared
            .navigation
            .store(Destination::Settings.code(), Ordering::Release);
        ctx.send_viewport_cmd(Focus);
        let out = update(&mut app);
        assert!(
            root(&out).commands.contains(&Focus),
            "precondition: the main window"
        );
        let window = settings_viewport(&out).expect("Ajustes opens");
        assert!(window.commands.contains(&Focus));
        assert_eq!(app.page, Page::Health, "over the page it was on");
        assert_eq!(app.shared.navigation.load(Ordering::Acquire), 0, "consumed");
        // O Focus da principal é tratado depois do pass imediato de Ajustes:
        // no frame seguinte, só Ajustes pede o foco, e fica por cima.
        let out = update(&mut app);
        assert!(
            commands(&out).contains(&Focus),
            "Ajustes again, after the main window"
        );
        assert!(!root(&out).commands.contains(&Focus));
        let out = update(&mut app);
        assert!(!commands(&out).contains(&Focus), "then no more");
        assert!(settings_viewport(&out).is_some(), "still open");
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
                // Ajustes não é página: um botão na lateral, nunca marcado.
                let settings: Vec<_> = update
                    .nodes
                    .iter()
                    .filter(|(_, n)| {
                        n.role() == Role::Button
                            && n.label() == Some("Ajustes")
                            && n.bounds().is_some_and(|b| b.x1 <= 232.)
                    })
                    .map(|(_, n)| n.toggled())
                    .collect();
                assert_eq!(settings, vec![None], "{theme:?} Ajustes on {page:?}");
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
                .map(|p| p.label())
                .chain(["Ajustes"])
                .map(|label| (label.to_owned(), widget_rect(&out, Role::Button, label)))
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
        use crate::menu::{MenuReading, entries};
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
        // (caso, vigiado?, leitura, contador na lateral, linha do menu)
        for (case, watch, health, badge, menu) in [
            (
                "nothing watched",
                false,
                reading(0, &scope, Some(3)),
                None,
                "Propostas: selecione um projeto em Ajustes",
            ),
            (
                "three pending",
                true,
                reading(0, &scope, Some(3)),
                Some("3"),
                "3 propostas pendentes · ws / proj",
            ),
            (
                "one pending",
                true,
                reading(0, &scope, Some(1)),
                Some("1"),
                "1 proposta pendente · ws / proj",
            ),
            (
                "199 pending",
                true,
                reading(0, &scope, Some(199)),
                Some("199"),
                "199 propostas pendentes · ws / proj",
            ),
            (
                "list limit",
                true,
                reading(0, &scope, Some(200)),
                Some("200+"),
                "200+ propostas pendentes · ws / proj",
            ),
            (
                "zero is no counter",
                true,
                reading(0, &scope, Some(0)),
                None,
                "0 propostas pendentes · ws / proj",
            ),
            (
                "denied is unknown",
                true,
                reading(0, &scope, None),
                None,
                "Propostas: — · ws / proj",
            ),
            (
                "stale is unknown",
                true,
                reading(91, &scope, Some(3)),
                None,
                "Propostas: — · ws / proj",
            ),
            (
                "another project",
                true,
                reading(0, &other, Some(3)),
                None,
                "Propostas: — · ws / proj",
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
            let reading = MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, now)
                .with_context(None, Some(&health), &config, now);
            // A linha das propostas é a que abre Revisão com texto da leitura.
            let row = entries(&reading)
                .iter()
                .map(|e| e.flat())
                .find(|f| f.action == Some("review") && f.dynamic)
                .unwrap();
            assert_eq!(row.label, menu, "{case}");
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

    /// Clique completo em `pos` (`widgets::tests::click_frames`).
    fn click_at(app: &mut MonitorApp, ctx: &egui::Context, size: [f32; 2], pos: egui::Pos2) {
        for events in crate::widgets::tests::click_frames(pos) {
            frame(app, ctx, size, events);
        }
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
    fn with_a_sheet_open_the_sidebar_and_top_bar_still_work_and_the_receipt_holds() {
        use crate::maintenance::{Action, Intent, Receipt};
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.page = Page::Review;
        app.management.hold_preview(
            Receipt {
                action: Action {
                    scope: crate::client::Scope {
                        workspace: "fixture".into(),
                        project: "demo".into(),
                    },
                    intent: Intent::Approve { id: "p-1".into() },
                },
                generation: 0,
                at: Instant::now(),
            },
            serde_json::json!({"preview": true}),
        );
        let ctx = styled();
        ctx.enable_accesskit();
        // O primeiro frame da folha só mede (sizing pass).
        frame(&mut app, &ctx, size, vec![]);
        let out = frame(&mut app, &ctx, size, vec![]);
        let title = widget_rect(&out, Role::Label, "Aprovar proposta?");
        // O véu cobre a área de conteúdo: da lateral (232) à direita, da
        // barra superior (52) à barra de status (860 − 28).
        let veil = egui::Color32::from_black_alpha(115);
        let veils: Vec<egui::Rect> = crate::widgets::tests::rects(&out)
            .into_iter()
            .filter(|r| r.fill == veil && r.blur_width == 0.)
            .map(|r| r.rect)
            .collect();
        let content = egui::Rect::from_min_max(egui::pos2(232., 52.), egui::pos2(1320., 832.));
        assert_eq!(veils, vec![content]);
        assert!(content.contains_rect(title), "{title:?}");
        assert!(title.top() < 52. + 30., "the sheet hangs from the top bar");
        // A barra superior continua clicável.
        let pause = widget_rect(&out, Role::Button, "Pausar atualização da tela");
        click_at(&mut app, &ctx, size, pause.center());
        assert!(app.paused, "the top bar still clicks");
        // A lateral também; a prévia prende o painel na outra página.
        let maintenance = widget_rect(&out, Role::Button, "Manutenção e backup");
        click_at(&mut app, &ctx, size, maintenance.center());
        assert_eq!(app.page, Page::Maintenance, "the sidebar still clicks");
        let out = frame(&mut app, &ctx, size, vec![]);
        let waiting = "Prévia aguardando confirmação em Revisão · Propostas. Confirme-a lá ou descarte-a para usar esta página.";
        assert_eq!(widgets(&out, Role::Label, waiting).len(), 1);
        assert!(
            widgets(&out, Role::Button, "Cancelar").is_empty(),
            "the sheet stays with its page"
        );
        assert!(app.management.has_receipt());
        let back = widget_rect(&out, Role::Button, "Ir para Revisão");
        click_at(&mut app, &ctx, size, back.center());
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(app.page, Page::Review);
        assert_eq!(
            widgets(&out, Role::Label, "Aprovar proposta?").len(),
            1,
            "back on its page, the sheet is open again"
        );
    }

    #[test]
    fn at_the_minimum_window_the_sheet_keeps_the_warning_and_the_field_above_its_buttons() {
        use crate::maintenance::{Action, Intent, Receipt};
        use egui::accesskit::Role;
        let size = MIN_WINDOW_SIZE;
        let scope = crate::client::Scope {
            workspace: "default".into(),
            project: "acme-servicos-portal-coleta-zero-carbono-v2".into(),
        };
        let target = scope.label();
        // A prévia de 06-sheet-expire-720: bem mais alta que a folha.
        let handoff = serde_json::json!({
            "agent": "claude-code",
            "at": "2026-11-03T18:25:11.964249Z",
            "cwd": "/home/fixture/projetos/portal-coleta-zero-carbono/.claude/worktrees/gifted-chebyshev-95fe00",
            "files_touched": ["src/a.rs", "src/b.rs"],
            "id": "01a1a2a8-771c-7fd1-9d5c-40340d1cd3fe",
            "next_steps": ["um", "dois"],
            "open_questions": ["três"],
        });
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let mut app = app_with_unsaved_draft();
            app.page = Page::Review;
            app.management.set_tab(1);
            app.management.hold_preview(
                Receipt {
                    action: Action {
                        scope: scope.clone(),
                        intent: Intent::ExpireHandoffs { days: 7 },
                    },
                    generation: 0,
                    at: Instant::now(),
                },
                serde_json::json!({ "limited_open_handoffs": vec![handoff.clone(); 20] }),
            );
            let ctx = styled();
            ctx.set_theme(theme);
            ctx.enable_accesskit();
            frame(&mut app, &ctx, size, vec![]);
            let out = frame(&mut app, &ctx, size, vec![]);
            let rects = crate::widgets::tests::rects(&out);
            let veil = egui::Color32::from_black_alpha(115);
            let content = rects
                .iter()
                .find(|r| r.fill == veil && r.blur_width == 0.)
                .expect("the veil over the content area")
                .rect;
            let sheet = rects
                .iter()
                .find(|r| r.corner_radius.se == 12 && r.corner_radius.nw == 0 && r.blur_width == 0.)
                .expect("the sheet")
                .rect;
            let p = theme::palette(&ctx.style_of(theme).visuals);
            let preview = rects
                .iter()
                .find(|r| {
                    r.stroke == egui::Stroke::new(1.0_f32, p.line)
                        && r.corner_radius == egui::CornerRadius::same(8)
                })
                .expect("the preview box")
                .rect;
            let fields: Vec<egui::Rect> = widgets(&out, Role::TextInput, "")
                .into_iter()
                .filter(|r| sheet.contains_rect(*r))
                .collect();
            let [field] = fields[..] else {
                let all = widgets(&out, Role::TextInput, "");
                panic!("{theme:?}: one field in the sheet {sheet:?}, got {fields:?} of {all:?}");
            };
            let warning = widget_rect(
                &out,
                Role::Label,
                "Atenção. O handoff deixa de estar disponível para retomada. A prévia de expiração é limitada a 200 itens; o servidor reavalia todos no momento da ação.",
            );
            let cancel = widget_rect(&out, Role::Button, "Cancelar");
            let confirm = widget_rect(&out, Role::Button, "Expirar");
            let case = format!("{theme:?}: {content:?}");
            // O cenário: a prévia encolheu (sem isso, nada foi exercitado).
            assert!(
                preview.height() < 240.,
                "{case}: the preview did not shrink: {preview:?}"
            );
            for (name, rect) in [
                ("field", field),
                ("warning", warning),
                ("Cancelar", cancel),
                ("Expirar", confirm),
            ] {
                assert!(
                    content.contains_rect(rect),
                    "{case}: {name} at {rect:?} is outside the content area"
                );
            }
            // Na ordem, sem nada rolado para fora: prévia, faixa, campo, botões.
            assert!(preview.bottom() <= warning.top(), "{case}: {warning:?}");
            assert!(warning.bottom() <= field.top(), "{case}: {field:?}");
            assert!(field.bottom() <= cancel.top(), "{case}: {cancel:?}");
            assert!(
                update_node(&out, Role::Button, "Expirar").is_disabled(),
                "{case}: precondition: nothing typed yet"
            );
            // O campo recebe a digitação sem clique.
            let out = frame(
                &mut app,
                &ctx,
                size,
                vec![egui::Event::Text(target.clone())],
            );
            assert!(
                !update_node(&out, Role::Button, "Expirar").is_disabled(),
                "{case}: the typed target enables Expirar"
            );
        }
    }

    #[test]
    fn the_page_scroll_bar_shows_without_hover_only_when_the_page_overflows() {
        use crate::widgets::tests::{THEMES, scroll_handles};
        // As páginas da rolagem principal (Sessões e Docker rolam por conta
        // própria): (janela, página, transborda?).
        let cases = [
            ([720., 560.], Page::Overview, true),
            ([720., 560.], Page::Health, true),
            ([720., 560.], Page::Maintenance, true),
            ([720., 560.], Page::Explore, false),
            ([720., 560.], Page::Review, false),
            ([720., 560.], Page::Admin, false),
            ([1320., 860.], Page::Overview, false),
            ([1320., 860.], Page::Health, false),
            ([1320., 860.], Page::Maintenance, false),
            ([1320., 860.], Page::Explore, false),
            ([1320., 860.], Page::Review, false),
            ([1320., 860.], Page::Admin, false),
        ];
        for (theme, p) in THEMES {
            for (size, page, shown) in cases {
                let mut app = app_with_unsaved_draft();
                app.api.projects.push(Project {
                    workspace_name: "acme".into(),
                    project_name: "api".into(),
                    ..Default::default()
                });
                // Com projeto: os três cartões da Manutenção.
                app.management
                    .open_handoff(&app.api.projects, "acme / api", "");
                app.page = page;
                let ctx = styled();
                ctx.set_theme(theme);
                // Sem ponteiro na janela. A barra aparece com uma animação
                // curta (1/60 s por frame).
                let mut out = frame(&mut app, &ctx, size, vec![]);
                for _ in 0..10 {
                    out = frame(&mut app, &ctx, size, vec![]);
                }
                // Na margem direita do painel (24): a barra termina a 8 da
                // borda da janela, a 16 da página.
                let area = egui::Rect::from_min_max(
                    egui::pos2(0., 52.),
                    egui::pos2(size[0] - 8., size[1] - 28.),
                );
                let handles = scroll_handles(&out, area, p.text2);
                let case = format!("{theme:?} {size:?} {page:?}");
                assert_eq!(handles.len(), usize::from(shown), "{case}: {handles:?}");
                if let Some(handle) = handles.first() {
                    assert_eq!(handle.width(), 4., "{case}");
                    assert!(
                        handle.left() >= size[0] - 24.,
                        "{case}: the bar covers the page: {handle:?}"
                    );
                }
            }
        }
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

    /// A tecla vírgula com `modifiers`.
    fn comma(modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Comma,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }
    /// O atalho de Ajustes como o sistema o entrega: ⌘, no macOS e Ctrl+,
    /// no Linux (os dois também marcam `command`); e a outra tecla, que não
    /// abre (Ctrl no macOS, Alt no Linux).
    #[cfg(target_os = "macos")]
    const SHORTCUT: egui::Modifiers = egui::Modifiers {
        mac_cmd: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    #[cfg(target_os = "macos")]
    const NOT_SHORTCUT: egui::Modifiers = egui::Modifiers::CTRL;
    #[cfg(not(target_os = "macos"))]
    const SHORTCUT: egui::Modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    #[cfg(not(target_os = "macos"))]
    const NOT_SHORTCUT: egui::Modifiers = egui::Modifiers::ALT;
    /// Janelas de Ajustes embutidas (sem janelas nativas, o egui as mostra
    /// como `egui::Window`), pelo título, que é a aba.
    fn settings_windows(out: &egui::FullOutput) -> usize {
        SettingsTab::ALL
            .iter()
            .map(|tab| widgets(out, egui::accesskit::Role::Window, tab.label()).len())
            .sum()
    }
    /// Ajustes aberta na aba `tab`, com AccessKit, já desenhada uma vez.
    fn settings_open_on(tab: SettingsTab) -> (MonitorApp, egui::Context, egui::FullOutput) {
        let mut app = app_with_unsaved_draft();
        app.settings.tab = tab;
        app.open_settings();
        let ctx = styled();
        ctx.enable_accesskit();
        frame(&mut app, &ctx, [1320., 860.], vec![]);
        let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
        (app, ctx, out)
    }
    /// Clica no campo `field`, vai ao fim do texto e digita `text`.
    fn type_into(
        app: &mut MonitorApp,
        ctx: &egui::Context,
        out: &egui::FullOutput,
        role: egui::accesskit::Role,
        field: &str,
        text: &str,
    ) {
        let size = [1320., 860.];
        let at = crate::widgets::tests::rect_of(&update_node(out, role, field));
        click_at(app, ctx, size, at.center());
        frame(app, ctx, size, vec![key(egui::Key::End)]);
        frame(app, ctx, size, vec![egui::Event::Text(text.into())]);
    }
    /// O papel do eframe numa viewport imediata: roda a janela num pass
    /// próprio de 780 × 620, com os eventos de `input`, e guarda o construtor
    /// e a saída de cada pass. `close` manda o botão de fechar da janela no
    /// próximo pass.
    struct Native {
        passes: std::rc::Rc<std::cell::RefCell<Vec<(egui::ViewportBuilder, egui::FullOutput)>>>,
        input: std::rc::Rc<std::cell::RefCell<Vec<egui::Event>>>,
        close: std::rc::Rc<std::cell::Cell<bool>>,
    }
    impl Native {
        fn install(ctx: &egui::Context) -> Self {
            ctx.set_embed_viewports(false);
            let native = Self {
                passes: Default::default(),
                input: Default::default(),
                close: Default::default(),
            };
            let (passes, input, close) = (
                native.passes.clone(),
                native.input.clone(),
                native.close.clone(),
            );
            egui::Context::set_immediate_viewport_renderer(move |ctx, viewport| {
                let mut raw = egui::RawInput {
                    viewport_id: viewport.ids.this,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        SETTINGS_SIZE.into(),
                    )),
                    events: std::mem::take(&mut *input.borrow_mut()),
                    ..Default::default()
                };
                let mut info = egui::ViewportInfo::default();
                if close.take() {
                    info.events.push(egui::ViewportEvent::Close);
                }
                raw.viewports.insert(viewport.ids.this, info);
                let mut show = viewport.viewport_ui_cb;
                let out = ctx.run(raw, |ctx| show(ctx));
                passes.borrow_mut().push((viewport.builder, out));
            });
            native
        }
        /// A saída do último pass da janela.
        fn last(&self) -> egui::FullOutput {
            self.passes
                .borrow()
                .last()
                .expect("a native pass")
                .1
                .clone()
        }
        /// Clique completo em `pos` dentro da janela, um frame da principal
        /// por evento.
        fn click(&self, app: &mut MonitorApp, ctx: &egui::Context, pos: egui::Pos2) {
            for events in crate::widgets::tests::click_frames(pos) {
                *self.input.borrow_mut() = events;
                frame(app, ctx, [1320., 860.], vec![]);
            }
        }
    }
    /// A janela de Ajustes neste frame da principal: o que egui manda ao
    /// eframe criar ou manter (ausente = a janela fecha).
    fn settings_viewport(out: &egui::FullOutput) -> Option<&egui::ViewportOutput> {
        out.viewport_output.get(&settings_id())
    }
    fn commands(out: &egui::FullOutput) -> Vec<egui::ViewportCommand> {
        settings_viewport(out).map_or(vec![], |v| v.commands.clone())
    }

    #[test]
    fn the_shortcut_opens_settings_once_and_never_leaves_the_page() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.page = Page::Docker;
        let ctx = styled();
        ctx.enable_accesskit();
        for (case, modifiers) in [
            ("a bare comma", egui::Modifiers::NONE),
            ("the other platform's key", NOT_SHORTCUT),
        ] {
            frame(&mut app, &ctx, size, vec![comma(modifiers)]);
            let out = frame(&mut app, &ctx, size, vec![]);
            assert!(!app.settings.open, "{case} must not open Ajustes");
            assert_eq!(settings_windows(&out), 0, "{case}");
        }
        // Com a janela aberta, o atalho de novo não abre outra.
        for press in 0..3 {
            frame(&mut app, &ctx, size, vec![comma(SHORTCUT)]);
            let out = frame(&mut app, &ctx, size, vec![]);
            assert!(app.settings.open, "press {press}");
            assert_eq!(settings_windows(&out), 1, "press {press}: one window");
            assert_eq!(
                widgets(&out, Role::Tab, "Geral").len(),
                1,
                "press {press}: one set of tabs"
            );
            assert_eq!(app.page, Page::Docker, "press {press}: the page stays");
        }
    }

    #[test]
    fn settings_is_its_own_780_by_620_window_titled_by_the_tab_and_raised_by_the_shortcut() {
        use egui::{ViewportCommand::*, accesskit::Role};
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        let ctx = styled();
        ctx.enable_accesskit();
        let native = Native::install(&ctx);
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(settings_viewport(&out).is_none(), "closed until asked");
        let out = frame(&mut app, &ctx, size, vec![comma(SHORTCUT)]);
        let window = settings_viewport(&out).expect("a native window");
        // Imediata: desenhada dentro do frame da principal, filha dela.
        assert!(window.viewport_ui_cb.is_none());
        assert_eq!(window.parent, egui::ViewportId::ROOT);
        assert_eq!(window.builder.title.as_deref(), Some("Geral"));
        assert_eq!(window.builder.inner_size, Some(Vec2::new(780., 620.)));
        assert_eq!(window.builder.resizable, Some(false));
        // macOS: o cromo vai até o topo, sob o título e os botões da janela.
        #[cfg(target_os = "macos")]
        assert_eq!(
            (
                window.builder.fullsize_content_view,
                window.builder.titlebar_shown
            ),
            (Some(true), Some(false))
        );
        assert_eq!(out.viewport_output.len(), 2, "the main window and Ajustes");
        assert!(window.commands.contains(&Focus), "{:?}", window.commands);
        assert_eq!(
            settings_windows(&out),
            0,
            "nothing embedded in the main window"
        );
        let inside = native.last();
        let tabs: Vec<_> = SettingsTab::ALL
            .iter()
            .map(|t| update_node(&inside, Role::Tab, t.label()).is_selected())
            .collect();
        assert_eq!(tabs, [Some(true), Some(false), Some(false), Some(false)]);
        // O frame seguinte não pede foco; o atalho, com ela aberta, sim.
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(!commands(&out).contains(&Focus), "{:?}", commands(&out));
        let out = frame(&mut app, &ctx, size, vec![comma(SHORTCUT)]);
        let raised = commands(&out);
        assert!(
            raised.contains(&Minimized(false)) && raised.contains(&Focus),
            "{raised:?}"
        );
        assert_eq!(out.viewport_output.len(), 2, "still one Ajustes");
        // A aba escolhida vira o título da janela.
        let conexao =
            crate::widgets::tests::rect_of(&update_node(&native.last(), Role::Tab, "Conexão"));
        native.click(&mut app, &ctx, conexao.center());
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(app.settings.tab, SettingsTab::Connection);
        assert_eq!(
            settings_viewport(&out).unwrap().builder.title.as_deref(),
            Some("Conexão")
        );
        update_node(&native.last(), Role::TextInput, "Servidor ai-memory");
    }

    #[test]
    fn the_settings_title_bar_follows_the_app_theme() {
        use egui::ViewportCommand::SetTheme;
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        let ctx = styled();
        let native = Native::install(&ctx);
        let themes = |out: &egui::FullOutput| -> Vec<egui::ViewportCommand> {
            commands(out)
                .into_iter()
                .filter(|c| matches!(c, SetTheme(_)))
                .collect()
        };
        let system = theme::window_theme(Appearance::System, theme::platform_theme());
        app.open_settings();
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(themes(&out), vec![SetTheme(system)], "a new window gets it");
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(themes(&out), vec![], "unchanged, not resent");
        app.choose_theme(Appearance::Light, |_| Ok(()));
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(themes(&out), vec![SetTheme(egui::SystemTheme::Light)]);
        // Fechada e aberta de novo: outra janela, que também precisa dele.
        native.close.set(true);
        frame(&mut app, &ctx, size, vec![]);
        assert!(!app.settings.open);
        app.open_settings();
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(themes(&out), vec![SetTheme(egui::SystemTheme::Light)]);
    }

    #[test]
    fn the_native_close_button_closes_only_the_window_and_keeps_the_draft() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.settings.tab = SettingsTab::Connection;
        let ctx = styled();
        ctx.enable_accesskit();
        let native = Native::install(&ctx);
        app.open_settings();
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(settings_viewport(&out).is_some());
        native.close.set(true);
        frame(&mut app, &ctx, size, vec![]);
        assert!(!app.settings.open, "the close button closes Ajustes");
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(
            settings_viewport(&out).is_none(),
            "and the window goes away"
        );
        assert_eq!(app.draft.endpoint, "https://draft.example");
        assert_eq!(app.draft.container, "draft-ai");
        assert_eq!(app.config.endpoint, "https://connected.example");
        assert_eq!(app.shared.generation.load(Ordering::Acquire), 0);
        // De novo: a mesma aba e o mesmo rascunho.
        frame(&mut app, &ctx, size, vec![comma(SHORTCUT)]);
        let inside = native.last();
        assert_eq!(
            update_node(&inside, Role::TextInput, "Servidor ai-memory").value(),
            Some("https://draft.example")
        );
        assert_eq!(
            update_node(&inside, Role::Tab, "Conexão").is_selected(),
            Some(true)
        );
    }

    #[test]
    fn the_window_close_button_closes_only_the_window_and_keeps_the_draft() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let (mut app, ctx, out) = settings_open_on(SettingsTab::Connection);
        assert_eq!(
            update_node(&out, Role::TextInput, "Servidor ai-memory").value(),
            Some("https://draft.example"),
            "precondition: the draft is in the window"
        );
        let close = widget_rect(&out, Role::Button, "Close window");
        click_at(&mut app, &ctx, size, close.center());
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(!app.settings.open);
        assert_eq!(settings_windows(&out), 0);
        assert_eq!(app.draft.endpoint, "https://draft.example");
        assert_eq!(app.draft.container, "draft-ai");
        assert_eq!(app.config.endpoint, "https://connected.example");
        assert_eq!(app.shared.generation.load(Ordering::Acquire), 0);
        frame(&mut app, &ctx, size, vec![comma(SHORTCUT)]);
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(
            update_node(&out, Role::TextInput, "Contêiner ai-memory").value(),
            Some("draft-ai"),
            "reopened on the same tab with the same draft"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn hiding_the_main_window_closes_settings_with_it() {
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        let ctx = styled();
        let _native = Native::install(&ctx);
        app.open_settings();
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(settings_viewport(&out).is_some(), "precondition: open");
        // O frame em que `update` encontra a principal escondida.
        let out = ctx.run(egui::RawInput::default(), |ctx| app.hide(ctx));
        assert!(
            out.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .contains(&egui::ViewportCommand::Visible(false))
        );
        assert!(settings_viewport(&out).is_none(), "Ajustes goes with it");
        // A principal de volta vem sozinha; o rascunho ficou.
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(
            settings_viewport(&out).is_none(),
            "the main window must come back alone, not with Ajustes"
        );
        assert!(!app.settings.open);
        assert_eq!(app.draft.endpoint, "https://draft.example");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn the_settings_title_strip_drags_the_window_and_never_maximizes_it() {
        use egui::{Event, PointerButton, ViewportCommand};
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        let ctx = styled();
        let native = Native::install(&ctx);
        app.open_settings();
        frame(&mut app, &ctx, size, vec![]);
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        // Arrasta da faixa do título (34) ou da linha das abas, entre elas.
        let drag = |app: &mut MonitorApp, from: egui::Pos2| {
            let to = from + Vec2::new(40., 0.);
            [
                vec![Event::PointerMoved(from), press(from, true)],
                vec![Event::PointerMoved(to)],
                vec![press(to, false)],
            ]
            .into_iter()
            .flat_map(|events| {
                *native.input.borrow_mut() = events;
                commands(&frame(app, &ctx, size, vec![]))
            })
            .collect::<Vec<_>>()
        };
        let title = egui::pos2(390., 17.);
        let sent = drag(&mut app, title);
        assert!(
            sent.contains(&ViewportCommand::StartDrag),
            "the title strip must drag the window: {sent:?}"
        );
        let sent = drag(&mut app, egui::pos2(60., 60.));
        assert!(
            !sent.contains(&ViewportCommand::StartDrag),
            "below the title strip: {sent:?}"
        );
        let mut sent = Vec::new();
        for pressed in [true, false, true, false] {
            *native.input.borrow_mut() = vec![press(title, pressed)];
            sent.extend(commands(&frame(&mut app, &ctx, size, vec![])));
        }
        assert!(
            !sent
                .iter()
                .any(|c| matches!(c, ViewportCommand::Maximized(_))),
            "a double click must not maximize a fixed-size window: {sent:?}"
        );
    }

    #[test]
    fn the_account_tab_keeps_the_human_session_controls() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let (mut app, ctx, out) = settings_open_on(SettingsTab::Account);
        widget_rect(
            &out,
            Role::Label,
            "Bearer configurado ou acesso anônimo. Login usa credenciais humanas separadas.",
        );
        app.shared.config.write().unwrap().human =
            Some(crate::auth::Session::for_test("sessao", "csrf"));
        let out = frame(&mut app, &ctx, size, vec![]);
        widget_rect(
            &out,
            Role::Label,
            "Sessão humana. Cookies e CSRF somente em memória.",
        );
        assert!(widgets(&out, Role::TextInput, "Usuário").is_empty());
        for button in [
            "Consultar identidade e capacidades",
            "Sair da sessão humana e voltar ao Bearer",
        ] {
            widget_rect(&out, Role::Button, button);
        }
        let fields = ["Senha atual", "Nova senha", "Confirmar nova senha"];
        assert!(
            fields
                .iter()
                .all(|f| widgets(&out, Role::PasswordInput, f).is_empty()),
            "the password fields wait for Alterar minha senha"
        );
        let change = widget_rect(&out, Role::Button, "Alterar minha senha");
        click_at(&mut app, &ctx, size, change.center());
        let out = frame(&mut app, &ctx, size, vec![]);
        for field in fields {
            widget_rect(&out, Role::PasswordInput, field);
        }
        assert!(!update_node(&out, Role::Button, "Alterar senha e renovar sessão").is_disabled());
        // Ocupado em Revisão: os campos e botões da conta esperam.
        app.management.busy = true;
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(update_node(&out, Role::Button, "Alterar senha e renovar sessão").is_disabled());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_new_linux_window_opens_without_the_settings_of_the_last_one() {
        let mut app = app_with_unsaved_draft();
        app.open_settings();
        app.attach(&styled());
        assert!(!app.settings.open);
        assert_eq!(
            app.draft.endpoint, "https://draft.example",
            "the draft stays"
        );
    }

    #[test]
    fn the_sidebar_item_and_open_settings_open_the_window_from_the_page() {
        use egui::accesskit::Role;
        for (size, compact) in [([1320., 860.], false), ([720., 560.], true)] {
            let mut app = app_with_unsaved_draft();
            app.page = Page::Docker;
            let ctx = styled();
            ctx.enable_accesskit();
            let out = frame(&mut app, &ctx, size, vec![]);
            let texts = sidebar_texts(&ctx, &out);
            let shortcut = crate::platform::SETTINGS_SHORTCUT;
            for text in ["Ajustes", shortcut] {
                assert_eq!(
                    texts.iter().any(|(t, _)| t == text),
                    !compact,
                    "{size:?}: {text} painted only in the full sidebar"
                );
            }
            // Oito páginas marcáveis e Ajustes, um botão.
            let update = out.platform_output.accesskit_update.as_ref().unwrap();
            let pages = update
                .nodes
                .iter()
                .filter(|(_, n)| {
                    n.role() == Role::Button
                        && n.toggled().is_some()
                        && n.bounds()
                            .is_some_and(|b| b.x1 <= sidebar_width(&ctx) as f64)
                })
                .count();
            assert_eq!(pages, 8, "{size:?}");
            let item = widget_rect(&out, Role::Button, "Ajustes");
            click_at(&mut app, &ctx, size, item.center());
            assert!(app.settings.open, "{size:?}: the item opens Ajustes");
            assert_eq!(app.page, Page::Docker, "{size:?}: the page stays");
        }
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.page = Page::Sessions;
        app.api.error = Some("Falha ao conectar.".into());
        let ctx = styled();
        ctx.enable_accesskit();
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(!app.settings.open);
        let open = widget_rect(&out, Role::Button, "Abrir Ajustes");
        click_at(&mut app, &ctx, size, open.center());
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(app.settings.open, "Abrir Ajustes opens the window");
        assert_eq!(settings_windows(&out), 1);
        assert_eq!(app.page, Page::Sessions);
    }

    #[test]
    fn the_connection_strip_opens_on_connection_and_the_rest_keep_the_last_tab() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let shown = |out: &egui::FullOutput, tab: SettingsTab| {
            for other in SettingsTab::ALL {
                assert_eq!(
                    update_node(out, Role::Tab, other.label()).is_selected(),
                    Some(other == tab),
                    "{tab:?} shown: {other:?}"
                );
            }
        };
        for last in SettingsTab::ALL {
            // ⌘, (Ctrl+, no Linux) e o item da lateral: a última aba usada.
            for way in ["shortcut", "sidebar"] {
                let mut app = app_with_unsaved_draft();
                app.settings.tab = last;
                app.api.error = Some("Falha ao conectar.".into());
                let ctx = styled();
                ctx.enable_accesskit();
                let out = frame(&mut app, &ctx, size, vec![]);
                assert!(!app.settings.open, "{way} {last:?}: precondition");
                if way == "shortcut" {
                    frame(&mut app, &ctx, size, vec![comma(SHORTCUT)]);
                } else {
                    let item = widget_rect(&out, Role::Button, "Ajustes");
                    click_at(&mut app, &ctx, size, item.center());
                }
                let out = frame(&mut app, &ctx, size, vec![]);
                assert!(app.settings.open, "{way} {last:?}");
                assert_eq!(app.settings.tab, last, "{way}: the last tab");
                shown(&out, last);
            }
            // "Abrir Ajustes" da faixa: Conexão, fechada ou aberta noutra aba.
            for open in [false, true] {
                let mut app = app_with_unsaved_draft();
                app.settings.tab = last;
                app.api.error = Some("Falha ao conectar.".into());
                if open {
                    app.open_settings();
                }
                let ctx = styled();
                ctx.enable_accesskit();
                frame(&mut app, &ctx, size, vec![]);
                let out = frame(&mut app, &ctx, size, vec![]);
                assert_eq!(settings_windows(&out), usize::from(open), "{last:?} {open}");
                let strip = widget_rect(&out, Role::Button, "Abrir Ajustes");
                click_at(&mut app, &ctx, size, strip.center());
                let out = frame(&mut app, &ctx, size, vec![]);
                assert!(app.settings.open, "{last:?} {open}");
                assert_eq!(app.settings.tab, SettingsTab::Connection, "{last:?} {open}");
                shown(&out, SettingsTab::Connection);
                widget_rect(&out, Role::TextInput, "Servidor ai-memory");
                assert_eq!(app.page, Page::Overview, "{last:?} {open}: the page stays");
            }
        }
    }

    #[test]
    fn the_settings_secondaries_never_look_like_the_fields_in_either_theme() {
        use crate::widgets::tests::THEMES;
        use egui::accesskit::Role;
        let load = format!("Carregar do {}", crate::platform::SECRET_STORE);
        // (aba, com sessão humana?, botões, o campo da aba: papel e nome)
        let cases = [
            (
                SettingsTab::Connection,
                false,
                vec![load.as_str()],
                (Role::PasswordInput, "Token"),
            ),
            (
                SettingsTab::Account,
                false,
                vec!["Entrar com sessão humana"],
                (Role::TextInput, "Usuário"),
            ),
            (
                SettingsTab::Account,
                true,
                vec![
                    "Consultar identidade e capacidades",
                    "Sair da sessão humana e voltar ao Bearer",
                    "Alterar senha e renovar sessão",
                ],
                (Role::PasswordInput, "Senha atual"),
            ),
        ];
        for (theme, p) in THEMES {
            for (tab, human, buttons, (role, field)) in &cases {
                let size = [1320., 860.];
                let mut app = app_with_unsaved_draft();
                app.settings.tab = *tab;
                if *human {
                    app.shared.config.write().unwrap().human =
                        Some(crate::auth::Session::for_test("sessao", "csrf"));
                }
                app.open_settings();
                let ctx = styled();
                ctx.set_theme(theme);
                ctx.enable_accesskit();
                // O 1º frame da janela embutida só mede.
                frame(&mut app, &ctx, size, vec![]);
                let mut out = frame(&mut app, &ctx, size, vec![]);
                if *human {
                    let change = widget_rect(&out, Role::Button, "Alterar minha senha");
                    click_at(&mut app, &ctx, size, change.center());
                }
                // A janela embutida aparece com uma animação curta (1/60 s
                // por frame): as cores só valem depois dela.
                for _ in 0..20 {
                    out = frame(&mut app, &ctx, size, vec![]);
                }
                let fill = |at: egui::Rect| -> Vec<egui::Color32> {
                    crate::widgets::tests::rects(&out)
                        .into_iter()
                        .filter(|r| r.rect == at)
                        .map(|r| r.fill)
                        .collect()
                };
                let typed = fill(widget_rect(&out, *role, field));
                assert_eq!(typed, [p.field], "{theme:?} {field}: precondition");
                for button in buttons {
                    let case = format!("{theme:?} {button}");
                    let pressed = fill(widget_rect(&out, Role::Button, button));
                    assert_eq!(pressed, [p.raised], "{case}");
                    assert_ne!(pressed, typed, "{case}: looks like the field");
                }
            }
        }
        // No escuro, o surf do artboard: mais claro que o corpo.
        assert_eq!(
            theme::DARK.raised,
            egui::Color32::from_rgb(0x2C, 0x2E, 0x34)
        );
    }

    #[test]
    fn the_address_and_the_docker_path_are_mono_and_the_other_fields_are_not() {
        use egui::accesskit::Role;
        let mut app = app_with_unsaved_draft();
        app.draft.token = "segredo".into();
        app.draft.docker_binary = "/usr/local/bin/docker".into();
        app.settings.tab = SettingsTab::Connection;
        app.open_settings();
        let ctx = styled();
        ctx.enable_accesskit();
        frame(&mut app, &ctx, [1320., 860.], vec![]);
        let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
        let fonts = |role: Role, name: &str| -> Vec<(String, egui::FontId)> {
            let at = widget_rect(&out, role, name);
            crate::widgets::tests::shapes(&out)
                .into_iter()
                .filter_map(|s| match s {
                    egui::Shape::Text(t)
                        if at.contains_rect(t.galley.rect.translate(t.pos.to_vec2())) =>
                    {
                        Some((
                            t.galley.text().to_owned(),
                            t.galley.job.sections.first()?.format.font_id.clone(),
                        ))
                    }
                    _ => None,
                })
                .collect()
        };
        let mono = egui::FontId::monospace(12.);
        let plain = egui::FontId::proportional(12.);
        for (role, name, want) in [
            (
                Role::TextInput,
                "Servidor ai-memory",
                ("https://draft.example", &mono),
            ),
            (
                Role::TextInput,
                "Executável Docker",
                ("/usr/local/bin/docker", &mono),
            ),
            (Role::TextInput, "Contêiner ai-memory", ("draft-ai", &plain)),
            (Role::PasswordInput, "Token", ("•••••••", &plain)),
        ] {
            assert_eq!(
                fonts(role, name),
                [(want.0.to_owned(), want.1.clone())],
                "{name}"
            );
        }
    }

    #[test]
    fn the_tabs_switch_the_content_and_expose_the_selected_one() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let (mut app, ctx, mut out) = settings_open_on(SettingsTab::General);
        // As quatro abas de 96 × 50, a 4 uma da outra, centradas na janela.
        let tabs: Vec<egui::Rect> = SettingsTab::ALL
            .iter()
            .map(|t| widget_rect(&out, Role::Tab, t.label()))
            .collect();
        let window = widget_rect(&out, Role::Window, "Geral");
        assert!(
            tabs.iter().all(|r| r.size() == Vec2::new(96., 50.)),
            "{tabs:?}"
        );
        assert!(
            tabs.windows(2)
                .all(|w| (w[1].left() - w[0].right() - 4.).abs() < 0.01),
            "{tabs:?}"
        );
        let middle = (tabs[0].left() + tabs[3].right()) / 2.;
        assert!(
            (middle - window.center().x).abs() < 1.,
            "{middle} in {window:?}"
        );
        // (aba, um controle só dela), saindo de Geral e voltando a ela.
        for (tab, role, only) in [
            (
                SettingsTab::Connection,
                Role::TextInput,
                "Servidor ai-memory",
            ),
            (SettingsTab::Tray, Role::ComboBox, "Desativado"),
            (SettingsTab::Account, Role::TextInput, "Usuário"),
            (SettingsTab::General, Role::RadioButton, "Sistema"),
        ] {
            assert!(
                widgets(&out, role, only).is_empty(),
                "{tab:?}: not shown before"
            );
            let at = widget_rect(&out, Role::Tab, tab.label());
            click_at(&mut app, &ctx, size, at.center());
            out = frame(&mut app, &ctx, size, vec![]);
            assert_eq!(app.settings.tab, tab);
            for other in SettingsTab::ALL {
                assert_eq!(
                    update_node(&out, Role::Tab, other.label()).is_selected(),
                    Some(other == tab),
                    "on {tab:?}: {other:?}"
                );
            }
            assert_eq!(
                widgets(&out, Role::Window, tab.label()).len(),
                1,
                "the title is the tab"
            );
            assert_eq!(
                widgets(&out, role, only).len(),
                1,
                "{tab:?} shows its content"
            );
        }
        let tray = if cfg!(target_os = "macos") {
            "Barra de menus"
        } else {
            "Bandeja"
        };
        assert_eq!(SettingsTab::Tray.label(), tray);
    }

    #[test]
    fn the_settings_tabs_keep_8_above_them_without_the_title_strip() {
        use egui::accesskit::Role;
        for native in [false, true] {
            // Só a faixa do título do macOS (34) fica a 2 das abas; sem
            // ela (Linux, e a janela embutida), 8.
            let above = if native && cfg!(target_os = "macos") {
                34. + 2.
            } else {
                8.
            };
            let mut app = app_with_unsaved_draft();
            let ctx = styled();
            ctx.enable_accesskit();
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        SETTINGS_SIZE.into(),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            let rect = ui.max_rect();
                            app.settings_ui(ui, rect, native);
                        });
                },
            );
            let p = theme::palette(&ctx.style().visuals);
            let body = crate::widgets::tests::rects(&out)
                .into_iter()
                .find(|r| r.fill == p.surf && r.rect.width() == SETTINGS_SIZE[0])
                .expect("the body")
                .rect;
            for tab in SettingsTab::ALL {
                let at = widget_rect(&out, Role::Tab, tab.label());
                assert_eq!(at.top(), above, "native {native}: {tab:?}");
                assert_eq!(body.top(), at.bottom() + 8., "native {native}: 8 below");
            }
        }
    }

    #[test]
    fn the_settings_body_shows_its_scroll_bar_only_when_it_overflows() {
        use crate::widgets::tests::{THEMES, scroll_handles};
        for (theme, p) in THEMES {
            for long in [false, true] {
                let mut app = app_with_unsaved_draft();
                app.settings.tab = SettingsTab::Account;
                if long {
                    let fields: serde_json::Map<String, serde_json::Value> = (0..60)
                        .map(|i| (format!("campo {i:02}"), serde_json::json!("valor")))
                        .collect();
                    app.authentication
                        .hold_info(serde_json::Value::Object(fields));
                }
                app.open_settings();
                let ctx = styled();
                ctx.set_theme(theme);
                let mut out = frame(&mut app, &ctx, [1320., 860.], vec![]);
                for _ in 0..10 {
                    out = frame(&mut app, &ctx, [1320., 860.], vec![]);
                }
                // O corpo: o retângulo surf mais alto da largura da janela
                // (a barra de título da janela embutida também é surf).
                let body = crate::widgets::tests::rects(&out)
                    .into_iter()
                    .filter(|r| r.fill == p.surf && r.rect.width() == SETTINGS_SIZE[0])
                    .map(|r| r.rect)
                    .max_by(|a, b| a.height().total_cmp(&b.height()))
                    .expect("the body");
                // A 8 da borda direita da janela, sem ponteiro sobre ela.
                let area = body.with_max_x(body.right() - 8.);
                let handles = scroll_handles(&out, area, p.text2);
                assert_eq!(
                    handles.len(),
                    usize::from(long),
                    "{theme:?} long {long}: {handles:?}"
                );
            }
        }
    }

    #[test]
    fn every_control_is_on_its_tab_and_the_ones_left_out_do_not_exist() {
        use SettingsTab::*;
        use egui::accesskit::Role;
        let store = crate::platform::SECRET_STORE;
        let load = format!("Carregar do {store}");
        let remember = format!("Guardar no {store} para este servidor");
        let mut controls: Vec<(SettingsTab, Role, &str)> = vec![
            (General, Role::RadioButton, "Sistema"),
            (General, Role::RadioButton, "Claro"),
            (General, Role::RadioButton, "Escuro"),
            (
                General,
                Role::SpinButton,
                "Intervalo de recursos em segundos",
            ),
            (
                General,
                Role::SpinButton,
                "Intervalo de sessões em segundos",
            ),
            (Connection, Role::TextInput, "Servidor ai-memory"),
            (Connection, Role::PasswordInput, "Token"),
            (Connection, Role::Button, &load),
            (Connection, Role::CheckBox, &remember),
            (Connection, Role::TextInput, "Contêiner ai-memory"),
            (Connection, Role::TextInput, "Executável Docker"),
            (Connection, Role::Label, "Diagnóstico da conexão"),
            (Tray, Role::ComboBox, "Desativado"),
            (Tray, Role::Label, crate::platform::PROPOSAL_WATCH_LABEL),
            (
                Tray,
                Role::CheckBox,
                "Ponto âmbar no ícone quando a saúde tiver alertas",
            ),
            (Account, Role::TextInput, "Usuário"),
            (Account, Role::PasswordInput, "Senha"),
            (Account, Role::Button, "Entrar com sessão humana"),
        ];
        // Os modos do item só existem no macOS; no Linux, nem na Bandeja.
        let modes = ["Só o ícone", "Ícone + CPU", "Completo"];
        let mut left_out = vec![
            "Densidade:",
            "Densidade",
            "Confortável",
            "Compacta",
            "Ao iniciar:",
            "Abrir o painel",
            "Iniciar com a sessão do usuário",
            "Autenticação:",
            "Autenticação",
            "Token Bearer",
            "Login humano",
            "Anônimo",
            "Testar conexão",
        ];
        if cfg!(target_os = "macos") {
            controls.extend(modes.map(|mode| (Tray, Role::RadioButton, mode)));
        } else {
            left_out.extend(modes);
            left_out.push("Mostrar na barra:");
        }
        for tab in SettingsTab::ALL {
            let (_, _, out) = settings_open_on(tab);
            for (home, role, name) in &controls {
                assert_eq!(
                    widgets(&out, *role, name).len(),
                    usize::from(*home == tab),
                    "{role:?} {name:?} on {tab:?}"
                );
            }
            for button in ["Restaurar padrões", "Salvar e conectar"] {
                widget_rect(&out, Role::Button, button);
            }
            let update = out.platform_output.accesskit_update.as_ref().unwrap();
            let painted = crate::widgets::tests::texts(&out);
            for &text in &left_out {
                assert!(
                    !update
                        .nodes
                        .iter()
                        .any(|(_, n)| n.label() == Some(text) || n.value() == Some(text)),
                    "{tab:?}: {text:?} is out"
                );
                assert!(
                    !painted.iter().any(|(t, _)| t == text),
                    "{tab:?}: {text:?} painted"
                );
            }
        }
    }

    #[test]
    fn the_intervals_keep_their_limits_when_typed() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let resources = "Intervalo de recursos em segundos";
        let sessions = "Intervalo de sessões em segundos";
        // (campo, digitado, valor): os limites e um além de cada um.
        for (field, typed, want) in [
            (resources, "0", 1),
            (resources, "1", 1),
            (resources, "60", 60),
            (resources, "61", 60),
            (sessions, "4", 5),
            (sessions, "5", 5),
            (sessions, "300", 300),
            (sessions, "301", 300),
        ] {
            let mut app = app_with_unsaved_draft();
            // Sementes fora de todos os casos: cada digitação muda o valor.
            app.draft.resource_seconds = 7;
            app.draft.api_seconds = 42;
            app.open_settings();
            let ctx = styled();
            ctx.enable_accesskit();
            frame(&mut app, &ctx, size, vec![]);
            let out = frame(&mut app, &ctx, size, vec![]);
            let node = update_node(&out, Role::SpinButton, field);
            let limits = if field == resources {
                (1., 60.)
            } else {
                (5., 300.)
            };
            assert_eq!(
                (node.min_numeric_value(), node.max_numeric_value()),
                (Some(limits.0), Some(limits.1)),
                "{field}"
            );
            let at = crate::widgets::tests::rect_of(&node);
            click_at(&mut app, &ctx, size, at.center());
            // Ganho o foco, o texto inteiro fica selecionado.
            frame(&mut app, &ctx, size, vec![]);
            frame(&mut app, &ctx, size, vec![egui::Event::Text(typed.into())]);
            frame(&mut app, &ctx, size, vec![key(egui::Key::Enter)]);
            frame(&mut app, &ctx, size, vec![]);
            let (got, other) = if field == resources {
                (app.draft.resource_seconds, (app.draft.api_seconds, 42))
            } else {
                (app.draft.api_seconds, (app.draft.resource_seconds, 7))
            };
            assert_eq!(got, want, "{field}: typed {typed}");
            assert_eq!(other.0, other.1, "{field}: the other interval is untouched");
            assert_eq!(app.config.resource_seconds, 2, "only the draft changes");
        }
    }

    #[test]
    fn changing_the_server_clears_the_token_the_human_session_and_the_watched_project() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let scope = crate::client::Scope {
            workspace: "ws".into(),
            project: "proj".into(),
        };
        let mut app = app_with_unsaved_draft();
        app.draft.token = "segredo".into();
        app.draft.human = Some(crate::auth::Session::for_test("sessao", "csrf"));
        app.draft.proposal_watch = Some(scope.clone());
        app.settings.tab = SettingsTab::Connection;
        app.open_settings();
        let ctx = styled();
        ctx.enable_accesskit();
        frame(&mut app, &ctx, size, vec![]);
        let out = frame(&mut app, &ctx, size, vec![]);
        // Outro campo (o contêiner) não limpa nada.
        type_into(
            &mut app,
            &ctx,
            &out,
            Role::TextInput,
            "Contêiner ai-memory",
            "x",
        );
        assert_eq!(app.draft.container, "draft-aix", "precondition: typed");
        assert_eq!(app.draft.token, "segredo");
        assert!(app.draft.human.is_some());
        assert_eq!(app.draft.proposal_watch, Some(scope));
        assert!(app.settings_message.is_empty());
        let out = frame(&mut app, &ctx, size, vec![]);
        type_into(
            &mut app,
            &ctx,
            &out,
            Role::TextInput,
            "Servidor ai-memory",
            "/",
        );
        assert_eq!(app.draft.endpoint, "https://draft.example/");
        assert_eq!(app.draft.token, "", "the token was for the old server");
        assert!(app.draft.human.is_none(), "so was the human session");
        assert_eq!(app.draft.proposal_watch, None, "and the watched project");
        let message =
            "Servidor alterado. Informe o token desse endereço ou use login humano após conectar.";
        assert_eq!(app.settings_message, message);
        // A mensagem se lê inteira no rodapé, em até duas linhas.
        let out = frame(&mut app, &ctx, size, vec![]);
        let shown = widget_rect(&out, Role::Label, message);
        let save = widget_rect(&out, Role::Button, "Salvar e conectar");
        assert!(
            (shown.center().y - save.center().y).abs() < 14. && shown.right() < save.left(),
            "in the footer, beside Salvar: {shown:?} / {save:?}"
        );
        let galleys: Vec<_> = crate::widgets::tests::shapes(&out)
            .into_iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) if t.galley.text() == message => Some(t.galley),
                _ => None,
            })
            .collect();
        assert_eq!(galleys.len(), 1);
        assert!(
            !galleys[0].elided && galleys[0].rows.len() <= 2,
            "{} rows, cut {}",
            galleys[0].rows.len(),
            galleys[0].elided
        );
        assert_eq!(app.config.endpoint, "https://connected.example");
        assert_eq!(
            app.shared.generation.load(Ordering::Acquire),
            0,
            "nothing saved"
        );
    }

    #[test]
    fn saving_an_invalid_config_shows_the_error_and_does_not_reconnect() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let (mut app, ctx, out) = settings_open_on(SettingsTab::General);
        // Inválida antes do clique: nada chega ao Keychain nem ao disco.
        app.draft.endpoint = "  não é uma URL  ".into();
        let save = widget_rect(&out, Role::Button, "Salvar e conectar");
        click_at(&mut app, &ctx, size, save.center());
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(app.settings_message, "URL do servidor inválida.");
        widget_rect(&out, Role::Label, "URL do servidor inválida.");
        assert_eq!(
            app.shared.generation.load(Ordering::Acquire),
            0,
            "no reconnect"
        );
        assert_eq!(app.config.endpoint, "https://connected.example");
        assert_eq!(app.draft.endpoint, "não é uma URL", "kept, trimmed, to fix");
        assert!(app.settings.open, "the window stays to fix it");
    }

    #[test]
    fn save_and_load_wait_while_review_login_or_admin_is_busy() {
        use egui::accesskit::Role;
        let load = format!("Carregar do {}", crate::platform::SECRET_STORE);
        for busy in ["", "management", "authentication", "operations"] {
            let (mut app, ctx, _) = settings_open_on(SettingsTab::Connection);
            match busy {
                "management" => app.management.busy = true,
                "authentication" => app.authentication.busy = true,
                "operations" => app.operations.busy = true,
                _ => {}
            }
            let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
            let disabled = |name: &str| update_node(&out, Role::Button, name).is_disabled();
            assert_eq!(disabled("Salvar e conectar"), !busy.is_empty(), "{busy:?}");
            assert_eq!(disabled(&load), !busy.is_empty(), "{busy:?}");
            assert!(
                !disabled("Restaurar padrões"),
                "{busy:?}: restoring only touches the draft"
            );
            app.settings.tab = SettingsTab::Account;
            let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
            assert_eq!(
                update_node(&out, Role::Button, "Entrar com sessão humana").is_disabled(),
                !busy.is_empty(),
                "{busy:?}: the login"
            );
        }
    }

    #[test]
    fn restore_defaults_resets_only_the_draft() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let (mut app, ctx, out) = settings_open_on(SettingsTab::Connection);
        let restore = widget_rect(&out, Role::Button, "Restaurar padrões");
        click_at(&mut app, &ctx, size, restore.center());
        assert_eq!(app.draft.endpoint, Config::default().endpoint);
        assert_eq!(app.draft.container, "ai-memory");
        assert_eq!(
            app.settings_message,
            "Campos restaurados ao padrão. Clique em Salvar e conectar para aplicar."
        );
        assert_eq!(app.config.endpoint, "https://connected.example");
        assert_eq!(app.shared.generation.load(Ordering::Acquire), 0);
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(
            update_node(&out, Role::TextInput, "Servidor ai-memory").value(),
            Some(Config::default().endpoint.as_str())
        );
    }

    thread_local! {
        static SAVED_THEMES: std::cell::RefCell<Vec<Appearance>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    #[test]
    fn a_theme_card_applies_at_once_saves_only_the_theme_and_does_not_reconnect() {
        use egui::accesskit::{Role, Toggled};
        let size = [1320., 860.];
        let (mut app, ctx, _) = settings_open_on(SettingsTab::General);
        app.save_theme = |theme| {
            SAVED_THEMES.with(|saved| saved.borrow_mut().push(theme));
            Ok(())
        };
        assert_eq!(
            ctx.options(|o| o.theme_preference),
            egui::ThemePreference::System,
            "precondition"
        );
        assert!(ctx.style().visuals.dark_mode, "precondition: dark");
        for (label, choice, dark) in [
            ("Claro", Appearance::Light, false),
            ("Escuro", Appearance::Dark, true),
        ] {
            let out = frame(&mut app, &ctx, size, vec![]);
            let card = widget_rect(&out, Role::RadioButton, label);
            click_at(&mut app, &ctx, size, card.center());
            assert_eq!(app.config.theme, choice);
            assert_eq!(app.draft.theme, choice);
            assert_eq!(app.shared.config.read().unwrap().theme, choice);
            // O próximo `update` aplica antes de desenhar.
            app.apply_theme(&ctx);
            let out = frame(&mut app, &ctx, size, vec![]);
            assert_eq!(ctx.style().visuals.dark_mode, dark, "{label}");
            for other in ["Sistema", "Claro", "Escuro"] {
                assert_eq!(
                    update_node(&out, Role::RadioButton, other).toggled(),
                    Some(if other == label {
                        Toggled::True
                    } else {
                        Toggled::False
                    }),
                    "{label}: {other}"
                );
            }
        }
        // De novo o escolhido: nada a gravar.
        let out = frame(&mut app, &ctx, size, vec![]);
        let card = widget_rect(&out, Role::RadioButton, "Escuro");
        click_at(&mut app, &ctx, size, card.center());
        assert_eq!(
            SAVED_THEMES.with(|saved| saved.borrow().clone()),
            [Appearance::Light, Appearance::Dark]
        );
        assert_eq!(app.shared.generation.load(Ordering::Acquire), 0);
        assert_eq!(app.draft.endpoint, "https://draft.example");
        assert_eq!(app.config.endpoint, "https://connected.example");
    }

    thread_local! {
        static SAVED_CONFIGS: std::cell::RefCell<Vec<Config>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    #[test]
    fn the_bar_options_change_only_the_draft_until_saved_and_the_bar_reads_the_saved_ones() {
        use egui::accesskit::{Role, Toggled};
        let size = [1320., 860.];
        let dot = "Ponto âmbar no ícone quando a saúde tiver alertas";
        let (mut app, ctx, out) = settings_open_on(SettingsTab::Tray);
        app.save_config = |config| {
            SAVED_CONFIGS.with(|saved| saved.borrow_mut().push(config.clone()));
            Ok(())
        };
        assert_eq!(
            update_node(&out, Role::CheckBox, dot).toggled(),
            Some(Toggled::True)
        );
        // macOS: os três modos, o padrão marcado; escolher Completo.
        #[cfg(target_os = "macos")]
        {
            let modes = ["Só o ícone", "Ícone + CPU", "Completo"];
            let state = |out: &egui::FullOutput| {
                modes.map(|m| update_node(out, Role::RadioButton, m).toggled())
            };
            let (on, off) = (Some(Toggled::True), Some(Toggled::False));
            assert_eq!(state(&out), [off, on, off], "the default");
            for (mode, want, marks) in [
                ("Completo", BarMode::Full, [off, off, on]),
                ("Só o ícone", BarMode::Icon, [on, off, off]),
                ("Completo", BarMode::Full, [off, off, on]),
            ] {
                let at = widget_rect(
                    &frame(&mut app, &ctx, size, vec![]),
                    Role::RadioButton,
                    mode,
                );
                click_at(&mut app, &ctx, size, at.center());
                assert_eq!(app.draft.bar_mode, want, "{mode}");
                assert_eq!(state(&frame(&mut app, &ctx, size, vec![])), marks, "{mode}");
            }
        }
        let at = widget_rect(&frame(&mut app, &ctx, size, vec![]), Role::CheckBox, dot);
        click_at(&mut app, &ctx, size, at.center());
        assert!(!app.draft.alert_dot);
        let out = frame(&mut app, &ctx, size, vec![]);
        assert_eq!(
            update_node(&out, Role::CheckBox, dot).toggled(),
            Some(Toggled::False)
        );
        let mode = if cfg!(target_os = "macos") {
            BarMode::Full
        } else {
            BarMode::IconCpu
        };
        // Nada valeu antes de salvar: nem a config nem a da barra.
        assert!(app.config.alert_dot && app.config.bar_mode == BarMode::IconCpu);
        let bar = |app: &MonitorApp| {
            let c = app.shared.config.read().unwrap();
            (c.bar_mode, c.alert_dot)
        };
        assert_eq!(bar(&app), (BarMode::IconCpu, true));
        assert!(SAVED_CONFIGS.with(|saved| saved.borrow().is_empty()));
        let save = widget_rect(&out, Role::Button, "Salvar e conectar");
        click_at(&mut app, &ctx, size, save.center());
        let saved = SAVED_CONFIGS.with(|saved| saved.borrow().clone());
        assert_eq!(saved.len(), 1, "{}", app.settings_message);
        assert_eq!((saved[0].bar_mode, saved[0].alert_dot), (mode, false));
        assert_eq!(
            saved[0].endpoint, "https://draft.example",
            "the whole draft"
        );
        // A barra e a bandeja releem `shared.config` a cada 1 s.
        assert_eq!(bar(&app), (mode, false));
        assert_eq!((app.config.bar_mode, app.config.alert_dot), (mode, false));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn each_bar_mode_previews_the_text_the_bar_would_show() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let modes = ["Só o ícone", "Ícone + CPU", "Completo"];
        // O texto pintado em cada linha de modo, fora o nome dele.
        let previews = |app: &mut MonitorApp, ctx: &egui::Context| {
            let out = frame(app, ctx, size, vec![]);
            modes.map(|mode| {
                let row = widget_rect(&out, Role::RadioButton, mode);
                crate::widgets::tests::shapes(&out)
                    .into_iter()
                    .filter_map(|s| match s {
                        egui::Shape::Text(t) if row.contains(t.pos) && t.galley.text() != mode => {
                            Some(t.galley.text().to_owned())
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
        };
        let (mut app, ctx, _) = settings_open_on(SettingsTab::Tray);
        assert_eq!(
            previews(&mut app, &ctx),
            ["", "—", "AI — | Docker —"],
            "no data"
        );
        let mib = 1 << 20;
        let usage = |cpu, memory| Usage {
            cpu,
            memory,
            ..Default::default()
        };
        app.resources = ResourceSnapshot {
            at: Some(Utc::now()),
            containers: vec![
                Container {
                    name: "ai-memory".into(),
                    state: "running".into(),
                    usage: Some(usage(12.4, 421 * mib)),
                    ..Default::default()
                },
                Container {
                    name: "db".into(),
                    state: "running".into(),
                    usage: Some(usage(3., 312 * mib)),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            previews(&mut app, &ctx),
            ["", "12%", "AI 12% 421M | Docker 15% 733M"],
            "fresh"
        );
    }

    #[test]
    fn the_proposals_select_says_its_choice_and_a_pick_changes_only_the_draft() {
        use egui::accesskit::{Role, Toggled};
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.api.projects = ["demo", "outro"]
            .map(|name| Project {
                workspace_name: "fixture".into(),
                project_name: name.into(),
                ..Default::default()
            })
            .to_vec();
        app.settings.tab = SettingsTab::Tray;
        app.open_settings();
        let ctx = styled();
        ctx.enable_accesskit();
        frame(&mut app, &ctx, size, vec![]);
        let names = ["Desativado", "fixture / demo", "fixture / outro"];
        let open = |app: &mut MonitorApp| {
            let out = frame(app, &ctx, size, vec![]);
            let select =
                crate::widgets::tests::rect_of(&update_node(&out, Role::ComboBox, "Propostas"));
            click_at(app, &ctx, size, select.center());
            let out = frame(app, &ctx, size, vec![]);
            let marks = names.map(|n| update_node(&out, Role::Button, n).toggled());
            (out, marks)
        };
        let (on, off) = (Some(Toggled::True), Some(Toggled::False));
        let pick = |app: &mut MonitorApp, out: &egui::FullOutput, name: &str| {
            let at = crate::widgets::tests::rect_of(&update_node(out, Role::Button, name));
            click_at(app, &ctx, size, at.center());
        };
        let value = |app: &mut MonitorApp| {
            let out = frame(app, &ctx, size, vec![]);
            update_node(&out, Role::ComboBox, "Propostas")
                .value()
                .map(str::to_owned)
        };
        assert_eq!(value(&mut app).as_deref(), Some("Desativado"));
        let (out, marks) = open(&mut app);
        assert_eq!(marks, [on, off, off]);
        pick(&mut app, &out, "fixture / outro");
        let outro = crate::client::Scope {
            workspace: "fixture".into(),
            project: "outro".into(),
        };
        assert_eq!(app.draft.proposal_watch, Some(outro));
        assert_eq!(app.config.proposal_watch, None, "only the draft");
        assert_eq!(value(&mut app).as_deref(), Some("fixture / outro"));
        let (out, marks) = open(&mut app);
        assert_eq!(marks, [off, off, on]);
        pick(&mut app, &out, "Desativado");
        assert_eq!(app.draft.proposal_watch, None);
        assert_eq!(value(&mut app).as_deref(), Some("Desativado"));
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

    /// Sessões com dados longos: nomes de projeto, títulos e contagens
    /// grandes, em três projetos, mais sessões e projetos.
    fn sessions_data() -> ApiSnapshot {
        let now = Utc::now();
        let at = |minutes: i64| (now - chrono::Duration::minutes(minutes)).to_rfc3339();
        let projects = [
            "api-pagamentos-com-um-nome-de-projeto-bem-longo",
            "portal-web",
            "notas",
        ];
        ApiSnapshot {
            at: Some(now),
            handoff_total: Some(1234),
            handoffs: (0..30)
                .map(|i| Handoff {
                    id: format!("01a19ebd-3267-4c1e-9a7e-0f31b6b467{i:02}"),
                    agent: if i % 3 == 0 { "codex" } else { "claude-code" }.into(),
                    at: at(i * 7),
                    state: "open".into(),
                    summary: Some(if i == 0 {
                        "Um título bem comprido que não cabe na linha da lista estreita de jeito nenhum".into()
                    } else {
                        format!("## Handoff {i}\n\nCorpo do handoff {i}.")
                    }),
                    next_steps: Some(vec!["Primeiro passo".into(), "Segundo passo".into()]),
                    scope: format!("acme / {}", projects[i as usize % 3]),
                    ..Default::default()
                })
                .collect(),
            sessions: (0..12)
                .map(|i| Session {
                    session_id: format!("sessao-{i:04}-0000-4000-8000-000000000000"),
                    agent_kind: "claude-code".into(),
                    started_at: at(i * 30),
                    ended_at: (i % 4 == 0).then(|| at(i)),
                    cwd: Some("/tmp/projeto".into()),
                    observation_count: 1_234_567,
                    scope: format!("acme / {}-{i}", projects[0]),
                    ..Default::default()
                })
                .collect(),
            projects: (0..5)
                .map(|i| Project {
                    workspace_name: "acme".into(),
                    project_name: format!("{}-{i}", projects[0]),
                    page_count: 1234,
                    open_session_count: Some(12),
                    observation_count: Some(9_876_543),
                    last_activity: Some(at(5)),
                    ..Default::default()
                })
                .collect(),
            warnings: vec!["acme / notas: lista limitada aos 200 handoffs mais recentes.".into()],
            ..Default::default()
        }
    }
    /// Retângulos dos nós da página (fora da lateral e das barras).
    fn page_widgets(out: &egui::FullOutput, side: f32, height: f32) -> Vec<(String, egui::Rect)> {
        use egui::accesskit::Role;
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        update
            .nodes
            .iter()
            .filter(|(_, n)| {
                matches!(
                    n.role(),
                    Role::Label
                        | Role::Button
                        | Role::CheckBox
                        | Role::ComboBox
                        | Role::TextInput
                        | Role::Link
                        | Role::Pane
                )
            })
            .filter_map(|(_, n)| {
                let b = n.bounds()?;
                let rect = egui::Rect::from_min_max(
                    egui::pos2(b.x0 as f32, b.y0 as f32),
                    egui::pos2(b.x1 as f32, b.y1 as f32),
                );
                let name = n.label().or(n.value()).unwrap_or_default().to_owned();
                (rect.left() >= side && rect.top() >= 52. && rect.bottom() <= height - 28.)
                    .then_some((name, rect))
            })
            .collect()
    }

    #[test]
    fn sessions_page_fits_720_without_sideways_scrolling() {
        use crate::sessions_ui::Tab;
        use egui::accesskit::Role;
        let size = [720., 560.];
        for (tab, mounted) in [
            (Tab::Handoffs, (Role::Pane, "Lista de handoffs")),
            (Tab::Sessions, (Role::CheckBox, "Só abertas")),
            (Tab::Projects, (Role::Label, "Atividade")),
        ] {
            let mut app = app_with_unsaved_draft();
            app.page = Page::Sessions;
            app.api = sessions_data();
            app.sessions.set_tab(tab);
            let ctx = styled();
            ctx.enable_accesskit();
            let out = frame(&mut app, &ctx, size, vec![]);
            let side = sidebar_width(&ctx);
            let (left, right) = (side + 24., size[0] - 24.);
            let check = |out: &egui::FullOutput| {
                let found = page_widgets(out, side, size[1]);
                assert!(found.len() > 10, "{tab:?}: the page mounted: {found:?}");
                for (name, rect) in &found {
                    assert!(
                        rect.left() >= left - 0.5 && rect.right() <= right + 0.5,
                        "{tab:?}: {name:?} {rect:?} leaves the content ({left}..{right})"
                    );
                }
                widget_rect(out, mounted.0, mounted.1)
            };
            let before = check(&out);
            // A roda para o lado não move nada: não há o que rolar.
            let pointer = egui::Event::PointerMoved(before.center());
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
            let mut out = frame(&mut app, &ctx, size, vec![pointer.clone()]);
            for _ in 0..30 {
                out = frame(&mut app, &ctx, size, vec![pointer.clone()]);
            }
            assert_eq!(check(&out), before, "{tab:?}: nothing scrolled sideways");
            // A nota de somente leitura cabe numa linha (ao lado ou abaixo).
            let note = widget_rect(
                &out,
                Role::Label,
                "Somente leitura · nenhum handoff é aceito por esta tela",
            );
            assert!(note.height() < 20., "{tab:?}: one line, {note:?}");
            // A legenda é a última coisa e fica acima da barra de status.
            let legend = match tab {
                Tab::Handoffs => "Lista limitada a 200 por projeto",
                Tab::Sessions => {
                    "Últimas 100 sessões por projeto. Aberta = sem encerramento registrado; não comprova atividade atual. Vínculos entre projetos podem se repetir."
                }
                Tab::Projects => {
                    "Atividade = último evento ou atualização informado pela API. Contagens por projeto podem compartilhar sessões."
                }
            };
            let legend = widget_rect(&out, Role::Label, legend);
            assert!(
                legend.bottom() <= size[1] - 28. - 20. + 0.5,
                "{tab:?}: legend {legend:?} must sit above the status bar"
            );
        }
    }

    #[test]
    fn sessions_list_and_detail_follow_the_width_of_every_frame() {
        use egui::accesskit::Role;
        let mut app = app_with_unsaved_draft();
        app.page = Page::Sessions;
        app.api = sessions_data();
        let ctx = styled();
        ctx.enable_accesskit();
        let panes = |out: &egui::FullOutput| {
            (
                widgets(out, Role::Pane, "Lista de handoffs"),
                widgets(out, Role::Pane, "Detalhe do handoff"),
            )
        };
        let content =
            |ctx: &egui::Context, size: [f32; 2]| (sidebar_width(ctx) + 24., size[0] - 24.);
        let narrow = [720., 560.];
        let wide = [1320., 860.];
        // Aberta em 720: só a lista, na largura toda do conteúdo.
        let out = frame(&mut app, &ctx, narrow, vec![]);
        let (left, right) = content(&ctx, narrow);
        let (list, detail) = panes(&out);
        assert!(detail.is_empty(), "720 opens on the list");
        assert!(
            (list[0].left() - left).abs() < 0.5 && (list[0].right() - right).abs() < 0.5,
            "the list spans the content at 720: {:?}",
            list[0]
        );
        // Alargada no mesmo contexto: lado a lado, nada guardado de 720; a
        // lista fica em 360–420 até numa tela larga.
        for wide in [wide, wide, [1920., 1080.]] {
            let out = frame(&mut app, &ctx, wide, vec![]);
            let (left, right) = content(&ctx, wide);
            let (list, detail) = panes(&out);
            let (list, detail) = (list[0], detail[0]);
            assert!(
                (360. ..=420.).contains(&list.width()),
                "list width {}",
                list.width()
            );
            assert!((list.left() - left).abs() < 0.5, "{list:?}");
            assert!(list.right() <= detail.left(), "{list:?} / {detail:?}");
            assert!((detail.right() - right).abs() < 0.5, "{detail:?}");
            // O detalhe é o do primeiro da lista, com o pé fixo dentro dele.
            widget_rect(
                &out,
                Role::Label,
                "Um título bem comprido que não cabe na linha da lista estreita de jeito nenhum",
            );
            let review = widget_rect(&out, Role::Link, "Gerenciar em Revisão →");
            assert!(
                detail.contains_rect(review),
                "{review:?} pinned in {detail:?}"
            );
        }
        // De volta a 720: a lista outra vez.
        let out = frame(&mut app, &ctx, narrow, vec![]);
        let (list, detail) = panes(&out);
        assert!(detail.is_empty() && list.len() == 1);
        // Um clique numa linha abre o detalhe no lugar da lista, com "← Voltar".
        let row = widget_rect(&out, Role::Button, "Handoff 3 · codex · 21 min");
        click_at(&mut app, &ctx, narrow, row.center());
        let out = frame(&mut app, &ctx, narrow, vec![]);
        let (list, detail) = panes(&out);
        let (left, right) = content(&ctx, narrow);
        assert!(list.is_empty(), "the detail replaces the list");
        assert!(
            (detail[0].left() - left).abs() < 0.5 && (detail[0].right() - right).abs() < 0.5,
            "{:?}",
            detail[0]
        );
        widget_rect(&out, Role::Label, "Handoff 3");
        for (name, rect) in page_widgets(&out, sidebar_width(&ctx), narrow[1]) {
            assert!(
                rect.left() >= left - 0.5 && rect.right() <= right + 0.5,
                "{name:?} {rect:?} leaves the detail at 720"
            );
        }
        // Em 720 × 560 o pé vai ao fim do texto: rolando o detalhe, ele
        // aparece dentro do painel.
        let review = widget_rect(&out, Role::Link, "Gerenciar em Revisão →");
        assert!(
            review.top() > detail[0].bottom(),
            "precondition: the footer is below the fold at 560: {review:?}"
        );
        let pointer = egui::Event::PointerMoved(detail[0].center());
        frame(&mut app, &ctx, narrow, vec![pointer.clone()]);
        frame(
            &mut app,
            &ctx,
            narrow,
            vec![
                pointer.clone(),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0., -2000.),
                    modifiers: Default::default(),
                },
            ],
        );
        let mut out = frame(&mut app, &ctx, narrow, vec![pointer.clone()]);
        for _ in 0..30 {
            out = frame(&mut app, &ctx, narrow, vec![pointer.clone()]);
        }
        let review = widget_rect(&out, Role::Link, "Gerenciar em Revisão →");
        assert!(
            detail[0].contains_rect(review),
            "{review:?} in {:?}",
            detail[0]
        );
        // Alargada com o detalhe aberto: os dois, com o mesmo handoff.
        let out = frame(&mut app, &ctx, wide, vec![]);
        let (list, detail) = panes(&out);
        assert_eq!((list.len(), detail.len()), (1, 1));
        widget_rect(&out, Role::Label, "Handoff 3");
        let out = frame(&mut app, &ctx, narrow, vec![]);
        let back = widget_rect(&out, Role::Button, "← Voltar");
        click_at(&mut app, &ctx, narrow, back.center());
        let out = frame(&mut app, &ctx, narrow, vec![]);
        let (list, detail) = panes(&out);
        assert!(detail.is_empty() && list.len() == 1, "back to the list");
        assert_eq!(
            update_node(&out, Role::Button, "Handoff 3 · codex · 21 min").toggled(),
            Some(egui::accesskit::Toggled::True),
            "the selection is kept"
        );
    }

    #[test]
    fn manage_in_review_opens_the_handoffs_tab_unless_the_panel_is_held() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let held = "Operação em andamento em Manutenção e backup · Manutenção. Aguarde a resposta lá; esta página usa o mesmo painel.";
        // O handoff no detalhe de Sessões e o projeto dele.
        let scope = "acme / api-pagamentos-com-um-nome-de-projeto-bem-longo";
        let id = "01a19ebd-3267-4c1e-9a7e-0f31b6b46700";
        // (aba antes, ocupado?, projeto na lista?, aba depois, aviso na
        // Revisão?, projeto e handoff escolhidos depois)
        for (tab, busy, listed, want, warned, picked) in [
            (0, false, true, 1, false, (Some(scope), id)),
            (1, false, true, 1, false, (Some(scope), id)),
            (2, false, true, 1, false, (Some(scope), id)),
            (3, false, true, 1, false, (Some(scope), id)),
            (0, false, false, 1, false, (None, "")),
            (0, true, true, 0, false, (None, "")),
            (2, true, true, 2, true, (None, "")),
        ] {
            let mut app = app_with_unsaved_draft();
            app.page = Page::Sessions;
            app.api = sessions_data();
            if listed {
                app.api.projects.push(Project {
                    workspace_name: "acme".into(),
                    project_name: "api-pagamentos-com-um-nome-de-projeto-bem-longo".into(),
                    ..Default::default()
                });
            }
            app.management.set_tab(tab);
            app.management.busy = busy;
            let ctx = styled();
            ctx.enable_accesskit();
            let out = frame(&mut app, &ctx, size, vec![]);
            let link = widget_rect(&out, Role::Link, "Gerenciar em Revisão →");
            click_at(&mut app, &ctx, size, link.center());
            assert_eq!(app.page, Page::Review, "tab {tab} busy {busy}");
            let out = frame(&mut app, &ctx, size, vec![]);
            assert_eq!(app.management.tab(), want, "tab {tab} busy {busy}");
            assert_eq!(
                widgets(&out, Role::Label, held).len(),
                usize::from(warned),
                "tab {tab} busy {busy}"
            );
            let (project, handoff) = app.management.picked();
            assert_eq!(
                (project.as_deref(), handoff),
                picked,
                "tab {tab} busy {busy} listed {listed}"
            );
        }
    }

    #[test]
    fn reconnect_clears_the_sessions_filters_and_selection() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.page = Page::Sessions;
        app.api = sessions_data();
        let ctx = styled();
        ctx.enable_accesskit();
        let out = frame(&mut app, &ctx, size, vec![]);
        let row = widget_rect(&out, Role::Button, "Handoff 3 · codex · 21 min");
        click_at(&mut app, &ctx, size, row.center());
        let scope = "acme / api-pagamentos-com-um-nome-de-projeto-bem-longo";
        app.sessions.set_filters(scope, "codex");
        let out = frame(&mut app, &ctx, size, vec![]);
        update_node(&out, Role::ComboBox, "Agente: codex");
        update_node(&out, Role::ComboBox, scope);
        assert_eq!(
            app.sessions.selected(),
            Some(&(
                scope.to_owned(),
                "01a19ebd-3267-4c1e-9a7e-0f31b6b46703".to_owned()
            )),
            "precondition: filtered, with a chosen handoff"
        );
        app.reconnect(app.config.clone());
        assert_eq!(app.sessions.selected(), None);
        app.api = sessions_data();
        let out = frame(&mut app, &ctx, size, vec![]);
        update_node(&out, Role::ComboBox, "Todos os projetos");
        update_node(&out, Role::ComboBox, "Agente: todos");
        assert_eq!(
            update_node(&out, Role::Button, "Um título bem comprido que não cabe na linha da lista estreita de jeito nenhum · codex · agora").toggled(),
            Some(egui::accesskit::Toggled::True),
            "a new connection opens on the first again"
        );
    }

    /// Docker com dados longos: oito contêineres (três em execução), um nome
    /// e uma imagem compridos, imagens, processos e o contexto.
    fn docker_data() -> ResourceSnapshot {
        let now = Utc::now();
        let container = |name: &str, state: &str, cpu: Option<f64>, ports: &str| Container {
            id: format!("{name}-{}", "8f3c21a9d0e4".repeat(5)),
            name: name.into(),
            image: format!("ghcr.io/organizacao/{name}:2026.11.03"),
            state: state.into(),
            status: "Up 3 days".into(),
            ports: ports.into(),
            usage: cpu.map(|cpu| Usage {
                cpu,
                memory: 1_234_567_890,
                limit: 8 << 30,
                tasks: Some(1234),
            }),
            net_io: "1.2GB / 840MB".into(),
            block_io: "3.4GB / 9.1GB".into(),
            ..Default::default()
        };
        ResourceSnapshot {
            at: Some(now),
            containers: vec![
                container(
                    "ai-memory",
                    "running",
                    Some(123.4),
                    "0.0.0.0:49374->49374/tcp, [::]:49374->49374/tcp",
                ),
                container(
                    "postgres-dev",
                    "running",
                    Some(2.7),
                    "0.0.0.0:5432->5432/tcp",
                ),
                container("redis-cache", "running", Some(0.4), "6379/tcp"),
                container(
                    "um-conteiner-com-um-nome-comprido-demais-para-a-coluna",
                    "exited",
                    None,
                    "",
                ),
                container("minio", "exited", None, ""),
                container("grafana", "paused", None, "3000/tcp"),
                container("loki", "created", None, ""),
                container("tempo", "dead", None, ""),
            ],
            images: (0..40)
                .map(|i| DockerImage {
                    id: format!("sha256:{i:064x}"),
                    tags: vec![format!("ghcr.io/organizacao/imagem-{i}:latest")],
                    size: 1_234_567_890,
                    containers: vec!["ai-memory".into()],
                    running: 1,
                })
                .collect(),
            images_at: Some(now - chrono::Duration::seconds(9)),
            processes: (0..30)
                .map(|i| HostProcess {
                    pid: 10_000 + i,
                    name: format!("com.docker.virtualization-processo-{i}"),
                    group: "Docker Desktop".into(),
                    usage: Usage {
                        cpu: 123.4,
                        memory: 1_234_567_890,
                        tasks: Some(1234),
                        ..Default::default()
                    },
                })
                .collect(),
            ai_processes: vec![("1".into(), "ai-memory".into(), 41)],
            docker_context: Some("desktop-linux".into()),
            ..Default::default()
        }
    }
    /// Roda a roda sobre `pos` e espera a animação; devolve o último frame.
    fn wheel(
        app: &mut MonitorApp,
        ctx: &egui::Context,
        size: [f32; 2],
        pos: egui::Pos2,
        dy: f32,
    ) -> egui::FullOutput {
        let pointer = egui::Event::PointerMoved(pos);
        frame(app, ctx, size, vec![pointer.clone()]);
        frame(
            app,
            ctx,
            size,
            vec![
                pointer.clone(),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0., dy),
                    modifiers: Default::default(),
                },
            ],
        );
        let mut out = frame(app, ctx, size, vec![pointer.clone()]);
        for _ in 0..30 {
            out = frame(app, ctx, size, vec![pointer.clone()]);
        }
        out
    }
    /// Linhas da tabela de contêineres, de cima para baixo.
    fn container_rows(out: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        let mut rows: Vec<(String, egui::Rect)> = update
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.role() == egui::accesskit::Role::Button
                    && n.label().is_some_and(|l| l.contains(" · CPU "))
            })
            .map(|(_, n)| {
                let b = n.bounds().unwrap();
                (
                    n.label().unwrap().split(" · ").next().unwrap().to_owned(),
                    egui::Rect::from_min_max(
                        egui::pos2(b.x0 as f32, b.y0 as f32),
                        egui::pos2(b.x1 as f32, b.y1 as f32),
                    ),
                )
            })
            .collect();
        rows.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
        rows
    }

    #[test]
    fn docker_page_fits_720_and_1320_in_both_themes_without_overlap() {
        use crate::docker_ui::Tab;
        use egui::accesskit::Role;
        let host = format!("Processos do {}", crate::platform::HOST);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            for size in [[720., 560.], [1320., 860.]] {
                for tab in [Tab::Containers, Tab::Images, Tab::Processes] {
                    let mut app = app_with_unsaved_draft();
                    app.page = Page::Docker;
                    app.config.container = "ai-memory".into();
                    app.resources = docker_data();
                    app.docker.set_tab(tab);
                    let ctx = styled();
                    ctx.set_theme(theme);
                    ctx.enable_accesskit();
                    let out = frame(&mut app, &ctx, size, vec![]);
                    let side = sidebar_width(&ctx);
                    let (left, right) = (side + 24., size[0] - 24.);
                    let case = format!("{theme:?} {size:?} {tab:?}");
                    let found = page_widgets(&out, side, size[1]);
                    assert!(found.len() > 10, "{case}: the page mounted: {found:?}");
                    for (name, rect) in &found {
                        assert!(
                            rect.left() >= left - 0.5 && rect.right() <= right + 0.5,
                            "{case}: {name:?} {rect:?} leaves the content ({left}..{right})"
                        );
                    }
                    // Linha de ferramentas: abas e busca lado a lado, sem se tocar.
                    let tabs = widget_rect(&out, Role::Button, &host);
                    let search = update_node(&out, Role::TextInput, "")
                        .bounds()
                        .map(|b| {
                            egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            )
                        })
                        .unwrap();
                    assert!(
                        tabs.right() < search.left() && search.right() <= right + 0.5,
                        "{case}: {tabs:?} / {search:?}"
                    );
                    // A legenda é a última coisa e fica acima da barra de status.
                    let legend = widget_rect(&out, Role::Label, "Imagens: inventário há 9 s");
                    assert!(
                        legend.bottom() <= size[1] - 28. - 20. + 0.5,
                        "{case}: legend {legend:?} must sit above the status bar"
                    );
                    if tab != Tab::Containers {
                        continue;
                    }
                    let switch = widget_rect(&out, Role::CheckBox, "Só em execução");
                    let summary = widget_rect(&out, Role::Label, "Tarefas 3.702");
                    assert!(
                        summary.right() < switch.left()
                            && (summary.center().y - switch.center().y).abs() < 4.,
                        "{case}: one summary line: {summary:?} / {switch:?}"
                    );
                    let table = widget_rect(&out, Role::Pane, "Contêineres");
                    assert!(table.top() > switch.bottom(), "{case}: {table:?}");
                    let rows = container_rows(&out);
                    assert_eq!(rows[0].0, "ai-memory", "{case}");
                    assert!(rows.windows(2).all(|w| w[0].1.bottom() <= w[1].1.top()));
                    let inspector = widgets(&out, Role::Pane, "Inspetor do contêiner")
                        .first()
                        .copied();
                    if size[0] == 1320. {
                        let inspector = inspector.expect("beside at 1320");
                        assert_eq!(inspector.width(), 320., "{case}");
                        assert!((inspector.right() - right).abs() < 0.5, "{case}");
                        assert_eq!(table.right() + 1., inspector.left(), "{case}");
                        widget_rect(&out, Role::Label, "Portas");
                    } else {
                        // Só a tabela; um clique abre o inspetor no lugar dela.
                        assert!(inspector.is_none(), "{case}: no inspector below");
                        assert!(widgets(&out, Role::Label, "Portas").is_empty(), "{case}");
                        click_at(&mut app, &ctx, size, rows[0].1.center());
                        let out = frame(&mut app, &ctx, size, vec![]);
                        assert!(
                            widgets(&out, Role::Pane, "Contêineres").is_empty(),
                            "{case}"
                        );
                        let inspector = widget_rect(&out, Role::Pane, "Inspetor do contêiner");
                        assert_eq!(inspector, table, "{case}: the table's place");
                        widget_rect(&out, Role::Button, "← Voltar");
                        // Em 720 × 560 os botões ficam abaixo da dobra: rolando
                        // o inspetor, aparecem inteiros dentro dele.
                        let out = wheel(&mut app, &ctx, size, inspector.center(), -3000.);
                        let copy = widget_rect(&out, Role::Button, "Copiar nome");
                        assert!(
                            inspector.contains_rect(copy),
                            "{case}: scrolled into view: {copy:?} in {inspector:?}"
                        );
                        for (name, rect) in page_widgets(&out, side, size[1]) {
                            assert!(
                                rect.left() >= left - 0.5 && rect.right() <= right + 0.5,
                                "{case}: {name:?} {rect:?} leaves the inspector"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn docker_table_follows_the_width_of_every_frame() {
        use egui::accesskit::Role;
        let mut app = app_with_unsaved_draft();
        app.page = Page::Docker;
        app.resources = docker_data();
        let ctx = styled();
        ctx.enable_accesskit();
        let content =
            |ctx: &egui::Context, size: [f32; 2]| (sidebar_width(ctx) + 24., size[0] - 24.);
        let narrow = [720., 560.];
        // (tamanho, inspetor ao lado?) — no mesmo contexto, nada guardado.
        for (size, beside) in [
            (narrow, false),
            ([1320., 860.], true),
            ([1920., 1080.], true),
            (narrow, false),
            ([1320., 860.], true),
        ] {
            let out = frame(&mut app, &ctx, size, vec![]);
            let (left, right) = content(&ctx, size);
            let table = widget_rect(&out, Role::Pane, "Contêineres");
            let want = if beside { right - 321. } else { right };
            assert!(
                (table.left() - left).abs() < 0.5 && (table.right() - want).abs() < 0.5,
                "{size:?}: table {table:?} must span {left}..{want}"
            );
            let rows = container_rows(&out);
            assert!(
                rows.iter()
                    .all(|(_, r)| (r.left() - left).abs() < 0.5 && (r.right() - want).abs() < 0.5),
                "{size:?}: rows follow the table: {rows:?}"
            );
            let inspector = widgets(&out, Role::Pane, "Inspetor do contêiner");
            assert_eq!(inspector.len(), usize::from(beside), "{size:?}");
            assert!(
                inspector.iter().all(|i| i.left() > table.left()),
                "{size:?}"
            );
        }
        // Aberto em 720 (um clique), segue aberto ao alargar e ao voltar:
        // ao lado em 1320, no lugar da tabela em 720; "← Voltar" volta.
        let out = frame(&mut app, &ctx, narrow, vec![]);
        let row = container_rows(&out)
            .into_iter()
            .find(|(n, _)| n == "postgres-dev")
            .unwrap()
            .1;
        click_at(&mut app, &ctx, narrow, row.center());
        for (size, beside) in [(narrow, false), ([1320., 860.], true), (narrow, false)] {
            let out = frame(&mut app, &ctx, size, vec![]);
            let (left, right) = content(&ctx, size);
            let inspector = widget_rect(&out, Role::Pane, "Inspetor do contêiner");
            let name = widget_rect(&out, Role::Label, "postgres-dev");
            assert!(inspector.contains_rect(name), "{size:?}: {name:?}");
            assert_eq!(
                widgets(&out, Role::Pane, "Contêineres").len(),
                usize::from(beside),
                "{size:?}"
            );
            let want_left = if beside { right - 320. } else { left };
            assert!(
                (inspector.left() - want_left).abs() < 0.5
                    && (inspector.right() - right).abs() < 0.5,
                "{size:?}: {inspector:?}"
            );
            assert_eq!(
                widgets(&out, Role::Button, "← Voltar").len(),
                usize::from(!beside),
                "{size:?}"
            );
        }
        let out = frame(&mut app, &ctx, narrow, vec![]);
        click_at(
            &mut app,
            &ctx,
            narrow,
            widget_rect(&out, Role::Button, "← Voltar").center(),
        );
        let out = frame(&mut app, &ctx, narrow, vec![]);
        assert!(widgets(&out, Role::Pane, "Inspetor do contêiner").is_empty());
        widget_rect(&out, Role::Pane, "Contêineres");
    }

    #[test]
    fn global_notices_are_strips_on_every_page_in_both_themes() {
        use egui::accesskit::Role;
        let docker = "Docker indisponível. Docker CLI não encontrado.";
        let api = "Sem conexão com o ai-memory. Falha ao conectar.";
        let partial = "Dados parciais: 2 aviso(s). Consulte Ajustes.";
        // (caso, erro do Docker, erro da API, avisos parciais, faixas na ordem)
        let cases: [(&str, bool, bool, usize, &[&str]); 5] = [
            ("none", false, false, 0, &[]),
            ("docker", true, false, 0, &[docker]),
            ("api", false, true, 0, &[api]),
            ("partial", false, false, 2, &[partial]),
            ("all", true, true, 2, &[docker, api, partial]),
        ];
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let p = if theme == egui::Theme::Dark {
                &theme::DARK
            } else {
                &theme::LIGHT
            };
            for (case, docker_down, api_down, warnings, want) in cases {
                for page in PAGES {
                    let mut app = app_with_unsaved_draft();
                    app.page = page;
                    app.resources.docker_error =
                        docker_down.then(|| "Docker CLI não encontrado.".into());
                    app.api.error = api_down.then(|| "Falha ao conectar.".into());
                    app.api.errors = vec!["aviso".into(); warnings];
                    let ctx = styled();
                    ctx.set_theme(theme);
                    ctx.enable_accesskit();
                    let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
                    let strips: Vec<egui::Rect> = want
                        .iter()
                        .map(|text| widget_rect(&out, Role::Label, text))
                        .collect();
                    assert!(
                        strips.windows(2).all(|w| w[0].bottom() < w[1].top()),
                        "{theme:?} {case} {page:?}: in order, apart"
                    );
                    let fills = |color: Color32| {
                        out.shapes
                            .iter()
                            .filter(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == color))
                            .count()
                    };
                    assert_eq!(
                        (
                            fills(p.bad.gamma_multiply(0.08)),
                            fills(p.warn.gamma_multiply(0.10))
                        ),
                        (
                            usize::from(docker_down) + usize::from(api_down),
                            usize::from(warnings > 0)
                        ),
                        "{theme:?} {case} {page:?}: Bad and Warn strips"
                    );
                    let open = widgets(&out, Role::Button, "Abrir Ajustes");
                    assert_eq!(open.len(), usize::from(api_down), "{case} {page:?}");
                    // Nada da página sobre as faixas.
                    let side = sidebar_width(&ctx);
                    for (name, rect) in page_widgets(&out, side, 860.) {
                        if want.contains(&name.as_str()) || name == "Abrir Ajustes" {
                            continue;
                        }
                        for strip in &strips {
                            assert!(
                                !rect.intersects(strip.shrink(0.5)),
                                "{theme:?} {case} {page:?}: {name:?} {rect:?} on a strip {strip:?}"
                            );
                        }
                    }
                }
            }
        }
        // "Abrir Ajustes" abre a janela de Ajustes, sem sair da página.
        let mut app = app_with_unsaved_draft();
        app.page = Page::Docker;
        app.api.error = Some("Falha ao conectar.".into());
        let ctx = styled();
        ctx.enable_accesskit();
        let out = frame(&mut app, &ctx, [1320., 860.], vec![]);
        let open = widget_rect(&out, Role::Button, "Abrir Ajustes");
        click_at(&mut app, &ctx, [1320., 860.], open.center());
        assert!(app.settings.open);
        assert_eq!(app.page, Page::Docker);
    }

    #[test]
    fn navigating_away_keeps_the_docker_search() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        for away in ["Visão geral", "Explorar"] {
            let mut app = app_with_unsaved_draft();
            app.page = Page::Docker;
            app.resources = docker_data();
            *app.docker.search_mut() = "redis".into();
            let ctx = styled();
            ctx.enable_accesskit();
            let out = frame(&mut app, &ctx, size, vec![]);
            assert_eq!(
                container_rows(&out)
                    .into_iter()
                    .map(|(n, _)| n)
                    .collect::<Vec<_>>(),
                ["redis-cache"]
            );
            click_at(
                &mut app,
                &ctx,
                size,
                widget_rect(&out, Role::Button, away).center(),
            );
            assert_ne!(app.page, Page::Docker, "{away}");
            let out = frame(&mut app, &ctx, size, vec![]);
            click_at(
                &mut app,
                &ctx,
                size,
                widget_rect(&out, Role::Button, "Docker").center(),
            );
            let out = frame(&mut app, &ctx, size, vec![]);
            assert_eq!(app.page, Page::Docker);
            assert_eq!(app.docker.search_mut(), "redis", "{away}");
            update_node(&out, Role::TextInput, "redis");
            assert_eq!(container_rows(&out).len(), 1, "{away}");
        }
        // Ajustes abre a janela própria: a página e a busca ficam.
        let mut app = app_with_unsaved_draft();
        app.page = Page::Docker;
        app.resources = docker_data();
        *app.docker.search_mut() = "redis".into();
        let ctx = styled();
        ctx.enable_accesskit();
        let out = frame(&mut app, &ctx, size, vec![]);
        click_at(
            &mut app,
            &ctx,
            size,
            widget_rect(&out, Role::Button, "Ajustes").center(),
        );
        let out = frame(&mut app, &ctx, size, vec![]);
        assert!(app.settings.open);
        assert_eq!(app.page, Page::Docker);
        assert_eq!(app.docker.search_mut(), "redis");
        update_node(&out, Role::TextInput, "redis");
        assert_eq!(container_rows(&out).len(), 1);
    }

    /// A linha `name` da tabela da Visão geral ("nome · CPU …").
    fn overview_row(out: &egui::FullOutput, name: &str) -> egui::Rect {
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        let found: Vec<egui::Rect> = update
            .nodes
            .iter()
            .filter(|(_, n)| {
                n.role() == egui::accesskit::Role::Button
                    && n.label()
                        .is_some_and(|l| l.starts_with(&format!("{name} · CPU ")))
            })
            .filter_map(|(_, n)| n.bounds())
            .map(|b| {
                egui::Rect::from_min_max(
                    egui::pos2(b.x0 as f32, b.y0 as f32),
                    egui::pos2(b.x1 as f32, b.y1 as f32),
                )
            })
            .collect();
        assert_eq!(found.len(), 1, "overview row {name}");
        found[0]
    }

    #[test]
    fn a_container_clicked_in_the_overview_opens_in_the_docker_inspector() {
        use egui::accesskit::Role;
        // 1320: a tabela aparece sem rolar; 720: só depois de rolar a página.
        for (size, scroll) in [([1320., 860.], false), ([720., 560.], true)] {
            let mut app = app_with_unsaved_draft();
            app.resources = docker_data();
            app.docker.set_tab(crate::docker_ui::Tab::Images);
            *app.docker.search_mut() = "redis".into();
            let ctx = styled();
            ctx.enable_accesskit();
            let mut out = frame(&mut app, &ctx, size, vec![]);
            assert_eq!(app.page, Page::Overview);
            let bottom = size[1] - 28. - 20.;
            let row = overview_row(&out, "postgres-dev");
            assert_eq!(row.bottom() > bottom, scroll, "{size:?}: precondition");
            if scroll {
                let strip = widget_rect(&out, Role::Pane, "Saúde");
                out = wheel(&mut app, &ctx, size, strip.center(), -2000.);
            }
            let row = overview_row(&out, "postgres-dev");
            assert!(
                row.top() >= 52. && row.bottom() <= bottom + 0.5,
                "{size:?}: on screen, {row:?}"
            );
            click_at(&mut app, &ctx, size, row.center());
            assert_eq!(app.page, Page::Docker, "{size:?}");
            let out = frame(&mut app, &ctx, size, vec![]);
            assert_eq!(app.docker.search_mut(), "", "the search would hide it");
            assert_eq!(
                app.docker.selected(),
                Some(&*format!("postgres-dev-{}", "8f3c21a9d0e4".repeat(5)))
            );
            update_node(&out, Role::Button, "Contêineres 8");
            // Em 1320 ao lado da tabela; em 720 no lugar dela, com "← Voltar".
            let inspector = widget_rect(&out, Role::Pane, "Inspetor do contêiner");
            assert!(inspector.contains_rect(widget_rect(&out, Role::Label, "postgres-dev")));
            assert_eq!(
                widgets(&out, Role::Pane, "Contêineres").is_empty(),
                scroll,
                "{size:?}"
            );
            assert_eq!(
                widgets(&out, Role::Button, "← Voltar").len(),
                usize::from(scroll),
                "{size:?}"
            );
        }
    }

    /// Visão geral com Docker, API e saúde: os dados longos das outras
    /// páginas, o ai-memory monitorado e um projeto vigiado.
    fn overview_app() -> MonitorApp {
        let mut app = app_with_unsaved_draft();
        app.config.container = "ai-memory".into();
        app.resources = docker_data();
        app.api = sessions_data();
        watching(&mut app, Some(3));
        app
    }
    /// Os títulos dos quatro indicadores.
    const KPIS: [&str; 4] = [
        "ai-memory · CPU",
        "ai-memory · Memória",
        "Contêineres",
        "Continuidade",
    ];

    #[test]
    fn overview_card_values_stay_on_one_line_at_720_in_both_themes() {
        use egui::accesskit::Role;
        // (cartão, valor falado, texto pintado do valor e da unidade)
        let want = [
            ("ai-memory · CPU", "123,4 %", "123,4%"),
            ("ai-memory · Memória", "1,1 GiB", "1,1GiB"),
            ("Contêineres", "3 de 8 ativos", "3de 8 ativos"),
            (
                "Continuidade",
                "1.234 handoffs abertos",
                "1.234handoffs abertos",
            ),
        ];
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            for size in [[720., 560.], [1320., 860.]] {
                let mut app = overview_app();
                let ctx = styled();
                ctx.set_theme(theme);
                ctx.enable_accesskit();
                let out = frame(&mut app, &ctx, size, vec![]);
                let case = format!("{theme:?} {size:?}");
                for (card, spoken, painted) in want {
                    let pane = widget_rect(&out, Role::Pane, card);
                    let value = widget_rect(&out, Role::Label, spoken);
                    assert!(pane.contains_rect(value), "{case}: {spoken} in {card}");
                    let galleys: Vec<_> = out
                        .shapes
                        .iter()
                        .filter_map(|c| match &c.shape {
                            egui::Shape::Text(t)
                                if t.galley.text() == painted && pane.contains(t.pos) =>
                            {
                                Some(t.galley.clone())
                            }
                            _ => None,
                        })
                        .collect();
                    assert_eq!(galleys.len(), 1, "{case}: {painted:?} painted in {card}");
                    assert_eq!(
                        (galleys[0].rows.len(), galleys[0].elided),
                        (1, false),
                        "{case}: {painted:?} is one whole line"
                    );
                    assert!(
                        value.height() < 40.,
                        "{case}: {spoken} one row tall, {value:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_overview_has_no_host_panel_and_no_project_count() {
        use egui::accesskit::Role;
        for size in [[720., 560.], [1320., 860.]] {
            let mut app = overview_app();
            let ctx = styled();
            ctx.enable_accesskit();
            let out = frame(&mut app, &ctx, size, vec![]);
            let side = sidebar_width(&ctx);
            let page = page_widgets(&out, side, size[1]);
            // O cenário tem projetos: a barra de status os conta.
            let status = widget_rect(&out, Role::Label, &host_status(&app.resources, &app.api));
            assert!(status.top() >= size[1] - 28., "{size:?}: {status:?}");
            assert!(host_status(&app.resources, &app.api).ends_with(" · 5 projetos"));
            assert!(
                page.iter().any(|(name, _)| name == "Recursos"),
                "{size:?}: the page mounted"
            );
            let content =
                egui::Rect::from_min_max(egui::pos2(side, 52.), egui::pos2(size[0], size[1] - 28.));
            let painted: Vec<String> = out
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) if content.contains(t.pos) => {
                        Some(t.galley.text().to_owned())
                    }
                    _ => None,
                })
                .collect();
            assert!(painted.iter().any(|t| t == "Recursos"), "{size:?}");
            for text in page.iter().map(|(n, _)| n).chain(&painted) {
                let lower = text.to_lowercase();
                assert!(
                    !lower.contains("este mac")
                        && !lower.contains("este computador")
                        && !lower.contains("projetos"),
                    "{size:?}: {text:?} on the page"
                );
            }
        }
    }

    #[test]
    fn the_overview_follows_the_width_of_every_frame() {
        use egui::accesskit::Role;
        let mut app = overview_app();
        let ctx = styled();
        ctx.enable_accesskit();
        // (janela, indicadores por linha, linha de baixo lado a lado?) — no
        // mesmo contexto, nada guardado.
        for (size, per_line, beside) in [
            ([720., 560.], 2, false),
            ([1320., 860.], 4, true),
            ([1920., 1080.], 4, true),
            ([720., 560.], 2, false),
            // 1000 − 232 − 48 = 720 de conteúdo: cabem três, vão 2 + 2.
            ([1000., 700.], 2, false),
            ([1320., 860.], 4, true),
        ] {
            let out = frame(&mut app, &ctx, size, vec![]);
            let side = sidebar_width(&ctx);
            let (left, right) = (side + 24., size[0] - 24.);
            let spans =
                |r: egui::Rect| (r.left() - left).abs() < 0.5 && (r.right() - right).abs() < 0.5;
            for pane in ["Saúde", "Recursos"] {
                let rect = widget_rect(&out, Role::Pane, pane);
                assert!(
                    spans(rect),
                    "{size:?}: {pane} {rect:?} spans {left}..{right}"
                );
            }
            let kpis: Vec<egui::Rect> = KPIS
                .iter()
                .map(|t| widget_rect(&out, Role::Pane, t))
                .collect();
            let lines = crate::overview_ui::kpi_lines(right - left);
            assert_eq!(lines[0].len(), per_line, "{size:?}");
            for line in &lines {
                let row: Vec<egui::Rect> = line.iter().map(|&(i, _)| kpis[i]).collect();
                assert!((row[0].left() - left).abs() < 0.5, "{size:?}: {row:?}");
                assert!(
                    (row.last().unwrap().right() - right).abs() < 0.5,
                    "{size:?}: {row:?}"
                );
                assert!(
                    row.windows(2)
                        .all(|w| (w[1].left() - w[0].right() - 12.).abs() < 0.5
                            && (w[0].top() - w[1].top()).abs() < 0.5
                            && (w[0].height() - w[1].height()).abs() < 0.5),
                    "{size:?}: {row:?}"
                );
            }
            let table = widget_rect(&out, Role::Pane, "Contêineres em execução");
            let recent = widget_rect(&out, Role::Pane, "Handoffs recentes");
            assert_eq!(recent.top() > table.bottom(), !beside, "{size:?}");
            if beside {
                assert!((table.left() - left).abs() < 0.5 && (recent.right() - right).abs() < 0.5);
                assert!(
                    (recent.left() - table.right() - 12.).abs() < 0.5,
                    "{size:?}"
                );
                // flex 3 1 420 e 2 1 300: a sobra na proporção 3 : 2.
                let free = right - left - 12. - 720.;
                assert!(
                    (table.width() - (420. + free * 0.6)).abs() < 0.5,
                    "{size:?}"
                );
                assert_eq!(
                    table.height(),
                    recent.height(),
                    "{size:?}: same line, same height"
                );
            } else {
                assert!(
                    spans(table) && spans(recent),
                    "{size:?}: {table:?} {recent:?}"
                );
            }
            // As linhas da tabela ocupam a largura do cartão (sem a borda).
            for name in ["ai-memory", "postgres-dev", "redis-cache"] {
                let row = overview_row(&out, name);
                assert!(
                    (row.left() - (table.left() + 1.)).abs() < 0.5
                        && (row.right() - (table.right() - 1.)).abs() < 0.5,
                    "{size:?}: {name} {row:?} spans {table:?}"
                );
            }
            for (name, rect) in page_widgets(&out, side, size[1]) {
                assert!(
                    rect.left() >= left - 0.5 && rect.right() <= right + 0.5,
                    "{size:?}: {name:?} {rect:?} leaves the content ({left}..{right})"
                );
            }
        }
    }

    #[test]
    fn overview_links_open_health_docker_and_sessions() {
        use egui::accesskit::Role;
        let size = [1320., 860.];
        for (link, page) in [
            ("Ver saúde →", Page::Health),
            ("Ver todos →", Page::Docker),
            ("Abrir →", Page::Sessions),
        ] {
            let mut app = overview_app();
            *app.docker.search_mut() = "redis".into();
            let ctx = styled();
            ctx.enable_accesskit();
            let out = frame(&mut app, &ctx, size, vec![]);
            click_at(
                &mut app,
                &ctx,
                size,
                widget_rect(&out, Role::Link, link).center(),
            );
            assert_eq!(app.page, page, "{link}");
            // "Ver todos" não escolhe contêiner nem limpa a busca.
            assert_eq!(app.docker.search_mut(), "redis", "{link}");
            assert_eq!(app.docker.selected(), None, "{link}");
        }
    }

    #[test]
    fn reconnect_clears_the_docker_selection_but_keeps_the_search() {
        let size = [1320., 860.];
        let mut app = app_with_unsaved_draft();
        app.page = Page::Docker;
        // O monitorado não é o primeiro da lista (ai-memory tem mais CPU).
        app.config.container = "redis-cache".into();
        app.resources = docker_data();
        let ctx = styled();
        ctx.enable_accesskit();
        let out = frame(&mut app, &ctx, size, vec![]);
        let row = container_rows(&out)
            .into_iter()
            .find(|(n, _)| n == "minio")
            .unwrap()
            .1;
        click_at(&mut app, &ctx, size, row.center());
        *app.docker.search_mut() = "i".into();
        frame(&mut app, &ctx, size, vec![]);
        assert!(
            app.docker
                .selected()
                .is_some_and(|id| id.starts_with("minio-")),
            "precondition: minio chosen"
        );
        app.reconnect(app.config.clone());
        assert_eq!(app.docker.selected(), None);
        assert_eq!(app.docker.search_mut(), "i");
        app.resources = docker_data();
        frame(&mut app, &ctx, size, vec![]);
        assert!(
            app.docker
                .selected()
                .is_some_and(|id| id.starts_with("redis-cache-")),
            "a new connection opens on the monitored container, not on the first"
        );
    }
}
