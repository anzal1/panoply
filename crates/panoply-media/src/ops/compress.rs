//! `media.compress`: make a video small enough to send or upload (spec 4.2, 5.3 for audio).
//!
//! Under a size: the picture gets `target * 0.98 * 8 / duration - audio` bits per second
//! ([`super::sizing`]); resolution and frame rate drop only as far as the codec's measured floor
//! requires and the card says so; hardware encoders cannot do true two-pass, so the file is
//! written, measured, and written again with the bit rate scaled by `target / actual * 0.97`
//! when it came out too big (the verify and retry loop). Smallest that looks good aims at the
//! codec's measured knee. Quality is measured on the real output with VMAF (libvmaf, BSD).

use std::path::{Path, PathBuf};

use crate::encoders::{self, Rate, VCodec, VEnc};
use crate::error::{MediaError, Result};
use crate::hdr::{Curve, Transfer};
use crate::io::{Io, Naming};
use crate::opts::Opts;
use crate::plan::{ARoute, SubRoute, Target, plan_convert};
use crate::presets;
use crate::probe::{Hdr, Kind, Profile, Stream};
use crate::units::{bitrate_text, duration_text, name_of, size_text};

use super::audio::{self, AudioFormat, AudioWish};
use super::hdr_to_sdr;
use super::quality;
use super::sizing::{self, AudioInfo, Floor, Request, VideoInfo, nice_size};
use super::{Args, Job, per_stream, with_name};

/// What the user asked for.
struct Wish {
    under: Option<u64>,
    preset: Option<presets::Resolved>,
    quality: Option<u8>,
    codec: String,
    max_height: Option<u32>,
    hdr: String,
    measure: bool,
}

/// Run `media.compress`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let preset_name = opts.text("preset", "none");
    let preset = if preset_name == "none" {
        None
    } else {
        Some(presets::resolve_size(preset_name).ok_or_else(|| {
            MediaError::BadOption(format!(
                "The upload limit for {preset_name} is not known (see its preset file)"
            ))
        })?)
    };
    let wish = Wish {
        under: opts.bytes("under")?,
        preset,
        quality: opts
            .f64("quality")
            .map(|q| q.round().clamp(1.0, 100.0) as u8),
        codec: opts.text("codec", "auto").to_string(),
        max_height: opts
            .f64("max_height")
            .filter(|h| *h > 0.0)
            .map(|h| h as u32),
        hdr: opts.text("hdr", "auto").to_string(),
        measure: opts.flag("measure_quality", true),
    };
    let mut job = Job::new(io)?;
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        one(&mut job, input, &wish).map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

fn target_bytes(w: &Wish) -> Option<(u64, String)> {
    match (&w.preset, w.under) {
        (Some(p), Some(u)) if u < p.bytes => {
            Some((u, format!("the {} you asked for", size_text(u))))
        }
        (Some(p), _) => Some((
            p.bytes,
            format!("the {} limit for {}", size_text(p.bytes), p.label),
        )),
        (None, Some(u)) => Some((u, format!("the {} you asked for", size_text(u)))),
        (None, None) => None,
    }
}

