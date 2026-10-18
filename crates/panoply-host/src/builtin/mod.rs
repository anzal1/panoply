//! Modules that ship inside the host.

use std::sync::Arc;

use crate::Module;

pub mod archive;
pub mod hash;
pub mod id;
pub mod image;
pub mod media;
pub mod ocr;
pub mod pdf;
pub mod tts;
pub mod vision;

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
    for m in media::all() {
        v.push(Arc::new(m));
    }
    for m in id::all() {
        v.push(Arc::new(m));
    }
    for m in ocr::all() {
        v.push(Arc::new(m));
    }
    for m in tts::all() {
        v.push(Arc::new(m));
    }
    for m in vision::all() {
        v.push(Arc::new(m));
    }
    for m in archive::all() {
        v.push(Arc::new(m));
    }
    v
}
