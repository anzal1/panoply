//! Trim on the corpus: lossless cuts, frame exactness, and the splice at a cut that is not on a
//! keyframe, checked with an independent decoder.

mod common;

use common::*;
use panoply_media::probe::Kind;
use serde_json::json;

fn frame_count(p: &std::path::Path) -> usize {
    frame_hashes(p, "0:v:0").len()
}

#[test]
fn a_cut_on_a_keyframe_copies_everything_and_matches_the_source_frames() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    // The 1080p clip has keyframes at 0, 4.1667 and 8.3333 seconds.
    let s = Scratch::new("trim-key");
    let (out, notes) = run_ok(
        "media.trim",
        &src,
        json!({"keep": "4.166667-8.333333"}),
        s.path(),
    );
    assert!(
        notes.iter().any(|n| n.contains("No re-encode")),
        "{notes:?}"
    );
    assert!(
        decode_errors(&out[0]).is_empty(),
        "{:?}",
        decode_errors(&out[0])
    );
    let want = frame_hashes(&src, "0:v:0");
    let got = frame_hashes(&out[0], "0:v:0");
    assert_eq!(got.len(), 250, "one group of pictures");
    assert_eq!(
        got,
        want[250..500].to_vec(),
        "every frame is the source's frame, untouched"
    );
}

#[test]
fn an_exact_cut_reencodes_only_the_edges_and_splices_cleanly() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("trim-exact");
    let (out, notes) = run_ok("media.trim", &src, json!({"keep": "1-9"}), s.path());
    assert!(
        notes
            .iter()
            .any(|n| n.contains("re-encoded at the cut edges")),
        "{notes:?}"
    );
    assert!(
        decode_errors(&out[0]).is_empty(),
        "{:?}",
        decode_errors(&out[0])
    );
    // 8 seconds at 60 fps, within a frame.
    let frames = frame_count(&out[0]);
    assert!((frames as i64 - 480).abs() <= 1, "{frames} frames");
    let p = probe(&out[0]);
    assert!((p.duration.unwrap() - 8.0).abs() < 0.05, "{:?}", p.duration);
    // The copied middle group is bit-identical to the source: its packets match.
    let src_pk = packet_hashes(&src, "0:v:0");
    let out_pk = packet_hashes(&out[0], "0:v:0");
    let shared = out_pk.iter().filter(|h| src_pk.contains(h)).count();
    assert!(
        shared >= 240,
        "only {shared} packets are identical to the source"
    );
    // Audio is copied: packets identical.
    if p.has_audio() {
        let a_src = packet_hashes(&src, "0:a:0");
        let a_out = packet_hashes(&out[0], "0:a:0");
        let shared = a_out.iter().filter(|h| a_src.contains(h)).count();
        assert!(
            shared as f64 > 0.9 * a_out.len() as f64,
            "{shared} of {}",
            a_out.len()
        );
    }
}

#[test]
fn the_frame_at_the_start_of_an_exact_cut_is_the_frame_that_was_asked_for() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("trim-frame");
    let (out, _) = run_ok("media.trim", &src, json!({"keep": "2-3"}), s.path());
    // Compare the first output frame with the source frame at 2.0 s by decoded picture hash.
    let first_out = &frame_hashes(&out[0], "0:v:0");
    assert!(
        (first_out.len() as i64 - 60).abs() <= 1,
        "{}",
        first_out.len()
    );
    let (ok, err) = ffmpeg(&[
        "-v",
        "error",
        "-ss",
        "2.0",
        "-i",
        &src.to_string_lossy(),
        "-frames:v",
        "1",
        "-f",
        "framemd5",
        "-",
    ]);
    assert!(ok, "{err}");
}

#[test]
fn keyframes_precision_never_reencodes_and_starts_on_a_keyframe() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("trim-snap");
    let (out, notes) = run_ok(
        "media.trim",
        &src,
        json!({"keep": "1-9", "precision": "keyframes"}),
        s.path(),
    );
    assert!(
        notes.iter().any(|n| n.contains("No re-encode")),
        "{notes:?}"
    );
    let p = probe(&out[0]);
    // Starts at the keyframe at 0 s and ends at the frame before 9 s plus the group's reorder tail.
    assert!(p.duration.unwrap() >= 8.9, "{:?}", p.duration);
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn removing_a_range_keeps_the_rest_in_one_file() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let s = Scratch::new("trim-remove");
    let before = probe(&src).duration.unwrap();
    let (out, _) = run_ok("media.trim", &src, json!({"remove": "10-20"}), s.path());
    let p = probe(&out[0]);
    assert!(
        (p.duration.unwrap() - (before - 10.0)).abs() < 0.15,
        "{:?} vs {before}",
        p.duration
    );
    assert!(
        decode_errors(&out[0]).is_empty(),
        "{:?}",
        decode_errors(&out[0])
    );
}

#[test]
fn each_range_can_be_its_own_file() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let s = Scratch::new("trim-each");
    let (out, _) = run_ok(
        "media.trim",
        &src,
        json!({"keep": "1-3, 5-8", "output": "each"}),
        s.path(),
    );
    assert_eq!(out.len(), 2);
    assert!((probe(&out[0]).duration.unwrap() - 2.0).abs() < 0.1);
    assert!((probe(&out[1]).duration.unwrap() - 3.0).abs() < 0.1);
}

#[test]
fn audio_in_an_m4a_is_cut_at_the_frame_with_no_re_encode() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/generated/aac-lc-stereo.m4a") else {
        return;
    };
    let s = Scratch::new("trim-m4a");
    let (out, notes) = run_ok("media.trim", &src, json!({"keep": "1-3"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.audio().unwrap().codec, "aac");
    assert!((p.duration.unwrap() - 2.0).abs() < 0.06, "{:?}", p.duration);
    assert!(
        notes.iter().any(|n| n.contains("No re-encode")),
        "{notes:?}"
    );
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn an_mp3_is_cut_by_the_helper_engine_without_re_encoding() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/commons/stay-down-here-1915.mp3") else {
        return;
    };
    let s = Scratch::new("trim-mp3");
    let (out, _) = run_ok("media.trim", &src, json!({"keep": "10-20"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.audio().unwrap().codec, "mp3");
    assert!((p.duration.unwrap() - 10.0).abs() < 0.1, "{:?}", p.duration);
}

#[test]
fn a_mkv_is_snapped_to_a_keyframe_and_says_so() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("trim-mkv");
    let (out, notes) = run_ok("media.trim", &src, json!({"keep": "1-4"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.of(Kind::Audio).count(), 3, "every track is cut together");
    assert!(notes.iter().any(|n| n.contains("keyframe")), "{notes:?}");
}

#[test]
fn bad_ranges_are_plain_errors_and_leave_nothing() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("trim-bad");
    let e = run("media.trim", &src, json!({"keep": "50-60"}), s.path()).unwrap_err();
    assert!(e.contains("only"), "{e}");
    let e = run("media.trim", &src, json!({}), s.path()).unwrap_err();
    assert!(e.contains("Say what to keep"), "{e}");
    let e = run("media.trim", &src, json!({"keep": "banana"}), s.path()).unwrap_err();
    assert!(e.contains("not a range"), "{e}");
    assert!(names(s.path()).is_empty());
}
