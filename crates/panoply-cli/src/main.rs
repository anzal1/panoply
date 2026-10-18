//! The `panoply` command line. Short, plain output.

mod disk_cmd;
mod mcp_cmd;
mod models_cmd;
mod vision_cmd;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
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
    /// Say yes to every model or engine download. Each yes is remembered per item. Without it
    /// Panoply asks first, and refuses when it cannot ask
    #[arg(long, global = true)]
    yes: bool,
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
    /// Archives: extract (ZIP, TAR, TAR.GZ, TAR.ZST, GZ) and create (ZIP, TAR.GZ, TAR.ZST)
    Archive {
        #[command(subcommand)]
        tool: ArchiveTool,
    },
    /// The same as `panoply archive extract`, under the module id
    #[command(name = "archive.extract", hide = true)]
    ArchiveExtractId(ExtractArgs),
    /// The same as `panoply archive create`, under the module id
    #[command(name = "archive.create", hide = true)]
    ArchiveCreateId(CreateArgs),
    /// PDF tools: merge, split, pages, compress, from, images, ocr
    Pdf {
        #[command(subcommand)]
        tool: PdfTool,
    },
    /// Image tools: ocr
    Image {
        #[command(subcommand)]
        tool: ImageTool,
    },
    /// Video tools: convert, trim, audio, and the rest of the video set
    Video {
        #[command(subcommand)]
        tool: VideoTool,
    },
    /// Audio tools: convert, trim, and the rest of the audio set
    Audio {
        #[command(subcommand)]
        tool: AudioTool,
    },
    /// Share files with other devices: list, send, receive, qr
    Share {
        #[command(subcommand)]
        tool: panoply_share::cli::ShareCommand,
    },
    /// ID and form photos: passport, sheet, signature
    Id {
        #[command(subcommand)]
        tool: IdTool,
    },
    /// Image AI: cutout, erase, unmark, upscale
    #[command(flatten)]
    Vision(vision_cmd::VisionCmd),
    /// The models and engines Panoply may download: list, fetch, allow, remove
    Models {
        #[command(subcommand)]
        cmd: Option<models_cmd::ModelsCmd>,
    },
    /// What is using the space under a folder: panoply disk ~/Downloads [--top 15] [--files 10] [--depth 2] [--json]
    Disk(disk_cmd::DiskArgs),
    /// Serve every tool to AI agents (Claude Code, Claude Desktop, Cursor) over MCP on stdio. See docs/MCP.md
    Mcp,
    /// Print the version
    Version,
}

#[derive(clap::Args)]
struct ExtractArgs {
    /// Archives to unpack, each into a new folder next to it
    #[arg(required = true)]
    files: Vec<PathBuf>,
    /// Stop when the unpacked size is more than this many times the archive
    #[arg(long, value_name = "N")]
    max_ratio: Option<u64>,
    /// Stop when the archive unpacks to more than this many GiB
    #[arg(long, value_name = "GIB")]
    max_size_gib: Option<f64>,
    /// Put the new folders in this folder instead of next to each archive
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
}

#[derive(clap::Args)]
struct CreateArgs {
    /// Files and folders to pack
    #[arg(required = true)]
    files: Vec<PathBuf>,
    /// zip, tar.gz or tar.zst
    #[arg(long, default_value = "zip")]
    format: String,
    /// fast, balanced or small
    #[arg(long)]
    level: Option<String>,
    /// Put the archive in this folder instead of next to the first file
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
}

