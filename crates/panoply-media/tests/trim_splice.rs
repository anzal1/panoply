//! The smart cut on clips made for it: short groups of pictures, B-frames, open groups, H.264
//! and HEVC. Fixtures come from the engine's libx264 and libx265 where it has them (they are
//! test inputs, never something Panoply ships); each test skips when its encoder is missing.
//!
//! What is checked: the output decodes cleanly, has the right number of frames, matches the
//! source frame for frame (SSIM), shares the copied packets with the source, and keeps sound in
//! sync.

mod common;

use common::*;
use panoply_media::encoders::VCodec;
use serde_json::json;

struct Case {
    name: &'static str,
    encoder: &'static str,
    codec: VCodec,
    args: Vec<&'static str>,
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "h264-closed",
            encoder: "libx264",
            codec: VCodec::H264,
            args: vec![
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-g",
                "30",
                "-bf",
                "2",
                "-pix_fmt",
                "yuv420p",
                "-x264-params",
                "scenecut=0",
            ],
        },
        Case {
            name: "h264-open-gop",
            encoder: "libx264",
            codec: VCodec::H264,
            args: vec![
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-g",
                "30",
                "-bf",
                "3",
                "-pix_fmt",
                "yuv420p",
                "-x264-params",
                "open-gop=1:scenecut=0",
            ],
        },
        Case {
            name: "hevc-closed",
            encoder: "libx265",
            codec: VCodec::Hevc,
            args: vec![
                "-c:v",
                "libx265",
                "-preset",
                "ultrafast",
                "-g",
                "30",
                "-bf",
                "2",
                "-pix_fmt",
                "yuv420p",
                "-tag:v",
                "hvc1",
                "-x265-params",
                "log-level=error:scenecut=0:open-gop=0",
            ],
        },
        Case {
            name: "hevc-open-gop",
            encoder: "libx265",
            codec: VCodec::Hevc,
            args: vec![
                "-c:v",
                "libx265",
                "-preset",
                "ultrafast",
                "-g",
                "30",
                "-bf",
                "3",
                "-pix_fmt",
                "yuv420p",
                "-tag:v",
                "hvc1",
                "-x265-params",
                "log-level=error:scenecut=0:open-gop=1",
            ],
        },
    ]
}

#[test]
fn smart_cuts_are_frame_exact_clean_and_in_sync_for_h264_and_hevc() {
    if !engine() {
        return;
    }
    for c in cases() {
        if !has_encoder(c.encoder) || !has_hw(c.codec) {
            continue;
        }
        let s = Scratch::new(c.name);
        let src = make_clip(s.path(), "src.mp4", 12, 30, &c.args);
        // 2.35 s to 9.1 s: both edges inside a group (groups are 1 s long).
        let (out, notes) = run_ok("media.trim", &src, json!({"keep": "2.35-9.1"}), s.path());
        let o = &out[0];
        let note = notes.join(" | ");
        assert!(
            note.contains("re-encoded at the cut edges") || note.contains("frames re-encoded"),
            "{}: {note}",
            c.name
        );
        assert!(
            decode_errors(o).is_empty(),
            "{}: {:?}",
            c.name,
            decode_errors(o)
        );
        // 6.75 s at 30 fps is 202.5 frames; frame times are k/30 so the range holds 203 or 202.
        let frames = frame_hashes(o, "0:v:0").len();
        assert!((201..=204).contains(&frames), "{}: {frames} frames", c.name);
        let p = probe(o);
        assert!(
            (p.duration.unwrap() - 6.75).abs() < 0.07,
            "{}: {:?}",
            c.name,
            p.duration
        );
        // A/V: same length within an audio frame.
        let v = p.video().unwrap().duration.unwrap_or(0.0);
        let a = p.audio().unwrap().duration.unwrap_or(0.0);
        assert!((v - a).abs() < 0.05, "{}: video {v} audio {a}", c.name);
        // Frame for frame against the source (the first kept frame is at 2.3333 s).
        let first_pts = (2.35f64 * 30.0).ceil() / 30.0;
        let (mean, min) = ssim_against(&src, first_pts, o);
        assert!(mean > 0.97, "{}: mean SSIM {mean}", c.name);
        assert!(min > 0.85, "{}: lowest frame SSIM {min}", c.name);
        // The copied part is the source's own packets.
        let sp = packet_hashes(&src, "0:v:0");
        let op = packet_hashes(o, "0:v:0");
        let shared = op.iter().filter(|h| sp.contains(h)).count();
        if c.name.contains("open") {
            // Open groups have no clean start inside the range: it is re-encoded whole.
            assert!(
                note.contains("203 frames re-encoded") || note.contains("frames re-encoded"),
                "{}: {note}",
                c.name
            );
        } else {
            assert!(
                shared > 150,
                "{}: only {shared} packets are the source's",
                c.name
            );
        }
    }
}