fn one(job: &mut Job, input: &Path, w: &Wish) -> Result<()> {
    let input = job.input(input)?;
    let p = job.probe(&input)?;
    if !p.has_video() {
        return compress_audio(job, &input, &p, w);
    }
    let duration = p.need_duration()?;
    let v = p.video().expect("has video").clone();
    let target = target_bytes(w);
    let preset_codec = w.preset.as_ref().map(|r| r.preset.video_codec.as_str());
    // HDR: tone-map when the output cannot hold it or the destination is an app.
    let transfer = hdr_to_sdr::source_transfer(&v)?;
    // 1. The codec and its encoder.
    let (codec, enc) = choose_encoder(
        job,
        &w.codec,
        preset_codec,
        transfer.is_some() && w.hdr != "sdr",
    )?;
    let to_sdr = transfer.is_some()
        && (w.hdr == "sdr" || w.preset.is_some() || !matches!(codec, VCodec::Hevc | VCodec::Av1));
    let ten = v.bit_depth >= 10 && !to_sdr && matches!(codec, VCodec::Hevc | VCodec::Av1);
    let hw = enc.accel.is_hardware();
    let fl = presets::floors(codec.name(), hw)
        .ok_or_else(|| MediaError::Failed("No quality floor is known for this encoder.".into()))?;
    let floor = Floor {
        floor: fl.floor_bpp,
        knee: fl.knee_bpp,
    };
    // 2. Already fine?
    let src_video_bps = v.bit_rate.unwrap_or_else(|| {
        let a: u64 = p.of(Kind::Audio).filter_map(|s| s.bit_rate).sum();
        ((p.size * 8) as f64 / duration) as u64 - a.min(((p.size * 8) as f64 / duration) as u64)
    });
    let audio_info: Vec<AudioInfo> = p
        .of(Kind::Audio)
        .map(|s| AudioInfo {
            index: s.index,
            codec: s.codec.clone(),
            channels: s.channels,
            bps: s.bit_rate.unwrap_or(128_000),
            commentary: s.commentary,
        })
        .collect();
    let info = VideoInfo {
        w: v.width,
        h: v.height,
        fps: if v.fps > 0.0 {
            v.fps
        } else {
            v.r_fps.max(24.0)
        },
        duration,
        audio: audio_info,
    };
    let compatible = match preset_codec {
        Some("h264") => {
            v.codec == "h264"
                && matches!(
                    p.container,
                    crate::probe::Container::Mp4 | crate::probe::Container::Mov
                )
                && !to_sdr
        }
        _ => true,
    };
    if let Some((bytes, label)) = &target
        && p.size <= *bytes
        && compatible
        && w.max_height.is_none_or(|h| v.width.min(v.height) <= h)
    {
        job.note(format!(
            "{} is {}, already under {label}, so it was left as it is.",
            name_of(&input),
            size_text(p.size)
        ));
        return Ok(());
    }
    // 3. The plan.
    let max_short = {
        let a = w
            .preset
            .as_ref()
            .map(|r| r.preset.max_height)
            .filter(|h| *h > 0);
        match (a, w.max_height) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (x, y) => x.or(y),
        }
    };
    let mut notes: Vec<String> = Vec::new();
    let (plan, mode_text) = match &target {
        Some((bytes, _)) => {
            let req = Request {
                target_bytes: *bytes,
                codec,
                floor,
                max_short_side: max_short,
                max_fps: 0.0,
            };
            (
                sizing::plan_size(&info, &req)?,
                format!("fit {}", size_text(*bytes)),
            )
        }
        None => {
            let q = w.quality.unwrap_or(50);
            let (mut ow, mut oh) = (info.w, info.h);
            if let Some(cap) = max_short
                && ow.min(oh) > cap
            {
                let f = f64::from(cap) / f64::from(ow.min(oh));
                let even = |x: f64| ((x / 2.0).round() as u32 * 2).max(2);
                (ow, oh) = if ow <= oh {
                    (cap, even(f64::from(oh) * f))
                } else {
                    (even(f64::from(ow) * f), cap)
                };
            }
            let bps = sizing::quality_bps(ow, oh, info.fps, floor, q, Some(src_video_bps * 9 / 10));
            let audio = info.audio.iter().map(sizing::audio_decision).collect();
            if src_video_bps > 0
                && sizing::quality_bps(ow, oh, info.fps, floor, q, None) >= src_video_bps * 92 / 100
                && (ow, oh) == (info.w, info.h)
            {
                job.note(format!(
                    "{} is already small: its picture uses {} ({:.3} bits per pixel), and this computer's encoder would need about {} to look as good, so it was left as it is.",
                    name_of(&input),
                    bitrate_text(src_video_bps as f64),
                    src_video_bps as f64 / (f64::from(info.w) * f64::from(info.h) * info.fps),
                    bitrate_text(sizing::quality_bps(ow, oh, info.fps, floor, q, None) as f64)
                ));
                return Ok(());
            }
            (
                sizing::Plan {
                    w: ow,
                    h: oh,
                    fps: None,
                    video_bps: bps,
                    audio,
                    notes: Vec::new(),
                },
                if w.quality.is_some() {
                    format!("quality {q}")
                } else {
                    "the smallest that still looks good".to_string()
                },
            )
        }
    };
    notes.extend(plan.notes.iter().cloned());
    if to_sdr {
        notes.push("The video is HDR and the result is standard range (SDR), so it was tone-mapped and will look right on every screen.".into());
    }
    // 4. Write, measure, retry.
    let target_cap = target.as_ref().map(|t| t.0);
    let container = if codec == VCodec::Vp9 {
        Target::Webm
    } else {
        Target::Mp4
    };
    let (dir, _) = {
        let d = job.io.output_dir_for(&input)?;
        (d, ())
    };
    let mut plan = plan;
    let mut bps = plan.video_bps.max(80_000);
    let mut best: Option<(PathBuf, u64, u64)> = None; // (part, size, bps)
    let mut tried: Vec<(u64, u64)> = Vec::new();
    let mut prev_over: Option<u64> = None;
    // Tries at the planned size, one with a smaller picture if the encoder will not go lower, and
    // the rest to close in. Almost always the first or second is enough.
    let max_attempts = if target_cap.is_some() { 6 } else { 1 };
    let mut attempt = 0usize;
    while attempt < max_attempts {
        let span = 0.85 / max_attempts as f32;
        let setup = EncodeSetup {
            input: &input,
            p: &p,
            v: &v,
            enc: &enc,
            codec,
            ten,
            container,
            plan: &plan,
            sdr: if to_sdr { transfer } else { None },
            dir: &dir,
        };
        let part = encode(
            job,
            &setup,
            bps,
            (
                0.02 + span * attempt as f32,
                0.02 + span * (attempt + 1) as f32,
            ),
        )?;
        let size = std::fs::metadata(&part)?.len();
        tried.push((bps, size));
        match target_cap {
            Some(cap) if size > cap => {
                job.stage.discard(&part);
                // A hardware encoder has a rate below which it will not go. When asking for less
                // changes nothing, or on the last try, give it a smaller picture instead.
                let stalled = prev_over.is_some_and(|p| size as f64 > p as f64 * 0.97);
                let repeated = prev_over.is_some();
                prev_over = Some(size);
                if attempt + 1 == max_attempts {
                    break;
                }
                if stalled || attempt + 3 == max_attempts {
                    let short = plan.w.min(plan.h);
                    let Some(rung) = sizing::LADDER.iter().copied().find(|r| *r < short) else {
                        break;
                    };
                    let req = Request {
                        target_bytes: cap,
                        codec,
                        floor,
                        max_short_side: Some(rung),
                        max_fps: 0.0,
                    };
                    let Ok(next_plan) = sizing::plan_size(&info, &req) else {
                        break;
                    };
                    notes.push(format!(
                        "The encoder would not go below {} for this picture (it came out {} bytes over the limit), so it was scaled down to {} x {}.",
                        bitrate_text(bps as f64),
                        size - cap,
                        next_plan.w,
                        next_plan.h
                    ));
                    bps = next_plan.video_bps.max(80_000);
                    plan = next_plan;
                    prev_over = None;
                } else {
                    let mut next = sizing::retry_bps(bps, cap, size);
                    if repeated {
                        // Missed twice in a row: leave more room than the usual margin.
                        next = (next as f64 * 0.95) as u64;
                    }
                    notes.push(format!(
                        "Attempt {} came out at {}, over the limit, so the bit rate was lowered from {} to {}.",
                        attempt + 1,
                        size_text(size),
                        bitrate_text(bps as f64),
                        bitrate_text(next as f64)
                    ));
                    bps = next;
                }
            }
            Some(cap) => {
                let better = best.as_ref().is_none_or(|b| size > b.1);
                if better {
                    if let Some((old, ..)) = best.take() {
                        job.stage.discard(&old);
                    }
                    best = Some((part.clone(), size, bps));
                } else {
                    job.stage.discard(&part);
                }
                // Close enough, or the last try: done. Otherwise use the room that is left.
                if size as f64 >= cap as f64 * 0.85 || attempt + 1 == max_attempts {
                    break;
                }
                let next = sizing::retry_bps(bps, cap, size);
                notes.push(format!(
                    "Attempt {} used only {} of the {}, so the bit rate was raised from {} to {} to use the room.",
                    attempt + 1,
                    size_text(size),
                    size_text(cap),
                    bitrate_text(bps as f64),
                    bitrate_text(next as f64)
                ));
                bps = next;
            }
            None => {
                best = Some((part, size, bps));
            }
        }
        attempt += 1;
    }
    // The default goal is a picture that looks good, and the planner reaches it from a typical
    // bit rate for the size. Clips that are hard to encode (noise, fast motion, a thin source) come
    // out well below that, so the first result is measured and, when it is visibly degraded, encoded
    // again with more bit rate (up to the source's own), keeping whichever stays smaller than the
    // original.
    let mut quality_warning: Option<String> = None;
    if target_cap.is_none()
        && w.quality.is_none()
        && w.measure
        && let Some((first_part, first_size, first_bps)) = best.clone()
    {
        let ceiling = src_video_bps * 9 / 10;
        let mut cur = (first_part, first_size, first_bps);
        let mut score = measure(job, &input, &cur.0, duration, &v, &dir)
            .filter(|q| q.vmaf)
            .map(|q| q.score);
        let mut boosts = 0;
        while let Some(sc) = score
            && sc < QUALITY_FLOOR_VMAF
            && boosts < 2
            && ceiling > cur.2 * 11 / 10
        {
            let next_bps = (cur.2 * 18 / 10).min(ceiling);
            let setup = EncodeSetup {
                input: &input,
                p: &p,
                v: &v,
                enc: &enc,
                codec,
                ten,
                container,
                plan: &plan,
                sdr: if to_sdr { transfer } else { None },
                dir: &dir,
            };
            let part2 = encode(job, &setup, next_bps, (0.9, 0.97))?;
            let size2 = std::fs::metadata(&part2)?.len();
            if size2 as f64 > p.size as f64 * 0.97 {
                job.stage.discard(&part2);
                break;
            }
            let sc2 = measure(job, &input, &part2, duration, &v, &dir)
                .filter(|q| q.vmaf)
                .map(|q| q.score);
            boosts += 1;
            notes.push(format!(
                "The first result scored VMAF {sc:.0}, which looks visibly worse than the original, so it was encoded again at {} (VMAF {}).",
                bitrate_text(next_bps as f64),
                sc2.map(|v| format!("{v:.0}")).unwrap_or_else(|| "not measured".into())
            ));
            job.stage.discard(&cur.0);
            cur = (part2, size2, next_bps);
            score = sc2;
        }
        best = Some(cur);
        if let Some(sc) = score
            && sc < QUALITY_FLOOR_VMAF
        {
            quality_warning = Some(format!(
                "Quality is VMAF {sc:.0}, below the {QUALITY_FLOOR_VMAF:.0} Panoply recommends: this video is hard to shrink without losing detail. Keep the original if it matters, or compress it less (quality 70 or more)."
            ));
        }
    }
    if let Some(wm) = quality_warning {
        notes.insert(0, wm);
    }
    let Some((part, size, used_bps)) = best else {
        let closest = tried.iter().map(|t| t.1).min().unwrap_or(0);
        return Err(MediaError::Failed(format!(
            "Panoply could not get this video under {} (the closest it came was {} bytes against {} bytes). Raise the size to {}, or choose a lower resolution.",
            size_text(target_cap.unwrap_or(0)),
            closest,
            target_cap.unwrap_or(0),
            size_text(nice_size(closest))
        )));
    };
    // Never bigger than what was there.
    if target_cap.is_none() && size as f64 > p.size as f64 * 0.97 {
        job.stage.discard(&part);
        job.note(format!(
            "{} is already small: the best Panoply could do was {} against {}, so it was left as it is.",
            name_of(&input),
            size_text(size),
            size_text(p.size)
        ));
        return Ok(());
    }
    let out = job.commit(
        &part,
        &dir,
        &input,
        &Naming::Labelled("compressed".into()),
        container.ext(),
    )?;
    let result = job.verify(&out, Some(duration))?;
    // 5. The card.
    let rv = result.video().cloned().unwrap_or(v.clone());
    let saved = (1.0 - size as f64 / p.size as f64) * 100.0;
    let fit = match &target {
        Some((_, label)) => format!(
            ", {} {label}",
            if size <= target_cap.unwrap_or(u64::MAX) {
                "under"
            } else {
                "OVER"
            }
        ),
        None => String::new(),
    };
    let mut line = format!(
        "{}: {} to {} ({:.0}% smaller{fit}). Goal: {mode_text}. {}, {} x {} at {:.0} fps, picture {}.",
        name_of(&out),
        size_text(p.size),
        size_text(size),
        saved.max(0.0),
        enc.label(),
        rv.width,
        rv.height,
        if rv.fps > 0.0 { rv.fps } else { info.fps },
        bitrate_text(used_bps as f64)
    );
    if w.measure
        && let Some(q) = measure(job, &input, &out, duration, &v, &dir)
    {
        line.push_str(&format!(
            " Quality {} on a {:.0} second sample.",
            q.text(),
            q.seconds
        ));
    }
    job.note(line);
    for n in notes {
        job.note(n);
    }
    Ok(())
}

