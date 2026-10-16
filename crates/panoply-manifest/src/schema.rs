//! The manifest data model and its parsing.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ManifestError, Pattern};

/// A parsed `module.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Identity and flow pattern.
    pub module: ModuleInfo,
    /// What the module takes: MIME types (`image/*`, `application/pdf`) or extension globs
    /// (`*.heic`). Empty only for modules that take no files.
    #[serde(default)]
    pub accepts: Vec<String>,
    /// What the module produces, in the same notation as `accepts`.
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Options the host renders as a form, keyed by option name.
    #[serde(default)]
    pub options: BTreeMap<String, OptionSpec>,
    /// What the sandbox grants. The default is the least possible.
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Where the module appears.
    #[serde(default)]
    pub surfaces: Surfaces,
}

/// The `[module]` table.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleInfo {
    /// Stable id like `pdf.compress`. Lowercase, dot separated. The MCP tool name is this id
    /// with dots turned into underscores.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Module version, `major.minor.patch`.
    pub version: String,
    /// Flow pattern, `A` to `I`.
    pub pattern: Pattern,
    /// One sentence a user would say, shown in the bar and used as the MCP description.
    pub summary: String,
    /// Community modules only: path to the compiled `.wasm` component, relative to this
    /// `module.toml`. When absent the host looks for `module.wasm` next to the manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm: Option<String>,
}

/// The kind of control an option renders as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionType {
    /// A number, with optional `min` and `max`.
    Number,
    /// One of a fixed list of `values`.
    Choice,
    /// On or off.
    Bool,
    /// Free text.
    Text,
    /// A byte size such as `2mb`, or a plain number of bytes.
    Size,
    /// A file chosen by the user.
    File,
}

/// One `[options.<key>]` table.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSpec {
    /// Control type.
    #[serde(rename = "type")]
    pub kind: OptionType,
    /// Default value. No default makes the option optional.
    #[serde(default)]
    pub default: Option<Value>,
    /// Lowest allowed number.
    #[serde(default)]
    pub min: Option<f64>,
    /// Highest allowed number.
    #[serde(default)]
    pub max: Option<f64>,
    /// Allowed values for a `choice`.
    #[serde(default)]
    pub values: Vec<String>,
    /// Label shown next to the control. Falls back to the key.
    #[serde(default)]
    pub label: Option<String>,
    /// One line of help shown under the control.
    #[serde(default)]
    pub help: Option<String>,
}

/// How much of the filesystem a module may touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesAccess {
    /// Only the files the user chose, and the chosen output folder.
    #[default]
    Chosen,
    /// No file access at all (generators).
    None,
}

/// A model a module downloads on first use.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpec {
    /// Model id.
    pub id: String,
    /// Download size in megabytes, shown before the first download.
    pub size_mb: u32,
    /// SPDX license id of the weights.
    pub license: String,
}

/// The `[capabilities]` table.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// File access. Defaults to `chosen`.
    #[serde(default)]
    pub files: FilesAccess,
    /// Network access. Defaults to false.
    #[serde(default)]
    pub network: bool,
    /// Why the network is needed. Required when `network` is true, and shown to the user.
    #[serde(default)]
    pub network_reason: Option<String>,
    /// Models the module loads.
    #[serde(default)]
    pub models: Vec<ModelSpec>,
}

/// What a module's network access is for, as a person should be told. Decided from the manifest alone:
/// a module that lists the models it loads uses the network to fetch them once, with the person's yes,
/// and no file leaves the computer. Any other module that asks for the network is a plain network user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkUse {
    /// No network access: everything is processed on this computer.
    None,
    /// A one-time model download after consent.
    ModelDownload,
    /// Network access that is not a model download (an engine download, exchange rates).
    Network,
}

impl Capabilities {
    /// How this module uses the network, from `network` and the `models` it lists.
    pub fn network_use(&self) -> NetworkUse {
        if !self.network {
            NetworkUse::None
        } else if !self.models.is_empty() {
            NetworkUse::ModelDownload
        } else {
            NetworkUse::Network
        }
    }
}

fn yes() -> bool {
    true
}

/// The `[surfaces]` table. Everything defaults to on.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Surfaces {
    /// Command bar.
    #[serde(default = "yes")]
    pub bar: bool,
    /// App window panel.
    #[serde(default = "yes")]
    pub panel: bool,
    /// Finder and Explorer right-click.
    #[serde(default = "yes")]
    pub right_click: bool,
    /// Phone share sheet.
    #[serde(default = "yes")]
    pub share_sheet: bool,
    /// Command line.
    #[serde(default = "yes")]
    pub cli: bool,
    /// MCP tool for agents.
    #[serde(default = "yes")]
    pub mcp: bool,
}

impl Default for Surfaces {
    fn default() -> Self {
        Self {
            bar: true,
            panel: true,
            right_click: true,
            share_sheet: true,
            cli: true,
            mcp: true,
        }
    }
}

