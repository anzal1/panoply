//! `media.convert`: turn a video or audio file into another container or format.
//!
//! Streams are routed one at a time (see [`crate::plan`]): a container change is a stream copy
//! whenever the codecs fit the target, and only a stream that does not fit is encoded, audio and
//! subtitles before video. Picture encodes use the OS hardware encoder first.

use std::path::{Path, PathBuf};

use crate::encoders::{self, Rate, VCodec, VEnc};
use crate::error::{MediaError, Result};
use crate::io::{Io, Naming};
use crate::opts::Opts;
use crate::plan::{ARoute, Plan, SubRoute, Target, VRoute, plan_convert};
use crate::probe::{Hdr, Kind, Profile};
use crate::units::{duration_text, ext_of, name_of, size_text};

use super::audio::{self, AudioFormat, AudioWish};
use super::{Args, Job, change_text, per_stream, with_name};

/// Run `media.convert`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let to = opts.text("to", "mp4").to_string();
    let mut job = Job::new(io)?;
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        convert_one(&mut job, input, &to, opts).map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

fn convert_one(job: &mut Job, input: &Path, to: &str, opts: &Opts) -> Result<()> {
    let input = job.input(input)?;
    let profile = job.probe(&input)?;
    if let Some(fmt) = AudioFormat::from_name(to) {
        return convert_to_audio(job, &input, &profile, fmt, opts);
    }
    let (target, force) = match to {
        "hevc" => (Target::Mp4, Some(VCodec::Hevc)),
        "av1" => (Target::Mp4, Some(VCodec::Av1)),
        other => (
            Target::from_name(other).ok_or_else(|| {
                MediaError::BadOption(format!(
                    "\"{other}\" is not a format Panoply can write here"
                ))
            })?,
            None,
        ),
    };
    if !profile.has_video() {
        return Err(MediaError::Unsupported(
            "This file has only sound, no picture, so it cannot become a video. Choose an audio format such as mp3 or m4a.".into(),
        ));
    }
    convert_video(job, &input, &profile, target, force)
}

fn convert_to_audio(
    job: &mut Job,
    input: &Path,
    profile: &Profile,
    fmt: AudioFormat,
    opts: &Opts,
) -> Result<()> {
    let tracks = audio::pick_tracks(profile, opts.text("track", "first"))?;
    let same = ext_of(input) == fmt.ext() && tracks.len() == 1 && fmt.can_copy(&tracks[0].codec);
    if same && !profile.has_video() {
        job.note(format!(
            "{} is already {}, so there was nothing to convert.",
            name_of(input),
            fmt.ext().to_uppercase()
        ));
        return Ok(());
    }
    for (k, s) in tracks.iter().enumerate() {
        let plan = audio::plan_audio(&job.engine, s, fmt, &AudioWish::default())?;
        let naming = if tracks.len() > 1 {
            Naming::Labelled(format!("track {}", k + 1))
        } else if ext_of(input) == fmt.ext() {
            Naming::Labelled("converted".into())
        } else {
            Naming::Plain
        };
        let before = profile.size;
        let out = audio::write_audio(job, input, profile, s, fmt, &plan, &naming, None)?;
        let after = std::fs::metadata(&out)?.len();
        job.verify(&out, s.duration.or(profile.duration))?;
        job.note(format!(
            "{}: {} ({}).",
            name_of(&out),
            plan.describe,
            change_text(before, after)
        ));
        for n in plan.notes {
            job.note(n);
        }
    }
    Ok(())
}

/// A bit rate for a picture encode that matches what the source spent, within sane bounds.
pub fn video_bitrate(
    codec: VCodec,
    hardware: bool,
    w: u32,
    h: u32,
    fps: f64,
    source_bps: Option<u64>,
) -> u64 {
    let pixels = f64::from(w.max(16)) * f64::from(h.max(16));
    let fps = if fps > 1.0 { fps.min(120.0) } else { 30.0 };
    // Bits per pixel per frame that look clean at each codec's efficiency. Hardware encoders
    // need a little more than the best software ones for the same look.
    let bpp = match codec {
        VCodec::H264 => 0.075,
        VCodec::Hevc => 0.05,
        VCodec::Vp9 => 0.05,
        VCodec::Av1 => 0.04,
    } * if hardware { 1.25 } else { 1.0 };
    let wanted = (bpp * pixels * fps) as u64;
    match source_bps {
        Some(s) if s > 0 => wanted.clamp(s / 2, s * 5 / 4).max(150_000),
        _ => wanted.max(150_000),
    }
}

