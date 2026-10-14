//! `media.trim`: keep some parts of a video or audio file and cut the rest, without re-encoding
//! what does not have to be re-encoded (spec 4.1 and 5.2).
//!
//! MP4 and MOV files go through Panoply's own index code ([`crate::mp4`]): whole groups of
//! pictures and every audio frame are copied bit for bit, and only the few frames at a cut that
//! is not on a keyframe are re-encoded by the hardware encoder and spliced in (SMART). The
//! splice needs the result to carry more than one sample description (the re-encoded frames have
//! their own H.264 or HEVC parameter sets), which ffmpeg's muxer cannot write, so the index is
//! written by `mp4::write`. Everything else (MKV, WebM, TS, AVI, MP3, FLAC, WAV, Ogg) is a
//! stream copy by the helper engine, snapped to keyframes or audio frames, and the result card
//! says how far a cut moved.

use std::path::{Path, PathBuf};

use crate::encoders::{self, Rate, VCodec};
use crate::error::{MediaError, Result};
use crate::io::{Io, Naming};
use crate::mp4::boxes::{self, bx};
use crate::mp4::cut::{self, Piece, Rel, Timeline};
use crate::mp4::movie::{Edit, Movie, Sample, Track, parse_movie};
use crate::mp4::source::Source;
use crate::mp4::write::{self, OutSample, OutTrack, Output};
use crate::opts::Opts;
use crate::probe::{Container, Profile};
use crate::units::{duration_text, ext_of, invert_ranges, name_of, parse_ranges, size_text};

use super::audio::{self, AudioFormat, AudioWish};
use super::{Args, Job, with_name};

type Range = (f64, Option<f64>);

/// Run `media.trim`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let keep = opts.str("keep").map(str::to_string);
    let remove = opts.str("remove").map(str::to_string);
    if keep.is_none() && remove.is_none() {
        return Err(MediaError::BadOption(
            "Say what to keep, for example 0:12-1:40, or what to remove with the remove option"
                .into(),
        ));
    }
    if keep.is_some() && remove.is_some() {
        return Err(MediaError::BadOption(
            "Use either keep or remove, not both".into(),
        ));
    }
    let mut job = Job::new(io)?;
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        trim_one(&mut job, input, opts, keep.as_deref(), remove.as_deref())
            .map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

fn trim_one(
    job: &mut Job,
    input: &Path,
    opts: &Opts,
    keep: Option<&str>,
    remove: Option<&str>,
) -> Result<()> {
    let input = job.input(input)?;
    let profile = job.probe(&input)?;
    let duration = profile.need_duration()?;
    let ranges: Vec<Range> = match (keep, remove) {
        (Some(k), _) => parse_ranges(k)?,
        (_, Some(r)) => invert_ranges(&parse_ranges(r)?, duration),
        _ => unreachable!("checked in run"),
    };
    let ranges = clamp_ranges(ranges, duration)?;
    let audio_only = !profile.has_video();
    let exact = match opts.text("precision", "auto") {
        "exact" => true,
        "keyframes" => false,
        _ => !audio_only,
    };
    let each = opts.text("output", "one") == "each";
    let groups: Vec<Vec<Range>> = if each {
        ranges.iter().map(|r| vec![*r]).collect()
    } else {
        vec![ranges]
    };
    for (k, group) in groups.iter().enumerate() {
        let naming = if each {
            Naming::Labelled(format!("part {}", k + 1))
        } else {
            Naming::Labelled("trimmed".into())
        };
        trim_group(job, &input, &profile, group, exact, audio_only, &naming)?;
    }
    Ok(())
}

fn clamp_ranges(ranges: Vec<Range>, duration: f64) -> Result<Vec<Range>> {
    let mut out = Vec::new();
    for (s, e) in ranges {
        if s >= duration - 0.0005 {
            return Err(MediaError::BadOption(format!(
                "The range starts at {}, but the file is only {} long",
                duration_text(s),
                duration_text(duration)
            )));
        }
        let e = e.filter(|e| *e < duration - 0.0005);
        out.push((s, e));
    }
    Ok(out)
}

