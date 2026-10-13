//! `files.hash`: SHA-256 of each input. Writes nothing.
//!
//! Files are read in 1 MiB chunks, so memory stays flat for a 20 GB file. The digest of each
//! file comes back as a report line `<hex>  <path>`, the same shape as `sha256sum`.

use std::io::Read;
use std::path::{Path, PathBuf};

use panoply_manifest::Manifest;
use sha2::{Digest, Sha256};

use crate::{Module, ModuleError, RunContext};

/// Read size. One buffer is allocated per run and reused.
pub const CHUNK: usize = 1024 * 1024;

const MANIFEST: &str = include_str!("../../../../modules/examples/hash/module.toml");

/// The `files.hash` module.
pub struct HashModule {
    manifest: Manifest,
}

impl HashModule {
    /// Build the module from its embedded manifest.
    pub fn new() -> Self {
        let manifest = Manifest::from_toml_str(MANIFEST).expect("hash manifest parses");
        Self { manifest }
    }
}

impl Default for HashModule {
    fn default() -> Self {
        Self::new()
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0xf) as usize] as char);
    }
    s
}

/// Stream a reader through SHA-256 in [`CHUNK`]-sized reads. `on_bytes` gets the running
/// byte count after each chunk, and the cancel check runs between chunks.
pub fn sha256_stream(
    mut reader: impl Read,
    buf: &mut [u8],
    path: &Path,
    ctx: &RunContext,
    mut on_bytes: impl FnMut(u64),
) -> Result<String, ModuleError> {
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    loop {
        ctx.check_cancel()?;
        let n = reader.read(buf).map_err(|source| ModuleError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
        on_bytes(total);
    }
    Ok(hex(hasher.finalize().as_slice()))
}

impl Module for HashModule {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run(
        &self,
        ctx: &RunContext,
        inputs: &[PathBuf],
        _options: &serde_json::Value,
    ) -> Result<Vec<PathBuf>, ModuleError> {
        let mut buf = vec![0u8; CHUNK];
        let count = inputs.len().max(1) as f32;
        for (i, path) in inputs.iter().enumerate() {
            let file = ctx.guard.open_read(path)?;
            let len = file.metadata().map(|m| m.len()).unwrap_or(0).max(1);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut last_step = 0u64;
            let digest = sha256_stream(file, &mut buf, path, ctx, |done| {
                // Report at most every 1 percent of the file, so a huge file stays quiet.
                let step = done * 100 / len;
                if step != last_step {
                    last_step = step;
                    ctx.progress(
                        (i as f32 + done as f32 / len as f32) / count,
                        format!("Hashing {name}"),
                    );
                }
            })?;
            ctx.note(format!("{digest}  {}", path.display()));
            ctx.progress((i + 1) as f32 / count, format!("Hashed {name}"));
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CapabilityGuard;
    use panoply_core::{CancelToken, JobEvent, JobSpec};
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-hash-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn run_on(file: &Path) -> Vec<String> {
        let m = HashModule::new();
        let guard = CapabilityGuard::new(&[file.to_path_buf()], None, false).unwrap();
        let ctx = RunContext::new(guard);
        let out = m
            .run(&ctx, &[file.to_path_buf()], &serde_json::Value::Null)
            .unwrap();
        assert!(out.is_empty(), "files.hash writes nothing");
        ctx.take_notes()
    }

    #[test]
    fn manifest_is_valid_and_read_only() {
        let m = HashModule::new();
        m.manifest().validate().unwrap();
        assert_eq!(m.manifest().module.id, "files.hash");
        assert_eq!(m.manifest().module.pattern.letter(), 'I');
    }

    #[test]
    fn known_vectors() {
        let d = scratch("vec");
        let abc = d.join("abc.txt");
        let empty = d.join("empty.txt");
        std::fs::write(&abc, "abc").unwrap();
        std::fs::write(&empty, "").unwrap();
        assert!(
            run_on(&abc)[0]
                .starts_with("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  ")
        );
        assert!(
            run_on(&empty)[0]
                .starts_with("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  ")
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn multi_chunk_file_matches_one_shot_digest() {
        let d = scratch("big");
        let f = d.join("big.bin");
        // 2.5 chunks of a repeating pattern, so chunk boundaries matter.
        let data: Vec<u8> = (0..(CHUNK * 5 / 2)).map(|i| (i % 251) as u8).collect();
        std::fs::File::create(&f).unwrap().write_all(&data).unwrap();
        let expected = hex(Sha256::digest(&data).as_slice());
        assert!(run_on(&f)[0].starts_with(&expected));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn cancel_stops_before_reading() {
        let d = scratch("cancel");
        let f = d.join("a.bin");
        std::fs::write(&f, vec![1u8; 4096]).unwrap();
        let guard = CapabilityGuard::new(std::slice::from_ref(&f), None, false).unwrap();
        let ctx = RunContext::new(guard);
        ctx.cancel.cancel();
        let r = HashModule::new().run(&ctx, &[f], &serde_json::Value::Null);
        assert!(matches!(r, Err(ModuleError::Cancelled)));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn files_outside_the_grant_are_refused() {
        let d = scratch("deny");
        let chosen = d.join("chosen.txt");
        let other = d.join("other.txt");
        std::fs::write(&chosen, "a").unwrap();
        std::fs::write(&other, "b").unwrap();
        let guard = CapabilityGuard::new(&[chosen], None, false).unwrap();
        let ctx = RunContext::new(guard);
        let r = HashModule::new().run(&ctx, &[other], &serde_json::Value::Null);
        assert!(matches!(r, Err(ModuleError::NotAllowed(_))));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn runs_through_the_registry_with_events() {
        let d = scratch("reg");
        let f = d.join("abc.txt");
        std::fs::write(&f, "abc").unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let e2 = events.clone();
        let spec = JobSpec {
            module_id: "files.hash".into(),
            inputs: vec![f],
            options: Default::default(),
        };
        let out = crate::Registry::with_builtins()
            .run_job(&spec, None, CancelToken::new(), move |e| {
                e2.lock().unwrap().push(e)
            })
            .unwrap();
        assert_eq!(out.notes.len(), 1);
        assert!(out.outputs.is_empty());
        let ev = events.lock().unwrap();
        assert!(matches!(ev.last(), Some(JobEvent::Done { .. })));
        assert!(ev.iter().any(|e| matches!(e, JobEvent::Progress { .. })));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
