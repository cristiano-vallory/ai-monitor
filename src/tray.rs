//! Bandeja do sistema via StatusNotifierItem (ksni): somente ícone,
//! menu espelhando o do macOS e atualização a cada 1 s quando algo muda.

use crate::{commands::Command, linux::Control, menu_bar::MenuReading, monitor::Shared};
use chrono::Utc;
use ksni::{MenuItem, Tray, menu::*};
use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::Duration,
};

enum MenuKind {
    Info,
    Separator,
    Action(Command),
}

struct MenuSpec {
    kind: MenuKind,
    label: String,
}

fn info(label: impl Into<String>) -> MenuSpec {
    MenuSpec {
        kind: MenuKind::Info,
        label: label.into(),
    }
}
fn separator() -> MenuSpec {
    MenuSpec {
        kind: MenuKind::Separator,
        label: String::new(),
    }
}
fn action(label: &str, command: Command) -> MenuSpec {
    MenuSpec {
        kind: MenuKind::Action(command),
        label: label.into(),
    }
}

/// Mesmo layout do menu do macOS (menu_bar.rs): linhas informativas
/// desabilitadas, separador, ações habilitadas.
fn menu_specs(reading: &MenuReading) -> Vec<MenuSpec> {
    vec![
        info("AI Monitor"),
        info(reading.ai.clone()),
        info(reading.docker.clone()),
        info(reading.freshness.clone()),
        info(reading.activity.clone()),
        info("CPU: 100% = 1 núcleo · M/G = MiB/GiB"),
        info("Docker = contêineres ativos, incluindo ai-memory"),
        separator(),
        action("Abrir painel", Command::Open(0)),
        action("Saúde e sessões", Command::Open(1)),
        action("Revisar propostas e handoffs", Command::Open(2)),
        action("Administração avançada", Command::Open(3)),
        action("Ocultar painel", Command::Hide),
        action("Sair do AI Monitor", Command::Quit),
    ]
}

/// "_" isolado é marcador de mnemônico no dbusmenu e some; "__" vira um "_".
fn escape_label(label: &str) -> String {
    label.replace('_', "__")
}

fn tooltip(reading: &MenuReading) -> String {
    format!(
        "AI Monitor\n{}\n{}\n{}\n{}\nCPU: 100% = 1 núcleo. M/G = MiB/GiB. Docker inclui ai-memory.",
        reading.ai, reading.docker, reading.freshness, reading.activity
    )
}

const GREEN: (u8, u8, u8) = (102, 224, 184);
const AMBER: (u8, u8, u8) = (244, 190, 103);

fn set_px(data: &mut [u8], size: i32, x: i32, y: i32, (r, g, b): (u8, u8, u8)) {
    if x >= 0 && x < size && y >= 0 && y < size {
        let i = ((y * size + x) * 4) as usize;
        // ARGB32 em ordem de rede: A, R, G, B.
        data[i..i + 4].copy_from_slice(&[0xff, r, g, b]);
    }
}
fn stamp(data: &mut [u8], size: i32, cx: f32, cy: f32, radius: f32, color: (u8, u8, u8)) {
    let r = radius.ceil() as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if (dx * dx + dy * dy) as f32 <= radius * radius {
                set_px(data, size, cx as i32 + dx, cy as i32 + dy, color);
            }
        }
    }
}
fn segment(data: &mut [u8], size: i32, a: (f32, f32), b: (f32, f32), radius: f32) {
    let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) * 2.) as i32 + 1;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        stamp(
            data,
            size,
            a.0 + (b.0 - a.0) * t,
            a.1 + (b.1 - a.1) * t,
            radius,
            GREEN,
        );
    }
}

