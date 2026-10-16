//! Panoply core: the small, dependency-light pieces every platform shares.
//!
//! - [`sniff`] identifies a file from its bytes, with the extension as a fallback.
//! - [`naming`] picks output names that never overwrite anything.
//! - [`cancel`] is the cooperative cancel flag every long job polls.
//! - [`job`] holds the job and event types exchanged with every UI.
//! - [`assets`] is the consent gate every model and engine download goes through.
//! - [`trash`] moves a file to the Trash or Recycle Bin and never deletes it for good.
//! - [`budgets`] holds the performance budgets from FLOWS.md as constants.

pub mod assets;
pub mod budgets;
pub mod cancel;
pub mod job;
pub mod modules;
pub mod naming;
pub mod sniff;
pub mod trash;

pub use cancel::CancelToken;
pub use job::{JobEvent, JobId, JobSpec};
pub use naming::unique_output_path;
pub use sniff::{FamilyHint, Sniffed, sniff, sniff_bytes};
pub use trash::{TrashError, move_to_trash};
