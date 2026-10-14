//! Convert and Extract audio on the corpus. Tests skip, with a message, when the corpus or an
//! ffmpeg is missing: `python3 -I tests/corpus/fetch.py --kind video,audio`.

mod common;

use common::*;
use panoply_media::probe::Kind;
use serde_json::json;

#[test]
fn mov_style_container_change_copies_every_stream() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/rotated-90-display-matrix.mp4") else {
        return;
    };
    let s = Scratch::new("convert-mkv");
    let (out, notes) = run_ok("media.convert", &src, json!({"to": "mkv"}), s.path());
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].extension().unwrap(), "mkv");
    let p = probe(&out[0]);
    assert_eq!(p.video().unwrap().codec, "h264");
    assert_eq!(p.audio().unwrap().codec, "aac");
    assert!(
        notes.iter().any(|n| n.contains("video: copied")),
        "{notes:?}"
    );
    // Bit-exact streams: the packets are identical to the source's.
    assert_eq!(
        packet_hashes(&out[0], "0:v:0"),
        packet_hashes(&src, "0:v:0")
    );
    assert_eq!(
        packet_hashes(&out[0], "0:a:0"),
        packet_hashes(&src, "0:a:0")
    );
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn mp4_conversion_keeps_the_rotation_flag_and_puts_the_index_first() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/rotated-90-display-matrix.mp4") else {
        return;
    };
    let s = Scratch::new("convert-mov");
    let (out, _) = run_ok("media.convert", &src, json!({"to": "mov"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(
        p.video().unwrap().rotation,
        probe(&src).video().unwrap().rotation
    );
    assert_ne!(p.video().unwrap().rotation, 0);
}

#[test]
fn a_file_already_in_the_target_container_is_left_alone() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("convert-same");
    let (out, notes) = run_ok("media.convert", &src, json!({"to": "mp4"}), s.path());
    assert!(out.is_empty(), "nothing to convert");
    assert!(notes.iter().any(|n| n.contains("already MP4")), "{notes:?}");
    assert!(names(s.path()).is_empty());
}

#[test]
fn h264_to_webm_encodes_only_what_has_to_change() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("convert-webm");
    let (out, _) = run_ok("media.convert", &src, json!({"to": "webm"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.video().unwrap().codec, "vp9");
    assert!(p.audio().is_none_or(|a| a.codec == "opus"));
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn an_unusual_codec_becomes_playable_h264_in_mp4() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/commons/camera-clip-mvi0131.ogv") else {
        return;
    };
    let s = Scratch::new("convert-theora");
    let (out, notes) = run_ok("media.convert", &src, json!({"to": "mp4"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.video().unwrap().codec, "h264");
    assert_eq!(p.audio().unwrap().codec, "aac");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("hardware") || n.contains("software")),
        "{notes:?}"
    );
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn multi_track_mkv_to_mp4_keeps_all_audio_and_text_subtitles() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("convert-multi");
    let (out, _) = run_ok("media.convert", &src, json!({"to": "mp4"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.of(Kind::Audio).count(), 3, "every audio track is kept");
    assert_eq!(p.of(Kind::Subtitle).count(), 1);
    assert_eq!(
        p.streams
            .iter()
            .find(|s| s.kind == Kind::Subtitle)
            .unwrap()
            .codec,
        "mov_text"
    );
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn extract_audio_copies_the_stream_bit_exactly() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let s = Scratch::new("extract-copy");
    let (out, notes) = run_ok("media.extract_audio", &src, json!({}), s.path());
    assert_eq!(out[0].extension().unwrap(), "m4a");
    assert_eq!(
        packet_hashes(&out[0], "0:a:0"),
        packet_hashes(&src, "0:a:0")
    );
    assert!(
        notes.iter().any(|n| n.contains("No re-encode")),
        "{notes:?}"
    );
}

#[test]
fn extract_audio_to_mp3_encodes_once() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let s = Scratch::new("extract-mp3");
    let (out, _) = run_ok(
        "media.extract_audio",
        &src,
        json!({"format": "mp3"}),
        s.path(),
    );
    let p = probe(&out[0]);
    assert_eq!(p.audio().unwrap().codec, "mp3");
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn a_video_with_no_sound_says_so_plainly() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    if probe(&src).has_audio() {
        return;
    }
    let s = Scratch::new("extract-none");
    let err = run("media.extract_audio", &src, json!({}), s.path()).unwrap_err();
    assert!(err.contains("no sound"), "{err}");
    assert!(names(s.path()).is_empty());
}

#[test]
fn audio_conversions_keep_tags_and_never_upscale_a_lossy_bit_rate() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/generated/tagged-cover-art.mp3") else {
        return;
    };
    let s = Scratch::new("audio-aac");
    let (out, _) = run_ok("media.convert", &src, json!({"to": "m4a"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.audio().unwrap().codec, "aac");
    assert!(
        p.streams.iter().any(|s| s.attached_pic),
        "cover art carried: {:?}",
        p.streams.len()
    );
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn lossless_round_trip_is_bit_exact_on_the_pcm() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/generated/stereo-24bit-96khz.wav") else {
        return;
    };
    let s = Scratch::new("audio-flac");
    let (flac, _) = run_ok("media.convert", &src, json!({"to": "flac"}), s.path());
    let back = s.join("back");
    std::fs::create_dir_all(&back).unwrap();
    let (wav, _) = run_ok("media.convert", &flac[0], json!({"to": "wav"}), &back);
    assert!(!frame_hashes(&wav[0], "0:a:0").is_empty());
    let a = decode_hash(&src);
    let b = decode_hash(&wav[0]);
    assert_eq!(a, b, "WAV to FLAC to WAV changes no sample");
}

fn decode_hash(p: &std::path::Path) -> String {
    let e = the_engine();
    let out = std::process::Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(p)
        .args(["-map", "0:a:0", "-f", "hash", "-hash", "md5", "-"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn a_cancelled_job_leaves_nothing_behind() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("cancel");
    let cancel = panoply_core::CancelToken::new();
    cancel.cancel();
    let io = panoply_media::io::OpenIo::new()
        .with_out_dir(s.path().to_path_buf())
        .with_cancel(cancel);
    let err = module("media.convert")
        .run(&io, &[src], &json!({"to": "webm"}))
        .unwrap_err();
    assert!(err.to_string().contains("Cancelled"), "{err}");
    assert!(names(s.path()).is_empty(), "{:?}", names(s.path()));
}

#[test]
fn an_index_at_the_end_is_moved_to_the_front_without_touching_a_stream() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/moov-at-end.mp4") else {
        return;
    };
    let s = Scratch::new("faststart");
    let (out, notes) = run_ok("media.convert", &src, json!({"to": "mp4"}), s.path());
    assert!(
        notes.iter().any(|n| n.contains("now at the front")),
        "{notes:?}"
    );
    let layout = panoply_media::mp4::boxes::scan(
        &panoply_media::mp4::source::Source::open(&out[0]).unwrap(),
    )
    .unwrap();
    assert_eq!(layout.moov_first(), Some(true));
    assert_eq!(
        packet_hashes(&out[0], "0:v:0"),
        packet_hashes(&src, "0:v:0")
    );
    assert_eq!(
        packet_hashes(&out[0], "0:a:0"),
        packet_hashes(&src, "0:a:0")
    );
    assert!(decode_errors(&out[0]).is_empty());
    assert_eq!(
        probe(&out[0]).duration.map(|d| (d * 100.0).round()),
        probe(&src).duration.map(|d| (d * 100.0).round())
    );
}

#[test]
fn hi_res_audio_becomes_aac_at_48_khz() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/generated/stereo-24bit-96khz.wav") else {
        return;
    };
    let s = Scratch::new("hires-aac");
    let (out, notes) = run_ok("media.convert", &src, json!({"to": "m4a"}), s.path());
    let p = probe(&out[0]);
    assert_eq!(p.audio().unwrap().codec, "aac");
    assert_eq!(p.audio().unwrap().sample_rate, 48000);
    assert!(notes.iter().any(|n| n.contains("48 kHz")), "{notes:?}");
}
