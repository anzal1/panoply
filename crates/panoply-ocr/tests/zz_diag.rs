mod common;
use common::*;
use std::io::Write;

fn say(s: &str) {
    eprintln!("DIAG {s}");
    let _ = std::io::stderr().flush();
}

#[test]
fn zz_probe_each_stage() {
    for l in ["en", "hi", "bn"] {
        say(&format!("lang {l}: parse"));
        let ls = langs(l);
        say("windows covers...");
        let w = panoply_ocr::engine::windows_ocr::covers(&ls);
        say(&format!("windows covers = {w}"));
        say("ppocr covers...");
        let p = panoply_ocr::ppocr::PpOcr::covers(&ls);
        say(&format!("ppocr covers = {p}"));
        say("tesseract program...");
        let t = panoply_ocr::engine::tesseract::program();
        say(&format!("tesseract = {t:?}"));
        say("tesseract covers...");
        let c = panoply_ocr::engine::tesseract::covers(&ls);
        say(&format!("tesseract covers = {c}"));
    }
    say("done");
}
