//! Finding, fetching and checking the files the fallback engines need: the PP-OCRv5 models,
//! the ONNX Runtime library, and Tesseract language data.
//!
//! Nothing here is in the repository or in the binary. The first job that needs a file
//! downloads it from the pinned address, checks its SHA-256 against the table below, and keeps
//! it in the engines folder (`PANOPLY_ENGINES_DIR`, else the user's cache folder, the same one
//! the PDF engine uses). A file whose checksum does not match is never used and never kept.
//! The download uses the system `curl`, and `tar` to unpack the runtime, which ship with
//! Windows 10 and later, macOS and mainstream Linux, so there is no HTTP client in the binary.
//!
//! The size of each download is told to the caller's `notice` callback before the network is
//! touched (FLOWS.md: network access is labelled before it runs).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::error::{OcrError, Result};

/// One file to fetch.
#[derive(Debug, Clone, Copy)]
pub struct Asset {
    /// A short id, also the cache file name stem.
    pub id: &'static str,
    /// What the user is told it is.
    pub label: &'static str,
    /// The exact address. Hugging Face and GitHub addresses carry a commit, never `main`.
    pub url: &'static str,
    /// SHA-256 of the file as served.
    pub sha256: &'static str,
    /// Size in bytes, for the notice shown before the download.
    pub size: u64,
    /// SPDX license of what the file holds.
    pub license: &'static str,
    /// File name in the cache folder.
    pub file: &'static str,
}

/// PP-OCRv5 mobile text detection, shared by every script. Apache-2.0.
pub const DET: Asset = Asset {
    id: "ppocrv5-mobile-det",
    label: "text detection model (PP-OCRv5 mobile, 4.8 MB)",
    url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx/resolve/e6f4fa85f00e168c862bc462aebca69eef9b3d3d/inference.onnx",
    sha256: "a431985659dc921974177a95adcfbb90fd9e51989a5e04d70d0b75f597b6e61d",
    size: 4_826_518,
    license: "Apache-2.0",
    file: "ppocrv5-mobile-det.onnx",
};

/// PP-OCRv5 mobile recognition, English.
pub const REC_EN: Asset = Asset {
    id: "ppocrv5-mobile-rec-en",
    label: "English text recognition model (PP-OCRv5 mobile, 7.8 MB)",
    url: "https://huggingface.co/PaddlePaddle/en_PP-OCRv5_mobile_rec_onnx/resolve/3fafbc3b5dcf93dd72add9f48368be8a3a2cd33b/inference.onnx",
    sha256: "b5f833dfc5d0eb71da397b4efa06ebeee9b431b690a47d6af40d77d8eabc557f",
    size: 7_848_423,
    license: "Apache-2.0",
    file: "ppocrv5-mobile-rec-en.onnx",
};

/// PP-OCRv5 mobile recognition, Latin script languages.
pub const REC_LATIN: Asset = Asset {
    id: "ppocrv5-mobile-rec-latin",
    label: "Latin script text recognition model (PP-OCRv5 mobile, 8.0 MB)",
    url: "https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f/inference.onnx",
    sha256: "7888113072263cb471b93f66dd5e2ad70548dc526fa1ace760d0d973dd121498",
    size: 8_042_023,
    license: "Apache-2.0",
    file: "ppocrv5-mobile-rec-latin.onnx",
};

/// PP-OCRv5 mobile recognition, Devanagari (Hindi, Marathi, Nepali) with Latin letters.
pub const REC_DEVANAGARI: Asset = Asset {
    id: "ppocrv5-mobile-rec-devanagari",
    label: "Devanagari text recognition model (PP-OCRv5 mobile, 7.9 MB)",
    url: "https://huggingface.co/PaddlePaddle/devanagari_PP-OCRv5_mobile_rec_onnx/resolve/251aec19e36739540d35e2cc943f6aa7503b98e5/inference.onnx",
    sha256: "cb789212ce96c69d3e74728ae4309d179281d68cb3945d0616b67cafab41c986",
    size: 7_912_311,
    license: "Apache-2.0",
    file: "ppocrv5-mobile-rec-devanagari.onnx",
};

/// Tesseract language data (tessdata_fast, Apache-2.0), pinned to one commit of the
/// repository. Only the languages the OS engines and PP-OCR do not read are listed.
pub const TESSDATA_BEN: Asset = Asset {
    id: "tessdata-fast-ben",
    label: "Bengali language data for Tesseract (0.9 MB)",
    url: "https://github.com/tesseract-ocr/tessdata_fast/raw/87416418657359cb625c412a48b6e1d6d41c29bd/ben.traineddata",
    sha256: "31163084c279aaebd376216f0c3d5c17ad4b5fee8db49dae79c20000b5de5964",
    size: 855_841,
    license: "Apache-2.0",
    file: "ben.traineddata",
};

