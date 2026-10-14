//! The model manager: three pinned models, fetched on first use.
//!
//! Every model is a [`Model`]: where it comes from, the SHA-256 of what is downloaded, its size
//! (shown before the download starts), its license, and the files it installs. [`ensure`] gives
//! the path of the model file, downloading it first when it is not in the per-user cache. A file
//! whose digest does not match its pin is never used and is removed.
//!
//! Nothing is downloaded unless the job's host allows the network ([`VisionIo::network_allowed`]),
//! and the first progress line of a download names the model and its size (FLOWS.md: size shown
//! before the first download, network use labelled before it runs).
//!
//! Sizes and sources, all read from the files themselves on 2026-10-14:
//!
//! | model | job | download | license |
//! |---|---|---|---|
//! | IS-Net general use, int8 | background removal fallback | 44.2 MB | MIT |
//! | MI-GAN 512 Places2 pipeline | erase and watermark fill | 28.1 MB | MIT |
//! | Real-ESRGAN general x4v3 | upscale | 4.5 MB | BSD-3-Clause |

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use panoply_image::error::{ImageError, Result};

use crate::VisionIo;
use crate::cache::models_dir;
use crate::fetch::{self, engine_err, host_of, mb, sha256_bytes, sha256_file};

/// How a download is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// The download is the model file itself.
    File,
    /// The download is a zip; each [`ModelFile`] names a member.
    Zip,
}

/// One file a model installs.
#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    /// File name in the model folder.
    pub name: &'static str,
    /// Member path inside the zip (empty for [`Packing::File`]).
    pub member: &'static str,
    /// SHA-256 of the installed file.
    pub sha256: &'static str,
    /// Size of the installed file in bytes.
    pub size: u64,
}

/// A pinned model.
#[derive(Debug)]
pub struct Model {
    /// Short id, also part of the cache folder name.
    pub id: &'static str,
    /// What the card and the progress line call it.
    pub name: &'static str,
    /// Download address (https only, pinned to a commit or a versioned release).
    pub url: &'static str,
    /// SHA-256 of the download.
    pub sha256: &'static str,
    /// Download size in bytes, shown before the download.
    pub size: u64,
    /// SPDX license id of the weights.
    pub license: &'static str,
    /// Where the weights come from, for the card and the docs.
    pub source: &'static str,
    /// How the download is packed.
    pub packing: Packing,
    /// The files it installs. The first one is the ONNX file that gets opened.
    pub files: &'static [ModelFile],
    /// Environment variable that points at a model file to use instead (no download, no check).
    pub env: &'static str,
}

/// IS-Net general use, weight-only int8 (QDQ) quantization of the imgly export: general objects,
/// 1024 px input, an `output` sigmoid map. The fp32 original has the same digest as imgly's
/// `onnx/model.onnx` (cc2c9f5c...), which is the official export of xuebinqin/DIS.
pub static ISNET: Model = Model {
    id: "isnet-general-int8",
    name: "the IS-Net background model",
    url: "https://huggingface.co/xrds/isnet-general-onnx-int8/resolve/71eff2372ec9c8edbc6ca637ded591423d23b65a/onnx/model_quantized.onnx",
    sha256: "3b21a6706dc8d6e4ba9f5b31ebc6940f6c785b58862e27bb25daa9dd4424b87f",
    size: 44_229_662,
    license: "MIT",
    source: "huggingface.co/xrds/isnet-general-onnx-int8 (int8 of imgly/isnet-general-onnx, DIS by Xuebin Qin)",
    packing: Packing::File,
    files: &[ModelFile {
        name: "isnet.onnx",
        member: "",
        sha256: "3b21a6706dc8d6e4ba9f5b31ebc6940f6c785b58862e27bb25daa9dd4424b87f",
        size: 44_229_662,
    }],
    env: "PANOPLY_MODEL_ISNET",
};

