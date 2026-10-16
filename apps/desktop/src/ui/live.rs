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

use super::app::{APP_HEIGHT, APP_MIN, APP_WIDTH, AppView, Route};
use super::bar::{Bar, BarMode, HEIGHT, WIDTH};
use super::icons::Icons;
use super::{FindStatus, Hub, Recents};
use crate::catalog::Catalog;
use crate::finder::{Config as FinderConfig, Finder};
use crate::history::History;

actions!(
    panoply,
    [
        Quit,
        CloseWindow,
        OpenSettings,
        About,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom,
        ToggleFullScreen,
        OpenFiles,
        GoHome,
        GoFind,
        GoHistory,
        GoModules,
        FocusSearch,
        ShowBar,
        OpenHelp,
        ReportIssue,
        FocusNextControl,
        FocusPrevControl,
    ]
);

/// The command bar's window, so the View menu and the hotkey can show it. macOS and Windows keep
/// one hidden window for the life of the app; on Linux the window exists only while the bar is up
/// (X11 maps every window at creation and cannot hide a pop-up), so this is empty in between.
#[derive(Clone, Default)]
struct BarSlot(Rc<RefCell<Option<WindowHandle<Bar>>>>);

impl Global for BarSlot {}

/// Show the bar if it is hidden, hide it if it is up. On Linux a closed bar is created here.
fn toggle_bar(cx: &mut App) {
    let slot = cx.global::<BarSlot>().clone();
    let existing = *slot.0.borrow();
    let toggled = existing.is_some_and(|h| {
        h.update(cx, |bar, window, cx| bar.toggle(window, cx))
            .is_ok()
    });
    #[cfg(target_os = "linux")]
    if toggled {
        return;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = toggled;
    #[cfg(target_os = "linux")]
    {
        let hub = cx.global::<Hub>().clone();
        let mut opts = bar_window_options(BarMode::Live);
        if let Some(d) = cx.primary_display() {
            let b = d.bounds();
            opts.window_bounds = Some(WindowBounds::Windowed(Bounds {
                origin: point(
                    b.origin.x + (b.size.width - px(WIDTH)) / 2.0,
                    b.origin.y + b.size.height * 0.22,
                ),
                size: size(px(WIDTH), px(HEIGHT)),
            }));
        }
        match cx.open_window(opts, move |window, cx| {
            cx.new(|cx| Bar::new(window, cx, hub, BarMode::Live))
        }) {
            Ok(h) => {
                *slot.0.borrow_mut() = Some(h);
                let _ = h.update(cx, |bar, window, cx| bar.show(window, cx));
            }
            Err(e) => eprintln!("could not open the command bar: {e}"),
        }
    }
}

const HELP_URL: &str = "https://github.com/anzal1/panoply/blob/main/docs/README.md";
const ISSUES_URL: &str = "https://github.com/anzal1/panoply/issues/new";

/// The one app window, when it is open.
#[derive(Clone, Default)]
struct AppWindow(Rc<RefCell<Option<WindowHandle<AppView>>>>);

impl Global for AppWindow {}

/// The notification-area icon, kept for as long as the app runs so Quit can take it down first.
#[cfg(windows)]
struct TrayHandle(Rc<crate::platform::tray::Tray>);

#[cfg(windows)]
impl Global for TrayHandle {}

/// Quit the app. On Windows the tray icon is removed first, so none is left behind.
fn quit_app(cx: &mut App) {
    #[cfg(windows)]
    {
        if let Some(t) = cx.try_global::<TrayHandle>() {
            t.0.remove();
        }
    }
    cx.quit();
}

/// Put the Panoply icon in the Windows notification area. The command bar keeps the process alive
/// after the app window closes, so the icon is what shows that it is running and lets the user
/// open it or quit.
#[cfg(windows)]
fn install_tray(cx: &mut App) {
    use crate::platform::tray::{self, TrayCmd};
    let (tx, mut rx) = mpsc::unbounded::<TrayCmd>();
    let keys = hotkey_caps().join("+");
    let Some(handle) = tray::install(&keys, move |c| {
        let _ = tx.unbounded_send(c);
    }) else {
        return;
    };
    cx.set_global(TrayHandle(Rc::new(handle)));
    cx.spawn(async move |cx| {
        while let Some(cmd) = rx.next().await {
            cx.update(|cx| match cmd {
                TrayCmd::OpenApp => open_app_window(cx, Vec::new()),
                TrayCmd::OpenBar => {
                    let bar = *cx.global::<BarSlot>().0.borrow();
                    if let Some(bar) = bar {
                        let _ = bar.update(cx, |bar, window, cx| bar.show(window, cx));
                    }
                }
                TrayCmd::Quit => quit_app(cx),
            });
        }
    })
    .detach();
}

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

/// Window options for the app window. `saved` is the remembered frame, already fitted to the
/// displays; without one the window opens in the default size near the top left.
pub(crate) fn app_window_options_at(
    offscreen: bool,
    saved: Option<crate::window_state::Saved>,
) -> WindowOptions {
    let default = Bounds {
        origin: if offscreen {
            point(px(-20000.), px(100.))
        } else {
            point(px(180.), px(90.))
        },
        size: size(px(APP_WIDTH), px(APP_HEIGHT)),
    };
    let bounds = saved.map_or(default, |s| Bounds {
        origin: point(px(s.x), px(s.y)),
        size: size(px(s.w), px(s.h)),
    });
    let window_bounds = match saved {
        Some(s) if s.maximized => WindowBounds::Maximized(bounds),
        _ => WindowBounds::Windowed(bounds),
    };
    let mac = cfg!(target_os = "macos");
    WindowOptions {
        window_bounds: Some(window_bounds),
        titlebar: Some(TitlebarOptions {
            title: Some("Panoply".into()),
            // macOS keeps its frame and the page reaches under it; Windows draws its own bar
            // (the client area has no caption when this is on); Linux asks the compositor.
            appears_transparent: mac || cfg!(windows),
            traffic_light_position: Some(point(
                px(super::chrome::TRAFFIC_LIGHTS.0),
                px(super::chrome::TRAFFIC_LIGHTS.1),
            )),
        }),
        focus: !offscreen,
        show: !offscreen,
        window_min_size: Some(size(px(APP_MIN.0), px(APP_MIN.1))),
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
    let displays: Vec<crate::window_state::Display> = cx
        .displays()
        .iter()
        .map(|d| {
            // The visible part (without a taskbar or menu bar), in points like the window's own
            // bounds: GPUI divides the monitor's pixels by its scale factor.
            let b = d.visible_bounds();
            (
                b.origin.x.as_f32(),
                b.origin.y.as_f32(),
                b.size.width.as_f32(),
                b.size.height.as_f32(),
            )
        })
        .collect();
    let saved = Some(
        crate::window_state::load("app")
            .map(|s| crate::window_state::fit(s, APP_MIN, &displays))
            .unwrap_or_else(|| {
                crate::window_state::first_frame(
                    (180.0, 90.0),
                    (APP_WIDTH, APP_HEIGHT),
                    APP_MIN,
                    &displays,
                )
            }),
    );
    match cx.open_window(app_window_options_at(false, saved), move |window, cx| {
        let app = cx.new(|cx| AppView::new(window, cx, hub));
        app.update(cx, |a, _| a.remember_bounds());
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

/// Open the app window on a page.
fn show_page(cx: &mut App, route: Route) {
    open_app_window(cx, Vec::new());
    let slot = cx.global::<AppWindow>().clone();
    let handle = *slot.0.borrow();
    if let Some(h) = handle {
        let _ = h.update(cx, |app, _, cx| app.go(route, cx));
    }
}

/// Tab and Shift+Tab in a one-line text field move between controls. The kit's field binds Tab to
/// "indent", which would put a tab character in the search box. Bound after the kit's own, so
/// these win. Also used by the headless runs.
pub(crate) fn bind_input_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", FocusNextControl, Some("Input")),
        KeyBinding::new("shift-tab", FocusPrevControl, Some("Input")),
    ]);
}

/// Key bindings, global actions and the menu bar.
fn register_actions(cx: &mut App) {
    bind_input_keys(cx);
    // The command key on a Mac, Control elsewhere.
    let m = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    let k = |keys: &str| format!("{m}-{keys}");
    cx.bind_keys([
        KeyBinding::new(&k("q"), Quit, None),
        KeyBinding::new(&k(","), OpenSettings, None),
        KeyBinding::new(&k("w"), CloseWindow, Some("App")),
        KeyBinding::new(&k("o"), OpenFiles, Some("App")),
        KeyBinding::new(&k("f"), FocusSearch, Some("App")),
        KeyBinding::new(&k("1"), GoHome, Some("App")),
        KeyBinding::new(&k("2"), GoFind, Some("App")),
        KeyBinding::new(&k("3"), GoHistory, Some("App")),
        KeyBinding::new(&k("4"), GoModules, Some("App")),
        // Undo on a result card (the text fields have their own, below).
        KeyBinding::new(&k("z"), gpui_base::input::Undo, Some("ToolPanel")),
    ]);
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, Some("App")),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, Some("App")),
        KeyBinding::new("alt-space", ShowBar, Some("App")),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("f11", ToggleFullScreen, Some("App")),
        KeyBinding::new("f1", OpenHelp, None),
        // The kit binds Undo and Redo in text fields on macOS only.
        KeyBinding::new("ctrl-z", gpui_base::input::Undo, Some("Input")),
        KeyBinding::new("ctrl-shift-z", gpui_base::input::Redo, Some("Input")),
        KeyBinding::new("ctrl-y", gpui_base::input::Redo, Some("Input")),
    ]);

    cx.on_action(|_: &Quit, cx| quit_app(cx));
    cx.on_action(|_: &OpenSettings, cx| show_page(cx, Route::Settings));
    cx.on_action(|_: &About, cx| show_page(cx, Route::Settings));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &OpenHelp, cx| cx.open_url(HELP_URL));
    cx.on_action(|_: &ReportIssue, cx| cx.open_url(ISSUES_URL));
    cx.on_action(|_: &ShowBar, cx| {
        toggle_bar(cx);
    });
    cx.set_menus(menus());
}

