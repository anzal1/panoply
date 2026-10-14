//! Engines against the labelled set. Tests that need model files download them on first run
//! (pinned checksums) and skip when that is not possible.

mod common;

use common::*;
use panoply_ocr::{EngineKind, Ocr, cer};

fn cer_of(ocr: &Ocr, id: &str, langs: &str) -> Option<f64> {
    let page = read(ocr, id, langs)?;
    let c = cer::cer(&label(id), &page.text());
    eprintln!("{id}: CER {:.2}% via {}", c * 100.0, page.engine);
    Some(c)
}

#[test]
fn ppocr_reads_clean_english_text() {
    let ocr = Ocr::only(EngineKind::PpOcr);
    for id in ["en-1", "en-2", "en-3"] {
        if let Some(c) = cer_of(&ocr, id, "en") {
            assert!(c < 0.03, "{id}: {c}");
        }
    }
}

#[test]
fn ppocr_reads_clean_hindi_and_mixed_hindi_english() {
    let ocr = Ocr::only(EngineKind::PpOcr);
    for (id, l) in [
        ("hi-1", "hi"),
        ("hi-2", "hi"),
        ("hi-3", "hi"),
        ("hien-1", "hi,en"),
    ] {
        if let Some(c) = cer_of(&ocr, id, l) {
            assert!(c < 0.05, "{id}: {c}");
        }
    }
}

#[test]
fn ppocr_reads_a_crooked_noisy_scan() {
    let ocr = Ocr::only(EngineKind::PpOcr);
    if let Some(c) = cer_of(&ocr, "en-2-scan", "en") {
        assert!(c < 0.06, "{c}");
    }
    if let Some(c) = cer_of(&ocr, "hi-1-scan", "hi") {
        assert!(c < 0.08, "{c}");
    }
}

#[test]
fn word_boxes_sit_on_the_words() {
    let ocr = Ocr::only(EngineKind::PpOcr);
    let Some(page) = read(&ocr, "en-1", "en") else {
        return;
    };
    // Every word box is inside the image, and the words of a line run left to right without
    // overlapping.
    for line in &page.lines {
        let mut right = f32::MIN;
        for w in &line.words {
            assert!(
                w.bbox.x >= -2.0 && w.bbox.right() <= page.width as f32 + 2.0,
                "{w:?}"
            );
            assert!(
                w.bbox.y >= -2.0 && w.bbox.bottom() <= page.height as f32 + 2.0,
                "{w:?}"
            );
            assert!(w.bbox.x >= right - 1.0, "{w:?} overlaps the previous word");
            right = w.bbox.right();
            assert!(w.confidence > 0.0 && w.confidence <= 1.0);
        }
    }
    // The first word "The" starts at the left margin (90 px) of the rendering.
    let first = &page.lines[0].words[0];
    assert_eq!(first.text, "The");
    assert!((first.bbox.x - 90.0).abs() < 25.0, "{first:?}");
}

#[cfg(target_os = "macos")]
#[test]
fn apple_vision_reads_english_and_finds_nothing_in_hindi() {
    let ocr = Ocr::only(EngineKind::AppleVision);
    let c = cer_of(&ocr, "en-1", "en").unwrap();
    assert!(c < 0.02, "{c}");
    // Measured: Vision has no Hindi, so a Hindi page comes back empty with no language chosen,
    // and asking for Hindi is refused up front.
    let page = read(&ocr, "hi-1", "").unwrap();
    assert_eq!(page.word_count(), 0);
    assert!(ocr.candidates(&langs("hi")).is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn automatic_mode_falls_through_to_devanagari_when_vision_finds_nothing() {
    let ocr = Ocr::new();
    let Some(page) = read(&ocr, "hi-1", "") else {
        return;
    };
    assert_eq!(page.engine, EngineKind::PpOcr);
    let c = cer::cer(&label("hi-1"), &page.text());
    assert!(c < 0.05, "{c}");
}

#[test]
fn bengali_goes_to_tesseract_when_it_is_installed() {
    let ocr = Ocr::new();
    let c = ocr.candidates(&langs("bn"));
    if panoply_ocr::engine::tesseract::program().is_none() {
        assert!(c.is_empty());
        return;
    }
    assert_eq!(c, vec![EngineKind::Tesseract]);
    if let Some(cer) = cer_of(&ocr, "bn-1", "bn") {
        assert!(cer < 0.06, "{cer}");
    }
}

#[test]
fn a_tall_page_read_in_strips_loses_no_lines() {
    // Three paragraphs stacked into one page: tall enough that the detector runs in several
    // overlapping strips, and the lines on the strip borders must come out once, whole.
    let ids = ["en-1", "en-2", "en-3"];
    let imgs: Vec<_> = ids.iter().map(|i| image(i)).collect();
    let w = imgs[0].width;
    assert!(imgs.iter().all(|i| i.width == w && i.channels == 3));
    let mut data = Vec::new();
    let mut h = 0;
    for i in &imgs {
        data.extend_from_slice(&i.data);
        h += i.height;
    }
    let page = panoply_ocr::OcrImage::rgb(w, h, data).unwrap();
    let ocr = Ocr::only(EngineKind::PpOcr);
    let Some(read) =
        with_fetch(|f| ocr.recognize(&page, &langs("en"), f, &panoply_core::CancelToken::new()))
    else {
        return;
    };
    let reference: String = ids.iter().map(|i| label(i)).collect::<Vec<_>>().join("\n");
    let c = cer::cer(&reference, &read.text());
    assert!(c < 0.03, "{c}\n{}", read.text());
    // Reading order is top to bottom.
    let ys: Vec<f32> = read.lines.iter().map(|l| l.bbox.y).collect();
    assert!(ys.windows(2).all(|p| p[0] <= p[1]), "{ys:?}");
}
