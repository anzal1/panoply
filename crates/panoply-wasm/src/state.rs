//! Per-job state behind the host imports, and the sandbox limits.
//!
//! One [`State`] lives exactly as long as one job. It holds the chosen input files, the
//! outputs being written, the cancel flag and the memory limiter, and it is the only thing a
//! module can reach through its imports.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use panoply_core::CancelToken;
use wasmtime::component::ResourceTable;
use wasmtime::{ResourceLimiter, Result};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::bindings::panoply::module::host::Host;
use crate::outputs::Outputs;

/// Most bytes one `read` call returns.
pub const MAX_READ: usize = 8 * 1024 * 1024;
/// Largest file `read-all` will return.
pub const MAX_READ_ALL: u64 = 64 * 1024 * 1024;
/// Most stderr text kept from a module, for error messages.
pub const STDERR_KEEP: usize = 8 * 1024;

/// The limits one instance runs under.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most linear memory the module may hold, in bytes.
    pub max_memory: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_memory: 256 * 1024 * 1024,
        }
    }
}

/// Enforces [`Limits`] and remembers whether the module hit the memory ceiling.
pub(crate) struct Limiter {
    max_memory: usize,
    pub(crate) hit_memory_limit: bool,
}

impl Limiter {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            max_memory: limits.max_memory,
            hit_memory_limit: false,
        }
    }
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool> {
        if desired > self.max_memory {
            self.hit_memory_limit = true;
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool> {
        Ok(desired <= 1_000_000)
    }

    fn instances(&self) -> usize {
        16
    }
    fn tables(&self) -> usize {
        16
    }
    fn memories(&self) -> usize {
        16
    }
}

/// One file the user chose.
pub(crate) struct Input {
    /// The path as the user gave it, used only to name outputs.
    pub(crate) chosen: PathBuf,
    pub(crate) file: File,
    pub(crate) size: u64,
}

/// A message from the worker thread to the supervising thread.
pub(crate) enum Msg {
    Progress(f32, String),
    Done,
}

/// Everything the instance can touch.
pub(crate) struct State {
    pub(crate) wasi: WasiCtx,
    pub(crate) table: ResourceTable,
    pub(crate) limiter: Limiter,
    pub(crate) inputs: Vec<Input>,
    pub(crate) outputs: Outputs,
    pub(crate) cancel: CancelToken,
    pub(crate) events: Sender<Msg>,
    pub(crate) stderr: MemoryOutputPipe,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl State {
    fn input(&mut self, handle: u32) -> std::result::Result<&mut Input, String> {
        self.inputs
            .get_mut(handle as usize)
            .ok_or_else(|| "That file handle is not one of the chosen files.".to_string())
    }

    fn check_cancel(&self) -> std::result::Result<(), String> {
        if self.cancel.is_cancelled() {
            Err("Cancelled.".into())
        } else {
            Ok(())
        }
    }

    fn input_path(&self, handle: Option<u32>) -> std::result::Result<Option<PathBuf>, String> {
        match handle {
            None => Ok(None),
            Some(h) => self
                .inputs
                .get(h as usize)
                .map(|i| Some(i.chosen.clone()))
                .ok_or_else(|| "That file handle is not one of the chosen files.".to_string()),
        }
    }
}

fn read_err(e: &std::io::Error) -> String {
    format!(
        "The file could not be read ({}). Check that it still exists.",
        e.kind()
    )
}

fn read_at(f: &mut File, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    f.take(len as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

impl Host for State {
    fn read(&mut self, handle: u32, offset: u64, len: u32) -> std::result::Result<Vec<u8>, String> {
        self.check_cancel()?;
        let want = (len as usize).min(MAX_READ);
        let input = self.input(handle)?;
        read_at(&mut input.file, offset, want).map_err(|e| read_err(&e))
    }

    fn read_all(&mut self, handle: u32) -> std::result::Result<Vec<u8>, String> {
        self.check_cancel()?;
        let input = self.input(handle)?;
        if input.size > MAX_READ_ALL {
            return Err(format!(
                "This file is {} MB, too big to read in one piece (limit {} MB).",
                input.size / (1024 * 1024),
                MAX_READ_ALL / (1024 * 1024)
            ));
        }
        read_at(&mut input.file, 0, input.size as usize).map_err(|e| read_err(&e))
    }

    fn save(
        &mut self,
        for_input: Option<u32>,
        label: String,
        ext: String,
        data: Vec<u8>,
    ) -> std::result::Result<String, String> {
        let id = self.begin_output(for_input, label, ext)?;
        self.append_output(id, data)?;
        self.finish_output(id)
    }

    fn begin_output(
        &mut self,
        for_input: Option<u32>,
        label: String,
        ext: String,
    ) -> std::result::Result<u32, String> {
        self.check_cancel()?;
        let path = self.input_path(for_input)?;
        self.outputs.begin(path.as_deref(), &label, &ext)
    }

    fn append_output(&mut self, output: u32, data: Vec<u8>) -> std::result::Result<(), String> {
        self.check_cancel()?;
        self.outputs.append(output, &data)
    }

    fn finish_output(&mut self, output: u32) -> std::result::Result<String, String> {
        self.outputs.finish(output)
    }

    fn progress(&mut self, fraction: f32, label: String) {
        let _ = self.events.send(Msg::Progress(fraction, label));
    }

    fn cancelled(&mut self) -> bool {
        self.cancel.is_cancelled()
    }
}
