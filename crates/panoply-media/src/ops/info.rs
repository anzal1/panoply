//! `media.info`: tell me everything about this file and what is wrong with it (spec 4.33).
//!
//! The report is the answer: the probe profile in plain words, the things that commonly make a
//! file trouble (variable frame rate, HDR that looks washed out elsewhere, an index at the end,
//! a cut-off file, rotation set only as a flag, hidden location data), and next to each one the
//! module that fixes it. It is read-only and fast: one probe, a few box headers and 200
//! packet times, never a decode unless `deep` asks for the loudness scan.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::error::{MediaError, Result};
use crate::io::Io;
use crate::mp4::{boxes, source::Source};
use crate::opts::Opts;
use crate::probe::{self, Container, Hdr, Kind, Profile, Spacing, Stream};
use crate::units::{bitrate_text, duration_text, name_of, size_text};

use super::scan;
use super::{Job, with_name};

/// Something worth fixing, with the module that fixes it.
#[derive(Debug, Clone)]
pub struct Problem {
    /// Plain words.
    pub text: String,
    /// The module id that fixes it, if one does.
    pub fix: Option<&'static str>,
}

/// The facts and problems of one file.
pub struct Report {
    /// One line per fact, grouped by heading.
    pub lines: Vec<String>,
    /// Problems found.
    pub problems: Vec<Problem>,
    /// The same information as data.
    pub json: Value,
}

/// Run `media.info`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let job = Job::new(io)?;
    let json = opts.flag("json", false);
    let deep = opts.flag("deep", false);
    let n = inputs.len();
    let mut all = Vec::new();
    for input in inputs {
        let r = report_for(&job, input, deep).map_err(|e| with_name(n > 1, input, e))?;
        if json {
            all.push(r.json);
        } else {
            if n > 1 {
                io.note(format!("== {}", name_of(input)));
            }
            for l in r.lines {
                io.note(l);
            }
            if r.problems.is_empty() {
                io.note("Nothing wrong was found.".to_string());
            } else {
                io.note("What to fix:".to_string());
                for p in r.problems {
                    match p.fix {
                        Some(m) => io.note(format!("  - {} Fix: {m}.", p.text)),
                        None => io.note(format!("  - {}", p.text)),
                    }
                }
            }
        }
    }
    if json {
        let doc = if all.len() == 1 {
            all.remove(0)
        } else {
            Value::Array(all)
        };
        io.note(serde_json::to_string_pretty(&doc).unwrap_or_default());
    }
    Ok(Vec::new())
}

fn fps_text(s: &Stream) -> String {
    let f = if s.fps > 0.0 { s.fps } else { s.r_fps };
    if (f - f.round()).abs() < 0.005 {
        format!("{:.0} fps", f)
    } else {
        format!("{f:.2} fps")
    }
}

fn channels_text(s: &Stream) -> String {
    match (s.channels, s.channel_layout.as_str()) {
        (1, _) => "mono".into(),
        (2, _) => "stereo".into(),
        (6, _) => "5.1".into(),
        (8, _) => "7.1".into(),
        (n, l) if !l.is_empty() => format!("{n} channels ({l})"),
        (n, _) => format!("{n} channels"),
    }
}

