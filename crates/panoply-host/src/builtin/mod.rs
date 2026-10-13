//! Modules that ship inside the host.

use std::sync::Arc;

use crate::Module;

pub mod hash;

/// Every built-in module.
pub fn all() -> Vec<Arc<dyn Module>> {
    vec![Arc::new(hash::HashModule::new())]
}
