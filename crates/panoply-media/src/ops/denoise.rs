//! `media.denoise`: take background noise out of a recording or the sound of a video (spec 5.5).
//!
//! The engine is RNNoise in pure Rust (`nnnoiseless`, BSD-3-Clause). Its model is inside the
//! program, so nothing is downloaded for it. Each chosen sound track is decoded by the helper to
//! 32-bit samples at 48 kHz (the rate the model works at, so other rates are resampled in and
//! back out), cleaned one channel at a time while it streams past, and parked in a raw file. A
//! second helper pass then writes the file again with that track taken from the raw file and
//! encoded once. Video, subtitles, chapters and the other tracks are copied untouched.
//!
//! Memory stays flat: the decoder is read in pieces of at most 256 KB, the cleaner holds one
//! frame per channel (10 ms), and the raw file lives on disk.
//!
//! Strength is the share of the cleaned signal in the mix with the original (the dry signal is
//! delayed by the model's own latency so the two line up).

use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use nnnoiseless::DenoiseState;

use crate::error::{MediaError, Result};
use crate::io::{Io, Naming, tool_path};
use crate::opts::Opts;
use crate::probe::Stream;
use crate::proc::{self, Progress};
use crate::units::name_of;

use super::audio;
use super::rewrite::{self, Replacement, Treatment};
use super::{Job, with_name};

/// The rate the model works at.
pub const MODEL_RATE: u32 = 48_000;
/// Samples the model takes per call, per channel (10 ms).
pub const FRAME: usize = DenoiseState::FRAME_SIZE;
/// The model wants samples on the scale of 16-bit integers.
const SCALE: f32 = 32768.0;

/// How much of the original stays in the mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strength {
    /// Keeps a good part of the original: the lightest touch.
    Light,
    /// The everyday setting.
    Normal,
    /// Only the cleaned signal.
    Strong,
}

impl Strength {
    /// Parse the option value.
    pub fn from_name(n: &str) -> Option<Strength> {
        match n {
            "light" => Some(Self::Light),
            "normal" | "" => Some(Self::Normal),
            "strong" => Some(Self::Strong),
            _ => None,
        }
    }

    /// The share of the cleaned signal in the mix, 0 to 1.
    pub fn wet(self) -> f32 {
        match self {
            Self::Light => 0.6,
            Self::Normal => 0.85,
            Self::Strong => 1.0,
        }
    }

    /// The word for the report.
    pub fn word(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Normal => "normal",
            Self::Strong => "strong",
        }
    }
}

/// One channel's model state and the samples waiting for it.
struct Lane {
    state: Box<DenoiseState<'static>>,
    /// Samples received and not yet part of a whole frame.
    pending: Vec<f32>,
    /// The previous input frame: the model answers one frame late, and this is what the cleaned
    /// frame is mixed with.
    prev: [f32; FRAME],
}

/// RNNoise over interleaved samples, any channel count, with a wet and dry mix. Feed it with
/// [`Denoiser::push`], end it with [`Denoiser::finish`]; the output has exactly as many frames
/// as the input and lines up with it.
pub struct Denoiser {
    lanes: Vec<Lane>,
    wet: f32,
    frames_in: u64,
    frames_out: u64,
    /// Model calls so far. The first call only warms the model up and gives no output.
    steps: u64,
    ended: bool,
}

impl Denoiser {
    /// A cleaner for `channels` channels at 48 kHz.
    pub fn new(channels: usize, wet: f32) -> Self {
        Self {
            lanes: (0..channels.max(1))
                .map(|_| Lane {
                    state: DenoiseState::new(),
                    pending: Vec::new(),
                    prev: [0.0; FRAME],
                })
                .collect(),
            wet: wet.clamp(0.0, 1.0),
            frames_in: 0,
            frames_out: 0,
            steps: 0,
            ended: false,
        }
    }

    /// Frames (one sample per channel) fed in so far.
    pub fn frames_in(&self) -> u64 {
        self.frames_in
    }

    /// Add interleaved samples. The cleaned samples that are ready are appended to `out`.
    pub fn push(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        let ch = self.lanes.len();
        let whole = interleaved.len() / ch * ch;
        for frame in interleaved[..whole].chunks_exact(ch) {
            for (lane, s) in self.lanes.iter_mut().zip(frame) {
                lane.pending
                    .push(if s.is_finite() { *s * SCALE } else { 0.0 });
            }
        }
        self.frames_in += (whole / ch) as u64;
        while self.lanes.iter().all(|l| l.pending.len() >= FRAME) {
            self.step(out);
        }
    }

