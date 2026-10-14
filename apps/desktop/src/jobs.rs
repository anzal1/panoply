//! Running a module without touching the UI thread.
//!
//! A job runs on its own worker thread through the same [`Registry::run_job`] the CLI uses.
//! Progress is thinned to about 25 messages a second so a fast module cannot flood the UI, and the
//! final report carries the before and after numbers the result card shows.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use panoply_core::assets::{Answer, AssetInfo, Asker};
use panoply_core::{CancelToken, JobEvent, JobSpec};
use panoply_host::{ModuleError, Registry};
use serde_json::Value;

use crate::units::format_bytes;

/// Longest gap between progress messages the UI sees.
const PROGRESS_EVERY: Duration = Duration::from_millis(40);

/// What to run.
#[derive(Debug, Clone, PartialEq)]
pub struct JobRequest {
    /// Module id.
    pub tool_id: String,
    /// Display name, for the history.
    pub tool_name: String,
    /// Input files.
    pub inputs: Vec<PathBuf>,
    /// Resolved options.
    pub options: Value,
    /// Output folder, or next to each original.
    pub out_dir: Option<PathBuf>,
}

/// How the UI answers a consent question: Download (true) or Not now (false). The job waits for
/// it, so nothing is fetched and nothing is written until the person decides.
#[derive(Debug, Clone)]
pub struct ConsentReply(std::sync::mpsc::Sender<bool>);

impl ConsentReply {
    /// Answer the question. A job that was cancelled meanwhile ignores a late answer.
    pub fn answer(&self, yes: bool) {
        let _ = self.0.send(yes);
    }
}

/// A message from the worker.
#[derive(Debug, Clone)]
pub enum JobMsg {
    /// The job needs a model or engine that was not allowed yet. Show the consent sheet and
    /// answer through `reply`. The job is paused until then.
    NeedsConsent {
        /// What would be downloaded: name, size, source, license, purpose.
        info: AssetInfo,
        /// Where the answer goes.
        reply: ConsentReply,
    },
    /// Work is moving.
    Progress {
        /// 0.0 to 1.0.
        fraction: f32,
        /// What is happening.
        label: String,
    },
    /// The job ended, one way or another. Always the last message.
    Finished(Box<JobReport>),
}

/// A finished job.
#[derive(Debug, Clone)]
pub struct JobReport {
    /// What was run.
    pub request: JobRequest,
    /// How it went.
    pub result: Result<Done, Failure>,
    /// Wall time on the worker.
    pub elapsed: Duration,
}

/// A job that worked.
#[derive(Debug, Clone, PartialEq)]
pub struct Done {
    /// Files written.
    pub outputs: Vec<PathBuf>,
    /// Report lines from the module.
    pub notes: Vec<String>,
    /// Total size of the inputs.
    pub before_bytes: u64,
    /// Total size of the outputs, when there are any.
    pub after_bytes: Option<u64>,
}

/// A job that did not work, in plain words.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// True when the person cancelled.
    pub cancelled: bool,
    /// What happened.
    pub message: String,
    /// What to do next.
    pub next_step: String,
}

impl Done {
    /// "4.2 MB to 880 KB, 79% smaller", "6 files, 12 MB", or the first report line.
    pub fn headline(&self, inputs: usize) -> String {
        let Some(after) = self.after_bytes else {
            return self
                .notes
                .first()
                .cloned()
                .unwrap_or_else(|| "Done".to_string());
        };
        let before = self.before_bytes;
        if self.outputs.len() == inputs && inputs > 0 && before > 0 {
            let change = 1.0 - after as f64 / before as f64;
            let pct = (change.abs() * 100.0).round() as u64;
            let tail = if pct == 0 {
                "about the same size".to_string()
            } else if change > 0.0 {
                format!("{pct}% smaller")
            } else {
                format!("{pct}% bigger")
            };
            format!(
                "{} to {}, {tail}",
                format_bytes(before),
                format_bytes(after)
            )
        } else {
            let n = self.outputs.len();
            format!(
                "{} {}, {}",
                n,
                if n == 1 { "file" } else { "files" },
                format_bytes(after)
            )
        }
    }
}

/// Size of a file, or of everything inside a folder (capped, so a huge tree cannot stall the worker).
pub fn size_of(path: &Path) -> u64 {
    fn walk(p: &Path, budget: &mut u32) -> u64 {
        let Ok(meta) = std::fs::metadata(p) else {
            return 0;
        };
        if !meta.is_dir() {
            return meta.len();
        }
        let mut total = 0;
        for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
            if *budget == 0 {
                break;
            }
            *budget -= 1;
            total += walk(&e.path(), budget);
        }
        total
    }
    walk(path, &mut 10_000)
}

/// A job in flight.
pub struct RunningJob {
    /// Cancel it. Instant, and the module leaves no partial files.
    pub cancel: CancelToken,
}

type Notify = Arc<dyn Fn(JobMsg) + Send + Sync>;

