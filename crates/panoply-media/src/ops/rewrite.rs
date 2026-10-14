//! Re-encoding the sound of a file in place, in the same container: the shared core of Stereo
//! to mono and Normalize. Video, subtitles, chapters, cover art and every sound track that was
//! not asked for are copied untouched (spec 4.10: "Mute and Replace leave video as COPY").

use std::path::{Path, PathBuf};

use crate::encoders::{ACodec, is_lossy_audio};
use crate::error::{MediaError, Result};
use crate::plan::Target;
use crate::probe::{Container, Kind, Profile, Stream};
use crate::units::ext_of;

use super::audio::{self, AudioFormat, AudioPlan, AudioWish};
use super::{Args, Job, per_stream};

/// What happens to one sound track.
pub struct Treatment {
    /// The ffmpeg audio filter chain, or `None` to copy the track.
    pub filter: Option<String>,
    /// Mix down to this many channels (1 for mono).
    pub channels: Option<u32>,
}

/// The container and muxer for an extension, and whether it is an audio-only file.
fn container_for(ext: &str) -> Option<(&'static str, &'static str, bool)> {
    Some(match ext {
        "mp3" => ("mp3", "mp3", true),
        "m4a" => ("m4a", "ipod", true),
        "aac" => ("aac", "adts", true),
        "wav" => ("wav", "wav", true),
        "flac" => ("flac", "flac", true),
        "ogg" => ("ogg", "ogg", true),
        "ogv" => ("ogv", "ogg", false),
        "opus" => ("opus", "opus", true),
        "mka" => ("mka", "matroska", true),
        "mp4" => ("mp4", "mp4", false),
        "m4v" => ("m4v", "mp4", false),
        "mov" => ("mov", "mov", false),
        "mkv" => ("mkv", "matroska", false),
        "webm" => ("webm", "webm", false),
        _ => return None,
    })
}

fn family(codec: &str) -> AudioFormat {
    match codec {
        "mp3" => AudioFormat::Mp3,
        "opus" => AudioFormat::Opus,
        "vorbis" => AudioFormat::Ogg,
        "flac" => AudioFormat::Flac,
        c if c.starts_with("pcm_") => AudioFormat::Wav,
        _ => AudioFormat::M4a,
    }
}

/// The stream with a bit rate to keep. Ogg and some other containers state none per stream; a
/// file that is only one sound track has it in its size and length, and writing it again at the
/// encoder's default rate (about 112 kb/s for Vorbis) made a 395 KB file 740 KB.
fn with_rate(s: &Stream, profile: &Profile) -> Stream {
    let mut s = s.clone();
    let only_sound = profile.of(Kind::Audio).count() == 1 && !profile.has_video();
    if s.bit_rate.is_none()
        && only_sound
        && let Some(d) = profile.duration.filter(|d| *d > 1.0 && d.is_finite())
    {
        s.bit_rate = Some((profile.size as f64 * 8.0 / d) as u64);
    }
    s
}

/// The plan for one track that is re-encoded in a container that may restrict the codec.
fn plan_for(job: &Job, s: &Stream, t: &Treatment, container_ext: &str) -> Result<AudioPlan> {
    let mut fmt = family(&s.codec);
    // WebM holds only Opus and Vorbis.
    if container_ext == "webm" && !matches!(fmt, AudioFormat::Opus | AudioFormat::Ogg) {
        fmt = AudioFormat::Opus;
    }
    // An engine without a Vorbis encoder writes Opus into the Ogg file instead.
    let mut extra = None;
    if fmt == AudioFormat::Ogg && !job.engine.has_encoder("libvorbis") {
        fmt = AudioFormat::Opus;
        extra = Some("This engine has no Vorbis encoder, so the sound was written as Opus, which Ogg files hold as well.".to_string());
    }
    let mut plan = audio::plan_audio(
        &job.engine,
        s,
        fmt,
        &AudioWish {
            force_encode: true,
            bitrate: None,
            channels: t.channels,
        },
    )?;
    plan.notes.extend(extra);
    Ok(plan)
}

/// A rewritten file that has not been given its final name yet.
pub struct Rewritten {
    /// The temporary file.
    pub part: PathBuf,
    /// The folder the result goes to.
    pub dir: PathBuf,
    /// The extension of the result.
    pub ext: String,
    /// Result card lines from planning.
    pub notes: Vec<String>,
    /// The plan of each re-encoded track, by source stream index.
    pub plans: Vec<(u32, AudioPlan)>,
}

