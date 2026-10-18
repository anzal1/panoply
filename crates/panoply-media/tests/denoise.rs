//! Remove noise on generated files: the noise really goes down, the picture of a video is copied
//! bit for bit, the sound keeps its rate and channels, cancel leaves nothing behind, and the speed
//! on a 60 second clip is reported as a multiple of real time.
#![allow(clippy::chunks_exact_to_as_chunks)]

mod common;

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use common::*;
use panoply_core::CancelToken;
use panoply_media::io::OpenIo;
use serde_json::json;

/// A voice-like tone (140 Hz and harmonics with a 4 Hz rhythm) over white noise, as a sound
/// source for ffmpeg's `lavfi`.
fn voice_over_noise(seconds: u32, rate: u32) -> String {
    format!(
        "aevalsrc='0.25*(0.5+0.5*pow(sin(2*PI*4*t),2))*(sin(2*PI*140*t)+0.6*sin(2*PI*280*t)+0.4*sin(2*PI*420*t)+0.3*sin(2*PI*700*t))':s={rate}:d={seconds}[v];anoisesrc=r={rate}:a=0.03:c=white:s=2:d={seconds}[n];[v][n]amix=inputs=2:normalize=0"
    )
}

/// Noise for all of `seconds`, and the voice only from second 3 to second 7, so seconds 0.5 to
/// 2.5 are noise alone.
fn gated_voice_over_noise(seconds: u32, rate: u32) -> String {
    format!(
        "aevalsrc='between(t,3,7)*0.25*(0.5+0.5*pow(sin(2*PI*4*t),2))*(sin(2*PI*140*t)+0.6*sin(2*PI*280*t)+0.4*sin(2*PI*420*t)+0.3*sin(2*PI*700*t))':s={rate}:d={seconds}[v];anoisesrc=r={rate}:a=0.05:c=white:s=1:d={seconds}[n];[v][n]amix=inputs=2:normalize=0"
    )
}

fn make_audio(path: &Path, graph: &str, rate: u32, channels: u32) {
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        graph,
        "-ar",
        &rate.to_string(),
        "-ac",
        &channels.to_string(),
        &path.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
}

/// RMS of the first audio stream, from 0.5 s on, decoded to mono 48 kHz by ffmpeg.
fn rms_of(path: &Path) -> f64 {
    rms_between(path, 0.5, f64::MAX)
}

/// RMS of the first audio stream between two times in seconds.
fn rms_between(path: &Path, from: f64, to: f64) -> f64 {
    let e = the_engine();
    let out = Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-ac",
            "1",
            "-ar",
            "48000",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-",
        ])
        .output()
        .unwrap();
    let all: Vec<f32> = out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let a = ((from * 48_000.0) as usize).min(all.len());
    let b = if to >= 1e9 {
        all.len()
    } else {
        ((to * 48_000.0) as usize).min(all.len())
    };
    let s = &all[a..b];
    (s.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / s.len().max(1) as f64).sqrt()
}