/// MI-GAN 512 Places2, the whole pipeline in one ONNX file (the one the MI-GAN README links):
/// uint8 `image` and `mask` in, uint8 `result` out, 255 in the mask means "keep".
pub static MIGAN: Model = Model {
    id: "migan-512-places2",
    name: "the MI-GAN fill model",
    url: "https://huggingface.co/andraniksargsyan/migan/resolve/406830d0fa60666da0071c342ad2fbc8f30c5c64/migan_pipeline_v2.onnx",
    sha256: "6f1f3530a1a2324b19752018ce756088b07973cda8d7d890034ace5c8a48c40b",
    size: 28_079_181,
    license: "MIT",
    source: "huggingface.co/andraniksargsyan/migan (the pre-converted pipeline linked from Picsart-AI-Research/MI-GAN)",
    packing: Packing::File,
    files: &[ModelFile {
        name: "migan.onnx",
        member: "",
        sha256: "6f1f3530a1a2324b19752018ce756088b07973cda8d7d890034ace5c8a48c40b",
        size: 28_079_181,
    }],
    env: "PANOPLY_MODEL_MIGAN",
};

/// Real-ESRGAN general x4v3 (SRVGGNetCompact, 1.21 M parameters), the float ONNX export Qualcomm
/// publishes: a fixed 128 x 128 tile in, a 512 x 512 tile out, values 0 to 1.
pub static ESRGAN: Model = Model {
    id: "realesr-general-x4v3",
    name: "the Real-ESRGAN upscale model",
    url: "https://qaihub-public-assets.s3.us-west-2.amazonaws.com/qai-hub-models/models/real_esrgan_general_x4v3/releases/v0.64.0/real_esrgan_general_x4v3-onnx-float.zip",
    sha256: "fc094329b24a00c936ebc1cc4c51bc59633e7dc36f4a553344342171cbd3c7cf",
    size: 4_540_895,
    license: "BSD-3-Clause",
    source: "huggingface.co/qualcomm/Real-ESRGAN-General-x4v3 (release v0.64.0 of xinntao/Real-ESRGAN weights)",
    packing: Packing::Zip,
    files: &[
        ModelFile {
            name: "real_esrgan_general_x4v3.onnx",
            member: "real_esrgan_general_x4v3-onnx-float/real_esrgan_general_x4v3.onnx",
            sha256: "09e6b675a0a18a97e1057c8dd7e984ef878a0a02ce104430da525b6241c858e7",
            size: 161_087,
        },
        ModelFile {
            name: "real_esrgan_general_x4v3.data",
            member: "real_esrgan_general_x4v3-onnx-float/real_esrgan_general_x4v3.data",
            sha256: "512d0ec9940c2e9d85d27f2952f12a0b77b7841dc22df4ce9f3ea458bc98f37f",
            size: 4_836_096,
        },
    ],
    env: "PANOPLY_MODEL_ESRGAN",
};

/// Every model, for listings.
pub fn all() -> [&'static Model; 3] {
    [&ISNET, &MIGAN, &ESRGAN]
}

/// The folder a model installs into: id and the first 8 digits of its pin, so a new pin never
/// reuses an old folder.
pub fn model_dir(model: &Model) -> PathBuf {
    models_dir().join(format!("{}-{}", model.id, &model.sha256[..8]))
}

/// Files already hashed in this process, so a batch does not hash the model for every job.
static VERIFIED: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

fn is_verified(id: &'static str) -> bool {
    VERIFIED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .is_some_and(|s| s.contains(id))
}

fn mark_verified(id: &'static str) {
    VERIFIED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert_with(HashSet::new)
        .insert(id);
}

/// True when every file of the model is in the cache with its pinned size.
fn present(model: &Model, dir: &Path) -> bool {
    model.files.iter().all(|f| {
        std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.is_file() && m.len() == f.size)
    })
}

/// Bytes still to download before this model can run: 0 when it is cached (or overridden by its
/// environment variable).
pub fn pending_download(model: &Model) -> u64 {
    if std::env::var_os(model.env).is_some() || present(model, &model_dir(model)) {
        0
    } else {
        model.size
    }
}

/// The path of the model's ONNX file when it is cached or overridden, without downloading.
pub fn cached_path(model: &Model) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(model.env) {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let dir = model_dir(model);
    present(model, &dir).then(|| dir.join(model.files[0].name))
}

