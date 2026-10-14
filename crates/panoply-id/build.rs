//! Lists the country rule files so the library can embed them (`include_str!`) without a
//! hand-kept table: dropping a new `.toml` into the rules folder is enough.

use std::fmt::Write;
use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dir = root.join("../../modules/builtin/id-passport-photo/rules");
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
    let mut out = String::from(
        "/// Every shipped rule file as (file stem, TOML text).\npub const RULE_FILES: &[(&str, &str)] = &[\n",
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
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("rules_gen.rs");
    std::fs::write(dest, out).unwrap();
}
