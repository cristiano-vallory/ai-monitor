//! Linux runtime: tray icon + janela sob demanda.
//! Wayland não esconde janelas, então cada abertura cria uma janela nova e o
//! `eframe::run_native` retorna quando ela fecha; coletores e a bandeja seguem
//! vivos o tempo todo.

use crate::{
    app::MonitorApp,
    commands::{Action, Command},
    model::Config,
    monitor::Shared,
};
use eframe::egui;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
};

const BUS_NAME: &str = "io.github.cristiano_vallory.AiMonitor";
const OBJECT_PATH: &str = "/io/github/cristiano_vallory/AiMonitor";
const INTERFACE: &str = "io.github.cristiano_vallory.AiMonitor1";

/// Everything a producer needs to reach the app: enqueue a command and wake
/// the live egui Context if a window is open right now.
pub struct Control {
    tx: Sender<Command>,
    rx: Mutex<Receiver<Command>>,
    ctx: Mutex<Option<egui::Context>>,
    window_open: AtomicBool,
}
impl Control {
    pub(crate) fn new() -> Arc<Self> {
        let (tx, rx) = channel();
        Arc::new(Self {
            tx,
            rx: Mutex::new(rx),
            ctx: Mutex::new(None),
            window_open: AtomicBool::new(false),
        })
    }
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
        if let Some(ctx) = self.ctx.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
    }
    pub(crate) fn try_recv(&self) -> Option<Command> {
        self.rx.lock().unwrap().try_recv().ok()
    }
    fn wait(&self) -> Option<Command> {
        self.rx.lock().unwrap().recv().ok()
    }
    pub fn window_open(&self) -> bool {
        self.window_open.load(Ordering::Acquire)
    }
}

struct ShowApi {
    control: Arc<Control>,
}
#[zbus::interface(name = "io.github.cristiano_vallory.AiMonitor1")]
impl ShowApi {
    fn show(&self) {
        self.control.send(Command::Open(0));
    }
}

enum SingleInstance {
    Owner(zbus::blocking::Connection),
    /// Outra instância avisada via Show; este processo deve sair.
    Forwarded(bool),
    NoBus,
}

use crate::commands::NameReply;
fn single_instance(control: &Arc<Control>) -> SingleInstance {
    let Ok(conn) = zbus::blocking::connection::Builder::session()
        .map_err(|_| ())
        .and_then(|b| {
            b.method_timeout(std::time::Duration::from_secs(5))
                .build()
                .map_err(|_| ())
        })
    else {
        return SingleInstance::NoBus;
    };
    if let Err(e) = conn.object_server().at(
        OBJECT_PATH,
        ShowApi {
            control: control.clone(),
        },
    ) {
        eprintln!("AI Monitor: não foi possível registrar {INTERFACE}: {e}");
    }
    let reply = match conn
        .request_name_with_flags(BUS_NAME, zbus::fdo::RequestNameFlags::DoNotQueue.into())
    {
        Ok(zbus::fdo::RequestNameReply::PrimaryOwner) => Ok(NameReply::PrimaryOwner),
        Ok(zbus::fdo::RequestNameReply::AlreadyOwner) => Ok(NameReply::AlreadyOwner),
        // Com DoNotQueue, Exists e InQueue nunca chegam como Ok: a zbus
        // mapeia Exists para Err(NameTaken). Mantidos por defesa.
        Ok(zbus::fdo::RequestNameReply::Exists) | Err(zbus::Error::NameTaken) => {
            Ok(NameReply::Exists)
        }
        Ok(zbus::fdo::RequestNameReply::InQueue) => Ok(NameReply::InQueue),
        Err(_) => Err(()),
    };
    match crate::commands::decide_instance(reply) {
        crate::commands::InstanceDecision::Owner => SingleInstance::Owner(conn),
        crate::commands::InstanceDecision::Forward => {
            let forwarded = zbus::blocking::Proxy::new(&conn, BUS_NAME, OBJECT_PATH, INTERFACE)
                .and_then(|proxy| proxy.call::<_, _, ()>("Show", &()));
            if let Err(e) = &forwarded {
                eprintln!("AI Monitor: instância existente não respondeu a Show: {e}");
            }
            SingleInstance::Forwarded(forwarded.is_ok())
        }
        crate::commands::InstanceDecision::Standalone => {
            eprintln!("AI Monitor: nome {BUS_NAME} recusado no D-Bus; instância única desativada.");
            SingleInstance::NoBus
        }
    }
}