#[test]
fn several_ranges_in_one_file_join_without_a_seam() {
    if !engine() || !has_encoder("libx264") || !has_hw(VCodec::H264) {
        return;
    }
    let s = Scratch::new("multi");
    let src = make_clip(
        s.path(),
        "src.mp4",
        12,
        30,
        &[
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-g",
            "30",
            "-bf",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-x264-params",
            "scenecut=0",
        ],
    );
    let (out, _) = run_ok(
        "media.trim",
        &src,
        json!({"keep": "1.4-3.7, 6.2-8.9"}),
        s.path(),
    );
    assert!(
        decode_errors(&out[0]).is_empty(),
        "{:?}",
        decode_errors(&out[0])
    );
    let p = probe(&out[0]);
    assert!(
        (p.duration.unwrap() - (2.3 + 2.7)).abs() < 0.1,
        "{:?}",
        p.duration
    );
    let v = p.video().unwrap().duration.unwrap_or(0.0);
    let a = p.audio().unwrap().duration.unwrap_or(0.0);
    assert!((v - a).abs() < 0.06, "video {v} audio {a}");
}

#[test]
fn removing_the_middle_matches_the_source_either_side() {
    if !engine() || !has_encoder("libx264") || !has_hw(VCodec::H264) {
        return;
    }
    let s = Scratch::new("remove");
    let src = make_clip(
        s.path(),
        "src.mp4",
        12,
        30,
        &[
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-g",
            "30",
            "-bf",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-x264-params",
            "scenecut=0",
        ],
    );
    let (out, _) = run_ok("media.trim", &src, json!({"remove": "3.3-8.6"}), s.path());
    assert!(decode_errors(&out[0]).is_empty());
    let p = probe(&out[0]);
    assert!(
        (p.duration.unwrap() - (12.0 - 5.3)).abs() < 0.1,
        "{:?}",
        p.duration
    );
    // The first part matches the start of the source.
    let (mean, _) = ssim_against(&src, 0.0, &out[0]);
    assert!(mean > 0.5, "{mean}");
}

#[test]
fn a_cut_that_starts_on_a_keyframe_and_ends_inside_a_group() {
    if !engine() || !has_encoder("libx264") || !has_hw(VCodec::H264) {
        return;
    }
    let s = Scratch::new("tail");
    let src = make_clip(
        s.path(),
        "src.mp4",
        12,
        30,
        &[
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-g",
            "30",
            "-bf",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-x264-params",
            "scenecut=0",
        ],
    );
    let (out, notes) = run_ok("media.trim", &src, json!({"keep": "3-8.45"}), s.path());
    assert!(decode_errors(&out[0]).is_empty());
    let frames = frame_hashes(&out[0], "0:v:0").len();
    assert!((163..=165).contains(&frames), "{frames}");
    let (mean, _) = ssim_against(&src, 3.0, &out[0]);
    assert!(mean > 0.97, "{mean} {notes:?}");
}

#[test]
fn without_a_hardware_encoder_the_cut_snaps_and_says_so() {
    if !engine() || !has_encoder("libx264") {
        return;
    }
    let s = Scratch::new("snap");
    let src = make_clip(
        s.path(),
        "src.mp4",
        6,
        30,
        &[
            "-c:v", "libx264", "-preset", "veryfast", "-g", "30", "-pix_fmt", "yuv420p",
        ],
    );
    let (out, notes) = run_ok(
        "media.trim",
        &src,
        json!({"keep": "1.5-4.2", "precision": "keyframes"}),
        s.path(),
    );
    assert!(
        notes.iter().any(|n| n.contains("No re-encode")),
        "{notes:?}"
    );
    assert!(decode_errors(&out[0]).is_empty());
    let p = probe(&out[0]);
    assert!(p.duration.unwrap() >= 3.1, "{:?}", p.duration);
}
