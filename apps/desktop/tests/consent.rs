//! The desktop side of download consent: a job that needs a model or engine pauses and asks the
//! panel, nothing is fetched or recorded until the person says Download, and Not now ends the
//! job with nothing changed. The module here is a stand-in that calls the same gate the real
//! engines call, so no test touches the network.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use panoply_core::assets::{self, AssetInfo, AssetKind};
use panoply_desktop::jobs::{self, ConsentReply, JobMsg, JobReport, JobRequest};
use panoply_host::{Module, ModuleError, Registry, RunContext};
use panoply_manifest::Manifest;

static TURN: Mutex<()> = Mutex::new(());

fn start(tag: &str) -> (MutexGuard<'static, ()>, PathBuf) {
    let guard = TURN.lock().unwrap_or_else(|p| p.into_inner());
    let dir = std::env::temp_dir().join(format!(
        "panoply-desktop-consent-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assets::set_data_dir_override(Some(dir.clone()));
    assets::ignore_environment_yes(true);
    assets::set_assume_yes(false);
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
        name: "IS-Net background model".into(),
        kind: AssetKind::Model,
        size: 44_229_662,
        source: "huggingface.co".into(),
        license: "MIT".into(),
        purpose: "removes backgrounds".into(),
    }
}

struct NeedsItem {
    manifest: Manifest,
    item: AssetInfo,
}

impl Module for NeedsItem {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    fn run(
        &self,
        _: &RunContext,
        _: &[PathBuf],
        _: &serde_json::Value,
    ) -> Result<Vec<PathBuf>, ModuleError> {
        assets::authorize(&self.item).map_err(|e| ModuleError::Failed(e.to_string()))?;
        Ok(vec![])
    }
}

fn registry(item: &str) -> Arc<Registry> {
    let manifest = Manifest::from_toml_str(
        r#"
accepts = ["*/*"]
outputs = []

[module]
id = "test.needs_model"
name = "Needs a model"
version = "0.1.0"
pattern = "I"
summary = "Test module."

[capabilities]
files = "none"
network = true
network_reason = "Downloads a test model once."

[surfaces]
bar = false
panel = false
right_click = false
share_sheet = false
cli = false
mcp = false
"#,
    )
    .unwrap();
    let mut r = Registry::new();
    r.register(Arc::new(NeedsItem {
        manifest,
        item: info(item),
    }))
    .unwrap();
    Arc::new(r)
}

fn spawn(item: &str) -> (jobs::RunningJob, Receiver<JobMsg>) {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let job = jobs::spawn(
        registry(item),
        JobRequest {
            tool_id: "test.needs_model".into(),
            tool_name: "Needs a model".into(),
            inputs: vec![],
            options: serde_json::Value::Null,
            out_dir: None,
        },
        move |m| {
            let _ = tx.lock().unwrap().send(m);
        },
    );
    (job, rx)
}

/// The next message that is not progress.
fn next(rx: &Receiver<JobMsg>) -> JobMsg {
    loop {
        match rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the job sent a message")
        {
            JobMsg::Progress { .. } => continue,
            m => return m,
        }
    }
}

fn expect_consent(rx: &Receiver<JobMsg>) -> (AssetInfo, ConsentReply) {
    match next(rx) {
        JobMsg::NeedsConsent { info, reply } => (info, reply),
        other => panic!("expected the consent question, got {other:?}"),
    }
}

fn expect_finished(rx: &Receiver<JobMsg>) -> JobReport {
    match next(rx) {
        JobMsg::Finished(r) => *r,
        other => panic!("expected the job to finish, got {other:?}"),
    }
}

#[test]
fn the_job_pauses_on_the_question_and_goes_on_after_download() {
    let (_t, dir) = start("yes");
    let (_job, rx) = spawn("isnet");
    let (shown, reply) = expect_consent(&rx);
    assert_eq!(shown.name, "IS-Net background model");
    assert_eq!(shown.size_label(), "44 MB");
    assert_eq!(
        (shown.source.as_str(), shown.license.as_str()),
        ("huggingface.co", "MIT")
    );

    // Paused: nothing finishes and nothing is recorded while the sheet is open.
    assert!(
        rx.recv_timeout(Duration::from_millis(400)).is_err(),
        "still waiting"
    );
    assert!(!dir.join("consent.json").exists());

    reply.answer(true);
    let report = expect_finished(&rx);
    assert!(report.result.is_ok(), "{:?}", report.result);
    assert!(assets::is_allowed("isnet"), "Download is remembered");

    // The same item never asks again; a different one does.
    let (_job, rx) = spawn("isnet");
    assert!(expect_finished(&rx).result.is_ok(), "no second question");
    let (_job, rx) = spawn("migan");
    let (_, reply) = expect_consent(&rx);
    reply.answer(true);
    assert!(expect_finished(&rx).result.is_ok());
    finish(dir);
}

#[test]
fn not_now_ends_the_job_with_nothing_recorded_and_asks_again_next_time() {
    let (_t, dir) = start("no");
    let (_job, rx) = spawn("isnet");
    let (_, reply) = expect_consent(&rx);
    reply.answer(false);
    let report = expect_finished(&rx);
    let failure = report.result.expect_err("declined");
    assert!(!failure.cancelled);
    assert!(
        failure.message.contains("you said no") && failure.message.contains("Nothing was changed"),
        "{}",
        failure.message
    );
    assert!(!assets::is_allowed("isnet"));
    assert!(!dir.join("consent.json").exists());

    let (_job, rx) = spawn("isnet");
    let (_, reply) = expect_consent(&rx);
    reply.answer(true);
    assert!(expect_finished(&rx).result.is_ok());
    finish(dir);
}

#[test]
fn closing_the_sheet_or_cancelling_stops_the_job_without_a_download() {
    let (_t, dir) = start("gone");
    // The panel went away: the reply is dropped without an answer.
    let (_job, rx) = spawn("isnet");
    let (_, reply) = expect_consent(&rx);
    drop(reply);
    assert!(expect_finished(&rx).result.is_err());

    // Cancel while the question is open.
    let (job, rx) = spawn("isnet");
    let (_, _reply) = expect_consent(&rx);
    job.cancel.cancel();
    assert!(expect_finished(&rx).result.is_err());
    assert!(!assets::is_allowed("isnet"));
    assert!(!dir.join("consent.json").exists());
    finish(dir);
}
