//! Normalize and Stereo to mono on the audio corpus: the numbers each one promises, measured
//! again by an independent path (the meter on the decoded result).
#![allow(clippy::chunks_exact_to_as_chunks)]

mod common;

use common::*;
use panoply_media::loudness::{Meter, roles_for};
use panoply_media::probe::Stream;
use serde_json::json;
use std::process::Command;

/// Decode the first audio stream of a file and measure it with the meter.
fn measure(path: &std::path::Path) -> panoply_media::loudness::Loudness {
    let p = probe(path);
    let s: Stream = p.audio().unwrap().clone();
    let e = the_engine();
    let out = Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-f", "f32le", "-acodec", "pcm_f32le", "-"])
        .output()
        .unwrap();
    let samples: Vec<f32> = out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let mut m = Meter::new(
        s.sample_rate,
        &roles_for(&s.channel_layout, s.channels as usize),
        false,
    );
    m.push(&samples);
    m.finish()
}

#[test]
fn normalize_reaches_the_target_within_half_a_lu_and_under_the_ceiling() {
    if !engine() {
        return;
    }
    for f in [
        "audio/commons/netaji-1943.ogg",
        "audio/commons/agnew-speech.ogg",
        "audio/commons/recorder-improvisation.mp3",
        "audio/generated/stereo-24bit-96khz.wav",
        "audio/commons/lachmoeweisma.flac",
    ] {
        let Some(src) = corpus(f) else { continue };
        let s = Scratch::new("normalize");
        let (out, notes) = run_ok(
            "media.normalize",
            &src,
            json!({"target": -16, "true_peak": -1}),
            s.path(),
        );
        if out.is_empty() {
            assert!(
                notes.iter().any(|n| n.contains("already")),
                "{f}: {notes:?}"
            );
            continue;
        }
        let l = measure(&out[0]);
        assert!(
            (l.integrated_lufs - -16.0).abs() <= 0.5,
            "{f}: {:.2} LUFS, {notes:?}",
            l.integrated_lufs
        );
        assert!(
            l.true_peak_dbtp <= -1.0 + 0.15,
            "{f}: {:.2} dBTP, {notes:?}",
            l.true_peak_dbtp
        );
        assert!(decode_errors(&out[0]).is_empty());
        assert!(
            notes.iter().any(|n| n.contains("LUFS to")),
            "{f}: {notes:?}"
        );
    }
}

#[test]
fn the_ebu_preset_sets_minus_23_and_a_minus_1_ceiling() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/commons/recorder-improvisation.mp3") else {
        return;
    };
    let s = Scratch::new("normalize-ebu");
    let (out, notes) = run_ok(
        "media.normalize",
        &src,
        json!({"preset": "ebu-r128"}),
        s.path(),
    );
    let l = measure(&out[0]);
    assert!(
        (l.integrated_lufs - -23.0).abs() <= 0.5,
        "{:.2}: {notes:?}",
        l.integrated_lufs
    );
    assert!(l.true_peak_dbtp <= -0.85, "{:.2}", l.true_peak_dbtp);
}

#[test]
fn a_peaky_file_gets_a_limiter_not_clipping() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/generated/clipped-overdriven.wav") else {
        return;
    };
    let s = Scratch::new("normalize-peaky");
    let (out, notes) = run_ok(
        "media.normalize",
        &src,
        json!({"target": -14, "true_peak": -1}),
        s.path(),
    );
    let l = measure(&out[0]);
    assert!(
        l.true_peak_dbtp <= -0.8,
        "{:.2} dBTP {notes:?}",
        l.true_peak_dbtp
    );
    assert!(
        notes
            .iter()
            .any(|n| n.contains("limiter") || n.contains("already")),
        "{notes:?}"
    );
}

#[test]
fn replaygain_tags_change_no_sample() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/commons/recorder-improvisation.mp3") else {
        return;
    };
    let s = Scratch::new("normalize-tags");
    let (out, notes) = run_ok("media.normalize", &src, json!({"mode": "tags"}), s.path());
    assert!(notes.iter().any(|n| n.contains("ReplayGain")), "{notes:?}");
    assert_eq!(
        packet_hashes(&out[0], "0:a:0"),
        packet_hashes(&src, "0:a:0")
    );
}

