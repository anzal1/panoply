//! Repair on deliberately broken files: truncated at 10, 50 and 90 percent, the index stripped,
//! bytes zeroed in the middle. No output is accepted unless it plays start to end.

mod common;

use common::*;
use panoply_media::mp4::boxes;
use panoply_media::mp4::movie::parse_movie;
use panoply_media::mp4::source::Source;
use serde_json::json;
use std::path::Path;

fn truncated_copy(src: &Path, dir: &Path, name: &str, keep: f64) -> std::path::PathBuf {
    let bytes = std::fs::read(src).unwrap();
    let out = dir.join(name);
    std::fs::write(&out, &bytes[..(bytes.len() as f64 * keep) as usize]).unwrap();
    out
}

fn reference_frames_in(src: &Path, upto: u64) -> usize {
    let s = Source::open(src).unwrap();
    let l = boxes::scan(&s).unwrap();
    let m = l.find(b"moov").unwrap();
    let movie = parse_movie(&s.read_exact_at(m.payload(), (m.size - 8) as usize).unwrap()).unwrap();
    let v = movie.tracks.iter().find(|t| t.is_video()).unwrap();
    v.samples
        .iter()
        .filter(|x| x.offset + u64::from(x.size) <= upto)
        .count()
}

#[test]
fn a_missing_index_is_rebuilt_from_a_reference_and_the_pictures_are_bit_exact() {
    if !engine() {
        return;
    }
    let Some(broken) = corpus("video/generated/broken-moov-truncated.mp4") else {
        return;
    };
    let Some(reference) = corpus("video/generated/moov-at-end.mp4") else {
        return;
    };
    let s = Scratch::new("repair-rebuild");
    let (out, notes) = run_many(
        "media.repair",
        &[broken.clone(), reference.clone()],
        json!({"reference": reference.to_string_lossy()}),
        s.path(),
    )
    .unwrap();
    assert_eq!(out.len(), 1, "{notes:?}");
    let note = notes.join(" | ");
    assert!(note.contains("no usable index"), "{note}");
    assert!(note.contains("repaired"), "{note}");
    assert!(!note.contains("not declared repaired"), "{note}");
    let want = reference_frames_in(&reference, std::fs::metadata(&broken).unwrap().len());
    let got = frame_hashes(&out[0], "0:v:0");
    assert!(
        (got.len() as i64 - want as i64).abs() <= 6,
        "recovered {} of {want}",
        got.len()
    );
    // Every recovered picture decodes to the reference's own picture.
    let refs = frame_hashes(&reference, "0:v:0");
    let n = got.len().min(refs.len()).saturating_sub(3);
    let first_diff = (0..n).find(|&i| got[i] != refs[i]);
    assert_eq!(
        first_diff,
        None,
        "the first {n} pictures are bit-exact ({} recovered, {} in the reference)",
        got.len(),
        refs.len()
    );
    assert!(
        decode_errors(&out[0])
            .iter()
            .all(|l| l.contains("non monotonically"))
    );
}

#[test]
fn without_a_reference_the_plain_error_asks_for_one() {
    if !engine() {
        return;
    }
    let Some(broken) = corpus("video/generated/broken-moov-truncated.mp4") else {
        return;
    };
    let s = Scratch::new("repair-noref");
    let e = run("media.repair", &broken, json!({}), s.path()).unwrap_err();
    assert!(e.contains("working video from the same camera"), "{e}");
    assert!(names(s.path()).is_empty());
}

