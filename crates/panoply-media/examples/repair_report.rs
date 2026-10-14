//! Recovery rates of `media.repair` on deliberately broken copies of corpus files: truncated at
//! 10, 50 and 90 percent, the index stripped, bytes zeroed in the middle. The baseline is what the
//! ffmpeg command line salvages from the same file (`ffmpeg -i broken -c copy`); untrunc is not
//! installed on the machine this was measured on, so it is not compared. "Recovered" is the share
//! of the pictures that are really in the file's bytes (counted from the intact original's index)
//! that come out of the result, and a result counts only when it decodes without errors.
//!
//! Run with `cargo run --release -p panoply-media --example repair_report`.

use std::path::{Path, PathBuf};
use std::process::Command;

use panoply_media::engine::Engine;
use panoply_media::io::OpenIo;
use panoply_media::modules;
use panoply_media::mp4::{boxes, movie::parse_movie, source::Source};

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/cache/video")
        .join(rel)
}

fn frames(engine: &Engine, p: &Path) -> Option<(usize, String)> {
    let o = Command::new(&engine.ffmpeg)
        .args(["-nostdin", "-hide_banner", "-v", "error", "-i"])
        .arg(p)
        .args(["-map", "0:v:0", "-f", "null", "-"])
        .output()
        .ok()?;
    let err: Vec<String> = String::from_utf8_lossy(&o.stderr)
        .lines()
        .filter(|l| !l.contains("non monotonically") && !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    let pr = Command::new(&engine.ffprobe)
        .args([
            "-v",
            "error",
            "-count_frames",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_read_frames",
            "-of",
            "csv=p=0",
        ])
        .arg(p)
        .output()
        .ok()?;
    let n: usize = String::from_utf8_lossy(&pr.stdout)
        .trim()
        .trim_end_matches(',')
        .parse()
        .ok()?;
    Some((n, err.first().cloned().unwrap_or_default()))
}

fn main() {
    assert!(Engine::use_path_programs(), "no ffmpeg on PATH");
    let engine = Engine::get(&OpenIo::new()).unwrap();
    let dir = std::env::temp_dir().join(format!("panoply-repair-report-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let repair = modules::modules()
        .into_iter()
        .find(|m| m.manifest.module.id == "media.repair")
        .unwrap();
    let sources = [
        (
            "sintel trailer 480p (video + audio, 52 s)",
            "blender/sintel-trailer-480p.mp4",
        ),
        (
            "phone portrait VFR (video + audio, 8 s)",
            "generated/vfr-phone-portrait.mp4",
        ),
        (
            "bbb 720p HEVC (video only, B-frames)",
            "blender/bbb-720p-hevc-10s.mp4",
        ),
        (
            "moov at end, 20 s (video + audio)",
            "generated/moov-at-end.mp4",
        ),
    ];
    println!(
        "| file | damage | pictures in the file | Panoply recovered | ffmpeg -c copy recovered |"
    );
    println!("|---|---|---|---|---|");
    for (label, rel) in sources {
        let src = corpus(rel);
        if !src.is_file() {
            continue;
        }
        // The intact file with its index first, so a cut of the data keeps the index.
        let front = dir.join("front.mp4");
        assert!(
            Command::new(&engine.ffmpeg)
                .args(["-nostdin", "-v", "error", "-y", "-i"])
                .arg(&src)
                .args(["-c", "copy", "-movflags", "+faststart"])
                .arg(&front)
                .status()
                .unwrap()
                .success()
        );
        let end = dir.join("end.mp4");
        assert!(
            Command::new(&engine.ffmpeg)
                .args(["-nostdin", "-v", "error", "-y", "-i"])
                .arg(&src)
                .args(["-c", "copy"])
                .arg(&end)
                .status()
                .unwrap()
                .success()
        );
        let (total_frames, _) = frames(&engine, &end).unwrap();
        let cases: Vec<(String, PathBuf, PathBuf, Option<PathBuf>)> = {
            let mut v = Vec::new();
            let fb = std::fs::read(&front).unwrap();
            for pct in [10, 50, 90] {
                let p = dir.join(format!("trunc{pct}.mp4"));
                std::fs::write(&p, &fb[..fb.len() * pct / 100]).unwrap();
                v.push((
                    format!("cut off at {pct}% (index kept)"),
                    p,
                    front.clone(),
                    None,
                ));
            }
            // Index stripped: all the data, no moov.
            let s = Source::open(&end).unwrap();
            let l = boxes::scan(&s).unwrap();
            let moov = l.find(b"moov").unwrap();
            let eb = std::fs::read(&end).unwrap();
            let p = dir.join("stripped.mp4");
            std::fs::write(&p, &eb[..moov.start as usize]).unwrap();
            v.push((
                "index stripped (all data kept)".into(),
                p,
                end.clone(),
                Some(end.clone()),
            ));
            // Index missing and 70% of the data (a recorder that crashed).
            let mdat = l.find(b"mdat").unwrap();
            let p = dir.join("crashed.mp4");
            std::fs::write(
                &p,
                &eb[..(mdat.payload() + (mdat.size - u64::from(mdat.header_len)) * 7 / 10)
                    as usize],
            )
            .unwrap();
            v.push((
                "recorder crashed at 70% (no index)".into(),
                p,
                end.clone(),
                Some(end.clone()),
            ));
            v
        };
        for (what, broken, intact, reference) in cases {
            // Pictures really in the broken file's bytes, from the intact index.
            let isrc = Source::open(&intact).unwrap();
            let il = boxes::scan(&isrc).unwrap();
            let m = il.find(b"moov").unwrap();
            let movie = parse_movie(
                &isrc
                    .read_exact_at(m.payload(), (m.size - 8) as usize)
                    .unwrap(),
            )
            .unwrap();
            let vt = movie.tracks.iter().find(|t| t.is_video()).unwrap();
            let len = std::fs::metadata(&broken).unwrap().len();
            // The intact file's data sits at different offsets in the cut copies only when the
            // index moved; for these cases offsets are the same, so count by position.
            let present = vt
                .samples
                .iter()
                .filter(|s| s.offset + u64::from(s.size) <= len)
                .count();
            let io = OpenIo::new().with_out_dir(dir.join("out"));
            let _ = std::fs::remove_dir_all(dir.join("out"));
            std::fs::create_dir_all(dir.join("out")).unwrap();
            let mut inputs = vec![broken.clone()];
            let mut opts = serde_json::json!({});
            if let Some(r) = &reference {
                inputs.push(r.clone());
                opts = serde_json::json!({"reference": r.to_string_lossy()});
            }
            let ours = match repair.run(&io, &inputs, &opts) {
                Ok(out) if !out.is_empty() => match frames(&engine, &out[0]) {
                    Some((n, err)) if err.is_empty() => {
                        format!("{n} ({:.0}%)", 100.0 * n as f64 / present.max(1) as f64)
                    }
                    Some((n, _)) => format!("{n} with decode errors"),
                    None => "unreadable".into(),
                },
                Ok(_) => "nothing written".into(),
                Err(e) => format!(
                    "refused: {}",
                    e.to_string().chars().take(50).collect::<String>()
                ),
            };
            let base = dir.join("base.mkv");
            let ok = Command::new(&engine.ffmpeg)
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-y",
                    "-err_detect",
                    "ignore_err",
                    "-i",
                ])
                .arg(&broken)
                .args(["-map", "0:v:0", "-c", "copy"])
                .arg(&base)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            let theirs = if ok {
                match frames(&engine, &base) {
                    Some((n, _)) => {
                        format!("{n} ({:.0}%)", 100.0 * n as f64 / present.max(1) as f64)
                    }
                    None => "unreadable".into(),
                }
            } else {
                "cannot open the file".into()
            };
            println!("| {label} | {what} | {present} | {ours} | {theirs} |");
            let _ = std::fs::remove_file(&base);
        }
        let _ = total_frames;
    }
    let _ = std::fs::remove_dir_all(&dir);
}