/// The line the first progress report of a download shows.
pub fn download_label(model: &Model) -> String {
    format!("Downloading {} ({}, once)", model.name, mb(model.size))
}

/// Make sure the model is on disk and return the path of its ONNX file.
///
/// Lookup order: the environment variable override, the cache (verified once per process), then a
/// download when the host allows the network.
pub fn ensure(io: &dyn VisionIo, model: &'static Model) -> Result<PathBuf> {
    ensure_at(io, model, &model_dir(model))
}

/// Like [`ensure`], with the model folder given (tests use a folder of their own).
pub fn ensure_at(io: &dyn VisionIo, model: &'static Model, dir: &Path) -> Result<PathBuf> {
    if let Some(p) = std::env::var_os(model.env) {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(engine_err(format!(
                "{} points at {}, which is not a file.",
                model.env,
                p.display()
            )))
        };
    }
    if present(model, dir) {
        if is_verified(model.id) && dir == model_dir(model) {
            return Ok(dir.join(model.files[0].name));
        }
        let mut ok = true;
        for f in model.files {
            match sha256_file(&dir.join(f.name)) {
                Ok(h) if h == f.sha256 => {}
                _ => ok = false,
            }
        }
        if ok {
            if dir == model_dir(model) {
                mark_verified(model.id);
            }
            return Ok(dir.join(model.files[0].name));
        }
        // A cached file that does not match its pin is never used.
        let _ = std::fs::remove_dir_all(dir);
    }
    if !io.network_allowed() {
        return Err(engine_err(format!(
            "Panoply needs to download {} ({}, once) for this and downloads are not allowed for this job. Allow the download and try again.",
            model.name,
            mb(model.size)
        )));
    }
    install(io, model, dir)?;
    if dir == model_dir(model) {
        mark_verified(model.id);
    }
    io.note(format!(
        "Downloaded {} ({}) from {}. This happens once; it stays on this computer.",
        model.name,
        mb(model.size),
        host_of(model.url)
    ));
    Ok(dir.join(model.files[0].name))
}

