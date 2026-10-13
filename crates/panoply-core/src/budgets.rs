//! The budgets every module must meet (FLOWS.md, "Budgets every module must meet").
//!
//! CI measures these for every module. A module that misses one does not ship.

/// Module start: click to its screen being interactive, in milliseconds.
pub const MODULE_START_MS: u64 = 50;
/// First preview for instant and tuned transforms on a typical file, in milliseconds.
pub const FIRST_PREVIEW_MS: u64 = 100;
/// Longest task allowed on the UI thread, in milliseconds.
pub const UI_TASK_MS: u64 = 4;
/// Peak memory per job for documents and images, in megabytes.
pub const PEAK_MEM_DOC_MB: u64 = 300;
/// Seconds of idle time after which a loaded model is unloaded.
pub const MODEL_IDLE_UNLOAD_S: u64 = 60;
/// Idle app memory with nothing running, in megabytes.
pub const IDLE_APP_MB: u64 = 100;
/// Jobs longer than this show progress, in milliseconds (FLOWS.md, shared capability 9).
pub const PROGRESS_AFTER_MS: u64 = 300;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_match_flows_doc() {
        assert_eq!(MODULE_START_MS, 50);
        assert_eq!(FIRST_PREVIEW_MS, 100);
        assert_eq!(UI_TASK_MS, 4);
        assert_eq!(PEAK_MEM_DOC_MB, 300);
        assert_eq!(MODEL_IDLE_UNLOAD_S, 60);
        assert_eq!(IDLE_APP_MB, 100);
    }

    #[test]
    fn preview_fits_inside_a_frame_budget_ordering() {
        // Start must be faster than the first preview, which must be faster than a second.
        const { assert!(MODULE_START_MS < FIRST_PREVIEW_MS) };
        const { assert!(UI_TASK_MS < MODULE_START_MS) };
    }
}
