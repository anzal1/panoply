//! `panoply-bench`: the budget harness command line.
//!
//! ```text
//! panoply-bench check                         everything below, one budget table
//! panoply-bench run --kind pdf --instant      a module over a corpus slice
//! panoply-bench scale --sizes 1MiB,2GiB       memory against input size, sparse files
//! panoply-bench idle                          idle CPU and memory of the host
//! panoply-bench idle --pid 4242               an app shell that is already running
//! panoply-bench corpus --list                 what is in the corpus and what is on disk
//! panoply-bench budgets                       the FLOWS.md table and how each row is measured
//! ```
//!
//! Exit status: 0 when no budget failed, 1 when one did, 2 when the harness itself could not run.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand};
use panoply_bench::budget::{self, Check};
use panoply_bench::corpus::{Corpus, Kind, Slice, Tier};
use panoply_bench::idle::{self, Target, Window};
use panoply_bench::report::{self, FullReport, HostInfo};
use panoply_bench::run::{Input, RunOptions, RunReport};
use panoply_bench::runner::{self, Runner};
use panoply_bench::scale::{self, ScaleReport};
use panoply_bench::stats;
use panoply_bench::{BenchError, Result, measure, worker};

#[derive(Parser)]
#[command(
    name = "panoply-bench",
    version,
    about = "Budget harness: measure modules against the FLOWS.md budgets"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a module over a corpus slice and check memory, first preview and speed
    Run(RunCmd),
    /// Check that peak memory stays flat as a sparse input grows to gigabytes
    Scale(ScaleCmd),
    /// Idle CPU and memory of the host, a command, or running processes
    Idle(IdleCmd),
    /// Run, scale and idle together and print one budget table
    Check(CheckCmd),
    /// Show what is in the corpus and what is on disk
    Corpus(CorpusCmd),
    /// Print the FLOWS.md budget table and how this harness measures each row
    Budgets,
    /// Internal: run one job in this process and print a JSON report
    #[command(hide = true)]
    Worker {
        #[arg(long)]
        module: String,
        #[arg(long)]
        out_dir: PathBuf,
        inputs: Vec<PathBuf>,
    },
    /// Internal: load the host registry and wait on stdin
    #[command(hide = true, name = "worker-idle")]
    WorkerIdle,
}

/// Which module to run and how to start it.
#[derive(Args, Clone)]
struct ModuleOpts {
    /// Module id in the host registry
    #[arg(long, default_value = "files.hash")]
    module: String,
    /// How to start it: `worker` (host registry, any module) or `cli` (the panoply binary)
    #[arg(long, default_value = "worker")]
    runner: String,
    /// Path of the panoply binary for `--runner cli` (default: next to this binary)
    #[arg(long)]
    cli_bin: Option<PathBuf>,
    /// Arguments for `--runner cli`; {input} and {out_dir} are replaced per run
    #[arg(long, default_value = "hash {input}", allow_hyphen_values = true)]
    cli_args: String,
    /// Write the JSON report here, or `-` for stdout (then no text is printed)
    #[arg(long)]
    json: Option<String>,
}

/// Which corpus files to use.
#[derive(Args, Clone)]
struct SliceOpts {
    /// Corpus folder (default: tests/corpus, or $PANOPLY_CORPUS)
    #[arg(long)]
    corpus: Option<PathBuf>,
    /// Kinds: pdf, image, video, audio, office, or all (comma separated)
    #[arg(long, value_delimiter = ',')]
    kind: Vec<String>,
    /// Only files with this edge case tag, for example encrypted or hdr (repeatable)
    #[arg(long)]
    tag: Vec<String>,
    /// Only this corpus id (repeatable)
    #[arg(long)]
    id: Vec<String>,
    /// Largest tier to include: default, large or huge
    #[arg(long, default_value = "default")]
    tier: String,
    /// Skip files smaller than this many MB
    #[arg(long, default_value_t = 0.0)]
    min_mb: f64,
    /// Skip files larger than this many MB
    #[arg(long)]
    max_mb: Option<f64>,
    /// Run on these paths instead of the corpus
    #[arg(long, num_args = 1..)]
    files: Vec<PathBuf>,
    /// Use at most this many files
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Args, Clone)]
struct RunOpts {
    /// Runs per file (wall and CPU are medians, memory is the maximum)
    #[arg(long, default_value_t = 3)]
    runs: usize,
    /// The module is an instant transform: check the first preview budget on typical files
    #[arg(long)]
    instant: bool,
    /// Files up to this many MB are typical files for the first preview budget
    #[arg(long, default_value_t = 10.0)]
    typical_mb: f64,
    /// Native command to compare speed with, {input} marks the file, e.g. 'shasum -a 256 {input}'
    #[arg(long)]
    baseline: Option<String>,
    /// Allowed ratio of our wall time to the native tool's
    #[arg(long, default_value_t = 1.10)]
    speed_tolerance: f64,
    /// Only compare speed on files of at least this many MB
    #[arg(long, default_value_t = 4.0)]
    baseline_min_mb: f64,
    /// Print every file, not only the summary
    #[arg(long)]
    verbose: bool,
    /// Do not print per-file progress on stderr
    #[arg(long)]
    quiet: bool,
}

