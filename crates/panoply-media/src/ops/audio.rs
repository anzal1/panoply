//! Writing one audio stream into an audio file: the shared core of Convert (audio targets),
//! Extract audio, Stereo to mono and Normalize.
//!
//! Copy comes first. The stream is copied whenever the target format can carry its codec as it
//! is (AAC from an MP4 into an M4A, MP3 into MP3, FLAC into FLAC), which is lossless and
//! finishes at disk speed. Otherwise it is encoded once, with a bit rate that never goes under
//! what the source had when the source was already lossy (spec 5.1: no generation loss by
//! stealth, no upscaling a 128 kb/s file to 320).

use std::path::{Path, PathBuf};

use crate::encoders::{self, ACodec};
use crate::engine::Engine;
use crate::error::{MediaError, Result};
use crate::io::Naming;
use crate::probe::{Kind, Profile, Stream};

use super::{Args, Job};

/// An audio file format Panoply can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// MP3.
    Mp3,
    /// AAC in an MP4 audio file (`.m4a`).
    M4a,
    /// Raw AAC (ADTS, `.aac`).
    Aac,
    /// WAV.
    Wav,
    /// FLAC.
    Flac,
    /// Ogg Vorbis (`.ogg`).
    Ogg,
    /// Opus in Ogg (`.opus`).
    Opus,
    /// Dolby Digital, only ever copied out of a video (`.ac3`).
    Ac3,
    /// Dolby Digital Plus, only ever copied (`.eac3`).
    Eac3,
    /// DTS, only ever copied (`.dts`).
    Dts,
}

impl AudioFormat {
    /// Parse the option value.
    pub fn from_name(n: &str) -> Option<AudioFormat> {
        match n {
            "mp3" => Some(Self::Mp3),
            "m4a" => Some(Self::M4a),
            "aac" => Some(Self::Aac),
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "ogg" => Some(Self::Ogg),
            "opus" => Some(Self::Opus),
            _ => None,
        }
    }

    /// File extension.
    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Aac => "aac",
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Ac3 => "ac3",
            Self::Eac3 => "eac3",
            Self::Dts => "dts",
        }
    }

    /// ffmpeg's muxer name.
    pub fn muxer(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "ipod",
            Self::Aac => "adts",
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Ac3 => "ac3",
            Self::Eac3 => "eac3",
            Self::Dts => "dts",
        }
    }

    /// The codec used when the stream has to be encoded.
    pub fn codec(self) -> ACodec {
        match self {
            Self::Mp3 => ACodec::Mp3,
            Self::M4a | Self::Aac | Self::Ac3 | Self::Eac3 | Self::Dts => ACodec::Aac,
            Self::Wav => ACodec::Pcm,
            Self::Flac => ACodec::Flac,
            Self::Ogg => ACodec::Vorbis,
            Self::Opus => ACodec::Opus,
        }
    }

    /// True when this format holds a stream of this codec as it is.
    pub fn can_copy(self, codec: &str) -> bool {
        match self {
            Self::Mp3 => codec == "mp3",
            Self::M4a => matches!(codec, "aac" | "alac"),
            Self::Aac => codec == "aac",
            Self::Wav => codec.starts_with("pcm_"),
            Self::Flac => codec == "flac",
            Self::Ogg => codec == "vorbis",
            Self::Opus => codec == "opus",
            Self::Ac3 => codec == "ac3",
            Self::Eac3 => codec == "eac3",
            Self::Dts => codec == "dts",
        }
    }

    /// The highest channel count the format holds.
    pub fn max_channels(self) -> u32 {
        match self {
            Self::Mp3 => 2,
            _ => 8,
        }
    }

    /// True when a cover picture can be attached.
    pub fn takes_cover(self) -> bool {
        matches!(self, Self::Mp3 | Self::M4a | Self::Flac)
    }

    /// The extension a stream of this codec gets when copied out as it is (Extract audio).
    pub fn for_codec(codec: &str) -> Option<AudioFormat> {
        match codec {
            "aac" | "alac" => Some(Self::M4a),
            "mp3" => Some(Self::Mp3),
            "opus" => Some(Self::Opus),
            "vorbis" => Some(Self::Ogg),
            "flac" => Some(Self::Flac),
            "ac3" => Some(Self::Ac3),
            "eac3" => Some(Self::Eac3),
            "dts" => Some(Self::Dts),
            c if c.starts_with("pcm_") => Some(Self::Wav),
            _ => None,
        }
    }
}

