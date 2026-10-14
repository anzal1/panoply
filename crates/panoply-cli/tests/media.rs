//! End to end checks of `panoply video` and `panoply audio`. They need an ffmpeg and ffprobe on
//! PATH (or PANOPLY_FFMPEG) and the corpus (`python3 -I tests/corpus/fetch.py --kind video,audio`);
//! each test skips, with a message, when they are missing.

use std::path::{Path, PathBuf};
use std::process::Command;

fn panoply(args: &[&str]) -> (String, String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_panoply"))
        .args(args)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

fn corpus(rel: &str) -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/cache")
        .join(rel);
    if !p.is_file() {
        eprintln!("skipping: {} is not in the corpus", p.display());
        return None;
    }
    let have = |n: &str| {
        Command::new(n)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    if std::env::var_os("PANOPLY_FFMPEG").is_none() && !(have("ffmpeg") && have("ffprobe")) {
        eprintln!("skipping: no ffmpeg on PATH");
        return None;
    }
    Some(p)
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("panoply-cli-media-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_media_modules_are_listed() {
    let (out, _, ok) = panoply(&["modules"]);
    assert!(ok);
    for id in [
        "media.convert",
        "media.extract_audio",
        "media.trim",
        "media.compress",
        "media.info",
        "media.repair",
        "media.normalize",
        "media.to_mono",
        "media.hdr_to_sdr",
        "media.streams",
    ] {
        assert!(out.contains(id), "{id} is not listed:\n{out}");
    }
}

#[test]
fn video_info_reports_and_never_writes() {
    let Some(src) = corpus("video/generated/moov-at-end.mp4") else {
        return;
    };
    let d = scratch("info");
    let (out, err, ok) = panoply(&["video", "info", src.to_str().unwrap()]);
    assert!(ok, "{err}");
    assert!(out.contains("h264"), "{out}");
    assert!(out.contains("index is at the end"), "{out}");
    let (json, _, ok) = panoply(&["video", "info", src.to_str().unwrap(), "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(json.trim()).expect("valid JSON");
    assert_eq!(v["streams"][0]["codec"], "h264");
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn video_trim_writes_next_to_the_original_with_a_plain_report() {
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let d = scratch("trim");
    let copy = d.join("clip.mp4");
    std::fs::copy(&src, &copy).unwrap();
    let (out, err, ok) = panoply(&[
        "video",
        "trim",
        copy.to_str().unwrap(),
        "--keep",
        "1-4",
        "--keyframes",
    ]);
    assert!(ok, "{err}");
    assert!(out.contains("kept"), "{out}");
    assert!(d.join("clip (trimmed).mp4").is_file(), "{out}");
    assert!(copy.is_file(), "the original is untouched");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn bad_input_is_a_plain_sentence_not_a_stack_trace() {
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    let (_, err, ok) = panoply(&["video", "trim", src.to_str().unwrap(), "--keep", "99-120"]);
    assert!(!ok);
    assert!(err.contains("only"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
    let (_, err, ok) = panoply(&[
        "video",
        "compress",
        src.to_str().unwrap(),
        "--under",
        "lots",
    ]);
    assert!(!ok);
    assert!(err.contains("not a size"), "{err}");
}

#[test]
fn audio_convert_and_normalize_run_from_the_command_line() {
    let Some(src) = corpus("audio/commons/agnew-speech.ogg") else {
        return;
    };
    let d = scratch("audio");
    let copy = d.join("speech.ogg");
    std::fs::copy(&src, &copy).unwrap();
    let (out, err, ok) = panoply(&["audio", "convert", copy.to_str().unwrap(), "--to", "wav"]);
    assert!(ok, "{err}");
    assert!(out.contains("speech.wav"), "{out}");
    let wav = d.join("speech.wav");
    let (out, err, ok) = panoply(&[
        "audio",
        "normalize",
        wav.to_str().unwrap(),
        "--target",
        "-18",
    ]);
    assert!(ok, "{err}");
    assert!(out.contains("LUFS"), "{out}");
    let _ = std::fs::remove_dir_all(&d);
}
