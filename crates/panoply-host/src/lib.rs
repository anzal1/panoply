//! The Panoply module host.
//!
//! A [`Module`] declares a manifest and does one job. The host hands it a [`RunContext`]
//! with a cancel token, a progress sink and a [`CapabilityGuard`]. The guard is the whole
//! sandbox contract for native modules: a module can read only the files the user chose,
//! and write only inside the chosen output folder. The WebAssembly runtime (`panoply-wasm`, on
//! wasmtime) enforces the same contract, so built-in and community modules behave the same.
//!
//! Built-in modules live in [`builtin`]. [`Registry`] registers and looks up modules.

pub mod builtin;
mod context;
mod error;
mod guard;
mod module;
mod registry;

pub use context::RunContext;
pub use error::ModuleError;
pub use guard::CapabilityGuard;
pub use module::Module;
pub use registry::{JobOutcome, Registry};
