//! Runs community modules compiled to WebAssembly components, inside a sandbox.
//!
//! A community module is a `module.toml` plus a `.wasm` component built from
//! `wit/module.wit`. [`WasmModule`] loads the pair and implements [`panoply_host::Module`],
//! so a community module registers, lists and runs exactly like a built-in one.
//!
//! The sandbox has no ambient authority:
//!
//! - **Files.** The module gets handles to the files the user chose and reads them through
//!   `read` and `read-all`. It gets no paths, no preopened folders and no directory
//!   listing. It writes only by asking the host for a new output, and the host picks the name
//!   with the same never-overwrite rule the native modules use.
//! - **Network.** None. A component that imports `wasi:sockets` or `wasi:http` is refused
//!   before it runs.
//! - **Memory.** 256 MB of linear memory per instance by default ([`Limits`]).
//! - **Time.** Epoch interruption stops a module that ignores `cancelled()`, usually within
//!   10 ms, and cancel removes every file the job created.
//! - **Idle.** Engine, compiled code and instance are created for the job and dropped after
//!   it. Nothing stays resident between jobs.

mod module;
mod options;
mod outputs;
mod state;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "module",
    });
}

pub use module::{Compiler, WasmModule};
pub use options::{from_pairs as options_from_pairs, resolve as resolve_options};
pub use state::{Limits, MAX_READ, MAX_READ_ALL};

/// The WIT source of the module interface, the same text the template ships.
pub const MODULE_WIT: &str = include_str!("../wit/module.wit");
