//! The Windows notification-area (tray) icon.
//!
//! The command bar window stays alive for the hotkey, so closing the app window leaves the process
//! running. The tray icon is what makes that visible: it shows the fan, opens the app or the bar,
//! and is the one place to quit. The icon bitmap comes from the same `.ico` the installer uses.

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// What the user asked for in the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrayCmd {
    /// Open (or raise) the app window.
    OpenApp,
    /// Show the command bar.
    OpenBar,
    /// Quit Panoply.
    Quit,
}

/// The live tray icon. Dropping it removes the icon from the notification area.
pub(crate) struct Tray {
    icon: TrayIcon,
}

impl Tray {
    /// Take the icon off the notification area now (before the process exits, so no ghost is left).
    pub(crate) fn remove(&self) {
        let _ = self.icon.set_visible(false);
    }
}

/// Create the tray icon and route its menu and clicks to `send`. Must run on the thread that pumps
/// Windows messages (GPUI's main thread). `bar_keys` is the hotkey as shown next to "Open command bar".
/// Returns `None` when the shell refuses the icon (no notification area, as on a bare session).
pub(crate) fn install(
    bar_keys: &str,
    send: impl Fn(TrayCmd) + Send + Sync + Clone + 'static,
) -> Option<Tray> {
    let open_app = MenuItem::with_id("open-app", "Open Panoply", true, None);
    // A tab puts the shortcut in the menu's right-hand column, the way Windows draws accelerators.
    let open_bar = MenuItem::with_id(
        "open-bar",
        format!("Open command bar\t{bar_keys}"),
        true,
        None,
    );
    let quit = MenuItem::with_id("quit", "Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &open_app,
        &open_bar,
        &PredefinedMenuItem::separator(),
        &quit,
    ])
    .ok()?;

    let ids: [(MenuId, TrayCmd); 3] = [
        (open_app.id().clone(), TrayCmd::OpenApp),
        (open_bar.id().clone(), TrayCmd::OpenBar),
        (quit.id().clone(), TrayCmd::Quit),
    ];
    let menu_send = send.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if let Some((_, cmd)) = ids.iter().find(|(id, _)| *id == e.id) {
            menu_send(*cmd);
        }
    }));
    // A left click opens the app (the right click shows the menu); so does a double click.
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| match e {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
        | TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            ..
        } => send(TrayCmd::OpenApp),
        _ => {}
    }));

    let (rgba, w, h) = super::ico::decode(super::ico::PANOPLY_ICO, super::small_icon_px())?;
    let icon = Icon::from_rgba(rgba, w, h).ok()?;
    let built = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Panoply")
        .with_icon(icon)
        .build();
    match built {
        Ok(icon) => Some(Tray { icon }),
        Err(e) => {
            eprintln!("tray icon unavailable: {e}");
            None
        }
    }
}
