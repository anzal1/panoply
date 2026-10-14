//! `media.repair`: this video will not play, fix it (spec 4.18).
//!
//! It diagnoses before it acts and says what it found in plain words. The three cases of damage
//! it can repair, and the one thing it cannot, are described in [`crate::mp4::repair`]. No file is
//! called repaired unless the result plays start to end in a decode test.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::{MediaError, Result};
use crate::io::{Io, Naming};
use crate::mp4::boxes::{self, bx};
use crate::mp4::movie::{Movie, parse_movie};
use crate::mp4::repair::{self, MoovState, Nal};
use crate::mp4::source::Source;
use crate::mp4::write::{self, Output};
use crate::opts::Opts;
use crate::probe::{self, Profile};
use crate::proc;
use crate::units::{duration_text, ext_of, name_of, size_text};

use super::{Args, Job, with_name};

/// Run `media.repair`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let mut job = Job::new(io)?;
    let method = opts.text("method", "auto").to_string();
    let reference = match opts.str("reference") {
        Some(r) => Some(job.input(Path::new(r))?),
        None => None,
    };
    let mut work: Vec<PathBuf> = Vec::new();
    for i in inputs {
        let real = job.input(i)?;
        if reference.as_ref().is_some_and(|r| *r == real) {
            continue;
        }
        work.push(real);
    }
    if work.is_empty() {
        return Err(MediaError::BadOption(
            "Choose the broken file, and a working one from the same camera as the reference"
                .into(),
        ));
    }
    let n = work.len();
    for (i, input) in work.iter().enumerate() {
        job.slice(i, n);
        one(&mut job, input, &method, reference.as_deref())
            .map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

fn read_movie(src: &Source) -> Result<(Movie, Vec<u8>)> {
    let layout = boxes::scan(src)?;
    let moov = layout
        .find(b"moov")
        .filter(|m| !m.truncated)
        .ok_or_else(|| {
            MediaError::Corrupt(
                "The reference file has no complete index either. Choose a file that plays.".into(),
            )
        })?;
    let body = src.read_exact_at(
        moov.payload(),
        (moov.size - u64::from(moov.header_len)) as usize,
    )?;
    let ftyp = match layout.find(b"ftyp") {
        Some(f) => src.read_exact_at(f.start, f.size as usize)?,
        None => bx(b"ftyp", b"isom\0\0\x02\0isomiso2mp41"),
    };
    Ok((parse_movie(&body)?, ftyp))
}

fn decode_check(job: &Job, path: &Path) -> Result<Vec<String>> {
    let args: Vec<OsString> = ["-nostdin", "-hide_banner", "-v", "error", "-i"]
        .iter()
        .map(OsString::from)
        .chain([OsString::from(crate::io::tool_path(path))])
        .chain(["-f", "null", "-"].iter().map(OsString::from))
        .collect();
    let done = proc::capture_ffmpeg(&job.engine, job.io, &args)?;
    // The null muxer grumbles about timestamps of streams that have none to speak of (raw
    // pictures, B-frame reordering at the end); that is not a decode problem.
    let mut lines: Vec<String> = done
        .tail
        .lines()
        .map(str::to_string)
        .filter(|l| {
            !l.trim().is_empty() && !l.contains("non monotonically increasing dts to muxer")
        })
        .collect();
    if !done.ok && lines.is_empty() {
        lines.push("the decoder stopped".into());
    }
    Ok(lines)
}

fn one(job: &mut Job, input: &Path, method: &str, reference: Option<&Path>) -> Result<()> {
    let src = Source::open(input)?;
    let diag = repair::diagnose(&src)?;
    let mp4_family = diag.has_ftyp || diag.mdat.is_some();
    let ext = ext_of(input);
    let name = name_of(input);
    if !mp4_family || diag.fragmented || method == "remux" {
        return remux(job, input, &ext, &name, method == "remux");
    }
    match diag.moov {
        MoovState::Complete => {
            let layout = boxes::scan(&src)?;
            let moov = layout.find(b"moov").expect("complete");
            let body = src.read_exact_at(
                moov.payload(),
                (moov.size - u64::from(moov.header_len)) as usize,
            )?;
            let movie = match parse_movie(&body) {
                Ok(m) => m,
                Err(_) => return remux(job, input, &ext, &name, false),
            };
            let ok = repair::intact(&movie, src.len());
            if ok.intact == ok.total && diag.garbage_at.is_none() && method != "rebuild" {
                let errs = decode_check(job, input)?;
                if errs.is_empty() {
                    job.note(format!(
                        "{name} is not damaged: its index is complete, all {} samples are in the file and it plays start to end. Nothing was changed.",
                        ok.total
                    ));
                    return Ok(());
                }
                job.note(format!("{name} has a complete index but the decoder found problems in the data ({}). Trying a re-mux.", errs[0]));
                return remux(job, input, &ext, &name, false);
            }
            if ok.intact < ok.total {
                job.note(format!(
                    "{name} is cut off in the middle of its data: the index lists {} but only {} of {} samples ({:.0}%) are in the file. About {} of {} survive.",
                    duration_text(ok.seconds_total),
                    ok.intact,
                    ok.total,
                    100.0 * ok.intact as f64 / ok.total.max(1) as f64,
                    duration_text(ok.seconds),
                    duration_text(ok.seconds_total)
                ));
                return truncate(job, input, &src, &movie, &ext, &name);
            }
            // Extra bytes after the last box: a re-mux drops them.
            job.note(format!(
                "{name} has extra bytes after its last part. They are not part of the video."
            ));
            truncate(job, input, &src, &movie, &ext, &name)
        }
        MoovState::Missing | MoovState::Truncated => {
            rebuild(job, input, &src, &diag, reference, &ext, &name)
        }
    }
}

fn finish(
    job: &mut Job,
    part: &Path,
    dir: &Path,
    input: &Path,
    ext: &str,
    name: &str,
    recovered: String,
) -> Result<()> {
    let errs = decode_check(job, part)?;
    let out = job.commit(part, dir, input, &Naming::Labelled("repaired".into()), ext)?;
    let size = std::fs::metadata(&out)?.len();
    if errs.is_empty() {
        job.note(format!(
            "{}: repaired. {recovered} It plays start to end in a decode test ({}).",
            name_of(&out),
            size_text(size)
        ));
    } else {
        job.note(format!(
            "{}: recovered what could be recovered ({}), but it is not declared repaired: the decode test found {} problem(s), the first being \"{}\". {recovered}",
            name_of(&out),
            size_text(size),
            errs.len(),
            errs[0]
        ));
    }
    let _ = name;
    Ok(())
}

fn remux(job: &mut Job, input: &Path, ext: &str, name: &str, forced: bool) -> Result<()> {
    let (dir, part) = job.part(input, ext)?;
    let mut a = Args::new();
    a.a("-y");
    a.kv("-fflags", "+genpts+discardcorrupt");
    a.kv("-err_detect", "ignore_err");
    a.input(input);
    a.kv("-map", "0");
    a.kv("-c", "copy");
    a.a("-ignore_unknown");
    a.kv("-map_metadata", "0");
    if matches!(ext, "mp4" | "mov" | "m4v") {
        a.kv("-movflags", "+faststart");
    }
    a.a(crate::io::tool_path(&part));
    let probe_ok = probe::probe(&job.engine, job.io, input).ok();
    let dur = probe_ok.as_ref().and_then(|p: &Profile| p.duration);
    job.note(format!(
        "{name}: {}. Its streams were re-read from the data and written into a fresh file; damaged stretches are skipped.",
        if forced { "re-mux requested" } else { "the structure is damaged in a way that needs a re-read" }
    ));
    job.ffmpeg(a, dur, "Re-reading the file", (0.0, 0.9), input)?;
    finish(job, &part, &dir, input, ext, name, String::new())
}

fn truncate(
    job: &mut Job,
    input: &Path,
    src: &Source,
    movie: &Movie,
    ext: &str,
    name: &str,
) -> Result<()> {
    let (tracks, cut) = repair::truncated_tracks(movie, src.len());
    if tracks.is_empty() {
        return Err(MediaError::Corrupt(
            "None of the media data is in the file, so nothing can be recovered.".into(),
        ));
    }
    let layout = boxes::scan(src)?;
    let ftyp = match layout.find(b"ftyp") {
        Some(f) => src.read_exact_at(f.start, f.size as usize)?,
        None => bx(b"ftyp", b"isom\0\0\x02\0isomiso2mp41"),
    };
    let (dir, part) = job.part(input, ext)?;
    let out = Output {
        ftyp,
        timescale: movie.timescale,
        mvhd: movie.mvhd.clone(),
        extra: movie.extra.clone(),
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&part)?;
    let io = job.io;
    let (lo, hi) = (job.lo, job.hi);
    let w = write::write(
        file,
        &out,
        &tracks,
        &[src],
        &|| io.cancel().is_cancelled(),
        &|f| io.progress(lo + (hi - lo) * f * 0.9, "Writing a new index"),
    );
    match w {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Err(MediaError::Cancelled),
        Err(e) => return Err(e.into()),
    }
    let secs = tracks
        .iter()
        .map(|t| {
            t.samples.iter().map(|s| f64::from(s.dur)).sum::<f64>() / f64::from(t.timescale.max(1))
        })
        .fold(0.0, f64::max);
    let lost = if cut > 0.0 {
        format!(
            " The last stretch after {} could not be kept in step, so all tracks end together.",
            duration_text(cut)
        )
    } else {
        String::new()
    };
    finish(
        job,
        &part,
        &dir,
        input,
        ext,
        name,
        format!(
            "Recovered {} of video and sound with a new index.{lost}",
            duration_text(secs)
        ),
    )
}

fn rebuild(
    job: &mut Job,
    input: &Path,
    src: &Source,
    diag: &repair::Diagnosis,
    reference: Option<&Path>,
    ext: &str,
    name: &str,
) -> Result<()> {
    let (start, end, _) = diag.mdat.ok_or_else(|| {
        MediaError::Corrupt(
            "There is no media data in this file, so there is nothing to recover.".into(),
        )
    })?;
    job.note(format!(
        "{name} has no usable index (the recorder stopped before writing it), so no player can read it. The media data is there: {}.",
        size_text(end - start)
    ));
    // Parameter sets: from the reference, else from the stream itself.
    let (params, fps, rotation, hints, ref_movie) = match reference {
        Some(r) => {
            let rs = Source::open(r)?;
            let (rm, rftyp) = read_movie(&rs)?;
            let vt =
                rm.tracks.iter().find(|t| t.is_video()).ok_or_else(|| {
                    MediaError::Corrupt("The reference file has no video.".into())
                })?;
            let p = repair::params_from_track(vt).ok_or_else(|| {
                MediaError::Unsupported("The reference file's video is not H.264 or HEVC, which is all Panoply can rebuild.".into())
            })?;
            let secs = vt.seconds().max(0.001);
            let rot = rotation_of(job, r);
            let hints = repair::hints_from_track(vt);
            (
                p,
                vt.samples.len() as f64 / secs,
                rot,
                hints,
                Some((rm, rftyp)),
            )
        }
        None => {
            let mut found = None;
            for codec in [Nal::Avc, Nal::Hevc] {
                if let Some(p) = repair::params_from_stream(src, start, end, codec, 4)? {
                    found = Some(p);
                    break;
                }
            }
            let Some(p) = found else {
                return Err(MediaError::Unsupported(
                    "This file does not carry the settings its video needs (most MP4 files keep them in the index that is missing). Choose a working video from the same camera or app as the reference, and try again."
                        .into(),
                ));
            };
            job.note("No reference file was given, so the video settings were read from the stream itself and the frame rate is assumed to be 30. A reference from the same camera gives exact timing.".to_string());
            {
                let hints = repair::Hints::from_params(&p);
                (p, 30.0, 0, hints, None)
            }
        }
    };
    let io = job.io;
    let cancel = || io.cancel().is_cancelled();
    let scan = repair::scan_pictures(
        src,
        start,
        end,
        params.codec,
        params.length_size,
        hints,
        &cancel,
    )
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::Interrupted {
            MediaError::Cancelled
        } else {
            e.into()
        }
    })?;
    if scan.pictures.is_empty() {
        return Err(MediaError::Corrupt(
            "No pictures could be found in the media data. The reference may not be from the same camera or app, or the data is damaged all through.".into(),
        ));
    }
    let frames = scan.pictures.len();
    let total = (end - start).max(1);
    let pct = 100.0 * scan.picture_bytes as f64 / total as f64;
    let (dir, part) = job.part(input, ext)?;
    let sound_note = |other: u64| -> String {
        if other as f64 > 0.05 * total as f64 {
            format!(
                " About {} of the data is sound, which cannot be rebuilt without the index (sound frames carry no length), so the result has no audio.",
                size_text(other)
            )
        } else {
            String::new()
        }
    };
    let lost_note = if scan.stopped_at + 1024 < end {
        format!(
            " The data after {} is damaged and was not used.",
            size_text(scan.stopped_at - start)
        )
    } else {
        String::new()
    };
    if let Some((rm, rftyp)) = ref_movie {
        // The reference supplies everything the index held; the pictures are copied as they are
        // and put in display order from their slice headers.
        let vt = rm
            .tracks
            .iter()
            .find(|t| t.is_video())
            .expect("checked above");
        let rebuilt = repair::rebuild_track(src, &scan, &params, vt, rm.timescale)?;
        let out = Output {
            ftyp: rftyp,
            timescale: rm.timescale,
            mvhd: rm.mvhd.clone(),
            extra: Vec::new(),
        };
        let file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&part)?;
        let (lo, hi) = (job.lo, job.hi);
        let w = write::write(file, &out, &[rebuilt.track], &[src], &cancel, &|f| {
            io.progress(lo + (hi - lo) * f * 0.9, "Writing a new index")
        });
        match w {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                return Err(MediaError::Cancelled);
            }
            Err(e) => return Err(e.into()),
        }
        let order = if rebuilt.ordered {
            ""
        } else {
            " The order pictures are shown in could not be read, so it was assumed to be the order they are stored in; fast motion may look wrong."
        };
        return finish(
            job,
            &part,
            &dir,
            input,
            ext,
            name,
            format!(
                "Recovered {frames} pictures ({} at {fps:.2} fps), about {pct:.0}% of the media data, copied as they were with a new index built from the reference.{}{}{order}",
                duration_text(frames as f64 / fps.max(1.0)),
                sound_note(scan.other_bytes),
                lost_note
            ),
        );
    }
    let raw = job.stage.scratch(
        &dir,
        if params.codec == Nal::Avc {
            "h264"
        } else {
            "h265"
        },
    );
    {
        let f = std::fs::File::create(&raw)?;
        let w = repair::write_annexb(src, &scan, &params, std::io::BufWriter::new(f), &cancel);
        match w {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                return Err(MediaError::Cancelled);
            }
            Err(e) => return Err(e.into()),
        }
    }
    let mut a = Args::new();
    a.a("-y");
    if rotation != 0 {
        // ffmpeg's display rotation is counter-clockwise.
        a.kv("-display_rotation", ((360 - rotation) % 360).to_string());
    }
    a.kv(
        "-f",
        if params.codec == Nal::Avc {
            "h264"
        } else {
            "hevc"
        },
    );
    a.kv("-framerate", format!("{fps:.3}"));
    a.input(&raw);
    a.kv("-c", "copy");
    if params.codec == Nal::Hevc {
        a.kv("-tag:v", "hvc1");
    }
    if matches!(ext, "mp4" | "mov" | "m4v") {
        a.kv("-movflags", "+faststart");
    }
    a.a(crate::io::tool_path(&part));
    job.ffmpeg(
        a,
        Some(frames as f64 / fps.max(1.0)),
        "Writing the rebuilt video",
        (0.3, 0.9),
        input,
    )?;
    job.stage.discard(&raw);
    finish(
        job,
        &part,
        &dir,
        input,
        ext,
        name,
        format!(
            "Recovered {frames} pictures ({} at {fps:.2} fps), about {pct:.0}% of the media data.{}{}",
            duration_text(frames as f64 / fps.max(1.0)),
            sound_note(scan.other_bytes),
            lost_note
        ),
    )
}

fn rotation_of(job: &Job, path: &Path) -> i32 {
    probe::probe(&job.engine, job.io, path)
        .ok()
        .and_then(|p| p.video().map(|v| v.rotation))
        .unwrap_or(0)
}
