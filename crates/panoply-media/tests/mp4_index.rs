//! Our own MP4 index reader against ffprobe, packet by packet, on real files.

mod common;

use common::*;
use panoply_media::mp4::boxes;
use panoply_media::mp4::movie::{Movie, parse_movie};
use panoply_media::mp4::source::Source;
use std::path::Path;
use std::process::Command;

fn movie_of(path: &Path) -> Movie {
    let src = Source::open(path).unwrap();
    let layout = boxes::scan(&src).unwrap();
    let moov = layout.find(b"moov").expect("moov");
    let body = src
        .read_exact_at(
            moov.payload(),
            (moov.size - u64::from(moov.header_len)) as usize,
        )
        .unwrap();
    parse_movie(&body).unwrap()
}

/// `(pos, size, dts_us, key)` for every packet of one stream, from ffprobe.
fn ffprobe_packets(path: &Path, spec: &str) -> Vec<(u64, u32, bool)> {
    let e = the_engine();
    let out = Command::new(&e.ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            spec,
            "-show_entries",
            "packet=pos,size,flags",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split(',');
            // ffprobe prints size before pos whatever the order asked for.
            let size: u32 = it.next()?.parse().ok()?;
            let pos: u64 = it.next()?.parse().ok()?;
            let key = it.next()?.contains('K');
            Some((pos, size, key))
        })
        .collect()
}

#[test]
fn every_sample_matches_ffprobe_on_mp4_and_mov_files() {
    if !engine() {
        return;
    }
    let files = [
        "video/blender/bbb-1080p-h264-10s.mp4",
        "video/blender/sintel-trailer-480p.mp4",
        "video/blender/bbb-720p-hevc-10s.mp4",
        "video/generated/rotated-90-display-matrix.mp4",
        "video/generated/vfr-phone-portrait.mp4",
        "video/generated/moov-at-end.mp4",
        "video/generated/hdr10-hevc-10bit.mp4",
        "video/generated/prores-proxy-720p.mov",
        "audio/generated/aac-lc-stereo.m4a",
    ];
    for f in files {
        let Some(path) = corpus(f) else { continue };
        let m = movie_of(&path);
        let mut v = 0;
        let mut a = 0;
        for t in &m.tracks {
            if matches!(
                t.codec().as_str(),
                "sowt" | "twos" | "lpcm" | "in24" | "in32" | "fl32" | "fl64" | "raw "
            ) {
                // Uncompressed audio in QuickTime: one sample per sample frame, ffprobe groups them.
                continue;
            }
            let spec = if t.is_video() {
                v += 1;
                format!("v:{}", v - 1)
            } else if t.is_audio() {
                a += 1;
                format!("a:{}", a - 1)
            } else {
                continue;
            };
            let want = ffprobe_packets(&path, &spec);
            assert_eq!(t.samples.len(), want.len(), "{f} {spec}: sample count");
            for (i, (s, w)) in t.samples.iter().zip(&want).enumerate() {
                assert_eq!((s.offset, s.size), (w.0, w.1), "{f} {spec} sample {i}");
                assert_eq!(s.sync, w.2, "{f} {spec} sample {i}: sync flag");
            }
        }
    }
}

#[test]
fn the_layout_reports_where_the_index_is() {
    let Some(end) = corpus("video/generated/moov-at-end.mp4") else {
        return;
    };
    let Some(front) = corpus("video/blender/bbb-1080p-h264-10s.mp4") else {
        return;
    };
    let l = boxes::scan(&Source::open(&end).unwrap()).unwrap();
    assert_eq!(l.moov_first(), Some(false));
    let l = boxes::scan(&Source::open(&front).unwrap()).unwrap();
    // Either layout is fine for the source; what matters is that the walk reports one.
    assert!(l.moov_first().is_some());
}
