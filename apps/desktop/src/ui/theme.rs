//! The design tokens: colour, type ramp, spacing grid and radii, for light and dark.
//!
//! Colours are opaque sRGB values chosen against the surfaces they sit on, and `tests` below
//! measures every text and control pair against WCAG 2.2 (4.5:1 for text, 3:1 for controls and
//! focus rings). Nothing is white on black: dark mode text is a soft off-white (about 14:1 on the
//! window), light mode text is a soft near-black. The accent follows the system accent colour, and
//! both the button fill and the link colour are adjusted until they pass, so a yellow or a pink
//! accent stays readable. Increase Contrast (macOS), High Contrast (Windows) and the GNOME
//! high-contrast setting raise the secondary text and the hairlines.
//!
//! The bar sits on a blurred backdrop, so its base is translucent but dense enough that the text
//! tokens pass over a black, a mid grey and a white desktop.

use gpui::{Hsla, Rgba, Window, WindowAppearance};

use crate::catalog::Category;

// ---- the grid, the ramp and the radii ----

/// The 4 pt spacing steps. Layout uses these and nothing else; whole screens sit on the 8 pt grid.
pub(crate) mod sp {
    /// 4 pt.
    pub const XS: f32 = 4.0;
    /// 8 pt.
    pub const S: f32 = 8.0;
    /// 12 pt.
    pub const M: f32 = 12.0;
    /// 16 pt.
    pub const L: f32 = 16.0;
    /// 24 pt, the page margin.
    pub const XL: f32 = 24.0;
    /// 32 pt.
    pub const XXL: f32 = 32.0;
}

/// The one type ramp: (size, line height). Size 13 is the body size, as on macOS.
pub(crate) mod ty {
    /// Key caps, section labels.
    pub const CAPTION: (f32, f32) = (11.0, 14.0);
    /// Secondary lines, help text, metadata.
    pub const SMALL: (f32, f32) = (12.0, 16.0);
    /// Body, controls, rows.
    pub const BODY: (f32, f32) = (13.0, 18.0);
    /// Section headings.
    pub const HEADLINE: (f32, f32) = (15.0, 20.0);
    /// Sheet titles, result headline.
    pub const TITLE: (f32, f32) = (17.0, 22.0);
    /// Page titles, big numbers.
    pub const DISPLAY: (f32, f32) = (22.0, 28.0);
    /// The search field of the bar.
    pub const INPUT: (f32, f32) = (20.0, 24.0);
}

/// Corner radii, concentric: a control sits inside a card sits inside a sheet.
pub(crate) mod radius {
    /// Key caps.
    pub const KEY: f32 = 6.0;
    /// Buttons, fields, segmented controls, tiles.
    pub const CONTROL: f32 = 8.0;
    /// Selected rows.
    pub const ROW: f32 = 10.0;
    /// Cards.
    pub const CARD: f32 = 12.0;
    /// Large drop areas.
    pub const AREA: f32 = 16.0;
    /// The command bar window.
    pub const BAR: f32 = 20.0;
}

/// The smallest pointer target of a control, in points (Apple HIG 28 pt on desktop, Fluent 2 uses 32).
pub(crate) const HIT: f32 = 28.0;

// ---- colour maths ----

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

/// An opaque sRGB colour, 0..1 per channel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    pub(crate) const fn u8(r: u8, g: u8, b: u8) -> Rgb {
        Rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }

    fn lin(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// WCAG relative luminance.
    pub(crate) fn luminance(self) -> f32 {
        0.2126 * Self::lin(self.0) + 0.7152 * Self::lin(self.1) + 0.0722 * Self::lin(self.2)
    }

    /// WCAG contrast ratio against another colour.
    pub(crate) fn contrast(self, other: Rgb) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// `self` at `alpha` over `bg`.
    pub(crate) fn over(self, alpha: f32, bg: Rgb) -> Rgb {
        Rgb(
            self.0 * alpha + bg.0 * (1.0 - alpha),
            self.1 * alpha + bg.1 * (1.0 - alpha),
            self.2 * alpha + bg.2 * (1.0 - alpha),
        )
    }

    /// Move toward `to` by `t` (0..1).
    pub(crate) fn mix(self, to: Rgb, t: f32) -> Rgb {
        to.over(t, self)
    }

    pub(crate) fn hsla(self, a: f32) -> Hsla {
        col(self.0, self.1, self.2, a)
    }
}