impl Manifest {
    /// Parse a manifest from TOML text. Parsing checks the shape, [`Manifest::validate`]
    /// checks the rules.
    pub fn from_toml_str(text: &str) -> Result<Manifest, ManifestError> {
        toml::from_str(text).map_err(|e| ManifestError::Parse(e.message().to_string()))
    }

    /// Read and parse a manifest file.
    pub fn from_file(path: &Path) -> Result<Manifest, ManifestError> {
        let text = std::fs::read_to_string(path).map_err(|source| ManifestError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_toml_str(&text)
    }

    /// True when the module takes a file of this MIME type and extension.
    ///
    /// `accepts` entries are MIME types (`application/pdf`), MIME wildcards (`image/*`, or
    /// `*/*` for anything) or extension globs (`*.heic`). A module with an empty list accepts nothing.
    pub fn accepts_file(&self, mime: &str, ext: &str) -> bool {
        let ext = ext.trim_start_matches('.');
        self.accepts.iter().any(|pat| accepts_one(pat, mime, ext))
    }
}

fn accepts_one(pat: &str, mime: &str, ext: &str) -> bool {
    if let Some(glob_ext) = pat.strip_prefix("*.") {
        return !ext.is_empty() && glob_ext.eq_ignore_ascii_case(ext);
    }
    if let Some(prefix) = pat.strip_suffix("/*") {
        return prefix == "*"
            || mime
                .split('/')
                .next()
                .is_some_and(|m| m.eq_ignore_ascii_case(prefix));
    }
    pat.eq_ignore_ascii_case(mime)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
accepts = ["image/*"]
[module]
id = "image.demo"
name = "Demo"
version = "0.1.0"
pattern = "A"
summary = "Does a demo."
"#;

    #[test]
    fn minimal_manifest_uses_safe_defaults() {
        let m = Manifest::from_toml_str(MINIMAL).unwrap();
        assert_eq!(m.capabilities.files, FilesAccess::Chosen);
        assert!(!m.capabilities.network);
        assert!(m.surfaces.mcp && m.surfaces.cli && m.surfaces.share_sheet);
        assert!(m.options.is_empty());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let bad = MINIMAL.replace("name = \"Demo\"", "name = \"Demo\"\ncolour = \"red\"");
        assert!(Manifest::from_toml_str(&bad).is_err());
    }

    #[test]
    fn accepts_matches_mime_wildcard_and_ext() {
        let mut m = Manifest::from_toml_str(MINIMAL).unwrap();
        assert!(m.accepts_file("image/png", "png"));
        assert!(!m.accepts_file("application/pdf", "pdf"));
        m.accepts = vec!["*.heic".into(), "application/pdf".into()];
        assert!(m.accepts_file("application/octet-stream", ".HEIC"));
        assert!(m.accepts_file("application/pdf", "pdf"));
        assert!(!m.accepts_file("image/png", "png"));
    }

    #[test]
    fn the_network_is_told_by_the_models_a_module_lists() {
        let mut m = Manifest::from_toml_str(MINIMAL).unwrap();
        assert_eq!(m.capabilities.network_use(), NetworkUse::None);
        m.capabilities.network = true;
        m.capabilities.network_reason = Some("Downloads exchange rates".into());
        assert_eq!(m.capabilities.network_use(), NetworkUse::Network);
        m.capabilities.models.push(ModelSpec {
            id: "demo".into(),
            size_mb: 28,
            license: "MIT".into(),
        });
        assert_eq!(m.capabilities.network_use(), NetworkUse::ModelDownload);
        // Listing a model without asking for the network does not make it a network user.
        m.capabilities.network = false;
        assert_eq!(m.capabilities.network_use(), NetworkUse::None);
    }

    /// The Modules page, the panel header and the consent sheet all read this, so the shipped
    /// modules decide what a person is told: the four picture tools that fetch a model once say so,
    /// and the engine downloads (video, PDF) stay plain network users.
    #[test]
    fn the_shipped_modules_that_fetch_a_model_are_model_downloads() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/builtin");
        let mut seen = std::collections::BTreeMap::new();
        for e in std::fs::read_dir(&dir).unwrap() {
            let f = e.unwrap().path().join("module.toml");
            let m = Manifest::from_file(&f).unwrap();
            seen.insert(m.module.id.clone(), m.capabilities.network_use());
        }
        for id in [
            "id.passport_photo",
            "image.erase",
            "image.remove_background",
            "image.remove_watermark",
            "image.upscale",
        ] {
            assert_eq!(seen[id], NetworkUse::ModelDownload, "{id}");
        }
        assert_eq!(seen["media.compress"], NetworkUse::Network);
        assert_eq!(seen["pdf.compress"], NetworkUse::Network);
    }
}
