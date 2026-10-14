//! A loaded model is dropped after it has been idle (FLOWS.md budgets: models unload after 60 s).
//!
//! This test has its own binary, so no other test holds or loads a model while it counts.

mod common;

use common::*;
use panoply_vision::models::ESRGAN;
use serde_json::json;

#[test]
fn a_model_is_dropped_after_it_has_been_idle() {
    if !have(&ESRGAN) {
        return;
    }
    let d = Scratch::new("idle");
    let input = d.path().join("p.png");
    gradient_png(&input, 96, 96);
    let io = panoply_image::ops::FsIo::new();
    module("image.upscale")
        .run(&io, &[input], &json!({"scale": "2"}))
        .unwrap();
    assert!(
        panoply_vision::runtime::loaded_count() >= 1,
        "the model should be loaded after a job"
    );
    // Not idle long enough: stays loaded.
    panoply_vision::runtime::evict_idle_with(std::time::Duration::from_secs(3600));
    assert!(panoply_vision::runtime::loaded_count() >= 1);
    std::thread::sleep(std::time::Duration::from_millis(60));
    panoply_vision::runtime::evict_idle_with(std::time::Duration::from_millis(50));
    assert_eq!(
        panoply_vision::runtime::loaded_count(),
        0,
        "an idle model must be unloaded"
    );
}
