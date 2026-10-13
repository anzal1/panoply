//! The `panoply` command line. Short, plain output.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use panoply_convert::Graph;
use panoply_core::{CancelToken, JobEvent, JobSpec, sniff};
use panoply_host::{Module, ModuleError, Registry};
use panoply_manifest::Manifest;
use panoply_wasm::WasmModule;

#[derive(Parser)]
#[command(name = "panoply", version, about = "On-device file utilities")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the formats Panoply understands
    Formats {
        /// Only this family, for example Audio
        #[arg(long)]
        family: Option<String>,
    },
    /// Show how one format converts to another
    Route {
        /// Source extension, for example docx
        from: String,
        /// Target extension, for example jpg
        to: String,
    },
    /// Identify a file from its bytes
    Sniff {
        /// File to look at
        file: PathBuf,
    },
    /// List built-in modules and the manifests in modules/examples
    Modules,
    /// SHA-256 of each file
    Hash {
        /// Files to hash
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Run a community module: a module.toml with its compiled WebAssembly next to it
    Run {
        /// The module's module.toml, or the folder that holds it
        module: PathBuf,
        /// Files to run it on
        files: Vec<PathBuf>,
        /// An option as key=value, repeatable. See the module's module.toml for the names
        #[arg(long = "opt", value_name = "KEY=VALUE")]
        opts: Vec<String>,
        /// Write outputs into this folder instead of next to each original
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
    /// Print the version
    Version,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Formats { family } => formats(family.as_deref()),
        Command::Route { from, to } => route(&from, &to),
        Command::Sniff { file } => sniff_cmd(&file),
        Command::Modules => modules(),
        Command::Hash { files } => hash(files),
        Command::Run {
            module,
            files,
            opts,
            out,
        } => run_module(&module, files, &opts, out.as_deref()),
        Command::Version => {
            println!("panoply {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
    }
}

fn formats(family: Option<&str>) -> ExitCode {
    let g = Graph::shared();
    let mut shown = 0;
    for f in g.formats() {
        if family.is_some_and(|want| !f.family.eq_ignore_ascii_case(want)) {
            continue;
        }
        let mode = match (f.read, f.write) {
            (true, true) => "rw",
            (true, false) => "r",
            (false, true) => "w",
            (false, false) => "-",
        };
        println!("{:<10}{:<20}{:<4}{}", f.ext, f.family, mode, f.name);
        shown += 1;
    }
    if shown == 0 {
        eprintln!(
            "No formats in family \"{}\". Families: {}.",
            family.unwrap_or(""),
            g.families().join(", ")
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn route(from: &str, to: &str) -> ExitCode {
    let g = Graph::shared();
    for ext in [from, to] {
        if g.format_by_ext(ext).is_none() {
            eprintln!("Unknown format \"{ext}\". Run `panoply formats` to see the list.");
            return ExitCode::FAILURE;
        }
    }
    match g.route(from, to) {
        Some(r) if r.is_model() => {
            println!("{}  (runs an on-device model)", r.chain());
            ExitCode::SUCCESS
        }
        Some(r) => {
            println!("{}", r.chain());
            ExitCode::SUCCESS
        }
        None => {
            println!("No route from {from} to {to}.");
            ExitCode::FAILURE
        }
    }
}

fn sniff_cmd(file: &Path) -> ExitCode {
    if !file.is_file() {
        eprintln!(
            "Could not find {}. Check the path and try again.",
            file.display()
        );
        return ExitCode::FAILURE;
    }
    let s = sniff(file);
    println!("mime    {}", s.mime);
    println!("ext     {}", if s.ext.is_empty() { "-" } else { &s.ext });
    println!("family  {}", s.family_hint.as_str());
    println!(
        "from    {}",
        if s.from_bytes {
            "file contents"
        } else {
            "file name"
        }
    );
    ExitCode::SUCCESS
}

/// Where example manifests live: `$PANOPLY_MODULES`, then `./modules/<name>`, then the
/// source tree this binary was built from.
fn examples_dir(name: &str) -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("PANOPLY_MODULES")
            .map(PathBuf::from)
            .filter(|_| name == "examples"),
        Some(PathBuf::from("modules").join(name)),
        Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../modules")
                .join(name),
        ),
    ];
    candidates.into_iter().flatten().find(|p| p.is_dir())
}

fn modules() -> ExitCode {
    let registry = Registry::with_builtins();
    let mut builtin_ids = Vec::new();
    for m in registry.list() {
        let info = &m.manifest().module;
        println!(
            "{:<22}{:<22}{}  built-in",
            info.id,
            info.name,
            info.pattern.letter()
        );
        builtin_ids.push(info.id.clone());
    }
    if let Some(dir) = examples_dir("examples") {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path().join("module.toml"))
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        for path in paths {
            match Manifest::from_file(&path) {
                Ok(m) if builtin_ids.contains(&m.module.id) => {}
                Ok(m) => {
                    let info = &m.module;
                    println!(
                        "{:<22}{:<22}{}  manifest only",
                        info.id,
                        info.name,
                        info.pattern.letter()
                    );
                }
                Err(e) => eprintln!("Skipped {}: {e}", path.display()),
            }
        }
    }
    if let Some(dir) = examples_dir("examples-wasm") {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path().join("module.toml"))
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        for path in paths {
            match Manifest::from_file(&path) {
                Ok(m) => {
                    let info = &m.module;
                    println!(
                        "{:<22}{:<22}{}  community (run with `panoply run {}`)",
                        info.id,
                        info.name,
                        info.pattern.letter(),
                        path.display()
                    );
                }
                Err(e) => eprintln!("Skipped {}: {e}", path.display()),
            }
        }
    }
    ExitCode::SUCCESS
}

fn run_module(module: &Path, files: Vec<PathBuf>, opts: &[String], out: Option<&Path>) -> ExitCode {
    let result = (|| {
        let wasm = WasmModule::load(module)?;
        let options = panoply_wasm::options_from_pairs(wasm.manifest(), opts)?;
        let spec = JobSpec {
            module_id: wasm.manifest().module.id.clone(),
            inputs: files,
            options,
        };
        let mut registry = Registry::new();
        registry.register(Arc::new(wasm))?;
        // Progress is for jobs that take a while: stay quiet under 300 ms.
        let started = Instant::now();
        let slow = Duration::from_millis(panoply_core::budgets::PROGRESS_AFTER_MS);
        registry.run_job(&spec, out, CancelToken::new(), move |event| {
            if let JobEvent::Progress { fraction, label } = event
                && started.elapsed() > slow
            {
                eprintln!("{:>3.0}%  {label}", fraction * 100.0);
            }
        })
    })();
    report(result)
}

fn report(result: Result<panoply_host::JobOutcome, ModuleError>) -> ExitCode {
    match result {
        Ok(out) => {
            for line in out.notes {
                println!("{line}");
            }
            for path in out.outputs {
                println!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e.plain_message());
            eprintln!("{}", e.next_step());
            ExitCode::FAILURE
        }
    }
}

fn hash(files: Vec<PathBuf>) -> ExitCode {
    let spec = JobSpec {
        module_id: "files.hash".into(),
        inputs: files,
        options: serde_json::Value::Null,
    };
    match Registry::with_builtins().run_job(&spec, None, CancelToken::new(), |_| {}) {
        Ok(out) => {
            for line in out.notes {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{}", e.plain_message());
            eprintln!("{}", e.next_step());
            ExitCode::FAILURE
        }
    }
}
