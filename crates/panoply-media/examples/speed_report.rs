//! Wall time of Panoply against the ffmpeg command line for the same operation on the same file,
//! median of several runs, both writing a real output file. Panoply runs through the `panoply`
//! binary (process start included, which is what a user pays). Build the binary first:
//! `cargo build --release -p panoply-cli`, then
//! `cargo run --release -p panoply-media --example speed_report -- [runs]`.
//!
//! The ffmpeg command for each row is printed, so the comparison can be checked by hand.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/cache")
        .join(rel)
}

fn panoply_bin() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let release = exe.parent().unwrap().parent().unwrap().to_path_buf();
    release.join("panoply")
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn time(runs: usize, mut f: impl FnMut() -> bool) -> Option<f64> {
    let mut t = Vec::new();
    for _ in 0..runs {
        let s = Instant::now();
        if !f() {
            return None;
        }
        t.push(s.elapsed().as_secs_f64() * 1000.0);
    }
    Some(median(t))
}

fn sh(cmd: &Path, args: &[String]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

struct Row {
    name: String,
    file: String,
    ours: Option<f64>,
    theirs: Option<f64>,
    cmd: String,
}

fn main() {
    let runs: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let ours = panoply_bin();
    assert!(
        ours.is_file(),
        "build the release `panoply` first: {}",
        ours.display()
    );
    let ffmpeg = which("ffmpeg");
    let ffprobe = which("ffprobe");
    let work = std::env::temp_dir().join(format!("panoply-speed-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let mut rows: Vec<Row> = Vec::new();
    let s = |x: &str| x.to_string();
    // Each case: name, corpus file, our args (with {out} for the folder), ffmpeg args (with {out} for a file).
    let cases: Vec<(&str, &str, Vec<String>, Vec<String>)> = vec![
        (
            "Convert, container change (copy)",
            "video/blender/bbb-1080p-h264-10s.mp4",
            vec![
                s("video"),
                s("convert"),
                s("{in}"),
                s("--to"),
                s("mkv"),
                s("--out"),
                s("{dir}"),
            ],
            vec![
                s("-i"),
                s("{in}"),
                s("-map"),
                s("0"),
                s("-c"),
                s("copy"),
                s("{out}.mkv"),
            ],
        ),
        (
            "Convert, container change (copy), 4K",
            "video/generated/4k-hevc-5s.mp4",
            vec![
                s("video"),
                s("convert"),
                s("{in}"),
                s("--to"),
                s("mov"),
                s("--out"),
                s("{dir}"),
            ],
            vec![
                s("-i"),
                s("{in}"),
                s("-map"),
                s("0"),
                s("-c"),
                s("copy"),
                s("-tag:v"),
                s("hvc1"),
                s("-movflags"),
                s("+faststart"),
                s("{out}.mov"),
            ],
        ),
        (
            "Extract audio (copy)",
            "video/blender/sintel-trailer-480p.mp4",
            vec![s("video"), s("audio"), s("{in}"), s("--out"), s("{dir}")],
            vec![
                s("-i"),
                s("{in}"),
                s("-vn"),
                s("-c:a"),
                s("copy"),
                s("{out}.m4a"),
            ],
        ),
        (
            "Trim to keyframes (copy)",
            "video/blender/bbb-1080p-h264-10s.mp4",
            vec![
                s("video"),
                s("trim"),
                s("{in}"),
                s("--keep"),
                s("1-9"),
                s("--keyframes"),
                s("--out"),
                s("{dir}"),
            ],
            vec![
                s("-ss"),
                s("1"),
                s("-to"),
                s("9"),
                s("-i"),
                s("{in}"),
                s("-c"),
                s("copy"),
                s("-movflags"),
                s("+faststart"),
                s("{out}.mp4"),
            ],
        ),
        (
            "Trim to the frame (smart cut) against a full re-encode of the clip",
            "video/blender/bbb-1080p-h264-10s.mp4",
            vec![
                s("video"),
                s("trim"),
                s("{in}"),
                s("--keep"),
                s("1-9"),
                s("--out"),
                s("{dir}"),
            ],
            vec![
                s("-ss"),
                s("1"),
                s("-to"),
                s("9"),
                s("-i"),
                s("{in}"),
                s("-c:v"),
                s("h264_videotoolbox"),
                s("-b:v"),
                s("8400k"),
                s("-c:a"),
                s("copy"),
                s("-movflags"),
                s("+faststart"),
                s("{out}.mp4"),
            ],
        ),
        (
            "Compress to 5 MB (verify loop, no quality measure) against one encode at the same rate",
            "video/blender/bbb-1080p-h264-10s.mp4",
            vec![
                s("video"),
                s("compress"),
                s("{in}"),
                s("--under"),
                s("5mb"),
                s("--codec"),
                s("h264"),
                s("--no-measure"),
                s("--out"),
                s("{dir}"),
            ],
            vec![
                s("-i"),
                s("{in}"),
                s("-c:v"),
                s("h264_videotoolbox"),
                s("-b:v"),
                s("3900k"),
                s("-an"),
                s("-movflags"),
                s("+faststart"),
                s("{out}.mp4"),
            ],
        ),
        (
            "Normalize to -16 LUFS against two-pass loudnorm",
            "audio/commons/recorder-improvisation.mp3",
            vec![
                s("audio"),
                s("normalize"),
                s("{in}"),
                s("--target"),
                s("-16"),
                s("--out"),
                s("{dir}"),
            ],
            vec![
                s("-i"),
                s("{in}"),
                s("-af"),
                s("loudnorm=I=-16:TP=-1:LRA=11"),
                s("-c:a"),
                s("libmp3lame"),
                s("-q:a"),
                s("2"),
                s("{out}.mp3"),
            ],
        ),
        (
            "Stereo to mono (looks at the channels first) against -ac 1",
            "audio/commons/agnew-speech.ogg",
            vec![
                s("audio"),
                s("channels"),
                s("{in}"),
                s("--mono"),
                s("--out"),
                s("{dir}"),
            ],
            vec![
                s("-i"),
                s("{in}"),
                s("-ac"),
                s("1"),
                s("-c:a"),
                s("libopus"),
                s("-b:a"),
                s("64k"),
                s("{out}.ogg"),
            ],
        ),
        (
            "HDR to SDR against a plain 8-bit H.264 encode",
            "video/generated/hdr10-hevc-10bit.mp4",
            vec![s("video"), s("sdr"), s("{in}"), s("--out"), s("{dir}")],
            vec![
                s("-i"),
                s("{in}"),
                s("-c:v"),
                s("h264_videotoolbox"),
                s("-b:v"),
                s("4000k"),
                s("-pix_fmt"),
                s("yuv420p"),
                s("-c:a"),
                s("copy"),
                s("{out}.mp4"),
            ],
        ),
    ];
    for (name, file, ours_args, ff_args) in cases {
        let input = corpus(file);
        if !input.is_file() {
            eprintln!("skipping {name}: {} is not in the corpus", input.display());
            continue;
        }
        let dir = work.join("o");
        let fill = |a: &[String], n: u32| -> Vec<String> {
            a.iter()
                .map(|x| {
                    x.replace("{in}", &input.to_string_lossy())
                        .replace("{dir}", &dir.to_string_lossy())
                        .replace("{out}", &work.join(format!("ff{n}")).to_string_lossy())
                })
                .collect()
        };
        let mut n = 0u32;
        let theirs = time(runs, || {
            n += 1;
            let mut args: Vec<String> = ["-nostdin", "-hide_banner", "-v", "error", "-y"]
                .map(String::from)
                .to_vec();
            args.extend(fill(&ff_args, n));
            let ok = sh(&ffmpeg, &args);
            for e in ["mkv", "mov", "m4a", "mp4", "mp3", "ogg"] {
                let _ = std::fs::remove_file(work.join(format!("ff{n}.{e}")));
            }
            ok
        });
        let o = time(runs, || {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            sh(&ours, &fill(&ours_args, 0))
        });
        let cmd = format!(
            "ffmpeg -y {}",
            fill(&ff_args, 0)
                .join(" ")
                .replace(&work.to_string_lossy().to_string(), "$TMP")
                .replace(&input.to_string_lossy().to_string(), file)
        );
        rows.push(Row {
            name: name.into(),
            file: file.into(),
            ours: o,
            theirs,
            cmd,
        });
    }
    // Info against ffprobe.
    let info_file = corpus("video/generated/1h-lowres.mp4");
    if info_file.is_file() {
        let o = time(runs, || {
            sh(
                &ours,
                &[
                    "video".into(),
                    "info".into(),
                    info_file.to_string_lossy().into_owned(),
                ],
            )
        });
        let t = time(runs, || {
            sh(
                &ffprobe,
                &[
                    "-v".into(),
                    "error".into(),
                    "-show_format".into(),
                    "-show_streams".into(),
                    info_file.to_string_lossy().into_owned(),
                ],
            )
        });
        rows.push(Row {
            name: "Info, 1 hour video".into(),
            file: "video/generated/1h-lowres.mp4".into(),
            ours: o,
            theirs: t,
            cmd: "ffprobe -show_format -show_streams".into(),
        });
    }
    println!("| operation | file | Panoply (ms) | ffmpeg CLI (ms) | ratio |");
    println!("|---|---|---|---|---|");
    for r in &rows {
        let ratio = match (r.ours, r.theirs) {
            (Some(a), Some(b)) => format!("{:.2}", a / b),
            _ => "n/a".into(),
        };
        let f = |x: Option<f64>| {
            x.map(|v| format!("{v:.0}"))
                .unwrap_or_else(|| "failed".into())
        };
        println!(
            "| {} | {} | {} | {} | {} |",
            r.name,
            r.file,
            f(r.ours),
            f(r.theirs),
            ratio
        );
    }
    println!();
    for r in &rows {
        println!("- {}: `{}`", r.name, r.cmd);
    }
    let _ = std::fs::remove_dir_all(&work);
}

fn which(name: &str) -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(name))
                .find(|p| p.is_file())
        })
        .unwrap_or_else(|| panic!("{name} is not on PATH"))
}
