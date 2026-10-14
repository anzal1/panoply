//! The fallback face engine's parts: ONNX Runtime and the YuNet model, each fetched once with a
//! pinned checksum and opened at run time.
//!
//! On macOS the OS engine (Apple Vision) is used and none of this is touched (ADR 0004). Where
//! there is no OS engine that passes the quality bar (Linux, Windows) the fallback runs:
//!
//! * **ONNX Runtime** (MIT) from `github.com/microsoft/onnxruntime`, release 1.28.3, a few tens of
//!   MB, extracted to the engines folder. The `ort` crate only loads it (`load-dynamic`); it is
//!   never linked or committed.
//! * **YuNet** (`face_detection_yunet_2023mar.onnx`, MIT, 232,589 bytes) from the
//!   `opencv/opencv_zoo` repository at a pinned commit.
//!
//! Both checksums are checked before anything is opened. Lookup order for each: an environment
//! variable (`PANOPLY_ORT_LIB`, `PANOPLY_YUNET_MODEL`: a path, no download), the engines folder
//! (`PANOPLY_ENGINES_DIR`, else the user's cache folder), then a download. The network is only
//! touched when the fallback is chosen and the file is missing, and the first line printed says
//! what and how big (FLOWS.md: network access is labelled before it runs).
//!
//! The session is created on first use in a job and dropped after 60 s without use.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use panoply_image::error::{ImageError, Result};

/// What the ONNX Runtime pin says about this release.
pub const ORT_VERSION: &str = "1.28.3";
/// The release's download page, for messages.
const ORT_HOST: &str = "github.com/microsoft/onnxruntime";

struct OrtPin {
    os: &'static str,
    arch: &'static str,
    asset: &'static str,
    sha256: &'static str,
    /// The library inside the archive.
    member: &'static str,
    /// Archive size in bytes, for the label shown before the download.
    size: u64,
}

/// SHA-256 of each archive in release v1.28.3, from the release's own digests (the GitHub
/// release API lists one per asset). The macOS arm64, Linux x64 and Windows x64 archives were also
/// checked against a download. Intel Macs have no archive in this release and use Apple Vision.
const ORT_PINS: &[OrtPin] = &[
    OrtPin {
        os: "macos",
        arch: "aarch64",
        asset: "onnxruntime-osx-arm64-1.28.3.tgz",
        sha256: "c436bd9f47dbce6f6311ccc21de97f829b3c862c09d24aa321958cb72f0a1d32",
        member: "onnxruntime-osx-arm64-1.28.3/lib/libonnxruntime.1.28.3.dylib",
        size: 33_057_743,
    },
    OrtPin {
        os: "linux",
        arch: "x86_64",
        asset: "onnxruntime-linux-x64-1.28.3.tgz",
        sha256: "db14e4863bd37893fc59729d986ab2a0d043d10b7d44da1913c4982b7e3d009c",
        member: "onnxruntime-linux-x64-1.28.3/lib/libonnxruntime.so.1.28.3",
        size: 9_130_098,
    },
    OrtPin {
        os: "linux",
        arch: "aarch64",
        asset: "onnxruntime-linux-aarch64-1.28.3.tgz",
        sha256: "6c6b1ae96d7b0be9f555092857c6a4f2b0b5587a5298d732d9e52e8253038ce7",
        member: "onnxruntime-linux-aarch64-1.28.3/lib/libonnxruntime.so.1.28.3",
        size: 8_123_797,
    },
    OrtPin {
        os: "windows",
        arch: "x86_64",
        asset: "onnxruntime-win-x64-1.28.3.zip",
        sha256: "1d6fab48e85f948436af7c8c971d2c145cf224e2c444755dec894f8b0de11a83",
        member: "onnxruntime-win-x64-1.28.3/lib/onnxruntime.dll",
        size: 78_606_946,
    },
    OrtPin {
        os: "windows",
        arch: "aarch64",
        asset: "onnxruntime-win-arm64-1.28.3.zip",
        sha256: "76d5e5a23fb7fdbc2ca0c3214bad1b58b2a41016c7052e74e60ed8004bea71b1",
        member: "onnxruntime-win-arm64-1.28.3/lib/onnxruntime.dll",
        size: 79_730_664,
    },
];

/// The pinned model: YuNet 2023mar, fp32 (MIT, Shiqi Yu and the OpenCV Zoo).
pub const MODEL_COMMIT: &str = "26cc381e4d2594bb9f47a26eb8fd96c94a13660d";
/// Its checksum.
pub const MODEL_SHA256: &str = "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4";
/// Its size in bytes.
pub const MODEL_BYTES: u64 = 232_589;
const MODEL_FILE: &str = "face_detection_yunet_2023mar.onnx";

fn engine_err(msg: impl Into<String>) -> ImageError {
    ImageError::Unsupported(msg.into())
}

fn ort_pin() -> Option<&'static OrtPin> {
    ORT_PINS
        .iter()
        .find(|p| p.os == std::env::consts::OS && p.arch == std::env::consts::ARCH)
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Where downloaded engines live (shared with the PDF engine).
pub fn engines_dir() -> PathBuf {
    panoply_pdf::pdfium::engines_dir()
}