/// English data, used with Bengali on one line (`ben+eng`) and for measuring Tesseract.
pub const TESSDATA_ENG: Asset = Asset {
    id: "tessdata-fast-eng",
    label: "English language data for Tesseract (4.1 MB)",
    url: "https://github.com/tesseract-ocr/tessdata_fast/raw/87416418657359cb625c412a48b6e1d6d41c29bd/eng.traineddata",
    sha256: "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2",
    size: 4_113_088,
    license: "Apache-2.0",
    file: "eng.traineddata",
};

/// Hindi data, for measuring Tesseract against the Devanagari recognizer.
pub const TESSDATA_HIN: Asset = Asset {
    id: "tessdata-fast-hin",
    label: "Hindi language data for Tesseract (1.1 MB)",
    url: "https://github.com/tesseract-ocr/tessdata_fast/raw/87416418657359cb625c412a48b6e1d6d41c29bd/hin.traineddata",
    sha256: "4c73ffc59d497c186b19d1e90f5d721d678ea6b2e277b719bee4e2af12271825",
    size: 1_122_751,
    license: "Apache-2.0",
    file: "hin.traineddata",
};

/// The ONNX Runtime release used. `ort` 2.0.0-rc.13 is built for the 1.28 line.
pub const ORT_VERSION: &str = "1.28.3";

struct OrtPin {
    os: &'static str,
    arch: &'static str,
    asset: &'static str,
    sha256: &'static str,
    /// The library inside the archive.
    member: &'static str,
    size: u64,
}

/// SHA-256 of each archive in release v1.28.3 of microsoft/onnxruntime, from the release's own
/// published digests (the linux and macOS archives were also checked against a download).
/// There is no macOS x86_64 archive in this release: Intel Macs read Latin script through Apple
/// Vision, and can point `PANOPLY_ORT_LIB` at a runtime for the other scripts.
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
    // The Windows archives are zip files that also hold 400 MB of debug symbols, so they are
    // large downloads (about 79 MB) for a 16 MB library. Windows reads Latin script and most
    // other languages through Windows.Media.Ocr, so this is only needed for the rest.
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

fn ort_pin() -> Option<&'static OrtPin> {
    ORT_PINS
        .iter()
        .find(|p| p.os == std::env::consts::OS && p.arch == std::env::consts::ARCH)
}

/// Where the OCR files live, inside the engines folder.
pub fn cache_dir() -> PathBuf {
    panoply_pdf::pdfium::engines_dir().join("ocr")
}

fn engine_err(msg: impl Into<String>) -> OcrError {
    OcrError::Engine(msg.into())
}

/// SHA-256 of a file, as lower case hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
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

/// Whether downloading is allowed, and where messages for the user go.
pub struct Fetch<'a> {
    /// `false` when the module was not granted the network: a missing file is then an error
    /// that says what to do.
    pub allow_download: bool,
    /// Receives one plain line before each download starts.
    pub notice: &'a mut dyn FnMut(&str),
}

