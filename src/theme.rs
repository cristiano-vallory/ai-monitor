//! Tokens de cor, fontes e tema claro/escuro. Toda cor de UI sai daqui.
use crate::model::Appearance;
use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, SystemTheme, TextStyle,
    Theme, ThemePreference, Vec2, Visuals,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub side: Color32,
    pub surf: Color32,
    pub surf2: Color32,
    pub line: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub acc: Color32,
    pub series2: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub on_acc: Color32,
    /// `acc` translúcido: aplicado sobre qualquer fundo.
    pub sel: Color32,
    /// Tom informativo (faixas de aviso `info`, Sessions.dc.html).
    pub info: Color32,
    /// Sombra leve dos cartões e das folhas (preto translúcido).
    pub shadow: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x14, 0x15, 0x18),
    side: Color32::from_rgb(0x1A, 0x1B, 0x1F),
    surf: Color32::from_rgb(0x1E, 0x1F, 0x23),
    surf2: Color32::from_rgb(0x26, 0x28, 0x2D),
    line: Color32::from_rgb(0x2E, 0x30, 0x36),
    text: Color32::from_rgb(0xEC, 0xED, 0xEF),
    text2: Color32::from_rgb(0x9E, 0xA2, 0xAA),
    acc: Color32::from_rgb(0x4F, 0xD1, 0xA5),
    series2: Color32::from_rgb(0x7C, 0x84, 0xFF),
    warn: Color32::from_rgb(0xF2, 0xB4, 0x5A),
    bad: Color32::from_rgb(0xFF, 0x7A, 0x7A),
    on_acc: Color32::from_rgb(0x06, 0x26, 0x1C),
    // 33/255 ≈ 13% de alfa.
    sel: Color32::from_rgba_unmultiplied_const(0x4F, 0xD1, 0xA5, 33),
    info: Color32::from_rgb(0x8B, 0x93, 0xFF),
    // 115/255 ≈ 45% de alfa.
    shadow: Color32::from_black_alpha(115),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF5, 0xF5, 0xF7),
    side: Color32::from_rgb(0xEC, 0xEC, 0xEF),
    surf: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    surf2: Color32::from_rgb(0xF1, 0xF1, 0xF4),
    line: Color32::from_rgb(0xE2, 0xE3, 0xE8),
    text: Color32::from_rgb(0x1D, 0x1D, 0x1F),
    text2: Color32::from_rgb(0x5E, 0x61, 0x69),
    acc: Color32::from_rgb(0x0B, 0x7A, 0x5E),
    series2: Color32::from_rgb(0x55, 0x5C, 0xD6),
    warn: Color32::from_rgb(0xA1, 0x5C, 0x00),
    bad: Color32::from_rgb(0xC4, 0x2B, 0x2B),
    on_acc: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    // 26/255 ≈ 10% de alfa.
    sel: Color32::from_rgba_unmultiplied_const(0x0B, 0x7A, 0x5E, 26),
    info: Color32::from_rgb(0x4F, 0x55, 0xD9),
    // 15/255 ≈ 6% de alfa.
    shadow: Color32::from_black_alpha(15),
};

/// Paleta do tema em uso, a partir dos `Visuals` ativos.
pub fn palette(visuals: &Visuals) -> &'static Palette {
    if visuals.dark_mode { &DARK } else { &LIGHT }
}

pub const SEMIBOLD: &str = "semibold";
pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

pub fn visuals(theme: Theme) -> Visuals {
    let (p, mut v) = match theme {
        Theme::Dark => (&DARK, Visuals::dark()),
        Theme::Light => (&LIGHT, Visuals::light()),
    };
    v.panel_fill = p.bg;
    v.extreme_bg_color = p.bg;
    v.window_fill = p.surf;
    v.faint_bg_color = p.surf2;
    v.text_edit_bg_color = Some(p.surf2);
    v.widgets.inactive.bg_fill = p.surf2;
    v.widgets.inactive.weak_bg_fill = p.surf2;
    v.window_stroke = Stroke::new(1.0_f32, p.line);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.line);
    // Mantém a paridade com o tema anterior: todo texto sem cor própria usa
    // `text`, qualquer que seja o estado do widget.
    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.text2);
    v.hyperlink_color = p.acc;
    v.selection.stroke = Stroke::new(1.0_f32, p.acc);
    v.selection.bg_fill = p.sel;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.bad;
    v
}

