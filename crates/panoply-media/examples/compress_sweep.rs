//! Measure the quality floors in `modules/builtin/media-compress/floors.toml`.
//!
//! For each corpus clip and each codec, encode a few seconds at a ladder of bits per pixel per
//! frame and score the result with VMAF (libvmaf, BSD) against the source. The floor is the
//! bits per pixel where VMAF crosses 80, the knee where it crosses 93, interpolated between the
//! two nearest points and combined over the clips (geometric mean here; the values in
//! `floors.toml` use the median over the clips, which a clip that never reaches the level cannot
//! pull around, and VMAF 90 as the knee because VMAF 93 is out of reach for the grainy clips). Run it with
//! `nice -n 10 cargo run --release -p panoply-media --example compress_sweep -- [seconds] [h264_hardware hevc_hardware av1_software vp9_software]`.
//!
//! Hardware encoders are driven through the same `panoply_media::encoders` the module uses, so
//! the numbers are the ones the module will meet. The software AV1 encoder is SVT-AV1 at preset 8.

use std::path::{Path, PathBuf};
use std::process::Command;

use panoply_media::encoders::{self, Rate, VCodec};
use panoply_media::engine::Engine;
use panoply_media::io::OpenIo;

struct Clip {
    id: &'static str,
    file: &'static str,
}

const CLIPS: &[Clip] = &[
    Clip {
        id: "vid-bbb-1080-h264",
        file: "blender/bbb-1080p-h264-10s.mp4",
    },
    Clip {
        id: "vid-sintel-720-h264",
        file: "blender/sintel-720p-h264-10s.mp4",
    },
    Clip {
        id: "vid-bbb-720-hevc",
        file: "blender/bbb-720p-hevc-10s.mp4",
    },
    Clip {
        id: "vid-sintel-trailer-480p",
        file: "blender/sintel-trailer-480p.mp4",
    },
];

const BPP: &[f64] = &[0.008, 0.012, 0.018, 0.027, 0.04, 0.06, 0.09, 0.135, 0.2];

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/cache/video")
        .join(rel)
}

