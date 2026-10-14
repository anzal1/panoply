//! Consent for downloads: the one gate every model and engine download goes through.
//!
//! Panoply never downloads anything silently (VISION.md: nothing leaves your device). Models
//! and engines (ONNX Runtime, the background, fill, upscale and face models, the OCR models,
//! pdfium, the video helper) are fetched once on first use, and only after the person has said
//! yes to that item. Each download site calls [`authorize`] right before it touches the
//! network, with an [`AssetInfo`] that says what the item is, how big it is, where it comes from
//! and its license. The answer is remembered per item in `consent.json` in the data folder, and
//! [`revoke`] forgets it.
//!
//! How [`authorize`] decides, in order:
//!
//! 1. The job's manifest did not declare the network ([`with_network_scope`]): refused.
//! 2. The item was allowed before: go ahead.
//! 3. `PANOPLY_YES` is set (the environment form of `--yes`): record the item and go ahead.
//! 4. A surface installed an [`Asker`] (the CLI prompt, the desktop sheet): ask, and record a yes.
//! 5. Otherwise refuse with a plain message that says how to allow it.
//!
//! Nothing here touches the network, and a refusal never changes anything on disk.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

/// What kind of thing is downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// A program or library: ONNX Runtime, pdfium, the video helper.
    Engine,
    /// Model weights or language data.
    Model,
}

impl AssetKind {
    /// "engine" or "model", for messages.
    pub fn word(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::Model => "model",
        }
    }
}

/// Everything a person needs to know before agreeing to a download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetInfo {
    /// Stable id, for example `isnet` or `pdfium`. The consent is remembered under it.
    pub id: String,
    /// What it is called, without an article: "IS-Net background model".
    pub name: String,
    /// Engine or model.
    pub kind: AssetKind,
    /// Download size in bytes.
    pub size: u64,
    /// Where it comes from: the host name, for example `huggingface.co`.
    pub source: String,
    /// SPDX license id of what is downloaded.
    pub license: String,
    /// What it is for, in a few plain words: "removes backgrounds when the system cannot".
    pub purpose: String,
}

impl AssetInfo {
    /// "44 MB" or "4.5 MB" or "232 KB", decimal like a file manager.
    pub fn size_label(&self) -> String {
        format_size(self.size)
    }

    /// One sentence for a prompt or a sheet: what, how big, from where, under which license.
    pub fn summary(&self) -> String {
        format!(
            "{} ({}, {} license, from {})",
            self.name,
            self.size_label(),
            self.license,
            self.source
        )
    }
}

/// The ONNX Runtime, shared by the face, OCR and image AI engines. Three crates cache it, so the
/// description lives here: one consent covers all of them.
pub fn onnxruntime(version: &str, size: u64) -> AssetInfo {
    AssetInfo {
        id: "onnxruntime".into(),
        name: format!("ONNX Runtime {version}"),
        kind: AssetKind::Engine,
        size,
        source: "github.com".into(),
        license: "MIT".into(),
        purpose: "runs the small models for faces, text and image editing".into(),
    }
}

/// A byte count the way a file manager shows it: "44 MB", "4.5 MB", "232 KB", "0 B".
pub fn format_size(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes >= 10_000_000 {
        format!("{:.0} MB", b / 1e6)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", b / 1e6)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// Why a download was not allowed. The message is plain words with a next step.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConsentError {
    /// Nobody has said yes to this item yet and nobody could be asked.
    #[error(
        "Panoply needs to download the {}, and that has not been allowed yet. Run the command again with --yes to allow it, or run `panoply models allow {}` first, then try again.",
        .0.summary(),
        .0.id
    )]
    Needed(Box<AssetInfo>),
    /// The person was asked and said no.
    #[error(
        "The {} was not downloaded because you said no. Nothing was changed. Run it again and say yes to continue.",
        .0.name
    )]
    Declined(Box<AssetInfo>),
    /// The module that needs the item never declared network access in its manifest.
    #[error(
        "This module needs the {} but its manifest does not declare network access, so the download was refused.",
        .0.name
    )]
    NotDeclared(Box<AssetInfo>),
}

impl ConsentError {
    /// The item the decision was about.
    pub fn info(&self) -> &AssetInfo {
        match self {
            Self::Needed(i) | Self::Declined(i) | Self::NotDeclared(i) => i.as_ref(),
        }
    }
}

/// The answer to "may Panoply download this?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Yes, and remember it.
    Yes,
    /// No. Not remembered, so the next job asks again.
    No,
}

