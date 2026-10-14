//! The child side: one job, the way the desktop app runs it, reported as one JSON line.
//!
//! The steps are the ones `ToolPanel` takes when a person drops files and presses Return:
//!
//! 1. the file is sniffed from its bytes ([`FileProfile::read`]),
//! 2. the module's manifest must accept that type (the panel's "does not take this kind of file"
//!    blocker),
//! 3. the options form is built from the manifest ([`Form::from_manifest`]) and its default values
//!    are completed by the host ([`Form::to_options`]),
//! 4. the job runs on [`jobs::run_blocking`] with the output next to the original,
//! 5. the result card text is [`jobs::Done::headline`].

use std::path::{Path, PathBuf};
use std::time::Instant;

use panoply_core::{CancelToken, sniff};
use panoply_desktop::catalog::Catalog;
use panoply_desktop::form::Form;
use panoply_desktop::jobs::{self, JobRequest};
use panoply_desktop::profile::FileProfile;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The folder of example WebAssembly modules, relative to the source tree.
pub fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm")
}

/// The catalog the app builds: every built-in module plus the example WebAssembly modules. Only
/// the example folder is searched, never the user's own module folder, so a run means the same
/// thing on every machine.
pub fn catalog() -> Catalog {
    Catalog::with_dirs(&[examples_dir()])
}

/// A file a module wrote.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutFile {
    /// File name.
    pub name: String,
    /// Size in bytes.
    pub bytes: u64,
    /// MIME type the bytes sniff as.
    pub mime: String,
    /// Extension the bytes sniff as, empty when unknown.
    pub ext: String,
}

/// How a job ended, in the words of the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The job wrote what the manifest promises.
    Pass,
    /// The job finished and wrote nothing, and the result card says why: the file was already in
    /// the target form ("already MP4"), or the module reports instead of writing ("5 streams").
    NoOp,
    /// The module refused in plain words with a next step: the file or the default options need a
    /// choice from the person, or the module cannot read that variant yet. This is what the app
    /// shows on its result card, so it works as designed, and it is listed as a known gap.
    Declined,
    /// The job broke a rule: wrong or missing output, empty card, an error with no next step.
    Fail,
}

impl Outcome {
    /// The word in the table.
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Pass => "PASS",
            Outcome::NoOp => "PASS (no change)",
            Outcome::Declined => "DECLINED",
            Outcome::Fail => "FAIL",
        }
    }
}

/// What one job did, as the child reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OneResult {
    /// Module id.
    pub module: String,
    /// How it ended.
    pub outcome: Outcome,
    /// The plain-language failure, when there was one.
    pub error: Option<String>,
    /// The next step the app would show with the failure.
    pub next_step: Option<String>,
    /// The result card text.
    pub headline: String,
    /// Report lines from the module.
    pub notes: Vec<String>,
    /// Files the job wrote, as found on disk afterwards.
    pub outputs: Vec<OutFile>,
    /// Total size of the inputs.
    pub input_bytes: u64,
    /// Total size of the outputs.
    pub output_bytes: u64,
    /// Time inside the job, milliseconds (the worker thread's wall time).
    pub job_ms: f64,
    /// Time to build the catalog (every module registered), milliseconds.
    pub catalog_ms: f64,
    /// The options the form produced from the manifest defaults.
    pub options: Value,
    /// Things that are wrong with an otherwise finished job (empty output, wrong type).
    pub problems: Vec<String>,
}

impl OneResult {
    /// True unless the outcome is [`Outcome::Fail`].
    pub fn ok(&self) -> bool {
        self.outcome != Outcome::Fail
    }

    fn failed(module: &str, error: impl Into<String>, next: impl Into<String>) -> OneResult {
        let next = next.into();
        OneResult {
            module: module.to_string(),
            outcome: Outcome::Fail,
            error: Some(error.into()),
            next_step: Some(next),
            headline: String::new(),
            notes: Vec::new(),
            outputs: Vec::new(),
            input_bytes: 0,
            output_bytes: 0,
            job_ms: 0.0,
            catalog_ms: 0.0,
            options: Value::Null,
            problems: Vec::new(),
        }
    }
}

/// Run `module` on `files` the way the app does. The outputs land next to the inputs, so `files`
/// should live in a scratch folder.
///
/// `typed` is what a person would type into the options form before pressing Return, as
/// `(option, text)` pairs. They go through [`Form::set_text`], the same call the panel's text
/// fields make, so they are parsed and checked like typed input.
pub fn run_one(module: &str, files: &[PathBuf], typed: &[(String, String)]) -> OneResult {
    let t_catalog = Instant::now();
    let catalog = catalog();
    let catalog_ms = t_catalog.elapsed().as_secs_f64() * 1000.0;
    let mut result = run_in(&catalog, module, files, typed);
    result.catalog_ms = catalog_ms;
    result
}