/// Write `input` again with `treat` applied to its sound tracks.
pub fn rewrite_audio(
    job: &mut Job,
    input: &Path,
    profile: &Profile,
    treat: &dyn Fn(&Stream) -> Option<Treatment>,
) -> Result<Rewritten> {
    let in_ext = ext_of(input);
    let (ext, muxer, audio_only) = match container_for(&in_ext) {
        Some((e, m, a)) => (e.to_string(), m, a),
        None if profile.has_video() => {
            // A video container with no writer here (AVI, TS, MPEG, WMV, FLV): Matroska holds
            // whatever is inside, and the picture is copied untouched.
            ("mkv".into(), "matroska", false)
        }
        None => {
            // A format with no writer here: lossless stays lossless in FLAC, the rest becomes M4A.
            let lossless = profile
                .audio()
                .map(|a| !is_lossy_audio(&a.codec))
                .unwrap_or(false);
            if lossless {
                ("flac".into(), "flac", true)
            } else {
                ("m4a".into(), "ipod", true)
            }
        }
    };
    let _ = Target::Mp4;
    let _ = Container::Mp4;
    let (dir, part) = job.part(input, &ext)?;
    let mut a = Args::new();
    a.a("-y");
    a.input(input);
    a.kv("-map_metadata", "0");
    a.kv("-map_chapters", "0");
    let mut plans: Vec<(u32, AudioPlan)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let (mut ov, mut oa, mut os) = (0usize, 0usize, 0usize);
    let mut cover_index = None;
    for s in &profile.streams {
        match s.kind {
            Kind::Video if s.attached_pic => {
                if audio_only {
                    cover_index = Some(s.index);
                    continue;
                }
                a.kv("-map", format!("0:{}", s.index));
                a.kv(&format!("-c:v:{ov}"), "copy");
                a.kv(&format!("-disposition:v:{ov}"), "attached_pic");
                ov += 1;
            }
            Kind::Video => {
                a.kv("-map", format!("0:{}", s.index));
                a.kv(&format!("-c:v:{ov}"), "copy");
                if s.codec == "hevc" && matches!(muxer, "mp4" | "mov") {
                    a.kv(&format!("-tag:v:{ov}"), "hvc1");
                }
                ov += 1;
            }
            Kind::Audio => {
                a.kv("-map", format!("0:{}", s.index));
                match treat(s) {
                    Some(t) => {
                        let plan = plan_for(job, &with_rate(s, profile), &t, &ext)?;
                        a.all(per_stream(&plan.args, 'a', oa));
                        if let Some(f) = &t.filter {
                            a.kv(&format!("-filter:a:{oa}"), f);
                        }
                        notes.extend(plan.notes.iter().cloned());
                        plans.push((s.index, plan));
                    }
                    None => {
                        a.kv(&format!("-c:a:{oa}"), "copy");
                    }
                }
                oa += 1;
            }
            Kind::Subtitle => {
                if audio_only {
                    continue;
                }
                a.kv("-map", format!("0:{}", s.index));
                a.kv(&format!("-c:s:{os}"), "copy");
                os += 1;
            }
            Kind::Attachment if muxer == "matroska" => {
                a.kv("-map", format!("0:{}", s.index));
                a.kv("-c:t", "copy");
            }
            _ => {}
        }
    }
    if let Some(ci) = cover_index {
        let takes = matches!(muxer, "mp3" | "ipod" | "flac");
        if takes {
            a.kv("-map", format!("0:{ci}"));
            a.kv(&format!("-c:v:{ov}"), "copy");
            a.kv(&format!("-disposition:v:{ov}"), "attached_pic");
        }
    }
    match muxer {
        "mp3" => {
            a.kv("-id3v2_version", "3");
        }
        "mp4" | "mov" | "ipod" => {
            a.kv("-movflags", "+faststart");
        }
        "wav" => {
            a.kv("-rf64", "auto");
        }
        _ => {}
    }
    a.kv("-f", muxer);
    a.a(crate::io::tool_path(&part));
    if plans.is_empty() {
        return Err(MediaError::Unsupported(
            "This file has no sound to work with. Check that the file is the one you meant.".into(),
        ));
    }
    job.ffmpeg(
        a,
        profile.duration,
        "Processing the sound",
        (0.0, 1.0),
        input,
    )?;
    Ok(Rewritten {
        part,
        dir,
        ext,
        notes,
        plans,
    })
}

/// Which codec a plan ended up with, for the result card.
pub fn describe(plans: &[(u32, AudioPlan)]) -> String {
    plans
        .iter()
        .map(|(_, p)| p.describe.clone())
        .collect::<Vec<_>>()
        .join("; ")
}

/// True when the codec is lossy and so is re-encoded once by this pass.
pub fn reencodes_lossy(s: &Stream) -> bool {
    is_lossy_audio(&s.codec) && ACodec::from_name(&s.codec).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::parse_profile;

    #[test]
    fn a_sound_with_no_stated_rate_keeps_the_rate_of_its_file() {
        let profile = parse_profile(
            Path::new("a.ogg"),
            br#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"vorbis","channels":2,"sample_rate":"44100"}],"format":{"format_name":"ogg","duration":"100.0","size":"400000"}}"#,
            400_000,
        )
        .unwrap();
        let s = &profile.streams[0];
        assert_eq!(s.bit_rate, None);
        assert_eq!(with_rate(s, &profile).bit_rate, Some(32_000));
    }

    #[test]
    fn ogg_video_keeps_its_container_and_sound_only_ogg_stays_sound_only() {
        assert_eq!(container_for("ogv"), Some(("ogv", "ogg", false)));
        assert_eq!(container_for("ogg"), Some(("ogg", "ogg", true)));
    }
}
