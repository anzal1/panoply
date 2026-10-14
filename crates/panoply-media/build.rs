//! Lists the preset data files (upload limits, loudness targets) so the library can embed them
//! with `include_str!` without a hand-kept table: dropping a new `.toml` into a presets folder
//! is enough.

use std::fmt::Write;
use std::path::{Path, PathBuf};

fn list(root: &Path, folder: &str, const_name: &str, out_name: &str) {
    let dir = root
        .join("../../modules/builtin")
        .join(folder)
        .join("presets");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut out = format!(
        "/// Every shipped preset file as (file stem, TOML text).\npub const {const_name}: &[(&str, &str)] = &[\n"
    );
    for f in &files {
        let stem = f.file_stem().unwrap().to_string_lossy();
        let abs = f.canonicalize().unwrap();
        let _ = writeln!(
            out,
            "    ({:?}, include_str!({:?})),",
            stem,
            abs.display().to_string()
        );
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join(out_name);
    std::fs::write(dest, out).unwrap();
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    list(
        &root,
        "media-compress",
        "SIZE_PRESET_FILES",
        "size_presets_gen.rs",
    );
    list(
        &root,
        "media-normalize",
        "LOUDNESS_PRESET_FILES",
        "loudness_presets_gen.rs",
    );
}