fn install(io: &dyn VisionIo, model: &'static Model, dir: &Path) -> Result<()> {
    let parent = dir
        .parent()
        .ok_or_else(|| engine_err("The model folder has no parent folder."))?;
    std::fs::create_dir_all(parent).map_err(|e| {
        ImageError::Io(format!(
            "Could not create {} ({e}). Check the folder and the free space, then try again.",
            parent.display()
        ))
    })?;
    let tmp = parent.join(format!(".part-{}-{}", model.id, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .map_err(|e| ImageError::Io(format!("Could not create {} ({e}).", tmp.display())))?;
    let result = (|| -> Result<()> {
        let label = download_label(model);
        // Say what and how big before touching the network.
        io.progress(0.0, &label);
        let archive = tmp.join("download");
        fetch::download(model.url, &archive, model.size, io, &label)?;
        place(&archive, model, dir, &tmp)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// Checks a downloaded file against its pin and installs it into `dir` (a rename, so another
/// process never sees half a model). `tmp` is a scratch folder next to it.
pub fn place(archive: &Path, model: &Model, dir: &Path, tmp: &Path) -> Result<()> {
    let got = sha256_file(archive)
        .map_err(|e| engine_err(format!("Could not read the download ({e}).")))?;
    if got != model.sha256 {
        return Err(engine_err(format!(
            "The downloaded model did not match its pinned checksum (expected {}, got {got}). It was not used.",
            model.sha256
        )));
    }
    let out = tmp.join("out");
    std::fs::create_dir_all(&out)
        .map_err(|e| ImageError::Io(format!("Could not create a folder ({e}).")))?;
    match model.packing {
        Packing::File => {
            std::fs::copy(archive, out.join(model.files[0].name))
                .map_err(|e| ImageError::Io(format!("Could not install the model ({e}).")))?;
        }
        Packing::Zip => {
            for f in model.files {
                let bytes = fetch::unzip_member(archive, f.member)?;
                if sha256_bytes(&bytes) != f.sha256 {
                    return Err(engine_err(
                        "A file inside the downloaded model did not match its pinned checksum. It was not used.",
                    ));
                }
                std::fs::write(out.join(f.name), bytes)
                    .map_err(|e| ImageError::Io(format!("Could not install the model ({e}).")))?;
            }
        }
    }
    if dir.exists() {
        let _ = std::fs::remove_dir_all(dir);
    }
    std::fs::rename(&out, dir)
        .map_err(|e| ImageError::Io(format!("Could not install the model ({e}).")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_SHA: &str = "2b2eb6f423932d62cafb015658d50a64d33e96e66fc9ec182172df817971e6f4";

    static TEST_MODEL: Model = Model {
        id: "test-model",
        name: "the test model",
        url: "https://example.invalid/model.onnx",
        sha256: HELLO_SHA,
        size: 11,
        license: "MIT",
        source: "a test",
        packing: Packing::File,
        files: &[ModelFile {
            name: "m.onnx",
            member: "",
            sha256: HELLO_SHA,
            size: 11,
        }],
        env: "PANOPLY_MODEL_TEST_NEVER_SET",
    };

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "panoply-vision-models-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_download_that_matches_its_pin_is_installed_and_one_that_does_not_is_refused() {
        let d = scratch("place");
        let good = d.join("good");
        std::fs::write(&good, b"hello model").unwrap();
        let dir = d.join("installed");
        place(&good, &TEST_MODEL, &dir, &d.join("tmp-good")).unwrap();
        assert_eq!(std::fs::read(dir.join("m.onnx")).unwrap(), b"hello model");

        let bad = d.join("bad");
        std::fs::write(&bad, b"hello modeX").unwrap();
        let dir2 = d.join("installed-bad");
        let err = place(&bad, &TEST_MODEL, &dir2, &d.join("tmp-bad"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("pinned checksum"), "{err}");
        assert!(
            !dir2.exists(),
            "a file that fails its pin must not be installed"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_cached_model_is_verified_and_a_tampered_one_is_dropped() {
        let io = crate::NoNetwork(panoply_image::ops::FsIo::new());
        let d = scratch("tamper");
        let dir = d.join("m");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.onnx"), b"hello model").unwrap();
        let p = ensure_at(&io, &TEST_MODEL, &dir).unwrap();
        assert_eq!(p, dir.join("m.onnx"));
        // Same size, different bytes: not used, removed, and with no network it says so plainly.
        std::fs::write(dir.join("m.onnx"), b"hello modeX").unwrap();
        let err = ensure_at(&io, &TEST_MODEL, &dir).unwrap_err().to_string();
        assert!(err.contains("downloads are not allowed"), "{err}");
        assert!(err.contains("the test model"), "{err}");
        assert!(!dir.exists(), "the tampered file was kept");
        // A missing model also names its size before any download.
        let err = ensure_at(&io, &TEST_MODEL, &d.join("missing"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("11 bytes") || err.contains("0.0 MB"), "{err}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn pending_download_is_the_pinned_size_until_the_model_is_cached() {
        assert_eq!(pending_download(&TEST_MODEL), TEST_MODEL.size);
    }

    #[test]
    fn pins_are_complete() {
        for m in all() {
            assert!(m.url.starts_with("https://"), "{}", m.id);
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(m.sha256.bytes().all(|b| b.is_ascii_hexdigit()), "{}", m.id);
            assert!(m.size > 0);
            assert!(!m.files.is_empty());
            for f in m.files {
                assert_eq!(f.sha256.len(), 64);
                assert!(f.size > 0);
            }
            assert!(matches!(m.license, "MIT" | "Apache-2.0" | "BSD-3-Clause"));
            if m.packing == Packing::File {
                assert_eq!(m.files.len(), 1);
                assert_eq!(m.files[0].sha256, m.sha256);
                assert_eq!(m.files[0].size, m.size);
            }
        }
    }

    #[test]
    fn cache_is_outside_the_repository() {
        assert!(!model_dir(&ISNET).starts_with(env!("CARGO_MANIFEST_DIR")));
    }

    #[test]
    fn sizes_are_shown_in_the_label() {
        assert_eq!(
            download_label(&ISNET),
            "Downloading the IS-Net background model (44 MB, once)"
        );
        assert!(download_label(&ESRGAN).contains("4.5 MB"));
    }
}