#[derive(Subcommand)]
enum ArchiveTool {
    /// Unpack into a new folder: panoply archive extract photos.zip
    Extract(ExtractArgs),
    /// Pack files and folders: panoply archive create project --format tar.zst
    Create(CreateArgs),
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
    /// Make a scan searchable: panoply pdf ocr in.pdf --lang hi,en
    Ocr {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Languages, for example hi,en (default: automatic)
        #[arg(long, alias = "languages")]
        lang: Option<String>,
        /// Also write the recognized text as name.txt
        #[arg(long)]
        text_file: bool,
        /// Resolution pages are read at, 150 to 600
        #[arg(long)]
        dpi: Option<f64>,
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

#[derive(Subcommand)]
enum ImageTool {
    /// Copy the text out of pictures: panoply image ocr a.png --lang hi,en
    Ocr {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Languages, for example hi,en (default: automatic)
        #[arg(long, alias = "languages")]
        lang: Option<String>,
        /// Markdown instead of plain text
        #[arg(long)]
        markdown: bool,
        /// One line of text per line of the picture
        #[arg(long)]
        keep_line_breaks: bool,
        /// Put the text on the clipboard
        #[arg(long)]
        copy: bool,
        /// Also write name.txt (or name.md) next to the picture
        #[arg(long)]
        save: bool,
        /// Write results into this folder instead of next to each original
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },
}

/// Run a built-in module. Says first when its manifest declares network access.
/// Progress lines on stderr. Slow jobs only (after the 300 ms budget), and one line per ten percent
/// or per new step, so a log or a pipe gets ten lines for a picture instead of seventy.
#[derive(Clone)]
pub(crate) struct Progress(Arc<Mutex<ProgressState>>);

struct ProgressState {
    started: Instant,
    last_bucket: i32,
    last_label: String,
}

impl Progress {
    pub(crate) fn new() -> Self {
        Progress(Arc::new(Mutex::new(ProgressState {
            started: Instant::now(),
            last_bucket: -1,
            last_label: String::new(),
        })))
    }

    pub(crate) fn show(&self, fraction: f32, label: &str) {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if s.started.elapsed() <= Duration::from_millis(panoply_core::budgets::PROGRESS_AFTER_MS) {
            return;
        }
        let pct = (fraction * 100.0).round() as i32;
        let bucket = pct / 10;
        if bucket != s.last_bucket || label != s.last_label {
            s.last_bucket = bucket;
            s.last_label = label.to_string();
            eprintln!("{pct:>3}%  {label}");
        }
    }
}

pub(crate) fn run_builtin(
    spec: &JobSpec,
    out: Option<&Path>,
    cancel: CancelToken,
    sink: impl Fn(JobEvent) + Send + Sync + Clone + 'static,
) -> Result<panoply_host::JobOutcome, ModuleError> {
    let registry = Registry::with_builtins();
    models_cmd::show_network_notice(&registry, &spec.module_id);
    registry.run_job(spec, out, cancel, sink)
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    models_cmd::install(cli.yes);
    match cli.command {
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
        Command::Archive { tool } => archive(tool),
        Command::ArchiveExtractId(a) => archive(ArchiveTool::Extract(a)),
        Command::ArchiveCreateId(a) => archive(ArchiveTool::Create(a)),
        Command::Pdf { tool } => pdf(tool),
        Command::Image { tool } => image(tool),
        Command::Video { tool } => video(tool),
        Command::Audio { tool } => audio(tool),
        Command::Share { tool } => panoply_share::cli::run(tool),
        Command::Id { tool } => id_cmd(tool),
        Command::Vision(cmd) => vision_cmd::run(cmd),
        Command::Models { cmd } => models_cmd::run(cmd),
        Command::Disk(args) => disk_cmd::run(args),
        Command::Mcp => mcp_cmd::run(),
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
        // The source tree this binary was built from: for `cargo run` while developing. A release
        // build must not look there (on the CI runner that folder exists and holds the examples), and
        // PANOPLY_NO_SOURCE_TREE=1 turns it off in a debug build too (the tests that check what a
        // shipped folder holds).
        (cfg!(debug_assertions) && std::env::var_os("PANOPLY_NO_SOURCE_TREE").is_none()).then(
            || {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../modules")
                    .join(name)
            },
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
    // Community modules: the folders the app scans (the user folder, the folder shipped with the
    // program) and the examples in a source checkout. A module found twice is listed once.
    let mut dirs = panoply_core::modules::search_dirs();
    dirs.extend(examples_dir("examples-wasm"));
    let mut listed: Vec<String> = Vec::new();
    {
        let mut paths: Vec<PathBuf> = dirs
            .iter()
            .flat_map(|d| panoply_core::modules::module_folders(d))
            .map(|d| d.join("module.toml"))
            .collect();
        paths.dedup();
        for path in paths {
            match Manifest::from_file(&path) {
                Ok(m) if listed.contains(&m.module.id) => {}
                Ok(m) => {
                    listed.push(m.module.id.clone());
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
    let text = module.to_string_lossy();
    let looks_like_an_id = !text.contains(['/', '\\']) && !text.ends_with(".toml");
    if builtin_id(module).is_none() && !module.exists() && looks_like_an_id {
        eprintln!(
            "There is no tool called {}. Run `panoply modules` to see the list.",
            module.display()
        );
        return ExitCode::from(2);
    }
    if builtin_id(module).is_some() && files.is_empty() {
        eprintln!(
            "Give {} at least one file, for example: panoply run {} photo.jpg",
            module.display(),
            module.display()
        );
        return ExitCode::from(2);
    }
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
        let progress = crate::Progress::new();
        models_cmd::show_network_notice(&registry, &spec.module_id);
        registry.run_job(&spec, out, CancelToken::new(), move |event| {
            if let JobEvent::Progress { fraction, label } = event {
                progress.show(fraction, &label);
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
    match run_builtin(&spec, None, CancelToken::new(), |_| {}) {
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

/// `panoply archive extract|create`: runs `archive.extract` or `archive.create` with progress on
/// standard error and the report lines on standard output.
fn archive(tool: ArchiveTool) -> ExitCode {
    use serde_json::{Map, json};
    let mut o = Map::new();
    let (id, files, out) = match tool {
        ArchiveTool::Extract(a) => {
            if let Some(r) = a.max_ratio {
                o.insert("max_ratio".into(), json!(r));
            }
            if let Some(g) = a.max_size_gib {
                o.insert("max_size_gib".into(), json!(g));
            }
            ("archive.extract", a.files, a.out)
        }
        ArchiveTool::Create(a) => {
            o.insert("format".into(), json!(a.format));
            if let Some(l) = a.level {
                o.insert("level".into(), json!(l));
            }
            ("archive.create", a.files, a.out)
        }
    };
    let spec = JobSpec {
        module_id: id.into(),
        inputs: files,
        options: serde_json::Value::Object(o),
    };
    let progress = Progress::new();
    report(run_builtin(
        &spec,
        out.as_deref(),
        CancelToken::new(),
        move |event| {
            if let JobEvent::Progress { fraction, label } = event {
                progress.show(fraction, &label);
            }
        },
    ))
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
        PdfTool::Ocr {
            files,
            lang,
            text_file,
            dpi,
            out,
        } => {
            if let Some(l) = lang {
                put("languages", json!(l));
            }
            put("text_file", json!(text_file));
            if let Some(d) = dpi {
                put("dpi", json!(d));
            }
            ("pdf.ocr", files, out)
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
    let progress = crate::Progress::new();
    report(run_builtin(
        &spec,
        out.out.as_deref(),
        CancelToken::new(),
        move |event| {
            if let JobEvent::Progress { fraction, label } = event {
                progress.show(fraction, &label);
            }
        },
    ))
}

/// `panoply image ocr`: the recognized text goes to standard output (so it pipes), and the
/// result card, the last note, to standard error.
fn image(tool: ImageTool) -> ExitCode {
    use serde_json::{Map, json};
    let ImageTool::Ocr {
        files,
        lang,
        markdown,
        keep_line_breaks,
        copy,
        save,
        out,
    } = tool;
    let mut o = Map::new();
    if let Some(l) = lang {
        o.insert("languages".into(), json!(l));
    }
    if markdown {
        o.insert("format".into(), json!("markdown"));
    }
    o.insert("keep_line_breaks".into(), json!(keep_line_breaks));
    o.insert("copy".into(), json!(copy));
    o.insert("save".into(), json!(save));
    let spec = JobSpec {
        module_id: "image.to_text".into(),
        inputs: files,
        options: serde_json::Value::Object(o),
    };
    let progress = crate::Progress::new();
    let result = run_builtin(&spec, out.as_deref(), CancelToken::new(), move |event| {
        if let JobEvent::Progress { fraction, label } = event {
            progress.show(fraction, &label);
        }
    });
    match result {
        Ok(o) => {
            let n = o.notes.len();
            for (i, line) in o.notes.iter().enumerate() {
                if i + 1 == n {
                    eprintln!("{line}");
                } else {
                    println!("{line}");
                }
            }
            for path in o.outputs {
                eprintln!("wrote {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => report(Err(e)),
    }
}

/// Where results go, shared by the video and audio tools.
#[derive(clap::Args)]
struct MediaOut {
    /// Write results into this folder instead of next to each original
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
}

#[derive(Subcommand)]
enum VideoTool {
    /// Turn a video into another container or codec: panoply video convert in.mov --to mp4
    Convert {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// mp4, mov, mkv, webm, avi, hevc or av1 (or an audio format such as mp3)
        #[arg(long, default_value = "mp4")]
        to: String,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Keep or remove parts: panoply video trim in.mp4 --keep 0:12-1:40 [--keyframes]
    Trim {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Parts to keep, for example 0:12-1:40 or 0:12-1:40,2:00-2:30
        #[arg(long)]
        keep: Option<String>,
        /// Parts to cut out instead
        #[arg(long)]
        remove: Option<String>,
        /// Copy everything and start each cut at the nearest earlier keyframe
        #[arg(long)]
        keyframes: bool,
        /// One file per kept part
        #[arg(long)]
        each: bool,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Make a video small enough to send: panoply video compress in.mp4 --under 25mb
    Compress {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Aim for at most this size, for example 25mb
        #[arg(long)]
        under: Option<String>,
        /// A service's upload limit: whatsapp, discord, discord_nitro_basic, discord_nitro, gmail, slack, telegram, x
        #[arg(long)]
        preset: Option<String>,
        /// Without a size: 1 to 100, 50 is the smallest that still looks good
        #[arg(long)]
        quality: Option<f64>,
        /// auto, h264, hevc, av1 or vp9
        #[arg(long)]
        codec: Option<String>,
        /// Nothing taller than this on the shorter side
        #[arg(long)]
        max_height: Option<u32>,
        /// Convert HDR to standard range
        #[arg(long)]
        sdr: bool,
        /// Do not measure the quality afterwards
        #[arg(long)]
        no_measure: bool,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Everything about a video, and what is wrong with it: panoply video info in.mp4 [--json]
    Info {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Print JSON
        #[arg(long)]
        json: bool,
        /// Also measure the loudness (decodes the sound once)
        #[arg(long)]
        deep: bool,
    },
    /// Fix a video that will not play: panoply video repair broken.mp4 [--reference ok.mp4]
    Repair {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// A working video from the same camera or app
        #[arg(long)]
        reference: Option<PathBuf>,
        /// auto, remux or rebuild
        #[arg(long)]
        method: Option<String>,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Make an HDR video look right everywhere: panoply video sdr in.mov
    Sdr {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// bt2390, hable or mobius
        #[arg(long)]
        curve: Option<String>,
        /// Source peak brightness in nits (0 reads it from the file)
        #[arg(long)]
        peak: Option<f64>,
        /// h264 or hevc
        #[arg(long)]
        codec: Option<String>,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Take out, drop or relabel tracks: panoply video streams in.mkv --extract 2 --drop 5
    Streams {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Stream numbers to leave out
        #[arg(long)]
        drop: Option<String>,
        /// Stream numbers to write out as files
        #[arg(long)]
        extract: Option<String>,
        /// A stream number to make the default of its kind
        #[arg(long)]
        default: Option<String>,
        /// Languages, for example 1=eng,2=fra
        #[arg(long)]
        language: Option<String>,
        /// Write MKV, which holds every kind of stream
        #[arg(long)]
        mkv: bool,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Pull the sound out: panoply video audio in.mp4 [--format mp3] [--track 2]
    Audio {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// copy (default), mp3, m4a, aac, wav, flac, ogg or opus
        #[arg(long)]
        format: Option<String>,
        /// first, a number, or all
        #[arg(long)]
        track: Option<String>,
        /// Bit rate in kb/s for lossy formats
        #[arg(long)]
        bitrate: Option<f64>,
        #[command(flatten)]
        out: MediaOut,
    },
}

#[derive(Subcommand)]
enum AudioTool {
    /// Turn audio into another format: panoply audio convert in.wav --to mp3
    Convert {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// mp3, m4a, aac, wav, flac, ogg or opus
        #[arg(long, default_value = "mp3")]
        to: String,
        /// For a video with several sound tracks: first, a number, or all
        #[arg(long)]
        track: Option<String>,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Take background noise out: panoply audio denoise in.wav --strength normal
    Denoise {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// light, normal or strong
        #[arg(long)]
        strength: Option<String>,
        /// first, a number, or all
        #[arg(long)]
        track: Option<String>,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Set the loudness: panoply audio normalize in.mp3 --target -16
    Normalize {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Integrated loudness target in LUFS, for example -16
        #[arg(long, allow_negative_numbers = true)]
        target: Option<f64>,
        /// A named standard: ebu-r128, atsc-a85, apple-podcasts, spotify, netflix, podcast-general, youtube
        #[arg(long)]
        preset: Option<String>,
        /// True peak ceiling in dBTP, for example -1
        #[arg(long, allow_negative_numbers = true)]
        true_peak: Option<f64>,
        /// loudness, peak or tags (ReplayGain tags only)
        #[arg(long)]
        mode: Option<String>,
        /// first, a number, or all
        #[arg(long)]
        track: Option<String>,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Mono, swap, split or merge channels: panoply audio channels in.wav --mono
    Channels {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Mix down to mono (looks at the channels first)
        #[arg(long)]
        mono: bool,
        /// Keep only the left channel
        #[arg(long)]
        left: bool,
        /// Keep only the right channel
        #[arg(long)]
        right: bool,
        /// Swap left and right
        #[arg(long)]
        swap: bool,
        /// Write the two channels as two mono files
        #[arg(long)]
        split: bool,
        /// Merge exactly two mono files into stereo
        #[arg(long)]
        merge: bool,
        /// Copy a mono file to both sides
        #[arg(long)]
        stereo: bool,
        #[command(flatten)]
        out: MediaOut,
    },
    /// Everything about an audio file: panoply audio info in.mp3 [--json]
    Info {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Print JSON
        #[arg(long)]
        json: bool,
        /// Also measure the loudness
        #[arg(long)]
        deep: bool,
    },
    /// Keep or remove parts: panoply audio trim in.mp3 --keep 0:10-1:40
    Trim {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Parts to keep, for example 0:10-1:40
        #[arg(long)]
        keep: Option<String>,
        /// Parts to cut out instead
        #[arg(long)]
        remove: Option<String>,
        /// Cut to the sample (re-encodes lossy audio once) instead of the nearest codec frame
        #[arg(long)]
        exact: bool,
        /// One file per kept part
        #[arg(long)]
        each: bool,
        #[command(flatten)]
        out: MediaOut,
    },
}

fn media_run(
    id: &str,
    files: Vec<PathBuf>,
    o: serde_json::Map<String, serde_json::Value>,
    out: MediaOut,
) -> ExitCode {
    let spec = JobSpec {
        module_id: id.into(),
        inputs: files,
        options: serde_json::Value::Object(o),
    };
    let progress = crate::Progress::new();
    report(run_builtin(
        &spec,
        out.out.as_deref(),
        CancelToken::new(),
        move |event| {
            if let JobEvent::Progress { fraction, label } = event {
                progress.show(fraction, &label);
            }
        },
    ))
}

fn video(tool: VideoTool) -> ExitCode {
    use serde_json::{Map, Value, json};
    let mut o = Map::new();
    let mut put = |k: &str, v: Value| {
        o.insert(k.to_string(), v);
    };
    let (id, files, out) = match tool {
        VideoTool::Convert { files, to, out } => {
            put("to", json!(to));
            ("media.convert", files, out)
        }
        VideoTool::Trim {
            files,
            keep,
            remove,
            keyframes,
            each,
            out,
        } => {
            if let Some(k) = keep {
                put("keep", json!(k));
            }
            if let Some(r) = remove {
                put("remove", json!(r));
            }
            if keyframes {
                put("precision", json!("keyframes"));
            }
            if each {
                put("output", json!("each"));
            }
            ("media.trim", files, out)
        }
        VideoTool::Compress {
            files,
            under,
            preset,
            quality,
            codec,
            max_height,
            sdr,
            no_measure,
            out,
        } => {
            if let Some(u) = under {
                put("under", json!(u));
            }
            if let Some(p) = preset {
                put("preset", json!(p));
            }
            if let Some(q) = quality {
                put("quality", json!(q));
            }
            if let Some(c) = codec {
                put("codec", json!(c));
            }
            if let Some(h) = max_height {
                put("max_height", json!(h));
            }
            if sdr {
                put("hdr", json!("sdr"));
            }
            if no_measure {
                put("measure_quality", json!(false));
            }
            ("media.compress", files, out)
        }
        VideoTool::Info { files, json, deep } => {
            put("json", json!(json));
            put("deep", json!(deep));
            ("media.info", files, MediaOut { out: None })
        }
        VideoTool::Repair {
            mut files,
            reference,
            method,
            out,
        } => {
            if let Some(r) = reference {
                // The reference is read like the broken file, so it is chosen as a file too.
                put("reference", json!(r.to_string_lossy()));
                files.push(r);
            }
            if let Some(m) = method {
                put("method", json!(m));
            }
            ("media.repair", files, out)
        }
        VideoTool::Sdr {
            files,
            curve,
            peak,
            codec,
            out,
        } => {
            if let Some(c) = curve {
                put("curve", json!(c));
            }
            if let Some(p) = peak {
                put("peak_nits", json!(p));
            }
            if let Some(c) = codec {
                put("codec", json!(c));
            }
            ("media.hdr_to_sdr", files, out)
        }
        VideoTool::Streams {
            files,
            drop,
            extract,
            default,
            language,
            mkv,
            out,
        } => {
            for (k, v) in [
                ("drop", drop),
                ("extract", extract),
                ("default", default),
                ("language", language),
            ] {
                if let Some(v) = v {
                    put(k, json!(v));
                }
            }
            if mkv {
                put("container", json!("mkv"));
            }
            ("media.streams", files, out)
        }
        VideoTool::Audio {
            files,
            format,
            track,
            bitrate,
            out,
        } => {
            if let Some(f) = format {
                put("format", json!(f));
            }
            if let Some(t) = track {
                put("track", json!(t));
            }
            if let Some(b) = bitrate {
                put("bitrate", json!(b));
            }
            ("media.extract_audio", files, out)
        }
    };
    media_run(id, files, o, out)
}

fn audio(tool: AudioTool) -> ExitCode {
    use serde_json::{Map, Value, json};
    let mut o = Map::new();
    let mut put = |k: &str, v: Value| {
        o.insert(k.to_string(), v);
    };
    let (id, files, out) = match tool {
        AudioTool::Convert {
            files,
            to,
            track,
            out,
        } => {
            put("to", json!(to));
            if let Some(t) = track {
                put("track", json!(t));
            }
            ("media.convert", files, out)
        }
        AudioTool::Denoise {
            files,
            strength,
            track,
            out,
        } => {
            if let Some(s) = strength {
                put("strength", json!(s));
            }
            if let Some(t) = track {
                put("track", json!(t));
            }
            ("media.denoise", files, out)
        }
        AudioTool::Normalize {
            files,
            target,
            preset,
            true_peak,
            mode,
            track,
            out,
        } => {
            if let Some(t) = target {
                put("target", json!(t));
            }
            if let Some(p) = preset {
                put("preset", json!(p));
            }
            if let Some(t) = true_peak {
                put("true_peak", json!(t));
            }
            if let Some(m) = mode {
                put("mode", json!(m));
            }
            if let Some(t) = track {
                put("track", json!(t));
            }
            ("media.normalize", files, out)
        }
        AudioTool::Channels {
            files,
            mono,
            left,
            right,
            swap,
            split,
            merge,
            stereo,
            out,
        } => {
            let action = if merge {
                "merge"
            } else if split {
                "split"
            } else if swap {
                "swap"
            } else if left {
                "left"
            } else if right {
                "right"
            } else if stereo {
                "mono_to_stereo"
            } else {
                // Mono is the default job: it looks at the channels first.
                let _ = mono;
                "auto"
            };
            put("action", json!(action));
            ("media.to_mono", files, out)
        }
        AudioTool::Info { files, json, deep } => {
            put("json", json!(json));
            put("deep", json!(deep));
            ("media.info", files, MediaOut { out: None })
        }
        AudioTool::Trim {
            files,
            keep,
            remove,
            exact,
            each,
            out,
        } => {
            if let Some(k) = keep {
                put("keep", json!(k));
            }
            if let Some(r) = remove {
                put("remove", json!(r));
            }
            if exact {
                put("precision", json!("exact"));
            }
            if each {
                put("output", json!("each"));
            }
            ("media.trim", files, out)
        }
    };
    media_run(id, files, o, out)
}

/// Where results go, shared by the ID tools.
#[derive(clap::Args)]
struct IdOut {
    /// Write results into this folder instead of next to each original
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
}

#[derive(Subcommand)]
enum IdTool {
    /// A photo that passes the passport or visa rules: panoply id passport me.jpg --country IN
    Passport {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Country code or name (IN, US, UK, Canada, Schengen, Bangladesh, UAE, China, Australia) or a rule id such as us-visa
        #[arg(long)]
        country: Option<String>,
        /// passport or visa
        #[arg(long)]
        document: Option<String>,
        /// single (the digital file, default), sheet or both
        #[arg(long)]
        output: Option<String>,
        /// Sheet paper: 4x6, 5x7, 6x8, a4, a5, a6, letter
        #[arg(long)]
        paper: Option<String>,
        /// Copies on the sheet (default: fill it)
        #[arg(long)]
        copies: Option<u32>,
        /// Print resolution, turns millimetres into pixels
        #[arg(long)]
        dpi: Option<u32>,
        /// Background color such as #ffffff (default: the rule's)
        #[arg(long)]
        background: Option<String>,
        /// Leave the background as it is
        #[arg(long)]
        no_whiten: bool,
        /// Do not turn the head level
        #[arg(long)]
        no_fix_tilt: bool,
        /// Do not correct a dark or bright face
        #[arg(long)]
        no_fix_exposure: bool,
        /// Which face when there are several (1 is the leftmost)
        #[arg(long)]
        face: Option<u32>,
        /// auto, vision or yunet
        #[arg(long)]
        engine: Option<String>,
        /// both, 1000 or 1024: what counts as 1 KB
        #[arg(long)]
        kb_basis: Option<String>,
        /// Write nothing when a check fails
        #[arg(long)]
        strict: bool,
        /// Also write the checks as a JSON file
        #[arg(long)]
        report: bool,
        #[command(flatten)]
        out: IdOut,
    },
    /// Copies of a photo on one sheet: panoply id sheet photo.jpg --paper a4 --size 35x45
    Sheet {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// 4x6, 5x7, 6x8, a4, a5, a6, letter
        #[arg(long)]
        paper: Option<String>,
        /// Photo size in millimetres (35x45), or 3.5x4.5cm, 1.5x2in
        #[arg(long)]
        size: Option<String>,
        /// Take the size from a country rule (in-passport, us-visa, UK)
        #[arg(long)]
        rule: Option<String>,
        /// Copies (default: as many as fit)
        #[arg(long)]
        copies: Option<u32>,
        /// No cut marks
        #[arg(long)]
        no_cut_marks: bool,
        /// Do not turn photos to fit more
        #[arg(long)]
        no_rotate: bool,
        /// Resolution, default 300
        #[arg(long)]
        dpi: Option<u32>,
        /// jpg, pdf or png
        #[arg(long)]
        format: Option<String>,
        #[command(flatten)]
        out: IdOut,
    },
    /// A photo of a signature to a clean transparent PNG: panoply id signature sig.jpg --size 140x60
    Signature {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Size in pixels, for example 140x60
        #[arg(long)]
        size: Option<String>,
        /// A preset such as ssc-signature (pixels, size window, JPG or PNG)
        #[arg(long)]
        preset: Option<String>,
        /// keep, black or blue
        #[arg(long)]
        ink: Option<String>,
        /// transparent or white
        #[arg(long)]
        background: Option<String>,
        /// Take ruled lines out
        #[arg(long)]
        remove_lines: bool,
        /// both, 1000 or 1024: what counts as 1 KB
        #[arg(long)]
        kb_basis: Option<String>,
        #[command(flatten)]
        out: IdOut,
    },
}

fn id_cmd(tool: IdTool) -> ExitCode {
    use serde_json::{Map, Value, json};
    let mut o = Map::new();
    let mut put = |k: &str, v: Value| {
        o.insert(k.to_string(), v);
    };
    let (id, files, out) = match tool {
        IdTool::Passport {
            files,
            country,
            document,
            output,
            paper,
            copies,
            dpi,
            background,
            no_whiten,
            no_fix_tilt,
            no_fix_exposure,
            face,
            engine,
            kb_basis,
            strict,
            report,
            out,
        } => {
            for (k, v) in [
                ("country", country),
                ("document", document),
                ("output", output),
                ("paper", paper),
                ("background", background),
                ("engine", engine),
                ("kb_basis", kb_basis),
            ] {
                if let Some(v) = v {
                    put(k, json!(v));
                }
            }
            for (k, v) in [("copies", copies), ("dpi", dpi), ("face", face)] {
                if let Some(v) = v {
                    put(k, json!(v));
                }
            }
            put("whiten", json!(!no_whiten));
            put("fix_tilt", json!(!no_fix_tilt));
            put("fix_exposure", json!(!no_fix_exposure));
            put("strict", json!(strict));
            put("report", json!(report));
            ("id.passport_photo", files, out)
        }
        IdTool::Sheet {
            files,
            paper,
            size,
            rule,
            copies,
            no_cut_marks,
            no_rotate,
            dpi,
            format,
            out,
        } => {
            for (k, v) in [
                ("paper", paper),
                ("size", size),
                ("rule", rule),
                ("format", format),
            ] {
                if let Some(v) = v {
                    put(k, json!(v));
                }
            }
            for (k, v) in [("copies", copies), ("dpi", dpi)] {
                if let Some(v) = v {
                    put(k, json!(v));
                }
            }
            put("cut_marks", json!(!no_cut_marks));
            put("rotate", json!(!no_rotate));
            ("id.print_sheet", files, out)
        }
        IdTool::Signature {
            files,
            size,
            preset,
            ink,
            background,
            remove_lines,
            kb_basis,
            out,
        } => {
            if let Some(s) = size {
                let parsed = s.to_ascii_lowercase().split_once('x').and_then(|(w, h)| {
                    Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
                });
                let Some((w, h)) = parsed else {
                    eprintln!("\"{s}\" is not a size. Write it like 140x60.");
                    return ExitCode::FAILURE;
                };
                put("width", json!(w));
                put("height", json!(h));
            }
            for (k, v) in [
                ("preset", preset),
                ("ink", ink),
                ("background", background),
                ("kb_basis", kb_basis),
            ] {
                if let Some(v) = v {
                    put(k, json!(v));
                }
            }
            put("remove_lines", json!(remove_lines));
            ("id.signature_cleaner", files, out)
        }
    };
    let spec = JobSpec {
        module_id: id.into(),
        inputs: files,
        options: Value::Object(o),
    };
    let progress = crate::Progress::new();
    report(run_builtin(
        &spec,
        out.out.as_deref(),
        CancelToken::new(),
        move |event| {
            if let JobEvent::Progress { fraction, label } = event {
                progress.show(fraction, &label);
            }
        },
    ))
}