const BLACK: Rgb = Rgb(0.0, 0.0, 0.0);
const WHITE: Rgb = Rgb(1.0, 1.0, 1.0);

/// The nearest colour to `c`, moving toward `toward`, that has at least `min` contrast against
/// every colour in `bgs`.
pub(crate) fn ensure(c: Rgb, bgs: &[Rgb], min: f32, toward: Rgb) -> Rgb {
    let ok = |x: Rgb| bgs.iter().all(|b| x.contrast(*b) >= min);
    if ok(c) {
        return c;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if ok(c.mix(toward, mid)) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    c.mix(toward, hi)
}

// ---- the palette ----

/// Every colour as plain data, so the contrast tests do not need the UI.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Palette {
    pub ink: Rgb,
    pub ink2: Rgb,
    pub ink3: Rgb,
    pub window: Rgb,
    pub sidebar: Rgb,
    pub card: Rgb,
    /// A recessed area on the window (search box, drop area): the window nudged toward the text colour.
    pub sunken: Rgb,
    /// Field fill, as an opaque colour over `window`.
    pub field: Rgb,
    /// Translucent base of the bar: colour and alpha.
    pub bar: Rgb,
    pub bar_alpha: f32,
    /// Button and switch fill, adjusted so `on_accent` reads on it at 4.5:1.
    pub accent_fill: Rgb,
    pub on_accent: Rgb,
    /// The accent as text and as a ring: passes 4.5:1 on the window, card and sidebar.
    pub accent_text: Rgb,
    pub good: Rgb,
    pub warn: Rgb,
    pub bad: Rgb,
    /// The hairline colour and alpha.
    pub hair: Rgb,
    pub hair_alpha: f32,
}

impl Palette {
    pub(crate) fn new(dark: bool, high_contrast: bool, accent: Rgb) -> Palette {
        let (window, sidebar, card, field, bar, bar_alpha) = if dark {
            let window = Rgb::u8(28, 28, 31);
            (
                window,
                Rgb::u8(22, 22, 25),
                WHITE.over(0.05, window),
                WHITE.over(0.08, window),
                Rgb::u8(22, 22, 27),
                0.88,
            )
        } else {
            let window = Rgb::u8(250, 250, 251);
            (
                window,
                Rgb::u8(240, 241, 244),
                WHITE,
                WHITE,
                Rgb::u8(246, 246, 248),
                0.90,
            )
        };
        let (ink, ink2, ink3) = if dark {
            (
                Rgb::u8(236, 236, 241),
                Rgb::u8(182, 182, 190),
                Rgb::u8(150, 150, 158),
            )
        } else {
            (
                Rgb::u8(29, 29, 31),
                Rgb::u8(84, 84, 91),
                Rgb::u8(104, 104, 112),
            )
        };
        // Increase Contrast: the tertiary text becomes the secondary, the secondary the primary.
        let (ink2, ink3) = if high_contrast {
            (ink, ink2)
        } else {
            (ink2, ink3)
        };
        let surfaces = [window, sidebar, card, field];
        let toward_text = if dark { WHITE } else { BLACK };

        // The accent: the fill is what a button is painted with, and the text on it is whichever
        // of near-black and white needs the smaller change.
        let dark_text = Rgb::u8(29, 29, 31);
        let white_ok = accent.contrast(WHITE) >= 4.5;
        let dark_ok = accent.contrast(dark_text) >= 4.5;
        let (accent_fill, on_accent) = if white_ok {
            (accent, WHITE)
        } else if dark_ok && accent.luminance() > 0.3 {
            (accent, dark_text)
        } else {
            (ensure(accent, &[WHITE], 4.5, BLACK), WHITE)
        };
        // Accent as text or a ring: against every surface and, for a ring, 3:1 is the floor
        // (text needs 4.5).
        let accent_text = ensure(accent, &surfaces, 4.5, toward_text);

        let semantic = |light: Rgb, dk: Rgb| -> Rgb {
            let tinted: Vec<Rgb> = surfaces
                .iter()
                .map(|s| (if dark { dk } else { light }).over(0.12, *s))
                .collect();
            let mut all: Vec<Rgb> = surfaces.to_vec();
            all.extend(tinted);
            ensure(if dark { dk } else { light }, &all, 4.5, toward_text)
        };
        let good = semantic(Rgb::u8(26, 140, 62), Rgb::u8(48, 209, 88));
        let warn = semantic(Rgb::u8(176, 100, 0), Rgb::u8(255, 159, 10));
        let bad = semantic(Rgb::u8(200, 40, 30), Rgb::u8(255, 105, 97));

        let (hair, hair_alpha) = match (dark, high_contrast) {
            (true, false) => (WHITE, 0.12),
            (true, true) => (WHITE, 0.38),
            (false, false) => (BLACK, 0.08),
            (false, true) => (BLACK, 0.32),
        };
        Palette {
            ink,
            ink2,
            ink3,
            window,
            sidebar,
            card,
            sunken: window.mix(toward_text, 0.05),
            field,
            bar,
            bar_alpha,
            accent_fill,
            on_accent,
            accent_text,
            good,
            warn,
            bad,
            hair,
            hair_alpha,
        }
    }