thread_local! {
    /// The running job's way to reach the UI and its cancel flag, set on its worker thread.
    static JOB_CONTEXT: std::cell::RefCell<Option<(Notify, CancelToken)>> =
        const { std::cell::RefCell::new(None) };
}

/// Asks through the UI from a job's worker thread.
struct SheetAsker;

impl Asker for SheetAsker {
    fn ask(&self, info: &AssetInfo) -> Answer {
        let Some((notify, cancel)) = JOB_CONTEXT.with(|c| c.borrow().clone()) else {
            // Not a UI job: nobody to ask, so nothing is downloaded.
            return Answer::No;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        notify(JobMsg::NeedsConsent {
            info: info.clone(),
            reply: ConsentReply(tx),
        });
        loop {
            if cancel.is_cancelled() {
                return Answer::No;
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(true) => return Answer::Yes,
                Ok(false) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Answer::No;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

/// Run a job on a new worker thread. `on_msg` is called from that thread, so it must only hand
/// the message to the UI (a channel), never touch UI state.
pub fn spawn(
    registry: Arc<Registry>,
    request: JobRequest,
    on_msg: impl Fn(JobMsg) + Send + Sync + 'static,
) -> RunningJob {
    let cancel = CancelToken::new();
    let token = cancel.clone();
    let on_msg = Arc::new(on_msg);
    // Downloads are asked about in the panel, before anything is fetched (docs/PRIVACY.md).
    panoply_core::assets::set_asker(Some(Arc::new(SheetAsker)));
    std::thread::Builder::new()
        .name("panoply-job".into())
        .spawn(move || {
            let notify: Notify = on_msg.clone();
            JOB_CONTEXT.with(|c| *c.borrow_mut() = Some((notify, token.clone())));
            let sink = on_msg.clone();
            let report = run_blocking(&registry, request, token, move |fraction, label| {
                sink(JobMsg::Progress { fraction, label });
            });
            JOB_CONTEXT.with(|c| *c.borrow_mut() = None);
            on_msg(JobMsg::Finished(Box::new(report)));
        })
        .expect("spawn job thread");
    RunningJob { cancel }
}

/// Run a job here and now. Used by the worker thread, and directly by tests.
pub fn run_blocking(
    registry: &Registry,
    request: JobRequest,
    cancel: CancelToken,
    on_progress: impl Fn(f32, String) + Send + Sync + 'static,
) -> JobReport {
    let started = Instant::now();
    let before_bytes: u64 = request.inputs.iter().map(|p| size_of(p)).sum();
    let spec = JobSpec {
        module_id: request.tool_id.clone(),
        inputs: request.inputs.clone(),
        options: request.options.clone(),
    };
    let last = Arc::new(Mutex::new(None::<Instant>));
    let on_progress = Arc::new(on_progress);
    let sink = move |event: JobEvent| {
        if let JobEvent::Progress { fraction, label } = event {
            let mut last = last.lock().unwrap_or_else(|p| p.into_inner());
            let due = last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY);
            if due {
                *last = Some(Instant::now());
                on_progress(fraction, label);
            }
        }
    };
    let result = match registry.run_job(&spec, request.out_dir.as_deref(), cancel, sink) {
        Ok(outcome) => {
            let after_bytes = (!outcome.outputs.is_empty())
                .then(|| outcome.outputs.iter().map(|p| size_of(p)).sum());
            Ok(Done {
                outputs: outcome.outputs,
                notes: outcome.notes,
                before_bytes,
                after_bytes,
            })
        }
        Err(e) => Err(failure(&e)),
    };
    JobReport {
        request,
        result,
        elapsed: started.elapsed(),
    }
}

fn failure(e: &ModuleError) -> Failure {
    Failure {
        cancelled: matches!(e, ModuleError::Cancelled),
        message: e.plain_message(),
        next_step: e.next_step(),
    }
}

/// What undo did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UndoReport {
    /// Files removed.
    pub removed: usize,
    /// Files that could not be removed, with the reason.
    pub failed: Vec<String>,
}

/// Undo a result: the files the job wrote go to the Trash through `trash`. The originals were
/// never touched, so that is the whole undo.
pub fn undo_outputs(
    outputs: &[PathBuf],
    trash: impl Fn(&Path) -> std::io::Result<()>,
) -> UndoReport {
    let mut report = UndoReport::default();
    for p in outputs {
        if !p.exists() {
            continue;
        }
        match trash(p) {
            Ok(()) => report.removed += 1,
            Err(e) => report.failed.push(format!("{}: {e}", p.display())),
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("panoply-jobs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A noisy PNG, so compression has something to do.
    fn write_png(path: &Path, w: u32, h: u32) {
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        let mut s = 0x1234_5678u32;
        for _ in 0..(w * h) {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.extend_from_slice(&[(s >> 24) as u8, (s >> 16) as u8, (s >> 8) as u8]);
        }
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(file, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&data).unwrap();
    }

    fn registry() -> Registry {
        Registry::with_builtins()
    }

    #[test]
    fn a_job_runs_off_thread_and_reports_before_and_after() {
        let d = temp("e2e");
        let input = d.join("photo.png");
        write_png(&input, 480, 360);
        let (tx, rx) = std::sync::mpsc::channel();
        let req = JobRequest {
            tool_id: "image.convert".into(),
            tool_name: "Convert image".into(),
            inputs: vec![input.clone()],
            options: json!({"to": "jpg"}),
            out_dir: None,
        };
        let main_thread = std::thread::current().id();
        let job = spawn(Arc::new(registry()), req, move |m| {
            assert_ne!(
                std::thread::current().id(),
                main_thread,
                "work stayed off the caller thread"
            );
            let _ = tx.send(m);
        });
        let mut finished = None;
        while let Ok(m) = rx.recv_timeout(Duration::from_secs(60)) {
            if let JobMsg::Finished(r) = m {
                finished = Some(r);
                break;
            }
        }
        drop(job);
        let report = finished.expect("job finished");
        let done = report.result.expect("convert worked");
        assert_eq!(done.outputs.len(), 1);
        assert!(done.outputs[0].exists());
        assert!(done.before_bytes > 0 && done.after_bytes.unwrap() > 0);
        assert!(
            done.after_bytes.unwrap() < done.before_bytes,
            "noise png to jpg shrinks"
        );
        assert!(done.headline(1).contains("smaller"), "{}", done.headline(1));
        // The original is untouched and the output sits next to it.
        assert!(input.exists());
        assert_eq!(
            done.outputs[0].parent().and_then(|p| p.file_name()),
            input.parent().and_then(|p| p.file_name())
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn cancel_is_a_plain_result_and_leaves_nothing() {
        let d = temp("cancel");
        let input = d.join("photo.png");
        write_png(&input, 64, 64);
        let cancel = CancelToken::new();
        cancel.cancel();
        let report = run_blocking(
            &registry(),
            JobRequest {
                tool_id: "image.convert".into(),
                tool_name: "Convert image".into(),
                inputs: vec![input],
                options: json!({"to": "jpg"}),
                out_dir: None,
            },
            cancel,
            |_, _| {},
        );
        let f = report.result.unwrap_err();
        assert!(f.cancelled);
        assert!(f.message.contains("Nothing was changed"));
        let leftovers: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 1, "only the original remains");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn read_only_modules_report_notes() {
        let d = temp("hash");
        let f = d.join("a.bin");
        std::fs::write(&f, b"hello").unwrap();
        let report = run_blocking(
            &registry(),
            JobRequest {
                tool_id: "files.hash".into(),
                tool_name: "Hash files".into(),
                inputs: vec![f],
                options: Value::Null,
                out_dir: None,
            },
            CancelToken::new(),
            |_, _| {},
        );
        let done = report.result.unwrap();
        assert!(done.outputs.is_empty() && done.after_bytes.is_none());
        assert!(
            done.notes[0].starts_with("2cf24dba5fb0a30e"),
            "{:?}",
            done.notes
        );
        assert!(done.headline(1).starts_with("2cf24dba"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_bad_file_is_a_plain_failure_with_a_next_step() {
        let report = run_blocking(
            &registry(),
            JobRequest {
                tool_id: "files.hash".into(),
                tool_name: "Hash files".into(),
                inputs: vec![PathBuf::from("/definitely/not/here.bin")],
                options: Value::Null,
                out_dir: None,
            },
            CancelToken::new(),
            |_, _| {},
        );
        let f = report.result.unwrap_err();
        assert!(!f.cancelled);
        assert!(!f.message.is_empty() && !f.next_step.is_empty());
    }

    #[test]
    fn headline_words() {
        let done = Done {
            outputs: vec![PathBuf::from("a")],
            notes: vec![],
            before_bytes: 4_200_000,
            after_bytes: Some(880_000),
        };
        assert_eq!(done.headline(1), "4.2 MB to 880 KB, 79% smaller");
        let many = Done {
            outputs: vec![PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c")],
            notes: vec![],
            before_bytes: 4_200_000,
            after_bytes: Some(12_000_000),
        };
        assert_eq!(many.headline(1), "3 files, 12 MB");
    }

    #[test]
    fn undo_trashes_what_the_job_wrote() {
        let d = temp("undo");
        let out = d.join("x (compressed).pdf");
        std::fs::write(&out, b"x").unwrap();
        let gone = d.join("never-existed");
        let r = undo_outputs(&[out.clone(), gone], |p| std::fs::remove_file(p));
        assert_eq!(r.removed, 1);
        assert!(r.failed.is_empty());
        assert!(!out.exists());
        let bad = d.join("again");
        std::fs::write(&bad, b"x").unwrap();
        let r = undo_outputs(&[bad], |_| Err(std::io::Error::other("busy")));
        assert_eq!(r.failed.len(), 1);
        let _ = std::fs::remove_dir_all(d);
    }
}