fn trim_group(
    job: &mut Job,
    input: &Path,
    profile: &Profile,
    ranges: &[Range],
    exact: bool,
    audio_only: bool,
    naming: &Naming,
) -> Result<()> {
    let own = matches!(profile.container, Container::Mp4 | Container::Mov);
    if own {
        match cut_mp4(job, input, profile, ranges, exact) {
            Ok(done) => {
                let out = job.commit(&done.part, &done.dir, input, naming, &ext_of(input))?;
                report(job, input, profile, &out, ranges, &done);
                return Ok(());
            }
            Err(CutErr::Hard(e)) => return Err(e),
            Err(CutErr::Fallback(why)) => {
                job.note(format!("Cut with the helper engine instead: {why}"));
            }
        }
    }
    let part_and_dir = copy_with_engine(job, input, profile, ranges, exact, audio_only)?;
    let out = job.commit(
        &part_and_dir.0,
        &part_and_dir.1,
        input,
        naming,
        &part_and_dir.2,
    )?;
    let after = std::fs::metadata(&out)?.len();
    let p = job.verify(&out, None)?;
    let want: f64 = ranges
        .iter()
        .map(|(s, e)| e.unwrap_or(profile.duration.unwrap_or(0.0)) - s)
        .sum();
    job.note(format!(
        "{}: kept {} ({}). Copied without re-encoding, so each cut starts on a keyframe or audio frame; the result is {} long where {} was asked.",
        name_of(&out),
        describe_ranges(ranges),
        size_text(after),
        duration_text(p.duration.unwrap_or(0.0)),
        duration_text(want)
    ));
    Ok(())
}

