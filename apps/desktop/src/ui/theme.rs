//! Colours for light and dark, and the tint of each category. The bar sits on a blurred backdrop,
//! so its base is translucent; the app window is opaque.

use gpui::{Hsla, Rgba, Window, WindowAppearance};

use crate::catalog::Category;

/// A colour from 0..1 channels.
pub(crate) fn col(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    Rgba { r, g, b, a }.into()
}

/// A colour from 8-bit channels.
pub(crate) fn rgb8(c: (u8, u8, u8), a: f32) -> Hsla {
    col(
        c.0 as f32 / 255.0,
        c.1 as f32 / 255.0,
        c.2 as f32 / 255.0,
        a,
    )
}

/// Every colour the UI uses.
#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub dark: bool,
    pub ink: Hsla,
    pub ink2: Hsla,
    pub ink3: Hsla,
    pub fill8: Hsla,
    pub fill12: Hsla,
    pub hair: Hsla,
    /// Translucent base of the bar over its blur.
    pub bar_base: Hsla,
    /// Opaque window background.
    pub window: Hsla,
    pub sidebar: Hsla,
    pub card: Hsla,
    pub field: Hsla,
    pub accent: Hsla,
    pub on_accent: Hsla,
    pub good: Hsla,
    pub warn: Hsla,
    pub bad: Hsla,
}

impl Theme {
    pub(crate) fn new(dark: bool) -> Self {
        let w = if dark { 1.0 } else { 0.0 };
        let ink = |a: f32| col(w, w, w, a);
        let (ar, ag, ab) = crate::platform::accent_rgb();
        Self {
            dark,
            ink: ink(0.85),
            ink2: ink(0.85 * 0.62),
            ink3: ink(0.85 * 0.42),
            fill8: ink(0.85 * 0.08),
            fill12: ink(0.85 * 0.12),
            hair: if dark {
                col(1.0, 1.0, 1.0, 0.12)
            } else {
                col(0.0, 0.0, 0.0, 0.08)
            },
            bar_base: if dark {
                col(0.11, 0.11, 0.125, 0.62)
            } else {
                col(0.965, 0.965, 0.975, 0.66)
            },
            window: if dark {
                col(0.13, 0.13, 0.145, 1.0)
            } else {
                col(0.975, 0.975, 0.98, 1.0)
            },
            sidebar: if dark {
                col(0.105, 0.105, 0.12, 1.0)
            } else {
                col(0.94, 0.945, 0.96, 1.0)
            },
            card: if dark {
                col(1.0, 1.0, 1.0, 0.05)
            } else {
                col(1.0, 1.0, 1.0, 0.9)
            },
            field: if dark {
                col(1.0, 1.0, 1.0, 0.07)
            } else {
                col(1.0, 1.0, 1.0, 0.85)
            },
            accent: col(ar, ag, ab, 1.0),
            on_accent: col(1.0, 1.0, 1.0, 1.0),
            good: if dark {
                rgb8((48, 209, 88), 1.0)
            } else {
                rgb8((26, 140, 62), 1.0)
            },
            warn: if dark {
                rgb8((255, 159, 10), 1.0)
            } else {
                rgb8((176, 100, 0), 1.0)
            },
            bad: if dark {
                rgb8((255, 105, 97), 1.0)
            } else {
                rgb8((200, 40, 30), 1.0)
            },
        }
    }
}

/// macOS system colours per category, light and dark.
pub(crate) fn tint(c: Category, dark: bool) -> (u8, u8, u8) {
    let (l, d) = match c {
        Category::Pdf => ((255, 59, 48), (255, 69, 58)),
        Category::IdForms => ((255, 149, 0), (255, 159, 10)),
        Category::Images => ((175, 82, 222), (191, 90, 242)),
        Category::Video => ((255, 100, 20), (255, 120, 40)),
        Category::Audio => ((255, 45, 85), (255, 55, 95)),
        Category::Documents => ((0, 122, 255), (10, 132, 255)),
        Category::Files => ((0, 122, 255), (10, 132, 255)),
        Category::Capture => ((48, 176, 199), (64, 200, 224)),
        Category::Text => ((52, 199, 89), (48, 209, 88)),
        Category::Calculators => ((88, 86, 214), (94, 92, 230)),
        Category::Developer => ((120, 120, 130), (152, 152, 157)),
        Category::Privacy => ((142, 142, 147), (152, 152, 157)),
    };
    if dark { d } else { l }
}

static FORCED_APPEARANCE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Force light or dark (dev checks and review renders). `None` follows the system.
#[cfg(feature = "snapshot")]
pub(crate) fn force_appearance(dark: Option<bool>) {
    let v = match dark {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    };
    FORCED_APPEARANCE.store(v, std::sync::atomic::Ordering::Relaxed);
}

fn forced_dark() -> Option<bool> {
    match FORCED_APPEARANCE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(false),
        2 => Some(true),
        _ => match std::env::var("PANOPLY_APPEARANCE").ok().as_deref() {
            Some("light") => Some(false),
            Some("dark") => Some(true),
            _ => None,
        },
    }
}

pub(crate) fn is_dark(window: &Window) -> bool {
    forced_dark().unwrap_or_else(|| {
        matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        )
    })
}