/// Everything decided about one audio stream going into one file.
#[derive(Debug, Clone)]
pub struct AudioPlan {
    /// True when the stream is copied as it is.
    pub copy: bool,
    /// The output codec arguments (`-c:a ...`).
    pub args: Vec<String>,
    /// Channels in the output.
    pub channels: u32,
    /// Result card lines for this plan.
    pub notes: Vec<String>,
    /// "AAC, copied" or "MP3 at 192 kb/s".
    pub describe: String,
}

/// Options for planning one stream.
#[derive(Debug, Clone, Default)]
pub struct AudioWish {
    /// Encode even when the stream could be copied.
    pub force_encode: bool,
    /// A bit rate in bits per second, when the user gave one.
    pub bitrate: Option<u64>,
    /// Downmix to this many channels (1 for mono).
    pub channels: Option<u32>,
}

/// Decide how `s` goes into `fmt`.
pub fn plan_audio(
    engine: &Engine,
    s: &Stream,
    fmt: AudioFormat,
    wish: &AudioWish,
) -> Result<AudioPlan> {
    let mut notes = Vec::new();
    let src_ch = s.channels.max(1);
    let out_ch = wish.channels.unwrap_or(src_ch).min(fmt.max_channels());
    if out_ch < src_ch && wish.channels.is_none() {
        notes.push(format!(
            "{} holds {} channels at most, so the {}-channel sound was mixed down.",
            fmt.ext().to_uppercase(),
            fmt.max_channels(),
            src_ch
        ));
    }
    let can_copy =
        fmt.can_copy(&s.codec) && !wish.force_encode && out_ch == src_ch && wish.bitrate.is_none();
    if can_copy {
        let name = ACodec::from_name(&s.codec)
            .map(ACodec::plain)
            .unwrap_or(&s.codec);
        return Ok(AudioPlan {
            copy: true,
            args: vec!["-c:a".into(), "copy".into()],
            channels: src_ch,
            notes,
            describe: format!("{name}, copied without re-encoding"),
        });
    }
    let target = fmt.codec();
    let src_lossy = encoders::is_lossy_audio(&s.codec);
    if src_lossy && !target.is_lossy() {
        notes.push("The sound was already lossy, so converting it to a lossless format gains nothing and the file is larger.".into());
    } else if src_lossy && target.is_lossy() {
        notes.push("Both formats are lossy, so there is a small quality loss. The bit rate was kept at or above the original's.".into());
    }
    // Bit rate: never below what a lossy source had, never above the format's useful range.
    let bitrate = wish.bitrate.or_else(|| {
        let b = s.bit_rate?;
        if !src_lossy {
            return None;
        }
        Some(match target {
            ACodec::Mp3 => b.clamp(128_000, 320_000),
            ACodec::Aac => b.clamp(128_000, 320_000),
            ACodec::Opus => b.clamp(64_000, 160_000),
            ACodec::Vorbis => b.clamp(96_000, 256_000),
            _ => b,
        })
    });
    let mut args = encoders::audio_args(engine, target, out_ch, bitrate)?;
    match target {
        ACodec::Pcm => {
            let depth = s.bit_depth;
            let codec = if depth >= 24 && !src_lossy {
                "pcm_s24le"
            } else {
                "pcm_s16le"
            };
            args = vec!["-c:a".into(), codec.into()];
            if depth > 16 && !src_lossy && depth > 24 {
                args = vec!["-c:a".into(), "pcm_s32le".into()];
            }
        }
        ACodec::Flac => {
            args.push("-sample_fmt".into());
            args.push(
                if s.bit_depth > 16 && !src_lossy {
                    "s32"
                } else {
                    "s16"
                }
                .into(),
            );
        }
        ACodec::Opus => {
            args.push("-ar".into());
            args.push("48000".into());
        }
        ACodec::Aac if s.sample_rate > 48_000 => {
            // AAC is delivered at 48 kHz at most, and AudioToolbox refuses higher rates.
            args.push("-ar".into());
            args.push("48000".into());
            notes.push(format!(
                "The sound was {} kHz; AAC is written at 48 kHz.",
                s.sample_rate / 1000
            ));
        }
        _ => {}
    }
    if out_ch != src_ch {
        args.push("-ac".into());
        args.push(out_ch.to_string());
    }
    let describe = match bitrate {
        Some(b) => format!("{} at {} kb/s", target.plain(), b / 1000),
        None => target.plain().to_string(),
    };
    Ok(AudioPlan {
        copy: false,
        args,
        channels: out_ch,
        notes,
        describe,
    })
}

