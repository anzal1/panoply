//! Running the helper program: progress, cancel, bounded output.
//!
//! The helper is a separate process (ADR 0002: the LGPL ffmpeg build is never linked), so a
//! corrupt file can crash it without touching the app, and cancel is a kill. Its standard output
//! is read on a thread and its standard error is kept as a short tail, so memory stays flat
//! however long the job runs.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::engine::Engine;
use crate::error::{MediaError, Result};
use crate::io::Io;

/// How often the supervising loop looks at the child and the cancel flag.
const POLL: Duration = Duration::from_millis(15);
/// Lines of the helper's error output kept for the plain error message.
const TAIL_LINES: usize = 40;

/// Where a run's progress sits inside the whole job, and how to label it.
#[derive(Clone)]
pub struct Progress<'a> {
    /// Length of the output in seconds (progress is output time over this). `None` means the
    /// run reports no fractions.
    pub seconds: Option<f64>,
    /// This run covers `lo..hi` of the whole job's progress.
    pub lo: f32,
    /// See `lo`.
    pub hi: f32,
    /// What is happening, in a few words.
    pub label: &'a str,
}

impl<'a> Progress<'a> {
    /// A run that is the whole job.
    pub fn whole(seconds: Option<f64>, label: &'a str) -> Self {
        Self {
            seconds,
            lo: 0.0,
            hi: 1.0,
            label,
        }
    }

    /// A run that is one part of a bigger job.
    pub fn part(seconds: Option<f64>, lo: f32, hi: f32, label: &'a str) -> Self {
        Self {
            seconds,
            lo,
            hi,
            label,
        }
    }
}

/// What a finished helper run left behind.
pub struct Finished {
    /// The last lines of its error output.
    pub tail: String,
    /// True when it exited with status 0.
    pub ok: bool,
    /// Standard output when the run was asked to capture it.
    pub stdout: Vec<u8>,
}

fn command(engine: &Engine, program: &std::path::Path) -> Command {
    let mut c = Command::new(program);
    engine.configure(&mut c);
    c.stdin(Stdio::null());
    c
}

/// Run ffmpeg with these arguments. `-nostdin -hide_banner -progress pipe:1 -nostats` are
/// added. Returns when it exits; kills it when the user cancels.
pub fn run_ffmpeg(
    engine: &Engine,
    io: &dyn Io,
    args: &[OsString],
    progress: &Progress,
) -> Result<Finished> {
    let mut cmd = command(engine, &engine.ffmpeg);
    cmd.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
        "-nostats",
    ])
    .args(args)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    supervise(cmd, io, Some(progress), false)
}

