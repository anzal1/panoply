//! Modules that ship inside the host.

use std::sync::Arc;

use crate::Module;

pub mod hash;
pub mod image;
pub mod pdf;

/// Every built-in module.
pub fn all() -> Vec<Arc<dyn Module>> {
    let mut v: Vec<Arc<dyn Module>> = vec![Arc::new(hash::HashModule::new())];
    v.extend(
        image::all()
            .into_iter()
            .map(|m| Arc::new(m) as Arc<dyn Module>),
    );
    for m in pdf::modules() {
        v.push(Arc::new(m));
    }
    v
}
