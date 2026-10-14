//! Loaded models are dropped after the idle timeout (60 s in production, shortened here). Its
//! own test binary, because the timeout and the loaded models are process wide.

mod common;

use std::time::{Duration, Instant};

use common::*;
use panoply_ocr::{EngineKind, Ocr, ppocr};

#[test]
fn models_unload_when_idle_and_load_again_on_demand() {
    ppocr::set_idle_timeout(Duration::from_millis(600));
    let ocr = Ocr::only(EngineKind::PpOcr);
    let Some(first) = read(&ocr, "en-1", "en") else {
        return;
    };
    assert!(first.word_count() > 30);
    assert!(ppocr::models_loaded(), "models are held right after a job");
    let start = Instant::now();
    while ppocr::models_loaded() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "models were never unloaded"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        start.elapsed() >= Duration::from_millis(400),
        "unloaded before the idle time"
    );
    // A new job loads them again.
    let again = read(&ocr, "en-1", "en").unwrap();
    assert_eq!(again.text(), first.text());
    assert!(ppocr::models_loaded());
    ppocr::unload();
    assert!(!ppocr::models_loaded());
}