fn run(engine: &Engine, args: &[String]) -> bool {
    Command::new(&engine.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-y"])
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn vmaf(
    engine: &Engine,
    reference: &Path,
    encoded: &Path,
    w: u32,
    h: u32,
    log: &Path,
) -> Option<f64> {
    let f = format!(
        "[0:v]scale={w}:{h}:flags=bicubic,format=yuv420p,setpts=PTS-STARTPTS[d];[1:v]format=yuv420p,setpts=PTS-STARTPTS[r];[d][r]libvmaf=n_threads=6:n_subsample=2:log_fmt=json:log_path={}",
        log.display()
    );
    let ok = Command::new(&engine.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(encoded)
        .arg("-i")
        .arg(reference)
        .args(["-lavfi", &f, "-f", "null", "-"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let text = std::fs::read_to_string(log).ok()?;
    panoply_media::ops::quality::parse_vmaf(&text, 0.0).map(|q| q.score)
}

/// bpp where the curve crosses `level`, interpolated in log bpp.
fn crossing(points: &[(f64, f64)], level: f64) -> Option<f64> {
    for w in points.windows(2) {
        let ((b0, v0), (b1, v1)) = (w[0], w[1]);
        if v0 <= level && v1 >= level && v1 > v0 {
            let t = (level - v0) / (v1 - v0);
            return Some((b0.ln() + t * (b1.ln() - b0.ln())).exp());
        }
    }
    None
}

fn main() {
    let seconds: f64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6.0);
    assert!(Engine::use_path_programs(), "no ffmpeg on PATH");
    let engine = Engine::get(&OpenIo::new()).unwrap();
    let dir = std::env::temp_dir().join(format!("panoply-sweep-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut configs: Vec<(&str, VCodec, bool)> = vec![
        ("h264_hardware", VCodec::H264, true),
        ("hevc_hardware", VCodec::Hevc, true),
        ("av1_software", VCodec::Av1, false),
        ("vp9_software", VCodec::Vp9, false),
    ];
    // Optional names after the number of seconds pick which encoders to sweep.
    let only: Vec<String> = std::env::args().skip(2).collect();
    if !only.is_empty() {
        configs.retain(|c| only.iter().any(|o| o == c.0));
    }
    println!("# VMAF sweep, {seconds} s per clip, libvmaf n_subsample=2\n");
    let mut floors: Vec<(String, Vec<f64>, Vec<f64>)> = Vec::new();
    for (name, codec, hw) in &configs {
        let Some(enc) = encoders::pick(&engine, *codec, *hw) else {
            println!("{name}: no encoder here, skipped");
            continue;
        };
        println!("## {name} ({})\n", enc.label());
        println!(
            "| clip | {} |",
            BPP.iter()
                .map(|b| format!("{b}"))
                .collect::<Vec<_>>()
                .join(" | ")
        );
        println!(
            "|---|{}|",
            BPP.iter().map(|_| "---").collect::<Vec<_>>().join("|")
        );
        let (mut lows, mut knees) = (Vec::new(), Vec::new());
        for clip in CLIPS {
            let src = corpus(clip.file);
            if !src.is_file() {
                continue;
            }
            let probe = panoply_media::probe::probe(&engine, &OpenIo::new(), &src).unwrap();
            let v = probe.video().unwrap().clone();
            let fps = if v.fps > 0.0 { v.fps } else { 30.0 };
            let mut points = Vec::new();
            let mut row = Vec::new();
            for bpp in BPP {
                let bps = (bpp * f64::from(v.width) * f64::from(v.height) * fps) as u64;
                let out = dir.join(format!("{}-{name}-{bpp}.mp4", clip.id));
                let mut args: Vec<String> = Vec::new();
                args.extend(enc.pre.clone());
                args.extend([
                    "-t".into(),
                    seconds.to_string(),
                    "-i".into(),
                    src.to_string_lossy().into_owned(),
                    "-map".into(),
                    "0:v:0".into(),
                    "-an".into(),
                ]);
                args.extend(enc.args(Rate::Bitrate(bps), false, fps));
                if let Some(f) = &enc.post_filter {
                    args.extend(["-vf".into(), format!("format={},{f}", enc.pix_fmt(false))]);
                }
                args.push(out.to_string_lossy().into_owned());
                if !run(&engine, &args) {
                    row.push("fail".to_string());
                    continue;
                }
                // The real size, as bits per pixel: hardware encoders miss the rate they are asked for.
                let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                let real = size as f64 * 8.0
                    / seconds.min(probe.duration.unwrap_or(seconds))
                    / (f64::from(v.width) * f64::from(v.height) * fps);
                let log = dir.join("v.json");
                // Compare the first `seconds` of the source with the result.
                let refclip = dir.join("ref.mp4");
                let ok = run(
                    &engine,
                    &[
                        "-t".into(),
                        seconds.to_string(),
                        "-i".into(),
                        src.to_string_lossy().into_owned(),
                        "-map".into(),
                        "0:v:0".into(),
                        "-c".into(),
                        "copy".into(),
                        refclip.to_string_lossy().into_owned(),
                    ],
                );
                let score = if ok {
                    vmaf(&engine, &refclip, &out, v.width, v.height, &log)
                } else {
                    None
                };
                match score {
                    Some(s) => {
                        points.push((real, s));
                        row.push(format!("{s:.1} ({real:.3})"));
                    }
                    None => row.push("n/a".to_string()),
                }
                let _ = std::fs::remove_file(&out);
            }
            println!("| {} | {} |", clip.id, row.join(" | "));
            points.sort_by(|a, b| a.0.total_cmp(&b.0));
            // Make the curve non-decreasing so a noisy point cannot create two crossings.
            let mut best = 0.0f64;
            for p in &mut points {
                best = best.max(p.1);
                p.1 = best;
            }
            if let Some(f) = crossing(&points, 80.0) {
                lows.push(f);
            }
            if let Some(k) = crossing(&points, 93.0) {
                knees.push(k);
            }
        }
        floors.push((name.to_string(), lows, knees));
        println!();
    }
    println!("## Floors (geometric mean over the clips that crossed the level)\n");
    println!("| encoder | floor bpp (VMAF 80) | knee bpp (VMAF 93) | clips |");
    println!("|---|---|---|---|");
    for (name, lows, knees) in &floors {
        let gm = |v: &Vec<f64>| {
            if v.is_empty() {
                f64::NAN
            } else {
                (v.iter().map(|x| x.ln()).sum::<f64>() / v.len() as f64).exp()
            }
        };
        println!(
            "| {name} | {:.4} | {:.4} | {} / {} |",
            gm(lows),
            gm(knees),
            lows.len(),
            knees.len()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
