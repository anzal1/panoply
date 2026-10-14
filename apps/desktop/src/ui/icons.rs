//! Lucide icons (ISC) compiled in as SVG text and served to GPUI as assets.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

use crate::icons_gen::ICONS;

/// Glyphs outside the generated Lucide set: the pop-up chevrons, and the Windows caption buttons
/// (10 by 10, 1 px strokes, as Fluent draws them).
const EXTRA: &[(&str, &str)] = &[
    // The four layers of the fan mark, drawn by the icon tool: the two back cards, the seam around
    // the front card and the front card. Each is one colour (set from the theme).
    (
        "fan-left",
        include_str!("../../packaging/icon/fan-layers/left.svg"),
    ),
    (
        "fan-right",
        include_str!("../../packaging/icon/fan-layers/right.svg"),
    ),
    (
        "fan-seam",
        include_str!("../../packaging/icon/fan-layers/front-seam.svg"),
    ),
    (
        "fan-front",
        include_str!("../../packaging/icon/fan-layers/front.svg"),
    ),
    // Lucide (ISC), drawn the way the generated set draws its icons.
    (
        "chevrons-up-down",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><path d="m7 15 5 5 5-5"/><path d="m7 9 5-5 5 5"/></svg>"#,
    ),
    (
        "win-min",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><path d="M0 5.5h10"/></svg>"#,
    ),
    (
        "win-max",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><rect x="0.5" y="0.5" width="9" height="9" rx="1.5"/></svg>"#,
    ),
    (
        "win-restore",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><rect x="0.5" y="2.5" width="7" height="7" rx="1.25"/><path d="M2.5 2.5v-1a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1"/></svg>"#,
    ),
    (
        "win-close",
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9"/></svg>"#,
    ),
];

/// The app's asset source: `icons/<name>.svg`.
pub(crate) struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let name = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"));
        Ok(name
            .and_then(|n| ICONS.iter().chain(EXTRA).find(|(k, _)| *k == n))
            .map(|(_, s)| Cow::Borrowed(s.as_bytes())))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .chain(EXTRA)
            .map(|(k, _)| SharedString::from(format!("icons/{k}.svg")))
            .collect())
    }
}
