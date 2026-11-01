//! The pdfium engine: finding it, fetching it once with a checksum, and loading it lazily.
//!
//! pdfium (BSD-3-Clause and Apache-2.0) is not linked and not in the repository. The first job
//! that needs to draw a page downloads the prebuilt library from
//! `github.com/bblanchon/pdfium-binaries` at a pinned release, checks its SHA-256 against the
//! table below, and keeps it in a cache folder. Jobs that only edit structure (merge, split,
//! organize) never touch pdfium, so they never download or load it. Compress uses it only to
//! decode JPEG 2000 and JBIG2 pictures, which nothing else here can decode.
//!
//! Lookup order: `PANOPLY_PDFIUM_LIB` (a path to a pdfium library, no download), then the cache
//! folder (`PANOPLY_ENGINES_DIR`, else the user's cache folder), then a download.
//!
//! pdfium-render keeps its bindings in a process-wide cell and offers no unload, so once loaded
//! the library stays mapped until the process ends. That is the known exception to "no memory
//! held when idle"; the CLI and the bench worker are one process per job, and the apps run jobs
//! that need pdfium in a helper process.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use pdfium_render::prelude::*;
use sha2::{Digest, Sha256};

use crate::error::{PdfError, Result};

/// The pinned release of pdfium-binaries.
pub const RELEASE: &str = "chromium/7881";
/// What that release contains, for messages.
pub const VERSION: &str = "pdfium 151.0.7881.0";

struct Pin {
    os: &'static str,
    arch: &'static str,
    asset: &'static str,
    sha256: &'static str,
    /// The library inside the archive.
    member: &'static str,
    /// Download size in bytes, for the label shown before the download.
    size: u64,
}

/// SHA-256 of each archive in release chromium/7881, taken from the release's own digests
/// (the GitHub release API lists one per asset). The macOS arm64 archive was also checked against
/// a download; the others match the published digest.
const PINS: &[Pin] = &[
    Pin {
        os: "macos",
        arch: "aarch64",
        asset: "pdfium-mac-arm64.tgz",
        sha256: "52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40",
        member: "lib/libpdfium.dylib",
        size: 3_533_019,
    },
    Pin {
        os: "macos",
        arch: "x86_64",
        asset: "pdfium-mac-x64.tgz",
        sha256: "6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b",
        member: "lib/libpdfium.dylib",
        size: 3_588_813,
    },
    Pin {
        os: "linux",
        arch: "x86_64",
        asset: "pdfium-linux-x64.tgz",
        sha256: "1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d",
        member: "lib/libpdfium.so",
        size: 3_644_759,
    },
    Pin {
        os: "linux",
        arch: "aarch64",
        asset: "pdfium-linux-arm64.tgz",
        sha256: "ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674",
        member: "lib/libpdfium.so",
        size: 3_588_127,
    },
    Pin {
        os: "windows",
        arch: "x86_64",
        asset: "pdfium-win-x64.tgz",
        sha256: "73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08",
        member: "bin/pdfium.dll",
        size: 3_733_154,
    },
    Pin {
        os: "windows",
        arch: "aarch64",
        asset: "pdfium-win-arm64.tgz",
        sha256: "d3035d4d2cacac6ecd1a2ece197a3d702a1b2a58466276b9f870b8cb278a9d84",
        member: "bin/pdfium.dll",
        size: 3_522_432,
    },
];

fn pin() -> Option<&'static Pin> {
    PINS.iter()
        .find(|p| p.os == std::env::consts::OS && p.arch == std::env::consts::ARCH)
}

/// Where downloaded engines live.
pub fn engines_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("PANOPLY_ENGINES_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        home.unwrap_or_default()
            .join("Library/Caches/Panoply/engines")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join("Panoply/cache/engines")
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".cache")))
            .unwrap_or_default()
            .join("panoply/engines")
    }
}

fn lib_dir() -> PathBuf {
    engines_dir().join(format!(
        "pdfium-{}",
        RELEASE.rsplit('/').next().unwrap_or("x")
    ))
}

