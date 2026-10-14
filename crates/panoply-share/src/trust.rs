//! Trusted devices and the four word check code.
//!
//! Trust is a stored fingerprint. A device that was trusted once is accepted without a prompt
//! (when the receiver is set to allow that). The check code lets two people confirm by eye that
//! both screens show the same pairing before they trust a device.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::proto::{is_fingerprint, iso8601};
use crate::words::WORDS;

/// One remembered device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustedDevice {
    /// Certificate fingerprint, the identity.
    pub fingerprint: String,
    /// Name it had when it was trusted.
    pub alias: String,
    /// When it was trusted (UTC, ISO 8601).
    pub added: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Doc {
    devices: Vec<TrustedDevice>,
}

/// The set of trusted fingerprints, optionally backed by a JSON file.
#[derive(Debug)]
pub struct TrustStore {
    path: Option<PathBuf>,
    devices: Mutex<BTreeMap<String, TrustedDevice>>,
}

impl TrustStore {
    /// A store that lives only in memory.
    pub fn in_memory() -> Self {
        TrustStore {
            path: None,
            devices: Mutex::default(),
        }
    }

    /// Open (or start) the store at `path`. An unreadable file starts an empty store rather than
    /// failing, and is replaced on the next change.
    pub fn open(path: &Path) -> Self {
        let devices = fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Doc>(&b).ok())
            .map(|d| {
                d.devices
                    .into_iter()
                    .map(|t| (t.fingerprint.clone(), t))
                    .collect()
            })
            .unwrap_or_default();
        TrustStore {
            path: Some(path.to_path_buf()),
            devices: Mutex::new(devices),
        }
    }

    /// True when `fingerprint` was trusted.
    pub fn is_trusted(&self, fingerprint: &str) -> bool {
        self.devices.lock().unwrap().contains_key(fingerprint)
    }

    /// Trust a device. Only well formed certificate fingerprints are stored.
    pub fn trust(&self, fingerprint: &str, alias: &str) -> Result<()> {
        if !is_fingerprint(fingerprint) {
            return Ok(());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.devices.lock().unwrap().insert(
            fingerprint.to_string(),
            TrustedDevice {
                fingerprint: fingerprint.to_string(),
                alias: alias.to_string(),
                added: iso8601(now),
            },
        );
        self.save()
    }

    /// Stop trusting a device.
    pub fn forget(&self, fingerprint: &str) -> Result<()> {
        self.devices.lock().unwrap().remove(fingerprint);
        self.save()
    }

    /// Every trusted device.
    pub fn list(&self) -> Vec<TrustedDevice> {
        self.devices.lock().unwrap().values().cloned().collect()
    }

    /// True when a trusted device with this alias exists under a different fingerprint, the
    /// sign of someone borrowing a known name.
    pub fn alias_conflict(&self, alias: &str, fingerprint: &str) -> bool {
        self.devices
            .lock()
            .unwrap()
            .values()
            .any(|t| t.alias == alias && t.fingerprint != fingerprint)
    }

    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let doc = Doc {
            devices: self.list(),
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&doc)?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Four words both devices derive from the two fingerprints. The order of the arguments does
/// not matter, so both screens show the same code.
pub fn check_code(a: &str, b: &str) -> String {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let mut h = Sha256::new();
    h.update(b"panoply-share-check-code");
    h.update(lo.as_bytes());
    h.update([0]);
    h.update(hi.as_bytes());
    let d = h.finalize();
    format!(
        "{} {} {} {}",
        WORDS[d[0] as usize], WORDS[d[1] as usize], WORDS[d[2] as usize], WORDS[d[3] as usize]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn check_code_is_symmetric_and_four_words() {
        assert_eq!(check_code(A, B), check_code(B, A));
        assert_eq!(check_code(A, B).split(' ').count(), 4);
        assert_ne!(check_code(A, B), check_code(A, A));
    }

    #[test]
    fn trust_persists_and_detects_alias_reuse() {
        let path = std::env::temp_dir().join(format!("panoply-trust-{}.json", std::process::id()));
        let _ = fs::remove_file(&path);
        let s = TrustStore::open(&path);
        s.trust(A, "Anzal's iPhone").unwrap();
        s.trust("not-a-fingerprint", "x").unwrap();
        assert!(TrustStore::open(&path).is_trusted(A));
        assert!(!s.is_trusted("not-a-fingerprint"));
        assert!(s.alias_conflict("Anzal's iPhone", B));
        assert!(!s.alias_conflict("Anzal's iPhone", A));
        s.forget(A).unwrap();
        assert!(!TrustStore::open(&path).is_trusted(A));
        let _ = fs::remove_file(&path);
    }
}
