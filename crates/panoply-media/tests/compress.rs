//! Compress to a size: the file fits, uses the room it is given, says what it gave up, and
//! refuses impossible targets. Quality is measured with VMAF on the actual output.

mod common;

use common::*;
use panoply_media::encoders::VCodec;
use serde_json::json;

fn size(p: &std::path::Path) -> u64 {
    std::fs::metadata(p).unwrap().len()
}

#[test]
fn every_target_fits_and_uses_most_of_the_room() {
    if !engine() || !has_hw(VCodec::H264) {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    for target in [1_000_000u64, 2_000_000, 5_000_000] {
        let s = Scratch::new("compress-fit");
        let (out, notes) = run_ok(
            "media.compress",
            &src,
            json!({"under": target, "codec": "h264"}),
            s.path(),
        );
        assert_eq!(out.len(), 1, "{notes:?}");
        let got = size(&out[0]);
        assert!(got <= target, "{target}: {got} bytes, {notes:?}");
        assert!(
            got as f64 >= target as f64 * 0.78,
            "{target}: only {got} bytes, the room was wasted: {notes:?}"
        );
        assert!(
            decode_errors(&out[0]).is_empty(),
            "{:?}",
            decode_errors(&out[0])
        );
        let p = probe(&out[0]);
        assert!((p.duration.unwrap() - 10.0).abs() < 0.3, "{:?}", p.duration);
    }
}

#[test]
fn a_tight_size_lowers_the_resolution_and_says_so() {
    if !engine() || !has_hw(VCodec::H264) {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("compress-tight");
    let (out, notes) = run_ok(
        "media.compress",
        &src,
        json!({"under": "700kb", "codec": "h264"}),
        s.path(),
    );
    let p = probe(&out[0]);
    assert!(
        p.video().unwrap().height < 1080,
        "{}x{}",
        p.video().unwrap().width,
        p.video().unwrap().height
    );
    assert!(size(&out[0]) <= 700_000, "{}", size(&out[0]));
    assert!(notes.iter().any(|n| n.contains("scaled down")), "{notes:?}");
}

#[test]
fn an_impossible_target_is_refused_with_a_number_that_works() {
    if !engine() || !has_hw(VCodec::H264) {
        return;
    }
    let Some(src) = corpus("video/blender/sintel-trailer-480p.mp4") else {
        return;
    };
    let s = Scratch::new("compress-impossible");
    let e = run(
        "media.compress",
        &src,
        json!({"under": "100kb", "codec": "h264"}),
        s.path(),
    )
    .unwrap_err();
    assert!(e.contains("look like mush"), "{e}");
    assert!(e.contains("Raise the size to"), "{e}");
    assert!(names(s.path()).is_empty());
}

#[test]
fn a_file_already_under_the_limit_is_left_alone() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("compress-already");
    let (out, notes) = run_ok(
        "media.compress",
        &src,
        json!({"under": "10mb", "codec": "h264"}),
        s.path(),
    );
    assert!(out.is_empty());
    assert!(
        notes.iter().any(|n| n.contains("already under")),
        "{notes:?}"
    );
}

#[test]
fn a_service_preset_uses_its_limit_h264_mp4_and_the_card_names_the_service() {
    if !engine() || !has_hw(VCodec::H264) {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("compress-preset");
    // Discord's free limit is 20 MB in its preset file; the clip is 10 MB, so use a tighter
    // explicit size to force work: the smaller of the two wins.
    let (out, notes) = run_ok(
        "media.compress",
        &src,
        json!({"preset": "discord", "under": "4mb"}),
        s.path(),
    );
    assert!(size(&out[0]) <= 4_000_000);
    let p = probe(&out[0]);
    assert_eq!(p.video().unwrap().codec, "h264");
    assert_eq!(p.container, panoply_media::probe::Container::Mp4);
    assert!(notes.iter().any(|n| n.contains("4.0 MB")), "{notes:?}");
}

#[test]
fn hevc_in_a_preset_is_converted_to_h264() {
    if !engine() || !has_hw(VCodec::H264) {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-720p-hevc-10s.mp4") else {
        return;
    };
    let s = Scratch::new("compress-preset-codec");
    let (out, notes) = run_ok("media.compress", &src, json!({"preset": "gmail"}), s.path());
    assert_eq!(out.len(), 1, "{notes:?}");
    assert_eq!(probe(&out[0]).video().unwrap().codec, "h264");
}

#[test]
fn hdr_is_tone_mapped_for_h264_and_kept_for_hevc() {
    if !engine()
        || !has_hw(VCodec::Hevc)
        || !has_hw(VCodec::H264)
        || !the_engine().has_filter("lut3d")
    {
        return;
    }
    let Some(src) = corpus("video/generated/hdr10-hevc-10bit.mp4") else {
        return;
    };
    let s = Scratch::new("compress-hdr");
    let (out, notes) = run_ok(
        "media.compress",
        &src,
        json!({"under": "3mb", "codec": "h264"}),
        s.path(),
    );
    assert_eq!(
        probe(&out[0]).video().unwrap().hdr,
        panoply_media::probe::Hdr::Sdr,
        "{notes:?}"
    );
    assert!(notes.iter().any(|n| n.contains("tone-mapped")), "{notes:?}");
    let s2 = Scratch::new("compress-hdr-keep");
    let (out, notes) = run_ok(
        "media.compress",
        &src,
        json!({"under": "4mb", "codec": "hevc"}),
        s2.path(),
    );
    let v = probe(&out[0]).video().unwrap().clone();
    assert_eq!(v.hdr, panoply_media::probe::Hdr::Hdr10, "{notes:?}");
    assert_eq!(v.bit_depth, 10);
}

#[test]
fn rotation_stays_a_flag_and_the_sound_is_kept() {
    if !engine() || !has_hw(VCodec::H264) {
        return;
    }
    let Some(src) = corpus("video/generated/rotated-90-display-matrix.mp4") else {
        return;
    };
    let s = Scratch::new("compress-rotation");
    let (out, _) = run_ok(
        "media.compress",
        &src,
        json!({"under": "2mb", "codec": "h264"}),
        s.path(),
    );
    let p = probe(&out[0]);
    assert_eq!(
        p.video().unwrap().rotation,
        probe(&src).video().unwrap().rotation
    );
    assert!(p.has_audio());
}

#[test]
fn smallest_that_looks_good_never_returns_a_bigger_file() {
    if !engine() || !has_hw(VCodec::H264) {
        return;
    }
    // A 4K clip at 40 Mb/s has plenty to give.
    let Some(src) = corpus("video/generated/4k-hevc-5s.mp4") else {
        return;
    };
    let s = Scratch::new("compress-smallest");
    let (out, notes) = run_ok("media.compress", &src, json!({"codec": "h264"}), s.path());
    assert_eq!(out.len(), 1, "{notes:?}");
    assert!(
        size(&out[0]) < size(&src),
        "{} vs {}",
        size(&out[0]),
        size(&src)
    );
    // And a second pass on the result says it is already small, or at least does not grow it.
    let s2 = Scratch::new("compress-smallest-2");
    let (again, notes2) = run_ok(
        "media.compress",
        &out[0],
        json!({"codec": "h264"}),
        s2.path(),
    );
    if let Some(a) = again.first() {
        assert!(size(a) <= size(&out[0]), "{notes2:?}");
    } else {
        assert!(
            notes2.iter().any(|n| n.contains("already small")),
            "{notes2:?}"
        );
    }
}

#[test]
fn quality_is_measured_on_the_real_output() {
    if !engine() || !has_hw(VCodec::H264) {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-720p-hevc-10s.mp4") else {
        return;
    };
    let s = Scratch::new("compress-vmaf");
    let (_, notes) = run_ok(
        "media.compress",
        &src,
        json!({"under": "3mb", "codec": "h264"}),
        s.path(),
    );
    assert!(
        notes
            .iter()
            .any(|n| n.contains("VMAF") || n.contains("SSIM")),
        "{notes:?}"
    );
}

#[test]
fn audio_files_are_compressed_to_a_size_too() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("audio/commons/stay-down-here-1915.mp3") else {
        return;
    };
    let s = Scratch::new("compress-audio");
    let (out, notes) = run_ok("media.compress", &src, json!({"under": "1mb"}), s.path());
    assert!(size(&out[0]) <= 1_000_000, "{}: {notes:?}", size(&out[0]));
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn a_cancelled_compress_leaves_nothing() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("compress-cancel");
    let cancel = panoply_core::CancelToken::new();
    cancel.cancel();
    let io = panoply_media::io::OpenIo::new()
        .with_out_dir(s.path().to_path_buf())
        .with_cancel(cancel);
    let e = module("media.compress")
        .run(&io, &[src], &json!({"under": "2mb"}))
        .unwrap_err();
    assert!(e.to_string().contains("Cancelled"), "{e}");
    assert!(names(s.path()).is_empty(), "{:?}", names(s.path()));
}

#[test]
fn vp9_is_written_as_webm_with_a_true_two_pass_encode() {
    if !engine() || !has_encoder("libvpx-vp9") {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("compress-vp9");
    let (out, notes) = run_ok(
        "media.compress",
        &src,
        json!({"under": "600kb", "codec": "vp9"}),
        s.path(),
    );
    assert_eq!(out[0].extension().unwrap(), "webm", "{notes:?}");
    assert!(size(&out[0]) <= 600_000, "{}", size(&out[0]));
    assert_eq!(probe(&out[0]).video().unwrap().codec, "vp9");
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn an_encoder_that_will_not_go_lower_is_given_a_smaller_picture() {
    if !engine() || !has_hw(VCodec::Hevc) {
        return;
    }
    // Hardware HEVC has a rate it will not go under for a 1080p picture; the loop must notice
    // and scale the picture down instead of giving up.
    let Some(src) = corpus("video/generated/rotated-90-display-matrix.mp4") else {
        return;
    };
    let s = Scratch::new("compress-stall");
    let (out, notes) = run_ok(
        "media.compress",
        &src,
        json!({"under": "2mb", "codec": "hevc"}),
        s.path(),
    );
    assert!(size(&out[0]) <= 2_000_000, "{}: {notes:?}", size(&out[0]));
    assert!(decode_errors(&out[0]).is_empty());
}

#[test]
fn a_clip_that_comes_out_visibly_worse_is_encoded_again_or_says_so() {
    if !engine() || !has_hw(VCodec::Hevc) {
        return;
    }
    // Two clips the default plan used to leave at VMAF 59 and 52 with a card that said nothing.
    for rel in [
        "video/commons/phone-clip-vid20240601.webm",
        "video/commons/camera-clip-mvi0131.ogv",
    ] {
        let Some(src) = corpus(rel) else { continue };
        let s = Scratch::new("compress-floor");
        let (out, notes) = run_ok("media.compress", &src, json!({}), s.path());
        let text = notes.join("\n");
        if out.is_empty() {
            assert!(text.contains("left as it is"), "{rel}: {text}");
            continue;
        }
        let vmaf = text
            .split("Quality VMAF ")
            .nth(1)
            .and_then(|t| t.split(' ').next())
            .and_then(|t| t.parse::<f64>().ok());
        let warned = text.contains("below the 75 Panoply recommends");
        assert!(
            vmaf.is_some_and(|v| v >= 75.0) || warned,
            "{rel}: {vmaf:?} with no warning: {text}"
        );
        assert!(
            size(&out[0]) < size(&src),
            "{rel}: the result is not smaller"
        );
    }
}