/// Marca do app gerada em código: linha de pulso verde sobre fundo
/// transparente; com alerta, um ponto âmbar no canto inferior direito.
pub fn pixmap(size: i32, alert: bool) -> ksni::Icon {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let pts: [(f32, f32); 7] = [
        (0.04, 0.62),
        (0.26, 0.62),
        (0.38, 0.28),
        (0.52, 0.82),
        (0.64, 0.50),
        (0.76, 0.62),
        (0.96, 0.62),
    ];
    let pts: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (x * s, y * s)).collect();
    let radius = (s * 0.055).max(1.0);
    for pair in pts.windows(2) {
        segment(&mut data, size, pair[0], pair[1], radius);
    }
    if alert {
        stamp(
            &mut data,
            size,
            s * 0.82,
            s * 0.82,
            (s * 0.16).max(1.5),
            AMBER,
        );
    }
    ksni::Icon {
        width: size,
        height: size,
        data,
    }
}

pub struct AiTray {
    control: Arc<Control>,
    reading: MenuReading,
}

impl Tray for AiTray {
    fn id(&self) -> String {
        "ai-monitor".into()
    }
    fn title(&self) -> String {
        "AI Monitor".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [22, 32, 48]
            .into_iter()
            .map(|s| pixmap(s, self.reading.alert))
            .collect()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "AI Monitor".into(),
            description: tooltip(&self.reading),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.control.send(Command::Open(0));
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        menu_specs(&self.reading)
            .into_iter()
            .map(|spec| match spec.kind {
                MenuKind::Separator => MenuItem::Separator,
                MenuKind::Info => MenuItem::Standard(StandardItem {
                    label: escape_label(&spec.label),
                    enabled: false,
                    ..Default::default()
                }),
                MenuKind::Action(command) => MenuItem::Standard(StandardItem {
                    label: escape_label(&spec.label),
                    enabled: true,
                    activate: Box::new(move |this: &mut Self| {
                        this.control.send(command);
                    }),
                    ..Default::default()
                }),
            })
            .collect()
    }
}

fn current_reading(shared: &Shared) -> Option<MenuReading> {
    let now = Utc::now();
    let config = shared.config.try_read().ok()?;
    let snapshot = shared.resources.try_lock().ok()?;
    let api = shared.api.try_lock().ok();
    let health = shared.health.try_lock().ok();
    Some(
        MenuReading::from_snapshot(&snapshot, &config.container, config.resource_seconds, now)
            .with_context(api.as_deref(), health.as_deref(), &config, now),
    )
}

