//! The disk map engine: what is eating your space.
//!
//! [`scan`] walks a folder in parallel with the fastest call each OS has and returns a [`Tree`] of
//! allocated sizes plus a [`ScanReport`] that says what was counted once (hard links, APFS clones), what
//! was skipped (other volumes, unreadable folders) and why. See the crate README for the rules.

mod backend;
mod scan;
mod tree;
mod volume;

pub use backend::{BackendChoice, SkipReason};
pub use scan::{Progress, Scan, ScanError, ScanOptions, ScanReport, Skipped, scan};
pub use tree::{Kind, NodeId, Tree, flags};
pub use volume::{Accounting, VolumeInfo, info as volume_info};