/// Run a helper program and capture its standard output (ffprobe, `ffmpeg -encoders`).
pub fn capture(
    engine: &Engine,
    io: Option<&dyn Io>,
    program: &std::path::Path,
    args: &[OsString],
) -> Result<Finished> {
    let mut cmd = command(engine, program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    match io {
        Some(io) => supervise(cmd, io, None, true),
        None => supervise_no_cancel(cmd),
    }
}

struct NoCancel(panoply_core::CancelToken);
impl Io for NoCancel {
    fn read_path(&self, p: &std::path::Path) -> Result<std::path::PathBuf> {
        Ok(p.to_path_buf())
    }
    fn output_dir_for(&self, p: &std::path::Path) -> Result<std::path::PathBuf> {
        Ok(p.to_path_buf())
    }
    fn cancel(&self) -> &panoply_core::CancelToken {
        &self.0
    }
    fn progress(&self, _: f32, _: &str) {}
    fn note(&self, _: String) {}
}

fn supervise_no_cancel(cmd: Command) -> Result<Finished> {
    let io = NoCancel(panoply_core::CancelToken::new());
    supervise(cmd, &io, None, true)
}

struct Shared {
    out_us: AtomicU64,
    tail: Mutex<VecDeque<String>>,
}

fn supervise(
    mut cmd: Command,
    io: &dyn Io,
    progress: Option<&Progress>,
    capture_stdout: bool,
) -> Result<Finished> {
    io.check_cancel()?;
    let mut child = cmd.spawn().map_err(|e| {
        MediaError::Engine(format!(
            "Could not start the video engine ({e}). Reinstall Panoply, or set PANOPLY_FFMPEG to an ffmpeg program."
        ))
    })?;
    let shared = Arc::new(Shared {
        out_us: AtomicU64::new(0),
        tail: Mutex::new(VecDeque::new()),
    });
    let stderr = child.stderr.take();
    let stdout = child.stdout.take();
    let t_err = stderr.map(|s| {
        let sh = shared.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(s).lines().map_while(|l| l.ok()) {
                let mut t = sh.tail.lock().unwrap_or_else(|p| p.into_inner());
                if t.len() >= TAIL_LINES {
                    t.pop_front();
                }
                t.push_back(line);
            }
        })
    });
    let t_out = stdout.map(|s| {
        let sh = shared.clone();
        std::thread::spawn(move || -> Vec<u8> {
            if capture_stdout {
                let mut buf = Vec::new();
                // Capped: a probe answer is small. Anything past 64 MB is dropped.
                let _ = s.take(64 << 20).read_to_end(&mut buf);
                buf
            } else {
                for line in BufReader::new(s).lines().map_while(|l| l.ok()) {
                    if let Some(v) = line.strip_prefix("out_time_us=")
                        && let Ok(us) = v.trim().parse::<i64>()
                        && us >= 0
                    {
                        sh.out_us.store(us as u64, Ordering::Relaxed);
                    }
                }
                Vec::new()
            }
        })
    });

    let mut last_report = -1.0f32;
    let status = loop {
        if io.cancel().is_cancelled() {
            kill(&mut child);
            let _ = child.wait();
            join(t_err);
            join_out(t_out);
            return Err(MediaError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {}
            Err(e) => {
                kill(&mut child);
                return Err(MediaError::Engine(format!(
                    "The video engine stopped unexpectedly ({e})."
                )));
            }
        }
        if let Some(p) = progress
            && let Some(total) = p.seconds
            && total > 0.0
        {
            let t = shared.out_us.load(Ordering::Relaxed) as f64 / 1e6;
            let f = (t / total).clamp(0.0, 1.0) as f32;
            if (f - last_report).abs() >= 0.02 {
                last_report = f;
                io.progress(p.lo + (p.hi - p.lo) * f, p.label);
            }
        }
        std::thread::sleep(POLL);
    };
    join(t_err);
    let stdout = join_out(t_out);
    let tail = shared
        .tail
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(p) = progress {
        io.progress(p.hi, p.label);
    }
    Ok(Finished {
        tail,
        ok: status.success(),
        stdout,
    })
}

fn kill(child: &mut Child) {
    let _ = child.kill();
}

fn join(t: Option<std::thread::JoinHandle<()>>) {
    if let Some(t) = t {
        let _ = t.join();
    }
}

fn join_out(t: Option<std::thread::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    t.and_then(|t| t.join().ok()).unwrap_or_default()
}

/// Run ffmpeg with exactly these arguments (nothing added) and capture its standard output and
/// the tail of its error output. Used for measurements whose answer is printed.
pub fn capture_ffmpeg(engine: &Engine, io: &dyn Io, args: &[OsString]) -> Result<Finished> {
    let mut cmd = command(engine, &engine.ffmpeg);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    supervise(cmd, io, None, true)
}

/// Run ffmpeg and hand its standard output to `sink` in pieces as it arrives (decoded audio as
/// raw samples, for the loudness meter). Memory stays flat: pieces are at most 256 KB and the
/// reader runs at most a few pieces ahead. Cancel kills the helper.
pub fn run_ffmpeg_stream(
    engine: &Engine,
    io: &dyn Io,
    args: &[OsString],
    mut sink: impl FnMut(&[u8]),
    progress: &Progress,
    total_bytes: Option<u64>,
) -> Result<Finished> {
    run_ffmpeg_stream_until(
        engine,
        io,
        args,
        |c| {
            sink(c);
            true
        },
        progress,
        total_bytes,
    )
}

