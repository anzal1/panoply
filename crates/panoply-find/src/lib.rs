//! Instant file-name search for Panoply.
//!
//! Three parts, each small enough to read in one sitting:
//!
//! * [`walk`]: a parallel walker that reads a folder tree into a [`Segment`], the compact name table
//!   (interned parent folders plus name bytes, no `String` per path; see [`segment`] for the layout and
//!   bytes per file).
//! * [`Index::search`]: the per-keystroke query engine. Exact and prefix matches come from a one-byte-
//!   per-file column, every other substring match from a SIMD search over one contiguous buffer of
//!   folded names, and fuzzy matching runs only on names a 32-bit character mask lets through. Results
//!   are ranked by match quality first, then recency, shallowness and name length.
//! * [`Exclusions`]: what a scan never enters (dependency folders, `*.noindex`, system folders, network
//!   mounts).
//!
//! Everything is safe Rust. Freshness (FSEvents, USN journal, inotify and fanotify) feeds the index
//! through small delta segments and tombstones; that layer is built on top of this crate.

mod exclude;
pub mod fold;
pub mod fuzzy;
mod index;
mod mounts;
mod query;
pub mod segment;
mod walk;

pub use exclude::Exclusions;
pub use index::{EntryInfo, Index};
pub use mounts::{network_mounts, parse_mount_output, parse_mountinfo};
pub use query::{Hit, Kind, SearchOptions};
pub use segment::{Segment, SegmentBuilder};
#[cfg(feature = "ignore-walker")]
pub use walk::walk_ignore;
pub use walk::{WalkOptions, WalkStats, walk};