/// Something that can put the question to a person: a terminal prompt, a sheet in the app.
/// It may block until they answer.
pub trait Asker: Send + Sync {
    /// Ask whether `info` may be downloaded.
    fn ask(&self, info: &AssetInfo) -> Answer;
}

static ASKER: RwLock<Option<Arc<dyn Asker>>> = RwLock::new(None);

/// Install the surface's way of asking (or remove it with `None`). One per process.
pub fn set_asker(asker: Option<Arc<dyn Asker>>) {
    *ASKER.write().unwrap_or_else(|p| p.into_inner()) = asker;
}

fn current_asker() -> Option<Arc<dyn Asker>> {
    ASKER.read().unwrap_or_else(|p| p.into_inner()).clone()
}

thread_local! {
    static NETWORK_SCOPE: Cell<Option<bool>> = const { Cell::new(None) };
    static REFUSAL: RefCell<Option<ConsentError>> = const { RefCell::new(None) };
}

/// Forget any refusal left on this thread. The host calls this before it runs a module.
pub fn clear_refusal() {
    REFUSAL.with(|r| r.borrow_mut().take());
}

/// The last refusal [`authorize`] gave on this thread since [`clear_refusal`], if any. Engines
/// turn it into a plain string deep inside their own error types, so the host uses this to
/// recover the structured reason when a module fails.
pub fn take_refusal() -> Option<ConsentError> {
    REFUSAL.with(|r| r.borrow_mut().take())
}

/// Run `f` with the network declared (or not) for the module running on this thread. The host
/// wraps every module run in this, so a module whose manifest says no network cannot get a
/// download through, whatever engine it calls. Outside any scope (the `models` command, tests)
/// downloads are decided by consent alone.
pub fn with_network_scope<T>(declared: bool, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            NETWORK_SCOPE.with(|s| s.set(self.0));
        }
    }
    let _restore = Restore(NETWORK_SCOPE.with(|s| s.replace(Some(declared))));
    f()
}

static YES_FLAG: AtomicBool = AtomicBool::new(false);
static IGNORE_ENV_YES: AtomicBool = AtomicBool::new(false);

/// Say yes to every download for the rest of this process (the command line's `--yes`).
pub fn set_assume_yes(yes: bool) {
    YES_FLAG.store(yes, Ordering::Relaxed);
}

/// True after [`set_assume_yes`], or when `PANOPLY_YES` is set to something other than empty,
/// `0`, `false` or `no`: the environment form of `--yes`, for scripts, CI and agents that cannot
/// answer a prompt. A yes given this way is still recorded per item.
pub fn assume_yes() -> bool {
    YES_FLAG.load(Ordering::Relaxed)
        || (!IGNORE_ENV_YES.load(Ordering::Relaxed)
            && yes_value(std::env::var("PANOPLY_YES").ok().as_deref()))
}

/// Make this process ignore `PANOPLY_YES`. Tests that check what happens without a yes call it,
/// because the test run itself (CI) may have `PANOPLY_YES` set so that download tests can run.
pub fn ignore_environment_yes(ignore: bool) {
    IGNORE_ENV_YES.store(ignore, Ordering::Relaxed);
}

fn yes_value(v: Option<&str>) -> bool {
    v.is_some_and(|v| {
        !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no"
        )
    })
}

static DATA_DIR_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Point [`data_dir`] somewhere else for this process (tests, which cannot set environment
/// variables). `None` goes back to the normal choice.
pub fn set_data_dir_override(dir: Option<PathBuf>) {
    *DATA_DIR_OVERRIDE.write().unwrap_or_else(|p| p.into_inner()) = dir;
}

/// Where Panoply keeps its data (history, consent, community modules). `PANOPLY_DATA_DIR`
/// overrides it (tests and portable installs).
pub fn data_dir() -> PathBuf {
    if let Some(p) = DATA_DIR_OVERRIDE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
    {
        return p;
    }
    if let Some(p) = std::env::var_os("PANOPLY_DATA_DIR") {
        return PathBuf::from(p);
    }
    let home = || {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Panoply")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData/Roaming"))
            .join("Panoply")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("panoply")
    }
}

/// The file the decisions are remembered in.
pub fn consent_file() -> PathBuf {
    data_dir().join("consent.json")
}

