//! Where Panoply keeps its data and looks for community modules.

use std::path::PathBuf;

/// The per-user data folder. `PANOPLY_DATA_DIR` overrides it (tests and portable installs). The
/// consent choices live here too, so the app, the CLI and the tests agree on one place.
pub fn data_dir() -> PathBuf {
    panoply_core::assets::data_dir()
}

/// Where the user drops community modules: one folder per module with its `module.toml`.
pub fn user_modules_dir() -> PathBuf {
    data_dir().join("modules")
}

/// Every folder scanned for community modules, in order.
///
/// `PANOPLY_MODULES` (several allowed, separated like `PATH`), the user folder, the folder shipped
/// inside the app (`Resources/modules` in a macOS bundle, `modules` next to the executable
/// elsewhere), and in debug builds the example modules in the source tree.
pub fn module_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(v) = std::env::var_os("PANOPLY_MODULES") {
        dirs.extend(std::env::split_paths(&v));
    }
    dirs.push(user_modules_dir());
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin_dir) = exe.parent()
    {
        if cfg!(target_os = "macos")
            && let Some(contents) = bin_dir.parent()
        {
            dirs.push(contents.join("Resources/modules"));
        }
        dirs.push(bin_dir.join("modules"));
    }
    if cfg!(debug_assertions) {
        dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm"));
    }
    dirs.retain(|d| d.is_dir());
    dirs.dedup();
    dirs
}

/// The job history file.
pub fn history_file() -> PathBuf {
    data_dir().join("history.jsonl")
}

/// The folder for the file index: the saved index, and the file system event ids that go with it.
pub fn find_dir() -> PathBuf {
    data_dir().join("find")
}
