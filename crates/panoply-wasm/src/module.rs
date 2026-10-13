use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use panoply_core::sniff;
use panoply_host::{Module, ModuleError, RunContext};
use panoply_manifest::{FilesAccess, Manifest};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, OptLevel, Store, Strategy, UpdateDeadline};
use wasmtime_wasi::WasiCtx;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

use crate::bindings::Module as Bindings;
use crate::bindings::panoply::module::host::{InputFile, Output};
use crate::options;
use crate::outputs::Outputs;
use crate::state::{Input, Limiter, Limits, Msg, STDERR_KEEP, State};

/// How often the supervising thread checks the cancel flag while a module runs.
const CANCEL_POLL: Duration = Duration::from_millis(10);

/// Where a module's component is looked for when `module.toml` does not say.
const BESIDE: &str = "module.wasm";
const CARGO_BUILD: &str = "target/wasm32-wasip2/release/module.wasm";

/// Interfaces a module may never import. They are refused at load, before any code runs.
const DENIED_IMPORTS: &[(&str, &str)] = &[
    ("wasi:sockets/", "the network"),
    ("wasi:http/", "the network"),
];

/// A community module: a manifest and the WebAssembly component it points to.
///
/// Holds only the manifest and a path. The engine, the compiled code and the instance are
/// built inside [`Module::run`] and dropped when it returns.
pub struct WasmModule {
    manifest: Manifest,
    wasm: PathBuf,
    limits: Limits,
    compiler: Compiler,
}

/// Which compiler turns the component into machine code for a job.
///
/// Community modules are glue (heavy work runs in host engines), and they are compiled on
/// every run because nothing is kept between jobs, so compile time is the cost that counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compiler {
    /// Winch, a baseline compiler. About a third of the compile time, slower generated code.
    #[default]
    Fast,
    /// Cranelift with optimizations. Slower to start, faster to run: for a module that
    /// loops over a lot of data inside the sandbox.
    Optimizing,
}

impl WasmModule {
    /// Load a module from its `module.toml`, or from the folder that contains one.
    ///
    /// The component is the `wasm` path in the manifest's `[module]` table, relative to the
    /// manifest, or `module.wasm` next to it.
    pub fn load(path: &Path) -> Result<Self, ModuleError> {
        let manifest_path = if path.is_dir() {
            path.join("module.toml")
        } else {
            path.to_path_buf()
        };
        let manifest = Manifest::from_file(&manifest_path)
            .map_err(|e| ModuleError::BadModule(e.to_string()))?;
        manifest
            .validate()
            .map_err(|errs| ModuleError::BadModule(errs.join(" ")))?;
        let dir = manifest_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let wasm = match manifest.module.wasm.as_deref() {
            Some(rel) => {
                let rel = Path::new(rel);
                if rel.is_absolute() {
                    return Err(ModuleError::BadModule(
                        "the wasm path in module.toml must be relative to module.toml".into(),
                    ));
                }
                dir.join(rel)
            }
            // Not declared: a file shipped next to the manifest, else a fresh cargo build.
            None => [BESIDE, CARGO_BUILD]
                .iter()
                .map(|rel| dir.join(rel))
                .find(|p| p.is_file())
                .unwrap_or_else(|| dir.join(CARGO_BUILD)),
        };
        if !wasm.is_file() {
            return Err(ModuleError::BadModule(format!(
                "the compiled module is missing at {}. Build it first with: cargo build \
                 --release --target wasm32-wasip2",
                wasm.display()
            )));
        }
        Ok(Self {
            manifest,
            wasm,
            limits: Limits::default(),
            compiler: Compiler::default(),
        })
    }

    /// Run under different sandbox limits.
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Compile with a different compiler.
    pub fn with_compiler(mut self, compiler: Compiler) -> Self {
        self.compiler = compiler;
        self
    }

    /// Where the compiled component was found.
    pub fn wasm_path(&self) -> &Path {
        &self.wasm
    }
}

fn engine(compiler: Compiler) -> Result<Engine, ModuleError> {
    let mut config = Config::new();
    config.epoch_interruption(true);
    match compiler {
        Compiler::Fast => config.strategy(Strategy::Winch),
        Compiler::Optimizing => config
            .strategy(Strategy::Cranelift)
            .cranelift_opt_level(OptLevel::Speed),
    };
    Engine::new(&config)
        .map_err(|e| ModuleError::BadModule(format!("the runtime would not start: {e}")))
}