#[test]
fn a_video_keeps_its_picture_bit_for_bit_and_its_sound_loses_the_noise() {
    if !engine() {
        return;
    }
    let s = Scratch::new("denoise-video");
    let src = s.join("clip.mp4");
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=320x240:rate=25:duration=9",
        "-f",
        "lavfi",
        "-i",
        &gated_voice_over_noise(9, 44_100),
        "-c:v",
        "mpeg4",
        "-c:a",
        "aac",
        "-shortest",
        &src.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    let (out, notes) = run_ok("media.denoise", &src, json!({}), s.path());
    assert_eq!(out.len(), 1, "{notes:?}");
    assert!(
        out[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("(clean)"),
        "{out:?}"
    );
    assert_eq!(
        packet_hashes(&out[0], "0:v:0"),
        packet_hashes(&src, "0:v:0"),
        "the video stream is copied, packet for packet"
    );
    // Noise alone after the voice (7.6 to 8.9 s), measured on the decoded AAC before and after.
    let (before, after) = (rms_between(&src, 7.6, 8.9), rms_between(&out[0], 7.6, 8.9));
    let drop = 20.0 * (before / after).log10();
    eprintln!("video clip, noise only: {before:.5} to {after:.5} rms, down {drop:.1} dB");
    assert!(drop >= 8.0, "noise went down by only {drop:.1} dB");
    // The lead-in (0.5 to 2.5 s, noise before any voice) is printed, not asserted. On steady
    // synthetic white noise with no speech in it the model's answer swings from 1 to 16 dB with
    // the noise level and the noise itself; the unit tests assert the lead-in on a scene where
    // the voice comes in after two seconds.
    let (lb, la) = (rms_between(&src, 0.5, 2.5), rms_between(&out[0], 0.5, 2.5));
    eprintln!(
        "video clip, lead-in noise only: down {:.1} dB (not asserted)",
        20.0 * (lb / la).log10()
    );
    // The voice is still there at about the same level.
    let (vb, va) = (rms_between(&src, 4.0, 6.5), rms_between(&out[0], 4.0, 6.5));
    let vd = 20.0 * (va / vb).log10();
    eprintln!("video clip, voice segment: {vd:+.2} dB");
    assert!(vd.abs() <= 3.0, "voice moved by {vd:+.2} dB");
    // The sound is still an AAC track at its own rate and channels, the length is kept.
    let (pi, po) = (probe(&src), probe(&out[0]));
    let (ai, ao) = (pi.audio().unwrap(), po.audio().unwrap());
    assert_eq!(
        (ai.codec.as_str(), ai.sample_rate, ai.channels),
        (ao.codec.as_str(), ao.sample_rate, ao.channels)
    );
    assert!((pi.duration.unwrap() - po.duration.unwrap()).abs() < 0.3);
    assert!(decode_errors(&out[0]).is_empty());
    assert!(
        notes.iter().any(|n| n.contains("Noise floor")),
        "the card says what changed: {notes:?}"
    );
    assert!(notes.iter().any(|n| n.contains("picture was copied")));
}

#[test]
fn stereo_sound_at_44100_comes_back_stereo_at_44100_with_the_voice_in_place() {
    if !engine() {
        return;
    }
    let s = Scratch::new("denoise-wav");
    let src = s.join("talk.wav");
    make_audio(&src, &voice_over_noise(8, 44_100), 44_100, 2);
    for (strength, floor) in [("light", 3.0), ("normal", 0.0), ("strong", 0.0)] {
        let (out, notes) = run_ok(
            "media.denoise",
            &src,
            json!({"strength": strength}),
            s.path(),
        );
        let po = probe(&out[0]);
        let a = po.audio().unwrap();
        assert_eq!((a.sample_rate, a.channels), (44_100, 2), "{strength}");
        let (before, after) = (rms_of(&src), rms_of(&out[0]));
        let diff = 20.0 * (after / before).log10();
        eprintln!("{strength}: level {diff:+.1} dB, {notes:?}");
        // The voice dominates the level, so the whole file stays within a few dB.
        assert!(diff.abs() <= 3.0 + floor, "{strength}: {diff:+.1} dB");
        assert!(decode_errors(&out[0]).is_empty());
    }
}

/// RMS of one channel of the first audio stream (0.5 s on).
fn rms_channel(path: &Path, channel: u32) -> f64 {
    let e = the_engine();
    let out = Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-af",
            &format!("pan=mono|c0=c{channel}"),
            "-ar",
            "48000",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-",
        ])
        .output()
        .unwrap();
    let all: Vec<f32> = out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let s = &all[24_000.min(all.len())..];
    (s.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / s.len().max(1) as f64).sqrt()
}

#[test]
fn five_point_one_keeps_every_channel_on_its_own_lane() {
    if !engine() {
        return;
    }
    let s = Scratch::new("denoise-51");
    let src = s.join("surround.wav");
    // The voice on channel 3 (the low-frequency one in 5.1 order), silence on the other five.
    let voice = "0.25*(0.5+0.5*pow(sin(2*PI*4*t),2))*(sin(2*PI*140*t)+0.6*sin(2*PI*280*t)+0.4*sin(2*PI*420*t)+0.3*sin(2*PI*700*t))";
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        &format!("aevalsrc='0|0|0|{voice}|0|0':c=5.1:s=48000:d=8"),
        &src.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    let (out, notes) = run_ok("media.denoise", &src, json!({}), s.path());
    assert_eq!(probe(&out[0]).audio().unwrap().channels, 6, "{notes:?}");
    let want = rms_channel(&src, 3);
    let got = rms_channel(&out[0], 3);
    eprintln!("5.1: channel 3 {want:.4} to {got:.4}");
    assert!(
        (20.0 * (got / want).log10()).abs() <= 6.0,
        "the voice kept its channel"
    );
    for c in [0, 1, 2, 4, 5] {
        let other = rms_channel(&out[0], c);
        assert!(
            other < want / 100.0,
            "channel {c} picked up sound: {other:.5} against {want:.5}"
        );
    }
}

/// The time the voice starts on the container's clock: where the audio stream starts plus how far
/// into its samples the first loud 10 ms is.
fn voice_onset(path: &Path) -> f64 {
    let e = the_engine();
    let start = Command::new(&e.ffprobe)
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
        .arg("stream=start_time")
        .args(["-of", "csv=p=0"])
        .arg(path)
        .output()
        .unwrap();
    let start: f64 = String::from_utf8_lossy(&start.stdout)
        .trim()
        .parse()
        .unwrap_or(0.0);
    let out = Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-ac",
            "1",
            "-ar",
            "48000",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-",
        ])
        .output()
        .unwrap();
    let all: Vec<f32> = out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    let win = 480;
    let first = all
        .chunks(win)
        .position(|w| (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt() > 0.1)
        .expect("a voice in the sound");
    start + (first * win) as f64 / 48_000.0
}