/// Like [`run_ffmpeg_stream`], but the sink says whether to go on. When it returns `false` (a
/// write failed, the disk is full) the helper is killed at once and the run ends with
/// `ok == false`, so a job does not decode hours of sound it can no longer keep.
pub fn run_ffmpeg_stream_until(
    engine: &Engine,
    io: &dyn Io,
    args: &[OsString],
    mut sink: impl FnMut(&[u8]) -> bool,
    progress: &Progress,
    total_bytes: Option<u64>,
) -> Result<Finished> {
    io.check_cancel()?;
    let mut cmd = command(engine, &engine.ffmpeg);
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error"])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        MediaError::Engine(format!(
            "Could not start the video engine ({e}). Reinstall Panoply, or set PANOPLY_FFMPEG to an ffmpeg program."
        ))
    })?;
    let tail = Arc::new(Mutex::new(VecDeque::<String>::new()));
    let stderr = child.stderr.take();
    let t_err = stderr.map(|s| {
        let tail = tail.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(s).lines().map_while(|l| l.ok()) {
                let mut t = tail.lock().unwrap_or_else(|p| p.into_inner());
                if t.len() >= TAIL_LINES {
                    t.pop_front();
                }
                t.push_back(line);
            }
        })
    });
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(4);
    let stdout = child.stdout.take();
    let t_out = stdout.map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        })
    });
    let mut seen = 0u64;
    let mut last = -1.0f32;
    loop {
        if io.cancel().is_cancelled() {
            kill(&mut child);
            let _ = child.wait();
            drop(rx);
            join(t_out);
            join(t_err);
            return Err(MediaError::Cancelled);
        }
        match rx.recv_timeout(POLL) {
            Ok(chunk) => {
                seen += chunk.len() as u64;
                if !sink(&chunk) {
                    kill(&mut child);
                    let _ = child.wait();
                    drop(rx);
                    join(t_out);
                    join(t_err);
                    return Ok(Finished {
                        tail: "stopped by the program that was reading the sound".into(),
                        ok: false,
                        stdout: Vec::new(),
                    });
                }
                if let Some(total) = total_bytes.filter(|t| *t > 0) {
                    let f = (seen as f64 / total as f64).clamp(0.0, 1.0) as f32;
                    if (f - last).abs() >= 0.02 {
                        last = f;
                        io.progress(
                            progress.lo + (progress.hi - progress.lo) * f,
                            progress.label,
                        );
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child
        .wait()
        .map_err(|e| MediaError::Engine(format!("The video engine stopped unexpectedly ({e}).")))?;
    join(t_out);
    join(t_err);
    let tail = tail
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    io.progress(progress.hi, progress.label);
    Ok(Finished {
        tail,
        ok: status.success(),
        stdout: Vec::new(),
    })
}

/// Turn the helper's last error lines into a plain sentence with a next step.
pub fn explain(tail: &str, what: &str) -> MediaError {
    let t = tail.to_ascii_lowercase();
    let has = |s: &str| t.contains(s);
    if has("moov atom not found") || has("truncated") && has("moov") {
        return MediaError::Corrupt(format!(
            "{what} looks cut off: the end of the file, where the index lives, is missing. Try Repair video."
        ));
    }
    if has("invalid data found when processing input") || has("error while decoding") {
        return MediaError::Corrupt(format!(
            "{what} has damaged data in it. Try Repair video, or export it again from the original."
        ));
    }
    if has("no space left on device") {
        return MediaError::Io(
            "The disk is full, so the result could not be written. Free some space and try again."
                .into(),
        );
    }
    if has("permission denied") {
        return MediaError::Io(format!(
            "Panoply does not have permission to write the result for {what}. Choose another output folder."
        ));
    }
    if has("could not find tag for codec") || has("codec not currently supported in container") {
        return MediaError::Unsupported(format!(
            "{what} holds a kind of video or audio that this format cannot carry. Pick another format such as MKV."
        ));
    }
    if has("unknown encoder") || has("encoder") && has("not found") {
        return MediaError::Unsupported(
            "This computer has no encoder for that format. Pick another format, such as MP4 with H.264."
                .into(),
        );
    }
    if has("unrecognized option") || has("option not found") {
        return MediaError::Engine(
            "The video engine installed here is too old for this job. Update it, or remove PANOPLY_FFMPEG so Panoply fetches its own."
                .into(),
        );
    }
    let last = tail
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    if last.is_empty() {
        MediaError::Failed(format!(
            "The video engine could not process {what}. Check that the file plays, then try again."
        ))
    } else {
        MediaError::Failed(format!(
            "The video engine could not process {what} ({}). Check that the file plays, then try again.",
            shorten(last)
        ))
    }
}

fn shorten(s: &str) -> String {
    let s: String = s.chars().take(160).collect();
    s.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_off_file_is_explained_in_plain_words() {
        let e = explain(
            "[mov,mp4] moov atom not found\nError opening input",
            "clip.mp4",
        );
        assert!(e.to_string().contains("cut off"), "{e}");
        assert!(e.to_string().contains("Repair video"));
    }

    #[test]
    fn unknown_failures_keep_a_short_last_line() {
        let e = explain("something odd happened\n", "a.mov");
        assert!(e.to_string().contains("something odd"), "{e}");
        assert!(!e.to_string().contains('\n'));
    }

    #[test]
    fn disk_full_is_its_own_message() {
        let e = explain(
            "av_interleaved_write_frame(): No space left on device",
            "a.mov",
        );
        assert!(e.to_string().contains("disk is full"));
    }
}