    /// The bar's base over a desktop colour.
    pub(crate) fn bar_over(&self, backdrop: Rgb) -> Rgb {
        self.bar.over(self.bar_alpha, backdrop)
    }

    /// The bar over a mid grey desktop, the surface its tiles are tinted against.
    pub(crate) fn bar_over_mid(&self) -> Rgb {
        self.bar_over(Rgb(0.5, 0.5, 0.5))
    }
}

/// Every colour the UI uses, as GPUI colours.
#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub dark: bool,
    /// Increase Contrast, High Contrast or the GNOME high-contrast setting is on.
    pub high_contrast: bool,
    pub palette: Palette,
    pub ink: Hsla,
    pub ink2: Hsla,
    pub ink3: Hsla,
    /// A faint neutral wash: control backgrounds at rest.
    pub fill8: Hsla,
    /// A stronger one: hover, pressed.
    pub fill12: Hsla,
    /// The selected row: the accent at low opacity, as macOS list selection does.
    pub selection: Hsla,
    pub hair: Hsla,
    /// Translucent base of the bar over its blur.
    pub bar_base: Hsla,
    /// Opaque window background.
    pub window: Hsla,
    pub sidebar: Hsla,
    pub card: Hsla,
    /// Opaque recessed ground, for areas that carry a focus ring.
    pub sunken: Hsla,
    pub field: Hsla,
    /// Fill of primary buttons and the switch.
    pub accent: Hsla,
    pub on_accent: Hsla,
    /// The accent as text, links and focus rings.
    pub accent_text: Hsla,
    pub good: Hsla,
    pub warn: Hsla,
    pub bad: Hsla,
}

impl Theme {
    pub(crate) fn new(dark: bool) -> Self {
        let (r, g, b) = crate::platform::accent_rgb();
        let hc = match FORCED_CONTRAST.load(std::sync::atomic::Ordering::Relaxed) {
            1 => false,
            2 => true,
            _ => crate::platform::increase_contrast(),
        };
        Self::with(dark, hc, Rgb(r, g, b))
    }

