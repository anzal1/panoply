//! `panoply mcp`: every installed tool as an MCP tool, over stdio, for AI agents.
//!
//! The protocol lives in `panoply-mcp`. This file supplies what a call does: it runs the module
//! through the same `Registry::run_job` path the command line and the app use, so a tool behaves
//! the same from an agent as from a terminal. The consent rules are the same too, except that
//! nothing can ask: stdin carries the protocol, so a download that was not allowed ahead of time
//! fails with a plain message that says how to allow it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use panoply_core::{CancelToken, JobEvent, JobSpec};
use panoply_host::{ModuleError, Registry};
use panoply_manifest::{Manifest, OptionType};
use panoply_mcp::server::{Backend, CallContext, ToolReply, UnknownTool};
use serde_json::{Map, Value};

/// Serve on stdin and stdout until the client closes the pipe.
pub fn run() -> ExitCode {
    // A prompt would read the protocol stream. Downloads need `panoply models allow` first, or
    // `panoply --yes mcp`.
    panoply_core::assets::set_asker(None);
    let backend = PanoplyBackend::load();
    eprintln!(
        "panoply mcp: serving {} tools on stdio",
        backend.tools().len()
    );
    let stdin = std::io::BufReader::new(std::io::stdin());
    match panoply_mcp::server::serve(
        stdin,
        std::io::stdout(),
        &backend,
        "panoply",
        env!("CARGO_PKG_VERSION"),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("panoply mcp: the connection ended: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The installed modules: the built-ins, then every community module that is compiled and found
/// in the folders the app scans.
struct PanoplyBackend {
    registry: Registry,
    /// MCP tool name to module id.
    names: BTreeMap<String, String>,
}

impl PanoplyBackend {
    fn load() -> Self {
        let mut registry = Registry::with_builtins();
        let mut dirs = panoply_core::modules::search_dirs();
        dirs.extend(crate::examples_dir("examples-wasm"));
        for dir in dirs {
            for folder in panoply_core::modules::module_folders(&dir) {
                match panoply_wasm::WasmModule::load(&folder) {
                    Ok(m) => {
                        // A built-in or an earlier folder wins over a second copy of the same id.
                        let _ = registry.register(Arc::new(m));
                    }
                    Err(e) => eprintln!("panoply mcp: skipped {}: {e}", folder.display()),
                }
            }
        }
        let mut names = BTreeMap::new();
        for m in registry.list() {
            let manifest = m.manifest();
            if !manifest.surfaces.mcp {
                continue;
            }
            let name = panoply_mcp::tool_name(&manifest.module.id);
            if let Some(first) = names.get(&name) {
                eprintln!(
                    "panoply mcp: skipped {}: its tool name {name} is already taken by {first}",
                    manifest.module.id
                );
                continue;
            }
            names.insert(name, manifest.module.id.clone());
        }
        Self { registry, names }
    }

    fn manifests(&self) -> impl Iterator<Item = &Manifest> {
        self.names
            .values()
            .filter_map(|id| self.registry.get(id))
            .map(|m| m.manifest())
    }
}

impl Backend for PanoplyBackend {
    fn tools(&self) -> Vec<Value> {
        panoply_mcp::tools(self.manifests())
    }

    fn call(
        &self,
        name: &str,
        arguments: &Map<String, Value>,
        ctx: &CallContext,
    ) -> Result<ToolReply, UnknownTool> {
        let id = self.names.get(name).ok_or(UnknownTool)?;
        let module = self.registry.get(id).ok_or(UnknownTool)?;
        let (spec, out_dir) = match prepare(module.manifest(), id, arguments) {
            Ok(p) => p,
            Err(message) => return Ok(ToolReply::error(message)),
        };
        let cancel = CancelToken::new();
        let hook = cancel.clone();
        ctx.on_cancel(move || hook.cancel());
        crate::models_cmd::show_network_notice(&self.registry, id);
        let progress = ctx.progress_sink();
        let result = self
            .registry
            .run_job(&spec, out_dir.as_deref(), cancel, move |event| {
                if let JobEvent::Progress { fraction, label } = event {
                    progress(fraction, &label);
                }
            });
        Ok(match result {
            Ok(done) => reply_for(done),
            Err(ModuleError::Cancelled) => ToolReply::error("The job was cancelled."),
            Err(e) => ToolReply::error(format!("{} {}", e.plain_message(), e.next_step())),
        })
    }
}

/// Check the arguments of a call against the manifest and build the job. Every problem is a
/// plain sentence that says what to change.
fn prepare(
    manifest: &Manifest,
    id: &str,
    args: &Map<String, Value>,
) -> Result<(JobSpec, Option<PathBuf>), String> {
    let mut inputs: Vec<PathBuf> = Vec::new();
    match args.get("files") {
        Some(Value::Array(list)) if !manifest.accepts.is_empty() => {
            for item in list {
                let text = item
                    .as_str()
                    .ok_or("files must be a list of absolute path strings.")?;
                inputs.push(existing_file(text, "file")?);
            }
        }
        Some(Value::Null) | None => {}
        Some(_) if manifest.accepts.is_empty() => {
            return Err(format!("{} does not take files.", manifest.module.name));
        }
        Some(_) => return Err("files must be a list of absolute path strings.".into()),
    }
    if !manifest.accepts.is_empty() && inputs.is_empty() {
        return Err(format!(
            "Give {} at least one file in \"files\", as absolute paths.",
            manifest.module.name
        ));
    }

    let out_dir = match args.get("output_dir") {
        Some(Value::String(text)) if !manifest.outputs.is_empty() => {
            let dir = Path::new(text);
            if !dir.is_absolute() {
                return Err(format!(
                    "output_dir must be an absolute path, and \"{text}\" is not."
                ));
            }
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("Could not use the output folder {text}: {e}."))?;
            Some(dir.to_path_buf())
        }
        Some(Value::Null) | None => None,
        Some(Value::String(_)) => return Err("This tool does not write files.".into()),
        Some(_) => return Err("output_dir must be a path string.".into()),
    };

    let mut options = Map::new();
    for (key, value) in args {
        if key == "files" || key == "output_dir" {
            continue;
        }
        let Some(spec) = manifest.options.get(key) else {
            let known: Vec<&str> = manifest.options.keys().map(String::as_str).collect();
            return Err(format!(
                "{id} has no option called {key}. Its options are: {}.",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ));
        };
        let value = match (spec.kind, value) {
            // Agents often send a choice such as 1024 as a number. The manifest lists text.
            (OptionType::Choice | OptionType::Text, Value::Number(n)) => {
                Value::String(n.to_string())
            }
            (OptionType::Choice | OptionType::Text, Value::Bool(b)) => Value::String(b.to_string()),
            (OptionType::File, Value::String(text)) => {
                // Read like the main files, so it is chosen as a file too.
                let path = existing_file(text, key)?;
                if !inputs.contains(&path) {
                    inputs.push(path);
                }
                value.clone()
            }
            _ => value.clone(),
        };
        options.insert(key.clone(), value);
    }

    Ok((
        JobSpec {
            module_id: id.to_string(),
            inputs,
            options: Value::Object(options),
        },
        out_dir,
    ))
}

/// An absolute path to a file that exists. Refuses anything else, so a typo never turns into a
/// job on the wrong file.
fn existing_file(text: &str, what: &str) -> Result<PathBuf, String> {
    let path = Path::new(text);
    if !path.is_absolute() {
        return Err(format!(
            "The {what} path \"{text}\" is not absolute. Give the full path, such as /Users/me/report.pdf."
        ));
    }
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(path.to_path_buf()),
        Ok(_) => Err(format!(
            "{text} is a folder, not a file. Give the paths of the files inside it."
        )),
        Err(_) => Err(format!(
            "The file {text} does not exist. Check the path and try again."
        )),
    }
}

fn reply_for(done: panoply_host::JobOutcome) -> ToolReply {
    let outputs: Vec<String> = done
        .outputs
        .iter()
        .map(|p| {
            std::path::absolute(p)
                .unwrap_or_else(|_| p.clone())
                .display()
                .to_string()
        })
        .collect();
    let mut lines = Vec::new();
    match outputs.len() {
        0 => {}
        1 => lines.push(format!("Wrote 1 file: {}", outputs[0])),
        n => {
            lines.push(format!("Wrote {n} files:"));
            lines.extend(outputs.iter().cloned());
        }
    }
    lines.extend(done.notes.iter().cloned());
    if lines.is_empty() {
        lines.push("Done. Nothing was written.".into());
    }
    ToolReply {
        is_error: false,
        summary: lines.join("\n"),
        outputs,
        notes: done.notes,
    }
}