#[test]
fn a_cut_off_file_with_its_index_keeps_everything_that_arrived() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let s = Scratch::new("repair-trunc");
    // Make sure the index is at the front, then cut the file in the data.
    let (fast, _) = run_ok("media.convert", &src, json!({"to": "mp4"}), s.path());
    let base = if fast.is_empty() {
        src.clone()
    } else {
        fast[0].clone()
    };
    let layout = boxes::scan(&Source::open(&base).unwrap()).unwrap();
    if layout.moov_first() != Some(true) {
        return;
    }
    for keep in [0.1, 0.5, 0.9] {
        let sub = Scratch::new("repair-trunc-run");
        let cut = truncated_copy(
            &base,
            sub.path(),
            &format!("cut{}.mp4", (keep * 100.0) as u32),
            keep,
        );
        let (out, notes) = run_ok("media.repair", &cut, json!({}), sub.path());
        let note = notes.join(" | ");
        assert_eq!(out.len(), 1, "{note}");
        assert!(note.contains("cut off in the middle of its data"), "{note}");
        assert!(!note.contains("not declared repaired"), "{keep}: {note}");
        let p = probe(&out[0]);
        let full = probe(&base).duration.unwrap();
        // About as much survives as arrived (the data is not spread evenly, so a wide band).
        let frac = p.duration.unwrap() / full;
        assert!(
            frac > keep * 0.5 && frac < keep * 1.3 + 0.05,
            "kept {keep}: got {frac:.2}"
        );
        assert!(
            decode_errors(&out[0]).is_empty(),
            "{keep}: {:?}",
            decode_errors(&out[0])
        );
        // Audio and video end together.
        let v = p.video().unwrap().duration.unwrap_or(0.0);
        let a = p.audio().map(|a| a.duration.unwrap_or(0.0)).unwrap_or(v);
        assert!((v - a).abs() < 0.1, "video {v} audio {a}");
    }
}

#[test]
fn an_undamaged_file_is_left_alone_and_says_so() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("repair-fine");
    let (out, notes) = run_ok("media.repair", &src, json!({}), s.path());
    assert!(out.is_empty());
    assert!(notes.iter().any(|n| n.contains("not damaged")), "{notes:?}");
}

#[test]
fn a_cut_off_mkv_is_re_read_by_the_helper() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("repair-mkv");
    let cut = truncated_copy(&src, s.path(), "cut.mkv", 0.5);
    let (out, notes) = run_ok("media.repair", &cut, json!({}), s.path());
    assert_eq!(out.len(), 1, "{notes:?}");
    let p = probe(&out[0]);
    assert!(p.duration.unwrap() > 1.0, "{:?}", p.duration);
    assert!(
        decode_errors(&out[0]).is_empty(),
        "{:?}",
        decode_errors(&out[0])
    );
}

#[test]
fn zeroed_bytes_in_the_middle_do_not_stop_the_file_from_being_read() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/multi-audio-3-tracks-subs.mkv") else {
        return;
    };
    let s = Scratch::new("repair-zero");
    let mut bytes = std::fs::read(&src).unwrap();
    let mid = bytes.len() / 2;
    for b in &mut bytes[mid..mid + 20_000] {
        *b = 0;
    }
    let bad = s.join("zeroed.mkv");
    std::fs::write(&bad, bytes).unwrap();
    let (out, notes) = run_ok("media.repair", &bad, json!({}), s.path());
    assert_eq!(out.len(), 1, "{notes:?}");
    assert!(probe(&out[0]).duration.unwrap() > 2.0);
}

/// A copy of a file with its index at the end and every picture's parameter sets either in the
/// index or (with `inband`) in the stream.
fn index_at_end(src: &Path, dir: &Path, name: &str, inband: bool) -> std::path::PathBuf {
    let out = dir.join(name);
    let mut a: Vec<&str> = vec!["-y", "-v", "error", "-i"];
    let s = src.to_string_lossy().into_owned();
    a.push(&s);
    a.extend(["-map", "0:v:0", "-c", "copy"]);
    if inband {
        a.extend(["-bsf:v", "dump_extra=freq=keyframe", "-tag:v", "avc3"]);
    }
    let o = out.to_string_lossy().into_owned();
    a.push(&o);
    let (ok, err) = ffmpeg(&a);
    assert!(ok, "{err}");
    out
}

fn strip_index(full: &Path, keep_data: f64, to: &Path) {
    let src = Source::open(full).unwrap();
    let l = boxes::scan(&src).unwrap();
    let mdat = l.find(b"mdat").unwrap();
    let cut = mdat.payload() + ((mdat.size - u64::from(mdat.header_len)) as f64 * keep_data) as u64;
    let bytes = std::fs::read(full).unwrap();
    std::fs::write(to, &bytes[..cut as usize]).unwrap();
}