fn convert_video(
    job: &mut Job,
    input: &Path,
    p: &Profile,
    target: Target,
    force: Option<VCodec>,
) -> Result<()> {
    let mut plan = plan_convert(p, target, force);
    if plan.video.is_empty() {
        return Err(MediaError::Unsupported(
            "This file has no picture to carry over.".into(),
        ));
    }
    let same_container = ext_of(input) == target.ext();
    // Already in the target container, but with the index at the end: move it to the front
    // without touching a sample.
    if same_container
        && force.is_none()
        && plan.is_copy_only()
        && matches!(target, Target::Mp4 | Target::Mov)
        && super::faststart::needs_faststart(input)
    {
        let (part, dir) = super::faststart::rewrite(job, input, target.ext())?;
        let out = job.commit(
            &part,
            &dir,
            input,
            &Naming::Labelled("faststart".into()),
            target.ext(),
        )?;
        job.verify(&out, p.duration)?;
        job.note(format!(
            "{}: the index was at the end of the file, so it could not start playing until it was all there. It is now at the front. No re-encode, every stream is bit-identical.",
            name_of(&out)
        ));
        return Ok(());
    }
    // Nothing to do: already in the target container with every stream kept as it is.
    if same_container
        && plan.is_copy_only()
        && plan.subs.iter().all(|(_, r)| *r == SubRoute::Copy)
        && force.is_none()
    {
        job.note(format!(
            "{} is already {}, so there was nothing to convert.",
            name_of(input),
            target.ext().to_uppercase()
        ));
        return Ok(());
    }
    // Pick the encoders for the streams that need one before touching the disk.
    let mut venc: Vec<Option<VEnc>> = Vec::new();
    for (idx, route) in plan.video.clone() {
        let s = p
            .streams
            .iter()
            .find(|s| s.index == idx)
            .expect("planned stream exists");
        let _ = s;
        venc.push(match route {
            VRoute::Copy => None,
            VRoute::Encode(c) => Some(
                encoders::pick(&job.engine, c, false)
                    .ok_or_else(|| encoders::none_for(c, false))?,
            ),
            VRoute::EncodeMpeg4 => None,
        });
    }
    if let Some(Some(e)) = venc.iter().find(|e| e.is_some()) {
        job.note(format!("Video encoded as {}.", e.label()));
    }
    let (dir, part) = job.part(input, target.ext())?;
    let mut a = Args::new();
    a.a("-y");
    // Keep rotation as a flag instead of baking it into the pixels (spec ROT).
    if plan.encodes_video() {
        a.a("-noautorotate");
    }
    for e in venc.iter().flatten() {
        a.all(&e.pre);
    }
    a.input(input);
    a.kv("-map_metadata", "0");
    a.kv(
        "-map_chapters",
        if target == Target::Avi { "-1" } else { "0" },
    );
    let mut out_v = 0usize;
    for (n, (idx, route)) in plan.video.iter().enumerate() {
        let s = p
            .streams
            .iter()
            .find(|s| s.index == *idx)
            .expect("planned stream exists");
        a.kv("-map", format!("0:{idx}"));
        match route {
            VRoute::Copy => {
                let mut c = vec!["-c:v".to_string(), "copy".to_string()];
                if s.codec == "hevc" && matches!(target, Target::Mp4 | Target::Mov) {
                    c.push("-tag:v".into());
                    c.push("hvc1".into());
                }
                a.all(if plan.video.len() > 1 {
                    per_stream(&c, 'v', out_v)
                } else {
                    c
                });
            }
            VRoute::Encode(codec) => {
                let enc = venc[n].as_ref().expect("encoder chosen");
                let ten =
                    s.bit_depth >= 10 && matches!(codec, VCodec::Hevc | VCodec::Av1 | VCodec::Vp9);
                let bps = video_bitrate(
                    *codec,
                    enc.accel.is_hardware(),
                    s.width,
                    s.height,
                    s.fps,
                    s.bit_rate.or(p.bit_rate),
                );
                let mut args = enc.args(Rate::Bitrate(bps), ten, s.fps);
                if matches!(target, Target::Webm) {
                    args.retain(|x| x != "hvc1");
                }
                a.all(if plan.video.len() > 1 {
                    per_stream(&args, 'v', out_v)
                } else {
                    args
                });
                if let Some(f) = &enc.post_filter {
                    a.kv(
                        &format!("-filter:v:{out_v}"),
                        format!("format={},{f}", enc.pix_fmt(ten)),
                    );
                }
            }
            VRoute::EncodeMpeg4 => {
                a.kv("-c:v", "mpeg4");
                a.kv("-q:v", "3");
            }
        }
        out_v += 1;
    }
    for (out_a, (idx, route)) in plan.audio.iter().enumerate() {
        let s = p
            .streams
            .iter()
            .find(|s| s.index == *idx)
            .expect("planned stream exists");
        a.kv("-map", format!("0:{idx}"));
        match route {
            ARoute::Copy => {
                a.kv(&format!("-c:a:{out_a}"), "copy");
            }
            ARoute::Encode(codec) => {
                let fmt = match codec {
                    encoders::ACodec::Opus => AudioFormat::Opus,
                    encoders::ACodec::Mp3 => AudioFormat::Mp3,
                    _ => AudioFormat::M4a,
                };
                let ap = audio::plan_audio(
                    &job.engine,
                    s,
                    fmt,
                    &AudioWish {
                        force_encode: true,
                        ..Default::default()
                    },
                )?;
                a.all(per_stream(&ap.args, 'a', out_a));
                plan.notes.extend(ap.notes);
                job.note(format!("Sound: {}.", ap.describe));
            }
        }
    }
    let mut out_s = 0usize;
    for (idx, route) in &plan.subs {
        match route {
            SubRoute::Copy => {
                a.kv("-map", format!("0:{idx}"));
                a.kv(&format!("-c:s:{out_s}"), "copy");
                out_s += 1;
            }
            SubRoute::Convert(codec) => {
                a.kv("-map", format!("0:{idx}"));
                a.kv(&format!("-c:s:{out_s}"), *codec);
                out_s += 1;
            }
            SubRoute::Drop(_) => {}
        }
    }
    for idx in &plan.covers {
        a.kv("-map", format!("0:{idx}"));
    }
    if !plan.covers.is_empty() {
        let k = out_v;
        a.kv(&format!("-c:v:{k}"), "copy");
        a.kv(&format!("-disposition:v:{k}"), "attached_pic");
    }
    if target == Target::Mkv {
        for s in p.streams.iter().filter(|s| s.kind == Kind::Attachment) {
            a.kv("-map", format!("0:{}", s.index));
            a.kv("-c:t", "copy");
        }
    }
    if target.faststart() {
        a.kv("-movflags", "+faststart");
    }
    a.kv("-f", target.format());
    a.a(crate::io::tool_path(&part));
    let label = if plan.encodes_video() {
        "Converting the video"
    } else {
        "Copying, no re-encode"
    };
    job.ffmpeg(a, p.duration, label, (0.0, 1.0), input)?;
    let naming = if same_container {
        Naming::Labelled("converted".into())
    } else {
        Naming::Plain
    };
    let out = job.commit(&part, &dir, input, &naming, target.ext())?;
    let after = std::fs::metadata(&out)?.len();
    let result = job.verify(&out, p.duration)?;
    describe_result(job, p, &plan, &result, &out, after);
    Ok(())
}