    /// One model call per channel on the next frame (padded with silence when short). From the
    /// second call on, the answer is the frame before, mixed with that frame as it came in.
    fn step(&mut self, out: &mut Vec<f32>) {
        let ch = self.lanes.len();
        let emit = self.steps > 0;
        let start = out.len();
        if emit {
            out.resize(start + FRAME * ch, 0.0);
            self.frames_out += FRAME as u64;
        }
        let (w, d) = (self.wet, 1.0 - self.wet);
        for (c, lane) in self.lanes.iter_mut().enumerate() {
            let mut cur = [0.0f32; FRAME];
            let take = lane.pending.len().min(FRAME);
            cur[..take].copy_from_slice(&lane.pending[..take]);
            lane.pending.drain(..take);
            let mut cleaned = [0.0f32; FRAME];
            lane.state.process_frame(&mut cleaned, &cur);
            if emit {
                for i in 0..FRAME {
                    out[start + i * ch + c] = (w * cleaned[i] + d * lane.prev[i]) / SCALE;
                }
            }
            lane.prev = cur;
        }
        self.steps += 1;
    }

    /// End the stream: the partial frame is padded with silence, one more frame brings the last
    /// answer, and the output is cut to exactly the input length.
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        if self.ended {
            return;
        }
        self.ended = true;
        let ch = self.lanes.len();
        let start = out.len();
        let before = self.frames_out;
        if self.lanes.iter().any(|l| !l.pending.is_empty()) {
            self.step(out);
        }
        self.step(out);
        let allowed = self.frames_in.saturating_sub(before) as usize;
        out.truncate(start + allowed * ch);
        self.frames_out = self.frames_in;
    }
}

/// How loud the quiet parts of a stream are: a histogram of the level of 100 ms blocks, in whole
/// dB, and the level the quietest tenth of the blocks sits at. That is the noise floor.
#[derive(Clone)]
struct Floor {
    bins: Vec<u64>,
    sum: f64,
    n: usize,
    block: usize,
}

impl Floor {
    fn new(channels: usize) -> Self {
        Self {
            bins: vec![0; 121],
            sum: 0.0,
            n: 0,
            block: MODEL_RATE as usize / 10 * channels.max(1),
        }
    }

    fn push(&mut self, samples: &[f32]) {
        for s in samples {
            self.sum += f64::from(*s) * f64::from(*s);
            self.n += 1;
            if self.n == self.block {
                let db = 10.0 * (self.sum / self.n as f64).max(1e-12).log10();
                let bin = (db.round().clamp(-120.0, 0.0) + 120.0) as usize;
                self.bins[bin] += 1;
                self.sum = 0.0;
                self.n = 0;
            }
        }
    }

    /// The level (dB, full scale RMS) under which the quietest tenth of the blocks fall.
    fn level(&self) -> Option<f64> {
        let total: u64 = self.bins.iter().sum();
        if total == 0 {
            return None;
        }
        let want = (total as f64 * 0.1).ceil() as u64;
        let mut seen = 0;
        for (i, c) in self.bins.iter().enumerate() {
            seen += c;
            if seen >= want {
                return Some(i as f64 - 120.0);
            }
        }
        None
    }
}

/// Run `media.denoise`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let strength_name = opts.text("strength", "normal");
    let strength = Strength::from_name(strength_name).ok_or_else(|| {
        MediaError::BadOption(format!(
            "Strength is light, normal or strong, and {strength_name} is none of them"
        ))
    })?;
    let track = opts.text("track", "first").to_string();
    let mut job = Job::new(io)?;
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        one(&mut job, input, strength, &track).map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

/// What cleaning one track found, for the report.
struct Cleaned {
    raw: PathBuf,
    channels: u32,
    before: Option<f64>,
    after: Option<f64>,
}