#[test]
fn hevc_with_b_frames_is_rebuilt_in_display_order() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-720p-hevc-10s.mp4") else {
        return;
    };
    let s = Scratch::new("repair-hevc");
    let full = index_at_end(&src, s.path(), "full.mp4", false);
    let l = boxes::scan(&Source::open(&full).unwrap()).unwrap();
    assert_eq!(l.moov_first(), Some(false));
    let broken = s.join("broken.mp4");
    strip_index(&full, 0.6, &broken);
    let (out, notes) = run_many(
        "media.repair",
        &[broken.clone(), full.clone()],
        json!({"reference": full.to_string_lossy()}),
        s.path(),
    )
    .unwrap();
    let note = notes.join(" | ");
    assert_eq!(out.len(), 1, "{note}");
    assert!(!note.contains("not declared repaired"), "{note}");
    assert!(
        !note.contains("assumed"),
        "the picture order was read from the slice headers: {note}"
    );
    let got = frame_hashes(&out[0], "0:v:0");
    let refs = frame_hashes(&full, "0:v:0");
    let n = got.len().min(refs.len()).saturating_sub(4);
    assert!(n > 100, "{} frames recovered", got.len());
    let first_diff = (0..n).find(|&i| got[i] != refs[i]);
    assert_eq!(
        first_diff,
        None,
        "HEVC frames match the reference ({} recovered)",
        got.len()
    );
}

#[test]
fn parameter_sets_found_in_the_stream_work_without_a_reference() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/generated/moov-at-end.mp4") else {
        return;
    };
    let s = Scratch::new("repair-inband");
    // A file whose pictures carry their own parameter sets before the first one, the way some
    // recorders write them, and which has no index at all.
    let msrc = Source::open(&src).unwrap();
    let l = boxes::scan(&msrc).unwrap();
    let m = l.find(b"moov").unwrap();
    let movie = parse_movie(
        &msrc
            .read_exact_at(m.payload(), (m.size - 8) as usize)
            .unwrap(),
    )
    .unwrap();
    let vt = movie.tracks.iter().find(|t| t.is_video()).unwrap();
    let params = panoply_media::mp4::repair::params_from_track(vt).unwrap();
    let mut data: Vec<u8> = Vec::new();
    for set in &params.sets {
        data.extend_from_slice(&(set.len() as u32).to_be_bytes());
        data.extend_from_slice(set);
    }
    let keep = (vt.samples.len() as f64 * 0.7) as usize;
    for smp in &vt.samples[..keep] {
        data.extend(msrc.read_exact_at(smp.offset, smp.size as usize).unwrap());
    }
    let mut file = Vec::new();
    file.extend_from_slice(&[
        0, 0, 0, 20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm', 0, 0, 2, 0, b'i', b's', b'o',
        b'm',
    ]);
    file.extend_from_slice(&((data.len() + 8) as u32).to_be_bytes());
    file.extend_from_slice(b"mdat");
    file.extend(data);
    let broken = s.join("noindex.mp4");
    std::fs::write(&broken, file).unwrap();
    let (out, notes) = run_ok("media.repair", &broken, json!({}), s.path());
    let note = notes.join(" | ");
    assert!(note.contains("No reference file was given"), "{note}");
    assert_eq!(out.len(), 1, "{note}");
    let got = frame_hashes(&out[0], "0:v:0").len();
    assert!(got as f64 > keep as f64 * 0.9, "{got} of {keep} frames");
}

#[test]
fn pictures_between_stretches_of_sound_are_not_confused_with_sound() {
    if !engine() {
        return;
    }
    // Sintel's trailer interleaves AAC frames of a few hundred bytes with the pictures. Sound
    // that happens to start with a plausible length and NAL header used to swallow real
    // pictures; the frame numbers of real pictures follow each other, the sound's do not.
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let s = Scratch::new("repair-interleaved");
    let full = s.join("full.mp4");
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-i",
        &src.to_string_lossy(),
        "-c",
        "copy",
        &full.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    let broken = s.join("broken.mp4");
    strip_index(&full, 0.7, &broken);
    let (out, notes) = run_many(
        "media.repair",
        &[broken.clone(), full.clone()],
        json!({"reference": full.to_string_lossy()}),
        s.path(),
    )
    .unwrap();
    let note = notes.join(" | ");
    assert_eq!(out.len(), 1, "{note}");
    assert!(!note.contains("not declared repaired"), "{note}");
    let want = reference_frames_in(&full, std::fs::metadata(&broken).unwrap().len());
    let got = frame_hashes(&out[0], "0:v:0");
    assert!(
        (got.len() as i64 - want as i64).abs() <= 3,
        "recovered {} of {want}",
        got.len()
    );
    assert!(decode_errors(&out[0]).is_empty());
}