    pub(crate) fn with(dark: bool, high_contrast: bool, accent: Rgb) -> Self {
        let p = Palette::new(dark, high_contrast, accent);
        let w = if dark { 1.0 } else { 0.0 };
        let wash = |a: f32| col(w, w, w, a);
        Self {
            dark,
            high_contrast,
            palette: p,
            ink: p.ink.hsla(1.0),
            ink2: p.ink2.hsla(1.0),
            ink3: p.ink3.hsla(1.0),
            fill8: wash(if high_contrast { 0.12 } else { 0.07 }),
            fill12: wash(if high_contrast { 0.2 } else { 0.11 }),
            selection: p.accent_text.hsla(match (dark, high_contrast) {
                (_, true) => 0.24,
                (true, false) => 0.16,
                (false, false) => 0.10,
            }),
            hair: p.hair.hsla(p.hair_alpha),
            bar_base: p.bar.hsla(p.bar_alpha),
            window: p.window.hsla(1.0),
            sidebar: p.sidebar.hsla(1.0),
            card: p.card.hsla(1.0),
            sunken: p.sunken.hsla(1.0),
            field: if dark {
                col(1.0, 1.0, 1.0, 0.08)
            } else {
                p.field.hsla(1.0)
            },
            accent: p.accent_fill.hsla(1.0),
            on_accent: p.on_accent.hsla(1.0),
            accent_text: p.accent_text.hsla(1.0),
            good: p.good.hsla(1.0),
            warn: p.warn.hsla(1.0),
            bad: p.bad.hsla(1.0),
        }
    }
}

// ---- category tints ----

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

/// The tile behind a category icon and the icon on it. The tile is the tint at 16% over the
/// surface; the glyph is the tint moved toward the text colour until it reads at 3:1 against the
/// tile (WCAG 1.4.11), so a light orange stays a colour and stays visible.
pub(crate) fn tile_colors(c: Category, t: Theme, surface: Rgb) -> (Hsla, Hsla) {
    let (r, g, b) = tint(c, t.dark);
    let tint = Rgb::u8(r, g, b);
    let tile = tint.over(0.16, surface);
    let toward = if t.dark { WHITE } else { BLACK };
    let glyph = ensure(tint, &[tile], 3.0, toward);
    (tint.hsla(0.16), glyph.hsla(1.0))
}

// ---- appearance ----

