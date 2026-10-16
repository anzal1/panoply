//! Where Panoply keeps its data and looks for community modules.

use std::path::PathBuf;

/// The per-user data folder. `PANOPLY_DATA_DIR` overrides it (tests and portable installs). The
/// consent choices live here too, so the app, the CLI and the tests agree on one place.
pub fn data_dir() -> PathBuf {
    panoply_core::assets::data_dir()
}

/// Where the user drops community modules: one folder per module with its `module.toml`.
pub fn user_modules_dir() -> PathBuf {
    panoply_core::modules::user_modules_dir()
}

/// Every folder scanned for community modules, in order.
///
/// The folders the command line scans too (`panoply_core::modules::search_dirs`: `PANOPLY_MODULES`,
/// the user folder, the folder shipped inside the app), and in debug builds the example modules in
/// the source tree.
pub fn module_search_dirs() -> Vec<PathBuf> {
    let mut dirs = panoply_core::modules::search_dirs();
    if cfg!(debug_assertions) {
        let examples =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm");
        if examples.is_dir() {
            dirs.push(examples);
        }
    }
    dirs.dedup();
    dirs
}

/// The job history file.
pub fn history_file() -> PathBuf {
    data_dir().join("history.jsonl")
}