#[test]
fn sound_that_starts_late_stays_in_sync_with_the_picture() {
    if !engine() {
        return;
    }
    let s = Scratch::new("denoise-offset");
    let src = s.join("late.mkv");
    // The sound starts half a second after the picture, as in a transport stream or an edit list.
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=160x120:rate=25:duration=9",
        "-itsoffset",
        "0.5",
        "-f",
        "lavfi",
        "-i",
        &gated_voice_over_noise(9, 48_000),
        "-c:v",
        "mpeg4",
        "-c:a",
        "aac",
        &src.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    let (out, notes) = run_ok("media.denoise", &src, json!({}), s.path());
    let (before, after) = (voice_onset(&src), voice_onset(&out[0]));
    eprintln!("voice starts at {before:.3} s in the original and {after:.3} s after: {notes:?}");
    assert!(
        (before - 3.5).abs() < 0.1,
        "the fixture starts the voice at 3.5 s, not {before:.3}"
    );
    assert!(
        (before - after).abs() < 0.06,
        "the sound moved by {:.3} s",
        after - before
    );
}

#[test]
fn a_decoder_that_cannot_go_on_is_stopped_at_once() {
    if !engine() {
        return;
    }
    use panoply_media::proc::{Progress, run_ffmpeg_stream_until};
    let s = Scratch::new("denoise-stop");
    let src = s.join("long.wav");
    make_audio(&src, &voice_over_noise(120, 48_000), 48_000, 2);
    let args: Vec<std::ffi::OsString> = ["-i", &src.to_string_lossy(), "-f", "f32le", "-"]
        .iter()
        .map(std::ffi::OsString::from)
        .collect();
    let mut pieces = 0;
    let t = Instant::now();
    let done = run_ffmpeg_stream_until(
        &the_engine(),
        &OpenIo::new(),
        &args,
        |_| {
            pieces += 1;
            false
        },
        &Progress::whole(None, "test"),
        None,
    )
    .unwrap();
    assert!(!done.ok);
    assert_eq!(pieces, 1, "the sink is not called again");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
}

#[test]
fn cancel_leaves_nothing_behind() {
    if !engine() {
        return;
    }
    let s = Scratch::new("denoise-cancel");
    let src = s.join("long.wav");
    make_audio(&src, &voice_over_noise(60, 48_000), 48_000, 2);
    let out_dir = s.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let token = CancelToken::new();
    let t = token.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        t.cancel();
    });
    let io = OpenIo::new()
        .with_out_dir(out_dir.clone())
        .with_cancel(token);
    let r = module("media.denoise").run(&io, &[src], &json!({}));
    stopper.join().unwrap();
    let e = r.expect_err("the job was cancelled").to_string();
    assert!(e.to_lowercase().contains("cancel"), "{e}");
    assert!(
        names(&out_dir).is_empty(),
        "left behind: {:?}",
        names(&out_dir)
    );
}

#[test]
fn a_file_without_sound_is_a_plain_error() {
    if !engine() {
        return;
    }
    let s = Scratch::new("denoise-nosound");
    let src = s.join("silent.mp4");
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=160x120:rate=10:duration=1",
        "-c:v",
        "mpeg4",
        &src.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    let e = run("media.denoise", &src, json!({}), s.path()).unwrap_err();
    assert!(e.contains("no sound"), "{e}");
}

#[test]
fn speed_on_a_60_second_clip() {
    if !engine() {
        return;
    }
    let s = Scratch::new("denoise-speed");
    let src = s.join("minute.wav");
    make_audio(&src, &voice_over_noise(60, 48_000), 48_000, 2);
    let io = OpenIo::new().with_out_dir(s.path().to_path_buf());
    let t = Instant::now();
    let out = module("media.denoise")
        .run(&io, std::slice::from_ref(&src), &json!({}))
        .unwrap();
    let took = t.elapsed().as_secs_f64();
    let times = 60.0 / took;
    eprintln!("SPEED: 60 s stereo clip in {took:.2} s = {times:.1}x real time");
    let events = io.progress_events();
    assert!(
        events.windows(2).all(|w| w[1].0 >= w[0].0 - 1e-6),
        "progress never goes back"
    );
    assert!(events.last().is_some_and(|e| e.0 >= 0.99), "{events:?}");
    assert!(out[0].is_file());
    assert!(times > 5.0, "{times:.1}x real time is too slow");
}
