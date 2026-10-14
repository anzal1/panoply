//! Start-up of the real app: the pre-created hidden command bar, the app window, the global
//! hotkey, the menu, and the Dock icon behaviour.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::*;

use super::app::{APP_HEIGHT, APP_WIDTH, AppView};
use super::bar::{Bar, BarMode, HEIGHT, WIDTH};
use super::icons::Icons;
use super::{Hub, Recents};
use crate::catalog::Catalog;
use crate::history::History;

actions!(panoply, [Quit, CloseWindow]);

/// The one app window, when it is open.
#[derive(Clone, Default)]
struct AppWindow(Rc<RefCell<Option<WindowHandle<AppView>>>>);

impl Global for AppWindow {}

/// The hotkey, Option+Space (Ctrl+Alt+Space off macOS) unless `PANOPLY_HOTKEY` says otherwise ("ctrl+alt+p").
fn hotkey() -> HotKey {
    std::env::var("PANOPLY_HOTKEY")
        .ok()
        .and_then(|s| HotKey::from_str(&s).ok())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                HotKey::new(Some(Modifiers::ALT), Code::Space)
            } else {
                // Alt+Space is the window menu on Windows and many Linux desktops.
                HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space)
            }
        })
}

/// The key caps shown next to the home search box.
pub(crate) fn hotkey_caps() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["\u{2325}", "Space"]
    } else {
        &["Ctrl", "Alt", "Space"]
    }
}

/// Window options for the app window.
pub(crate) fn app_window_options(offscreen: bool) -> WindowOptions {
    let origin = if offscreen {
        point(px(-20000.), px(100.))
    } else {
        point(px(180.), px(90.))
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin,
            size: size(px(APP_WIDTH), px(APP_HEIGHT)),
        })),
        titlebar: Some(TitlebarOptions {
            title: Some("Panoply".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(18.), px(18.))),
        }),
        focus: !offscreen,
        show: !offscreen,
        window_min_size: Some(size(px(860.), px(560.))),
        ..Default::default()
    }
}

/// Window options for the command bar.
pub(crate) fn bar_window_options(mode: BarMode) -> WindowOptions {
    let origin = if mode == BarMode::Offscreen {
        point(px(-20000.), px(200.))
    } else {
        point(px(200.), px(200.))
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin,
            size: size(px(WIDTH), px(HEIGHT)),
        })),
        titlebar: None,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        inactive_frame_interval: None,
        window_background: WindowBackgroundAppearance::Blurred,
        ..Default::default()
    }
}

/// Open the app window, or bring the open one forward. Files, when given, go to its tool list.
fn open_app_window(cx: &mut App, files: Vec<PathBuf>) {
    let slot = cx.global::<AppWindow>().clone();
    let existing = *slot.0.borrow();
    if let Some(h) = existing
        && h.update(cx, |app, window, cx| {
            window.activate_window();
            if !files.is_empty() {
                app.open_files(files.clone(), cx);
            }
        })
        .is_ok()
    {
        cx.activate(true);
        return;
    }
    let hub = cx.global::<Hub>().clone();
    let files2 = files.clone();
    match cx.open_window(app_window_options(false), move |window, cx| {
        let app = cx.new(|cx| AppView::new(window, cx, hub));
        if !files2.is_empty() {
            let f = files2.clone();
            app.update(cx, |a, cx| a.open_files(f, cx));
        }
        app
    }) {
        Ok(h) => {
            *slot.0.borrow_mut() = Some(h);
            cx.activate(true);
        }
        Err(e) => eprintln!("could not open the app window: {e}"),
    }
}

fn paths_from_urls(urls: &[String]) -> Vec<PathBuf> {
    urls.iter()
        .filter_map(|u| u.strip_prefix("file://"))
        .map(|p| PathBuf::from(p.replace("%20", " ")))
        .collect()
}

/// Run the real app.
pub(crate) fn run(args: Vec<String>) {
    let background = args.iter().any(|a| a == "--background");
    let start_files: Vec<PathBuf> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect();
    let catalog = Arc::new(Catalog::load());
    let history = History::open_default();
    let app = gpui_platform::application().with_assets(Icons);
    app.on_reopen(|cx| open_app_window(cx, Vec::new()));
    // Files dropped on the Dock icon, or opened with the app, arrive as file URLs.
    let (url_tx, mut url_rx) = mpsc::unbounded::<Vec<PathBuf>>();
    app.on_open_urls(move |urls| {
        let _ = url_tx.unbounded_send(paths_from_urls(&urls));
    });
    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        cx.set_activation_policy(ActivationPolicy::Regular);
        let recents = cx.new(|_| Recents {
            entries: Vec::new(),
        });
        let hub = Hub {
            catalog,
            history: history.clone(),
            recents: recents.clone(),
        };
        cx.set_global(hub.clone());
        cx.set_global(AppWindow::default());

        // Recent jobs load off the UI thread.
        cx.spawn(async move |cx| {
            let entries = cx
                .background_executor()
                .spawn(async move { history.recent(64) })
                .await;
            recents.update(cx, |r, cx| {
                r.entries = entries;
                cx.notify();
            });
        })
        .detach();

        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        cx.bind_keys([
            KeyBinding::new(&format!("{modifier}-q"), Quit, None),
            KeyBinding::new(&format!("{modifier}-w"), CloseWindow, Some("App")),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![Menu {
            name: "Panoply".into(),
            items: vec![MenuItem::action("Quit Panoply", Quit)],
            disabled: false,
        }]);

        // The command bar is created now, hidden, so showing it later is instant.
        let bar_hub = hub.clone();
        let bar = cx
            .open_window(bar_window_options(BarMode::Live), move |window, cx| {
                cx.new(|cx| Bar::new(window, cx, bar_hub, BarMode::Live))
            })
            .expect("open the command bar window");

        if !background {
            open_app_window(cx, start_files);
        }

        // Global hotkey. The event handler only forwards to this loop; nothing runs on that thread.
        let (tx, mut rx) = mpsc::unbounded::<()>();
        match GlobalHotKeyManager::new() {
            Ok(mgr) => {
                if let Err(e) = mgr.register(hotkey()) {
                    eprintln!("hotkey not registered: {e}");
                }
                GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
                    if e.state == HotKeyState::Pressed {
                        let _ = tx.unbounded_send(());
                    }
                }));
                std::mem::forget(mgr);
            }
            Err(e) => eprintln!("global hotkey unavailable: {e}"),
        }
        cx.spawn(async move |cx| {
            while let Some(paths) = url_rx.next().await {
                cx.update(|cx| open_app_window(cx, paths));
            }
        })
        .detach();
        cx.spawn(async move |cx| {
            while rx.next().await.is_some() {
                let _ = bar.update(cx, |bar, window, cx| bar.toggle(window, cx));
            }
        })
        .detach();
    });
}
