//! Status-bar presentation is independent of the dashboard's paint loop.
use crate::model::{ResourceSnapshot, Usage, bytes};
use chrono::{DateTime, Utc};

#[derive(Debug, PartialEq)]
pub struct MenuReading {
    pub title: String,
    pub ai: String,
    pub docker: String,
    pub freshness: String,
}

impl MenuReading {
    pub fn from_snapshot(
        snapshot: &ResourceSnapshot,
        container: &str,
        interval: u64,
        now: DateTime<Utc>,
    ) -> Self {
        let age = snapshot.at.map(|at| (now - at).num_seconds().max(0));
        let fresh = age.is_some_and(|age| age <= (interval.saturating_mul(3).max(10)) as i64);
        let ai = fresh.then(|| snapshot.ai_usage(container)).flatten();
        let docker = fresh.then(|| snapshot.docker_total()).flatten();
        Self {
            title: format!(
                "AI {}  |  Docker {}",
                compact(ai.as_ref()),
                compact(docker.as_ref())
            ),
            ai: detail("ai-memory", ai.as_ref()),
            docker: detail("Docker (contêineres)", docker.as_ref()),
            freshness: match age {
                None => "Aguardando a primeira coleta…".into(),
                Some(age) if !fresh => format!("Coleta atrasada: última atualização há {age} s"),
                Some(age) if snapshot.docker_error.is_some() => {
                    format!("Docker indisponível · tentativa há {age} s")
                }
                Some(age) => format!("Recursos atualizados há {age} s"),
            },
        }
    }
}

