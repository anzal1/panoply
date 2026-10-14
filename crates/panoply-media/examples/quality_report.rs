//! Quality at equal size: Panoply's `media.compress` against other encoders given the same number
//! of bytes, scored with VMAF (libvmaf, BSD) against the source on the whole clip.
//!
//! For each clip and each target size the module is run first; every other contender is then
//! driven to the same output size (within 3%, up to five tries, bit rate scaled by
//! `target / actual`) and scored the same way. Contenders other than Panoply are ffmpeg command
//! lines, printed with the results. x264 is GPL and is NOT something Panoply ships: it is here
//! only as the usual yardstick, so the table shows how far the permissive and hardware encoders are
//! from the best software one. Run with:
//! `nice -n 10 cargo run --release -p panoply-media --example quality_report`
//! after `cargo build --release -p panoply-cli`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use panoply_media::engine::Engine;
use panoply_media::io::OpenIo;
use panoply_media::ops::quality::parse_vmaf;

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/cache/video")
        .join(rel)
}

fn size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

fn ffmpeg(engine: &Engine, args: &[String]) -> bool {
    Command::new(&engine.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-y"])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn vmaf(engine: &Engine, src: &Path, enc: &Path, w: u32, h: u32, log: &Path) -> Option<f64> {
    let f = format!(
        "[0:v]scale={w}:{h}:flags=bicubic,format=yuv420p,setpts=PTS-STARTPTS[d];[1:v]format=yuv420p,setpts=PTS-STARTPTS[r];[d][r]libvmaf=n_threads=6:n_subsample=2:log_fmt=json:log_path={}",
        log.display()
    );
    let ok = Command::new(&engine.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(enc)
        .arg("-i")
        .arg(src)
        .args([
            "-map_metadata",
            "-1",
            "-lavfi",
            &f,
            "-an",
            "-f",
            "null",
            "-",
        ])
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    parse_vmaf(&std::fs::read_to_string(log).ok()?, 0.0).map(|q| q.score)
}

struct Contender {
    name: &'static str,
    /// ffmpeg output arguments for a bit rate in bits per second.
    args: fn(u64) -> Vec<String>,
    gpl: bool,
}

fn s(x: &str) -> String {
    x.to_string()
}

fn main() {
    assert!(Engine::use_path_programs(), "no ffmpeg on PATH");
    let engine = Engine::get(&OpenIo::new()).unwrap();
    let ours = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("panoply");
    assert!(ours.is_file(), "build the release `panoply` first");
    let work = std::env::temp_dir().join(format!("panoply-quality-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let contenders = [
        Contender {
            name: "h264_videotoolbox (naive bit rate, no scaling)",
            gpl: false,
            args: |b| {
                vec![
                    s("-c:v"),
                    s("h264_videotoolbox"),
                    s("-b:v"),
                    b.to_string(),
                    s("-pix_fmt"),
                    s("yuv420p"),
                ]
            },
        },
        Contender {
            name: "hevc_videotoolbox",
            gpl: false,
            args: |b| {
                vec![
                    s("-c:v"),
                    s("hevc_videotoolbox"),
                    s("-b:v"),
                    b.to_string(),
                    s("-tag:v"),
                    s("hvc1"),
                ]
            },
        },
        Contender {
            name: "SVT-AV1 preset 8",
            gpl: false,
            args: |b| {
                vec![
                    s("-c:v"),
                    s("libsvtav1"),
                    s("-preset"),
                    s("8"),
                    s("-b:v"),
                    b.to_string(),
                ]
            },
        },
        Contender {
            name: "x264 medium (GPL, yardstick only)",
            gpl: true,
            args: |b| {
                vec![
                    s("-c:v"),
                    s("libx264"),
                    s("-preset"),
                    s("medium"),
                    s("-b:v"),
                    b.to_string(),
                    s("-pix_fmt"),
                    s("yuv420p"),
                ]
            },
        },
    ];
    let clips = [
        (
            "vid-bbb-1080-h264",
            "blender/bbb-1080p-h264-10s.mp4",
            3_000_000u64,
        ),
        (
            "vid-sintel-720-h264",
            "blender/sintel-720p-h264-10s.mp4",
            1_500_000,
        ),
        (
            "vid-bbb-720-hevc",
            "blender/bbb-720p-hevc-10s.mp4",
            2_500_000,
        ),
        (
            "vid-sintel-trailer-480p",
            "blender/sintel-trailer-480p.mp4",
            1_000_000,
        ),
    ];
    println!(
        "| clip | target | Panoply compress (default codec) | Panoply compress --codec av1 | {} |",
        contenders
            .iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
            .join(" | ")
    );
    println!(
        "|---|---|---|---|{}|",
        contenders
            .iter()
            .map(|_| "---")
            .collect::<Vec<_>>()
            .join("|")
    );
    for (id, file, target) in clips {
        let src = corpus(file);
        if !src.is_file() {
            continue;
        }
        let probe = panoply_media::probe::probe(&engine, &OpenIo::new(), &src).unwrap();
        let v = probe.video().unwrap().clone();
        let dur = probe.duration.unwrap();
        let log = work.join("v.json");
        // Panoply: the module, no quality measure (it is scored the same way as the others below).
        let run_ours = |codec: Option<&str>| -> (String, u64) {
            let outdir = work.join(format!("{id}-{}", codec.unwrap_or("auto")));
            let _ = std::fs::remove_dir_all(&outdir);
            std::fs::create_dir_all(&outdir).unwrap();
            let mut cmd = Command::new(&ours);
            cmd.args(["video", "compress"]).arg(&src).args([
                "--under",
                &format!("{target}b"),
                "--no-measure",
                "--out",
            ]);
            cmd.arg(&outdir);
            if let Some(c) = codec {
                cmd.args(["--codec", c]);
            }
            let ok = cmd
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            let out = std::fs::read_dir(&outdir)
                .ok()
                .and_then(|mut d| d.next())
                .and_then(|e| e.ok())
                .map(|e| e.path());
            match (ok, out) {
                (true, Some(p)) => {
                    let sz = size(&p);
                    let pv = panoply_media::probe::probe(&engine, &OpenIo::new(), &p).unwrap();
                    let vv = pv.video().unwrap();
                    let score = vmaf(&engine, &src, &p, v.width, v.height, &log);
                    (
                        format!(
                            "{} {} at {}x{} ({} B)",
                            score.map(|x| format!("{x:.1}")).unwrap_or("n/a".into()),
                            vv.codec,
                            vv.width,
                            vv.height,
                            sz
                        ),
                        sz,
                    )
                }
                _ => ("failed".to_string(), target),
            }
        };
        let (our_cell, our_size) = run_ours(None);
        let (av1_cell, _) = run_ours(Some("av1"));
        let mut cells = Vec::new();
        for c in &contenders {
            // Drive the encoder to Panoply's size: scale the rate by target / actual until within 3%.
            let mut bps = (our_size as f64 * 8.0 / dur * 0.98) as u64;
            let mut best: Option<(PathBuf, u64)> = None;
            for _ in 0..5 {
                let out = work.join("c.mp4");
                let mut args: Vec<String> = vec![
                    s("-i"),
                    src.to_string_lossy().into_owned(),
                    s("-map"),
                    s("0:v:0"),
                    s("-an"),
                ];
                args.extend((c.args)(bps));
                args.push(out.to_string_lossy().into_owned());
                if !ffmpeg(&engine, &args) {
                    break;
                }
                let sz = size(&out);
                let keep = work.join("keep.mp4");
                let _ = std::fs::copy(&out, &keep);
                best = Some((keep, sz));
                let ratio = our_size as f64 / sz.max(1) as f64;
                if (0.97..=1.03).contains(&ratio) {
                    break;
                }
                bps = (bps as f64 * ratio) as u64;
            }
            cells.push(match best {
                Some((p, sz)) => {
                    let score = vmaf(&engine, &src, &p, v.width, v.height, &log);
                    format!(
                        "{} ({} B)",
                        score.map(|x| format!("{x:.1}")).unwrap_or("n/a".into()),
                        sz
                    )
                }
                None => "n/a".into(),
            });
            if c.gpl {
                // marked in the header; nothing else to do
            }
        }
        println!(
            "| {id} | {} B | {our_cell} | {av1_cell} | {} |",
            target,
            cells.join(" | ")
        );
    }
    let _ = std::fs::remove_dir_all(&work);
}