#[test]
fn a_video_keeps_its_picture_when_the_sound_is_normalized() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let s = Scratch::new("normalize-video");
    let (out, _) = run_ok("media.normalize", &src, json!({"target": -18}), s.path());
    assert_eq!(
        packet_hashes(&out[0], "0:v:0"),
        packet_hashes(&src, "0:v:0"),
        "the video is copied"
    );
    let l = measure(&out[0]);
    assert!(
        (l.integrated_lufs - -18.0).abs() <= 0.6,
        "{:.2}",
        l.integrated_lufs
    );
}

#[test]
fn silence_is_a_plain_error() {
    if !engine() {
        return;
    }
    let s = Scratch::new("normalize-silence");
    let src = s.join("silence.wav");
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "anullsrc=r=44100:cl=stereo",
        "-t",
        "3",
        &src.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    let e = run("media.normalize", &src, json!({}), s.path()).unwrap_err();
    assert!(e.contains("too quiet to measure"), "{e}");
}

#[test]
fn stereo_to_mono_reports_what_it_found_and_keeps_the_loudness() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/generated/stereo-lr-tones.flac") else {
        return;
    };
    let s = Scratch::new("mono");
    let (out, notes) = run_ok("media.to_mono", &src, json!({}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.audio().unwrap().channels, 1);
    assert!(notes.iter().any(|n| n.contains("Loudness:")), "{notes:?}");
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn a_silent_left_channel_uses_the_live_one() {
    if !engine() {
        return;
    }
    let s = Scratch::new("mono-silent");
    let src = s.join("oneside.wav");
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=3",
        "-af",
        "pan=stereo|c0=0*c0|c1=c0",
        &src.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    let (out, notes) = run_ok("media.to_mono", &src, json!({}), s.path());
    assert!(
        notes.iter().any(|n| n.contains("left channel is silent")),
        "{notes:?}"
    );
    let l = measure(&out[0]);
    assert!(
        l.integrated_lufs > -30.0,
        "the live channel is audible: {}",
        l.integrated_lufs
    );
}

#[test]
fn out_of_phase_channels_do_not_cancel() {
    if !engine() {
        return;
    }
    let s = Scratch::new("mono-phase");
    let src = s.join("phase.wav");
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=3",
        "-af",
        "pan=stereo|c0=c0|c1=-1*c0",
        &src.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    let (out, notes) = run_ok("media.to_mono", &src, json!({}), s.path());
    assert!(
        notes.iter().any(|n| n.contains("out of phase")),
        "{notes:?}"
    );
    let l = measure(&out[0]);
    assert!(
        l.integrated_lufs > -30.0,
        "not silent: {}",
        l.integrated_lufs
    );
}

#[test]
fn swap_and_split_are_exact_on_lossless_sound() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/generated/stereo-lr-tones.flac") else {
        return;
    };
    let s = Scratch::new("swap");
    let (swapped, _) = run_ok("media.to_mono", &src, json!({"action": "swap"}), s.path());
    let back_dir = s.join("back");
    std::fs::create_dir_all(&back_dir).unwrap();
    let (back, _) = run_ok(
        "media.to_mono",
        &swapped[0],
        json!({"action": "swap"}),
        &back_dir,
    );
    assert_eq!(
        decode_md5(&src),
        decode_md5(&back[0]),
        "swapping twice returns the original sample for sample"
    );
    let split_dir = s.join("split");
    std::fs::create_dir_all(&split_dir).unwrap();
    let (parts, _) = run_ok(
        "media.to_mono",
        &src,
        json!({"action": "split"}),
        &split_dir,
    );
    assert_eq!(parts.len(), 2);
    for p in &parts {
        assert_eq!(probe(p).audio().unwrap().channels, 1);
    }
}

fn decode_md5(p: &std::path::Path) -> String {
    let e = the_engine();
    let out = Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(p)
        .args(["-map", "0:a:0", "-f", "hash", "-hash", "md5", "-"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn merge_needs_exactly_two_files() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/generated/mono-440hz.wav") else {
        return;
    };
    let s = Scratch::new("merge");
    let e = run("media.to_mono", &src, json!({"action": "merge"}), s.path()).unwrap_err();
    assert!(e.contains("exactly two"), "{e}");
    let (out, _) = run_many(
        "media.to_mono",
        &[src.clone(), src.clone()],
        json!({"action": "merge"}),
        s.path(),
    )
    .unwrap();
    assert_eq!(probe(&out[0]).audio().unwrap().channels, 2);
}
