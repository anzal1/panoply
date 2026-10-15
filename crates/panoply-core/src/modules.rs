//! Where community modules are looked for. The app and the command line share this list, so a
//! module that shows in one shows in the other.

use std::path::PathBuf;

/// Where the user drops community modules: one folder per module with its `module.toml`.
pub fn user_modules_dir() -> PathBuf {
    crate::assets::data_dir().join("modules")
}

/// Every folder scanned for community modules, in order: `PANOPLY_MODULES` (several allowed,
/// separated like `PATH`), the user folder, then the folder shipped with the program. A macOS
/// bundle ships it as `Contents/Resources/modules`; every other install has a `modules` folder next
/// to the executable (Windows install folder and zip, Linux `bin/` in the tarball and AppImage, and
/// next to the command line tool in its archive). Only folders that exist are returned.
pub fn search_dirs() -> Vec<PathBuf> {
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
    dirs.retain(|d| d.is_dir());
    dirs.dedup();
    dirs
}

/// The module folders inside one search folder: each sub-folder with a `module.toml`, sorted, or
/// the folder itself when it is one.
pub fn module_folders(dir: &std::path::Path) -> Vec<PathBuf> {
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
    fn folders_are_the_subfolders_with_a_manifest() {
        let d = std::env::temp_dir().join(format!("panoply-modfolders-{}", std::process::id()));
        for (name, manifest) in [("b", true), ("a", true), ("notes", false)] {
            std::fs::create_dir_all(d.join(name)).unwrap();
            if manifest {
                std::fs::write(d.join(name).join("module.toml"), "").unwrap();
            }
        }
        let found: Vec<String> = module_folders(&d)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(found, ["a", "b"]);
        std::fs::write(d.join("module.toml"), "").unwrap();
        assert_eq!(module_folders(&d), vec![d.clone()]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn panoply_modules_comes_first() {
        let d = std::env::temp_dir();
        // search_dirs keeps only folders that exist, and PANOPLY_MODULES leads the list.
        let dirs = {
            // SAFETY: a test that sets one variable no other test in this crate reads.
            unsafe { std::env::set_var("PANOPLY_MODULES", &d) };
            let v = search_dirs();
            unsafe { std::env::remove_var("PANOPLY_MODULES") };
            v
        };
        assert_eq!(dirs.first(), Some(&d));
    }
}