/// Estilo completo de um tema; espaçamento e tamanhos iguais nos dois.
pub fn style(theme: Theme) -> egui::Style {
    let mut style = egui::Style {
        visuals: visuals(theme),
        ..Default::default()
    };
    style.spacing.item_spacing = Vec2::new(10., 10.);
    style.spacing.button_padding = Vec2::new(12., 8.);
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(14.));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.));
    let heading = style.text_styles[&TextStyle::Heading].size;
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::new(heading, semibold()));
    style
}

/// Inter e JetBrains Mono na frente; as fontes padrão do egui seguem como
/// fallback, na ordem original.
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, data) in [
        (
            "Inter-Regular",
            &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..],
        ),
        (
            "Inter-SemiBold",
            &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..],
        ),
        (
            "JetBrainsMonoNL-Regular",
            &include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(data)));
    }
    let proportional = fonts.families[&FontFamily::Proportional].clone();
    let mut semibold_family = vec!["Inter-SemiBold".to_owned()];
    semibold_family.extend(proportional.iter().cloned());
    fonts.families.insert(semibold(), semibold_family);
    for (family, first) in [
        (FontFamily::Proportional, "Inter-Regular"),
        (FontFamily::Monospace, "JetBrainsMonoNL-Regular"),
    ] {
        fonts
            .families
            .get_mut(&family)
            .unwrap()
            .insert(0, first.into());
    }
    fonts
}

/// Fontes e os estilos dos dois temas. Precisa rodar em todo `egui::Context`
/// novo (no Linux, cada janela tem o seu).
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    for theme in [Theme::Dark, Theme::Light] {
        ctx.set_style_of(theme, style(theme));
    }
}

/// Preferência do egui para a escolha do usuário. `platform` é o tema do
/// sistema já lido pelo app (`platform_theme`); `None` = sem leitura, e aí o
/// egui segue o tema que o winit informa.
pub fn resolve(choice: Appearance, platform: Option<Theme>) -> ThemePreference {
    match choice {
        Appearance::Light => ThemePreference::Light,
        Appearance::Dark => ThemePreference::Dark,
        Appearance::System => platform.map_or(ThemePreference::System, ThemePreference::from),
    }
}

/// Regra do Linux para o `color-scheme` do portal: 1 = escuro, 2 = claro,
/// 0 = sem preferência (claro). Sem resposta ou fora de 0..=2, escuro.
#[cfg(any(target_os = "linux", test))]
fn portal_theme(scheme: Option<u32>) -> Theme {
    match scheme {
        Some(0 | 2) => Theme::Light,
        _ => Theme::Dark,
    }
}

/// Tema do sistema lido a cada frame. No Linux o winit não informa o tema
/// (None no X11; no Wayland, só o das decorações), então vale o portal.
#[cfg(target_os = "linux")]
pub fn platform_theme() -> Option<Theme> {
    Some(portal_theme(crate::portal::current()))
}