fn run_in(
    catalog: &Catalog,
    module: &str,
    files: &[PathBuf],
    typed: &[(String, String)],
) -> OneResult {
    let Some(i) = catalog.index_of(module) else {
        return OneResult::failed(
            module,
            format!("{module} is not in the catalog."),
            "Build the example modules (see modules/examples-wasm) or pick another tool.",
        );
    };
    let tool = catalog.tool(i);
    let profiles: Vec<FileProfile> = files.iter().map(|f| FileProfile::read(f)).collect();
    let input_bytes: u64 = profiles.iter().map(|p| p.bytes).sum();

    // The panel's blockers, in its order.
    if tool.takes_files() && profiles.is_empty() {
        return OneResult::failed(module, "Add a file to run this tool.", "");
    }
    if let Some(p) = profiles
        .iter()
        .find(|p| !tool.manifest.accepts_file(&p.sniffed.mime, &p.sniffed.ext))
    {
        return OneResult::failed(
            module,
            format!(
                "{} does not take {} files ({} sniffs as {}).",
                tool.name, p.sniffed.ext, p.name, p.sniffed.mime
            ),
            "Remove the file or pick another tool.",
        );
    }
    let mut form = Form::from_manifest(&tool.manifest);
    for (key, text) in typed {
        if let Err(e) = form.set_text(key, text) {
            return OneResult::failed(module, e, "The smoke preset does not fit the manifest.");
        }
    }
    let options = match form.to_options() {
        Ok(o) => o,
        Err(e) => return OneResult::failed(module, e, "The default options do not run."),
    };

    let request = JobRequest {
        tool_id: tool.id.clone(),
        tool_name: tool.name.clone(),
        inputs: files.to_vec(),
        options: options.clone(),
        out_dir: None,
    };
    let n_inputs = request.inputs.len();
    let report = jobs::run_blocking(catalog.registry(), request, CancelToken::new(), |_, _| {});
    let job_ms = report.elapsed.as_secs_f64() * 1000.0;
    match report.result {
        Err(f) => {
            let mut r = OneResult::failed(module, f.message, f.next_step);
            // A plain refusal with a next step is what the result card shows. An error with no
            // next step breaks the "plain errors" rule and stays a failure.
            if !f.cancelled && r.next_step.as_deref().is_some_and(|n| !n.trim().is_empty()) {
                r.outcome = Outcome::Declined;
            }
            r.input_bytes = input_bytes;
            r.job_ms = job_ms;
            r.options = options;
            r
        }
        Ok(done) => {
            let mut problems = Vec::new();
            let outputs: Vec<OutFile> = done
                .outputs
                .iter()
                .map(|p| {
                    let s = sniff(p);
                    OutFile {
                        name: p
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        bytes: jobs::size_of(p),
                        mime: s.mime,
                        ext: s.ext,
                    }
                })
                .collect();
            let headline = done.headline(n_inputs);
            problems.extend(check_outputs(&tool.manifest, &done.outputs, &outputs));
            problems.extend(check_size(
                module,
                input_bytes,
                outputs.iter().map(|o| o.bytes).sum(),
            ));
            if headline.trim().is_empty() {
                problems.push("the result card has no text".into());
            }
            let outcome = if !problems.is_empty() {
                Outcome::Fail
            } else if outputs.is_empty() && !tool.manifest.outputs.is_empty() {
                Outcome::NoOp
            } else {
                Outcome::Pass
            };
            OneResult {
                module: module.to_string(),
                outcome,
                error: None,
                next_step: None,
                headline,
                notes: done.notes,
                output_bytes: outputs.iter().map(|o| o.bytes).sum(),
                outputs,
                input_bytes,
                job_ms,
                catalog_ms: 0.0,
                options,
                problems,
            }
        }
    }
}

/// Modules whose whole purpose is a smaller file. Their result may not be bigger than what went
/// in: "compressed" or "mono" files that came out larger were wrong output the matrix once let
/// through (a 7 kb/s Opus recording became 68 KB, a stereo Vorbis file became a bigger mono one).
const MUST_NOT_GROW: &[&str] = &[
    "media.compress",
    "image.compress",
    "pdf.compress",
    "media.to_mono",
];

/// A result that is bigger than its input, for a module that exists to make files smaller.
fn check_size(module: &str, input_bytes: u64, output_bytes: u64) -> Vec<String> {
    // A little room for container overhead on tiny files.
    let room = input_bytes / 50 + 4096;
    if MUST_NOT_GROW.contains(&module) && output_bytes > input_bytes + room {
        vec![format!(
            "the result is {output_bytes} bytes, bigger than the {input_bytes} bytes that went in, for a module that makes files smaller"
        )]
    } else {
        Vec::new()
    }
}

/// What the manifest promises about the outputs: a module that lists output types writes at least
/// one non-empty file, and each file is one of those types. A module that lists none (a report)
/// writes no files.
fn check_outputs(
    manifest: &panoply_manifest::Manifest,
    paths: &[PathBuf],
    outs: &[OutFile],
) -> Vec<String> {
    let mut problems = Vec::new();
    if manifest.outputs.is_empty() {
        if !outs.is_empty() {
            problems.push(format!(
                "the manifest lists no output types but {} file(s) were written",
                outs.len()
            ));
        }
        return problems;
    }
    // No file is fine when the card says why (already in the target form, a report).
    let mut promised = manifest.clone();
    promised.accepts = manifest.outputs.clone();
    for (path, o) in paths.iter().zip(outs) {
        if !path.exists() {
            problems.push(format!("{} was reported but is not on disk", o.name));
        } else if o.bytes == 0 {
            problems.push(format!("{} is empty", o.name));
        } else if !promised.accepts_file(&o.mime, &o.ext) {
            problems.push(format!(
                "{} is {} ({}), the manifest promises {}",
                o.name,
                o.mime,
                if o.ext.is_empty() { "?" } else { &o.ext },
                manifest.outputs.join(", ")
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_smaller_file_may_not_come_out_bigger() {
        assert!(check_size("media.compress", 10_000, 68_000).len() == 1);
        assert!(check_size("media.to_mono", 395_000, 738_000).len() == 1);
        // Within the room for container overhead, and for modules that do not shrink.
        assert!(check_size("media.compress", 10_000, 10_100).is_empty());
        assert!(check_size("image.convert", 10_000, 68_000).is_empty());
    }
}
