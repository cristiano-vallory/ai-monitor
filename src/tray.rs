//! Bandeja do sistema via StatusNotifierItem (ksni): somente ícone, o menu
//! de `menu::entries` em texto e atualização a cada 1 s quando algo muda.

use crate::{
    commands::Command,
    linux::Control,
    menu::{MenuAction, MenuReading, Row},
    monitor::Shared,
};
use chrono::Utc;
use ksni::{MenuItem, Tray, menu::*};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    thread,
    time::Duration,
};

/// O comando que cada ação do menu manda ao app.
fn command(action: MenuAction) -> Command {
    match action {
        MenuAction::Open(destination) => Command::Open(destination.code()),
        MenuAction::Quit => Command::Quit,
    }
}

/// "_" isolado é marcador de mnemônico no dbusmenu e some; "__" vira um "_".
fn escape_label(label: &str) -> String {
    label.replace('_', "__")
}

const GREEN: (u8, u8, u8) = (102, 224, 184);
const AMBER: (u8, u8, u8) = crate::menu::ALERT_DOT;
/// O ponto de alerta nas 24 unidades de `menu::PULSE`: canto superior
/// direito, acima do traço. O pixmap e o SVG o desenham daqui.
const DOT_AT: (f32, f32) = (19.2, 4.8);
const DOT_RADIUS: f32 = 4.32;
/// O traço no SVG para hosts que não recolorem: cinza médio, ≥ 3:1 numa
/// barra clara (#E1E2E7) e numa escura (#16161E).
const SYMBOLIC_INK: &str = "#808080";

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

/// Marca do app gerada em código: o pulso (`menu::PULSE`) verde sobre fundo
/// transparente; com `dot`, o ponto âmbar no canto superior direito.
pub fn pixmap(size: i32, dot: bool) -> ksni::Icon {
    let mut data = vec![0u8; (size * size * 4) as usize];
    let s = size as f32;
    let pts: Vec<(f32, f32)> = crate::menu::PULSE
        .iter()
        .map(|&(x, y)| (x / 24. * s, y / 24. * s))
        .collect();
    let radius = (s * crate::menu::PULSE_WIDTH / 48.).max(1.0);
    for pair in pts.windows(2) {
        segment(&mut data, size, pair[0], pair[1], radius);
    }
    if dot {
        stamp(
            &mut data,
            size,
            DOT_AT.0 / 24. * s,
            DOT_AT.1 / 24. * s,
            (DOT_RADIUS / 24. * s).max(1.5),
            AMBER,
        );
    }
    ksni::Icon {
        width: size,
        height: size,
        data,
    }
}

/// O ícone simbólico (`-symbolic`) em SVG. O recolorir do GTK troca o `fill`
/// (texto da barra; `class="warning"`, o aviso), não o `stroke`: o traço do
/// pulso vira um quadrilátero por trecho e um círculo por vértice (pontas e
/// junções redondas). Com `dot`, o ponto de alerta.
fn symbolic_svg(dot: bool) -> String {
    use crate::menu::{PULSE, PULSE_WIDTH};
    let half = PULSE_WIDTH / 2.;
    let mut svg = String::from(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24">"#,
    );
    svg.push('\n');
    for pair in PULSE.windows(2) {
        let ((ax, ay), (bx, by)) = (pair[0], pair[1]);
        let length = (bx - ax).hypot(by - ay);
        let (nx, ny) = (-(by - ay) / length * half, (bx - ax) / length * half);
        svg.push_str(&format!(
            r#"<path fill="{SYMBOLIC_INK}" d="M{:.3} {:.3}L{:.3} {:.3}L{:.3} {:.3}L{:.3} {:.3}Z"/>"#,
            ax + nx,
            ay + ny,
            bx + nx,
            by + ny,
            bx - nx,
            by - ny,
            ax - nx,
            ay - ny,
        ));
        svg.push('\n');
    }
    for (x, y) in PULSE {
        svg.push_str(&format!(
            r#"<circle fill="{SYMBOLIC_INK}" cx="{x}" cy="{y}" r="{half}"/>"#
        ));
        svg.push('\n');
    }
    if dot {
        let (r, g, b) = AMBER;
        svg.push_str(&format!(
            r##"<circle class="warning" fill="#{r:02X}{g:02X}{b:02X}" cx="{}" cy="{}" r="{DOT_RADIUS}"/>"##,
            DOT_AT.0, DOT_AT.1
        ));
        svg.push('\n');
    }
    svg.push_str("</svg>\n");
    svg
}
fn symbolic_name(dot: bool) -> &'static str {
    if dot {
        "ai-monitor-alert-symbolic"
    } else {
        "ai-monitor-symbolic"
    }
}
/// Onde gravar os ícones: `$XDG_RUNTIME_DIR/ai-monitor`; sem ele, o cache do
/// usuário (`$XDG_CACHE_HOME` ou `~/.cache`). Só valem caminhos absolutos.
/// Sem os dois, nenhum: um caminho previsível no /tmp compartilhado seguiria
/// symlinks de outro usuário (CWE-377/CWE-59), e a bandeja fica no pixmap.
fn icon_base(
    runtime: Option<OsString>,
    cache: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    let absolute = |v: Option<OsString>| v.map(PathBuf::from).filter(|p| p.is_absolute());
    absolute(runtime)
        .or_else(|| absolute(cache))
        .or_else(|| absolute(home).map(|h| h.join(".cache")))
        .map(|dir| dir.join("ai-monitor"))
}
/// Grava os dois ícones simbólicos em `base/icons` (no tema hicolor e soltos,
/// para hosts que não leem o tema) e devolve esse `IconThemePath`; `None`
/// se a gravação falhar (a bandeja fica só com o pixmap). O último trecho
/// do caminho tem de ser `icons`: o Plasma só acha o tema assim.
fn install_icons(base: &Path) -> Option<PathBuf> {
    let root = base.join("icons");
    for dir in [root.join("hicolor/scalable/status"), root.clone()] {
        std::fs::create_dir_all(&dir).ok()?;
        for dot in [false, true] {
            let file = dir.join(format!("{}.svg", symbolic_name(dot)));
            std::fs::write(file, symbolic_svg(dot)).ok()?;
        }
    }
    Some(root)
}

