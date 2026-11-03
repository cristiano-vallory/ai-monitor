mod advanced;
mod api;
mod app;
mod auth;
mod client;
#[cfg(any(target_os = "linux", test))]
mod commands;
mod console;
mod docker;
mod docker_ui;
mod handoff_text;
mod health;
mod health_ui;
mod keychain;
#[cfg(target_os = "linux")]
mod linux;
mod local_service;
mod maintenance;
mod management_ui;
mod mcp;
mod menu_bar;
mod model;
mod monitor;
mod operations_ui;
mod platform;
#[cfg(target_os = "linux")]
mod portal;
mod queries;
mod sessions_ui;
#[cfg(test)]
mod test_support;
mod theme;
#[cfg(target_os = "linux")]
mod tray;
mod widgets;

fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--version") {
        println!(
            "AI Monitor {} ({})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::ARCH
        );
        return Ok(());
    }
    let config = model::Config::load();
    if std::env::args().any(|a| a == "--diagnose") {
        let mut system = sysinfo::System::new();
        // Warm up CPU deltas; the first sysinfo sample is not a utilization interval.
        let _ = monitor::sample_resources(&config, &mut system);
        std::thread::sleep(std::time::Duration::from_secs(1));
        let mut resources = monitor::sample_resources(&config, &mut system);
        match docker::images(&config.docker_binary, &mut resources.containers) {
            Ok(images) => resources.images = images,
            Err(e) => resources.inventory_error = Some(e),
        }
        resources.docker_context = docker::context(&config.docker_binary);
        let api = api::collect(
            &config,
            &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let result = serde_json::json!({"resources":resources,"api":{
            "version":api.version,"projects":api.projects.len(),"sessions":api.sessions.len(),
            "open_sessions_unique":api.open_sessions(),"open_handoffs":api.handoff_total,"listed_handoffs":api.handoffs.len(),
            "open_session_links":api.open_session_links(),"warnings":api.warnings,
            "error":api.error,"partial_errors":api.errors
        }});
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    return linux::run(config);
    #[cfg(not(target_os = "linux"))]
    {
        let show_dashboard = std::env::args().any(|a| a == "--show-dashboard");
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size(app::WINDOW_SIZE)
                .with_min_inner_size(app::MIN_WINDOW_SIZE)
                .with_title("AI Monitor")
                // Barra de título transparente, com o conteúdo por baixo; os
                // botões da janela ficam na posição padrão do AppKit.
                .with_fullsize_content_view(true)
                .with_titlebar_shown(false)
                .with_title_shown(false)
                .with_visible(show_dashboard)
                .with_active(show_dashboard),
            ..Default::default()
        };
        eframe::run_native(
            "AI Monitor",
            options,
            Box::new(move |cc| Ok(Box::new(app::MonitorApp::new(cc, config)))),
        )
    }
}