fn describe_stream(s: &Stream) -> String {
    let mut parts: Vec<String> = Vec::new();
    match s.kind {
        Kind::Video => {
            let (w, h) = (s.width, s.height);
            let upright = if s.rotation == 90 || s.rotation == 270 {
                format!(" (shown as {h} x {w})")
            } else {
                String::new()
            };
            parts.push(format!(
                "{} {}, {w} x {h}{upright}, {}",
                s.codec,
                s.profile,
                fps_text(s)
            ));
            if s.bit_depth > 8 {
                parts.push(format!("{}-bit", s.bit_depth));
            }
            if s.hdr.is_hdr() {
                parts.push(s.hdr.plain());
            }
            if let Some(b) = s.bit_rate {
                parts.push(bitrate_text(b as f64));
            }
            if s.rotation != 0 {
                parts.push(format!("rotated {} degrees by a flag", s.rotation));
            }
        }
        Kind::Audio => {
            parts.push(format!(
                "{}, {}, {} Hz",
                s.codec,
                channels_text(s),
                s.sample_rate
            ));
            if let Some(b) = s.bit_rate {
                parts.push(bitrate_text(b as f64));
            }
            if s.bit_depth > 0 && !crate::encoders::is_lossy_audio(&s.codec) {
                parts.push(format!("{}-bit", s.bit_depth));
            }
        }
        Kind::Subtitle => {
            parts.push(format!(
                "{} ({})",
                s.codec,
                if s.text_subtitle { "text" } else { "pictures" }
            ));
        }
        Kind::Attachment => parts.push(format!("attached file {}", s.title)),
        Kind::Data => parts.push(format!("data ({})", s.codec)),
    }
    let mut tags = Vec::new();
    if !s.language.is_empty() && s.language != "und" {
        tags.push(s.language.clone());
    }
    if !s.title.is_empty() {
        tags.push(format!("\"{}\"", s.title));
    }
    if s.default {
        tags.push("default".into());
    }
    if s.forced {
        tags.push("forced".into());
    }
    if s.commentary {
        tags.push("commentary".into());
    }
    if s.attached_pic {
        tags.push("cover art".into());
    }
    let tail = if tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", tags.join(", "))
    };
    format!("{}{}", parts.join(", "), tail)
}