/// The remembered decisions, kept in one small JSON file.
#[derive(Debug, Clone)]
pub struct ConsentStore {
    path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    version: u32,
    /// Item id to the Unix time (seconds) it was allowed.
    allowed: BTreeMap<String, u64>,
}

impl ConsentStore {
    /// The store in the user's data folder.
    pub fn user() -> Self {
        Self::at(consent_file())
    }

    /// A store in this file (tests).
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn load(&self) -> Stored {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, s: &Stored) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self
            .path
            .with_extension(format!("json.part-{}", std::process::id()));
        let body = serde_json::to_vec_pretty(s).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, body)?;
        // Rename is atomic on one volume, so a reader never sees half a file.
        std::fs::rename(&tmp, &self.path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }

    /// True when `id` was allowed.
    pub fn is_allowed(&self, id: &str) -> bool {
        self.load().allowed.contains_key(id)
    }

    /// Every allowed id, with the Unix time it was allowed.
    pub fn allowed(&self) -> BTreeMap<String, u64> {
        self.load().allowed
    }

    /// Remember a yes for `id`.
    pub fn allow(&self, id: &str) -> std::io::Result<()> {
        let mut s = self.load();
        s.version = 1;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        s.allowed.insert(id.to_string(), now);
        self.save(&s)
    }

    /// Forget the decision for `id`, so the next download asks again.
    pub fn forget(&self, id: &str) -> std::io::Result<()> {
        let mut s = self.load();
        if s.allowed.remove(id).is_some() {
            s.version = 1;
            self.save(&s)?;
        }
        Ok(())
    }
}

/// Decide whether `info` may be downloaded now, against `store`. See the module docs for the
/// order. A yes from the environment or an asker is recorded; a failure to record it does not
/// stop the download (the person said yes), it only means the question is asked again later.
pub fn authorize_with(store: &ConsentStore, info: &AssetInfo) -> Result<(), ConsentError> {
    authorize_inner(store, info, assume_yes())
}

fn authorize_inner(store: &ConsentStore, info: &AssetInfo, yes: bool) -> Result<(), ConsentError> {
    let r = decide(store, info, yes);
    if let Err(e) = &r {
        REFUSAL.with(|x| *x.borrow_mut() = Some(e.clone()));
    }
    r
}

fn decide(store: &ConsentStore, info: &AssetInfo, yes: bool) -> Result<(), ConsentError> {
    if NETWORK_SCOPE.with(|s| s.get()) == Some(false) {
        return Err(ConsentError::NotDeclared(Box::new(info.clone())));
    }
    if store.is_allowed(&info.id) {
        return Ok(());
    }
    if yes {
        let _ = store.allow(&info.id);
        return Ok(());
    }
    match current_asker() {
        Some(asker) => match asker.ask(info) {
            Answer::Yes => {
                let _ = store.allow(&info.id);
                Ok(())
            }
            Answer::No => Err(ConsentError::Declined(Box::new(info.clone()))),
        },
        None => Err(ConsentError::Needed(Box::new(info.clone()))),
    }
}

/// [`authorize_with`] on the user's store. Every download site calls this, once, right before
/// it touches the network.
pub fn authorize(info: &AssetInfo) -> Result<(), ConsentError> {
    authorize_with(&ConsentStore::user(), info)
}

/// True when the person already allowed `id`.
pub fn is_allowed(id: &str) -> bool {
    ConsentStore::user().is_allowed(id)
}

/// Record a yes for `id` (the `models allow` command, the sheet's Download button).
pub fn allow(id: &str) -> std::io::Result<()> {
    ConsentStore::user().allow(id)
}

/// Forget the decision for `id`.
pub fn forget(id: &str) -> std::io::Result<()> {
    ConsentStore::user().forget(id)
}

/// One downloadable item and where it lives on disk once installed.
#[derive(Debug, Clone)]
pub struct Entry {
    /// What it is.
    pub info: AssetInfo,
    /// The files or folders it occupies when installed. More than one when two crates cache the
    /// same item in different places (the ONNX Runtime).
    pub paths: Vec<PathBuf>,
}

/// What an [`Entry`] looks like on disk right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// True when at least one of its paths exists.
    pub installed: bool,
    /// Bytes on disk across its paths.
    pub bytes: u64,
    /// True when the person allowed the download (an item can be installed without it, from
    /// before consent existed or put there by hand).
    pub allowed: bool,
}

impl Entry {
    /// Look at the disk and the consent store.
    pub fn status(&self) -> Status {
        self.status_in(&ConsentStore::user())
    }