#[derive(Args, Clone)]
struct ScaleOpts {
    /// Sparse input sizes, comma separated (KiB, MiB, GiB are binary, KB, MB, GB decimal)
    #[arg(long, default_value = "1MiB,64MiB,512MiB,2GiB", value_delimiter = ',')]
    sizes: Vec<String>,
    /// Allowed spread between the largest and smallest peak, in MB
    #[arg(long, default_value_t = 8.0)]
    tolerance_mb: f64,
    /// Make the sparse files here instead of the system temp folder
    #[arg(long)]
    tmp_dir: Option<PathBuf>,
}

#[derive(Args, Clone)]
struct IdleOpts {
    /// Seconds to wait after start before sampling
    #[arg(long, default_value_t = 2.0)]
    settle_s: f64,
    /// Length of the sampled window in seconds
    #[arg(long, default_value_t = 10.0)]
    window_s: f64,
    /// Milliseconds between memory samples
    #[arg(long, default_value_t = 500)]
    interval_ms: u64,
}

#[derive(Args)]
struct RunCmd {
    #[command(flatten)]
    module: ModuleOpts,
    #[command(flatten)]
    slice: SliceOpts,
    #[command(flatten)]
    run: RunOpts,
}

#[derive(Args)]
struct ScaleCmd {
    #[command(flatten)]
    module: ModuleOpts,
    #[command(flatten)]
    scale: ScaleOpts,
    /// Runs per size
    #[arg(long, default_value_t = 1)]
    runs: usize,
}

#[derive(Args)]
struct IdleCmd {
    /// Sample a process that is already running (repeatable, for app shells and their helpers)
    #[arg(long)]
    pid: Vec<u32>,
    #[command(flatten)]
    idle: IdleOpts,
    /// Write the JSON report here, or `-` for stdout
    #[arg(long)]
    json: Option<String>,
    /// Start this command and sample it: `idle -- panoply mcp`
    #[arg(last = true)]
    cmd: Vec<String>,
}

#[derive(Args)]
struct CheckCmd {
    #[command(flatten)]
    module: ModuleOpts,
    #[command(flatten)]
    slice: SliceOpts,
    #[command(flatten)]
    run: RunOpts,
    #[command(flatten)]
    scale: ScaleOpts,
    #[command(flatten)]
    idle: IdleOpts,
}

#[derive(Args)]
struct CorpusCmd {
    /// Corpus folder (default: tests/corpus, or $PANOPLY_CORPUS)
    #[arg(long)]
    corpus: Option<PathBuf>,
    /// Only these kinds
    #[arg(long, value_delimiter = ',')]
    kind: Vec<String>,
    /// Only files with this tag
    #[arg(long)]
    tag: Vec<String>,
    /// List every entry
    #[arg(long)]
    list: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Worker {
            module,
            out_dir,
            inputs,
        } => {
            return ExitCode::from(worker::run_worker(&module, &out_dir, inputs) as u8);
        }
        Command::WorkerIdle => return ExitCode::from(worker::run_idle_worker() as u8),
        Command::Budgets => {
            print!("{}", report::budgets_text(&budget::merge(vec![])));
            return ExitCode::SUCCESS;
        }
        Command::Corpus(c) => corpus_cmd(c).map(|()| true),
        Command::Run(c) => run_cmd(c),
        Command::Scale(c) => scale_cmd(c),
        Command::Idle(c) => idle_cmd(c),
        Command::Check(c) => check_cmd(c),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("panoply-bench: {e}");
            ExitCode::from(2)
        }
    }
}

