//! Looking at the sound of a file: loudness, peaks, clipping and the balance of the channels.
//! The audio is decoded by the helper to raw 32-bit samples and read as it arrives, so a
//! three hour file costs time and no memory.

use std::ffi::OsString;
use std::path::Path;

use crate::engine::Engine;
use crate::error::{MediaError, Result};
use crate::io::{Io, tool_path};
use crate::loudness::{Loudness, Meter, roles_for};
use crate::probe::Stream;
use crate::proc::{self, Progress};

/// What a pass over the channels found.
#[derive(Debug, Clone, Copy, Default)]
pub struct Balance {
    /// Mean square of each of the first two channels.
    pub energy: [f64; 2],
    /// Sum of left times right.
    pub cross: f64,
    /// Frames counted.
    pub frames: u64,
}

impl Balance {
    /// Correlation of left and right, -1 to 1.
    pub fn correlation(&self) -> f64 {
        let d = (self.energy[0] * self.energy[1]).sqrt();
        if d <= 0.0 {
            0.0
        } else {
            (self.cross / d).clamp(-1.0, 1.0)
        }
    }

    /// Level of each channel in dB relative to full scale (RMS).
    pub fn db(&self, ch: usize) -> f64 {
        let n = self.frames.max(1) as f64;
        let ms = self.energy[ch] / n;
        if ms <= 0.0 {
            f64::NEG_INFINITY
        } else {
            10.0 * ms.log10()
        }
    }
}

/// The result of one scan.
pub struct Scan {
    /// Loudness and peaks.
    pub loudness: Loudness,
    /// Left and right balance (for stereo streams).
    pub balance: Option<Balance>,
}

/// Decode one audio stream and measure it. `stream` is the stream to look at.
pub fn scan(
    engine: &Engine,
    io: &dyn Io,
    path: &Path,
    stream: &Stream,
    seconds: Option<f64>,
    lo_hi: (f32, f32),
    label: &str,
) -> Result<Scan> {
    let channels = stream.channels.max(1) as usize;
    let rate = if stream.sample_rate >= 8000 {
        stream.sample_rate
    } else {
        48_000
    };
    let roles = roles_for(&stream.channel_layout, channels);
    let mut meter = Meter::new(rate, &roles, false);
    let mut bal = Balance::default();
    let mut carry: Vec<u8> = Vec::new();
    let mut samples: Vec<f32> = Vec::new();
    let mut frame_acc: Vec<f32> = Vec::new();
    let args: Vec<OsString> = [
        "-i".to_string(),
        tool_path(path),
        "-map".to_string(),
        format!("0:{}", stream.index),
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
        "-f".into(),
        "f32le".into(),
        "-acodec".into(),
        "pcm_f32le".into(),
        "-".into(),
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let total = seconds.map(|s| (s * f64::from(rate) * channels as f64 * 4.0) as u64);
    let progress = Progress::part(seconds, lo_hi.0, lo_hi.1, label);
    let done = proc::run_ffmpeg_stream(
        engine,
        io,
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
            meter.push(&samples);
            if channels == 2 {
                frame_acc.extend_from_slice(&samples);
                let pairs = frame_acc.len() / 2 * 2;
                for p in frame_acc[..pairs].chunks_exact(2) {
                    let (l, r) = (f64::from(p[0]), f64::from(p[1]));
                    bal.energy[0] += l * l;
                    bal.energy[1] += r * r;
                    bal.cross += l * r;
                    bal.frames += 1;
                }
                frame_acc.drain(..pairs);
            }
        },
        &progress,
        total,
    )?;
    if !done.ok {
        return Err(proc::explain(&done.tail, &crate::units::name_of(path)));
    }
    if bal.frames == 0 && channels == 2 {
        return Err(MediaError::Corrupt(
            "The sound could not be decoded.".into(),
        ));
    }
    Ok(Scan {
        loudness: meter.finish(),
        balance: (channels == 2).then_some(bal),
    })
}