struct WindowApp {
    inner: Rc<RefCell<MonitorApp>>,
    control: Arc<Control>,
    shared: Arc<Shared>,
}
impl eframe::App for WindowApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        *self.control.ctx.lock().unwrap() = Some(ctx.clone());
        while let Some(cmd) = self.control.try_recv() {
            match crate::commands::decide(true, cmd) {
                Action::FocusAndNavigate(nav) => {
                    if nav != 0 {
                        self.shared.navigation.store(nav, Ordering::Release);
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Action::CloseWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Action::Exit => {
                    self.shared.stop.store(true, Ordering::Relaxed);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Action::OpenWindow(_) | Action::Ignore => {}
            }
        }
        self.inner.borrow_mut().update(ctx, frame);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        *self.control.ctx.lock().unwrap() = None;
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn prepare_window(
    ctx: &egui::Context,
    slot: &Rc<RefCell<Option<Rc<RefCell<MonitorApp>>>>>,
    shared: &Arc<Shared>,
) -> Rc<RefCell<MonitorApp>> {
    // Cada run_native cria um egui Context novo; fontes, os estilos dos dois
    // temas e o tema atual precisam ser aplicados em toda janela.
    crate::theme::install(ctx);
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(Rc::new(RefCell::new(MonitorApp::for_shared(
            shared.clone(),
        ))));
    }
    let app = slot.clone().unwrap();
    app.borrow_mut().attach(ctx);
    app
}

fn open_window(
    shared: &Arc<Shared>,
    control: &Arc<Control>,
    app: &Rc<RefCell<Option<Rc<RefCell<MonitorApp>>>>>,
    nav: u8,
) {
    if nav != 0 {
        shared.navigation.store(nav, Ordering::Release);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(crate::app::WINDOW_SIZE)
            .with_min_inner_size(crate::app::MIN_WINDOW_SIZE)
            .with_title("AI Monitor")
            .with_app_id("ai-monitor"),
        ..Default::default()
    };
    let app_cell = app.clone();
    let control_cell = control.clone();
    let shared_cell = shared.clone();
    control.window_open.store(true, Ordering::Release);
    let result = eframe::run_native(
        "AI Monitor",
        options,
        Box::new(move |cc| {
            Ok(Box::new(WindowApp {
                inner: prepare_window(&cc.egui_ctx, &app_cell, &shared_cell),
                control: control_cell,
                shared: shared_cell,
            }))
        }),
    );
    control.window_open.store(false, Ordering::Release);
    *control.ctx.lock().unwrap() = None;
    if let Err(e) = result {
        eprintln!("AI Monitor: falha ao abrir a janela: {e}");
        // Não deixar navegação pendente de um Open que falhou.
        shared.navigation.store(0, Ordering::Release);
    }
}

pub fn run(config: Config) -> eframe::Result {
    let control = Control::new();
    let _connection = match single_instance(&control) {
        SingleInstance::Forwarded(ok) => {
            // Sai 0 somente quando a instância existente confirmou o Show.
            std::process::exit(if ok { 0 } else { 1 });
        }
        SingleInstance::Owner(conn) => Some(conn),
        SingleInstance::NoBus => {
            eprintln!("AI Monitor: barramento de sessão indisponível; instância única desativada.");
            None
        }
    };
    let shared = Shared::start(config);
    crate::portal::start();
    let tray = crate::tray::start(&shared, &control);
    let has_tray = tray.is_some();
    if !has_tray {
        eprintln!(
            "AI Monitor: nenhuma bandeja do sistema (StatusNotifierWatcher) disponível; \
             a janela abrirá diretamente e fechar encerra o app."
        );
    }
    let app: Rc<RefCell<Option<Rc<RefCell<MonitorApp>>>>> = Rc::new(RefCell::new(None));
    let mut open_now = !has_tray || std::env::args().any(|a| a == "--show-dashboard");
    loop {
        let cmd = if open_now {
            open_now = false;
            Some(Command::Open(0))
        } else {
            // O próprio Control guarda um Sender, então o canal nunca fecha:
            // a saída vem de Action::Exit, de shared.stop ou do modo sem bandeja.
            control.wait()
        };
        let Some(cmd) = cmd else {
            break;
        };
        match crate::commands::decide(control.window_open(), cmd) {
            Action::OpenWindow(nav) => {
                open_window(&shared, &control, &app, nav);
                // Sem bandeja, fechar a janela encerra o app (fase 1). Com
                // bandeja, um Quit consumido pela janela já esvaziou o canal;
                // a saída é sinalizada pelo stop, não por outro comando.
                if !has_tray || shared.stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            Action::Exit => break,
            Action::FocusAndNavigate(_) | Action::CloseWindow | Action::Ignore => {}
        }
    }
    shared.stop.store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_window_styles_every_context_and_reuses_the_app() {
        let shared = Shared::unstarted(crate::model::Config::default());
        let slot: Rc<RefCell<Option<Rc<RefCell<MonitorApp>>>>> = Rc::new(RefCell::new(None));
        let first = prepare_window(&egui::Context::default(), &slot, &shared);
        let second = prepare_window(&egui::Context::default(), &slot, &shared);
        assert!(
            Rc::ptr_eq(&first, &second),
            "the same MonitorApp must serve every window"
        );
        // egui::Context::default() shares no state: the bg of both themes, the
        // explicit theme and the semibold family on every fresh context prove
        // they were applied again.
        for ctx in [egui::Context::default(), egui::Context::default()] {
            prepare_window(&ctx, &slot, &shared);
            for (theme, palette) in [
                (egui::Theme::Dark, &crate::theme::DARK),
                (egui::Theme::Light, &crate::theme::LIGHT),
            ] {
                assert_eq!(
                    ctx.style_of(theme).visuals.panel_fill,
                    palette.bg,
                    "each fresh egui Context must receive the {theme:?} style"
                );
            }
            // Tests never start the portal: System without an answer is dark,
            // set explicitly (not left to the winit fallback).
            assert_eq!(
                ctx.options(|o| o.theme_preference),
                egui::ThemePreference::Dark
            );
            let out = ctx.run(egui::RawInput::default(), |_| {});
            assert!(
                ctx.fonts(|f| f.families().contains(&crate::theme::semibold())),
                "each fresh egui Context must receive the app fonts"
            );
            // The reused MonitorApp already sent SetTheme to an earlier
            // context; a new window must get it again (CSD decorations).
            assert!(
                out.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .contains(&egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark)),
                "each fresh egui Context must receive the window theme"
            );
        }
    }
}
