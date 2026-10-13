//! The child side: run one job through the host registry and report on it.
//!
//! The harness starts `panoply-bench worker ...` as a child so the module runs in a process of
//! its own and the kernel can report that process's memory high-water mark. The worker prints
//! one JSON line, [`WorkerReport`], on stdout and exits 0 when the job succeeded.
//!
//! `worker-idle` is the other child: it builds the registry, says `ready`, and then waits on
//! stdin without doing anything. [`crate::idle`] measures it to see what the host costs while
//! no job runs.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Instant;

use panoply_core::{CancelToken, JobSpec};
use panoply_host::Registry;
use serde::{Deserialize, Serialize};

/// What the worker tells the harness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkerReport {
    /// True when the job finished without error.
    pub ok: bool,
    /// Milliseconds spent inside the registry call, excluding process start.
    pub inner_ms: f64,
    /// Files the job wrote.
    pub outputs: Vec<PathBuf>,
    /// How many report lines a read-only module produced.
    pub notes: usize,
    /// Plain error message when the job failed.
    pub error: Option<String>,
}

/// Run one job and print the report. Returns the process exit code.
pub fn run_worker(module: &str, out_dir: &std::path::Path, inputs: Vec<PathBuf>) -> i32 {
    let registry = Registry::with_builtins();
    let spec = JobSpec {
        module_id: module.to_string(),
        inputs,
        options: serde_json::Value::Null,
    };
    let started = Instant::now();
    let result = registry.run_job(&spec, Some(out_dir), CancelToken::new(), |_| {});
    let inner_ms = started.elapsed().as_secs_f64() * 1000.0;
    let report = match result {
        Ok(o) => WorkerReport {
            ok: true,
            inner_ms,
            outputs: o.outputs,
            notes: o.notes.len(),
            error: None,
        },
        Err(e) => WorkerReport {
            ok: false,
            inner_ms,
            outputs: Vec::new(),
            notes: 0,
            error: Some(format!("{} {}", e.plain_message(), e.next_step())),
        },
    };
    println!(
        "{}",
        serde_json::to_string(&report).expect("report serializes")
    );
    i32::from(!report.ok)
}

/// Build the registry, print `ready <ms>`, then wait on stdin until it closes.
pub fn run_idle_worker() -> i32 {
    let started = Instant::now();
    let registry = Registry::with_builtins();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    // Keep the registry alive for the whole wait: that is the memory a running host holds.
    println!("ready {ms:.3} {}", registry.len());
    let _ = std::io::stdout().flush();
    let mut sink = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut sink);
    drop(registry);
    0
}

/// Parse the line [`run_idle_worker`] prints: milliseconds spent building the registry.
pub fn parse_ready_line(line: &str) -> Option<f64> {
    let mut it = line.split_whitespace();
    (it.next()? == "ready").then(|| it.next()?.parse().ok())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_line_parses() {
        assert_eq!(parse_ready_line("ready 1.250 3"), Some(1.25));
        assert_eq!(parse_ready_line("nope 1.0"), None);
        assert_eq!(parse_ready_line(""), None);
    }

    #[test]
    fn report_round_trips() {
        let r = WorkerReport {
            ok: true,
            inner_ms: 1.5,
            outputs: vec![PathBuf::from("/tmp/x")],
            notes: 2,
            error: None,
        };
        let back: WorkerReport = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(r, back);
    }
}
