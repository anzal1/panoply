//! Cooperative cancellation. Cloning shares the same flag.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A shared cancel flag. Long jobs poll [`CancelToken::is_cancelled`] between chunks
/// so cancel is instant and leaves nothing half-written.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask every holder of this token to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// True once [`CancelToken::cancel`] has been called on any clone.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_clear_and_cancels() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn clones_share_state() {
        let a = CancelToken::new();
        let b = a.clone();
        b.cancel();
        assert!(a.is_cancelled());
    }

    #[test]
    fn works_across_threads() {
        let t = CancelToken::new();
        let t2 = t.clone();
        std::thread::spawn(move || t2.cancel()).join().unwrap();
        assert!(t.is_cancelled());
    }
}
