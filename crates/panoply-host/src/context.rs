use std::sync::Mutex;

use panoply_core::{CancelToken, JobEvent};

use crate::{CapabilityGuard, ModuleError};

type Sink = Box<dyn Fn(JobEvent) + Send + Sync>;

/// Everything a module gets for one run: cancel, progress, a capability guard, and a place
/// to put report lines for read-only modules.
pub struct RunContext {
    /// Poll this between chunks of work.
    pub cancel: CancelToken,
    /// The only way to open files.
    pub guard: CapabilityGuard,
    sink: Sink,
    notes: Mutex<Vec<String>>,
}

impl RunContext {
    /// A context with the given guard, a fresh cancel token and a silent progress sink.
    pub fn new(guard: CapabilityGuard) -> Self {
        Self {
            cancel: CancelToken::new(),
            guard,
            sink: Box::new(|_| {}),
            notes: Mutex::new(Vec::new()),
        }
    }

    /// Use this cancel token.
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Receive progress events.
    pub fn with_progress(mut self, sink: impl Fn(JobEvent) + Send + Sync + 'static) -> Self {
        self.sink = Box::new(sink);
        self
    }

    /// Report progress. `fraction` is clamped to 0.0 to 1.0.
    pub fn progress(&self, fraction: f32, label: impl Into<String>) {
        (self.sink)(JobEvent::Progress {
            fraction: fraction.clamp(0.0, 1.0),
            label: label.into(),
        });
    }

    /// Add one line to the job's report. Read-only modules (pattern I) answer this way.
    pub fn note(&self, line: impl Into<String>) {
        self.notes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(line.into());
    }

    /// Take the report lines collected so far.
    pub fn take_notes(&self) -> Vec<String> {
        std::mem::take(&mut *self.notes.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// Fail with [`ModuleError::Cancelled`] when cancel was requested.
    pub fn check_cancel(&self) -> Result<(), ModuleError> {
        if self.cancel.is_cancelled() {
            Err(ModuleError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn progress_is_clamped_and_delivered() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let ctx = RunContext::new(CapabilityGuard::none())
            .with_progress(move |e| s2.lock().unwrap().push(e));
        ctx.progress(2.0, "x");
        match &seen.lock().unwrap()[0] {
            JobEvent::Progress { fraction, label } => {
                assert_eq!(*fraction, 1.0);
                assert_eq!(label, "x");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn notes_are_collected_once() {
        let ctx = RunContext::new(CapabilityGuard::none());
        ctx.note("a");
        ctx.note("b");
        assert_eq!(ctx.take_notes(), vec!["a", "b"]);
        assert!(ctx.take_notes().is_empty());
    }

    #[test]
    fn cancel_is_visible() {
        let ctx = RunContext::new(CapabilityGuard::none());
        assert!(ctx.check_cancel().is_ok());
        ctx.cancel.cancel();
        assert!(matches!(ctx.check_cancel(), Err(ModuleError::Cancelled)));
    }
}