fn report_for(job: &Job, input: &Path, deep: bool) -> Result<Report> {
    let input = job.input(input)?;
    let p = match job.probe(&input) {
        Ok(p) => p,
        // Too broken to probe: say what the box walker sees instead of only refusing.
        Err(e @ MediaError::Corrupt(_)) => return unreadable_report(&input, e),
        Err(e) => return Err(e),
    };
    let spacing = if p.has_video() {
        probe::video_spacing(&job.engine, job.io, &input)
            .ok()
            .flatten()
    } else {
        None
    };
    let layout = if matches!(p.container, Container::Mp4 | Container::Mov) {
        Source::open(&input).ok().and_then(|s| boxes::scan(&s).ok())
    } else {
        None
    };
    let mut lines = Vec::new();
    let mut problems = Vec::new();
    lines.push(format!(
        "{}: {}, {}, {}{}",
        name_of(&input),
        container_name(&p),
        p.duration
            .map(duration_text)
            .unwrap_or_else(|| "length unknown".into()),
        size_text(p.size),
        p.bit_rate
            .map(|b| format!(", {}", bitrate_text(b as f64)))
            .unwrap_or_default()
    ));
    for s in &p.streams {
        let kind = match s.kind {
            Kind::Video if s.attached_pic => "Cover",
            Kind::Video => "Video",
            Kind::Audio => "Audio",
            Kind::Subtitle => "Subtitles",
            Kind::Attachment => "Attachment",
            Kind::Data => "Data",
        };
        lines.push(format!(
            "  stream {} {kind}: {}",
            s.index,
            describe_stream(s)
        ));
    }
    if p.chapters > 0 {
        lines.push(format!("  {} chapters", p.chapters));
    }
    if let Some(l) = &layout {
        lines.push(format!("  index: {}", index_text(l)));
    }
    let hidden = hidden_data(&p);
    if !hidden.is_empty() {
        lines.push(format!("  hidden data: {}", hidden.join(", ")));
    }
    // Problems.
    for s in p.of(Kind::Video) {
        if let Some(sp) = spacing
            && (sp.is_variable() || (s.r_fps > 0.0 && s.fps > 0.0 && (s.r_fps - s.fps).abs() > 0.5))
        {
            problems.push(Problem {
                text: vfr_text(&sp, s),
                fix: Some("media.convert keeps it as is; editors that need a fixed rate want it converted to one"),
            });
        }
        match s.hdr {
            Hdr::Sdr => {}
            Hdr::Hlg => problems.push(Problem {
                text: "The video is HLG, so it looks washed out on screens that are not HDR.".into(),
                fix: Some("media.hdr_to_sdr"),
            }),
            Hdr::Hdr10 => problems.push(Problem {
                text: "The video is HDR10, so it looks washed out on screens that are not HDR.".into(),
                fix: Some("media.hdr_to_sdr"),
            }),
            Hdr::DolbyVision { profile, compat } => problems.push(Problem {
                text: if profile == 5 {
                    "The video is Dolby Vision profile 5, which shows purple and green in players that do not read Dolby Vision.".into()
                } else {
                    format!(
                        "The video is Dolby Vision profile {profile} ({}), so it looks washed out on non-HDR screens.",
                        if compat == 4 { "HLG base layer" } else { "HDR10 base layer" }
                    )
                },
                fix: Some("media.hdr_to_sdr"),
            }),
        }
        if s.rotation != 0 {
            problems.push(Problem {
                text: format!(
                    "The picture is turned {} degrees only by a flag, so a player or editor that ignores the flag shows it sideways.",
                    s.rotation
                ),
                fix: None,
            });
        }
        if matches!(s.field_order.as_str(), "tt" | "bb" | "tb" | "bt") {
            problems.push(Problem {
                text: "The video is interlaced, so moving edges show combing on a computer screen."
                    .into(),
                fix: None,
            });
        }
        if !s.sar.is_empty() && s.sar != "1:1" && s.sar != "0:1" && s.sar != "N/A" {
            problems.push(Problem {
                text: format!(
                    "The pixels are not square (aspect {}), so some players stretch the picture.",
                    s.sar
                ),
                fix: None,
            });
        }
        if s.width % 2 == 1 || s.height % 2 == 1 {
            problems.push(Problem {
                text: format!(
                    "The size {} x {} is odd, and many encoders need even sizes.",
                    s.width, s.height
                ),
                fix: None,
            });
        }
        if s.bit_depth >= 10 && !s.hdr.is_hdr() {
            problems.push(Problem {
                text: format!(
                    "The video is {}-bit, which many players cannot decode.",
                    s.bit_depth
                ),
                fix: Some("media.convert"),
            });
        }
    }
    if let Some(l) = &layout {
        if l.moov_first() == Some(false) {
            problems.push(Problem {
                text: "The index is at the end of the file, so it cannot start playing or uploading until it is all there.".into(),
                fix: Some("media.convert to the same container (moves the index to the front, no re-encode)"),
            });
        }
        if l.find(b"moov").is_none() {
            problems.push(Problem {
                text: "The file has no index, so no player can read it. It looks cut off.".into(),
                fix: Some("media.repair"),
            });
        } else if l.find(b"moov").is_some_and(|m| m.truncated)
            || l.find(b"mdat").is_some_and(|m| m.truncated)
        {
            problems.push(Problem {
                text: "The file ends in the middle of its data, so it looks cut off.".into(),
                fix: Some("media.repair"),
            });
        }
        if l.garbage_at.is_some() {
            problems.push(Problem {
                text: "There are extra bytes after the last part of the file.".into(),
                fix: Some("media.repair"),
            });
        }
    }
    let audio: Vec<&Stream> = p.of(Kind::Audio).collect();
    if audio.len() > 1 && !audio.iter().any(|a| a.default) {
        problems.push(Problem {
            text: format!(
                "The file has {} audio tracks and none is marked as the default.",
                audio.len()
            ),
            fix: Some("media.streams"),
        });
    }
    if !hidden.is_empty() && hidden.iter().any(|h| h.starts_with("location")) {
        problems.push(Problem {
            text: "The file carries the place it was recorded (GPS), which travels with it when shared.".into(),
            fix: None,
        });
    }
    if p.has_video() && !p.has_audio() {
        lines.push("  no sound track".to_string());
    }
    let mut loud_json = Value::Null;
    if deep && let Some(a) = p.audio() {
        let s = scan::scan(
            &job.engine,
            job.io,
            &input,
            a,
            p.duration,
            (0.0, 1.0),
            "Measuring the loudness",
        )?;
        let l = s.loudness;
        lines.push(format!(
            "  loudness: {:.1} LUFS, range {:.1} LU, true peak {:.1} dBTP{}",
            l.integrated_lufs,
            l.range_lu,
            l.true_peak_dbtp,
            if l.clipped_samples > 0 {
                format!(", {} clipped samples", l.clipped_samples)
            } else {
                String::new()
            }
        ));
        if l.clipped_samples > 0 {
            problems.push(Problem {
                text: format!(
                    "The sound is clipped ({} samples at full scale).",
                    l.clipped_samples
                ),
                fix: None,
            });
        }
        if l.integrated_lufs.is_finite() && (l.integrated_lufs < -30.0 || l.integrated_lufs > -8.0)
        {
            problems.push(Problem {
                text: format!(
                    "The sound is {:.0} LUFS, far from the usual -23 to -14.",
                    l.integrated_lufs
                ),
                fix: Some("media.normalize"),
            });
        }
        loud_json = json!({
            "integrated_lufs": finite(l.integrated_lufs),
            "range_lu": l.range_lu,
            "true_peak_dbtp": finite(l.true_peak_dbtp),
            "clipped_samples": l.clipped_samples,
        });
    }
    let json = json!({
        "file": name_of(&input),
        "container": container_name(&p),
        "duration_s": p.duration,
        "size_bytes": p.size,
        "bit_rate": p.bit_rate,
        "chapters": p.chapters,
        "streams": p.streams.iter().map(stream_json).collect::<Vec<_>>(),
        "index": layout.as_ref().map(index_text),
        "hidden_data": hidden,
        "loudness": loud_json,
        "problems": problems.iter().map(|x| json!({"text": x.text, "fix": x.fix})).collect::<Vec<_>>(),
    });
    Ok(Report {
        lines,
        problems,
        json,
    })
}

