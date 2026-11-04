//! Status-bar presentation is independent of the dashboard's paint loop.
//! The item (pulse, the mode's text and the alert dot) and the menu render
//! `menu::entries`; nothing here decides what the menu says.

#[cfg(target_os = "macos")]
mod native {
    use crate::{
        menu::{self, Entry, MenuAction, MenuReading, Row, State},
        model::BarMode,
        monitor::Shared,
        theme,
    };
    use chrono::Utc;
    use eframe::egui::{Color32, Context, ViewportCommand};
    use objc2::{
        AllocAnyThread, DefinedClass, MainThreadOnly, define_class, msg_send,
        rc::Retained,
        runtime::{AnyObject, ProtocolObject},
        sel,
    };
    use objc2_app_kit::{
        NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
        NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSBezierPath, NSBitmapImageRep,
        NSBox, NSBoxType, NSCellImagePosition, NSColor, NSDeviceRGBColorSpace, NSFont,
        NSFontAttributeName, NSFontWeight, NSFontWeightMedium, NSFontWeightRegular,
        NSFontWeightSemibold, NSForegroundColorAttributeName, NSGraphicsContext, NSImage,
        NSImageScaling, NSImageView, NSLineBreakMode, NSLineCapStyle, NSLineJoinStyle, NSMenu,
        NSMenuItem, NSStatusBar, NSStatusBarButton, NSStatusItem, NSTextAlignment, NSTextField,
        NSTitlePosition, NSVariableStatusItemLength, NSView,
    };
    use objc2_foundation::{
        MainThreadMarker, NSActivityOptions, NSArray, NSAttributedString, NSDictionary,
        NSMutableAttributedString, NSObject, NSObjectProtocol, NSPoint, NSProcessInfo, NSRect,
        NSRunLoop, NSRunLoopCommonModes, NSSize, NSString, NSTimer, ns_string,
    };
    use std::{
        cell::{Cell, RefCell},
        sync::{Arc, atomic::Ordering},
    };

    /// O menu de MenuBar.dc.html: 318 de largura (o mínimo; as views seguem
    /// o menu quando ele é mais largo), o texto a 15 das bordas (5 da margem
    /// do menu e 10 da linha), onde terminam também os atalhos.
    const WIDTH: f64 = 318.;
    const INSET: f64 = 15.;
    /// As colunas de CPU e memória das métricas, alinhadas à direita, a 14
    /// uma da outra.
    const CPU: f64 = 64.;
    const MEMORY: f64 = 80.;
    const GAP: f64 = 14.;
    /// O ícone na barra e o ponto de alerta, em pontos.
    const ICON: f64 = 15.;
    const DOT: f64 = 6.;

    /// O item nativo de cada linha de `menu::entries`, na mesma ordem.
    enum Native {
        Header {
            item: Retained<NSMenuItem>,
            dot: Retained<NSBox>,
            state: Retained<NSTextField>,
        },
        Metric {
            item: Retained<NSMenuItem>,
            name: Retained<NSTextField>,
            cpu: Retained<NSTextField>,
            memory: Retained<NSTextField>,
            /// "indisponível", nas duas colunas juntas, no lugar delas.
            missing: Retained<NSTextField>,
        },
        Alert {
            item: Retained<NSMenuItem>,
            band: Retained<NSBox>,
            icon: Option<Retained<NSImageView>>,
            text: Retained<NSTextField>,
        },
        Separator(Retained<NSMenuItem>),
        Action(Retained<NSMenuItem>),
    }
    impl Native {
        fn item(&self) -> &NSMenuItem {
            match self {
                Native::Header { item, .. }
                | Native::Metric { item, .. }
                | Native::Alert { item, .. }
                | Native::Separator(item)
                | Native::Action(item) => item,
            }
        }
    }