/// Write audio stream `stream` of `input` as `fmt`, naming the result by `naming`.
#[allow(clippy::too_many_arguments)]
pub fn write_audio(
    job: &mut Job,
    input: &Path,
    profile: &Profile,
    stream: &Stream,
    fmt: AudioFormat,
    plan: &AudioPlan,
    naming: &Naming,
    filter: Option<&str>,
) -> Result<PathBuf> {
    let (part, dir) = render_audio(job, input, profile, stream, fmt, plan, filter)?;
    let out = job.commit(&part, &dir, input, naming, fmt.ext())?;
    Ok(out)
}

/// Like [`write_audio`] but leaves the result under its temporary name, so the caller can look
/// at it (its size, for a size target) before it is given a final name or thrown away.
#[allow(clippy::too_many_arguments)]
pub fn render_audio(
    job: &mut Job,
    input: &Path,
    profile: &Profile,
    stream: &Stream,
    fmt: AudioFormat,
    plan: &AudioPlan,
    filter: Option<&str>,
) -> Result<(PathBuf, PathBuf)> {
    let (dir, part) = job.part(input, fmt.ext())?;
    let mut a = Args::new();
    a.a("-y");
    a.input(input);
    a.kv("-map", format!("0:{}", stream.index));
    // A cover picture, when the format can carry one.
    let cover = profile
        .streams
        .iter()
        .find(|s| s.kind == Kind::Video && s.attached_pic)
        .filter(|_| fmt.takes_cover());
    if let Some(c) = cover {
        a.kv("-map", format!("0:{}", c.index));
    }
    if let Some(f) = filter {
        a.kv("-af", f);
    }
    a.all(&plan.args);
    if cover.is_some() {
        a.kv("-c:v", "copy");
        a.kv("-disposition:v:0", "attached_pic");
    }
    a.kv("-map_metadata", "0");
    match fmt {
        AudioFormat::Mp3 => {
            a.kv("-id3v2_version", "3");
        }
        AudioFormat::M4a => {
            a.kv("-movflags", "+faststart");
        }
        AudioFormat::Wav => {
            a.kv("-rf64", "auto");
        }
        _ => {}
    }
    a.kv("-f", fmt.muxer());
    a.a(crate::io::tool_path(&part));
    let seconds = stream.duration.or(profile.duration);
    job.ffmpeg(a, seconds, "Writing the audio", (0.0, 1.0), input)?;
    Ok((part, dir))
}

/// The audio streams to work on: the one asked for (1 based position among audio streams), the
/// default or first one, or every one.
pub fn pick_tracks<'a>(profile: &'a Profile, track: &str) -> Result<Vec<&'a Stream>> {
    let audio: Vec<&Stream> = profile.of(Kind::Audio).collect();
    if audio.is_empty() {
        return Err(MediaError::Unsupported(
            "This file has no sound to work with. Check that the file is the one you meant.".into(),
        ));
    }
    match track {
        "all" => Ok(audio),
        "" | "default" | "first" => Ok(vec![profile.audio().unwrap_or(audio[0])]),
        n => {
            let i: usize = n.parse().map_err(|_| {
                MediaError::BadOption("track must be a number such as 2, or all".into())
            })?;
            audio
                .get(i.wrapping_sub(1))
                .copied()
                .map(|s| vec![s])
                .ok_or_else(|| {
                    MediaError::BadOption(format!(
                        "This file has {} audio track{}, so track {i} does not exist.",
                        audio.len(),
                        if audio.len() == 1 { "" } else { "s" }
                    ))
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_know_what_they_can_copy() {
        assert!(AudioFormat::M4a.can_copy("aac"));
        assert!(AudioFormat::Mp3.can_copy("mp3"));
        assert!(!AudioFormat::Mp3.can_copy("aac"));
        assert!(AudioFormat::Wav.can_copy("pcm_s24le"));
        assert_eq!(AudioFormat::for_codec("opus"), Some(AudioFormat::Opus));
        assert_eq!(AudioFormat::for_codec("ac3"), Some(AudioFormat::Ac3));
        assert_eq!(AudioFormat::for_codec("truehd"), None);
    }

    #[test]
    fn mp3_holds_two_channels() {
        assert_eq!(AudioFormat::Mp3.max_channels(), 2);
        assert_eq!(AudioFormat::Flac.max_channels(), 8);
    }
}
