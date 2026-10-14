//! HDR to SDR: the whole pipeline checked against the mapping function, pixel by pixel, on a
//! clip of known colours, and on the corpus HDR files.

mod common;

use common::*;
use panoply_media::encoders::VCodec;
use panoply_media::hdr::{Params, Transfer, map_pixel};
use panoply_media::probe::Hdr;
use serde_json::json;
use std::process::Command;

/// Eight neutral levels and four colours, as encoded 8-bit PQ code values.
const PATCHES: &[[u8; 3]] = &[
    [0, 0, 0],
    [40, 40, 40],
    [80, 80, 80],
    [120, 120, 120],
    [148, 148, 148], // 0.58 is PQ of 203 nits, diffuse white
    [180, 180, 180],
    [215, 215, 215],
    [255, 255, 255],
    [150, 110, 90],
    [90, 140, 100],
    [100, 100, 170],
    [200, 160, 80],
];

fn make_hdr_clip(dir: &std::path::Path, transfer: &str) -> std::path::PathBuf {
    let out = dir.join(format!("patches-{transfer}.mkv"));
    let w = 64;
    let mut boxes = String::new();
    for (i, p) in PATCHES.iter().enumerate() {
        boxes.push_str(&format!(
            "drawbox=x={}:y=0:w={w}:h=64:color=0x{:02X}{:02X}{:02X}:t=fill,",
            i * w,
            p[0],
            p[1],
            p[2]
        ));
    }
    let vf = format!(
        "format=gbrp,{boxes}scale=out_color_matrix=bt2020nc:out_range=tv:flags=accurate_rnd+full_chroma_int,format=yuv420p10le,setparams=colorspace=bt2020nc:color_primaries=bt2020:color_trc={transfer}:range=tv"
    );
    let (ok, err) = ffmpeg(&[
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        &format!("color=c=black:s={}x64:r=24:d=2", w * PATCHES.len()),
        "-vf",
        &vf,
        "-c:v",
        "ffv1",
        "-level",
        "3",
        "-pix_fmt",
        "yuv420p10le",
        "-colorspace",
        "bt2020nc",
        "-color_primaries",
        "bt2020",
        "-color_trc",
        transfer,
        &out.to_string_lossy(),
    ]);
    assert!(ok, "{err}");
    out
}

fn pixel(path: &std::path::Path, x: usize, y: usize) -> [u8; 3] {
    let e = the_engine();
    let o = Command::new(&e.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!(
                "scale=in_color_matrix=bt709:in_range=tv:out_range=pc,format=rgb24,crop=1:1:{x}:{y}"
            ),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        o.stdout.len() >= 3,
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    [o.stdout[0], o.stdout[1], o.stdout[2]]
}

fn check(transfer_name: &str, transfer: Transfer) {
    let s = Scratch::new("hdr");
    let src = make_hdr_clip(s.path(), transfer_name);
    let (out, notes) = run_ok(
        "media.hdr_to_sdr",
        &src,
        json!({"peak_nits": 1000}),
        s.path(),
    );
    let p = probe(&out[0]);
    let v = p.video().unwrap();
    assert_eq!(v.hdr, Hdr::Sdr, "{notes:?}");
    assert_eq!(v.color_primaries, "bt709");
    assert_eq!(v.color_transfer, "bt709");
    assert_eq!(v.bit_depth, 8);
    let params = Params::new(transfer);
    let mut worst = 0i32;
    for (i, patch) in PATCHES.iter().enumerate() {
        let want = map_pixel(
            &params,
            [
                f64::from(patch[0]) / 255.0,
                f64::from(patch[1]) / 255.0,
                f64::from(patch[2]) / 255.0,
            ],
        );
        let got = pixel(&out[0], i * 64 + 32, 32);
        for c in 0..3 {
            let w = (want[c] * 255.0).round() as i32;
            let d = (i32::from(got[c]) - w).abs();
            worst = worst.max(d);
            // Lossy hardware encode, 4:2:0 chroma and 8-bit rounding: a few code values.
            let tol = if i < 8 { 5 } else { 9 };
            assert!(
                d <= tol,
                "{transfer_name} patch {i} {patch:?} channel {c}: got {got:?}, wanted {want:?} (off by {d})"
            );
        }
    }
    eprintln!(
        "{transfer_name}: worst error {worst} code values over {} patches",
        PATCHES.len()
    );
}

#[test]
fn the_pq_pipeline_matches_the_mapping_function_pixel_by_pixel() {
    if !engine()
        || !has_encoder("ffv1")
        || !has_hw(VCodec::H264)
        || !the_engine().has_filter("lut3d")
    {
        return;
    }
    check("smpte2084", Transfer::Pq);
}

#[test]
fn the_hlg_pipeline_matches_the_mapping_function_pixel_by_pixel() {
    if !engine()
        || !has_encoder("ffv1")
        || !has_hw(VCodec::H264)
        || !the_engine().has_filter("lut3d")
    {
        return;
    }
    check("arib-std-b67", Transfer::Hlg);
}

#[test]
fn corpus_hdr_files_become_sdr_and_keep_their_sound_and_rotation_flag() {
    if !engine() || !has_hw(VCodec::H264) || !the_engine().has_filter("lut3d") {
        return;
    }
    for f in [
        "video/generated/hdr10-hevc-10bit.mp4",
        "video/generated/hlg-hevc-10bit.mp4",
    ] {
        let Some(src) = corpus(f) else { continue };
        let s = Scratch::new("hdr-corpus");
        let (out, notes) = run_ok("media.hdr_to_sdr", &src, json!({}), s.path());
        let p = probe(&out[0]);
        assert_eq!(p.video().unwrap().hdr, Hdr::Sdr, "{f}: {notes:?}");
        assert_eq!(p.video().unwrap().codec, "h264");
        assert_eq!(
            packet_hashes(&out[0], "0:a:0"),
            packet_hashes(&src, "0:a:0"),
            "{f}: audio is copied"
        );
        assert!(
            decode_errors(&out[0]).is_empty(),
            "{f}: {:?}",
            decode_errors(&out[0])
        );
        assert!((p.duration.unwrap() - probe(&src).duration.unwrap()).abs() < 0.1);
    }
}

#[test]
fn an_sdr_video_is_left_alone_and_says_so() {
    if !engine() {
        return;
    }
    let Some(src) = corpus("video/blender/bbb-360p-h264-10s.mp4") else {
        return;
    };
    let s = Scratch::new("hdr-sdr");
    let (out, notes) = run_ok("media.hdr_to_sdr", &src, json!({}), s.path());
    assert!(out.is_empty());
    assert!(
        notes.iter().any(|n| n.contains("already standard range")),
        "{notes:?}"
    );
}