// ---------------------------------------------------------------------------------------------

fn make_runner(m: &ModuleOpts) -> Result<Runner> {
    match m.runner.as_str() {
        "worker" => {
            let registry = panoply_host::Registry::with_builtins();
            if registry.get(&m.module).is_none() {
                let known: Vec<String> = registry
                    .list()
                    .map(|x| x.manifest().module.id.clone())
                    .collect();
                return Err(BenchError::Usage(format!(
                    "unknown module {}; built-in modules: {}",
                    m.module,
                    known.join(", ")
                )));
            }
            Ok(Runner::Worker {
                exe: measure::current_exe()?,
            })
        }
        "cli" => {
            let bin = m.cli_bin.clone().or_else(runner::default_cli_bin).ok_or_else(|| {
                BenchError::Usage(
                    "cannot find the panoply binary; build it with `cargo build -p panoply-cli` or pass --cli-bin"
                        .into(),
                )
            })?;
            let args = m.cli_args.split_whitespace().map(str::to_string).collect();
            Ok(Runner::Cli { bin, args })
        }
        other => Err(BenchError::Usage(format!(
            "--runner must be worker or cli, not {other}"
        ))),
    }
}

fn mb_to_bytes(mb: f64) -> u64 {
    (mb * 1_000_000.0) as u64
}

fn build_inputs(s: &SliceOpts) -> Result<(Vec<Input>, usize)> {
    if !s.files.is_empty() {
        let mut v = Vec::new();
        for p in &s.files {
            let meta = std::fs::metadata(p)
                .map_err(|e| BenchError::io(format!("reading {}", p.display()), e))?;
            v.push(Input {
                id: p
                    .file_name()
                    .map_or("file".into(), |n| n.to_string_lossy().into_owned()),
                kind: "file".into(),
                path: p.clone(),
                bytes: meta.len(),
            });
        }
        return Ok((v, 0));
    }
    let root = s.corpus.clone().unwrap_or_else(Corpus::default_root);
    let corpus = Corpus::load(&root)?;
    let mut kinds = Vec::new();
    for k in &s.kind {
        if k.eq_ignore_ascii_case("all") {
            kinds.clear();
            break;
        }
        kinds.push(Kind::parse(k).ok_or_else(|| BenchError::Usage(format!("unknown kind {k}")))?);
    }
    let tier = Tier::parse(&s.tier)
        .ok_or_else(|| BenchError::Usage("--tier must be default, large or huge".into()))?;
    let slice = Slice {
        kinds,
        tags: s.tag.clone(),
        ids: s.id.clone(),
        max_tier: Some(tier),
        min_bytes: mb_to_bytes(s.min_mb),
        max_bytes: s.max_mb.map(mb_to_bytes),
    };
    let (found, missing) = corpus.select(&slice);
    let mut inputs: Vec<Input> = found
        .into_iter()
        .map(|(e, path, bytes)| Input {
            id: e.id.clone(),
            kind: e.kind().as_str().to_string(),
            path,
            bytes,
        })
        .collect();
    if let Some(n) = s.limit {
        inputs.truncate(n);
    }
    if inputs.is_empty() {
        return Err(BenchError::Usage(format!(
            "no files matched ({missing} matching entries are not on disk; run tests/corpus/fetch.py and generate.py)"
        )));
    }
    Ok((inputs, missing))
}

fn run_options(r: &RunOpts) -> RunOptions {
    RunOptions {
        runs: r.runs,
        instant: r.instant,
        typical_bytes: mb_to_bytes(r.typical_mb),
        baseline: r.baseline.clone(),
        speed_tolerance: r.speed_tolerance,
        baseline_min_bytes: mb_to_bytes(r.baseline_min_mb),
    }
}

fn parse_sizes(s: &[String]) -> Result<Vec<u64>> {
    s.iter()
        .map(|t| {
            stats::parse_size(t).ok_or_else(|| BenchError::Usage(format!("cannot read size {t:?}")))
        })
        .collect()
}