fn lib_file(p: &Pin) -> PathBuf {
    lib_dir().join(Path::new(p.member).file_name().unwrap_or_default())
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

fn engine_err(msg: impl Into<String>) -> PdfError {
    PdfError::Engine(msg.into())
}

/// Make sure the pdfium library is on disk and return its path, downloading it once if needed.
pub fn ensure_library() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("PANOPLY_PDFIUM_LIB") {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(engine_err(format!(
                "PANOPLY_PDFIUM_LIB points at {}, which is not a file.",
                p.display()
            )))
        };
    }
    let pin = pin().ok_or_else(|| {
        engine_err(format!(
            "There is no prebuilt PDF engine for {} on {}. Set PANOPLY_PDFIUM_LIB to a pdfium library.",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    let lib = lib_file(pin);
    if lib.is_file() {
        return Ok(lib);
    }
    download(pin, &lib)?;
    Ok(lib)
}

/// What the consent sheet and the `models` list say about the PDF engine on this platform.
pub fn asset() -> panoply_core::assets::AssetInfo {
    use panoply_core::assets::{AssetInfo, AssetKind};
    AssetInfo {
        id: "pdfium".into(),
        name: format!("PDF engine ({VERSION})"),
        kind: AssetKind::Engine,
        size: pin().map_or(0, |p| p.size),
        source: "github.com".into(),
        license: "BSD-3-Clause AND Apache-2.0".into(),
        purpose: "draws PDF pages for previews, images and scans".into(),
    }
}

/// The PDF engine as a listing entry: where it is kept once downloaded.
pub fn entry() -> panoply_core::assets::Entry {
    panoply_core::assets::Entry {
        info: asset(),
        paths: vec![lib_dir()],
    }
}

fn download(pin: &Pin, lib: &Path) -> Result<()> {
    // Nothing is fetched until the person has said yes to this item (docs/PRIVACY.md).
    panoply_core::assets::authorize(&asset()).map_err(|e| engine_err(e.to_string()))?;
    let dir = lib_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", dir.display())))?;
    // Say so before touching the network (FLOWS.md: network access is labelled before it runs).
    eprintln!(
        "Downloading the PDF engine ({VERSION}, {:.1} MB) from github.com/bblanchon/pdfium-binaries. This happens once.",
        pin.size as f64 / 1e6
    );
    let tmp = dir.join(format!(".part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", tmp.display())))?;
    let result = (|| -> Result<()> {
        let archive = tmp.join("pdfium.tgz");
        let url = format!(
            "https://github.com/bblanchon/pdfium-binaries/releases/download/{RELEASE}/{}",
            pin.asset
        );
        let status = Command::new("curl")
            .args(["--fail", "--silent", "--show-error", "--location", "--proto", "=https", "--tlsv1.2"])
            .args(["--max-time", "600", "--output"])
            .arg(&archive)
            .arg(&url)
            .status()
            .map_err(|e| {
                engine_err(format!(
                    "Panoply needs its PDF engine for this and could not start curl to download it ({e}). \
                     Install curl, or set PANOPLY_PDFIUM_LIB to a pdfium library."
                ))
            })?;
        if !status.success() {
            return Err(engine_err(
                "Panoply needs its PDF engine for this and could not download it. Check your connection and try again, \
                 or set PANOPLY_PDFIUM_LIB to a pdfium library.",
            ));
        }
        let got = sha256_file(&archive)
            .map_err(|e| engine_err(format!("Could not read the download: {e}")))?;
        if got != pin.sha256 {
            return Err(engine_err(format!(
                "The downloaded PDF engine did not match its pinned checksum (expected {}, got {got}). It was not used.",
                pin.sha256
            )));
        }
        let extract = tmp.join("x");
        std::fs::create_dir_all(&extract)
            .map_err(|e| engine_err(format!("Could not create a folder: {e}")))?;
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .arg("-C")
            .arg(&extract)
            .arg(pin.member)
            .arg("LICENSE")
            .status()
            .map_err(|e| {
                engine_err(format!(
                    "Could not start tar to unpack the PDF engine ({e})."
                ))
            })?;
        if !status.success() {
            return Err(engine_err("Could not unpack the PDF engine."));
        }
        let unpacked = extract.join(pin.member);
        if !unpacked.is_file() {
            return Err(engine_err(
                "The PDF engine archive did not contain the library.",
            ));
        }
        let _ = std::fs::copy(extract.join("LICENSE"), dir.join("LICENSE"));
        // Rename is atomic on one volume, so another process never sees half a library.
        std::fs::rename(&unpacked, lib)
            .map_err(|e| engine_err(format!("Could not install the PDF engine: {e}")))?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// pdfium is not safe to call from two threads at once, whatever the wrapper does about its own
/// bookkeeping (a test binary that drew pages on two threads crashed within a few runs). Every
/// call into pdfium holds this lock, so one process draws one thing at a time. Jobs that need
/// parallel drawing run in separate processes.
pub fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

static ENGINE: OnceLock<Pdfium> = OnceLock::new();
static LOAD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The loaded pdfium engine. The first call finds (and if needed downloads) the library and
/// binds to it. Later calls are free. Only a success is kept: a refused or failed download is
/// asked about again on the next job, not remembered as a failure for the whole run.
pub fn engine() -> Result<&'static Pdfium> {
    if let Some(p) = ENGINE.get() {
        return Ok(p);
    }
    let _one_at_a_time = LOAD.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(p) = ENGINE.get() {
        return Ok(p);
    }
    #[cfg(target_arch = "wasm32")]
    {
        return Err(PdfError::Engine("The native pdfium engine is not available in the browser build.".into()));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let lib = ensure_library()?;
        let bindings = Pdfium::bind_to_library(&lib).map_err(|e| {
            PdfError::Engine(format!(
                "The PDF engine at {} could not be loaded: {e}",
                lib.display()
            ))
        })?;
        Ok(ENGINE.get_or_init(|| Pdfium::new(bindings)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_has_a_full_checksum_and_a_member() {
        for p in PINS {
            assert_eq!(p.sha256.len(), 64, "{}", p.asset);
            assert!(p.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(p.member.starts_with("lib/") || p.member.starts_with("bin/"));
        }
        // The platforms Phase 1 ships on all have a pin.
        for (os, arch) in [
            ("macos", "aarch64"),
            ("macos", "x86_64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("windows", "x86_64"),
            ("windows", "aarch64"),
        ] {
            assert!(
                PINS.iter().any(|p| p.os == os && p.arch == arch),
                "{os} {arch}"
            );
        }
    }

    #[test]
    fn the_cache_folder_is_outside_the_repository() {
        let d = engines_dir();
        assert!(!d.starts_with(env!("CARGO_MANIFEST_DIR")));
    }
}
