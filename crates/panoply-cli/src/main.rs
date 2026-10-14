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
    /// Run a built-in module by id (panoply run image.compress photo.jpg), or a community
    /// module: a module.toml with its compiled WebAssembly next to it
    Run {
        /// A built-in module id such as image.compress, or a community module's module.toml (or the folder that holds it)
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
    /// PDF tools: merge, split, pages, compress, from, images
    Pdf {
        #[command(subcommand)]
        tool: PdfTool,
    },
    /// Share files with other devices: list, send, receive, qr
    Share {
        #[command(subcommand)]
        tool: panoply_share::cli::ShareCommand,
    },
    /// Print the version
    Version,
}

/// Where results go, shared by the PDF tools.
#[derive(clap::Args)]
struct PdfOut {
    /// Write results into this folder instead of next to each original
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
}

#[derive(Subcommand)]
enum PdfTool {
    /// Put PDFs in one file: panoply pdf merge a.pdf b.pdf
    Merge {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Interleave two files (front scans, back scans)
        #[arg(long)]
        interleave: bool,
        /// With --interleave, take the second file's pages in reverse
        #[arg(long)]
        reverse_second: bool,
        /// Keep the order the files were typed in instead of natural name order
        #[arg(long)]
        given_order: bool,
        /// Do not add a bookmark per file
        #[arg(long)]
        no_bookmarks: bool,
        #[command(flatten)]
        out: PdfOut,
    },
    /// Cut a PDF into parts: panoply pdf split in.pdf --every 10
    Split {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Pages per part
        #[arg(long)]
        every: Option<u32>,
        /// One part per range, for example "1-3, 7, 9-"
        #[arg(long)]
        ranges: Option<String>,
        /// File names, with {name}, {n} and {pages}
        #[arg(long)]
        name_pattern: Option<String>,
        #[command(flatten)]
        out: PdfOut,
    },
    /// Keep, delete, rotate or reverse pages: panoply pdf pages in.pdf --keep 1-5,9 --rotate 90
    Pages {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Pages to keep, in the order wanted
        #[arg(long)]
        keep: Option<String>,
        /// Pages to delete
        #[arg(long)]
        delete: Option<String>,
        /// Degrees clockwise: 90, 180 or 270
        #[arg(long)]
        rotate: Option<String>,
        /// Only rotate these pages
        #[arg(long)]
        rotate_pages: Option<String>,
        /// Reverse the page order
        #[arg(long)]
        reverse: bool,
        #[command(flatten)]
        out: PdfOut,
    },
    /// Make PDFs smaller: panoply pdf compress in.pdf --under 2mb
    Compress {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Aim for at most this size, for example 2mb or 500kb
        #[arg(long)]
        under: Option<String>,
        /// A fixed picture quality from 1 to 100 instead of the smallest that looks good
        #[arg(long)]
        quality: Option<u32>,
        /// Turn colour pictures gray
        #[arg(long)]
        grayscale: bool,
        /// Highest picture resolution to keep, in DPI
        #[arg(long)]
        max_dpi: Option<u32>,
        /// Remove document information and XMP
        #[arg(long)]
        strip_metadata: bool,
        /// Remove embedded files
        #[arg(long)]
        strip_attachments: bool,
        /// Remove the form and what was typed into it
        #[arg(long)]
        strip_forms: bool,
        #[command(flatten)]
        out: PdfOut,
    },
    /// Make a PDF from pictures: panoply pdf from img1.jpg img2.jpg
    From {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// One PDF per picture instead of one for all
        #[arg(long)]
        separate: bool,
        /// auto, a4, letter or image
        #[arg(long)]
        page_size: Option<String>,
        /// Margin in millimetres
        #[arg(long)]
        margin: Option<f64>,
        /// Keep the order the files were typed in
        #[arg(long)]
        given_order: bool,
        #[command(flatten)]
        out: PdfOut,
    },
    /// Turn PDF pages into pictures: panoply pdf images in.pdf --dpi 150
    Images {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// png or jpg
        #[arg(long)]
        format: Option<String>,
        /// Resolution
        #[arg(long)]
        dpi: Option<f64>,
        /// JPG quality
        #[arg(long)]
        quality: Option<u32>,
        /// Pages, for example "1-3, 7"
        #[arg(long)]
        pages: Option<String>,
        /// Put the pictures in a ZIP
        #[arg(long)]
        zip: bool,
        /// Password for a protected PDF
        #[arg(long)]
        password: Option<String>,
        #[command(flatten)]
        out: PdfOut,
    },
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
        Command::Pdf { tool } => pdf(tool),
        Command::Share { tool } => panoply_share::cli::run(tool),
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

/// The id of a built-in module when `module` names one (`image.compress`) and no file or folder
/// of that name is in the way.
fn builtin_id(module: &Path) -> Option<String> {
    let id = module.to_str()?;
    if module.exists() || Registry::with_builtins().get(id).is_none() {
        return None;
    }
    Some(id.to_string())
}

fn run_module(module: &Path, files: Vec<PathBuf>, opts: &[String], out: Option<&Path>) -> ExitCode {
    let result = (|| {
        let (registry, module_id, options) = match builtin_id(module) {
            Some(id) => {
                let registry = Registry::with_builtins();
                let options = {
                    let m = registry.get(&id).expect("builtin_id checked the registry");
                    panoply_host::options::from_pairs(m.manifest(), opts)?
                };
                (registry, id, options)
            }
            None => {
                let wasm = WasmModule::load(module)?;
                let options = panoply_wasm::options_from_pairs(wasm.manifest(), opts)?;
                let id = wasm.manifest().module.id.clone();
                let mut registry = Registry::new();
                registry.register(Arc::new(wasm))?;
                (registry, id, options)
            }
        };
        let spec = JobSpec {
            module_id,
            inputs: files,
            options,
        };
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

/// "2mb", "500kb", "1.5gb" or a bare number of megabytes, as megabytes.
fn size_to_mb(text: &str) -> Option<f64> {
    let t = text.trim().to_ascii_lowercase();
    let (num, unit) = match t.find(|c: char| c.is_ascii_alphabetic()) {
        Some(i) => (&t[..i], &t[i..]),
        None => (t.as_str(), "mb"),
    };
    let n: f64 = num.trim().parse().ok()?;
    match unit.trim() {
        "kb" | "k" => Some(n / 1000.0),
        "mb" | "m" => Some(n),
        "gb" | "g" => Some(n * 1000.0),
        _ => None,
    }
}

fn pdf(tool: PdfTool) -> ExitCode {
    use serde_json::{Map, Value, json};
    let mut o = Map::new();
    let mut put = |k: &str, v: Value| {
        o.insert(k.to_string(), v);
    };
    let (id, files, out) = match tool {
        PdfTool::Merge {
            files,
            interleave,
            reverse_second,
            given_order,
            no_bookmarks,
            out,
        } => {
            put("interleave", json!(interleave));
            put("reverse_second", json!(reverse_second));
            if given_order || interleave {
                put("order", json!("given"));
            }
            put("bookmarks", json!(!no_bookmarks));
            ("pdf.merge", files, out)
        }
        PdfTool::Split {
            files,
            every,
            ranges,
            name_pattern,
            out,
        } => {
            if let Some(r) = ranges {
                put("mode", json!("ranges"));
                put("ranges", json!(r));
            } else if let Some(n) = every {
                put("every", json!(n));
            }
            if let Some(p) = name_pattern {
                put("name_pattern", json!(p));
            }
            ("pdf.split", files, out)
        }
        PdfTool::Pages {
            files,
            keep,
            delete,
            rotate,
            rotate_pages,
            reverse,
            out,
        } => {
            for (k, v) in [
                ("keep", keep),
                ("delete", delete),
                ("rotate", rotate),
                ("rotate_pages", rotate_pages),
            ] {
                if let Some(v) = v {
                    put(k, json!(v));
                }
            }
            put("reverse", json!(reverse));
            ("pdf.organize", files, out)
        }
        PdfTool::Compress {
            files,
            under,
            quality,
            grayscale,
            max_dpi,
            strip_metadata,
            strip_attachments,
            strip_forms,
            out,
        } => {
            if let Some(u) = under {
                let Some(mb) = size_to_mb(&u) else {
                    eprintln!("\"{u}\" is not a size. Try 2mb or 500kb.");
                    return ExitCode::FAILURE;
                };
                put("under_mb", json!(mb));
            } else if let Some(q) = quality {
                put("target", json!("quality"));
                put("quality", json!(q));
            }
            put("grayscale", json!(grayscale));
            put("strip_metadata", json!(strip_metadata));
            put("strip_attachments", json!(strip_attachments));
            put("strip_forms", json!(strip_forms));
            if let Some(d) = max_dpi {
                put("max_dpi", json!(d));
            }
            ("pdf.compress", files, out)
        }
        PdfTool::From {
            files,
            separate,
            page_size,
            margin,
            given_order,
            out,
        } => {
            put("merge", json!(!separate));
            if given_order {
                put("order", json!("given"));
            }
            if let Some(p) = page_size {
                put("page_size", json!(p));
            }
            if let Some(m) = margin {
                put("margin", json!(m));
            }
            ("pdf.images_to_pdf", files, out)
        }
        PdfTool::Images {
            files,
            format,
            dpi,
            quality,
            pages,
            zip,
            password,
            out,
        } => {
            if let Some(f) = format {
                put("format", json!(f));
            }
            if let Some(d) = dpi {
                put("dpi", json!(d));
            }
            if let Some(q) = quality {
                put("quality", json!(q));
            }
            if let Some(p) = pages {
                put("pages", json!(p));
            }
            put("zip", json!(zip));
            if let Some(p) = password {
                put("password", json!(p));
            }
            ("pdf.to_images", files, out)
        }
    };
    let spec = JobSpec {
        module_id: id.into(),
        inputs: files,
        options: Value::Object(o),
    };
    let started = Instant::now();
    let slow = Duration::from_millis(panoply_core::budgets::PROGRESS_AFTER_MS);
    report(Registry::with_builtins().run_job(
        &spec,
        out.out.as_deref(),
        CancelToken::new(),
        move |event| {
            if let JobEvent::Progress { fraction, label } = event
                && started.elapsed() > slow
            {
                eprintln!("{:>3.0}%  {label}", fraction * 100.0);
            }
        },
    ))
}