    /// [`Entry::status`] against a given store.
    pub fn status_in(&self, store: &ConsentStore) -> Status {
        let live: Vec<&PathBuf> = self.paths.iter().filter(|p| p.exists()).collect();
        Status {
            installed: !live.is_empty(),
            bytes: live.iter().map(|p| disk_bytes(p)).sum(),
            allowed: store.is_allowed(&self.info.id),
        }
    }

    /// Delete the item from disk and forget the consent. Returns the bytes freed. The next job
    /// that needs it asks again. Never touches anything but this item's own paths.
    pub fn remove(&self) -> std::io::Result<u64> {
        self.remove_in(&ConsentStore::user())
    }

    /// [`Entry::remove`] against a given store.
    pub fn remove_in(&self, store: &ConsentStore) -> std::io::Result<u64> {
        let mut freed = 0;
        for p in &self.paths {
            if !p.exists() {
                continue;
            }
            freed += disk_bytes(p);
            if p.is_dir() {
                std::fs::remove_dir_all(p)?;
            } else {
                std::fs::remove_file(p)?;
            }
        }
        store.forget(&self.info.id)?;
        Ok(freed)
    }
}

/// Size of a file, or of everything inside a folder.
pub fn disk_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| disk_bytes(&e.path()))
        .sum()
}