/// The report for a file ffprobe cannot read: what is in it, in plain words, and where to go.
fn unreadable_report(input: &Path, why: MediaError) -> Result<Report> {
    let src = Source::open(input)?;
    let l = boxes::scan(&src)?;
    if l.boxes.is_empty() {
        return Err(why);
    }
    let mut lines = vec![format!(
        "{}: cannot be read as video or audio, {}",
        name_of(input),
        size_text(src.len())
    )];
    lines.push(format!("  index: {}", index_text(&l)));
    let mdat = l.all(b"mdat").map(|b| b.size).max().unwrap_or(0);
    if mdat > 0 {
        lines.push(format!("  media data: {} is in the file", size_text(mdat)));
    }
    let mut problems = Vec::new();
    if l.find(b"moov").is_none() || l.find(b"moov").is_some_and(|m| m.truncated) {
        problems.push(Problem {
            text: "The index that lists where every picture and sound is stored is missing or cut off, so no player can read the file, but the data is there.".into(),
            fix: Some("media.repair (give it a working video from the same camera as the reference)"),
        });
    } else {
        problems.push(Problem {
            text: "The file is laid out like a video but its streams could not be read.".into(),
            fix: Some("media.repair"),
        });
    }
    let json = json!({
        "file": name_of(input),
        "readable": false,
        "size_bytes": src.len(),
        "index": index_text(&l),
        "problems": problems.iter().map(|x| json!({"text": x.text, "fix": x.fix})).collect::<Vec<_>>(),
    });
    Ok(Report {
        lines,
        problems,
        json,
    })
}

fn finite(x: f64) -> Value {
    if x.is_finite() { json!(x) } else { Value::Null }
}

fn stream_json(s: &Stream) -> Value {
    json!({
        "index": s.index,
        "kind": match s.kind { Kind::Video => "video", Kind::Audio => "audio", Kind::Subtitle => "subtitle", Kind::Data => "data", Kind::Attachment => "attachment" },
        "codec": s.codec,
        "profile": s.profile,
        "width": s.width,
        "height": s.height,
        "pix_fmt": s.pix_fmt,
        "bit_depth": s.bit_depth,
        "fps": s.fps,
        "hdr": s.hdr.plain(),
        "rotation": s.rotation,
        "channels": s.channels,
        "sample_rate": s.sample_rate,
        "bit_rate": s.bit_rate,
        "language": s.language,
        "title": s.title,
        "default": s.default,
    })
}