fn ort_dir() -> PathBuf {
    engines_dir().join(format!("onnxruntime-{ORT_VERSION}"))
}

fn model_dir() -> PathBuf {
    engines_dir().join("models")
}

/// Fetch `url` into `dest` with curl, check the SHA-256, never keep a file that fails.
fn fetch(url: &str, dest: &Path, sha256: &str, what: &str) -> Result<()> {
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "600",
            "--output",
        ])
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| {
            engine_err(format!(
                "Panoply needs {what} for this and could not start curl to download it ({e}). Install curl and try again."
            ))
        })?;
    if !status.success() {
        let _ = std::fs::remove_file(dest);
        return Err(engine_err(format!(
            "Panoply needs {what} for this and could not download it. Check your connection and try again."
        )));
    }
    let got =
        sha256_file(dest).map_err(|e| engine_err(format!("Could not read the download: {e}")))?;
    if got != sha256 {
        let _ = std::fs::remove_file(dest);
        return Err(engine_err(format!(
            "The downloaded {what} did not match its pinned checksum (expected {sha256}, got {got}). It was not used."
        )));
    }
    Ok(())
}

/// Make sure the ONNX Runtime library is on disk and return its path, downloading it once.
pub fn ensure_onnxruntime() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("PANOPLY_ORT_LIB") {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(engine_err(format!(
                "PANOPLY_ORT_LIB points at {}, which is not a file.",
                p.display()
            )))
        };
    }
    let pin = ort_pin().ok_or_else(|| {
        engine_err(format!(
            "There is no prebuilt face engine for {} on {}. Set PANOPLY_ORT_LIB to an ONNX Runtime library.",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    let dir = ort_dir();
    let lib = dir.join(Path::new(pin.member).file_name().unwrap_or_default());
    if lib.is_file() {
        return Ok(lib);
    }
    // Nothing is fetched until the person has said yes to this item (docs/PRIVACY.md).
    panoply_core::assets::authorize(&panoply_core::assets::onnxruntime(ORT_VERSION, pin.size))
        .map_err(|e| engine_err(e.to_string()))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", dir.display())))?;
    eprintln!(
        "Downloading the face engine (ONNX Runtime {ORT_VERSION}, {:.1} MB) from {ORT_HOST}. This happens once.",
        pin.size as f64 / 1e6
    );
    let tmp = dir.join(format!(".part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", tmp.display())))?;
    let result = (|| -> Result<()> {
        let archive = tmp.join(pin.asset);
        fetch(
            &format!(
                "https://{ORT_HOST}/releases/download/v{ORT_VERSION}/{}",
                pin.asset
            ),
            &archive,
            pin.sha256,
            "the face engine",
        )?;
        let extract = tmp.join("x");
        std::fs::create_dir_all(&extract)
            .map_err(|e| engine_err(format!("Could not create a folder: {e}")))?;
        let status = Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&extract)
            .arg(pin.member)
            .status()
            .map_err(|e| {
                engine_err(format!(
                    "Could not start tar to unpack the face engine ({e})."
                ))
            })?;
        let unpacked = extract.join(pin.member);
        if !status.success() || !unpacked.is_file() {
            return Err(engine_err("Could not unpack the face engine."));
        }
        // Rename is atomic on one volume, so another process never sees half a library.
        std::fs::rename(&unpacked, &lib)
            .map_err(|e| engine_err(format!("Could not install the face engine: {e}")))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    Ok(lib)
}

/// Make sure the YuNet model is on disk and return its path, downloading it once.
pub fn ensure_model() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("PANOPLY_YUNET_MODEL") {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(engine_err(format!(
                "PANOPLY_YUNET_MODEL points at {}, which is not a file.",
                p.display()
            )))
        };
    }
    let dir = model_dir();
    let path = dir.join(MODEL_FILE);
    if path.is_file() {
        return Ok(path);
    }
    // Nothing is fetched until the person has said yes to this item (docs/PRIVACY.md).
    panoply_core::assets::authorize(&yunet_asset()).map_err(|e| engine_err(e.to_string()))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", dir.display())))?;
    eprintln!(
        "Downloading the face model (YuNet, MIT license, {:.0} KB) from github.com/opencv/opencv_zoo. This happens once.",
        MODEL_BYTES as f64 / 1e3
    );
    let tmp = dir.join(format!(".part-{}-{MODEL_FILE}", std::process::id()));
    fetch(
        &format!(
            "https://media.githubusercontent.com/media/opencv/opencv_zoo/{MODEL_COMMIT}/models/face_detection_yunet/{MODEL_FILE}"
        ),
        &tmp,
        MODEL_SHA256,
        "the face model",
    )?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| engine_err(format!("Could not install the face model: {e}")))?;
    Ok(path)
}