fn describe_result(
    job: &Job,
    p: &Profile,
    plan: &Plan,
    _out: &Profile,
    out_path: &Path,
    after: u64,
) {
    let route = |r: &str| r.to_string();
    let v = if plan.encodes_video() {
        route("video: re-encoded")
    } else {
        route("video: copied")
    };
    let a = if plan.audio.is_empty() {
        "no sound".to_string()
    } else if plan.audio.iter().all(|(_, r)| *r == ARoute::Copy) {
        "audio: copied".to_string()
    } else {
        "audio: re-encoded".to_string()
    };
    let subs = plan
        .subs
        .iter()
        .filter(|(_, r)| !matches!(r, SubRoute::Drop(_)))
        .count();
    let mut line = format!("{}: {v}, {a}", name_of(out_path));
    if subs > 0 {
        line.push_str(&format!(", subtitles: {subs}"));
    }
    line.push_str(&format!(" ({}).", change_text(p.size, after)));
    if plan.is_copy_only() {
        line.push_str(&format!(
            " No re-encode, lossless, took no longer than copying {}.",
            size_text(p.size)
        ));
    }
    job.note(line);
    if let Some(d) = p.duration {
        let _ = duration_text(d);
    }
    for n in &plan.notes {
        job.note(n.clone());
    }
    if let Some(v) = p.video()
        && v.hdr != Hdr::Sdr
        && !plan.encodes_video()
    {
        job.note("The HDR video was kept exactly as it was.");
    }
}
