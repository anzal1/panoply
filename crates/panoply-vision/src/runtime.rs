//! ONNX Runtime: found or fetched once, opened at run time, one session per model, dropped after
//! 60 seconds without use.
//!
//! The `ort` crate is built with `load-dynamic`, so nothing links against the runtime. The
//! library comes from a pinned official release (microsoft/onnxruntime, MIT) in the engines
//! folder, shared with the other crates that use it. Lookup order: `PANOPLY_ORT_LIB` (a path, no
//! download), the engines folder, then a download that says what it is and how big before it
//! starts.
//!
//! Execution providers follow the engine ladder of ADR 0004 for the hardware: CoreML on macOS for
//! the models that gain from it, DirectML on Windows where the runtime build has it, the CPU
//! everywhere else and whenever a provider cannot take a model. `PANOPLY_VISION_EP=cpu` forces
//! the CPU.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ort::session::builder::GraphOptimizationLevel;
use ort::session::{Session, SessionInputValue};
use ort::value::Tensor;
use panoply_image::error::{ImageError, Result};

use crate::VisionIo;
use crate::cache::{engines_dir, scratch_dir};
use crate::fetch::{self, engine_err, host_of, mb, sha256_file};
use crate::models::{self, Model};

/// The ONNX Runtime release used. `ort` 2.0.0-rc.13 is built for the 1.28 line.
pub const ORT_VERSION: &str = "1.28.3";

/// How long a loaded model stays when nobody uses it (FLOWS.md budgets).
pub const IDLE_UNLOAD: Duration = Duration::from_secs(60);

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
/// published digests. There is no macOS x86_64 archive in this release: Intel Macs use Apple
/// Vision for background removal, and can point `PANOPLY_ORT_LIB` at a runtime for the rest.
/// The Windows archives are the CPU build; a DirectML build (the NuGet package) can be named with
/// `PANOPLY_ORT_LIB` and is then used by the DirectML provider.
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

fn ort_pin() -> Option<&'static OrtPin> {
    ORT_PINS
        .iter()
        .find(|p| p.os == std::env::consts::OS && p.arch == std::env::consts::ARCH)
}

fn lib_name(pin: &OrtPin) -> &'static str {
    Path::new(pin.member)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("onnxruntime")
}

/// Folders an already fetched runtime may be in: this crate's, and the OCR crate's.
fn runtime_dirs() -> [PathBuf; 2] {
    [
        engines_dir().join(format!("onnxruntime-{ORT_VERSION}")),
        engines_dir()
            .join("ocr")
            .join(format!("onnxruntime-{ORT_VERSION}")),
    ]
}

/// The runtime library when it is already on disk (or named by `PANOPLY_ORT_LIB`), without
/// downloading anything.
pub fn runtime_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PANOPLY_ORT_LIB") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let pin = ort_pin()?;
    runtime_dirs()
        .into_iter()
        .map(|d| d.join(lib_name(pin)))
        .find(|p| p.is_file())
}

fn asset(pin: &OrtPin) -> panoply_core::assets::AssetInfo {
    panoply_core::assets::onnxruntime(ORT_VERSION, pin.size)
}

/// The ONNX Runtime as a listing entry: both folders it may have been cached in.
pub fn entry() -> Option<panoply_core::assets::Entry> {
    ort_pin().map(|pin| panoply_core::assets::Entry {
        info: asset(pin),
        paths: runtime_dirs().to_vec(),
    })
}

/// The download size of the pinned runtime for this platform, when there is one.
pub fn pinned_size() -> Option<u64> {
    ort_pin().map(|p| p.size)
}

/// Bytes still to download before any model can run: the runtime, when it is not on disk.
pub fn pending_runtime_download() -> u64 {
    if runtime_path().is_some() {
        return 0;
    }
    ort_pin().map_or(0, |p| p.size)
}