pub struct AiTray {
    control: Arc<Control>,
    reading: MenuReading,
    /// O `IconThemePath` dos ícones simbólicos; `None` = só o pixmap.
    icons: Option<PathBuf>,
}

impl Tray for AiTray {
    fn id(&self) -> String {
        "ai-monitor".into()
    }
    fn title(&self) -> String {
        "AI Monitor".into()
    }
    fn icon_theme_path(&self) -> String {
        self.icons
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
    /// O ponto segue `alert_dot`; sem os SVGs gravados, nenhum nome (o host
    /// usa o pixmap).
    fn icon_name(&self) -> String {
        match self.icons {
            Some(_) => symbolic_name(self.reading.alert_dot).into(),
            None => String::new(),
        }
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        [22, 32, 48]
            .into_iter()
            .map(|s| pixmap(s, self.reading.alert_dot))
            .collect()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "AI Monitor".into(),
            description: crate::menu::tooltip(&self.reading),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.control.send(Command::Open(0));
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        crate::menu::entries(&self.reading)
            .into_iter()
            .map(|entry| {
                let flat = entry.flat();
                let item = StandardItem {
                    label: escape_label(&flat.label),
                    enabled: flat.enabled,
                    visible: flat.visible,
                    ..Default::default()
                };
                match entry.row {
                    Row::Separator => MenuItem::Separator,
                    Row::Action { action, .. } => MenuItem::Standard(StandardItem {
                        activate: Box::new(move |this: &mut Self| {
                            this.control.send(command(action));
                        }),
                        ..item
                    }),
                    _ => MenuItem::Standard(item),
                }
            })
            .collect()
    }
}

/// `None` com algum lock ocupado: o tique é pulado e a leitura anterior fica.
fn current_reading(shared: &Shared) -> Option<MenuReading> {
    crate::menu::read(shared, Utc::now()).map(|(reading, _)| reading)
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
    let base = icon_base(
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::var_os("XDG_CACHE_HOME"),
        std::env::var_os("HOME"),
    );
    let tray = AiTray {
        control: control.clone(),
        reading,
        icons: base.as_deref().and_then(install_icons),
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
                        "state": r.state.text(),
                        "alert": !r.alerts.is_empty(),
                        "alert_dot": r.alert_dot,
                        "window_open": control.window_open(),
                        "history_samples": shared.history.try_lock().ok().map(|h| h.len()),
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
    use crate::menu::{Destination, Proposals};

    fn tray(reading: MenuReading) -> AiTray {
        AiTray {
            control: Control::new(),
            reading,
            icons: None,
        }
    }
    /// Uma leitura sem dados, como a da bandeja ao iniciar.
    fn empty() -> MenuReading {
        let now = Utc::now();
        MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, now).with_context(
            Some(&Default::default()),
            Some(&Default::default()),
            &crate::model::Config::default(),
            now,
        )
    }
    /// Com alertas (a faixa visível) e o ponto ligado.
    fn alerting() -> MenuReading {
        let now = Utc::now();
        let health = crate::health::Snapshot {
            at: Some(now),
            status: Some(serde_json::json!({"consolidation": {"failed": 1}})),
            ..Default::default()
        };
        MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, now).with_context(
            None,
            Some(&health),
            &crate::model::Config::default(),
            now,
        )
    }
    /// (rótulo, habilitado, visível, separador) de cada item da bandeja.
    fn rendered(tray: &AiTray) -> Vec<(String, bool, bool, bool)> {
        tray.menu()
            .iter()
            .map(|item| match item {
                MenuItem::Standard(s) => (s.label.clone(), s.enabled, s.visible, false),
                MenuItem::Separator => (String::new(), false, true, true),
                _ => panic!("only standard items and separators"),
            })
            .collect()
    }