fn one(job: &mut Job, input: &Path, strength: Strength, track: &str) -> Result<()> {
    let input = job.input(input)?;
    let p = job.probe(&input)?;
    let tracks: Vec<Stream> = audio::pick_tracks(&p, track)?
        .into_iter()
        .cloned()
        .collect();
    let (lo, hi) = (job.lo, job.hi);
    let span = hi - lo;
    // 1. Clean every chosen track into a raw file. This is most of the time.
    let mut cleaned: Vec<(u32, Cleaned)> = Vec::new();
    for (k, s) in tracks.iter().enumerate() {
        let (a, b) = (
            lo + span * 0.85 * k as f32 / tracks.len() as f32,
            lo + span * 0.85 * (k + 1) as f32 / tracks.len() as f32,
        );
        let (dir, raw) = job.part(&input, "raw")?;
        let _ = dir;
        let c = clean_track(job, &input, s, p.duration, strength, &raw, (a, b))?;
        cleaned.push((s.index, c));
    }
    // 2. Write the file again with those tracks taken from the raw files.
    job.lo = lo + span * 0.85;
    job.hi = hi;
    let wanted: Vec<(u32, Replacement, u32)> = cleaned
        .iter()
        .map(|(idx, c)| {
            let rate = tracks
                .iter()
                .find(|s| s.index == *idx)
                .map(|s| s.sample_rate)
                .filter(|r| *r >= 8000)
                .unwrap_or(MODEL_RATE);
            (
                *idx,
                Replacement {
                    path: c.raw.clone(),
                    rate: MODEL_RATE,
                    channels: c.channels,
                },
                rate,
            )
        })
        .collect();
    let rw = rewrite::rewrite_audio(job, &input, &p, &|s: &Stream| {
        wanted
            .iter()
            .find(|(i, ..)| *i == s.index)
            .map(|(_, r, rate)| Treatment {
                // Back to the rate the track had, when it was not 48 kHz.
                filter: (*rate != MODEL_RATE).then(|| format!("aresample={rate}")),
                channels: None,
                replace: Some(r.clone()),
            })
    });
    for (_, c) in &cleaned {
        job.stage.discard(&c.raw);
    }
    let rw = rw?;
    job.lo = lo;
    job.hi = hi;
    let out = job.commit(
        &rw.part,
        &rw.dir,
        &input,
        &Naming::Labelled("clean".into()),
        &rw.ext,
    )?;
    let (notes, plans) = (rw.notes, rw.plans);
    job.verify(&out, p.duration)?;
    let many = cleaned.len() > 1;
    for (k, (_, c)) in cleaned.iter().enumerate() {
        let label = if many {
            format!("Track {}: ", k + 1)
        } else {
            String::new()
        };
        let floor = match (c.before, c.after) {
            (Some(b), Some(a)) => format!(
                " Noise floor (the quietest tenth): {b:.0} dB before, {a:.0} dB after ({:+.0} dB).",
                a - b
            ),
            _ => String::new(),
        };
        job.note(format!(
            "{label}{}: background noise removed at {} strength ({}).{floor}",
            name_of(&out),
            strength.word(),
            rewrite::describe(&plans)
        ));
    }
    if p.has_video() {
        job.note("The picture was copied as it is.");
    }
    job.note(
        "The noise model is trained on speech: voices come through clearly, music and a second voice in the background can be dulled. Light keeps more of the original.",
    );
    for n in notes {
        job.note(n);
    }
    Ok(())
}

