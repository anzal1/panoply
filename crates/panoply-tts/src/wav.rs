//! A streaming 16 bit PCM WAV writer, and a reader for the WAV a Windows stream holds.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use crate::error::{Result, TtsError};

/// Appends samples to a WAV file and fixes the sizes in the header when finished.
pub struct WavWriter {
    out: BufWriter<File>,
    sample_rate: u32,
    channels: u16,
    data_bytes: u64,
}

impl WavWriter {
    /// Create `path` and write a header with sizes still to fill in.
    pub fn create(path: &Path, sample_rate: u32, channels: u16) -> Result<WavWriter> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&header(sample_rate, channels, 0))?;
        Ok(WavWriter {
            out,
            sample_rate,
            channels,
            data_bytes: 0,
        })
    }

    /// The sample rate the file was started with.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// The channel count the file was started with.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Append samples (interleaved when stereo).
    pub fn write(&mut self, samples: &[i16]) -> Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.out.write_all(&bytes)?;
        self.data_bytes += bytes.len() as u64;
        Ok(())
    }

    /// Seconds written so far.
    pub fn seconds(&self) -> f64 {
        self.data_bytes as f64 / (f64::from(self.sample_rate) * f64::from(self.channels) * 2.0)
    }

    /// Fix the header and flush. Returns the seconds of audio in the file.
    pub fn finish(mut self) -> Result<f64> {
        let seconds = self.seconds();
        let data = u32::try_from(self.data_bytes).map_err(|_| {
            TtsError::BadInput(
                "This text is longer than a WAV file can hold. Split it into parts.".into(),
            )
        })?;
        self.out.flush()?;
        let file = self.out.get_mut();
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header(self.sample_rate, self.channels, data))?;
        file.flush()?;
        Ok(seconds)
    }
}

fn header(sample_rate: u32, channels: u16, data: u32) -> [u8; 44] {
    let block = channels * 2;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&(sample_rate * u32::from(block)).to_le_bytes());
    h[32..34].copy_from_slice(&block.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// Samples, rate and channels of a 16 bit PCM WAV held in memory.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse(bytes: &[u8]) -> Result<(u32, u16, Vec<i16>)> {
    let bad =
        || TtsError::Engine("The system voice returned audio in a format this cannot read.".into());
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad());
    }
    let (mut rate, mut channels, mut bits) = (0u32, 0u16, 0u16);
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size =
            u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().map_err(|_| bad())?) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            let tag = u16::from_le_bytes([bytes[body], bytes[body + 1]]);
            channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]);
            rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().map_err(|_| bad())?);
            bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
            if tag != 1 && tag != 0xFFFE {
                return Err(bad());
            }
        } else if id == b"data" {
            if bits != 16 || !(1..=2).contains(&channels) || rate == 0 {
                return Err(bad());
            }
            let end = (body + size).min(bytes.len());
            let samples = bytes[body..end]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            return Ok((rate, channels, samples));
        }
        pos = body + size + (size & 1);
    }
    Err(bad())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_parses_back_with_the_right_sizes() {
        let dir = std::env::temp_dir().join(format!("panoply-tts-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wav");
        let mut w = WavWriter::create(&path, 22_050, 1).unwrap();
        w.write(&[0, 1000, -1000, 5]).unwrap();
        w.write(&[7, 8]).unwrap();
        let secs = w.finish().unwrap();
        assert!((secs - 6.0 / 22_050.0).abs() < 1e-9);
        let (rate, ch, samples) = parse(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!((rate, ch), (22_050, 1));
        assert_eq!(samples, [0, 1000, -1000, 5, 7, 8]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn junk_is_refused_in_plain_words() {
        assert!(parse(b"not a wav file at all").is_err());
    }
}
