//! The models and engines Panoply may download, for the Modules page: what each one is, how big
//! it is, whether it is on this computer, and a Remove that frees the space and forgets the yes.
//!
//! The consent gate itself is `panoply_core::assets`; the sheet that asks is in the tool panel.

use panoply_core::assets::format_size;
use panoply_host::assets as host_assets;

/// One line on the Modules page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Item id, also what `panoply models remove` takes.
    pub id: String,
    /// "IS-Net background model".
    pub name: String,
    /// What it is for, in plain words.
    pub purpose: String,
    /// Where it comes from and under which license.
    pub origin: String,
    /// True when it is on this computer.
    pub installed: bool,
    /// "44 MB on this computer", or "44 MB when needed".
    pub size_text: String,
}

fn rows_from(entries: &[panoply_core::assets::Entry]) -> Vec<Row> {
    let mut rows: Vec<Row> = entries
        .iter()
        .map(|e| {
            let st = e.status();
            Row {
                id: e.info.id.clone(),
                name: e.info.name.clone(),
                purpose: e.info.purpose.clone(),
                origin: format!("from {}, {} license", e.info.source, e.info.license),
                installed: st.installed,
                size_text: if st.installed {
                    format!("{} on this computer", format_size(st.bytes))
                } else {
                    format!("{} when needed", e.info.size_label())
                },
            }
        })
        .collect();
    // What is taking space comes first.
    rows.sort_by_key(|r| !r.installed);
    rows
}

/// Every downloadable item on this platform, installed ones first.
pub fn rows() -> Vec<Row> {
    rows_from(&host_assets::catalog())
}

/// Remove one item from disk and forget the yes. Returns a plain line for the page.
pub fn remove(id: &str) -> Result<String, String> {
    let entry = host_assets::find(id).ok_or_else(|| format!("There is no download called {id}."))?;
    match entry.remove() {
        Ok(0) => Ok(format!("{} was not on this computer.", entry.info.name)),
        Ok(n) => Ok(format!("Removed {}, {} freed.", entry.info.name, format_size(n))),
        Err(e) => Err(format!(
            "Could not remove {} ({e}). Close anything using it and try again.",
            entry.info.name
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use panoply_core::assets::{AssetInfo, AssetKind, Entry};

    #[test]
    fn installed_items_come_first_with_their_real_size_and_removal_frees_it() {
        let d = std::env::temp_dir().join(format!("panoply-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let file = d.join("model.onnx");
        std::fs::write(&file, vec![0u8; 3000]).unwrap();
        let info = |id: &str| AssetInfo {
            id: id.into(),
            name: format!("Model {id}"),
            kind: AssetKind::Model,
            size: 44_000_000,
            source: "example.org".into(),
            license: "MIT".into(),
            purpose: "tests".into(),
        };
        let entries = vec![
            Entry {
                info: info("absent"),
                paths: vec![d.join("nope")],
            },
            Entry {
                info: info("present"),
                paths: vec![file.clone()],
            },
        ];
        let rows = rows_from(&entries);
        assert_eq!(rows[0].id, "present");
        assert_eq!(rows[0].size_text, "3 KB on this computer");
        assert_eq!(rows[1].size_text, "44 MB when needed");
        assert!(rows[1].origin.contains("example.org") && rows[1].origin.contains("MIT"));
        assert!(!rows[1].installed);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_real_catalog_lists_the_engines_and_models() {
        let ids: Vec<String> = rows().into_iter().map(|r| r.id).collect();
        for want in ["pdfium", "onnxruntime", "isnet", "migan", "esrgan", "ppocr-det"] {
            assert!(ids.iter().any(|i| i == want), "{want} in {ids:?}");
        }
    }

    #[test]
    fn removing_something_unknown_is_a_plain_error() {
        assert!(remove("nonsense").unwrap_err().contains("no download called"));
    }
}