fn window(i: &IdleOpts) -> Window {
    Window {
        settle: Duration::from_secs_f64(i.settle_s),
        window: Duration::from_secs_f64(i.window_s),
        interval: Duration::from_millis(i.interval_ms.max(50)),
    }
}

/// Text output goes to stdout, progress to stderr, and both stay silent when JSON owns stdout.
struct Say {
    on: bool,
}

impl Say {
    fn new(json: &Option<String>) -> Say {
        Say {
            on: json.as_deref() != Some("-"),
        }
    }

    fn out(&self, text: &str) {
        if self.on {
            print!("{text}");
        }
    }

    fn err(&self, text: &str) {
        if self.on {
            eprintln!("{text}");
        }
    }
}

fn header(say: &Say) {
    let h = HostInfo::detect();
    say.out(&format!(
        "panoply-bench {} on {} {} ({} cpus{}), {} build\n",
        env!("CARGO_PKG_VERSION"),
        h.os,
        h.arch,
        h.cpus,
        h.cpu_brand
            .as_deref()
            .map_or(String::new(), |b| format!(", {b}")),
        h.build_profile
    ));
    if h.build_profile == "debug" {
        say.out(
            "note: debug build. Memory numbers hold, speed and CPU do not. Use `cargo build --release -p panoply-bench`.\n",
        );
    }
    say.out("\n");
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn finish(
    command: &str,
    json: &Option<String>,
    started: u64,
    parts: (
        Option<RunReport>,
        Option<ScaleReport>,
        Option<idle::IdleReport>,
    ),
    checks: Vec<Check>,
) -> Result<bool> {
    let budgets = budget::merge(checks);
    let passed = budget::all_passed(&budgets);
    let (run, scale, idle) = parts;
    let full = FullReport {
        schema: report::SCHEMA,
        tool: format!("panoply-bench {}", env!("CARGO_PKG_VERSION")),
        started_unix: started,
        host: HostInfo::detect(),
        command: command.to_string(),
        run,
        scale,
        idle,
        budgets: budgets.clone(),
        passed,
    };
    if json.as_deref() != Some("-") {
        print!("\n{}", report::budgets_text(&budgets));
    }
    if let Some(dest) = json {
        report::write_json(dest, &full)
            .map_err(|e| BenchError::io(format!("writing {dest}"), e))?;
        if dest != "-" {
            println!("JSON written to {dest}");
        }
    }
    Ok(passed)
}

fn do_run(
    say: &Say,
    runner: &Runner,
    module: &ModuleOpts,
    slice: &SliceOpts,
    opts: &RunOpts,
) -> Result<RunReport> {
    let (inputs, missing) = build_inputs(slice)?;
    say.err(&format!(
        "running {} on {} files, {} run(s) each",
        module.module,
        inputs.len(),
        opts.runs
    ));
    let report = panoply_bench::run::run_inputs(
        runner,
        &module.module,
        &inputs,
        &run_options(opts),
        missing,
        |i, n, input| {
            if !opts.quiet && say.on {
                eprintln!(
                    "  [{i}/{n}] {} ({})",
                    input.id,
                    stats::human_bytes(input.bytes)
                );
            }
        },
    )?;
    say.out(&report::run_text(&report, opts.verbose));
    Ok(report)
}

fn do_scale(
    say: &Say,
    runner: &Runner,
    module: &ModuleOpts,
    scale: &ScaleOpts,
    runs: usize,
) -> Result<ScaleReport> {
    let sizes = parse_sizes(&scale.sizes)?;
    say.err(&format!("scale: {} over sparse inputs", module.module));
    let report = scale::run_scale(
        runner,
        &module.module,
        &sizes,
        runs,
        scale.tmp_dir.as_deref(),
        mb_to_bytes(scale.tolerance_mb),
        |m| say.err(m),
    )?;
    say.out(&report::scale_text(&report));
    Ok(report)
}

fn do_idle(say: &Say, target: Target, opts: &IdleOpts) -> Result<idle::IdleReport> {
    say.err(&format!(
        "idle: settling {:.0} s, then sampling {:.0} s",
        opts.settle_s, opts.window_s
    ));
    let report = idle::measure(&target, window(opts))?;
    say.out(&report::idle_text(&report));
    Ok(report)
}

fn run_cmd(c: RunCmd) -> Result<bool> {
    let started = now();
    let say = Say::new(&c.module.json);
    let runner = make_runner(&c.module)?;
    header(&say);
    let r = do_run(&say, &runner, &c.module, &c.slice, &c.run)?;
    let checks = budget::from_run(&r);
    finish(
        "run",
        &c.module.json,
        started,
        (Some(r), None, None),
        checks,
    )
}

fn scale_cmd(c: ScaleCmd) -> Result<bool> {
    let started = now();
    let say = Say::new(&c.module.json);
    let runner = make_runner(&c.module)?;
    header(&say);
    let r = do_scale(&say, &runner, &c.module, &c.scale, c.runs)?;
    let checks = budget::from_scale(&r);
    finish(
        "scale",
        &c.module.json,
        started,
        (None, Some(r), None),
        checks,
    )
}

fn idle_cmd(c: IdleCmd) -> Result<bool> {
    let started = now();
    let say = Say::new(&c.json);
    header(&say);
    let target = if !c.pid.is_empty() {
        Target::Pids(c.pid.clone())
    } else if !c.cmd.is_empty() {
        Target::Command {
            argv: c.cmd.clone(),
        }
    } else {
        Target::HostWorker {
            exe: measure::current_exe()?,
        }
    };
    let r = do_idle(&say, target, &c.idle)?;
    let checks = budget::from_idle(&r);
    finish("idle", &c.json, started, (None, None, Some(r)), checks)
}

fn check_cmd(c: CheckCmd) -> Result<bool> {
    let started = now();
    let say = Say::new(&c.module.json);
    let runner = make_runner(&c.module)?;
    header(&say);
    let r = do_run(&say, &runner, &c.module, &c.slice, &c.run)?;
    say.out("\n");
    let s = do_scale(&say, &runner, &c.module, &c.scale, c.run.runs.min(2))?;
    say.out("\n");
    let i = do_idle(
        &say,
        Target::HostWorker {
            exe: measure::current_exe()?,
        },
        &c.idle,
    )?;
    let mut checks = budget::from_run(&r);
    checks.extend(budget::from_scale(&s));
    checks.extend(budget::from_idle(&i));
    finish(
        "check",
        &c.module.json,
        started,
        (Some(r), Some(s), Some(i)),
        checks,
    )
}

fn corpus_cmd(c: CorpusCmd) -> Result<()> {
    let root = c.corpus.clone().unwrap_or_else(Corpus::default_root);
    let corpus = Corpus::load(&root)?;
    let kinds: Vec<Kind> = if c.kind.is_empty() {
        Kind::ALL.to_vec()
    } else {
        c.kind
            .iter()
            .map(|k| Kind::parse(k).ok_or_else(|| BenchError::Usage(format!("unknown kind {k}"))))
            .collect::<Result<_>>()?
    };
    let mut rows = Vec::new();
    let mut listing = Vec::new();
    for kind in kinds {
        let (mut total, mut generated, mut on_disk, mut bytes) = (0, 0, 0, 0u64);
        for e in corpus.entries().iter().filter(|e| e.kind() == kind) {
            if !c.tag.is_empty() && !e.tags.iter().any(|t| c.tag.contains(t)) {
                continue;
            }
            total += 1;
            generated += usize::from(e.is_generated());
            let size = std::fs::metadata(corpus.path_of(e))
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len());
            if let Some(s) = size {
                on_disk += 1;
                bytes += s;
            }
            listing.push(vec![
                e.id.clone(),
                e.tier.clone().unwrap_or_else(|| "default".into()),
                size.map_or("missing".to_string(), stats::human_bytes),
                e.license.clone(),
                e.tags.join(","),
            ]);
        }
        rows.push(vec![
            kind.as_str().to_string(),
            total.to_string(),
            (total - generated).to_string(),
            generated.to_string(),
            on_disk.to_string(),
            stats::human_bytes(bytes),
        ]);
    }
    print!(
        "{}",
        report::table(
            &[
                "kind",
                "entries",
                "real",
                "generated",
                "on disk",
                "size on disk"
            ],
            &rows,
            &[1, 2, 3, 4, 5]
        )
    );
    if c.list {
        println!();
        print!(
            "{}",
            report::table(&["id", "tier", "size", "license", "tags"], &listing, &[])
        );
    }
    Ok(())
}