    struct MenuState {
        shared: Arc<Shared>,
        ctx: Context,
        visible: Cell<bool>,
        status: Retained<NSStatusItem>,
        /// O ponto de alerta, sobre o canto superior direito do pulso.
        dot: Retained<NSBox>,
        rows: Vec<Native>,
        /// A ação de cada item habilitado, pela `tag` dele.
        actions: Vec<MenuAction>,
        /// A última leitura aplicada, com o modo e a aparência (escura?).
        previous: RefCell<Option<(MenuReading, BarMode, bool)>>,
        trace_remaining: Cell<u32>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements. All AppKit access is
        // confined to the main thread by MainThreadOnly and the main run loop.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = MenuState]
        struct MenuActions;
        unsafe impl NSObjectProtocol for MenuActions {}
        impl MenuActions {
            // AppKit's target/action ABI: void (id sender); the sender is the
            // item, whose tag indexes `actions`.
            #[unsafe(method(perform:))]
            fn perform(&self, sender: Option<&NSMenuItem>) {
                let state = self.ivars();
                let Some(action) = sender.and_then(|s| state.actions.get(s.tag() as usize)) else {
                    return;
                };
                match *action {
                    MenuAction::Open(destination) => {
                        if destination.code() != 0 {
                            state.shared.navigation.store(destination.code(), Ordering::Release);
                        }
                        self.show_dashboard();
                    }
                    MenuAction::Quit => {
                        state.shared.stop.store(true, Ordering::Relaxed);
                        // Explicit Quit uses the application's native termination path,
                        // independently of the dashboard's close-to-hide behavior.
                        NSApplication::sharedApplication(self.mtm()).terminate(None);
                    }
                }
            }
            #[unsafe(method(refreshMetrics:))]
            fn refresh_metrics(&self,_timer:&NSTimer) {self.refresh();}
        }
    );