/// Below this VMAF the result looks visibly worse than the original.
const QUALITY_FLOOR_VMAF: f64 = 75.0;

fn measure(
    job: &mut Job,
    src: &Path,
    out: &Path,
    duration: f64,
    v: &Stream,
    dir: &Path,
) -> Option<quality::Quality> {
    let log = job.stage.scratch(dir, "vmaf.json");
    let r = quality::measure(
        &job.engine,
        job.io,
        src,
        out,
        duration,
        v.width,
        v.height,
        20.0,
        &log,
    )
    .ok()
    .flatten();
    job.stage.discard(&log);
    r
}

fn choose_encoder(
    job: &Job,
    choice: &str,
    preset_codec: Option<&str>,
    hdr_source: bool,
) -> Result<(VCodec, VEnc)> {
    let wanted: Vec<VCodec> = match (choice, preset_codec) {
        ("h264", _) => vec![VCodec::H264],
        ("hevc", _) => vec![VCodec::Hevc],
        ("av1", _) => vec![VCodec::Av1],
        ("vp9", _) => vec![VCodec::Vp9],
        (_, Some("h264")) => vec![VCodec::H264],
        _ if hdr_source => vec![VCodec::Hevc, VCodec::H264, VCodec::Av1],
        _ => vec![VCodec::Hevc, VCodec::H264, VCodec::Av1],
    };
    for c in &wanted {
        if let Some(e) = encoders::pick(&job.engine, *c, false) {
            return Ok((*c, e));
        }
    }
    Err(encoders::none_for(wanted[0], false))
}

