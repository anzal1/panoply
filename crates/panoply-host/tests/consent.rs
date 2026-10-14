//! The host's side of download consent: what a job does when a module reaches for a model or
//! engine, with and without a yes, and with and without a network declaration.
//!
//! The modules here are tiny test modules that call the same gate every real download site
//! calls (`panoply_core::assets::authorize`), so nothing touches the network. The real download
//! sites are covered by the command line tests, which run the binary with its own data folder.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use panoply_core::assets::{self, Answer, Asker, AssetInfo, AssetKind, ConsentError};
use panoply_core::{CancelToken, JobSpec};
use panoply_host::{Module, ModuleError, Registry, RunContext};
use panoply_manifest::Manifest;

/// The gate keeps process-wide state (the asker, the data folder), so these tests take turns.
static TURN: Mutex<()> = Mutex::new(());

/// Start a test: take the turn, send the consent file to a fresh folder, ignore `PANOPLY_YES`
/// (CI sets it for the download tests), and forget any asker.
fn start(tag: &str) -> (MutexGuard<'static, ()>, PathBuf) {
    let guard = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir =
        std::env::temp_dir().join(format!("panoply-host-consent-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assets::set_data_dir_override(Some(dir.clone()));
    assets::ignore_environment_yes(true);
    assets::set_assume_yes(false);
    assets::set_asker(None);
    (guard, dir)
}

fn finish(dir: PathBuf) {
    assets::set_asker(None);
    assets::set_data_dir_override(None);
    let _ = std::fs::remove_dir_all(dir);
}

fn info(id: &str) -> AssetInfo {
    AssetInfo {
        id: id.into(),
        name: format!("{id} test model"),
        kind: AssetKind::Model,
        size: 44_000_000,
        source: "example.org".into(),
        license: "MIT".into(),
        purpose: "tests".into(),
    }
}

/// A module that needs `item` before it can do its work, like every engine that downloads.
struct NeedsItem {
    manifest: Manifest,
    item: AssetInfo,
}

fn manifest(id: &str, network: bool) -> Manifest {
    let reason = if network {
        "network_reason = \"Downloads a test model once.\"\n"
    } else {
        ""
    };
    Manifest::from_toml_str(&format!(
        r#"
accepts = ["*/*"]
outputs = []

[module]
id = "{id}"
name = "Needs a model"
version = "0.1.0"
pattern = "I"
summary = "Test module."

[capabilities]
files = "none"
network = {network}
{reason}
[surfaces]
bar = false
panel = false
right_click = false
share_sheet = false
cli = false
mcp = false
"#
    ))
    .expect("test manifest parses")
}

impl Module for NeedsItem {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run(
        &self,
        _ctx: &RunContext,
        _inputs: &[PathBuf],
        _options: &serde_json::Value,
    ) -> Result<Vec<PathBuf>, ModuleError> {
        // What an engine does: ask the gate, and when it is refused, fail with its own plain
        // error string (the host recovers the structured reason).
        assets::authorize(&self.item).map_err(|e| ModuleError::Failed(format!("engine: {e}")))?;
        Ok(vec![])
    }
}

fn registry(item: &str, network: bool) -> Registry {
    let mut r = Registry::new();
    r.register(Arc::new(NeedsItem {
        manifest: manifest("test.needs_model", network),
        item: info(item),
    }))
    .unwrap();
    r
}

fn run(r: &Registry) -> Result<panoply_host::JobOutcome, ModuleError> {
    let spec = JobSpec {
        module_id: "test.needs_model".into(),
        inputs: vec![],
        options: serde_json::Value::Null,
    };
    r.run_job(&spec, None, CancelToken::new(), |_| {})
}

struct Counting(Answer, Mutex<Vec<String>>);

impl Counting {
    fn new(a: Answer) -> Arc<Self> {
        Arc::new(Self(a, Mutex::new(Vec::new())))
    }
    fn asked(&self) -> Vec<String> {
        self.1.lock().unwrap().clone()
    }
}

impl Asker for Counting {
    fn ask(&self, info: &AssetInfo) -> Answer {
        self.1.lock().unwrap().push(info.id.clone());
        self.0
    }
}

#[test]
fn without_a_yes_or_a_way_to_ask_the_job_stops_with_the_reason_and_writes_nothing() {
    let (_t, dir) = start("needed");
    let err = run(&registry("isnet", true)).unwrap_err();
    let ModuleError::Consent(ConsentError::Needed(i)) = &err else {
        panic!("expected a consent error, got {err:?}");
    };
    assert_eq!(i.id, "isnet");
    let said = err.plain_message();
    assert!(
        said.contains("44 MB") && said.contains("example.org") && said.contains("MIT"),
        "{said}"
    );
    let next = err.next_step();
    assert!(
        next.contains("panoply models allow isnet") && next.contains("--yes"),
        "{next}"
    );
    assert!(
        !dir.join("consent.json").exists(),
        "a refusal records nothing"
    );
    finish(dir);
}

#[test]
fn a_no_stops_the_job_and_is_asked_again_next_time() {
    let (_t, dir) = start("no");
    let asker = Counting::new(Answer::No);
    assets::set_asker(Some(asker.clone()));
    let r = registry("migan", true);
    let err = run(&r).unwrap_err();
    assert!(
        matches!(err, ModuleError::Consent(ConsentError::Declined(_))),
        "{err:?}"
    );
    assert!(err.plain_message().contains("Nothing was changed"));
    assert!(run(&r).is_err());
    assert_eq!(asker.asked(), ["migan", "migan"], "a no is not remembered");
    assert!(!dir.join("consent.json").exists());
    finish(dir);
}

#[test]
fn a_yes_lets_the_job_run_and_is_remembered_per_item() {
    let (_t, dir) = start("yes");
    let asker = Counting::new(Answer::Yes);
    assets::set_asker(Some(asker.clone()));
    let r = registry("isnet", true);
    run(&r).expect("allowed");
    run(&r).expect("still allowed");
    assert_eq!(asker.asked(), ["isnet"], "asked once, then remembered");
    assert!(assets::is_allowed("isnet"));
    assert!(dir.join("consent.json").is_file());

    // A yes covers that item only.
    run(&registry("esrgan", true)).expect("allowed after asking");
    assert_eq!(asker.asked(), ["isnet", "esrgan"]);

    // Remembered across a restart: no asker at all and it still runs.
    assets::set_asker(None);
    run(&r).expect("remembered from the file");

    // Forgetting (what Remove does) means the question comes back.
    assets::forget("isnet").unwrap();
    assert!(matches!(
        run(&r).unwrap_err(),
        ModuleError::Consent(ConsentError::Needed(_))
    ));
    finish(dir);
}

#[test]
fn yes_on_the_command_line_allows_and_records() {
    let (_t, dir) = start("flag");
    assets::set_assume_yes(true);
    run(&registry("isnet", true)).expect("--yes");
    assets::set_assume_yes(false);
    assert!(assets::is_allowed("isnet"), "the yes is recorded per item");
    finish(dir);
}

#[test]
fn a_module_that_never_declared_the_network_gets_no_download_even_after_a_yes() {
    let (_t, dir) = start("scope");
    assets::allow("isnet").unwrap();
    let asker = Counting::new(Answer::Yes);
    assets::set_asker(Some(asker.clone()));
    let err = run(&registry("isnet", false)).unwrap_err();
    assert!(
        matches!(err, ModuleError::Consent(ConsentError::NotDeclared(_))),
        "{err:?}"
    );
    assert!(
        err.plain_message()
            .contains("does not declare network access")
    );
    assert!(
        asker.asked().is_empty(),
        "nobody is asked on behalf of a module that may not ask"
    );
    // The same item for a module that did declare it is fine.
    run(&registry("isnet", true)).expect("declared");
    finish(dir);
}

#[test]
fn a_refusal_does_not_leak_into_the_next_job() {
    let (_t, dir) = start("leak");
    assert!(run(&registry("isnet", true)).is_err());
    // A module that fails for its own reasons reports that, not the earlier refusal.
    struct Broken(Manifest);
    impl Module for Broken {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn run(
            &self,
            _: &RunContext,
            _: &[PathBuf],
            _: &serde_json::Value,
        ) -> Result<Vec<PathBuf>, ModuleError> {
            Err(ModuleError::Failed("its own problem".into()))
        }
    }
    let mut r = Registry::new();
    r.register(Arc::new(Broken(manifest("test.needs_model", true))))
        .unwrap();
    let err = run(&r).unwrap_err();
    assert!(
        matches!(err, ModuleError::Failed(ref m) if m == "its own problem"),
        "{err:?}"
    );
    finish(dir);
}

#[test]
fn every_downloadable_item_says_what_it_is_before_anyone_agrees_to_it() {
    let (_t, dir) = start("catalog");
    let catalog = panoply_host::assets::catalog();
    let ids: Vec<&str> = catalog.iter().map(|e| e.info.id.as_str()).collect();
    for want in [
        "pdfium",
        "onnxruntime",
        "isnet",
        "migan",
        "esrgan",
        "ppocr-det",
    ] {
        assert!(ids.contains(&want), "{want} is listed: {ids:?}");
    }
    let mut seen = std::collections::BTreeSet::new();
    for e in &catalog {
        let i = &e.info;
        assert!(seen.insert(i.id.clone()), "{} is listed twice", i.id);
        assert!(i.size > 0, "{} has a size", i.id);
        assert!(
            !i.name.is_empty() && !i.purpose.is_empty(),
            "{} has words",
            i.id
        );
        assert!(i.license.len() > 2, "{} has a license", i.id);
        assert!(
            i.source.contains('.') && !i.source.contains("://"),
            "{}: the source is a host name, got {:?}",
            i.id,
            i.source
        );
        assert!(!e.paths.is_empty(), "{} says where it lives", i.id);
    }
    // Nothing is on this computer or allowed in a fresh data folder.
    assert!(catalog.iter().all(|e| !e.status().allowed));
    finish(dir);
}

#[test]
fn the_modules_that_can_download_say_so_in_their_manifest() {
    let (_t, dir) = start("manifests");
    let r = Registry::with_builtins();
    for m in r.list() {
        let c = &m.manifest().capabilities;
        if c.network {
            assert!(
                c.network_reason.as_deref().is_some_and(|t| t.len() > 20),
                "{} asks for the network without saying why",
                m.manifest().module.id
            );
        }
    }
    for id in [
        "image.remove_background",
        "image.erase",
        "image.remove_watermark",
        "image.upscale",
        "media.convert",
        "media.compress",
    ] {
        let m = r.get(id).unwrap_or_else(|| panic!("{id} exists"));
        assert!(
            m.manifest().capabilities.network,
            "{id} downloads, so it declares the network"
        );
    }
    for id in ["image.compress", "image.resize", "files.hash"] {
        let m = r.get(id).unwrap_or_else(|| panic!("{id} exists"));
        assert!(!m.manifest().capabilities.network, "{id} is offline");
    }
    finish(dir);
}
