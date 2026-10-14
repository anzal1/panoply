//! Windows and Linux. The same surface as macOS, built from portable GPUI calls, and so far
//! compiled and checked but not driven by hand on those systems.

use std::path::Path;

use gpui::Window;

/// Stand-in for the macOS panel handle. Visibility is done with `activate_window` and
/// `minimize_window`, which GPUI implements on both systems.
#[derive(Clone, Copy)]
pub(crate) struct Panel;

/// A rectangle, for the dev probes that read the window frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Pt {
    pub x: f64,
    pub y: f64,
}
/// Size part of [`Rect`].
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Sz {
    pub w: f64,
    pub h: f64,
}
/// Frame rectangle.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Rect {
    pub o: Pt,
    pub s: Sz,
}

impl Panel {
    pub(crate) fn of(_window: &Window) -> Option<Panel> {
        Some(Panel)
    }
    pub(crate) fn setup_bar(self, _radius: f64) {}
    pub(crate) fn set_alpha(self, _a: f64) {}
    pub(crate) fn set_ignores_mouse(self, _v: bool) {}
    pub(crate) fn show_key(self) {}
    pub(crate) fn order_front_regardless(self) {}
    pub(crate) fn order_out(self) {}
    pub(crate) fn is_key(self) -> bool {
        false
    }
    pub(crate) fn is_visible(self) -> bool {
        false
    }
    pub(crate) fn frame(self) -> Rect {
        Rect::default()
    }
    pub(crate) fn alpha(self) -> f64 {
        1.0
    }
    pub(crate) fn invalidate_shadow(self) {}
    pub(crate) fn set_origin(self, _x: f64, _y: f64) {}
    pub(crate) fn set_motion(self, _scale: f64, _dy: f64) {}
    pub(crate) fn position(self, _w: f64, _h: f64, _top_frac: f64) {}
    pub(crate) fn wake_a11y(self) {}
    pub(crate) fn blur_view_count(self) -> usize {
        0
    }
}

/// One boolean system setting kept in GNOME's settings: where its answer is cached, and what to ask.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct Watch {
    slot: &'static std::sync::atomic::AtomicU8,
    started: &'static std::sync::Once,
    schema: &'static str,
    key: &'static str,
    on: &'static str,
}

/// Whether a boolean system setting is on, without asking from the UI thread. The first call
/// starts one background thread that reads the value once and then follows changes through a
/// single long-lived `gsettings monitor` child; nothing polls. Asking costs an atomic load.
#[cfg(target_os = "linux")]
fn gnome_flag(w: Watch) -> bool {
    use std::sync::atomic::Ordering;
    w.started.call_once(|| {
        std::thread::spawn(move || follow(w));
    });
    w.slot.load(Ordering::Relaxed) == 2
}

/// Read a setting once, then block on its change feed. A desktop without the schema (anything but
/// GNOME, or no `gsettings` at all) keeps the default and says so once.
#[cfg(target_os = "linux")]
fn follow(w: Watch) {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    use std::sync::atomic::Ordering;
    let store = |text: &str| {
        w.slot
            .store(1 + u8::from(text.trim() == w.on), Ordering::Relaxed);
    };
    let first = Command::new("gsettings")
        .args(["get", w.schema, w.key])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match first {
        Ok(o) if o.status.success() => store(&String::from_utf8_lossy(&o.stdout)),
        _ => {
            eprintln!(
                "setting {} {} is not available here, using the default",
                w.schema, w.key
            );
            return;
        }
    }
    let Ok(mut child) = Command::new("gsettings")
        .args(["monitor", w.schema, w.key])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(out) = child.stdout.take() {
        // Each change arrives as one line, `key: value`.
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some((_, v)) = line.rsplit_once(": ") {
                store(v);
            }
        }
    }
    let _ = child.wait();
}

/// Give keyboard focus to the command bar. On X11 the bar is an override-redirect pop-up, which the
/// window manager never focuses and `_NET_ACTIVE_WINDOW` cannot reach, so without this Escape goes to
/// whatever had focus before. Returns the window that had it, for [`x11_restore_focus`]. Wayland
/// and every failure return `None`.
#[cfg(target_os = "linux")]
pub(crate) fn x11_take_focus(window: &Window) -> Option<u32> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::CURRENT_TIME;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, InputFocus};
    let id = match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xcb(h) => h.window.get(),
        RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok()?,
        _ => return None,
    };
    let (conn, _) = x11rb::connect(None).ok()?;
    let prev = conn.get_input_focus().ok()?.reply().ok()?.focus;
    conn.set_input_focus(InputFocus::POINTER_ROOT, id, CURRENT_TIME)
        .ok()?;
    conn.flush().ok()?;
    // The previous focus is only worth handing back when it was a real window.
    (prev > 1).then_some(prev)
}

