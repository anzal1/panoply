//! Windows and Linux. The same surface as macOS, built from portable GPUI calls, and so far
//! compiled and checked but not driven by hand on those systems.

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

/// One boolean system setting read through gsettings: where its answer is kept, and what to ask.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct Watch {
    slot: &'static std::sync::atomic::AtomicU8,
    schema: &'static str,
    key: &'static str,
    on: &'static str,
}

/// Whether a boolean system setting is on, read off the UI thread. The answer is cached and
/// refreshed every few seconds by a small background thread, so asking costs an atomic load.
#[cfg(target_os = "linux")]
fn gnome_flag(w: Watch) -> bool {
    use std::sync::atomic::Ordering;
    static STARTED: std::sync::Once = std::sync::Once::new();
    static WATCHED: std::sync::Mutex<Vec<Watch>> = std::sync::Mutex::new(Vec::new());
    if let Ok(mut list) = WATCHED.lock()
        && !list.iter().any(|x| std::ptr::eq(x.slot, w.slot))
    {
        list.push(w);
    }
    STARTED.call_once(|| {
        std::thread::spawn(|| {
            loop {
                let list: Vec<Watch> = WATCHED.lock().map(|l| l.clone()).unwrap_or_default();
                for x in list {
                    let v = std::process::Command::new("gsettings")
                        .args(["get", x.schema, x.key])
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == x.on);
                    if let Some(v) = v {
                        x.slot.store(1 + u8::from(v), Ordering::Relaxed);
                    }
                }
                std::thread::sleep(std::time::Duration::from_secs(4));
            }
        });
    });
    w.slot.load(Ordering::Relaxed) == 2
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
        gnome_flag(Watch {
            slot: &SLOT,
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
        gnome_flag(Watch {
            slot: &SLOT,
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