/// What the consent sheet and the `models` list say about the face model.
pub fn yunet_asset() -> panoply_core::assets::AssetInfo {
    use panoply_core::assets::{AssetInfo, AssetKind};
    AssetInfo {
        id: "yunet".into(),
        name: "YuNet face model".into(),
        kind: AssetKind::Model,
        size: MODEL_BYTES,
        source: "github.com (opencv/opencv_zoo)".into(),
        license: "MIT".into(),
        purpose: "finds faces for passport and ID photos where the system has no finder".into(),
    }
}

/// The downloadable parts of the face engine: the ONNX Runtime and the YuNet model.
pub fn assets() -> Vec<panoply_core::assets::Entry> {
    use panoply_core::assets::Entry;
    let mut v = Vec::new();
    if let Some(pin) = ort_pin() {
        v.push(Entry {
            info: panoply_core::assets::onnxruntime(ORT_VERSION, pin.size),
            paths: vec![ort_dir()],
        });
    }
    v.push(Entry {
        info: yunet_asset(),
        paths: vec![model_dir().join(MODEL_FILE)],
    });
    v
}

/// How long a loaded session stays when nobody uses it (FLOWS.md budgets: models unload after
/// 60 s idle).
pub const IDLE_UNLOAD: Duration = Duration::from_secs(60);

static IDLE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(60_000);

/// Change the idle time before the session unloads. For tests; the default is [`IDLE_UNLOAD`].
#[doc(hidden)]
pub fn set_idle_unload(d: Duration) {
    IDLE_MS.store(d.as_millis() as u64, std::sync::atomic::Ordering::Relaxed);
}

struct Loaded {
    session: ort::session::Session,
    last_used: Instant,
}

static SESSION: Mutex<Option<Loaded>> = Mutex::new(None);
/// Set once the runtime library is open. Only success is remembered, so a failed download (no
/// network the first time) is tried again on the next job instead of failing for the whole run.
static LIB_READY: OnceLock<()> = OnceLock::new();

fn init_runtime() -> Result<()> {
    if LIB_READY.get().is_some() {
        return Ok(());
    }
    let lib = ensure_onnxruntime()?;
    let builder = ort::init_from(&lib).map_err(|e| {
        ImageError::Unsupported(format!(
            "The face engine at {} could not be loaded: {e}",
            lib.display()
        ))
    })?;
    // `false` means another part of Panoply configured the runtime first: its settings stand.
    let _ = builder.with_name("panoply").commit();
    let _ = LIB_READY.set(());
    Ok(())
}

fn watch_idle() {
    std::thread::spawn(|| {
        loop {
            let idle = Duration::from_millis(IDLE_MS.load(std::sync::atomic::Ordering::Relaxed));
            std::thread::sleep(
                (idle / 4).clamp(Duration::from_millis(100), Duration::from_secs(5)),
            );
            let mut g = SESSION.lock().unwrap_or_else(|p| p.into_inner());
            match g.as_ref() {
                Some(l) if l.last_used.elapsed() >= idle => {
                    *g = None;
                    return;
                }
                None => return,
                _ => {}
            }
        }
    });
}

/// Run `f` with the YuNet session, creating it on first use. Jobs are serialised: one inference
/// at a time per process.
pub fn with_session<T>(f: impl FnOnce(&mut ort::session::Session) -> Result<T>) -> Result<T> {
    let mut g = SESSION.lock().unwrap_or_else(|p| p.into_inner());
    if g.is_none() {
        init_runtime()?;
        let model = ensure_model()?;
        let bytes = std::fs::read(&model).map_err(|e| {
            engine_err(format!(
                "Could not read the face model {}: {e}",
                model.display()
            ))
        })?;
        let open_err = |e: String| engine_err(format!("The face model could not be opened: {e}"));
        let mut builder = ort::session::Session::builder().map_err(|e| open_err(e.to_string()))?;
        builder = builder
            .with_intra_threads(2)
            .map_err(|e| open_err(e.to_string()))?;
        let session = builder
            .commit_from_memory(&bytes)
            .map_err(|e| open_err(e.to_string()))?;
        *g = Some(Loaded {
            session,
            last_used: Instant::now(),
        });
        watch_idle();
    }
    let loaded = g.as_mut().expect("session was just created");
    let out = f(&mut loaded.session);
    loaded.last_used = Instant::now();
    out
}

/// True when a session is currently loaded (for tests of the idle unload).
pub fn session_loaded() -> bool {
    SESSION.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_has_a_full_checksum_and_a_member() {
        for p in ORT_PINS {
            assert_eq!(p.sha256.len(), 64, "{}", p.asset);
            assert!(p.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(p.member.contains("/lib/"), "{}", p.asset);
            assert!(p.asset.contains(ORT_VERSION), "{}", p.asset);
        }
        assert_eq!(MODEL_SHA256.len(), 64);
        for (os, arch) in [
            ("macos", "aarch64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("windows", "x86_64"),
        ] {
            assert!(
                ORT_PINS.iter().any(|p| p.os == os && p.arch == arch),
                "{os} {arch}"
            );
        }
    }

    #[test]
    fn engines_live_outside_the_repository() {
        assert!(!engines_dir().starts_with(env!("CARGO_MANIFEST_DIR")));
    }
}