/// Hand focus back to the window [`x11_take_focus`] saw, so the app window keeps its keyboard.
#[cfg(target_os = "linux")]
pub(crate) fn x11_restore_focus(prev: u32) {
    use x11rb::CURRENT_TIME;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, InputFocus};
    if let Ok((conn, _)) = x11rb::connect(None) {
        let _ = conn.set_input_focus(InputFocus::PARENT, prev, CURRENT_TIME);
        let _ = conn.flush();
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[repr(C)]
    struct HighContrastW {
        cb_size: u32,
        flags: u32,
        default_scheme: *mut u16,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, pv: *mut c_void, win_ini: u32) -> i32;
    }

    const SPI_GETHIGHCONTRAST: u32 = 0x0042;
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    const HCF_HIGHCONTRASTON: u32 = 0x1;

    /// `SPI_GETCLIENTAREAANIMATION` is false when animations are off.
    pub(super) fn animations_off() -> bool {
        let mut on: i32 = 1;
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut on as *mut i32).cast(),
                0,
            )
        };
        ok != 0 && on == 0
    }

    pub(super) fn high_contrast() -> bool {
        let mut hc = HighContrastW {
            cb_size: std::mem::size_of::<HighContrastW>() as u32,
            flags: 0,
            default_scheme: std::ptr::null_mut(),
        };
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                hc.cb_size,
                (&mut hc as *mut HighContrastW).cast(),
                0,
            )
        };
        ok != 0 && hc.flags & HCF_HIGHCONTRASTON != 0
    }
}

/// System Reduce Motion setting. `PANOPLY_REDUCE_MOTION=1` forces it. Windows is asked for the
/// client-area animation setting, GNOME for `enable-animations`.
pub(crate) fn reduce_motion() -> bool {
    if std::env::var("PANOPLY_REDUCE_MOTION").is_ok_and(|v| v == "1") {
        return true;
    }
    #[cfg(windows)]
    {
        win::animations_off()
    }
    #[cfg(target_os = "linux")]
    {
        static SLOT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        static ONCE: std::sync::Once = std::sync::Once::new();
        gnome_flag(Watch {
            slot: &SLOT,
            started: &ONCE,
            schema: "org.gnome.desktop.interface",
            key: "enable-animations",
            on: "false",
        })
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// System high-contrast setting: Windows High Contrast, GNOME's high-contrast switch.
/// `PANOPLY_HIGH_CONTRAST=1` forces it.
pub(crate) fn increase_contrast() -> bool {
    if std::env::var("PANOPLY_HIGH_CONTRAST").is_ok_and(|v| v == "1") {
        return true;
    }
    #[cfg(windows)]
    {
        win::high_contrast()
    }
    #[cfg(target_os = "linux")]
    {
        static SLOT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
        static ONCE: std::sync::Once = std::sync::Once::new();
        gnome_flag(Watch {
            slot: &SLOT,
            started: &ONCE,
            schema: "org.gnome.desktop.a11y.interface",
            key: "high-contrast",
            on: "true",
        })
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// A fixed blue where the system gives no accent colour.
pub(crate) fn accent_rgb() -> (f32, f32, f32) {
    (0.0, 0.478, 1.0)
}

/// Move a file to the Trash. Linux follows the freedesktop trash spec; Windows removes the file
/// (the recycle bin needs a shell call that is not wired up yet, see README.md).
pub(crate) fn os_trash(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux_trash(path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::fs::remove_file(path).or_else(|_| std::fs::remove_dir_all(path))
    }
}

#[cfg(target_os = "linux")]
fn linux_trash(path: &Path) -> std::io::Result<()> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })
        .ok_or_else(|| std::io::Error::other("no home folder"))?
        .join("Trash");
    let files = base.join("files");
    let info = base.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?
        .to_string_lossy()
        .into_owned();
    let mut target = files.join(&name);
    let mut n = 1;
    while target.exists() {
        n += 1;
        target = files.join(format!("{name}.{n}"));
    }
    let abs = std::fs::canonicalize(path)?;
    let encoded: String = abs
        .to_string_lossy()
        .bytes()
        .map(|b| match b {
            b'/' | b'.' | b'-' | b'_' | b'~' | b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect();
    let stamp = crate::units::now_secs();
    let info_name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or(name);
    std::fs::write(
        info.join(format!("{info_name}.trashinfo")),
        format!("[Trash Info]\nPath={encoded}\nDeletionDate={stamp}\n"),
    )?;
    std::fs::rename(&abs, &target).or_else(|_| {
        std::fs::copy(&abs, &target)?;
        std::fs::remove_file(&abs)
    })
}