/// The macOS menu bar: App, File, Edit, View, Window, Help, with the standard items.
fn menus() -> Vec<Menu> {
    use gpui_base::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
    vec![
        Menu {
            name: "Panoply".into(),
            items: vec![
                MenuItem::action("About Panoply", About),
                MenuItem::separator(),
                MenuItem::action("Settings\u{2026}", OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide Panoply", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Panoply", Quit),
            ],
            disabled: false,
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Open\u{2026}", OpenFiles),
                MenuItem::separator(),
                MenuItem::action("Close Window", CloseWindow),
            ],
            disabled: false,
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Undo", Undo, OsAction::Undo),
                MenuItem::os_action("Redo", Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", Cut, OsAction::Cut),
                MenuItem::os_action("Copy", Copy, OsAction::Copy),
                MenuItem::os_action("Paste", Paste, OsAction::Paste),
                MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            ],
            disabled: false,
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Home", GoHome),
                MenuItem::action("Find", GoFind),
                MenuItem::action("History", GoHistory),
                MenuItem::action("Modules", GoModules),
                MenuItem::separator(),
                MenuItem::action("Search Tools", FocusSearch),
                MenuItem::action("Show Command Bar", ShowBar),
                MenuItem::separator(),
                MenuItem::action("Enter Full Screen", ToggleFullScreen),
            ],
            disabled: false,
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Zoom", Zoom),
            ],
            disabled: false,
        },
        Menu {
            name: "Help".into(),
            items: vec![
                MenuItem::action("Panoply Help", OpenHelp),
                MenuItem::action("Report an Issue\u{2026}", ReportIssue),
            ],
            disabled: false,
        },
    ]
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
            loaded: false,
        });
        // The file index starts on its own thread; its notifications come back here over a channel
        // and update one entity the Find page and the bar watch. Nothing polls.
        let (find_tx, mut find_rx) = mpsc::unbounded::<()>();
        let finder = if std::env::var("PANOPLY_FIND").is_ok_and(|v| v == "off") {
            Finder::off()
        } else {
            Finder::start(FinderConfig::app(), move || {
                let _ = find_tx.unbounded_send(());
            })
        };
        let find_status = cx.new(|_| FindStatus {
            state: finder.state(),
        });
        let hub = Hub {
            catalog,
            history: history.clone(),
            recents: recents.clone(),
            finder: finder.clone(),
            find_status: find_status.clone(),
        };
        cx.set_global(hub.clone());
        cx.set_global(AppWindow::default());
        {
            let finder = finder.clone();
            cx.spawn(async move |cx| {
                while find_rx.next().await.is_some() {
                    // Several updates may be queued; one redraw covers them.
                    while find_rx.try_recv().is_ok() {}
                    let state = finder.state();
                    find_status.update(cx, |s, cx| {
                        if s.state != state {
                            s.state = state;
                            cx.notify();
                        }
                    });
                }
            })
            .detach();
        }
        // Quit: stop the watcher and save the index, so the next start loads it.
        cx.on_app_quit(move |_| {
            finder.shutdown();
            async {}
        })
        .detach();

        // Recent jobs load off the UI thread.
        cx.spawn(async move |cx| {
            let entries = cx
                .background_executor()
                .spawn(async move { history.recent(64) })
                .await;
            recents.update(cx, |r, cx| {
                r.entries = entries;
                r.loaded = true;
                cx.notify();
            });
        })
        .detach();

        register_actions(cx);

        cx.set_global(BarSlot::default());
        #[cfg(target_os = "linux")]
        cx.on_app_quit(|_| {
            crate::platform::stop_watchers();
            async {}
        })
        .detach();
        // The command bar is created now, hidden, so showing it later is instant. Not on Linux: X11
        // would put it on screen from the first frame, so there it is created by the first hotkey.
        #[cfg(not(target_os = "linux"))]
        {
            let bar_hub = hub.clone();
            let bar = cx
                .open_window(bar_window_options(BarMode::Live), move |window, cx| {
                    cx.new(|cx| Bar::new(window, cx, bar_hub, BarMode::Live))
                })
                .expect("open the command bar window");
            *cx.global::<BarSlot>().0.borrow_mut() = Some(bar);
        }
        #[cfg(windows)]
        install_tray(cx);
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
                cx.update(toggle_bar);
            }
        })
        .detach();
    });
}
