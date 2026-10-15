//! What the walker never descends into.
//!
//! An excluded folder still appears in the index as a name (so "node_modules" is findable) but its
//! contents are not read. The root a caller asks for is never excluded, so a scan of a folder whose own
//! name ends in `.noindex` works when it is asked for by name.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The exclusion policy for one scan.
#[derive(Clone, Debug)]
pub struct Exclusions {
    /// Folder names (folded: lowercase, accent free) never entered, anywhere.
    pub names: Vec<Vec<u8>>,
    /// Folder name suffixes (folded) never entered: `.noindex`, `.photoslibrary`, `.app`.
    pub suffixes: Vec<Vec<u8>>,
    /// Absolute folders never entered.
    pub paths: HashSet<PathBuf>,
    /// Mount points of network file systems. Never entered unless the user said yes.
    pub network_mounts: HashSet<PathBuf>,
    /// Skip a folder called `target` that sits next to a `Cargo.toml` (a Rust build folder).
    pub cargo_target: bool,
    /// Stay on the file system of the root (do not cross into other volumes or bind mounts).
    pub one_file_system: bool,
}

impl Exclusions {
    /// Nothing excluded.
    pub fn none() -> Self {
        Exclusions {
            names: Vec::new(),
            suffixes: Vec::new(),
            paths: HashSet::new(),
            network_mounts: HashSet::new(),
            cargo_target: false,
            one_file_system: false,
        }
    }

    /// The default policy: dependency and build folders, version-control internals, trash and index
    /// folders, `*.noindex`, app bundles, the system folders of the running OS, and network mounts.
    pub fn defaults() -> Self {
        let names = [
            "node_modules",
            "__pycache__",
            ".git",
            ".hg",
            ".svn",
            ".trash",
            ".trashes",
            ".spotlight-v100",
            ".fseventsd",
            ".documentrevisions-v100",
            ".temporaryitems",
            "$recycle.bin",
            "system volume information",
            "lost+found",
        ];
        let suffixes = [".noindex", ".photoslibrary", ".app", ".framework"];
        let mut paths: HashSet<PathBuf> = HashSet::new();
        for p in system_paths() {
            paths.insert(PathBuf::from(p));
        }
        Exclusions {
            names: names.iter().map(|s| s.as_bytes().to_vec()).collect(),
            suffixes: suffixes.iter().map(|s| s.as_bytes().to_vec()).collect(),
            paths,
            network_mounts: crate::mounts::network_mounts().into_iter().collect(),
            cargo_target: true,
            one_file_system: true,
        }
    }

    /// Adds an absolute folder to skip.
    pub fn with_path(mut self, p: impl Into<PathBuf>) -> Self {
        self.paths.insert(p.into());
        self
    }

    /// True when a folder with this folded name must not be entered.
    pub fn name_excluded(&self, folded: &[u8]) -> bool {
        self.names.iter().any(|n| n.as_slice() == folded)
            || self.suffixes.iter().any(|s| folded.ends_with(s))
    }

    /// True when this absolute folder must not be entered (system folder or network mount).
    pub fn path_excluded(&self, p: &Path) -> bool {
        self.paths.contains(p) || self.network_mounts.contains(p)
    }
}

#[cfg(target_os = "macos")]
fn system_paths() -> &'static [&'static str] {
    &[
        "/System",
        "/private",
        "/cores",
        "/dev",
        "/Volumes",
        "/usr",
        "/bin",
        "/sbin",
        "/Library",
        "/opt/homebrew/Cellar",
    ]
}

#[cfg(target_os = "linux")]
fn system_paths() -> &'static [&'static str] {
    &[
        "/proc",
        "/sys",
        "/dev",
        "/run",
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib64",
        "/boot",
        "/var/lib",
        "/var/cache",
        "/snap",
    ]
}

#[cfg(windows)]
fn system_paths() -> &'static [&'static str] {
    &[
        "C:\\Windows",
        "C:\\$Recycle.Bin",
        "C:\\System Volume Information",
        "C:\\ProgramData\\Microsoft",
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn system_paths() -> &'static [&'static str] {
    &[]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_suffixes() {
        let e = Exclusions::defaults();
        assert!(e.name_excluded(b"node_modules"));
        assert!(e.name_excluded(b"panoply-shared.noindex"));
        assert!(e.name_excluded(b"safari.app"));
        assert!(!e.name_excluded(b"documents"));
        assert!(!e.name_excluded(b"application support"));
    }
}