/// No macOS o tema não vem do `system_theme` do egui: depois que a janela
/// recebe aparência explícita (Claro/Escuro), o winit descarta o
/// `ThemeChanged` enquanto ela durar (winit-0.30.13
/// macos/window_delegate.rs:464-466) e, ao voltar para a do sistema, só o
/// emite se o tema efetivo da janela mudar naquele instante (:472-474). Uma
/// troca do sistema nesse intervalo deixaria o egui com o tema antigo. O
/// NSApp não recebe aparência explícita, então o seu `effectiveAppearance`
/// acompanha o sistema.
#[cfg(target_os = "macos")]
pub fn platform_theme() -> Option<Theme> {
    use objc2::sel;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{MainThreadMarker, NSArray, NSObjectProtocol, ns_string};
    // Fora da thread principal não há leitura: o egui decide (System).
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    if !app.respondsToSelector(sel!(effectiveAppearance)) {
        return None;
    }
    // Mesmo critério do winit (`appearance_to_theme`, window_delegate.rs:1875-1892).
    let dark = ns_string!("NSAppearanceNameDarkAqua");
    let names = NSArray::from_slice(&[ns_string!("NSAppearanceNameAqua"), dark]);
    let best = app
        .effectiveAppearance()
        .bestMatchFromAppearancesWithNames(&names)?;
    Some(if best.isEqualToString(dark) {
        Theme::Dark
    } else {
        Theme::Light
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn platform_theme() -> Option<Theme> {
    None
}

/// Tema da janela nativa (`ViewportCommand::SetTheme`): fora do Linux,
/// Sistema devolve a janela à aparência do sistema; no Linux vai o tema
/// resolvido, para as decorações acompanharem o conteúdo.
pub fn window_theme(choice: Appearance, platform: Option<Theme>) -> SystemTheme {
    window_theme_on(cfg!(target_os = "linux"), choice, platform)
}

fn window_theme_on(linux: bool, choice: Appearance, platform: Option<Theme>) -> SystemTheme {
    match (linux, resolve(choice, platform)) {
        (false, _) if choice == Appearance::System => SystemTheme::SystemDefault,
        (_, ThemePreference::System) => SystemTheme::SystemDefault,
        (_, ThemePreference::Light) => SystemTheme::Light,
        (_, ThemePreference::Dark) => SystemTheme::Dark,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const THEMES: [(Theme, &Palette); 2] = [(Theme::Dark, &DARK), (Theme::Light, &LIGHT)];

    #[test]
    fn visuals_map_every_token_for_each_theme() {
        for (theme, p) in THEMES {
            let v = visuals(theme);
            assert_eq!(v.dark_mode, theme == Theme::Dark, "{theme:?}");
            assert_eq!(v.panel_fill, p.bg, "{theme:?} panel_fill");
            assert_eq!(v.extreme_bg_color, p.bg, "{theme:?} extreme_bg_color");
            assert_eq!(v.window_fill, p.surf, "{theme:?} window_fill");
            assert_eq!(v.faint_bg_color, p.surf2, "{theme:?} faint_bg_color");
            assert_eq!(v.text_edit_bg_color, Some(p.surf2), "{theme:?}");
            assert_eq!(v.text_edit_bg_color(), p.surf2, "{theme:?}");
            assert_eq!(v.widgets.inactive.bg_fill, p.surf2, "{theme:?}");
            assert_eq!(v.widgets.inactive.weak_bg_fill, p.surf2, "{theme:?}");
            let line = Stroke::new(1.0_f32, p.line);
            assert_eq!(v.window_stroke, line, "{theme:?} window_stroke");
            assert_eq!(v.widgets.noninteractive.bg_stroke, line, "{theme:?}");
            assert_eq!(v.widgets.inactive.bg_stroke, line, "{theme:?}");
            assert_eq!(v.override_text_color, Some(p.text), "{theme:?}");
            assert_eq!(v.text_color(), p.text, "{theme:?}");
            assert_eq!(v.weak_text_color(), p.text2, "{theme:?}");
            assert_eq!(v.hyperlink_color, p.acc, "{theme:?}");
            assert_eq!(v.selection.stroke, Stroke::new(1.0_f32, p.acc), "{theme:?}");
            assert_eq!(v.selection.bg_fill, p.sel, "{theme:?}");
            assert_eq!(v.warn_fg_color, p.warn, "{theme:?}");
            assert_eq!(v.error_fg_color, p.bad, "{theme:?}");
            assert_eq!(palette(&v), p, "palette() must follow dark_mode");
        }
        assert_eq!(palette(&Visuals::dark()), &DARK);
        assert_eq!(palette(&Visuals::light()), &LIGHT);
    }

    /// Contraste WCAG 2.x entre duas cores opacas.
    pub(crate) fn contrast(a: Color32, b: Color32) -> f64 {
        let luminance = |c: Color32| {
            let [r, g, b] = [c.r(), c.g(), c.b()].map(|v| {
                let v = f64::from(v) / 255.;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            });
            0.2126 * r + 0.7152 * g + 0.0722 * b
        };
        let (hi, lo) = {
            let (x, y) = (luminance(a), luminance(b));
            if x > y { (x, y) } else { (y, x) }
        };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn every_foreground_token_is_readable_as_text() {
        // Nesta fase todos esses tokens também pintam texto (valores dos
        // indicadores, legendas), então valem os 4,5:1 de texto normal.
        for (name, p) in [("escuro", &DARK), ("claro", &LIGHT)] {
            for (fg, color) in [
                ("text", p.text),
                ("text2", p.text2),
                ("acc", p.acc),
                ("series2", p.series2),
                ("warn", p.warn),
                ("bad", p.bad),
                ("info", p.info),
            ] {
                for (bg, back) in [("bg", p.bg), ("surf", p.surf), ("surf2", p.surf2)] {
                    let ratio = contrast(color, back);
                    assert!(
                        ratio >= 4.5,
                        "{name}: {fg} sobre {bg} tem {ratio:.2}:1, abaixo de 4,5:1"
                    );
                }
            }
        }
    }

    #[test]
    fn contrast_matches_known_wcag_values() {
        assert_eq!(contrast(Color32::BLACK, Color32::WHITE), 21.0);
        assert_eq!(contrast(Color32::WHITE, Color32::WHITE), 1.0);
        let gray = contrast(Color32::from_rgb(0x76, 0x76, 0x76), Color32::WHITE);
        assert!((gray - 4.54).abs() < 0.01, "{gray}");
    }

    #[test]
    fn selection_is_translucent_accent() {
        for (p, alpha) in [(&DARK, 33), (&LIGHT, 26)] {
            assert_eq!(p.sel.a(), alpha);
            assert_eq!(
                p.sel,
                Color32::from_rgba_unmultiplied(p.acc.r(), p.acc.g(), p.acc.b(), alpha)
            );
        }
    }

    #[test]
    fn both_styles_keep_spacing_and_text_sizes() {
        for (theme, _) in THEMES {
            let s = style(theme);
            assert_eq!(s.spacing.item_spacing, Vec2::new(10., 10.), "{theme:?}");
            assert_eq!(s.spacing.button_padding, Vec2::new(12., 8.), "{theme:?}");
            for (text, size) in [
                (TextStyle::Body, 14.),
                (TextStyle::Button, 14.),
                (TextStyle::Small, 12.),
            ] {
                assert_eq!(s.text_styles[&text], FontId::proportional(size), "{text:?}");
            }
            let default_heading = egui::Style::default().text_styles[&TextStyle::Heading].size;
            assert_eq!(
                s.text_styles[&TextStyle::Heading],
                FontId::new(default_heading, semibold())
            );
        }
    }

    #[test]
    fn fonts_put_inter_and_jetbrains_first_and_keep_egui_fallbacks_in_order() {
        let default = FontDefinitions::default();
        let fonts = fonts();
        let with_first = |first: &str, family: &FontFamily| {
            let mut v = vec![first.to_owned()];
            v.extend(default.families[family].iter().cloned());
            v
        };
        assert_eq!(
            fonts.families[&FontFamily::Proportional],
            with_first("Inter-Regular", &FontFamily::Proportional)
        );
        assert_eq!(
            fonts.families[&FontFamily::Monospace],
            with_first("JetBrainsMonoNL-Regular", &FontFamily::Monospace)
        );
        assert_eq!(
            fonts.families[&semibold()],
            with_first("Inter-SemiBold", &FontFamily::Proportional)
        );
        for name in fonts.families.values().flatten() {
            assert!(
                fonts.font_data.contains_key(name),
                "missing data for {name}"
            );
        }
    }

    #[test]
    fn install_sets_both_styles_and_fonts_that_render() {
        let ctx = egui::Context::default();
        install(&ctx);
        for (theme, p) in THEMES {
            assert_eq!(ctx.style_of(theme).visuals.panel_fill, p.bg, "{theme:?}");
        }
        // Um frame com texto nas três famílias carrega e rasteriza as fontes.
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Aparência");
                ui.label("Olá 1.234,5 ✓");
                ui.monospace("0O1l");
            });
        });
        ctx.fonts(|f| {
            assert!(f.families().contains(&semibold()));
            assert_eq!(
                f.definitions().families[&FontFamily::Proportional][0],
                "Inter-Regular"
            );
        });
    }

    const CHOICES: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];
    const PORTALS: [Option<u32>; 6] = [None, Some(0), Some(1), Some(2), Some(3), Some(u32::MAX)];
    /// Leituras possíveis do NSApp no macOS.
    const MACOS: [Option<Theme>; 3] = [None, Some(Theme::Dark), Some(Theme::Light)];

    #[test]
    fn resolution_matrix_on_linux_and_macos() {
        use Appearance::*;
        use ThemePreference as P;
        for choice in CHOICES {
            match choice {
                System | Light | Dark => {}
            }
            for platform in MACOS {
                let want = match (choice, platform) {
                    (Light, _) => P::Light,
                    (Dark, _) => P::Dark,
                    (System, Some(Theme::Dark)) => P::Dark,
                    (System, Some(Theme::Light)) => P::Light,
                    (System, None) => P::System,
                };
                assert_eq!(
                    resolve(choice, platform),
                    want,
                    "macos {choice:?} {platform:?}"
                );
            }
            for portal in PORTALS {
                let want = match (choice, portal) {
                    (Light, _) => P::Light,
                    (Dark, _) => P::Dark,
                    (System, Some(1)) => P::Dark,
                    (System, Some(2)) => P::Light,
                    (System, Some(0)) => P::Light,
                    (System, None | Some(3) | Some(u32::MAX)) => P::Dark,
                    (System, Some(other)) => unreachable!("not in matrix: {other}"),
                };
                assert_eq!(
                    resolve(choice, Some(portal_theme(portal))),
                    want,
                    "linux {choice:?} {portal:?}"
                );
            }
        }
    }

    #[test]
    fn window_theme_matrix_on_linux_and_macos() {
        use Appearance::*;
        use SystemTheme as W;
        for choice in CHOICES {
            match choice {
                System | Light | Dark => {}
            }
            for platform in MACOS {
                let want = match choice {
                    System => W::SystemDefault,
                    Light => W::Light,
                    Dark => W::Dark,
                };
                assert_eq!(
                    window_theme_on(false, choice, platform),
                    want,
                    "macos {choice:?} {platform:?}"
                );
            }
            for portal in PORTALS {
                let want = match (choice, portal) {
                    (Light, _) | (System, Some(0 | 2)) => W::Light,
                    (Dark, _) | (System, None | Some(1 | 3 | u32::MAX)) => W::Dark,
                    (System, Some(other)) => unreachable!("not in matrix: {other}"),
                };
                assert_eq!(
                    window_theme_on(true, choice, Some(portal_theme(portal))),
                    want,
                    "linux {choice:?} {portal:?}"
                );
            }
        }
        assert_eq!(
            window_theme(System, None),
            window_theme_on(cfg!(target_os = "linux"), System, None)
        );
    }

    /// A leitura ObjC só acontece na thread principal, onde os testes não
    /// rodam; aqui se exercita de verdade o caminho sem leitura.
    #[test]
    #[cfg(target_os = "macos")]
    fn macos_reading_off_the_main_thread_falls_back_to_egui() {
        assert!(
            objc2_foundation::MainThreadMarker::new().is_none(),
            "precondition: tests run off the main thread"
        );
        assert_eq!(platform_theme(), None);
        assert_eq!(
            resolve(Appearance::System, platform_theme()),
            ThemePreference::System
        );
    }
}