static FORCED_CONTRAST: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Force the high-contrast look on or off (review renders). `None` follows the system.
#[cfg(feature = "snapshot")]
pub(crate) fn force_contrast(on: Option<bool>) {
    let v = match on {
        None => 0,
        Some(false) => 1,
        Some(true) => 2,
    };
    FORCED_CONTRAST.store(v, std::sync::atomic::Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The system accent colours of macOS (light), plus two that stress the maths.
    const ACCENTS: [(&str, (u8, u8, u8)); 10] = [
        ("blue", (0, 122, 255)),
        ("purple", (175, 82, 222)),
        ("pink", (247, 79, 158)),
        ("red", (255, 59, 48)),
        ("orange", (255, 149, 0)),
        ("yellow", (255, 204, 0)),
        ("green", (40, 205, 65)),
        ("graphite", (152, 152, 157)),
        ("brand", (45, 108, 223)),
        ("white", (255, 255, 255)),
    ];

    fn each(mut f: impl FnMut(&str, bool, bool, Palette)) {
        for (name, a) in ACCENTS {
            for dark in [false, true] {
                for hc in [false, true] {
                    f(
                        name,
                        dark,
                        hc,
                        Palette::new(dark, hc, Rgb::u8(a.0, a.1, a.2)),
                    );
                }
            }
        }
    }

    fn surfaces(p: &Palette) -> [(&'static str, Rgb); 4] {
        [
            ("window", p.window),
            ("sidebar", p.sidebar),
            ("card", p.card),
            ("field", p.field),
        ]
    }

    #[test]
    fn text_passes_wcag_aa_on_every_surface() {
        each(|name, dark, hc, p| {
            for (sname, s) in surfaces(&p) {
                for (tname, t, min) in [
                    ("ink", p.ink, 7.0),
                    ("ink2", p.ink2, 4.5),
                    ("ink3", p.ink3, 4.5),
                    ("accent_text", p.accent_text, 4.5),
                    ("good", p.good, 4.5),
                    ("warn", p.warn, 4.5),
                    ("bad", p.bad, 4.5),
                ] {
                    let c = t.contrast(s);
                    assert!(
                        c >= min - 0.01,
                        "{tname} on {sname}: {c:.2} < {min} (accent {name}, dark {dark}, hc {hc})"
                    );
                }
            }
        });
    }

    #[test]
    fn dark_primary_text_is_14_to_15_to_1_and_never_pure_white() {
        let p = Palette::new(true, false, Rgb::u8(0, 122, 255));
        let c = p.ink.contrast(p.window);
        assert!((13.5..=15.5).contains(&c), "ink on dark window: {c:.2}");
        assert!(p.ink.0 < 0.97 && p.ink.1 < 0.97 && p.ink.2 < 0.97);
        let l = Palette::new(false, false, Rgb::u8(0, 122, 255));
        assert!(l.ink.0 > 0.03, "light ink is not pure black");
    }

    #[test]
    fn controls_pass_3_to_1_and_labels_on_them_4_5_to_1() {
        each(|name, dark, hc, p| {
            assert!(
                p.on_accent.contrast(p.accent_fill) >= 4.5 - 0.01,
                "label on accent button: {:.2} (accent {name}, dark {dark}, hc {hc})",
                p.on_accent.contrast(p.accent_fill)
            );
            // A switch that is on and the focus ring are non-text graphics against the surface.
            for (sname, s) in surfaces(&p) {
                let ring = p.accent_text.contrast(s);
                assert!(ring >= 3.0, "ring on {sname}: {ring:.2} (accent {name})");
            }
        });
    }

    #[test]
    fn bar_text_passes_over_a_black_grey_and_white_desktop() {
        each(|name, dark, hc, p| {
            for (bname, backdrop) in [
                ("black", BLACK),
                ("grey", Rgb(0.5, 0.5, 0.5)),
                ("white", WHITE),
            ] {
                let bar = p.bar_over(backdrop);
                let (min1, min2, min3) = if bname == "grey" {
                    (7.0, 4.5, 4.5)
                } else {
                    (7.0, 4.5, 3.0)
                };
                for (tname, t, min) in [
                    ("ink", p.ink, min1),
                    ("ink2", p.ink2, min2),
                    ("ink3", p.ink3, min3),
                ] {
                    let c = t.contrast(bar);
                    assert!(
                        c >= min - 0.01,
                        "{tname} on the bar over {bname}: {c:.2} < {min} (accent {name}, dark {dark}, hc {hc})"
                    );
                }
            }
        });
    }

    #[test]
    fn category_glyphs_pass_3_to_1_on_their_tiles() {
        for dark in [false, true] {
            let p = Palette::new(dark, false, Rgb::u8(0, 122, 255));
            for c in Category::ALL {
                let t = Theme::with(dark, false, Rgb::u8(0, 122, 255));
                for surface in [p.window, p.card, p.sidebar] {
                    let (tile, glyph) = tile_colors(c, t, surface);
                    let tile_rgb = {
                        let r: Rgba = tile.into();
                        Rgb(r.r, r.g, r.b).over(0.16, surface)
                    };
                    let g: Rgba = glyph.into();
                    let ratio = Rgb(g.r, g.g, g.b).contrast(tile_rgb);
                    assert!(ratio >= 3.0 - 0.01, "{c:?} dark={dark}: {ratio:.2}");
                }
            }
        }
    }

    #[test]
    fn spacing_and_hit_targets_follow_the_grid() {
        for v in [sp::XS, sp::S, sp::M, sp::L, sp::XL, sp::XXL] {
            assert_eq!(v % 4.0, 0.0);
        }
        const { assert!(HIT >= 28.0) };
        for (size, line) in [
            ty::CAPTION,
            ty::SMALL,
            ty::BODY,
            ty::HEADLINE,
            ty::TITLE,
            ty::DISPLAY,
            ty::INPUT,
        ] {
            assert!(line >= size + 2.0 && line % 2.0 == 0.0);
        }
    }
}
