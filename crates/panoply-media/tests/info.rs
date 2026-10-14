//! Info on the corpus: the problems each edge-case file should be flagged for, and the speed.

mod common;

use common::*;
use serde_json::json;
use std::time::Instant;

fn report(file: &str, opts: serde_json::Value) -> Option<Vec<String>> {
    let src = corpus(file)?;
    let s = Scratch::new("info");
    let (out, notes) = run_ok("media.info", &src, opts, s.path());
    assert!(out.is_empty(), "info writes no files");
    assert!(names(s.path()).is_empty());
    Some(notes)
}

fn has(notes: &[String], needle: &str) -> bool {
    notes.iter().any(|n| n.contains(needle))
}

#[test]
fn the_edge_case_files_are_flagged_for_what_is_wrong_with_them() {
    if !engine() {
        return;
    }
    if let Some(n) = report("video/generated/moov-at-end.mp4", json!({})) {
        assert!(has(&n, "index is at the end"), "{n:?}");
        assert!(has(&n, "media.convert"), "{n:?}");
    }
    if let Some(n) = report("video/generated/hdr10-hevc-10bit.mp4", json!({})) {
        assert!(has(&n, "HDR10"), "{n:?}");
        assert!(has(&n, "media.hdr_to_sdr"), "{n:?}");
    }
    if let Some(n) = report("video/generated/hlg-hevc-10bit.mp4", json!({})) {
        assert!(has(&n, "HLG"), "{n:?}");
    }
    if let Some(n) = report("video/generated/vfr-phone-portrait.mp4", json!({})) {
        assert!(has(&n, "frame rate varies"), "{n:?}");
    }
    if let Some(n) = report("video/generated/rotated-90-display-matrix.mp4", json!({})) {
        assert!(has(&n, "turned 270 degrees"), "{n:?}");
        assert!(has(&n, "shown as 1080 x 1920"), "{n:?}");
    }
    if let Some(n) = report("video/generated/h264-high10-10bit.mp4", json!({})) {
        assert!(has(&n, "10-bit"), "{n:?}");
    }
    if let Some(n) = report("video/generated/interlaced-mpeg2.ts", json!({})) {
        assert!(has(&n, "interlaced"), "{n:?}");
    }
    if let Some(n) = report("video/generated/multi-audio-3-tracks-subs.mkv", json!({})) {
        assert_eq!(
            n.iter().filter(|l| l.contains("Audio:")).count(),
            3,
            "{n:?}"
        );
        assert!(has(&n, "Subtitles:"), "{n:?}");
    }
    if let Some(n) = report("video/blender/bbb-360p-h264-10s.mp4", json!({})) {
        assert!(
            has(&n, "Nothing wrong was found") || has(&n, "What to fix"),
            "{n:?}"
        );
    }
}

#[test]
fn a_cut_off_file_is_reported_as_cut_off_with_a_way_forward() {
    if !engine() {
        return;
    }
    let Some(n) = report("video/generated/broken-moov-truncated.mp4", json!({})) else {
        return;
    };
    assert!(has(&n, "cannot be read"), "{n:?}");
    assert!(has(&n, "index: missing"), "{n:?}");
    assert!(has(&n, "media.repair"), "{n:?}");
}

#[test]
fn json_output_is_one_document_with_the_streams_and_problems() {
    if !engine() {
        return;
    }
    let Some(n) = report(
        "video/generated/hdr10-hevc-10bit.mp4",
        json!({"json": true}),
    ) else {
        return;
    };
    assert_eq!(n.len(), 1);
    let v: serde_json::Value = serde_json::from_str(&n[0]).expect("valid JSON");
    assert_eq!(v["streams"][0]["codec"], "hevc");
    assert!(
        v["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["fix"] == "media.hdr_to_sdr")
    );
}

#[test]
fn the_deep_scan_measures_loudness_and_clipping() {
    if !engine() {
        return;
    }
    let Some(n) = report(
        "audio/generated/clipped-overdriven.wav",
        json!({"deep": true}),
    ) else {
        return;
    };
    assert!(has(&n, "loudness:"), "{n:?}");
    assert!(has(&n, "clipped"), "{n:?}");
}

#[test]
fn the_report_comes_back_fast_whatever_the_size() {
    if !engine() {
        return;
    }
    // Warm the engine, then time the report on the 3 hour FLAC and the 1 hour video if present.
    for f in [
        "video/generated/1h-lowres.mp4",
        "audio/generated/3-hours-tone.flac",
        "video/generated/4k-hevc-5s.mp4",
    ] {
        let Some(src) = corpus(f) else { continue };
        let s = Scratch::new("info-fast");
        let _ = run_ok("media.info", &src, json!({}), s.path());
        let t = Instant::now();
        let _ = run_ok("media.info", &src, json!({}), s.path());
        let ms = t.elapsed().as_millis();
        assert!(ms < 1000, "{f}: {ms} ms");
    }
}