/// Decode one track to 48 kHz samples, clean it as it streams and write the raw samples.
fn clean_track(
    job: &Job,
    input: &Path,
    s: &Stream,
    duration: Option<f64>,
    strength: Strength,
    raw: &Path,
    lo_hi: (f32, f32),
) -> Result<Cleaned> {
    let channels = s.channels.max(1) as usize;
    let mut den = Denoiser::new(channels, strength.wet());
    let mut writer = BufWriter::with_capacity(
        256 * 1024,
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(raw)
            .map_err(|e| {
                MediaError::Io(format!(
                    "Could not write the temporary sound file: {}. Check the free space and try again.",
                    panoply_core::naming::plain_io(&e)
                ))
            })?,
    );
    let (mut before, mut after) = (Floor::new(channels), Floor::new(channels));
    let mut carry: Vec<u8> = Vec::new();
    let mut samples: Vec<f32> = Vec::new();
    let mut cleaned: Vec<f32> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut write_error: Option<std::io::Error> = None;
    let args: Vec<std::ffi::OsString> = [
        "-i".to_string(),
        tool_path(input),
        "-map".into(),
        format!("0:{}", s.index),
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
        "-ar".into(),
        MODEL_RATE.to_string(),
        "-f".into(),
        "f32le".into(),
        "-acodec".into(),
        "pcm_f32le".into(),
        "-".into(),
    ]
    .into_iter()
    .map(std::ffi::OsString::from)
    .collect();
    let seconds = s.duration.or(duration);
    let total = seconds.map(|d| (d * f64::from(MODEL_RATE) * channels as f64 * 4.0) as u64);
    let progress = Progress::part(seconds, lo_hi.0, lo_hi.1, "Removing the noise");
    let mut write = |den: &mut Denoiser, end: bool, cleaned: &mut Vec<f32>| {
        if end {
            den.finish(cleaned);
        }
        after.push(cleaned);
        bytes.clear();
        for v in cleaned.iter() {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        cleaned.clear();
        if write_error.is_none()
            && let Err(e) = writer.write_all(&bytes)
        {
            write_error = Some(e);
        }
    };
    let done = proc::run_ffmpeg_stream(
        &job.engine,
        job.io,
        &args,
        |chunk| {
            carry.extend_from_slice(chunk);
            let whole = carry.len() / 4 * 4;
            samples.clear();
            samples.extend(
                carry[..whole]
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            );
            carry.drain(..whole);
            before.push(&samples);
            den.push(&samples, &mut cleaned);
            write(&mut den, false, &mut cleaned);
        },
        &progress,
        total,
    )?;
    if !done.ok {
        return Err(proc::explain(&done.tail, &name_of(input)));
    }
    if den.frames_in() == 0 {
        return Err(MediaError::Corrupt(
            "The sound could not be decoded, or it is empty.".into(),
        ));
    }
    write(&mut den, true, &mut cleaned);
    let frames = den.frames_in();
    if let Some(e) = write_error {
        return Err(MediaError::Io(format!(
            "Could not write the temporary sound file: {}. Check the free space and try again.",
            panoply_core::naming::plain_io(&e)
        )));
    }
    writer.flush().map_err(|e| {
        MediaError::Io(format!(
            "Could not write the temporary sound file: {}. Check the free space and try again.",
            panoply_core::naming::plain_io(&e)
        ))
    })?;
    drop(writer);
    let want = frames * channels as u64 * 4;
    let got = std::fs::metadata(raw).map(|m| m.len()).unwrap_or(0);
    if got != want {
        return Err(MediaError::Failed(format!(
            "The cleaned sound is {got} bytes and should be {want}, so it was not kept. Try again."
        )));
    }
    Ok(Cleaned {
        raw: raw.to_path_buf(),
        channels: channels as u32,
        before: before.level(),
        after: after.level(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small deterministic noise generator: white, roughly Gaussian (sum of uniforms).
    struct Noise(u64);

    impl Noise {
        fn next(&mut self) -> f32 {
            let mut s = 0.0;
            for _ in 0..4 {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                s += ((self.0 >> 40) as f32 / (1u64 << 24) as f32) - 0.5;
            }
            s * 1.732 // unit variance: four uniforms of variance 1/12 each
        }
    }

    /// A voice-like signal: a gliding pitch around 140 Hz, harmonics shaped by two formants, and
    /// a 4 Hz syllable rhythm. Peak about 0.5.
    fn voice(n: usize) -> Vec<f32> {
        let rate = MODEL_RATE as f32;
        let mut phase = 0.0f32;
        (0..n)
            .map(|i| {
                let t = i as f32 / rate;
                let f0 = 140.0 + 25.0 * (2.0 * std::f32::consts::PI * 0.7 * t).sin();
                phase += 2.0 * std::f32::consts::PI * f0 / rate;
                let syllable = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 4.0 * t).sin().powi(2);
                let mut v = 0.0;
                for h in 1..=24 {
                    let f = f0 * h as f32;
                    let formant = |c: f32, w: f32| (-((f - c) / w).powi(2)).exp();
                    let gain =
                        0.15 / h as f32 + formant(700.0, 250.0) + 0.6 * formant(1300.0, 300.0);
                    v += gain * (phase * h as f32).sin();
                }
                v * syllable * 0.12
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn db(a: f64, b: f64) -> f64 {
        20.0 * (a / b).log10()
    }

    fn run_all(den: &mut Denoiser, x: &[f32], piece: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for c in x.chunks(piece) {
            den.push(c, &mut out);
        }
        den.finish(&mut out);
        out
    }

    /// Eight seconds: noise alone, then voice with the noise, then noise alone again.
    fn scene() -> (Vec<f32>, Vec<f32>) {
        let r = MODEL_RATE as usize;
        let n = 8 * r;
        let mut noise = Noise(7);
        let clean: Vec<f32> = voice(n)
            .into_iter()
            .enumerate()
            .map(|(i, v)| if (2 * r..6 * r).contains(&i) { v } else { 0.0 })
            .collect();
        let mixed = clean.iter().map(|v| v + 0.02 * noise.next()).collect();
        (clean, mixed)
    }

    #[test]
    fn noise_alone_drops_by_ten_db_and_the_voice_stays_within_three() {
        let r = MODEL_RATE as usize;
        let (clean, mixed) = scene();
        let mut den = Denoiser::new(1, Strength::Normal.wet());
        let out = run_all(&mut den, &mixed, 4096);
        assert_eq!(out.len(), mixed.len(), "the length is kept exactly");
        // The noise after the voice, once the model has settled. The lead-in before any voice has
        // come is reported and not asserted: from a cold start the model's answer to steady white
        // noise swings between 2 and 16 dB with the noise itself.
        let lead = db(
            rms(&mixed[r / 2..2 * r - r / 4]),
            rms(&out[r / 2..2 * r - r / 4]),
        );
        eprintln!("noise only, lead-in: down {lead:.1} dB (not asserted)");
        let tail = 6 * r + r / 2..8 * r;
        let drop = db(rms(&mixed[tail.clone()]), rms(&out[tail]));
        eprintln!("noise only, after the voice: down {drop:.1} dB");
        assert!(drop >= 10.0, "noise only: down {drop:.1} dB");
        let voice_range = 2 * r + r / 2..6 * r - r / 2;
        let diff = db(rms(&out[voice_range.clone()]), rms(&clean[voice_range]));
        eprintln!("voice segment: {diff:+.2} dB against the clean voice");
        assert!(diff.abs() <= 3.0, "voice level moved by {diff:+.2} dB");
    }

    #[test]
    fn strength_orders_the_noise_removal_and_strong_is_never_less_than_normal() {
        let r = MODEL_RATE as usize;
        let (_, mixed) = scene();
        let noise_level = |s: Strength| {
            let mut den = Denoiser::new(1, s.wet());
            let out = run_all(&mut den, &mixed, 1000);
            rms(&out[r / 2..2 * r - r / 4])
        };
        let (l, n, s) = (
            noise_level(Strength::Light),
            noise_level(Strength::Normal),
            noise_level(Strength::Strong),
        );
        eprintln!("noise rms: light {l:.5}, normal {n:.5}, strong {s:.5}");
        assert!(l > n && n >= s, "{l} {n} {s}");
        // Light still takes the noise down by a clear margin.
        let before = rms(&mixed[r / 2..2 * r - r / 4]);
        assert!(
            db(before, l) >= 3.0,
            "light removed only {:.1} dB",
            db(before, l)
        );
    }

    #[test]
    fn the_output_lines_up_with_the_input_and_pieces_do_not_matter() {
        let (_, mixed) = scene();
        let mixed = &mixed[..48_000 + 123];
        let a = run_all(&mut Denoiser::new(1, 0.0), mixed, 480);
        // All dry: the output is the input, delayed by nothing.
        assert_eq!(a.len(), mixed.len());
        let worst = a
            .iter()
            .zip(mixed)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "dry mix drifted by {worst}");
        // Any way of cutting the stream into pieces gives the same samples.
        let b = run_all(&mut Denoiser::new(1, 1.0), mixed, 7);
        let c = run_all(&mut Denoiser::new(1, 1.0), mixed, 100_000);
        assert_eq!(b, c);
        // A very short stream still comes out whole.
        let tiny = run_all(&mut Denoiser::new(1, 1.0), &mixed[..100], 100);
        assert_eq!(tiny.len(), 100);
    }

    #[test]
    fn channels_are_cleaned_apart_and_stay_in_their_place() {
        let r = MODEL_RATE as usize;
        let (clean, mixed) = scene();
        // Left carries the noisy voice, right is silence.
        let stereo: Vec<f32> = mixed[..4 * r].iter().flat_map(|v| [*v, 0.0]).collect();
        let out = run_all(&mut Denoiser::new(2, 1.0), &stereo, 3000);
        assert_eq!(out.len(), stereo.len());
        let right: Vec<f32> = out.iter().skip(1).step_by(2).copied().collect();
        assert!(rms(&right) < 1e-6, "silence on the right stays silent");
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let range = 2 * r + r / 2..4 * r;
        let diff = db(rms(&left[range.clone()]), rms(&clean[range]));
        assert!(diff.abs() <= 3.0, "{diff:+.2} dB");
    }

    #[test]
    fn the_noise_floor_meter_reads_a_known_level() {
        let mut f = Floor::new(1);
        let tone: Vec<f32> = (0..48_000)
            .map(|i| 0.1 * (i as f32 * 0.05).sin() * std::f32::consts::SQRT_2)
            .collect();
        f.push(&tone);
        // RMS 0.1 is -20 dB.
        assert_eq!(f.level(), Some(-20.0));
    }
}