fn compact(usage: Option<&Usage>) -> String {
    usage
        .map(|u| format!("{:.0}% {}", u.cpu, compact_bytes(u.memory)))
        .unwrap_or("—% —".into())
}
fn compact_bytes(n: u64) -> String {
    if n >= 1 << 30 {
        format!("{:.1}G", n as f64 / (1u64 << 30) as f64)
    } else {
        format!("{:.0}M", n as f64 / (1u64 << 20) as f64)
    }
}
fn detail(name: &str, usage: Option<&Usage>) -> String {
    usage
        .map(|u| format!("{name} · CPU {:.1}% · memória {}", u.cpu, bytes(u.memory)))
        .unwrap_or_else(|| format!("{name} · métricas indisponíveis"))
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use crate::monitor::Shared;
    use eframe::egui::{Context, ViewportCommand};
    use objc2::{
        DefinedClass, MainThreadOnly, define_class, msg_send,
        rc::Retained,
        runtime::{AnyObject, ProtocolObject},
        sel,
    };
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSFont, NSMenu, NSMenuItem, NSStatusBar,
        NSStatusItem, NSVariableStatusItemLength,
    };
    use objc2_foundation::{
        MainThreadMarker, NSActivityOptions, NSObject, NSObjectProtocol, NSProcessInfo, NSRunLoop,
        NSRunLoopCommonModes, NSString, NSTimer, ns_string,
    };
    use std::{
        cell::{Cell, RefCell},
        sync::{Arc, atomic::Ordering},
    };

    struct MenuState {
        shared: Arc<Shared>,
        ctx: Context,
        visible: Cell<bool>,
        status: Retained<NSStatusItem>,
        ai: Retained<NSMenuItem>,
        docker: Retained<NSMenuItem>,
        freshness: Retained<NSMenuItem>,
        previous: RefCell<Option<MenuReading>>,
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
            // These selectors use AppKit's target/action ABI: void (id sender).
            #[unsafe(method(openPanel:))]
            fn open_panel(&self,_sender:Option<&AnyObject>) {
                let state=self.ivars();
                state.visible.set(true);
                let app=NSApplication::sharedApplication(self.mtm());
                for window in app.windows().iter().filter(|w|w.title().to_string()=="AI Monitor") {
                    window.deminiaturize(None);
                    window.makeKeyAndOrderFront(None);
                }
                #[allow(deprecated)]
                app.activateIgnoringOtherApps(true);
                state.ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                state.ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                state.ctx.send_viewport_cmd(ViewportCommand::Focus);
                state.ctx.request_repaint();
            }
            #[unsafe(method(hidePanel:))]
            fn hide_panel(&self,_sender:Option<&AnyObject>) {
                self.hide_dashboard();
            }
            #[unsafe(method(quitMonitor:))]
            fn quit_monitor(&self,_sender:Option<&AnyObject>) {
                self.ivars().shared.stop.store(true,Ordering::Relaxed);
                // Explicit Quit uses the application's native termination path,
                // independently of the dashboard's close-to-hide behavior.
                NSApplication::sharedApplication(self.mtm()).terminate(None);
            }
            #[unsafe(method(refreshMetrics:))]
            fn refresh_metrics(&self,_timer:&NSTimer) {self.refresh();}
        }
    );

    impl MenuActions {
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
            let state = self.ivars();
            let reading = {
                // Never wait for a collector or a configuration change on AppKit.
                let Ok(config) = state.shared.config.try_read() else {
                    return;
                };
                let Ok(snapshot) = state.shared.resources.try_lock() else {
                    return;
                };
                let mut reading = MenuReading::from_snapshot(
                    &snapshot,
                    &config.container,
                    config.resource_seconds,
                    Utc::now(),
                );
                if let Ok(health) = state.shared.health.try_lock() {
                    if health.fresh(Utc::now()) && !health.alerts().is_empty() {
                        reading.title.push_str("  !");
                    }
                    reading
                        .freshness
                        .push_str(&format!(" · {}", health.menu_summary(Utc::now())));
                }
                reading
            };
            let mut previous = state.previous.borrow_mut();
            if previous.as_ref() != Some(&reading) {
                if previous
                    .as_ref()
                    .is_none_or(|old| old.title != reading.title)
                    && let Some(button) = state.status.button(self.mtm())
                {
                    button.setTitle(&NSString::from_str(&reading.title));
                }
                state.ai.setTitle(&NSString::from_str(&reading.ai));
                state.docker.setTitle(&NSString::from_str(&reading.docker));
                state
                    .freshness
                    .setTitle(&NSString::from_str(&reading.freshness));
                if let Some(button) = state.status.button(self.mtm()) {
                    button.setToolTip(Some(&NSString::from_str(&format!("AI Monitor\n{}\n{}\n{}\nCPU: 100% = 1 núcleo. M/G = MiB/GiB. Docker inclui ai-memory.",reading.ai,reading.docker,reading.freshness))));
                }
                *previous = Some(reading);
            }
            if let Some(button) = state.status.button(self.mtm())
                && state.trace_remaining.get() > 0
            {
                state.trace_remaining.set(state.trace_remaining.get() - 1);
                let freshness = previous
                    .as_ref()
                    .map(|r| r.freshness.as_str())
                    .unwrap_or("");
                // Opt-in, bounded diagnostics read the actual AppKit state;
                // no prompts, handoffs, credentials or paths are included.
                println!(
                    "{}",
                    serde_json::json!({
                        "at":Utc::now(),"menu_title":button.title().to_string(),
                        "status_visible":state.status.isVisible(),
                        "panel_visible":NSApplication::sharedApplication(self.mtm()).windows().iter()
                            .any(|w|w.title().to_string()=="AI Monitor"&&w.isVisible()),
                        "history_samples":state.shared.history.try_lock().ok().map(|h|h.len()),
                        "freshness":freshness,
                    })
                );
            }
        }
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
            button.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                11., 0.,
            )));
            let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("AI Monitor"));
            menu.setAutoenablesItems(false);
            let ai = item(mtm, "ai-memory · aguardando", None);
            let docker = item(mtm, "Docker · aguardando", None);
            let freshness = item(mtm, "Aguardando a primeira coleta…", None);
            let actions = MenuActions::new(
                mtm,
                MenuState {
                    shared,
                    ctx,
                    visible: Cell::new(show_dashboard),
                    status: status.clone(),
                    ai: ai.clone(),
                    docker: docker.clone(),
                    freshness: freshness.clone(),
                    previous: RefCell::new(None),
                    trace_remaining: Cell::new(if std::env::args().any(|a| a == "--trace-menu") {
                        120
                    } else {
                        0
                    }),
                },
            );
            menu.addItem(&item(mtm, "AI Monitor", None));
            menu.addItem(&ai);
            menu.addItem(&docker);
            menu.addItem(&freshness);
            menu.addItem(&item(mtm, "CPU: 100% = 1 núcleo · M/G = MiB/GiB", None));
            menu.addItem(&item(
                mtm,
                "Docker = contêineres ativos, incluindo ai-memory",
                None,
            ));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            for (title, selector) in [
                ("Abrir painel", sel!(openPanel:)),
                ("Ocultar painel", sel!(hidePanel:)),
                ("Sair do AI Monitor", sel!(quitMonitor:)),
            ] {
                let entry = item(mtm, title, Some(selector));
                // SAFETY: the retained target implements each selector with the
                // action ABI above and outlives the menu and its timer.
                unsafe {
                    entry.setTarget(Some(&actions));
                }
                menu.addItem(&entry);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Container;
    fn snapshot() -> ResourceSnapshot {
        ResourceSnapshot {
            at: Some(Utc::now()),
            containers: vec![
                Container {
                    name: "ai-memory".into(),
                    state: "running".into(),
                    usage: Some(Usage {
                        cpu: 235.14,
                        memory: 512 * 1024 * 1024,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                Container {
                    name: "db".into(),
                    state: "running".into(),
                    usage: Some(Usage {
                        cpu: 12.,
                        memory: 1024 * 1024 * 1024,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn shows_both_sources_without_capping_multicore_cpu() {
        let snapshot = snapshot();
        let reading = MenuReading::from_snapshot(&snapshot, "ai-memory", 2, Utc::now());
        assert_eq!(reading.title, "AI 235% 512M  |  Docker 247% 1.5G");
        assert!(reading.ai.contains("235.1%"));
        assert!(reading.docker.contains("1.5 GiB"));
    }
    #[test]
    fn failures_and_old_samples_do_not_masquerade_as_current_numbers() {
        let mut snapshot = snapshot();
        snapshot.docker_error = Some("offline".into());
        let reading = MenuReading::from_snapshot(&snapshot, "ai-memory", 2, Utc::now());
        assert_eq!(reading.title, "AI —% —  |  Docker —% —");
        assert!(reading.freshness.contains("indisponível"));
        snapshot.docker_error = None;
        snapshot.at = Some(Utc::now() - chrono::Duration::seconds(30));
        let reading = MenuReading::from_snapshot(&snapshot, "ai-memory", 2, Utc::now());
        assert_eq!(reading.title, "AI —% —  |  Docker —% —");
        assert!(reading.freshness.contains("atrasada"));
    }
    #[test]
    fn zero_is_distinct_from_unavailable_and_interval_controls_freshness() {
        let mut snapshot = ResourceSnapshot {
            at: Some(Utc::now() - chrono::Duration::seconds(20)),
            ..Default::default()
        };
        let reading = MenuReading::from_snapshot(&snapshot, "ai-memory", 10, Utc::now());
        assert_eq!(reading.title, "AI —% —  |  Docker 0% 0M");
        snapshot.at = None;
        assert!(
            MenuReading::from_snapshot(&snapshot, "ai-memory", 10, Utc::now())
                .freshness
                .contains("primeira")
        );
    }
}