/// The path of the runtime library, downloading it once if needed.
pub fn ensure_runtime(io: &dyn VisionIo) -> Result<PathBuf> {
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
    if let Some(p) = runtime_path() {
        return Ok(p);
    }
    let _turn = fetch::install_lock();
    // Another thread may have installed it while this one waited.
    if let Some(p) = runtime_path() {
        return Ok(p);
    }
    if !io.network_allowed() {
        return Err(engine_err(format!(
            "Panoply needs the ONNX Runtime ({}, once) for this and downloads are not allowed for this job. Allow the download and try again.",
            mb(pin.size)
        )));
    }
    // Nothing is fetched until the person has said yes to this item (docs/PRIVACY.md).
    panoply_core::assets::authorize(&asset(pin)).map_err(|e| engine_err(e.to_string()))?;
    let dir = engines_dir().join(format!("onnxruntime-{ORT_VERSION}"));
    let lib = dir.join(lib_name(pin));
    let label = format!(
        "Downloading the ONNX Runtime {ORT_VERSION} ({}, once)",
        mb(pin.size)
    );
    io.progress(0.0, &label);
    std::fs::create_dir_all(&dir).map_err(|e| {
        ImageError::Io(format!(
            "Could not create {} ({e}). Check the folder and the free space, then try again.",
            dir.display()
        ))
    })?;
    let tmp = dir.join(format!(".part-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .map_err(|e| ImageError::Io(format!("Could not create {} ({e}).", tmp.display())))?;
    let result = (|| -> Result<()> {
        let archive = tmp.join(pin.asset);
        let url = format!(
            "https://github.com/microsoft/onnxruntime/releases/download/v{ORT_VERSION}/{}",
            pin.asset
        );
        fetch::download(&url, &archive, pin.size, io, &label)?;
        let got = sha256_file(&archive)
            .map_err(|e| engine_err(format!("Could not read the download ({e}).")))?;
        if got != pin.sha256 {
            return Err(engine_err(format!(
                "The downloaded ONNX Runtime did not match its pinned checksum (expected {}, got {got}). It was not used.",
                pin.sha256
            )));
        }
        let extract = tmp.join("x");
        std::fs::create_dir_all(&extract)
            .map_err(|e| ImageError::Io(format!("Could not create a folder ({e}).")))?;
        let root = pin.member.split('/').next().unwrap_or("");
        let license_member = format!("{root}/LICENSE");
        let status = Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&extract)
            .arg(pin.member)
            .arg(&license_member)
            .stdin(Stdio::null())
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
            .map_err(|e| ImageError::Io(format!("Could not install the ONNX Runtime ({e}).")))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    io.note(format!(
        "Downloaded the ONNX Runtime {ORT_VERSION} ({}) from {} (MIT). This happens once.",
        mb(pin.size),
        host_of("https://github.com/microsoft/onnxruntime")
    ));
    Ok(lib)
}

static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

fn init_runtime(io: &dyn VisionIo) -> Result<()> {
    if let Some(r) = INIT.get() {
        return r.clone().map_err(ImageError::Unsupported);
    }
    // Finding or fetching the library is outside the cell: a download the person declined is
    // asked about again on the next job, instead of being remembered as a failure.
    let lib = ensure_runtime(io)?;
    let r = INIT.get_or_init(|| {
        let builder = ort::init_from(&lib).map_err(|e| {
            format!(
                "The ONNX Runtime at {} could not be loaded: {e}",
                lib.display()
            )
        })?;
        builder.with_name("panoply").commit();
        Ok(())
    });
    r.clone().map_err(ImageError::Unsupported)
}

/// Which hardware a session may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Prefer {
    /// The CPU only (models whose shapes or operators no accelerator takes).
    Cpu,
    /// The platform's accelerator when there is one (CoreML on macOS, DirectML on Windows), with
    /// the CPU taking whatever it cannot.
    Accelerated,
}

/// A loaded model.
pub struct Loaded {
    session: Mutex<Session>,
    /// What runs the graph, for the result card ("CoreML", "DirectML", "CPU").
    pub provider: &'static str,
    last_used: Mutex<Instant>,
}

/// One input tensor.
pub enum In<'a> {
    /// 32 bit float data.
    F32(&'a str, Vec<i64>, Vec<f32>),
    /// 8 bit unsigned data.
    U8(&'a str, Vec<i64>, Vec<u8>),
}

/// One output tensor.
pub enum Out {
    /// 32 bit float data, with its shape.
    F32(Vec<i64>, Vec<f32>),
    /// 8 bit unsigned data, with its shape.
    U8(Vec<i64>, Vec<u8>),
}

fn ort_err(what: &str, e: impl std::fmt::Display) -> ImageError {
    ImageError::Failed(format!(
        "The model run failed ({what}: {e}). Try again; if it keeps failing, set PANOPLY_VISION_EP=cpu."
    ))
}

impl Loaded {
    /// Run the model. `want` names the outputs to compute and return, in order; naming only the
    /// ones needed lets the runtime skip and free the rest.
    pub fn run(&self, inputs: Vec<In<'_>>, want: &[&str]) -> Result<Vec<Out>> {
        let mut feed: Vec<(Cow<'static, str>, SessionInputValue<'static>)> = Vec::new();
        for i in inputs {
            match i {
                In::F32(name, shape, data) => {
                    let t = Tensor::<f32>::from_array((shape, data))
                        .map_err(|e| ort_err("input", e))?;
                    feed.push((Cow::Owned(name.to_string()), SessionInputValue::from(t)));
                }
                In::U8(name, shape, data) => {
                    let t =
                        Tensor::<u8>::from_array((shape, data)).map_err(|e| ort_err("input", e))?;
                    feed.push((Cow::Owned(name.to_string()), SessionInputValue::from(t)));
                }
            }
        }
        let mut selector = ort::session::OutputSelector::no_default();
        for w in want {
            selector = selector.with(*w);
        }
        let options = ort::session::RunOptions::new()
            .map_err(|e| ort_err("options", e))?
            .with_outputs(selector);
        let mut session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        let outputs = session
            .run_with_options(feed, &options)
            .map_err(|e| ort_err("run", e))?;
        let mut result = Vec::with_capacity(want.len());
        for w in want {
            let v = &outputs[*w];
            if let Ok((shape, data)) = v.try_extract_tensor::<f32>() {
                result.push(Out::F32(shape.to_vec(), data.to_vec()));
            } else if let Ok((shape, data)) = v.try_extract_tensor::<u8>() {
                result.push(Out::U8(shape.to_vec(), data.to_vec()));
            } else {
                return Err(ImageError::Failed(format!(
                    "The model returned {w} in a type Panoply does not read."
                )));
            }
        }
        drop(outputs);
        drop(session);
        *self.last_used.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
        Ok(result)
    }
}

type Key = (&'static str, Prefer);

static SESSIONS: Mutex<Option<HashMap<Key, Arc<Loaded>>>> = Mutex::new(None);
static REAPER: Mutex<bool> = Mutex::new(false);

/// How long an idle model is kept: `PANOPLY_MODEL_IDLE_SECS` (0 drops it when the job ends),
/// else 60 seconds.
pub fn idle_ttl() -> Duration {
    std::env::var("PANOPLY_MODEL_IDLE_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(IDLE_UNLOAD, Duration::from_secs)
}

fn threads() -> usize {
    if let Some(n) = std::env::var("PANOPLY_VISION_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        return n.clamp(1, 64);
    }
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 8)
}

/// Whether the CPU arena allocator is on. Off by default: peak memory matters more than the
/// small speed gain. `PANOPLY_VISION_ARENA=1` turns it on.
fn arena() -> bool {
    std::env::var("PANOPLY_VISION_ARENA").is_ok_and(|v| v == "1")
}

fn want_cpu() -> bool {
    std::env::var("PANOPLY_VISION_EP").is_ok_and(|v| v.eq_ignore_ascii_case("cpu"))
}

fn providers(prefer: Prefer) -> (Vec<ort::ep::ExecutionProviderDispatch>, &'static str) {
    #[allow(unused_mut)]
    let mut list: Vec<ort::ep::ExecutionProviderDispatch> = Vec::new();
    #[allow(unused_mut)]
    let mut name = "CPU";
    if prefer == Prefer::Accelerated && !want_cpu() {
        #[cfg(target_os = "macos")]
        {
            let _ = std::fs::create_dir_all(scratch_dir());
            list.push(
                ort::ep::CoreML::default()
                    .with_model_format(ort::ep::coreml::ModelFormat::MLProgram)
                    .with_static_input_shapes(true)
                    .with_model_cache_dir(scratch_dir().to_string_lossy())
                    .build(),
            );
            name = "CoreML";
        }
        #[cfg(windows)]
        {
            list.push(ort::ep::DirectML::default().build());
            name = "DirectML";
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = scratch_dir;
    (list, name)
}

fn build_session(path: &Path, prefer: Prefer) -> Result<(Session, &'static str)> {
    let (eps, name) = providers(prefer);
    let make =
        |eps: &[ort::ep::ExecutionProviderDispatch]| -> ort::Result<Session> {
            let mut b = Session::builder()?
                .with_optimization_level(match std::env::var("PANOPLY_VISION_OPT").as_deref() {
                    Ok("0") => GraphOptimizationLevel::Disable,
                    Ok("1") => GraphOptimizationLevel::Level1,
                    Ok("2") => GraphOptimizationLevel::Level2,
                    _ => GraphOptimizationLevel::All,
                })?
                .with_intra_threads(threads())?
                .with_intra_op_spinning(false)?;
            if !eps.is_empty() {
                b = b.with_execution_providers(eps)?;
            } else if !arena() {
                // Models run on the CPU with no arena: the runtime hands memory back after each
                // run instead of keeping the largest run's working set for the life of the session.
                b = b.with_memory_pattern(false)?.with_execution_providers([
                    ort::ep::CPU::default().with_arena_allocator(false).build(),
                ])?;
            }
            #[cfg(windows)]
            if !eps.is_empty() {
                // DirectML requires sequential execution and no memory pattern.
                b = b
                    .with_parallel_execution(false)?
                    .with_memory_pattern(false)?;
            }
            b.commit_from_file(path)
        };
    match make(&eps) {
        Ok(s) => Ok((s, name)),
        Err(e) if !eps.is_empty() => {
            // The accelerator could not take the model: the CPU always can.
            make(&[]).map(|s| (s, "CPU")).map_err(|e2| {
                ImageError::Failed(format!(
                    "The model could not be opened ({name}: {e}; CPU: {e2})."
                ))
            })
        }
        Err(e) => Err(ImageError::Failed(format!(
            "The model could not be opened ({e})."
        ))),
    }
}

/// The loaded model for this job: opens the runtime, makes sure the model is on disk (both may
/// download once), and loads it unless an earlier job left it loaded.
pub fn acquire(io: &dyn VisionIo, model: &'static Model, prefer: Prefer) -> Result<Arc<Loaded>> {
    init_runtime(io)?;
    let key = (model.id, prefer);
    if let Some(l) = SESSIONS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .and_then(|m| m.get(&key))
    {
        return Ok(l.clone());
    }
    let path = models::ensure(io, model)?;
    io.progress(0.0, &format!("Loading {}", model.name));
    let (session, provider) = build_session(&path, prefer)?;
    let loaded = Arc::new(Loaded {
        session: Mutex::new(session),
        provider,
        last_used: Mutex::new(Instant::now()),
    });
    SESSIONS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(key, loaded.clone());
    Ok(loaded)
}

/// Held for the length of a job; when it drops (success, error or cancel) the idle rules run.
pub struct JobScope;

impl Drop for JobScope {
    fn drop(&mut self) {
        job_done();
    }
}

/// Called when a job ends: drops models that have been idle past the limit, and starts the
/// watcher that drops the rest once they have been idle for 60 seconds. With
/// `PANOPLY_MODEL_IDLE_SECS=0` every model is dropped right away.
pub fn job_done() {
    evict_idle();
    let mut running = REAPER.lock().unwrap_or_else(|p| p.into_inner());
    let empty = SESSIONS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .is_none_or(HashMap::is_empty);
    if *running || empty {
        return;
    }
    *running = true;
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(Duration::from_secs(5).min(idle_ttl().max(Duration::from_secs(1))));
            evict_idle();
            let empty = SESSIONS
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .is_none_or(HashMap::is_empty);
            if empty {
                *REAPER.lock().unwrap_or_else(|p| p.into_inner()) = false;
                return;
            }
        }
    });
}

/// Drop every loaded model nobody is using that has been idle for the limit.
pub fn evict_idle() {
    evict_idle_with(idle_ttl());
}

/// Like [`evict_idle`], with the limit given.
pub fn evict_idle_with(ttl: Duration) {
    let mut g = SESSIONS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(m) = g.as_mut() {
        m.retain(|_, l| {
            let idle = l
                .last_used
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .elapsed();
            Arc::strong_count(l) > 1 || idle < ttl
        });
    }
}

/// Number of models currently loaded (tests and the idle check).
pub fn loaded_count() -> usize {
    SESSIONS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map_or(0, HashMap::len)
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
    fn idle_limit_defaults_to_sixty_seconds() {
        if std::env::var_os("PANOPLY_MODEL_IDLE_SECS").is_none() {
            assert_eq!(idle_ttl(), Duration::from_secs(60));
        }
    }

    #[test]
    fn engines_live_outside_the_repository() {
        assert!(!engines_dir().starts_with(env!("CARGO_MANIFEST_DIR")));
    }
}
