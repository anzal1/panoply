//! The tool catalog: every module the host can run, built-in and community, as the bar and the
//! app window show them. Built from the real [`Registry`], never from a hand-kept list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use panoply_host::{Module, Registry};
use panoply_manifest::{Manifest, Pattern};
use panoply_wasm::WasmModule;

/// The categories in the sidebar, in the order of the home wireframe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// PDF tools.
    Pdf,
    /// Photos and forms for IDs and portals.
    IdForms,
    /// Pictures.
    Images,
    /// Video.
    Video,
    /// Audio.
    Audio,
    /// Office documents and ebooks.
    Documents,
    /// Any file: hashes, archives, rename.
    Files,
    /// Camera and scanner flows.
    Capture,
    /// Text.
    Text,
    /// Calculators and converters.
    Calculators,
    /// Developer tools.
    Developer,
    /// Privacy tools.
    Privacy,
}

impl Category {
    /// Every category, sidebar order.
    pub const ALL: [Category; 12] = [
        Category::Pdf,
        Category::IdForms,
        Category::Images,
        Category::Video,
        Category::Audio,
        Category::Documents,
        Category::Files,
        Category::Capture,
        Category::Text,
        Category::Calculators,
        Category::Developer,
        Category::Privacy,
    ];

    /// The name in the sidebar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::IdForms => "ID and forms",
            Self::Images => "Images",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Documents => "Documents",
            Self::Files => "Files",
            Self::Capture => "Capture",
            Self::Text => "Text",
            Self::Calculators => "Calculators",
            Self::Developer => "Developer",
            Self::Privacy => "Privacy",
        }
    }

    /// Short label used at the right of a bar row.
    pub fn short(self) -> &'static str {
        match self {
            Self::Images => "Image",
            other => other.label(),
        }
    }

    /// The icon name in `icons_gen`.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Pdf => "file-text",
            Self::IdForms => "id-card",
            Self::Images => "image",
            Self::Video => "film",
            Self::Audio => "audio-lines",
            Self::Documents => "file-text",
            Self::Files => "folder",
            Self::Capture => "camera",
            Self::Text => "type",
            Self::Calculators => "calculator",
            Self::Developer => "code-xml",
            Self::Privacy => "shield",
        }
    }

    /// The category a module id belongs to. Module ids are `family.name`, so the family decides.
    /// Anything unknown lands in Files, which is the catch-all.
    pub fn from_module_id(id: &str) -> Category {
        match id.split('.').next().unwrap_or("") {
            "pdf" => Category::Pdf,
            "id" | "forms" => Category::IdForms,
            "image" | "images" | "photo" => Category::Images,
            "video" | "media" => Category::Video,
            "audio" => Category::Audio,
            "doc" | "docs" | "office" | "document" | "ebook" => Category::Documents,
            "capture" | "scan" => Category::Capture,
            "text" | "string" => Category::Text,
            "calc" | "unit" => Category::Calculators,
            "dev" | "data" | "code" => Category::Developer,
            "privacy" | "metadata" => Category::Privacy,
            _ => Category::Files,
        }
    }
}

/// Where a module came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Native Rust inside the host.
    BuiltIn,
    /// A WebAssembly module loaded from this folder.
    Community(PathBuf),
}

/// One module as the UI sees it.
#[derive(Debug, Clone)]
pub struct Tool {
    /// Module id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-sentence summary.
    pub summary: String,
    /// Flow pattern.
    pub pattern: Pattern,
    /// Sidebar category.
    pub category: Category,
    /// The full manifest, which the panel renders its form from.
    pub manifest: Manifest,
    /// Built-in or community.
    pub source: Source,
}

impl Tool {
    /// True when the module takes files.
    pub fn takes_files(&self) -> bool {
        !self.manifest.accepts.is_empty()
    }

    /// Shown in the command bar.
    pub fn on_bar(&self) -> bool {
        self.manifest.surfaces.bar
    }
}

/// Every runnable module.
pub struct Catalog {
    registry: Arc<Registry>,
    tools: Vec<Tool>,
    by_id: HashMap<String, usize>,
    /// Plain-language notes about modules that could not be loaded.
    pub notes: Vec<String>,
}

impl Catalog {
    /// Built-in modules plus every community module found in [`crate::paths::module_search_dirs`].
    pub fn load() -> Catalog {
        Self::with_dirs(&crate::paths::module_search_dirs())
    }

