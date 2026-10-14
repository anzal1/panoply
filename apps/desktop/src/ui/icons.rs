//! Lucide icons (ISC) compiled in as SVG text and served to GPUI as assets.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

use crate::icons_gen::ICONS;

/// The app's asset source: `icons/<name>.svg`.
pub(crate) struct Icons;

impl AssetSource for Icons {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let name = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"));
        Ok(name
            .and_then(|n| ICONS.iter().find(|(k, _)| *k == n))
            .map(|(_, s)| Cow::Borrowed(s.as_bytes())))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(k, _)| SharedString::from(format!("icons/{k}.svg")))
            .collect())
    }
}