/// Open the chosen files, check each against what the manifest accepts, and describe them
/// to the module. Only these files are ever readable by it.
fn open_inputs(
    ctx: &RunContext,
    manifest: &Manifest,
    paths: &[PathBuf],
) -> Result<(Vec<Input>, Vec<InputFile>), ModuleError> {
    if paths.is_empty() && manifest.capabilities.files == FilesAccess::Chosen {
        return Err(ModuleError::Failed(format!(
            "{} needs at least one file. Choose a file and run it again.",
            manifest.module.name
        )));
    }
    let mut inputs = Vec::new();
    let mut described = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let file = ctx.guard.open_read(path)?;
        let meta = file.metadata().map_err(|source| ModuleError::Io {
            path: path.clone(),
            source,
        })?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !meta.is_file() {
            return Err(ModuleError::Failed(format!(
                "{name} is a folder. Choose the files inside it."
            )));
        }
        let sniffed = sniff(path);
        if !manifest.accepts_file(&sniffed.mime, &sniffed.ext) {
            return Err(ModuleError::Failed(format!(
                "{name} is a {} file, and {} works on {}.",
                sniffed.mime,
                manifest.module.name,
                manifest.accepts.join(", ")
            )));
        }
        described.push(InputFile {
            handle: i as u32,
            name,
            mime: sniffed.mime.clone(),
            size: meta.len(),
        });
        inputs.push(Input {
            chosen: path.clone(),
            file,
            size: meta.len(),
        });
    }
    Ok((inputs, described))
}

/// How a job ended, before it is turned into a [`ModuleError`].
enum Failure {
    Cancelled,
    /// The module returned its own plain message.
    Module(String),
    /// The runtime stopped the module, or the component could not be loaded.
    Sandbox(String),
    Load(String),
}

struct Finished {
    outcome: Result<Vec<Output>, Failure>,
    outputs: Outputs,
}

fn check_imports(engine: &Engine, component: &Component) -> Result<(), Failure> {
    for (name, _) in component.component_type().imports(engine) {
        for (prefix, what) in DENIED_IMPORTS {
            if name.starts_with(prefix) {
                return Err(Failure::Load(format!(
                    "it asks for {what} ({name}), which Panoply modules cannot have"
                )));
            }
        }
    }
    Ok(())
}

/// Compile, link, instantiate and call `run`. Runs on the worker thread.
fn execute(
    engine: &Engine,
    wasm: &Path,
    limits: Limits,
    parts: Parts,
    described: &[InputFile],
    options_json: &str,
) -> Finished {
    let stderr = MemoryOutputPipe::new(STDERR_KEEP);
    let state = State {
        // No preopened folders, no environment, no arguments, no sockets, stdout discarded.
        wasi: WasiCtx::builder().stderr(stderr.clone()).build(),
        table: ResourceTable::new(),
        limiter: Limiter::new(limits),
        inputs: parts.inputs,
        outputs: parts.outputs,
        cancel: parts.cancel,
        events: parts.events,
        stderr,
    };
    let mut store = Store::new(engine, state);
    store.limiter(|s| &mut s.limiter);
    // The supervisor bumps the epoch when the user cancels. Until then nothing interrupts.
    store.set_epoch_deadline(1);
    store.epoch_deadline_callback(|ctx| {
        if ctx.data().cancel.is_cancelled() {
            Err(wasmtime::Error::msg("cancelled"))
        } else {
            Ok(UpdateDeadline::Continue(1))
        }
    });
    let outcome = call(&mut store, engine, wasm, described, options_json, limits);
    let outputs = std::mem::replace(&mut store.data_mut().outputs, Outputs::new(None));
    Finished { outcome, outputs }
}