    impl MenuActions {
        fn show_dashboard(&self) {
            let state = self.ivars();
            state.visible.set(true);
            let app = NSApplication::sharedApplication(self.mtm());
            for window in app
                .windows()
                .iter()
                .filter(|w| w.title().to_string() == "AI Monitor")
            {
                window.deminiaturize(None);
                window.makeKeyAndOrderFront(None);
            }
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
            state.ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            state
                .ctx
                .send_viewport_cmd(ViewportCommand::Minimized(false));
            state.ctx.send_viewport_cmd(ViewportCommand::Focus);
            state.ctx.request_repaint();
        }
        fn hide_dashboard(&self) {
            let state = self.ivars();
            state.visible.set(false);
            for window in NSApplication::sharedApplication(self.mtm())
                .windows()
                .iter()
                .filter(|w| w.title().to_string() == "AI Monitor")
            {
                window.orderOut(None);
            }
            state.ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            state.ctx.request_repaint();
        }
        fn new(mtm: MainThreadMarker, state: MenuState) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(state);
            // SAFETY: NSObject's designated initializer and correct superclass.
            unsafe { msg_send![super(this), init] }
        }
        fn refresh(&self) {
            // Never wait for a collector or a configuration change on AppKit:
            // a busy lock skips the tick and keeps the last reading.
            let Some((reading, mode)) = menu::read(&self.ivars().shared, Utc::now()) else {
                return;
            };
            self.apply(reading, mode);
        }
        /// Shows `reading` on the item and in the menu (only what changed).
        fn apply(&self, reading: MenuReading, mode: BarMode) {
            let state = self.ivars();
            let mtm = self.mtm();
            let current = (reading, mode, dark(mtm));
            let Some(button) = state.status.button(mtm) else {
                return;
            };
            let mut previous = state.previous.borrow_mut();
            if previous.as_ref() != Some(&current) {
                let (reading, mode, dark) = &current;
                let title = menu::bar_title(reading, *mode);
                if previous
                    .as_ref()
                    .is_none_or(|(r, m, _)| menu::bar_title(r, *m) != title)
                {
                    button.setTitle(&NSString::from_str(&title.text));
                    button.setImagePosition(if title.text.is_empty() {
                        NSCellImagePosition::ImageOnly
                    } else {
                        NSCellImagePosition::ImageLeft
                    });
                    // Esmaecido como o sistema esmaece: ícone e texto.
                    button.setAppearsDisabled(title.dimmed);
                }
                button.setToolTip(Some(&NSString::from_str(&menu::tooltip(reading))));
                state.dot.setHidden(!reading.alert_dot);
                for (entry, row) in menu::entries(reading).iter().zip(&state.rows) {
                    update(row, entry, *dark);
                }
                *previous = Some(current);
            }
            // O botão muda de largura depois do título; o ponto acompanha.
            place_dot(&button, &state.dot);
            if state.trace_remaining.get() > 0 {
                state.trace_remaining.set(state.trace_remaining.get() - 1);
                let (reading, mode) = previous
                    .as_ref()
                    .map(|(r, m, _)| (Some(r), *m))
                    .unwrap_or((None, BarMode::default()));
                // Opt-in, bounded diagnostics read the actual AppKit state;
                // no prompts, handoffs, credentials, projects or paths.
                println!(
                    "{}",
                    serde_json::json!({
                        "at":Utc::now(),"menu_title":button.title().to_string(),
                        "bar_mode":mode,"dimmed":button.appearsDisabled(),
                        "alert_dot":!state.dot.isHidden(),
                        "status_visible":state.status.isVisible(),
                        "panel_visible":NSApplication::sharedApplication(mtm).windows().iter()
                            .any(|w|w.title().to_string()=="AI Monitor"&&w.isVisible()),
                        "history_samples":state.shared.history.try_lock().ok().map(|h|h.len()),
                        "state":reading.map(|r| r.state.text()),
                    })
                );
            }
        }
    }

    /// A aparência do sistema (o menu e o item a seguem; a de Ajustes vale
    /// só para as janelas do app).
    fn dark(mtm: MainThreadMarker) -> bool {
        // SAFETY: AppKit's appearance name constants.
        let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        NSApplication::sharedApplication(mtm)
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[aqua, dark]))
            .is_some_and(|name| &*name == dark)
    }
    fn palette(dark: bool) -> &'static theme::Palette {
        if dark { &theme::DARK } else { &theme::LIGHT }
    }
    fn color(c: Color32, alpha: f64) -> Retained<NSColor> {
        let channel = |v: u8| v as f64 / 255.;
        NSColor::colorWithSRGBRed_green_blue_alpha(
            channel(c.r()),
            channel(c.g()),
            channel(c.b()),
            alpha,
        )
    }
    fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }
    fn label(
        mtm: MainThreadMarker,
        size: f64,
        weight: NSFontWeight,
        color: &NSColor,
        alignment: NSTextAlignment,
        frame: NSRect,
    ) -> Retained<NSTextField> {
        let field = NSTextField::labelWithString(ns_string!(""), mtm);
        field.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
            size, weight,
        )));
        field.setTextColor(Some(color));
        field.setAlignment(alignment);
        field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        field.setFrame(frame);
        field
    }
    /// Um retângulo preenchido de cantos `radius` (NSBox custom: fundo e raio
    /// sem camada).
    fn fill(mtm: MainThreadMarker, frame: NSRect, radius: f64) -> Retained<NSBox> {
        let fill = NSBox::initWithFrame(NSBox::alloc(mtm), frame);
        fill.setBoxType(NSBoxType::Custom);
        fill.setTitlePosition(NSTitlePosition::NoTitle);
        fill.setBorderWidth(0.);
        fill.setCornerRadius(radius);
        fill.setContentViewMargins(NSSize::new(0., 0.));
        fill
    }
    /// Uma linha informativa: uma view de `height` que o menu estica até a
    /// largura dele; cada filha diz com `autoresizing` como acompanha.
    fn view_item(mtm: MainThreadMarker, height: f64, children: &[&NSView]) -> Retained<NSMenuItem> {
        let view = NSView::initWithFrame(NSView::alloc(mtm), rect(0., 0., WIDTH, height));
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        for child in children {
            view.addSubview(child);
        }
        let item = item(mtm, "", None);
        item.setView(Some(&view));
        item
    }

    /// O item nativo de `entry`, ainda sem os valores (`update`). As ações
    /// entram em `actions`, e a `tag` do item é o índice.
    fn build(mtm: MainThreadMarker, entry: &Entry, actions: &mut Vec<MenuAction>) -> Native {
        // SAFETY: AppKit's font weight constants.
        let (regular, semibold) = unsafe { (NSFontWeightRegular, NSFontWeightSemibold) };
        let left = NSTextAlignment::Left;
        let right = NSTextAlignment::Right;
        match &entry.row {
            Row::Header(_) => {
                let name = label(
                    mtm,
                    13.,
                    semibold,
                    &NSColor::labelColor(),
                    left,
                    rect(INSET, 4., 150., 17.),
                );
                name.setStringValue(ns_string!("AI Monitor"));
                let state = label(
                    mtm,
                    11.5,
                    regular,
                    &NSColor::secondaryLabelColor(),
                    right,
                    rect(0., 5., 0., 15.),
                );
                let dot = fill(mtm, rect(0., 0., DOT, DOT), DOT / 2.);
                // O estado fica preso à borda direita quando o menu alarga.
                state.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
                dot.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
                Native::Header {
                    item: view_item(mtm, 26., &[&name, &dot, &state]),
                    dot,
                    state,
                }
            }
            Row::Metric { name, .. } => {
                // A grade do artboard: 4 acima da linha do ai-memory, 3 entre
                // as duas e 6 abaixo da do Docker.
                let (height, y) = if name == "ai-memory" {
                    (21., 1.)
                } else {
                    (24., 6.)
                };
                let name = label(
                    mtm,
                    12.5,
                    regular,
                    &NSColor::labelColor(),
                    left,
                    rect(INSET, y, WIDTH - 2. * INSET - CPU - MEMORY - 2. * GAP, 16.),
                );
                let cpu = label(
                    mtm,
                    12.5,
                    regular,
                    &NSColor::labelColor(),
                    right,
                    rect(WIDTH - INSET - MEMORY - GAP - CPU, y, CPU, 16.),
                );
                let memory = label(
                    mtm,
                    12.5,
                    regular,
                    &NSColor::secondaryLabelColor(),
                    right,
                    rect(WIDTH - INSET - MEMORY, y, MEMORY, 16.),
                );
                let missing = label(
                    mtm,
                    12.5,
                    regular,
                    &NSColor::secondaryLabelColor(),
                    right,
                    rect(
                        WIDTH - INSET - MEMORY - GAP - CPU,
                        y,
                        CPU + GAP + MEMORY,
                        16.,
                    ),
                );
                missing.setStringValue(&NSString::from_str(menu::UNAVAILABLE));
                // O nome ganha a largura extra; as colunas ficam à direita.
                name.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
                for column in [&cpu, &memory, &missing] {
                    column.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
                }
                Native::Metric {
                    item: view_item(mtm, height, &[&name, &cpu, &memory, &missing]),
                    name,
                    cpu,
                    memory,
                    missing,
                }
            }
            Row::Alert(_) => {
                // A faixa: 2 acima e 4 abaixo, a 5 das bordas do menu, raio 6;
                // estica com ele.
                let band = fill(mtm, rect(5., 4., WIDTH - 10., 27.), 6.);
                band.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
                // SF Symbol (macOS 11+); sem ele, a faixa fica só com o texto.
                let icon = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                    ns_string!("exclamationmark.triangle"),
                    None,
                )
                .map(|image| {
                    let icon = NSImageView::imageViewWithImage(&image, mtm);
                    icon.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
                    icon.setFrame(rect(13., 11., 13., 13.));
                    icon
                });
                let text = label(
                    mtm,
                    12.,
                    regular,
                    &NSColor::labelColor(),
                    left,
                    rect(13. + 13. + 6., 9.5, WIDTH - 32. - 13., 16.),
                );
                text.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
                let mut children: Vec<&NSView> = vec![&band];
                if let Some(icon) = &icon {
                    children.push(icon);
                }
                children.push(&text);
                Native::Alert {
                    item: view_item(mtm, 33., &children),
                    band,
                    icon,
                    text,
                }
            }
            Row::Separator => Native::Separator(NSMenuItem::separatorItem(mtm)),
            Row::Action { label, action, .. } => {
                let native = item(mtm, label, Some(sel!(perform:)));
                native.setTag(actions.len() as isize);
                // Os atalhos do artboard (⌘O, ⌘, e ⌘Q), com o menu aberto.
                let key = match action {
                    MenuAction::Open(menu::Destination::Panel) => "o",
                    MenuAction::Open(menu::Destination::Settings) => ",",
                    MenuAction::Quit => "q",
                    MenuAction::Open(_) => "",
                };
                native.setKeyEquivalent(&NSString::from_str(key));
                actions.push(*action);
                Native::Action(native)
            }
        }
    }

    /// Aplica `entry` ao item nativo dela, com as cores da aparência.
    fn update(native: &Native, entry: &Entry, dark: bool) {
        let p = palette(dark);
        native.item().setHidden(!entry.visible);
        match (native, &entry.row) {
            (Native::Header { dot, state, .. }, Row::Header(reading)) => {
                state.setStringValue(&NSString::from_str(&reading.text()));
                state.sizeToFit();
                let size = state.frame().size;
                // A largura que o menu deu à view, não a mínima.
                let width = native
                    .item()
                    .view()
                    .map_or(WIDTH, |view| view.frame().size.width);
                state.setFrameOrigin(NSPoint::new(width - INSET - size.width, 5.));
                dot.setFrameOrigin(NSPoint::new(
                    width - INSET - size.width - 6. - DOT,
                    5. + (size.height - DOT) / 2.,
                ));
                let tone = match reading {
                    State::Live(_) => p.acc,
                    State::Late(_) => p.warn,
                    State::DockerDown(_) => p.bad,
                    State::Waiting => p.text2,
                };
                dot.setFillColor(&color(tone, 1.));
            }
            (
                Native::Metric {
                    name: field,
                    cpu,
                    memory,
                    missing,
                    ..
                },
                Row::Metric { name, value },
            ) => {
                field.setStringValue(&NSString::from_str(name));
                // Sem medida, "indisponível" inteiro na borda das colunas.
                if let Some((c, m)) = value {
                    cpu.setStringValue(&NSString::from_str(c));
                    memory.setStringValue(&NSString::from_str(m));
                }
                cpu.setHidden(value.is_none());
                memory.setHidden(value.is_none());
                missing.setHidden(value.is_some());
            }
            (
                Native::Alert {
                    band, icon, text, ..
                },
                Row::Alert(alert),
            ) => {
                text.setStringValue(&NSString::from_str(alert));
                text.setTextColor(Some(&color(p.warn, 1.)));
                band.setFillColor(&color(p.warn, if dark { 0.14 } else { 0.12 }));
                if let Some(icon) = icon {
                    icon.setContentTintColor(Some(&color(p.warn, 1.)));
                }
            }
            (Native::Action(item), Row::Action { label, detail, .. }) if entry.dynamic => {
                let (main, rest) = menu::action_text(label, detail.as_deref());
                match rest {
                    Some(rest) => item.setAttributedTitle(Some(&secondary_tail(&main, &rest))),
                    None => {
                        item.setAttributedTitle(None);
                        item.setTitle(&NSString::from_str(&main));
                    }
                }
            }
            _ => {}
        }
    }

    /// `main` na cor do menu e, logo depois, `rest` (o projeto das propostas)
    /// em cor secundária, na mesma fonte: o item segue nativo, sem tabulação.
    fn secondary_tail(main: &str, rest: &str) -> Retained<NSAttributedString> {
        let font = NSFont::menuFontOfSize(0.);
        let font: &NSFont = &font;
        let part = |text: &str, color: &NSColor| {
            // SAFETY: AppKit's attribute keys with values of their types.
            unsafe {
                let keys = [NSFontAttributeName, NSForegroundColorAttributeName];
                let values: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
                NSAttributedString::initWithString_attributes(
                    NSAttributedString::alloc(),
                    &NSString::from_str(text),
                    Some(&NSDictionary::from_slices(&keys, &values)),
                )
            }
        };
        let title = NSMutableAttributedString::new();
        title.appendAttributedString(&part(main, &NSColor::labelColor()));
        title.appendAttributedString(&part(rest, &NSColor::secondaryLabelColor()));
        Retained::into_super(title)
    }

    /// O pulso de `menu::PULSE` como imagem template de 15 pt (1×, 2× e 3×):
    /// o AppKit o pinta com a cor da barra, clara ou escura.
    fn pulse() -> Retained<NSImage> {
        let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(ICON, ICON));
        for scale in [1., 2., 3.] {
            let px = (ICON * scale) as isize;
            // SAFETY: null planes make AppKit allocate the pixels; 8-bit RGBA.
            let rep = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(),
                    std::ptr::null_mut(),
                    px,
                    px,
                    8,
                    4,
                    true,
                    false,
                    NSDeviceRGBColorSpace,
                    0,
                    0,
                )
            };
            let Some(rep) = rep else {
                continue;
            };
            if let Some(context) = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep) {
                NSGraphicsContext::saveGraphicsState_class();
                NSGraphicsContext::setCurrentContext(Some(&context));
                let unit = px as f64 / 24.;
                let path = NSBezierPath::bezierPath();
                for (i, &(x, y)) in menu::PULSE.iter().enumerate() {
                    // O path do artboard desce; o bitmap sobe.
                    let point = NSPoint::new(x as f64 * unit, (24. - y as f64) * unit);
                    if i == 0 {
                        path.moveToPoint(point);
                    } else {
                        path.lineToPoint(point);
                    }
                }
                path.setLineWidth(menu::PULSE_WIDTH as f64 * unit);
                path.setLineCapStyle(NSLineCapStyle::Round);
                path.setLineJoinStyle(NSLineJoinStyle::Round);
                NSColor::blackColor().setStroke();
                path.stroke();
                NSGraphicsContext::restoreGraphicsState_class();
            }
            rep.setSize(NSSize::new(ICON, ICON));
            image.addRepresentation(&rep);
        }
        image.setTemplate(true);
        image
    }

    /// O ponto de 6 no canto superior direito do pulso, onde o botão o
    /// desenha agora (o lado do ícone muda com o modo).
    fn place_dot(button: &NSStatusBarButton, dot: &NSBox) {
        let Some(cell) = button.cell() else {
            return;
        };
        let image = cell.imageRectForBounds(button.bounds());
        let (cx, cy) = (
            image.origin.x + image.size.width / 2.,
            image.origin.y + image.size.height / 2.,
        );
        let top = if button.isFlipped() {
            cy - ICON / 2.
        } else {
            cy + ICON / 2. - DOT
        };
        dot.setFrame(rect(cx + ICON / 2. - DOT, top, DOT, DOT));
    }

    pub struct MenuBar {
        actions: Retained<MenuActions>,
        timer: Retained<NSTimer>,
        activity: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    }
    impl MenuBar {
        pub fn new(ctx: Context, shared: Arc<Shared>, show_dashboard: bool) -> Option<Self> {
            let mtm = MainThreadMarker::new()?;
            let status =
                NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
            let Some(button) = status.button(mtm) else {
                NSStatusBar::systemStatusBar().removeStatusItem(&status);
                return None;
            };
            // SAFETY: AppKit's font weight constant.
            let medium = unsafe { NSFontWeightMedium };
            button.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                12.5, medium,
            )));
            button.setImage(Some(&pulse()));
            let (r, g, b) = menu::ALERT_DOT;
            let dot = fill(mtm, rect(0., 0., DOT, DOT), DOT / 2.);
            dot.setFillColor(&color(Color32::from_rgb(r, g, b), 1.));
            dot.setHidden(true);
            button.addSubview(&dot);
            let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("AI Monitor"));
            menu.setAutoenablesItems(false);
            // Sem a coluna de marcação: o texto começa onde começam as views.
            menu.setShowsStateColumn(false);
            // A estrutura vem da leitura provisória; `refresh` aplica a real.
            let mut actions = Vec::new();
            let empty = MenuReading::waiting(Utc::now());
            let rows: Vec<Native> = menu::entries(&empty)
                .iter()
                .map(|entry| build(mtm, entry, &mut actions))
                .collect();
            let actions = MenuActions::new(
                mtm,
                MenuState {
                    shared,
                    ctx,
                    visible: Cell::new(show_dashboard),
                    status: status.clone(),
                    dot,
                    rows,
                    actions,
                    previous: RefCell::new(None),
                    trace_remaining: Cell::new(if std::env::args().any(|a| a == "--trace-menu") {
                        120
                    } else {
                        0
                    }),
                },
            );
            for row in &actions.ivars().rows {
                if let Native::Action(entry) = row {
                    // SAFETY: the retained target implements perform: with the
                    // action ABI above and outlives the menu and its timer.
                    unsafe {
                        entry.setTarget(Some(&actions));
                    }
                }
                menu.addItem(row.item());
            }
            status.setMenu(Some(&menu));
            // SAFETY: refreshMetrics: takes exactly one NSTimer argument. Both
            // target and timer stay retained; scheduling is on the main loop only.
            let timer = unsafe {
                let timer = NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                    1.,
                    &actions,
                    sel!(refreshMetrics:),
                    None,
                    true,
                );
                NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
                timer
            };
            NSApplication::sharedApplication(mtm)
                .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            // A provisória primeiro: com um lock ocupado no 1º tique, o menu
            // já tem nome nas métricas e estado no cabeçalho.
            let mode = actions
                .ivars()
                .shared
                .config
                .try_read()
                .map_or(BarMode::default(), |c| c.bar_mode);
            actions.apply(empty, mode);
            actions.refresh();
            // Keep real-time sampling eligible while the dashboard is hidden,
            // without preventing display sleep or normal system sleep.
            let activity = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
                ns_string!("Monitoramento de CPU e memória na barra de menus"),
            );
            Some(Self {
                actions,
                timer,
                activity,
            })
        }
        pub fn is_visible(&self) -> bool {
            self.actions.ivars().visible.get()
        }
        pub fn hide(&self) {
            self.actions.hide_dashboard();
        }
    }
    impl Drop for MenuBar {
        fn drop(&mut self) {
            self.timer.invalidate();
            NSStatusBar::systemStatusBar().removeStatusItem(&self.actions.ivars().status);
            // SAFETY: this exact activity token was created above and is ended once.
            unsafe {
                NSProcessInfo::processInfo().endActivity(&self.activity);
            }
        }
    }
    fn item(
        mtm: MainThreadMarker,
        title: &str,
        selector: Option<objc2::runtime::Sel>,
    ) -> Retained<NSMenuItem> {
        // SAFETY: action selectors are implemented by MenuActions; nil is used
        // for informational rows. Target is assigned before a menu can open.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                selector,
                ns_string!(""),
            )
        };
        item.setEnabled(selector.is_some());
        item
    }
}
#[cfg(target_os = "macos")]
pub use native::MenuBar;
