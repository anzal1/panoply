//! What a frame-exact cut costs: for several clips and ranges, the share of the output's video
//! packets that are byte-identical to the source's (the rest were re-encoded), and the SSIM of
//! the output against the same range of the source, mean and worst frame. A cut that only
//! copies would show 100% and an SSIM of 1.0; the smart cut re-encodes the edge groups of
//! pictures only. Run after `cargo build --release -p panoply-cli`:
//! `nice -n 10 cargo run --release -p panoply-media --example cut_report`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use panoply_media::engine::Engine;
use panoply_media::io::OpenIo;

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/cache/video")
        .join(rel)
}

fn panoply_bin() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().join("panoply")
}

fn packet_hashes(engine: &Engine, p: &Path) -> Vec<String> {
    let o = Command::new(&engine.ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries"])
        .arg("packet=data_hash")
        .args(["-show_data_hash", "MD5", "-of", "csv=p=0"])
        .arg(p)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

fn ssim(engine: &Engine, src: &Path, start: f64, len: f64, out: &Path) -> Option<(f64, f64)> {
    let log = std::env::temp_dir().join(format!("cut-ssim-{}.log", std::process::id()));
    let st = Command::new(&engine.ffmpeg)
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(out)
        .args(["-ss", &start.to_string(), "-t", &len.to_string(), "-i"])
        .arg(src)
        .args(["-filter_complex"])
        .arg(format!(
            "[0:v]setpts=PTS-STARTPTS[a];[1:v]setpts=PTS-STARTPTS[b];[a][b]ssim=stats_file={}",
            log.display()
        ))
        .args(["-f", "null", "-"])
        .stdout(Stdio::null())
        .status()
        .ok()?;
    if !st.success() {
        return None;
    }
    let text = std::fs::read_to_string(&log).ok()?;
    let _ = std::fs::remove_file(&log);
    let mut all = Vec::new();
    for l in text.lines() {
        if let Some(i) = l.find("All:") {
            let v: f64 = l[i + 4..].split_whitespace().next()?.parse().ok()?;
            all.push(v);
        }
    }
    let mean = all.iter().sum::<f64>() / all.len().max(1) as f64;
    let worst = all.iter().copied().fold(1.0, f64::min);
    Some((mean, worst))
}

fn main() {
    assert!(Engine::use_path_programs(), "no ffmpeg on PATH");
    let engine = Engine::get(&OpenIo::new()).unwrap();
    let dir = std::env::temp_dir().join(format!("panoply-cut-report-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cases = [
        (
            "bbb 1080p H.264, 60 fps",
            "blender/bbb-1080p-h264-10s.mp4",
            1.0,
            9.0,
        ),
        (
            "bbb 1080p H.264, 60 fps",
            "blender/bbb-1080p-h264-10s.mp4",
            2.5,
            3.3,
        ),
        (
            "sintel 720p H.264",
            "blender/sintel-720p-h264-10s.mp4",
            1.3,
            8.7,
        ),
        (
            "bbb 720p HEVC, B-frames",
            "blender/bbb-720p-hevc-10s.mp4",
            1.2,
            8.8,
        ),
        (
            "sintel trailer 480p, with sound",
            "blender/sintel-trailer-480p.mp4",
            5.0,
            45.0,
        ),
        (
            "moov at the end, with sound",
            "generated/moov-at-end.mp4",
            3.0,
            17.0,
        ),
    ];
    println!(
        "| clip | range (s) | wall ms | video packets copied untouched | SSIM mean | SSIM worst frame |"
    );
    println!("|---|---|---:|---:|---:|---:|");
    for (label, rel, a, b) in cases {
        let src = corpus(rel);
        if !src.is_file() {
            continue;
        }
        let out = dir.join("out");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let t = Instant::now();
        let ok = Command::new(panoply_bin())
            .args(["video", "trim"])
            .arg(&src)
            .args(["--keep", &format!("{a}-{b}"), "--out"])
            .arg(&out)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let Some(res) = std::fs::read_dir(&out)
            .ok()
            .and_then(|mut d| d.next())
            .and_then(|e| e.ok())
            .map(|e| e.path())
        else {
            println!("| {label} | {a}-{b} | - | failed | - | - |");
            continue;
        };
        if !ok {
            println!("| {label} | {a}-{b} | - | failed | - | - |");
            continue;
        }
        let src_pk = packet_hashes(&engine, &src);
        let out_pk = packet_hashes(&engine, &res);
        let shared = out_pk.iter().filter(|h| src_pk.contains(h)).count();
        let s = ssim(&engine, &src, a, b - a, &res);
        println!(
            "| {label} | {a} to {b} | {ms:.0} | {shared} of {} ({:.0}%) | {} | {} |",
            out_pk.len(),
            100.0 * shared as f64 / out_pk.len().max(1) as f64,
            s.map(|x| format!("{:.4}", x.0)).unwrap_or("n/a".into()),
            s.map(|x| format!("{:.4}", x.1)).unwrap_or("n/a".into())
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
