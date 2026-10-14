use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use crate::ModuleError;

/// The capability contract for one job.
///
/// Reading is allowed only for the chosen input paths (and anything beneath a chosen
/// folder). Writing is allowed only inside the chosen output folder, one new file at a time.
/// Network is off unless the manifest asked for it and the user approved.
#[derive(Debug, Clone)]
pub struct CapabilityGuard {
    read_roots: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    network: bool,
}

fn io_err(path: &Path, source: std::io::Error) -> ModuleError {
    ModuleError::Io {
        path: path.to_path_buf(),
        source,
    }
}

impl CapabilityGuard {
    /// Grant reads on `inputs` and writes inside `out_dir`. Paths are resolved up front, so
    /// symlinks cannot be used to step outside the grant later.
    pub fn new(
        inputs: &[PathBuf],
        out_dir: Option<&Path>,
        network: bool,
    ) -> Result<Self, ModuleError> {
        let read_roots = inputs
            .iter()
            .map(|p| p.canonicalize().map_err(|e| io_err(p, e)))
            .collect::<Result<Vec<_>, _>>()?;
        let out_dir = match out_dir {
            Some(d) => Some(d.canonicalize().map_err(|e| io_err(d, e))?),
            None => None,
        };
        Ok(Self {
            read_roots,
            out_dir,
            network,
        })
    }

    /// A guard that grants nothing. Useful for modules that take no files.
    pub fn none() -> Self {
        Self {
            read_roots: Vec::new(),
            out_dir: None,
            network: false,
        }
    }

    /// The output folder, if one was chosen.
    pub fn out_dir(&self) -> Option<&Path> {
        self.out_dir.as_deref()
    }

    /// True when the path is one of the chosen inputs or beneath a chosen folder.
    pub fn can_read(&self, path: &Path) -> bool {
        match path.canonicalize() {
            Ok(real) => self.read_roots.iter().any(|root| real.starts_with(root)),
            Err(_) => false,
        }
    }

    /// Where results for `input` go: the chosen output folder, or the folder the input is in.
    /// A result is only ever placed next to a file the user chose, never anywhere else.
    pub fn output_dir_for(&self, input: &Path) -> Result<PathBuf, ModuleError> {
        if let Some(d) = &self.out_dir {
            return Ok(d.clone());
        }
        if !self.can_read(input) {
            return Err(ModuleError::NotAllowed(input.to_path_buf()));
        }
        let real = input.canonicalize().map_err(|e| io_err(input, e))?;
        real.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| ModuleError::NotAllowed(input.to_path_buf()))
    }

    /// Open a chosen input for reading.
    pub fn open_read(&self, path: &Path) -> Result<File, ModuleError> {
        if !self.can_read(path) {
            // A missing file is reported as missing, not as denied, when it was chosen.
            return match File::open(path) {
                Err(e) if e.kind() == ErrorKind::NotFound => Err(io_err(path, e)),
                _ => Err(ModuleError::NotAllowed(path.to_path_buf())),
            };
        }
        File::open(path).map_err(|e| io_err(path, e))
    }

    /// Create a new file inside the output folder. `file_name` must be a bare name: no
    /// folders, no `..`. An existing file is never overwritten.
    pub fn create_output(&self, file_name: &str) -> Result<(PathBuf, File), ModuleError> {
        let dir = self
            .out_dir
            .as_ref()
            .ok_or_else(|| ModuleError::NotAllowed(PathBuf::from(file_name)))?;
        let name = Path::new(file_name);
        let mut parts = name.components();
        let bare = matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none();
        if !bare {
            return Err(ModuleError::NotAllowed(dir.join(name)));
        }
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => Ok((path, f)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Err(ModuleError::OutputExists(path)),
            Err(e) => Err(io_err(&path, e)),
        }
    }

    /// Ask for the network. Fails unless the manifest requested it and the user approved.
    pub fn require_network(&self) -> Result<(), ModuleError> {
        if self.network {
            Ok(())
        } else {
            Err(ModuleError::NetworkDenied)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-guard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reads_chosen_but_not_others() {
        let d = scratch("read");
        let chosen = d.join("chosen.txt");
        let other = d.join("other.txt");
        std::fs::write(&chosen, "a").unwrap();
        std::fs::write(&other, "b").unwrap();
        let g = CapabilityGuard::new(std::slice::from_ref(&chosen), None, false).unwrap();
        assert!(g.open_read(&chosen).is_ok());
        assert!(matches!(
            g.open_read(&other),
            Err(ModuleError::NotAllowed(_))
        ));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn dotdot_cannot_escape_a_chosen_folder() {
        let d = scratch("dots");
        let inner = d.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(d.join("secret.txt"), "s").unwrap();
        std::fs::write(inner.join("ok.txt"), "o").unwrap();
        let g = CapabilityGuard::new(std::slice::from_ref(&inner), None, false).unwrap();
        assert!(g.can_read(&inner.join("ok.txt")));
        assert!(!g.can_read(&inner.join("..").join("secret.txt")));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_out_of_the_grant_is_denied() {
        let d = scratch("link");
        let inner = d.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(d.join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(d.join("secret.txt"), inner.join("link.txt")).unwrap();
        let g = CapabilityGuard::new(std::slice::from_ref(&inner), None, false).unwrap();
        assert!(!g.can_read(&inner.join("link.txt")));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn writes_only_inside_out_dir_and_never_overwrites() {
        let d = scratch("write");
        let out = d.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let g = CapabilityGuard::new(&[], Some(&out), false).unwrap();
        let (p, mut f) = g.create_output("a.txt").unwrap();
        f.write_all(b"x").unwrap();
        assert!(p.starts_with(g.out_dir().unwrap()));
        assert!(matches!(
            g.create_output("a.txt"),
            Err(ModuleError::OutputExists(_))
        ));
        assert!(matches!(
            g.create_output("../evil.txt"),
            Err(ModuleError::NotAllowed(_))
        ));
        assert!(matches!(
            g.create_output("sub/evil.txt"),
            Err(ModuleError::NotAllowed(_))
        ));
        assert!(matches!(
            g.create_output("/tmp/evil.txt"),
            Err(ModuleError::NotAllowed(_))
        ));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn no_out_dir_means_no_writes() {
        let g = CapabilityGuard::none();
        assert!(matches!(
            g.create_output("a.txt"),
            Err(ModuleError::NotAllowed(_))
        ));
    }

    #[test]
    fn network_is_off_by_default() {
        assert!(matches!(
            CapabilityGuard::none().require_network(),
            Err(ModuleError::NetworkDenied)
        ));
        let g = CapabilityGuard::new(&[], None, true).unwrap();
        assert!(g.require_network().is_ok());
    }

    #[test]
    fn missing_chosen_input_fails_at_grant_time() {
        let r = CapabilityGuard::new(&[PathBuf::from("/no/such/file.bin")], None, false);
        assert!(matches!(r, Err(ModuleError::Io { .. })));
    }
}