/// Put entries with the same id together (the ONNX Runtime is cached by three crates), keeping
/// the first description and gathering every path. Order of first appearance is kept.
pub fn merge_entries(entries: impl IntoIterator<Item = Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for e in entries {
        match out.iter_mut().find(|x| x.info.id == e.info.id) {
            Some(x) => {
                for p in e.paths {
                    if !x.paths.contains(&p) {
                        x.paths.push(p);
                    }
                }
            }
            None => out.push(e),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that install the process-wide asker or read `PANOPLY_YES` run one at a time.
    static SERIAL_LOCK: Mutex<()> = Mutex::new(());

    /// Take the lock, and ignore `PANOPLY_YES` (CI sets it for the download tests).
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        let g = SERIAL_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        ignore_environment_yes(true);
        g
    }

    fn info(id: &str) -> AssetInfo {
        AssetInfo {
            id: id.into(),
            name: "Test model".into(),
            kind: AssetKind::Model,
            size: 44_229_662,
            source: "example.org".into(),
            license: "MIT".into(),
            purpose: "tests".into(),
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-assets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    struct Fixed(Answer, Mutex<u32>);
    impl Asker for Fixed {
        fn ask(&self, _: &AssetInfo) -> Answer {
            *self.1.lock().unwrap() += 1;
            self.0
        }
    }

    #[test]
    fn sizes_read_like_a_file_manager() {
        assert_eq!(format_size(44_229_662), "44 MB");
        assert_eq!(format_size(4_540_895), "4.5 MB");
        assert_eq!(format_size(232_589), "233 KB");
        assert_eq!(format_size(12), "12 B");
    }

    #[test]
    fn nothing_downloads_without_a_yes_and_the_message_says_how_to_allow_it() {
        let _s = serial();
        set_asker(None);
        let d = scratch("none");
        let store = ConsentStore::at(d.join("consent.json"));
        let err = authorize_with(&store, &info("isnet")).unwrap_err();
        assert!(matches!(err, ConsentError::Needed(_)));
        let m = err.to_string();
        assert!(
            m.contains("44 MB") && m.contains("MIT") && m.contains("example.org"),
            "{m}"
        );
        assert!(m.contains("panoply models allow isnet"), "{m}");
        assert!(!store.is_allowed("isnet"));
        assert!(!d.join("consent.json").exists(), "a refusal writes nothing");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn the_last_refusal_can_be_taken_back_out() {
        let _s = serial();
        set_asker(None);
        let d = scratch("refusal");
        let store = ConsentStore::at(d.join("consent.json"));
        clear_refusal();
        assert!(take_refusal().is_none());
        let _ = authorize_with(&store, &info("isnet"));
        let r = take_refusal().expect("the refusal was kept");
        assert_eq!(r.info().id, "isnet");
        assert!(take_refusal().is_none(), "taking it clears it");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_yes_is_remembered_per_item() {
        let _s = serial();
        let d = scratch("yes");
        let store = ConsentStore::at(d.join("consent.json"));
        let asker = Arc::new(Fixed(Answer::Yes, Mutex::new(0)));
        set_asker(Some(asker.clone()));
        authorize_with(&store, &info("isnet")).unwrap();
        authorize_with(&store, &info("isnet")).unwrap();
        assert_eq!(*asker.1.lock().unwrap(), 1, "asked once, then remembered");
        assert!(store.is_allowed("isnet"));
        assert!(!store.is_allowed("migan"), "a yes covers one item only");
        authorize_with(&store, &info("migan")).unwrap();
        assert_eq!(*asker.1.lock().unwrap(), 2);
        set_asker(None);
        // A fresh store object reads the same file.
        assert!(ConsentStore::at(d.join("consent.json")).is_allowed("migan"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_no_is_not_remembered_and_nothing_is_recorded() {
        let _s = serial();
        let d = scratch("no");
        let store = ConsentStore::at(d.join("consent.json"));
        set_asker(Some(Arc::new(Fixed(Answer::No, Mutex::new(0)))));
        let err = authorize_with(&store, &info("isnet")).unwrap_err();
        assert!(matches!(err, ConsentError::Declined(_)));
        assert!(err.to_string().contains("Nothing was changed"));
        assert!(!store.is_allowed("isnet"));
        set_asker(Some(Arc::new(Fixed(Answer::Yes, Mutex::new(0)))));
        assert!(
            authorize_with(&store, &info("isnet")).is_ok(),
            "asked again"
        );
        set_asker(None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_manifest_without_network_gets_no_download_even_after_a_yes() {
        let _s = serial();
        set_asker(None);
        let d = scratch("scope");
        let store = ConsentStore::at(d.join("consent.json"));
        store.allow("isnet").unwrap();
        let err = with_network_scope(false, || authorize_with(&store, &info("isnet"))).unwrap_err();
        assert!(matches!(err, ConsentError::NotDeclared(_)));
        assert!(with_network_scope(true, || authorize_with(&store, &info("isnet"))).is_ok());
        // The scope ends with the closure.
        assert!(authorize_with(&store, &info("isnet")).is_ok());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn yes_in_the_environment_records_the_item() {
        let _s = serial();
        set_asker(None);
        let d = scratch("env");
        let store = ConsentStore::at(d.join("consent.json"));
        assert!(authorize_inner(&store, &info("isnet"), true).is_ok());
        assert!(store.is_allowed("isnet"));
        assert!(authorize_inner(&store, &info("migan"), false).is_err());
        assert!(yes_value(Some("1")) && yes_value(Some("true")) && yes_value(Some("yes")));
        assert!(!yes_value(Some("0")) && !yes_value(Some("")) && !yes_value(Some("No")));
        assert!(!yes_value(None));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn removing_an_entry_frees_its_files_and_forgets_the_consent() {
        let d = scratch("remove");
        let store = ConsentStore::at(d.join("consent.json"));
        let dir = d.join("models").join("isnet-1234");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("isnet.onnx"), vec![0u8; 5000]).unwrap();
        let other = d.join("models").join("keep.onnx");
        std::fs::write(&other, b"keep").unwrap();
        store.allow("isnet").unwrap();
        let e = Entry {
            info: info("isnet"),
            paths: vec![dir.clone(), d.join("never-there")],
        };
        let s = e.status_in(&store);
        assert!(s.installed && s.allowed && s.bytes == 5000);
        assert_eq!(e.remove_in(&store).unwrap(), 5000);
        assert!(!dir.exists() && other.exists(), "only its own files go");
        let s = e.status_in(&store);
        assert!(!s.installed && !s.allowed && s.bytes == 0);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn entries_with_the_same_id_merge_their_paths() {
        let a = Entry {
            info: info("onnxruntime"),
            paths: vec![PathBuf::from("/a")],
        };
        let b = Entry {
            info: info("onnxruntime"),
            paths: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        };
        let c = Entry {
            info: info("pdfium"),
            paths: vec![PathBuf::from("/c")],
        };
        let m = merge_entries([a, c, b]);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].paths, vec![PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(m[1].info.id, "pdfium");
    }

    #[test]
    fn a_broken_consent_file_means_nothing_is_allowed() {
        let d = scratch("broken");
        let f = d.join("consent.json");
        std::fs::write(&f, b"{ not json").unwrap();
        let store = ConsentStore::at(&f);
        assert!(!store.is_allowed("isnet"));
        store.allow("isnet").unwrap();
        assert!(store.is_allowed("isnet"));
        let _ = std::fs::remove_dir_all(d);
    }
}