    #[test]
    fn the_tray_menu_is_the_flat_spec_line_by_line() {
        // Um projeto com "_": o rótulo publicado é o escapado.
        let mut underscored = empty();
        underscored.proposals = Proposals::Watched {
            scope: "ws_a / proj_b".into(),
            pending: Some(1),
        };
        for (case, reading) in [
            ("no data", empty()),
            ("alerts", alerting()),
            ("underscores", underscored),
        ] {
            let want: Vec<_> = crate::menu::entries(&reading)
                .iter()
                .map(|e| {
                    let f = e.flat();
                    (escape_label(&f.label), f.enabled, f.visible, f.separator)
                })
                .collect();
            assert_eq!(rendered(&tray(reading)), want, "{case}");
        }
        // A faixa de alerta: escondida sem alertas, visível com eles.
        assert!(!rendered(&tray(empty()))[3].2);
        let band = &rendered(&tray(alerting()))[3];
        assert_eq!(
            (band.0.as_str(), band.1, band.2),
            ("Saúde: Consolidações com falha", false, true)
        );
    }

    #[test]
    fn every_action_sends_its_command() {
        use Destination::*;
        // A matriz de todas as ações.
        for (action, want) in [
            (MenuAction::Open(Panel), Command::Open(0)),
            (MenuAction::Open(Health), Command::Open(1)),
            (MenuAction::Open(Review), Command::Open(2)),
            (MenuAction::Open(Sessions), Command::Open(3)),
            (MenuAction::Open(Settings), Command::Open(4)),
            (MenuAction::Quit, Command::Quit),
        ] {
            assert_eq!(command(action), want, "{action:?}");
        }
        // Cada item habilitado manda o comando da ação da sua linha.
        let actions: Vec<MenuAction> = crate::menu::entries(&empty())
            .into_iter()
            .filter_map(|e| match e.row {
                Row::Action { action, .. } => Some(action),
                _ => None,
            })
            .collect();
        assert_eq!(actions.len(), 7);
        for (index, action) in actions.into_iter().enumerate() {
            let mut tray = tray(empty());
            let items = tray.menu();
            let enabled: Vec<_> = items
                .iter()
                .filter_map(|item| match item {
                    MenuItem::Standard(s) if s.enabled => Some(s),
                    _ => None,
                })
                .collect();
            (enabled[index].activate)(&mut tray);
            assert_eq!(
                tray.control.try_recv(),
                Some(command(action)),
                "item {index}"
            );
            assert!(tray.control.try_recv().is_none(), "exactly one command");
            // activate (clique esquerdo no ícone) abre o painel
            tray.activate(0, 0);
            assert_eq!(tray.control.try_recv(), Some(Command::Open(0)));
        }
    }

    #[test]
    fn underscores_in_dynamic_labels_are_escaped_for_dbusmenu() {
        let mut reading = empty();
        reading.proposals = Proposals::Watched {
            scope: "proj_x / ws_y".into(),
            pending: Some(2),
        };
        let labels: Vec<String> = rendered(&tray(reading)).into_iter().map(|r| r.0).collect();
        assert!(
            labels
                .iter()
                .any(|l| l == "2 propostas pendentes · proj__x / ws__y"),
            "single _ must become __, got: {labels:?}"
        );
    }

