//! Streams on the multi-track MKV: every kept stream stays bit-identical.

mod common;

use common::*;
use panoply_media::probe::Kind;
use serde_json::json;

#[test]
fn with_no_option_it_lists_the_streams() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("streams-list");
    let (out, notes) = run_ok("media.streams", &src, json!({}), s.path());
    assert!(out.is_empty());
    assert!(
        notes.iter().any(|n| n.contains("stream 3: audio")),
        "{notes:?}"
    );
}

#[test]
fn dropping_a_track_keeps_the_others_bit_identical() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("streams-drop");
    let (out, _) = run_ok("media.streams", &src, json!({"drop": "2"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.of(Kind::Audio).count(), 2);
    assert_eq!(
        packet_hashes(&out[0], "0:v:0"),
        packet_hashes(&src, "0:v:0")
    );
    assert_eq!(
        packet_hashes(&out[0], "0:a:0"),
        packet_hashes(&src, "0:a:0")
    );
    assert_eq!(
        packet_hashes(&out[0], "0:a:1"),
        packet_hashes(&src, "0:a:2")
    );
}

#[test]
fn extracting_writes_each_stream_in_a_folder_named_after_the_source() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("streams-extract");
    let (out, _) = run_ok("media.streams", &src, json!({"extract": "1,4"}), s.path());
    assert_eq!(out.len(), 2, "{out:?}");
    let dir = out[0].parent().unwrap();
    assert!(
        dir.file_name()
            .unwrap()
            .to_string_lossy()
            .contains("(streams)")
    );
    let names: Vec<_> = out
        .iter()
        .map(|p| p.extension().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.contains(&"m4a".to_string()) && names.contains(&"srt".to_string()),
        "{names:?}"
    );
    assert_eq!(
        packet_hashes(&out[0], "0:a:0"),
        packet_hashes(&src, "0:a:0")
    );
}

#[test]
fn a_default_track_and_a_language_can_be_set() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("streams-default");
    let (out, _) = run_ok(
        "media.streams",
        &src,
        json!({"default": "3", "language": "2=deu"}),
        s.path(),
    );
    let p = probe(&out[0]);
    let audio: Vec<_> = p.of(Kind::Audio).collect();
    assert!(
        audio[2].default && !audio[0].default,
        "{:?}",
        audio.iter().map(|a| a.default).collect::<Vec<_>>()
    );
    assert_eq!(audio[1].language, "deu");
}

#[test]
fn bad_stream_numbers_are_plain_errors() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("streams-bad");
    let e = run("media.streams", &src, json!({"drop": "9"}), s.path()).unwrap_err();
    assert!(e.contains("no stream 9"), "{e}");
    let e = run(
        "media.streams",
        &src,
        json!({"drop": "0,1,2,3,4"}),
        s.path(),
    )
    .unwrap_err();
    assert!(
        e.contains("nothing would be left") || e.contains("no stream"),
        "{e}"
    );
    assert!(names(s.path()).is_empty());
}