struct EncodeSetup<'a> {
    input: &'a Path,
    p: &'a Profile,
    v: &'a Stream,
    enc: &'a VEnc,
    codec: VCodec,
    ten: bool,
    container: Target,
    plan: &'a sizing::Plan,
    sdr: Option<Transfer>,
    dir: &'a Path,
}

/// One full encode at this bit rate. Returns the temporary result.
fn encode(job: &mut Job, s: &EncodeSetup, bps: u64, frac: (f32, f32)) -> Result<PathBuf> {
    let ext = s.container.ext();
    let part = job.stage.part(s.dir, ext)?;
    let pass_log = (s.codec == VCodec::Vp9).then(|| job.stage.scratch(s.dir, "pass"));
    let fps_out = s
        .plan
        .fps
        .unwrap_or(if s.v.fps > 0.0 { s.v.fps } else { 30.0 });
    let tp = plan_convert(s.p, s.container, None);
    let mut cube_path = None;
    // The picture chain: scale, frame rate, then either the SDR table or a plain format.
    let mut parts: Vec<String> = Vec::new();
    if (s.plan.w, s.plan.h) != (s.v.width, s.v.height) {
        parts.push(format!(
            "scale={}:{}:flags=bicubic+accurate_rnd",
            s.plan.w, s.plan.h
        ));
    }
    if let Some(f) = s.plan.fps {
        parts.push(format!("fps={f}"));
    }
    let pix = s.enc.pix_fmt(s.ten);
    if let Some(t) = s.sdr {
        let (chain, cube, _) = hdr_to_sdr::sdr_chain(
            job,
            s.dir,
            s.v,
            t,
            Curve::Bt2390,
            None,
            true,
            pix,
            s.enc.post_filter.as_deref(),
        )?;
        cube_path = Some(cube);
        parts.push(chain);
    } else {
        if s.v.bit_depth > 8 && !s.ten {
            // 10-bit to 8-bit with dithering, never truncation (spec B10).
            parts.push("scale=flags=bicubic+accurate_rnd+error_diffusion".into());
        }
        parts.push(format!("format={pix}"));
        if let Some(f) = &s.enc.post_filter {
            parts.push(f.clone());
        }
    }
    let vf = parts.join(",");
    let build = |pass: Option<u8>, out: &Path| -> Result<Args> {
        let mut a = Args::new();
        a.a("-y").a("-noautorotate");
        a.all(&s.enc.pre);
        a.input(s.input);
        a.kv("-map_metadata", "0");
        a.kv("-map_chapters", "0");
        a.kv("-map", format!("0:{}", s.v.index));
        a.kv("-vf", &vf);
        a.all(s.enc.args(Rate::Bitrate(bps), s.ten, fps_out));
        if s.container == Target::Mp4 && s.v.rotation != 0 {
            // The rotation stays a flag: -noautorotate keeps it in the display matrix.
        }
        if s.codec != VCodec::Vp9 && s.sdr.is_none() && s.ten {
            // HDR is kept: the colour tags and mastering metadata travel with the frames.
        }
        match pass {
            Some(1) => {
                a.kv("-pass", "1");
                a.kv("-passlogfile", pass_log.as_ref().expect("vp9 has a log"));
                a.all(["-an", "-sn", "-f", "null"]);
                a.a(if cfg!(windows) { "NUL" } else { "/dev/null" });
                return Ok(a);
            }
            Some(2) => {
                a.kv("-pass", "2");
                a.kv("-passlogfile", pass_log.as_ref().expect("vp9 has a log"));
            }
            _ => {}
        }
        // Sound.
        for (oa, d) in s.plan.audio.iter().enumerate() {
            let st =
                s.p.streams
                    .iter()
                    .find(|x| x.index == d.index)
                    .expect("planned stream exists");
            a.kv("-map", format!("0:{}", d.index));
            let routed = tp
                .audio
                .iter()
                .find(|(i, _)| *i == d.index)
                .map(|(_, r)| *r);
            if d.copy && routed == Some(ARoute::Copy) {
                a.kv(&format!("-c:a:{oa}"), "copy");
            } else {
                let fmt = if s.container == Target::Webm {
                    AudioFormat::Opus
                } else {
                    AudioFormat::M4a
                };
                let ap = audio::plan_audio(
                    &job.engine,
                    st,
                    fmt,
                    &AudioWish {
                        force_encode: true,
                        bitrate: Some(d.bps.max(32_000)),
                        channels: None,
                    },
                )?;
                a.all(per_stream(&ap.args, 'a', oa));
            }
        }
        let mut os = 0usize;
        for (idx, route) in &tp.subs {
            match route {
                SubRoute::Copy => {
                    a.kv("-map", format!("0:{idx}"));
                    a.kv(&format!("-c:s:{os}"), "copy");
                    os += 1;
                }
                SubRoute::Convert(c) => {
                    a.kv("-map", format!("0:{idx}"));
                    a.kv(&format!("-c:s:{os}"), *c);
                    os += 1;
                }
                SubRoute::Drop(_) => {}
            }
        }
        if s.container.faststart() {
            a.kv("-movflags", "+faststart");
        }
        a.kv("-f", s.container.format());
        a.a(crate::io::tool_path(out));
        Ok(a)
    };
    let dur = s.p.duration;
    if s.codec == VCodec::Vp9 {
        let a1 = build(Some(1), &part)?;
        job.ffmpeg(
            a1,
            dur,
            "Compressing, first pass",
            (frac.0, frac.0 + (frac.1 - frac.0) * 0.4),
            s.input,
        )?;
        let a2 = build(Some(2), &part)?;
        job.ffmpeg(
            a2,
            dur,
            "Compressing, second pass",
            (frac.0 + (frac.1 - frac.0) * 0.4, frac.1),
            s.input,
        )?;
        if let Some(l) = &pass_log {
            // libvpx writes <log>-0.log
            let mut name = l.clone().into_os_string();
            name.push("-0.log");
            let _ = std::fs::remove_file(PathBuf::from(name));
            job.stage.discard(l);
        }
    } else {
        let a = build(None, &part)?;
        job.ffmpeg(a, dur, "Compressing", frac, s.input)?;
    }
    if let Some(c) = cube_path {
        job.stage.discard(&c);
    }
    Ok(part)
}

