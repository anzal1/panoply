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

/// System Reduce Motion setting. `PANOPLY_REDUCE_MOTION=1` forces it; GNOME is asked through gsettings.
pub(crate) fn reduce_motion() -> bool {
    if std::env::var("PANOPLY_REDUCE_MOTION").is_ok_and(|v| v == "1") {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(out) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "enable-animations"])
            .output()
        {
            return String::from_utf8_lossy(&out.stdout).trim() == "false";
        }
    }
    false
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