    /// Built-in modules plus community modules found in these folders. Each module is a
    /// sub-folder with a `module.toml`; the folder itself may also be one.
    pub fn with_dirs(dirs: &[PathBuf]) -> Catalog {
        let mut registry = Registry::with_builtins();
        let mut sources: HashMap<String, Source> = registry
            .list()
            .map(|m| (m.manifest().module.id.clone(), Source::BuiltIn))
            .collect();
        let mut notes = Vec::new();
        for dir in dirs {
            for module_dir in module_folders(dir) {
                match WasmModule::load(&module_dir) {
                    Ok(m) => {
                        let id = m.manifest().module.id.clone();
                        match registry.register(Arc::new(m)) {
                            Ok(()) => {
                                sources.insert(id, Source::Community(module_dir));
                            }
                            Err(e) => notes.push(format!("{}: {}", module_dir.display(), e)),
                        }
                    }
                    Err(e) => notes.push(format!("{}: {}", module_dir.display(), e)),
                }
            }
        }
        let tools: Vec<Tool> = registry
            .list()
            .map(|m| {
                let manifest = m.manifest().clone();
                let id = manifest.module.id.clone();
                Tool {
                    source: sources.remove(&id).unwrap_or(Source::BuiltIn),
                    name: manifest.module.name.clone(),
                    summary: manifest.module.summary.clone(),
                    pattern: manifest.module.pattern,
                    category: Category::from_module_id(&id),
                    id,
                    manifest,
                }
            })
            .collect();
        let by_id = tools
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        Catalog {
            registry: Arc::new(registry),
            tools,
            by_id,
            notes,
        }
    }

    /// The registry that runs jobs.
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// Every tool, sorted by id.
    pub fn tools(&self) -> &[Tool] {
        &self.tools
    }

    /// A tool by index.
    pub fn tool(&self, i: usize) -> &Tool {
        &self.tools[i]
    }

    /// The index of a tool by module id.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.by_id.get(id).copied()
    }

    /// How many tools each category holds. Categories with none are left out.
    pub fn category_counts(&self) -> Vec<(Category, usize)> {
        Category::ALL
            .iter()
            .filter_map(|c| {
                let n = self.tools.iter().filter(|t| t.category == *c).count();
                (n > 0).then_some((*c, n))
            })
            .collect()
    }

    /// Indices of the tools in a category.
    pub fn in_category(&self, c: Category) -> Vec<usize> {
        (0..self.tools.len())
            .filter(|&i| self.tools[i].category == c)
            .collect()
    }

    /// How many tools take files.
    pub fn file_tool_count(&self) -> usize {
        self.tools.iter().filter(|t| t.takes_files()).count()
    }
}

/// The module folders inside `dir`, or `dir` itself when it holds a `module.toml`.
fn module_folders(dir: &Path) -> Vec<PathBuf> {
    if dir.join("module.toml").is_file() {
        return vec![dir.to_path_buf()];
    }
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("module.toml").is_file())
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_follow_the_module_id_family() {
        assert_eq!(Category::from_module_id("pdf.compress"), Category::Pdf);
        assert_eq!(Category::from_module_id("image.resize"), Category::Images);
        assert_eq!(Category::from_module_id("text.word_count"), Category::Text);
        assert_eq!(Category::from_module_id("files.hash"), Category::Files);
        assert_eq!(Category::from_module_id("weird.thing"), Category::Files);
    }

    #[test]
    fn builtins_are_all_in_the_catalog() {
        let c = Catalog::with_dirs(&[]);
        assert_eq!(c.tools().len(), c.registry().len());
        assert!(c.index_of("pdf.compress").is_some());
        assert!(c.index_of("image.resize_to_kb").is_some());
        let counts = c.category_counts();
        let pdf = counts.iter().find(|(c, _)| *c == Category::Pdf).unwrap().1;
        assert!(pdf >= 6, "pdf tools: {pdf}");
        assert!(counts.iter().all(|(_, n)| *n > 0));
    }

    #[test]
    fn community_modules_load_from_a_folder() {
        let examples =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm");
        let c = Catalog::with_dirs(&[examples]);
        // The wasm examples are built in this repo; when they are missing the note says why.
        match c.index_of("text.word_count") {
            Some(i) => {
                assert!(matches!(c.tool(i).source, Source::Community(_)));
                assert_eq!(c.tool(i).category, Category::Text);
            }
            None => assert!(!c.notes.is_empty(), "no module and no note"),
        }
    }

    #[test]
    fn the_community_plugins_are_community_and_sit_in_the_right_category() {
        let examples =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm");
        let c = Catalog::with_dirs(&[examples]);
        if c.index_of("image.qr_code").is_none() {
            return; // wasm examples not built on this machine
        }
        for (id, category) in [
            ("image.qr_code", Category::Images),
            ("doc.markdown_to_html", Category::Documents),
            ("data.format", Category::Developer),
        ] {
            let i = c
                .index_of(id)
                .unwrap_or_else(|| panic!("{id} is not in the catalog"));
            assert!(matches!(c.tool(i).source, Source::Community(_)), "{id}");
            assert_eq!(c.tool(i).category, category, "{id}");
        }
    }

    #[test]
    fn a_broken_module_folder_is_a_note_not_a_crash() {
        let dir = std::env::temp_dir().join(format!("panoply-cat-{}", std::process::id()));
        let m = dir.join("broken");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::write(m.join("module.toml"), "this is not a manifest").unwrap();
        let c = Catalog::with_dirs(std::slice::from_ref(&dir));
        assert_eq!(c.notes.len(), 1);
        assert!(!c.tools().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