    /// O pixel (A, R, G, B) em `(x, y)` frações do lado.
    fn pixel(icon: &ksni::Icon, x: f32, y: f32) -> [u8; 4] {
        let (px, py) = (
            (x * icon.width as f32) as i32,
            (y * icon.height as f32) as i32,
        );
        let i = ((py * icon.width + px) * 4) as usize;
        icon.data[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn pixmap_dimensions_opacity_and_the_pulse() {
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
            // Os vértices do pulso do artboard (M2 13h4l2.5-6 4 11 3-7H22),
            // pintados; o centro do quadro, acima da linha, não.
            for (x, y) in crate::menu::PULSE {
                assert_eq!(
                    pixel(&icon, x / 24., y / 24.),
                    [0xff, GREEN.0, GREEN.1, GREEN.2],
                    "size {size}: ({x}, {y})"
                );
            }
            assert_eq!(pixel(&icon, 0.5, 0.15)[0], 0, "size {size}");
        }
    }

    #[test]
    fn the_alert_dot_is_amber_at_the_top_right_only_with_alerts_and_the_option() {
        let amber = [0xff, AMBER.0, AMBER.1, AMBER.2];
        let now = Utc::now();
        for (alert, option) in [(false, false), (false, true), (true, false), (true, true)] {
            let status = serde_json::json!({"consolidation": {"failed": u8::from(alert)}});
            let health = crate::health::Snapshot {
                at: Some(now),
                status: Some(status),
                ..Default::default()
            };
            let config = crate::model::Config {
                alert_dot: option,
                ..Default::default()
            };
            let reading = MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, now)
                .with_context(None, Some(&health), &config, now);
            let shown = alert && option;
            for icon in tray(reading).icon_pixmap() {
                let size = icon.width;
                let corner = pixel(&icon, 0.8, 0.2);
                if shown {
                    assert_eq!(corner, amber, "size {size}: the dot, top right");
                } else {
                    assert_eq!(corner[0], 0, "size {size}: alert {alert} option {option}");
                }
                // Nunca no canto inferior direito (onde ficava antes).
                assert_ne!(pixel(&icon, 0.82, 0.82), amber, "size {size}");
                let (px, _) = icon.data.as_chunks::<4>();
                assert_eq!(
                    px.contains(&amber),
                    shown,
                    "size {size}: amber only with the dot"
                );
            }
        }
    }
    /// Os elementos do SVG: (tag, atributos), um por linha.
    fn elements(svg: &str) -> Vec<(String, std::collections::HashMap<String, String>)> {
        svg.lines()
            .filter(|l| l.starts_with("<path") || l.starts_with("<circle"))
            .map(|l| {
                let tag = l[1..].split(' ').next().unwrap().to_owned();
                let attrs = l
                    .split('"')
                    .collect::<Vec<_>>()
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| {
                        let name = c[0].rsplit(' ').next().unwrap().trim_end_matches('=');
                        (name.to_owned(), c[1].to_owned())
                    })
                    .collect();
                (tag, attrs)
            })
            .collect()
    }
    fn num(v: &str) -> f32 {
        v.parse().unwrap()
    }

    #[test]
    fn the_symbolic_svg_fills_the_pulse_and_carries_the_dot_only_with_it() {
        use crate::menu::{PULSE, PULSE_WIDTH};
        let half = PULSE_WIDTH / 2.;
        for dot in [false, true] {
            let svg = symbolic_svg(dot);
            assert!(svg.starts_with("<svg ") && svg.ends_with("</svg>\n"));
            // O GTK recolore o fill: nada de stroke.
            assert!(!svg.contains("stroke"), "{svg}");
            let all = elements(&svg);
            let ink: Vec<_> = all
                .iter()
                .filter(|(_, a)| !a.contains_key("class"))
                .collect();
            assert!(ink.iter().all(|(_, a)| a["fill"] == SYMBOLIC_INK));
            // Um círculo de meia espessura em cada vértice do pulso, na ordem.
            let vertices: Vec<(f32, f32, f32)> = ink
                .iter()
                .filter(|(t, _)| t == "circle")
                .map(|(_, a)| (num(&a["cx"]), num(&a["cy"]), num(&a["r"])))
                .collect();
            let want: Vec<(f32, f32, f32)> = PULSE.iter().map(|&(x, y)| (x, y, half)).collect();
            assert_eq!(vertices, want);
            // Um quadrilátero por trecho: os cantos a meia espessura das
            // pontas, de lados opostos.
            let quads: Vec<Vec<(f32, f32)>> = ink
                .iter()
                .filter(|(t, _)| t == "path")
                .map(|(_, a)| {
                    a["d"]
                        .trim_start_matches('M')
                        .trim_end_matches('Z')
                        .split('L')
                        .map(|p| {
                            let (x, y) = p.split_once(' ').unwrap();
                            (num(x), num(y))
                        })
                        .collect()
                })
                .collect();
            assert_eq!(quads.len(), PULSE.len() - 1);
            for (i, quad) in quads.iter().enumerate() {
                let (a, b) = (PULSE[i], PULSE[i + 1]);
                let distance = |p: (f32, f32), q: (f32, f32)| (p.0 - q.0).hypot(p.1 - q.1);
                for (corner, end) in [(0, a), (1, b), (2, b), (3, a)] {
                    assert!(
                        (distance(quad[corner], end) - half).abs() < 0.01,
                        "segment {i} corner {corner}: {quad:?}"
                    );
                }
                assert!((distance(quad[0], quad[3]) - PULSE_WIDTH).abs() < 0.01);
            }
            let warning: Vec<_> = all
                .iter()
                .filter(|(_, a)| a.get("class").map(String::as_str) == Some("warning"))
                .collect();
            assert_eq!(warning.len(), usize::from(dot), "dot {dot}");
            if let [(tag, a)] = &warning[..] {
                assert_eq!(tag, "circle");
                assert_eq!(
                    (num(&a["cx"]), num(&a["cy"]), num(&a["r"])),
                    (DOT_AT.0, DOT_AT.1, DOT_RADIUS)
                );
                assert_eq!(a["fill"], "#F2A33A");
            }
        }
    }

    /// Uma pasta de teste nova sob o temporário, apagada no fim.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("ai-monitor-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_icon_name_follows_the_alert_dot_and_names_a_written_svg() {
        let dir = TempDir::new("tray-icons");
        let icons = install_icons(&dir.0).expect("written");
        assert_eq!(icons, dir.0.join("icons"));
        let now = Utc::now();
        for (alert, option) in [(false, false), (false, true), (true, false), (true, true)] {
            let health = crate::health::Snapshot {
                at: Some(now),
                status: Some(serde_json::json!({"consolidation": {"failed": u8::from(alert)}})),
                ..Default::default()
            };
            let config = crate::model::Config {
                alert_dot: option,
                ..Default::default()
            };
            let reading = MenuReading::from_snapshot(&Default::default(), "ai-memory", 2, now)
                .with_context(None, Some(&health), &config, now);
            let mut tray = tray(reading);
            tray.icons = Some(icons.clone());
            let shown = alert && option;
            let name = tray.icon_name();
            assert_eq!(
                name,
                if shown {
                    "ai-monitor-alert-symbolic"
                } else {
                    "ai-monitor-symbolic"
                },
                "alert {alert} option {option}"
            );
            assert_eq!(tray.icon_theme_path(), icons.to_string_lossy());
            for file in [
                icons.join(format!("hicolor/scalable/status/{name}.svg")),
                icons.join(format!("{name}.svg")),
            ] {
                let svg = std::fs::read_to_string(&file).expect("the named icon exists");
                assert_eq!(svg, symbolic_svg(shown), "{}", file.display());
                assert_eq!(svg.contains("class=\"warning\""), shown);
            }
        }
    }

    #[test]
    fn without_the_svgs_the_tray_names_no_icon_and_keeps_the_pixmap() {
        // A base sob um arquivo comum: create_dir_all falha.
        let dir = TempDir::new("tray-icons-blocked");
        std::fs::create_dir_all(&dir.0).unwrap();
        let file = dir.0.join("not-a-dir");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(install_icons(&file.join("base")), None);
        let mut tray = tray(alerting());
        tray.icons = None;
        assert_eq!(
            (tray.icon_name(), tray.icon_theme_path()),
            (String::new(), String::new())
        );
        assert!(!tray.icon_pixmap().is_empty(), "the pixmap stays");
    }

    #[test]
    fn the_icons_go_to_the_runtime_dir_else_the_cache_else_nowhere() {
        let os = |s: &str| Some(OsString::from(s));
        let at = |s: &str| Some(PathBuf::from(s));
        for (case, runtime, cache, home, want) in [
            (
                "runtime",
                os("/run/user/1000"),
                os("/c"),
                os("/h"),
                at("/run/user/1000/ai-monitor"),
            ),
            (
                "no runtime: cache",
                None,
                os("/c"),
                os("/h"),
                at("/c/ai-monitor"),
            ),
            (
                "empty runtime",
                os(""),
                os("/c"),
                os("/h"),
                at("/c/ai-monitor"),
            ),
            (
                "relative runtime",
                os("run"),
                None,
                os("/h"),
                at("/h/.cache/ai-monitor"),
            ),
            (
                "relative cache: home",
                None,
                os("c"),
                os("/h"),
                at("/h/.cache/ai-monitor"),
            ),
            // Nunca o /tmp compartilhado: sem diretório do usuário, nada.
            ("nothing", None, None, None, None),
            ("relative home", None, None, os("h"), None),
            ("only relative", os("r"), os("c"), os("h"), None),
        ] {
            assert_eq!(icon_base(runtime, cache, home), want, "{case}");
        }
    }
}