fn container_name(p: &Profile) -> String {
    match p.container {
        Container::Mp4 => "MP4".into(),
        Container::Mov => "QuickTime MOV".into(),
        Container::Mkv => "Matroska (MKV)".into(),
        Container::Webm => "WebM".into(),
        Container::Avi => "AVI".into(),
        Container::Ts => "MPEG transport stream".into(),
        Container::Ogg => "Ogg".into(),
        Container::Mp3 => "MP3".into(),
        Container::Wav => "WAV".into(),
        Container::Flac => "FLAC".into(),
        Container::Aac => "AAC".into(),
        Container::Other => p.format_name.split(',').next().unwrap_or("").to_string(),
    }
}

fn index_text(l: &boxes::Layout) -> String {
    let mut t = match (l.find(b"moov"), l.moov_first()) {
        (None, _) => "missing (the file looks cut off)".to_string(),
        (Some(m), _) if m.truncated => "cut off".to_string(),
        (Some(_), Some(true)) => "at the front, so it starts playing at once".into(),
        (Some(_), Some(false)) => {
            "at the end, so it cannot start playing until it is all there".into()
        }
        (Some(_), None) => "present".into(),
    };
    if l.is_fragmented() {
        t.push_str(", fragmented");
    }
    if !l.brands.is_empty() {
        t.push_str(&format!(", brand {}", l.brands[0]));
    }
    t
}

fn vfr_text(sp: &Spacing, s: &Stream) -> String {
    format!(
        "The frame rate varies (gaps from {:.0} to {:.0} ms between frames, about {}), as phone videos do. Editors and GIF makers that expect a fixed rate can lose sync.",
        sp.min * 1000.0,
        sp.max * 1000.0,
        fps_text(s)
    )
}

fn hidden_data(p: &Profile) -> Vec<String> {
    let mut v = Vec::new();
    for (k, val) in &p.tags {
        let kl = k.to_ascii_lowercase();
        if kl == "location"
            || kl.contains("location.iso6709")
            || kl == "com.apple.quicktime.location.iso6709"
        {
            v.push(format!("location {val}"));
        } else if kl.contains("quicktime.make") || kl == "make" {
            v.push(format!("camera make {val}"));
        } else if kl.contains("quicktime.model") || kl == "model" {
            v.push(format!("camera model {val}"));
        } else if kl == "encoder" || kl == "software" || kl.contains("quicktime.software") {
            v.push(format!("software {val}"));
        } else if kl == "creation_time" {
            v.push(format!("recorded {val}"));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_data_names_the_location_and_the_camera() {
        let p = Profile {
            path: "a.mov".into(),
            size: 1,
            format_name: "mov".into(),
            container: Container::Mov,
            duration: Some(1.0),
            bit_rate: None,
            streams: vec![],
            chapters: 0,
            tags: vec![
                (
                    "com.apple.quicktime.location.ISO6709".into(),
                    "+12.34-056.78/".into(),
                ),
                ("com.apple.quicktime.model".into(), "iPhone 15".into()),
                ("creation_time".into(), "2026-01-01T00:00:00Z".into()),
            ],
        };
        let h = hidden_data(&p);
        assert!(h.iter().any(|x| x.starts_with("location")), "{h:?}");
        assert!(h.iter().any(|x| x.contains("iPhone 15")));
        assert!(h.iter().any(|x| x.starts_with("recorded")));
    }

    #[test]
    fn rates_read_like_a_person_would_say_them() {
        let mut s = crate::probe::parse_profile(
            std::path::Path::new("a.mp4"),
            br#"{"streams":[{"index":0,"codec_type":"video","avg_frame_rate":"30000/1001","r_frame_rate":"30000/1001"}],"format":{"format_name":"mp4"}}"#,
            1,
        )
        .unwrap()
        .streams
        .remove(0);
        assert_eq!(fps_text(&s), "29.97 fps");
        s.fps = 60.0;
        assert_eq!(fps_text(&s), "60 fps");
    }
}