fn call(
    store: &mut Store<State>,
    engine: &Engine,
    wasm: &Path,
    described: &[InputFile],
    options_json: &str,
    limits: Limits,
) -> Result<Vec<Output>, Failure> {
    let component = Component::from_file(engine, wasm).map_err(|e| {
        Failure::Load(format!(
            "this is not a WebAssembly component ({}). Build it for wasm32-wasip2",
            first_line(&e.to_string())
        ))
    })?;
    check_imports(engine, &component)?;
    let mut linker: Linker<State> = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(|e| Failure::Load(e.to_string()))?;
    Bindings::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
        .map_err(|e| Failure::Load(e.to_string()))?;

    let result = Bindings::instantiate(&mut *store, &component, &linker)
        .map_err(|e| {
            Failure::Load(format!(
                "it does not fit the Panoply module interface ({})",
                first_line(&e.to_string())
            ))
        })
        .map(|m| m.call_run(&mut *store, described, options_json));
    let state = store.data();
    let said = String::from_utf8_lossy(&state.stderr.contents())
        .trim()
        .to_string();
    match result? {
        _ if state.cancel.is_cancelled() => Err(Failure::Cancelled),
        Ok(Ok(list)) => Ok(list),
        Ok(Err(message)) => Err(Failure::Module(message)),
        Err(_) if state.limiter.hit_memory_limit => Err(Failure::Sandbox(format!(
            "it needed more than {} MB of memory, which is its limit",
            limits.max_memory / (1024 * 1024)
        ))),
        Err(e) if said.is_empty() => Err(Failure::Sandbox(format!(
            "the module crashed ({})",
            first_line(&e.to_string())
        ))),
        Err(_) => Err(Failure::Sandbox(format!(
            "the module crashed and said: {}",
            tail(&said)
        ))),
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

fn tail(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let keep = if lines.len() > 6 {
        &lines[lines.len() - 6..]
    } else {
        &lines[..]
    };
    keep.join(" / ")
}

/// What moves from the supervising thread into the worker.
struct Parts {
    inputs: Vec<Input>,
    outputs: Outputs,
    cancel: panoply_core::CancelToken,
    events: std::sync::mpsc::Sender<Msg>,
}

impl Module for WasmModule {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn run(
        &self,
        ctx: &RunContext,
        inputs: &[PathBuf],
        options: &serde_json::Value,
    ) -> Result<Vec<PathBuf>, ModuleError> {
        ctx.check_cancel()?;
        let options_json = options::resolve(&self.manifest, options)?.to_string();
        let (opened, described) = open_inputs(ctx, &self.manifest, inputs)?;
        let engine = engine(self.compiler)?;
        let (tx, rx) = channel();
        let parts = Parts {
            inputs: opened,
            outputs: Outputs::new(ctx.guard.out_dir().map(Path::to_path_buf)),
            cancel: ctx.cancel.clone(),
            events: tx.clone(),
        };

        let limits = self.limits;
        let wasm = self.wasm.as_path();
        let finished = std::thread::scope(|scope| {
            let worker_engine = engine.clone();
            let worker = scope.spawn(move || {
                let done = tx;
                let finished = execute(
                    &worker_engine,
                    wasm,
                    limits,
                    parts,
                    &described,
                    &options_json,
                );
                let _ = done.send(Msg::Done);
                finished
            });
            // This thread is the supervisor: it forwards progress, and when the user cancels
            // it bumps the epoch so a module that never polls `cancelled()` still stops.
            loop {
                match rx.recv_timeout(CANCEL_POLL) {
                    Ok(Msg::Progress(fraction, label)) => ctx.progress(fraction, label),
                    Ok(Msg::Done) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        if ctx.cancel.is_cancelled() {
                            engine.increment_epoch();
                        }
                    }
                }
            }
            worker.join()
        });
        drop(engine);

        let Finished {
            outcome,
            mut outputs,
        } = finished.map_err(|_| {
            ModuleError::Sandbox("the runtime failed while running the module".into())
        })?;
        match outcome {
            Err(failure) => {
                outputs.rollback();
                Err(match failure {
                    Failure::Cancelled => ModuleError::Cancelled,
                    Failure::Module(m) => ModuleError::Failed(m),
                    Failure::Sandbox(m) => ModuleError::Sandbox(m),
                    Failure::Load(m) => ModuleError::BadModule(m),
                })
            }
            Ok(list) => {
                outputs.discard_unfinished();
                collect(ctx, &outputs, list).inspect_err(|_| outputs.rollback())
            }
        }
    }
}

/// Turn the module's returned list into the host's result: files in the order the module
/// listed them, then any finished file it forgot to list, and notes into the report.
fn collect(
    ctx: &RunContext,
    outputs: &Outputs,
    list: Vec<Output>,
) -> Result<Vec<PathBuf>, ModuleError> {
    let mut taken = vec![false; outputs.finished().len()];
    let mut files = Vec::new();
    for item in list {
        match item {
            Output::Note(line) => ctx.note(line),
            Output::File(name) => {
                let slot = outputs
                    .finished()
                    .iter()
                    .enumerate()
                    .find(|(i, (n, _))| !taken[*i] && *n == name);
                match slot {
                    Some((i, (_, path))) => {
                        taken[i] = true;
                        files.push(path.clone());
                    }
                    None => {
                        return Err(ModuleError::Failed(format!(
                            "The module reported a file called {name} that it did not write."
                        )));
                    }
                }
            }
        }
    }
    for (i, (_, path)) in outputs.finished().iter().enumerate() {
        if !taken[i] {
            files.push(path.clone());
        }
    }
    Ok(files)
}