// ---------------------------------------------------------------------------------------------
// Audio only.

fn compress_audio(job: &mut Job, input: &Path, p: &Profile, w: &Wish) -> Result<()> {
    let duration = p.need_duration()?;
    let s = p
        .audio()
        .ok_or_else(|| {
            MediaError::Unsupported("This file has neither picture nor sound to compress.".into())
        })?
        .clone();
    let target = target_bytes(w);
    let fmt = match w.preset.as_ref().map(|r| r.preset.audio_codec.as_str()) {
        Some("opus") => AudioFormat::Opus,
        _ => AudioFormat::M4a,
    };
    let bps = match &target {
        Some((bytes, _)) => {
            ((*bytes as f64 * sizing::OVERHEAD * 8.0 / duration) as u64).min(320_000)
        }
        None => {
            let q = w.quality.unwrap_or(50);
            let base = if s.channels <= 1 { 48_000.0 } else { 96_000.0 };
            (base * (0.5 + f64::from(q) / 100.0)) as u64
        }
    };
    // Speech at a low rate is mono (one channel carries it at half the bits).
    let mono = s.channels > 1 && bps < 64_000;
    let floor = if mono || s.channels <= 1 {
        24_000
    } else {
        48_000
    };
    if bps < floor {
        let need = (floor as f64 * duration / 8.0 / sizing::OVERHEAD) as u64;
        return Err(MediaError::Failed(format!(
            "{} at {} leaves {} for the sound, which would break up. Raise the size to {}.",
            duration_text(duration),
            target.as_ref().map(|t| size_text(t.0)).unwrap_or_default(),
            bitrate_text(bps as f64),
            size_text(nice_size(need))
        )));
    }
    if let Some((bytes, label)) = &target
        && p.size <= *bytes
    {
        job.note(format!(
            "{} is {}, already under {label}, so it was left as it is.",
            name_of(input),
            size_text(p.size)
        ));
        return Ok(());
    }
    // Write, look at the size, and write again at a corrected rate if it came out too big (the
    // same verify and retry loop as for video), keeping the best result that fits.
    let cap = target.as_ref().map(|t| t.0);
    let mut rate = bps;
    let mut best: Option<(PathBuf, PathBuf, u64, audio::AudioPlan)> = None;
    let attempts = if cap.is_some() { 3 } else { 1 };
    for attempt in 0..attempts {
        let wish = AudioWish {
            force_encode: true,
            bitrate: Some(rate),
            channels: mono.then_some(1),
        };
        let plan = audio::plan_audio(&job.engine, &s, fmt, &wish)?;
        let (part, dir) = audio::render_audio(
            job,
            input,
            p,
            &s,
            fmt,
            &plan,
            mono.then_some("pan=mono|c0=0.5*c0+0.5*c1"),
        )?;
        let size = std::fs::metadata(&part)?.len();
        match cap {
            Some(c) if size > c => {
                job.stage.discard(&part);
                rate = sizing::retry_bps(rate, c, size).max(floor / 2);
            }
            Some(c) => {
                let better = best.as_ref().is_none_or(|b| size > b.2);
                if better {
                    if let Some((old, ..)) = best.take() {
                        job.stage.discard(&old);
                    }
                    best = Some((part, dir, size, plan));
                } else {
                    job.stage.discard(&part);
                }
                if size as f64 >= c as f64 * 0.85 || attempt + 1 == attempts {
                    break;
                }
                rate = sizing::retry_bps(rate, c, size);
            }
            None => {
                best = Some((part, dir, size, plan));
            }
        }
    }
    let Some((part, dir, after, plan)) = best else {
        return Err(MediaError::Failed(format!(
            "Panoply could not get this sound under {}. Raise the size, or choose a lower rate.",
            size_text(cap.unwrap_or(0))
        )));
    };
    // A sound that is already coded at a low rate does not shrink at the default quality. Keep
    // the original rather than hand back a bigger "compressed" file.
    if target.is_none() && after >= p.size {
        job.stage.discard(&part);
        job.note(format!(
            "{} is already small for its length ({}), so Compress could not make it smaller and left it as it is.",
            name_of(input),
            size_text(p.size)
        ));
        return Ok(());
    }
    let out = job.commit(
        &part,
        &dir,
        input,
        &Naming::Labelled("compressed".into()),
        fmt.ext(),
    )?;
    job.verify(&out, Some(duration))?;
    job.note(format!(
        "{}: {} to {} ({:.0}% smaller{}). {}{}.",
        name_of(&out),
        size_text(p.size),
        size_text(after),
        ((1.0 - after as f64 / p.size as f64) * 100.0).max(0.0),
        target
            .as_ref()
            .map(|t| format!(", under {}", t.1))
            .unwrap_or_default(),
        plan.describe,
        if mono { ", mono" } else { "" }
    ));
    for n in plan.notes {
        job.note(n);
    }
    let _ = Hdr::Sdr;
    Ok(())
}
