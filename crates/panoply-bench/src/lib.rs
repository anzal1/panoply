//! The Panoply budget harness.
//!
//! FLOWS.md ("Budgets every module must meet") turns the product philosophy into numbers and
//! says CI measures them for every module. This crate is that measurement. It runs a module over
//! a slice of the test corpus (`tests/corpus`), in a child process so every job gets its own
//! memory high-water mark, and compares what it sees with [`panoply_core::budgets`].
//!
//! The pieces:
//!
//! - [`corpus`] reads the manifests and picks a slice of files.
//! - [`measure`] runs one command under `/usr/bin/time` and reads wall time, CPU time, peak
//!   resident set size and (on macOS) peak memory footprint. No `unsafe`, no extra dependencies.
//! - [`run`] runs a module over a list of files and collects one result per file.
//! - [`runner`] decides how a module is run: through the host registry (`panoply-bench worker`)
//!   or through the `panoply` command line binary.
//! - [`scale`] makes sparse files on the fly and checks that peak memory stays flat as the
//!   input grows from megabytes to gigabytes.
//! - [`idle`] samples a long-running process for CPU and memory while nothing runs.
//! - [`budget`] is the FLOWS.md table as data, with pass, fail and not measured verdicts.
//! - [`report`] prints the table and writes the JSON.
//!
//! What this harness cannot measure (UI thread time, frame rate, model unloading, output quality)
//! is listed in the report as "not measured", never silently passed.

pub mod budget;
pub mod corpus;
pub mod idle;
pub mod measure;
pub mod report;
pub mod run;
pub mod runner;
pub mod scale;
pub mod stats;
pub mod worker;

/// Errors from the harness itself, not from the module under test.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    /// A file or directory could not be read or written.
    #[error("{context}: {source}")]
    Io {
        /// What was being done.
        context: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// A manifest could not be parsed.
    #[error("manifest {path}: {message}")]
    Manifest {
        /// The manifest file.
        path: String,
        /// What is wrong with it.
        message: String,
    },
    /// The measuring tool (`/usr/bin/time`) is not available on this platform.
    #[error("{0}")]
    Unsupported(String),
    /// A child process could not be started or its output could not be understood.
    #[error("{0}")]
    Run(String),
    /// A command line value is not valid.
    #[error("{0}")]
    Usage(String),
}

impl BenchError {
    /// Wrap an I/O error with what was being done.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

/// Result alias for the harness.
pub type Result<T> = std::result::Result<T, BenchError>;