fn describe_ranges(r: &[Range]) -> String {
    r.iter()
        .map(|(s, e)| match e {
            Some(e) => format!("{} to {}", duration_text(*s), duration_text(*e)),
            None => format!("{} to the end", duration_text(*s)),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------------------------
// The helper engine path: stream copy, snapped to keyframes or audio frames.

fn copy_with_engine(
    job: &mut Job,
    input: &Path,
    profile: &Profile,
    ranges: &[Range],
    exact: bool,
    audio_only: bool,
) -> Result<(PathBuf, PathBuf, String)> {
    let ext = ext_of(input);
    let (dir, final_part) = job.part(input, &ext)?;
    let mut pieces: Vec<PathBuf> = Vec::new();
    let n = ranges.len();
    for (k, (s, e)) in ranges.iter().enumerate() {
        let part = if n == 1 {
            final_part.clone()
        } else {
            job.stage.part(&dir, &ext)?
        };
        let mut a = Args::new();
        a.a("-y");
        if *s > 0.0 {
            a.kv("-ss", format!("{s:.6}"));
        }
        a.input(input);
        if let Some(e) = e {
            a.kv("-t", format!("{:.6}", e - s));
        }
        a.kv("-map", "0");
        a.kv("-map_metadata", "0");
        // Lossless audio files can be cut to the sample when exactness is asked for.
        if exact && audio_only {
            let st = profile
                .audio()
                .ok_or_else(|| MediaError::Unsupported("There is no sound to cut.".into()))?;
            if let Some(fmt) = AudioFormat::for_codec(&st.codec).or(AudioFormat::from_name(&ext))
                && !crate::encoders::is_lossy_audio(&st.codec)
                && fmt != AudioFormat::Wav
            {
                let plan = audio::plan_audio(
                    &job.engine,
                    st,
                    fmt,
                    &AudioWish {
                        force_encode: true,
                        ..Default::default()
                    },
                )?;
                a.all(&plan.args);
            } else {
                a.kv("-c", "copy");
            }
        } else {
            a.kv("-c", "copy");
        }
        a.kv("-avoid_negative_ts", "make_zero");
        if matches!(ext.as_str(), "mp4" | "mov" | "m4a" | "m4v") {
            a.kv("-movflags", "+faststart");
        }
        a.a(crate::io::tool_path(&part));
        let dur = e.unwrap_or(profile.duration.unwrap_or(0.0)) - s;
        let (lo, hi) = (k as f32 / n as f32, (k + 1) as f32 / n as f32);
        job.ffmpeg(a, Some(dur), "Cutting, no re-encode", (lo, hi), input)?;
        pieces.push(part);
    }
    if n == 1 {
        return Ok((final_part, dir, ext));
    }
    // Join the pieces with the concat demuxer.
    let list = job.stage.part(&dir, "txt")?;
    let mut text = String::new();
    for p in &pieces {
        text.push_str(&format!(
            "file '{}'\n",
            p.to_string_lossy().replace('\'', "'\\''")
        ));
    }
    std::fs::write(&list, text)?;
    let mut a = Args::new();
    a.a("-y");
    a.kv("-f", "concat");
    a.kv("-safe", "0");
    a.input(&list);
    a.kv("-map", "0");
    a.kv("-c", "copy");
    a.kv("-map_metadata", "0");
    a.a(crate::io::tool_path(&final_part));
    job.ffmpeg(a, None, "Joining the pieces", (0.95, 1.0), input)?;
    for p in pieces {
        job.stage.discard(&p);
    }
    job.stage.discard(&list);
    Ok((final_part, dir, ext))
}

// ---------------------------------------------------------------------------------------------
// The MP4 path.

enum CutErr {
    /// Something Panoply's own cut cannot do: use the helper engine instead.
    Fallback(String),
    Hard(MediaError),
}

impl From<MediaError> for CutErr {
    fn from(e: MediaError) -> Self {
        match e {
            MediaError::Unsupported(m) => CutErr::Fallback(m),
            other => CutErr::Hard(other),
        }
    }
}

impl From<std::io::Error> for CutErr {
    fn from(e: std::io::Error) -> Self {
        CutErr::Hard(e.into())
    }
}

struct Done {
    part: PathBuf,
    dir: PathBuf,
    copied_frames: usize,
    encoded_frames: usize,
    encoded_with: Option<String>,
    out_seconds: f64,
    notes: Vec<String>,
}

struct Parsed {
    src: Source,
    ftyp: Vec<u8>,
    movie: Movie,
}

fn open_mp4(path: &Path) -> std::result::Result<Parsed, CutErr> {
    let src = Source::open(path)?;
    let layout = boxes::scan(&src)?;
    if layout.is_fragmented() {
        return Err(CutErr::Fallback("this file is fragmented".into()));
    }
    let moov = layout.find(b"moov").ok_or_else(|| {
        CutErr::Hard(MediaError::Corrupt(
            "This file looks cut off: its index is missing. Try Repair video.".into(),
        ))
    })?;
    if moov.truncated {
        return Err(CutErr::Hard(MediaError::Corrupt(
            "This file looks cut off: its index is incomplete. Try Repair video.".into(),
        )));
    }
    let body = src.read_exact_at(
        moov.payload(),
        (moov.size - u64::from(moov.header_len)) as usize,
    )?;
    let ftyp = match layout.find(b"ftyp") {
        Some(f) => src.read_exact_at(f.start, f.size as usize)?,
        None => bx(b"ftyp", b"isom\0\0\x02\0isomiso2mp41"),
    };
    let movie = parse_movie(&body)?;
    Ok(Parsed { src, ftyp, movie })
}

fn cut_mp4(
    job: &mut Job,
    input: &Path,
    profile: &Profile,
    ranges: &[Range],
    exact: bool,
) -> std::result::Result<Done, CutErr> {
    let parsed = open_mp4(input)?;
    let mut notes = Vec::new();
    let mut exact_now = exact;
    for attempt in 0..2 {
        match try_cut(job, input, profile, &parsed, ranges, exact_now, &mut notes) {
            Err(CutErr::Fallback(why))
                if attempt == 0 && exact_now && why.starts_with("@snap:") =>
            {
                notes.push(why.trim_start_matches("@snap:").trim().to_string());
                exact_now = false;
            }
            r => return r,
        }
    }
    Err(CutErr::Fallback("the cut could not be planned".into()))
}

fn video_track(movie: &Movie) -> Option<(usize, &Track)> {
    movie
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_video())
        .max_by_key(|(_, t)| t.samples.len())
}

fn keeps(t: &Track) -> bool {
    t.is_video() || t.is_audio() || matches!(&t.handler, b"text" | b"sbtl" | b"subt" | b"clcp")
}

fn encodable_codec(t: &Track) -> Option<VCodec> {
    match t.codec().as_str() {
        "avc1" | "avc3" => Some(VCodec::H264),
        "hvc1" | "hev1" => Some(VCodec::Hevc),
        _ => None,
    }
}

/// The length of the NAL length prefix from the `avcC` or `hvcC` box of a sample entry.
fn nal_length_size(t: &Track) -> Option<usize> {
    let e = t.stsd.first()?;
    if e.len() <= 86 {
        return None;
    }
    for (typ, s, end) in boxes::children(&e[86..]) {
        let body = &e[86 + s..86 + end];
        match &typ {
            b"avcC" if body.len() > 4 => return Some(usize::from(body[4] & 3) + 1),
            b"hvcC" if body.len() > 21 => return Some(usize::from(body[21] & 3) + 1),
            _ => {}
        }
    }
    None
}

/// True when the sample starts a coded video sequence that needs nothing before it: an IDR
/// picture in H.264, an IDR or BLA picture in HEVC. A plain "sync" sample may be an open-group
/// recovery point, whose new parameter sets cannot be joined to a different stream.
fn is_clean_start(src: &Source, s: &Sample, hevc: bool, len_size: usize) -> bool {
    let mut pos = 0u64;
    for _ in 0..16 {
        if pos + len_size as u64 + 2 > u64::from(s.size) {
            return false;
        }
        let Ok(h) = src.read_exact_at(s.offset + pos, len_size + 2) else {
            return false;
        };
        let len = h[..len_size]
            .iter()
            .fold(0u64, |a, b| (a << 8) | u64::from(*b));
        let nal = h[len_size];
        if hevc {
            let t = (nal >> 1) & 0x3F;
            if t < 32 {
                return (16..=20).contains(&t);
            }
        } else {
            let t = nal & 0x1F;
            if (1..=5).contains(&t) {
                return t == 5;
            }
        }
        pos += len_size as u64 + len;
    }
    false
}

/// A copy of the track in which only clean starts count as sync samples.
fn clean_starts_only(src: &Source, t: &Track) -> Track {
    let hevc = matches!(t.codec().as_str(), "hvc1" | "hev1");
    let Some(len_size) = nal_length_size(t) else {
        return t.clone();
    };
    let mut c = t.clone();
    for s in &mut c.samples {
        if s.sync {
            s.sync = is_clean_start(src, s, hevc, len_size);
        }
    }
    c
}

fn is_spatial(t: &Track) -> bool {
    // A video sample entry is 8 bytes of box header and 78 bytes of fixed fields, then child
    // boxes. Spatial video (MV-HEVC) carries `vexu` and `hero` there.
    t.stsd.iter().any(|e| {
        e.len() > 86
            && boxes::children(&e[86..])
                .iter()
                .any(|(typ, _, _)| matches!(typ, b"vexu" | b"hero" | b"eyes" | b"stri"))
    })
}

#[allow(clippy::too_many_arguments)]
fn try_cut(
    job: &mut Job,
    input: &Path,
    profile: &Profile,
    parsed: &Parsed,
    ranges: &[Range],
    exact: bool,
    notes: &mut Vec<String>,
) -> std::result::Result<Done, CutErr> {
    let movie = &parsed.movie;
    let ts_movie = movie.timescale;
    let vt = video_track(movie);
    // One cut plan per range for the picture track.
    let mut plans: Vec<cut::VideoCut> = Vec::new();
    let mut tl_v: Option<Timeline> = None;
    let mut encoded_with = None;
    if let Some((_, t)) = vt {
        let tl = cut::timeline_of(t, ts_movie)?;
        tl_v = Some(tl);
        let intra = t.samples.iter().all(|s| s.sync);
        let mut can_encode = encodable_codec(t).is_some() && !is_spatial(t);
        if exact && !can_encode && !intra {
            notes.push(format!(
                "Smart cutting works on H.264 and HEVC; this video is {}, so each cut starts at the nearest earlier keyframe.",
                t.codec().trim()
            ));
        }
        if is_spatial(t) && exact {
            notes.push("Spatial video cannot be re-encoded without turning flat, so each cut starts at the nearest earlier keyframe.".into());
        }
        let mut venc = None;
        if exact
            && can_encode
            && let Some(c) = encodable_codec(t)
        {
            venc = encoders::pick(&job.engine, c, true);
            if venc.is_none() {
                can_encode = false;
                notes.push(format!(
                    "This computer has no hardware {} encoder, so each cut starts at the nearest earlier keyframe.",
                    c.plain()
                ));
            }
        }
        let want_exact = exact && (can_encode || intra);
        // Joining re-encoded frames to copied ones is only safe at a clean start.
        let clean = if want_exact && !intra {
            Some(clean_starts_only(&parsed.src, t))
        } else {
            None
        };
        let planning_track = clean.as_ref().unwrap_or(t);
        for (s, e) in ranges {
            let start = tl.ticks(*s);
            let end = e.map(|e| tl.ticks(e));
            let mut plan = cut::plan_video(planning_track, &tl, start, end, want_exact)?;
            // A long range with nothing safe to copy would be re-encoded whole: snap instead.
            let whole = plan.pieces.len() == 1 && matches!(plan.pieces[0], Piece::Encode { .. });
            if want_exact && whole && tl.secs(plan.end - plan.start) > 30.0 {
                notes.push(
                    "This video has no keyframe that is safe to join at inside that range (open groups of pictures), so the cut starts at the nearest earlier keyframe instead of re-encoding all of it."
                        .into(),
                );
                plan = cut::plan_video(t, &tl, start, end, false)?;
            }
            plans.push(plan);
        }
        if let Some(e) = &venc
            && plans.iter().any(|p| p.frames_encoded > 0)
        {
            encoded_with = Some(e.label());
        }
    }
    // Encode the edges.
    struct EncPart {
        range: usize,
        piece: usize,
        movie: Movie,
        src: Source,
        index: usize,
    }
    let mut enc_parts: Vec<EncPart> = Vec::new();
    let mut part_paths: Vec<PathBuf> = Vec::new();
    if let (Some((vi, t)), Some(tl)) = (vt, tl_v) {
        let total_pieces: usize = plans
            .iter()
            .map(|p| {
                p.pieces
                    .iter()
                    .filter(|x| matches!(x, Piece::Encode { .. }))
                    .count()
            })
            .sum();
        let mut done_pieces = 0usize;
        let codec = encodable_codec(t);
        let order = cut::presentation_order(t, &tl);
        for (ri, plan) in plans.iter().enumerate() {
            for (pi, piece) in plan.pieces.iter().enumerate() {
                let Piece::Encode { start, end } = piece else {
                    continue;
                };
                let codec = codec.ok_or_else(|| {
                    CutErr::Fallback("@snap: this video cannot be re-encoded here".into())
                })?;
                let enc = encoders::pick(&job.engine, codec, true)
                    .ok_or_else(|| CutErr::Fallback("@snap: no hardware encoder".into()))?;
                let expected: usize = order
                    .iter()
                    .filter(|&&i| {
                        let p = tl.pres(&t.samples[i]);
                        p >= *start && p < *end
                    })
                    .count();
                let (dir, part) = job.part(input, "mp4")?;
                let _ = dir;
                part_paths.push(part.clone());
                let first = order
                    .iter()
                    .find(|&&i| tl.pres(&t.samples[i]) >= *start)
                    .map(|&i| t.samples[i].dur)
                    .unwrap_or(1);
                let guard = 0.25 * tl.secs(i64::from(first));
                let ss = (tl.secs(*start) - guard).max(0.0);
                let dur = tl.secs(*end) - tl.secs(*start);
                let secs = t.seconds().max(0.001);
                let fps = t.samples.len() as f64 / secs;
                let src_bps = ((t.bytes() * 8) as f64 / secs) as u64;
                let stream = profile.video();
                let ten =
                    stream.map(|s| s.bit_depth >= 10).unwrap_or(false) && codec == VCodec::Hevc;
                let mut a = Args::new();
                a.a("-y").a("-noautorotate");
                a.all(&enc.pre);
                a.kv("-ss", format!("{ss:.6}"));
                a.input(input);
                a.kv("-t", format!("{dur:.6}"));
                let vcount = movie.tracks[..vi].iter().filter(|x| x.is_video()).count();
                a.kv("-map", format!("0:v:{vcount}"));
                a.all(["-an", "-sn", "-dn"]);
                a.kv("-map_metadata", "-1");
                a.kv("-map_chapters", "-1");
                a.all(enc.args(Rate::Bitrate(src_bps.max(500_000)), ten, fps));
                if let Some(f) = &enc.post_filter {
                    a.kv("-vf", format!("format={},{f}", enc.pix_fmt(ten)));
                }
                a.kv("-fps_mode", "passthrough");
                a.kv("-video_track_timescale", tl.ts.to_string());
                a.kv("-f", "mp4");
                a.a(crate::io::tool_path(&part));
                let lo = 0.05 + 0.8 * done_pieces as f32 / total_pieces.max(1) as f32;
                let hi = 0.05 + 0.8 * (done_pieces + 1) as f32 / total_pieces.max(1) as f32;
                job.ffmpeg(a, Some(dur), "Re-encoding the cut edges", (lo, hi), input)?;
                done_pieces += 1;
                let p = open_mp4_loose(&part)?;
                let n_frames = p
                    .movie
                    .tracks
                    .iter()
                    .find(|x| x.is_video())
                    .map(|x| x.samples.len())
                    .unwrap_or(0);
                if n_frames != expected {
                    return Err(CutErr::Fallback(format!(
                        "@snap: the re-encoded edge came out with {n_frames} frames where {expected} were expected, so the cut snaps to keyframes instead."
                    )));
                }
                enc_parts.push(EncPart {
                    range: ri,
                    piece: pi,
                    movie: p.movie,
                    src: p.src,
                    index: enc_parts.len() + 1,
                });
            }
        }
    }
    // Assemble the picture track.
    let mut out_tracks: Vec<OutTrack> = Vec::new();
    let mut sources: Vec<&Source> = vec![&parsed.src];
    for p in &enc_parts {
        sources.push(&p.src);
    }
    let mut span_seconds_per_range: Vec<(f64, f64)> = Vec::new();
    let mut piece_boundaries: Vec<f64> = Vec::new();
    let mut copied_frames = 0usize;
    let mut encoded_frames = 0usize;
    for track in &movie.tracks {
        if !keeps(track) {
            notes.push("A timecode or data track was not carried into the cut.".into());
        }
    }
    let mut v_out: Option<(usize, OutTrack)> = None;
    if let (Some((vi, t)), Some(tl)) = (vt, tl_v) {
        let mut stsd = t.stsd.clone();
        let mut rels: Vec<Vec<Rel>> = Vec::new();
        let mut osamples: Vec<Vec<OutSample>> = Vec::new();
        let mut bounds_ticks = 0i64;
        for (ri, plan) in plans.iter().enumerate() {
            copied_frames += plan.frames_copied;
            encoded_frames += plan.frames_encoded;
            for (pi, piece) in plan.pieces.iter().enumerate() {
                match piece {
                    Piece::Copy { first, end, skip } => {
                        let idx: Vec<usize> =
                            (*first..*end).filter(|i| !skip.contains(i)).collect();
                        let sm: Vec<&Sample> = idx.iter().map(|&i| &t.samples[i]).collect();
                        let min_cts = sm.iter().map(|s| s.cts()).min().unwrap_or(0);
                        let mut dts = 0i64;
                        let mut rel = Vec::with_capacity(sm.len());
                        let mut out = Vec::with_capacity(sm.len());
                        for s in &sm {
                            rel.push(Rel {
                                dts,
                                cts: s.cts() - min_cts,
                                dur: s.dur,
                            });
                            out.push(OutSample::from_source(0, s));
                            dts += i64::from(s.dur);
                        }
                        rels.push(rel);
                        osamples.push(out);
                    }
                    Piece::Encode { .. } => {
                        let ep = enc_parts
                            .iter()
                            .find(|e| e.range == ri && e.piece == pi)
                            .ok_or_else(|| {
                                CutErr::Fallback("@snap: an edge was not encoded".into())
                            })?;
                        let pt = ep
                            .movie
                            .tracks
                            .iter()
                            .find(|x| x.is_video())
                            .expect("checked above");
                        // Sample descriptions of the piece join the track's, without duplicates.
                        let mut map = Vec::new();
                        for e in &pt.stsd {
                            let key = boxes::config_key(e);
                            let pos = stsd
                                .iter()
                                .position(|x| boxes::config_key(x) == key)
                                .unwrap_or_else(|| {
                                    stsd.push(e.clone());
                                    stsd.len() - 1
                                });
                            map.push(pos as u16 + 1);
                        }
                        let scale = f64::from(tl.ts) / f64::from(pt.timescale.max(1));
                        let conv = |x: i64| (x as f64 * scale).round() as i64;
                        let dts0 = pt.samples.first().map(|s| s.dts).unwrap_or(0);
                        let min_cts = pt.samples.iter().map(|s| s.cts()).min().unwrap_or(0);
                        let mut rel = Vec::new();
                        let mut out = Vec::new();
                        for (k, s) in pt.samples.iter().enumerate() {
                            let d0 = conv(s.dts - dts0);
                            let d1 = pt
                                .samples
                                .get(k + 1)
                                .map(|n| conv(n.dts - dts0))
                                .unwrap_or_else(|| conv(s.dts - dts0 + i64::from(s.dur)));
                            rel.push(Rel {
                                dts: d0,
                                cts: conv(s.cts() - min_cts),
                                dur: (d1 - d0).max(1) as u32,
                            });
                            let mut o = OutSample::from_source(ep.index, s);
                            o.desc = map[usize::from(s.desc.max(1)) - 1];
                            out.push(o);
                        }
                        rels.push(rel);
                        osamples.push(out);
                    }
                }
                // Record where this piece starts on the new timeline, for the junction check.
                piece_boundaries.push(tl.secs(bounds_ticks));
                let span = rels
                    .last()
                    .map(|r| {
                        r.iter()
                            .map(|x| x.cts + i64::from(x.dur))
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                bounds_ticks += span;
            }
        }
        let (placed, shift) = cut::place_pieces(&rels);
        let mut samples = Vec::new();
        for (pl, os) in placed.iter().zip(osamples.iter_mut()) {
            for (p, o) in pl.iter().zip(os.iter_mut()) {
                o.dur = p.dur;
                o.cts_off = (p.cts + shift - p.dts).clamp(0, i64::from(i32::MAX)) as i32;
            }
            samples.extend(os.iter().copied());
        }
        let total: i64 = rels
            .iter()
            .map(|r| {
                r.iter()
                    .map(|x| x.cts + i64::from(x.dur))
                    .max()
                    .unwrap_or(0)
            })
            .sum();
        let mut edits = Vec::new();
        if shift > 0 {
            edits.push(cut::start_edit(total.max(0) as u64, tl.ts, ts_movie, shift));
        }
        // Keep only the descriptions the result uses, the first one used first: a reader that
        // opens its decoder from the first description must get the one the first frame needs.
        let mut used: Vec<u16> = Vec::new();
        for s in &samples {
            if !used.contains(&s.desc) {
                used.push(s.desc);
            }
        }
        let stsd: Vec<Vec<u8>> = used
            .iter()
            .map(|d| stsd[usize::from(*d) - 1].clone())
            .collect();
        for s in &mut samples {
            s.desc = used.iter().position(|d| *d == s.desc).unwrap_or(0) as u16 + 1;
        }
        let ot = OutTrack {
            id: t.id,
            handler: t.handler,
            timescale: t.timescale,
            tkhd: t.tkhd.clone(),
            mdhd: t.mdhd.clone(),
            hdlr: t.hdlr.clone(),
            minf_head: t.minf_head.clone(),
            mdia_extra: t.mdia_extra.clone(),
            trak_extra: t.trak_extra.clone(),
            stsd,
            samples,
            edits,
        };
        for plan in &plans {
            span_seconds_per_range.push((tl.secs(plan.start), tl.secs(plan.end)));
        }
        v_out = Some((vi, ot));
    }
    // The other tracks follow the picture's presentation windows.
    for (ti, track) in movie.tracks.iter().enumerate() {
        if let Some((vi, ot)) = &v_out
            && ti == *vi
        {
            out_tracks.push(ot.clone());
            continue;
        }
        if !keeps(track) || track.is_video() {
            continue;
        }
        let tl = cut::timeline_of(track, ts_movie)?;
        let mut samples: Vec<OutSample> = Vec::new();
        let mut edits = Vec::new();
        let mut base = 0i64;
        let end_all = track.seconds() + 1.0;
        for (ri, (s, e)) in ranges.iter().enumerate() {
            let (ws, we) = span_seconds_per_range
                .get(ri)
                .copied()
                .unwrap_or((*s, e.unwrap_or(end_all)));
            let win = cut::plan_window(track, &tl, tl.ticks(ws), tl.ticks(we));
            let seg_ticks_movie = ((we - ws) * f64::from(ts_movie)).round().max(0.0) as u64;
            let Some(win) = win else {
                // Nothing in this track for the range: leave a gap of silence.
                edits.push(Edit {
                    duration: seg_ticks_movie,
                    media_time: -1,
                    rate: 0x10000,
                });
                continue;
            };
            let first = &track.samples[win.first];
            let dts0 = first.dts;
            let mut seg_len = 0i64;
            for s in &track.samples[win.first..win.end] {
                let mut o = OutSample::from_source(0, s);
                o.cts_off = 0;
                samples.push(o);
                seg_len += i64::from(s.dur);
            }
            let _ = dts0;
            if win.offset >= 0 {
                edits.push(Edit {
                    duration: seg_ticks_movie,
                    media_time: base + win.offset,
                    rate: 0x10000,
                });
            } else {
                let lead = ((-win.offset) as f64 * f64::from(ts_movie) / f64::from(track.timescale))
                    .round() as u64;
                edits.push(Edit {
                    duration: lead.min(seg_ticks_movie),
                    media_time: -1,
                    rate: 0x10000,
                });
                edits.push(Edit {
                    duration: seg_ticks_movie.saturating_sub(lead),
                    media_time: base,
                    rate: 0x10000,
                });
            }
            base += seg_len;
        }
        if samples.is_empty() {
            continue;
        }
        out_tracks.push(OutTrack {
            id: track.id,
            handler: track.handler,
            timescale: track.timescale,
            tkhd: track.tkhd.clone(),
            mdhd: track.mdhd.clone(),
            hdlr: track.hdlr.clone(),
            minf_head: track.minf_head.clone(),
            mdia_extra: track.mdia_extra.clone(),
            trak_extra: track.trak_extra.clone(),
            stsd: track.stsd.clone(),
            samples,
            edits,
        });
    }
    if out_tracks.is_empty() {
        return Err(CutErr::Hard(MediaError::BadOption(
            "Nothing in the file falls inside that range".into(),
        )));
    }
    // Write.
    let (dir, part) = job.part(input, &ext_of(input))?;
    let out = Output {
        ftyp: parsed.ftyp.clone(),
        timescale: ts_movie,
        mvhd: movie.mvhd.clone(),
        extra: movie.extra.clone(),
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&part)?;
    let io = job.io;
    let (lo, hi) = (0.9f32, 1.0f32);
    let last = std::cell::Cell::new(-1.0f32);
    let r = write::write(
        file,
        &out,
        &out_tracks,
        &sources,
        &|| io.cancel().is_cancelled(),
        &|f| {
            if (f - last.get()).abs() >= 0.02 || f >= 1.0 {
                last.set(f);
                io.progress(lo + (hi - lo) * f, "Writing the result");
            }
        },
    );
    match r {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
            return Err(CutErr::Hard(MediaError::Cancelled));
        }
        Err(e) => return Err(e.into()),
    }
    for p in &part_paths {
        job.stage.discard(p);
    }
    // A decode check around every splice: a bad junction falls back to the safe cut.
    if encoded_frames > 0 && copied_frames > 0 {
        for t in piece_boundaries.iter().skip(1) {
            // Each piece begins on a clean start, so decoding from there must be clean.
            let (ok, errs) = decode_window(job, &part, t + 0.002, 0.6)?;
            if !ok {
                job.stage.discard(&part);
                return Err(CutErr::Fallback(format!(
                    "@snap: the splice near {} did not decode cleanly ({}), so the cut snaps to keyframes instead.",
                    duration_text(*t),
                    errs
                )));
            }
        }
    }
    let total_secs: f64 = span_seconds_per_range
        .iter()
        .map(|(s, e)| e - s)
        .sum::<f64>()
        .max(0.0);
    let out_seconds = if total_secs > 0.0 {
        total_secs
    } else {
        out_tracks
            .iter()
            .map(|t| {
                t.samples.iter().map(|s| f64::from(s.dur)).sum::<f64>() / f64::from(t.timescale)
            })
            .fold(0.0, f64::max)
    };
    Ok(Done {
        part,
        dir,
        copied_frames,
        encoded_frames,
        encoded_with,
        out_seconds,
        notes: std::mem::take(notes),
    })
}

fn open_mp4_loose(path: &Path) -> std::result::Result<Parsed, CutErr> {
    open_mp4(path).map_err(|e| match e {
        CutErr::Fallback(m) => CutErr::Fallback(format!("@snap: {m}")),
        other => other,
    })
}

/// Decode `seconds` of video starting at `start` and report whether it was clean.
fn decode_window(
    job: &Job,
    path: &Path,
    start: f64,
    seconds: f64,
) -> std::result::Result<(bool, String), CutErr> {
    use std::ffi::OsString;
    let mut cmd = std::process::Command::new(&job.engine.ffmpeg);
    job.engine.configure(&mut cmd);
    let args: Vec<OsString> = [
        "-nostdin",
        "-hide_banner",
        "-v",
        "error",
        "-ss",
        &format!("{start:.3}"),
        "-t",
        &format!("{seconds:.3}"),
        "-i",
    ]
    .iter()
    .map(OsString::from)
    .chain([OsString::from(crate::io::tool_path(path))])
    .chain(
        ["-map", "0:v:0", "-f", "null", "-"]
            .iter()
            .map(OsString::from),
    )
    .collect();
    let out = cmd
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| {
            CutErr::Hard(MediaError::Engine(format!(
                "Could not start the video engine ({e})."
            )))
        })?;
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Ok((
        out.status.success() && err.is_empty(),
        err.lines().next().unwrap_or("").to_string(),
    ))
}

fn report(job: &Job, input: &Path, profile: &Profile, out: &Path, ranges: &[Range], d: &Done) {
    let after = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
    let mut line = format!(
        "{}: kept {} ({}, {} long).",
        name_of(out),
        describe_ranges(ranges),
        size_text(after),
        duration_text(d.out_seconds)
    );
    if d.encoded_frames == 0 {
        line.push_str(" No re-encode, lossless, finished at disk speed.");
    } else {
        line.push_str(&format!(
            " {} frames re-encoded at the cut edges ({}), {} frames copied untouched.",
            d.encoded_frames,
            d.encoded_with.clone().unwrap_or_else(|| "hardware".into()),
            d.copied_frames
        ));
    }
    job.note(line);
    for n in &d.notes {
        job.note(n.clone());
    }
    let _ = (input, profile);
}