pub fn start(
    shared: &Arc<Shared>,
    control: &Arc<Control>,
) -> Option<ksni::blocking::Handle<AiTray>> {
    // O Handle precisa ficar vivo: ao ser descartado a bandeja sai do ar.
    use ksni::blocking::TrayMethods;
    let reading = current_reading(shared).unwrap_or_else(|| {
        MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, Utc::now())
    });
    let tray = AiTray {
        control: control.clone(),
        reading,
    };
    let handle = tray.spawn().ok()?;
    let worker = handle.clone();
    let shared = shared.clone();
    let control = control.clone();
    let trace = std::env::args().any(|a| a == "--trace-menu");
    thread::spawn(move || {
        let mut remaining = if trace { 120u32 } else { 0 };
        let mut last = current_reading(&shared);
        loop {
            if shared.stop.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_secs(1));
            let Some(reading) = current_reading(&shared) else {
                continue;
            };
            if last.as_ref() != Some(&reading) {
                let next = reading.clone();
                worker.update(|t| t.reading = next);
                last = Some(reading);
            }
            if remaining > 0 {
                remaining -= 1;
                let r = last.as_ref().unwrap();
                println!(
                    "{}",
                    serde_json::json!({
                        "at": Utc::now(),
                        "menu_title": r.title,
                        "alert": r.alert,
                        "window_open": control.window_open(),
                        "history_samples": shared.history.try_lock().ok().map(|h| h.len()),
                        "freshness": r.freshness,
                    })
                );
            }
        }
    });
    Some(handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tray() -> AiTray {
        let config = crate::model::Config::default();
        AiTray {
            control: Control::new(),
            reading: MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, Utc::now())
                .with_context(None, None, &config, Utc::now()),
        }
    }

    #[test]
    fn menu_labels_order_and_enabled_flags_mirror_macos() {
        let tray = tray();
        let items = tray.menu();
        let labels: Vec<String> = items
            .iter()
            .map(|item| match item {
                MenuItem::Standard(s) => s.label.clone(),
                MenuItem::Separator => "---".into(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(
            labels,
            [
                "AI Monitor",
                "ai-memory · métricas indisponíveis",
                "Docker (contêineres) · métricas indisponíveis",
                "Aguardando a primeira coleta…",
                "Handoffs abertos — · Propostas: aguardando",
                "CPU: 100% = 1 núcleo · M/G = MiB/GiB",
                "Docker = contêineres ativos, incluindo ai-memory",
                "---",
                "Abrir painel",
                "Saúde e sessões",
                "Revisar propostas e handoffs",
                "Administração avançada",
                "Ocultar painel",
                "Sair do AI Monitor",
            ]
        );
        let enabled: Vec<bool> = items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Standard(s) => Some(s.enabled),
                _ => None,
            })
            .collect();
        assert_eq!(
            enabled,
            [
                false, false, false, false, false, false, false, true, true, true, true, true, true
            ]
        );
    }

    #[test]
    fn every_action_sends_the_right_command() {
        let expected = [
            Command::Open(0),
            Command::Open(1),
            Command::Open(2),
            Command::Open(3),
            Command::Hide,
            Command::Quit,
        ];
        for (index, want) in expected.into_iter().enumerate() {
            let mut tray = tray();
            let items = tray.menu();
            let actions: Vec<_> = items
                .iter()
                .filter_map(|item| match item {
                    MenuItem::Standard(s) if s.enabled => Some(s),
                    _ => None,
                })
                .collect();
            (actions[index].activate)(&mut tray);
            let got = tray.control.try_recv().expect("command must be sent");
            assert_eq!(got, want, "action index {index}");
            assert!(tray.control.try_recv().is_none(), "exactly one command");
            // activate (clique esquerdo no ícone) abre o painel
            tray.activate(0, 0);
            assert_eq!(tray.control.try_recv(), Some(Command::Open(0)));
        }
    }

    #[test]
    fn underscores_in_dynamic_labels_are_escaped_for_dbusmenu() {
        let mut tray = tray();
        tray.reading.activity = "Projeto proj_x / ws_y".into();
        let labels: Vec<String> = tray
            .menu()
            .iter()
            .filter_map(|item| match item {
                MenuItem::Standard(s) => Some(s.label.clone()),
                _ => None,
            })
            .collect();
        assert!(
            labels.iter().any(|l| l == "Projeto proj__x / ws__y"),
            "single _ must become __, got: {labels:?}"
        );
    }

    #[test]
    fn empty_activity_row_is_info_not_separator() {
        let mut tray = tray();
        tray.reading.activity = String::new();
        let kinds: Vec<&str> = tray
            .menu()
            .iter()
            .map(|item| match item {
                MenuItem::Standard(_) => "item",
                MenuItem::Separator => "sep",
                _ => "?",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "item", "item", "item", "item", "item", "item", "item", "sep", "item", "item",
                "item", "item", "item", "item"
            ],
            "empty activity must still be an info row, not a separator"
        );
    }

    #[test]
    fn pixmap_dimensions_opacity_and_alert_dot() {
        for size in [22, 32, 48] {
            let icon = pixmap(size, false);
            assert_eq!((icon.width, icon.height), (size, size));
            assert_eq!(icon.data.len() as i64, size as i64 * size as i64 * 4);
            let (px, _) = icon.data.as_chunks::<4>();
            assert!(
                px.iter().any(|px| px[0] != 0),
                "size {size} must have opaque pixels"
            );
            assert!(
                px.iter().any(|px| px[0] == 0),
                "size {size} must keep transparent background"
            );
        }
        let normal = pixmap(32, false);
        let alert = pixmap(32, true);
        assert_ne!(normal.data, alert.data, "alert must change pixels");
        let amber = [0xff, AMBER.0, AMBER.1, AMBER.2];
        let (px, _) = alert.data.as_chunks::<4>();
        assert!(px.contains(&amber), "alert dot must be amber");
    }
}