impl<'a> Fetch<'a> {
    /// Downloads allowed, messages to `notice`.
    pub fn new(notice: &'a mut dyn FnMut(&str)) -> Self {
        Fetch {
            allow_download: true,
            notice,
        }
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

fn curl(url: &str, out: &Path) -> Result<()> {
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
            "900",
            "--output",
        ])
        .arg(out)
        .arg(url)
        .status()
        .map_err(|e| {
            engine_err(format!(
                "Panoply needs to download an OCR file and could not start curl ({e}). Install curl and try again."
            ))
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(engine_err(
            "Panoply could not download an OCR file. Check your connection and try again.",
        ))
    }
}

fn download_to(url: &str, expected: &str, final_path: &Path) -> Result<()> {
    let dir = final_path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", dir.display())))?;
    let tmp = dir.join(format!(
        ".part-{}-{}",
        std::process::id(),
        final_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_file(&tmp);
    let r = (|| -> Result<()> {
        curl(url, &tmp)?;
        let got = sha256_file(&tmp)
            .map_err(|e| engine_err(format!("Could not read the download: {e}")))?;
        if got != expected {
            return Err(engine_err(format!(
                "A downloaded OCR file did not match its pinned checksum (expected {expected}, got {got}). It was not used."
            )));
        }
        // Rename is atomic on one volume, so another process never sees half a file.
        std::fs::rename(&tmp, final_path)
            .map_err(|e| engine_err(format!("Could not install the OCR file: {e}")))
    })();
    let _ = std::fs::remove_file(&tmp);
    r
}

/// Make sure an asset is on disk and its checksum matches, downloading it once if needed.
/// Returns its path.
pub fn ensure(asset: &Asset, fetch: &mut Fetch<'_>) -> Result<PathBuf> {
    let path = cache_dir().join(asset.file);
    if path.is_file() {
        // The size is a cheap first check. The full checksum was verified when the file was
        // installed, and is verified again here because a model is loaded once per run.
        let ok = std::fs::metadata(&path)
            .map(|m| m.len() == asset.size)
            .unwrap_or(false)
            && sha256_file(&path)
                .map(|h| h == asset.sha256)
                .unwrap_or(false);
        if ok {
            return Ok(path);
        }
        let _ = std::fs::remove_file(&path);
    }
    if !fetch.allow_download {
        return Err(engine_err(format!(
            "Panoply needs the {} for this and may not use the network right now. Allow the download and try again.",
            asset.label
        )));
    }
    (fetch.notice)(&format!(
        "Downloading the {} from {} ({}). This happens once.",
        asset.label,
        host_of(asset.url),
        asset.license
    ));
    download_to(asset.url, asset.sha256, &path)?;
    Ok(path)
}

fn host_of(url: &str) -> &str {
    url.trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(url)
}

/// The path of the ONNX Runtime library, downloading it once if needed.
///
/// Lookup order: `PANOPLY_ORT_LIB` (a path to the library, no download), the engines folder,
/// then a pinned download.
pub fn ensure_ort(fetch: &mut Fetch<'_>) -> Result<PathBuf> {
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
            "There is no prebuilt ONNX Runtime for {} on {} in the version Panoply uses. Set PANOPLY_ORT_LIB to an ONNX Runtime {ORT_VERSION} library.",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    let dir = cache_dir().join(format!("onnxruntime-{ORT_VERSION}"));
    let lib = dir.join(Path::new(pin.member).file_name().unwrap_or_default());
    if lib.is_file() {
        return Ok(lib);
    }
    if !fetch.allow_download {
        return Err(engine_err(
            "Panoply needs the ONNX Runtime for this and may not use the network right now. Allow the download and try again.",
        ));
    }
    (fetch.notice)(&format!(
        "Downloading the ONNX Runtime {ORT_VERSION} ({}) from github.com/microsoft/onnxruntime (MIT). This happens once.",
        mb(pin.size)
    ));
    std::fs::create_dir_all(&dir)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", dir.display())))?;
    let tmp = dir.join(format!(".part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", tmp.display())))?;
    let result = (|| -> Result<()> {
        let archive = tmp.join(pin.asset);
        let url = format!(
            "https://github.com/microsoft/onnxruntime/releases/download/v{ORT_VERSION}/{}",
            pin.asset
        );
        curl(&url, &archive)?;
        let got = sha256_file(&archive)
            .map_err(|e| engine_err(format!("Could not read the download: {e}")))?;
        if got != pin.sha256 {
            return Err(engine_err(format!(
                "The downloaded ONNX Runtime did not match its pinned checksum (expected {}, got {got}). It was not used.",
                pin.sha256
            )));
        }
        let extract = tmp.join("x");
        std::fs::create_dir_all(&extract)
            .map_err(|e| engine_err(format!("Could not create a folder: {e}")))?;
        let root = pin.member.split('/').next().unwrap_or("");
        let license_member = format!("{root}/LICENSE");
        let status = Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&extract)
            .arg(pin.member)
            .arg(&license_member)
            .status()
            .map_err(|e| {
                engine_err(format!(
                    "Could not start tar to unpack the ONNX Runtime ({e})."
                ))
            })?;
        if !status.success() {
            return Err(engine_err("Could not unpack the ONNX Runtime."));
        }
        let unpacked = extract.join(pin.member);
        if !unpacked.is_file() {
            return Err(engine_err(
                "The ONNX Runtime archive did not contain the library.",
            ));
        }
        let _ = std::fs::copy(extract.join(&license_member), dir.join("LICENSE"));
        std::fs::rename(&unpacked, &lib)
            .map_err(|e| engine_err(format!("Could not install the ONNX Runtime: {e}")))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    Ok(lib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_has_a_full_checksum_and_an_https_address() {
        for a in [
            DET,
            REC_EN,
            REC_LATIN,
            REC_DEVANAGARI,
            TESSDATA_BEN,
            TESSDATA_ENG,
            TESSDATA_HIN,
        ] {
            assert_eq!(a.sha256.len(), 64, "{}", a.id);
            assert!(a.sha256.bytes().all(|b| b.is_ascii_hexdigit()), "{}", a.id);
            assert!(a.url.starts_with("https://"), "{}", a.id);
            // Pinned to a commit, never a moving branch.
            assert!(
                !a.url.contains("/main/") && !a.url.contains("/master/"),
                "{}",
                a.id
            );
            assert_eq!(a.license, "Apache-2.0");
        }
        for p in ORT_PINS {
            assert_eq!(p.sha256.len(), 64, "{}", p.asset);
            assert!(p.member.contains(ORT_VERSION));
        }
    }

    #[test]
    fn the_cache_is_outside_the_repository() {
        assert!(!cache_dir().starts_with(env!("CARGO_MANIFEST_DIR")));
    }

    #[test]
    fn a_missing_file_without_network_says_what_to_do() {
        let mut said = Vec::new();
        let mut notice = |s: &str| said.push(s.to_string());
        let mut f = Fetch::new(&mut notice);
        f.allow_download = false;
        let missing = Asset {
            file: "does-not-exist.onnx",
            ..DET
        };
        let e = ensure(&missing, &mut f).unwrap_err().to_string();
        assert!(e.contains("may not use the network"), "{e}");
        assert!(said.is_empty());
    }
}
