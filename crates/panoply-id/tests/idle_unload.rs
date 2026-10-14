//! The YuNet session is dropped after the idle time.
//!
//! This is its own test binary on purpose. The session is process wide, and this test sets a short
//! idle time and waits for the session to be gone, so no other test may run YuNet in the same
//! process while it runs: cargo runs the tests of one binary in parallel, but runs binaries one
//! after the other. Do not add other YuNet tests to this file.

mod common;

use common::*;
use panoply_id::face::{self, Engine, EnginePref};

#[test]
fn the_fallback_model_unloads_when_idle() {
    if !engines_available() {
        return;
    }
    if panoply_id::runtime::ensure_onnxruntime().is_err()
        || panoply_id::runtime::ensure_model().is_err()
    {
        eprintln!("skipping: the YuNet engine could not be fetched here");
        return;
    }
    let Some(p) = portrait("hoshide") else { return };
    // Only "loaded after use, gone after the idle time", with a short idle.
    panoply_id::runtime::set_idle_unload(std::time::Duration::from_millis(800));
    let img = read(&p);
    let (faces, e) = face::detect(&img, EnginePref::YuNet).unwrap();
    assert_eq!((faces.len(), e), (1, Engine::YuNet));
    let t = std::time::Instant::now();
    while panoply_id::runtime::session_loaded() && t.elapsed() < std::time::Duration::from_secs(10)
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panoply_id::runtime::set_idle_unload(std::time::Duration::from_secs(60));
    assert!(
        !panoply_id::runtime::session_loaded(),
        "the session was still loaded after the idle time"
    );
}
