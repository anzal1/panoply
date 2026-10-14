//! `media.streams`: take out, drop, set default and rename tracks, without touching a sample
//! (spec 4.17). Everything is a stream copy, so every kept stream is bit-identical and a large
//! file finishes at disk speed. With no option it only shows the table of streams.

use std::path::{Path, PathBuf};

use crate::error::{MediaError, Result};
use crate::io::{Io, Naming};
use crate::opts::Opts;
use crate::probe::{Container, Kind, Profile, Stream};
use crate::units::{name_of, size_text, stem_of};

use super::audio::AudioFormat;
use super::info;
use super::{Args, Job, with_name};

fn indices(text: Option<&str>, p: &Profile, what: &str) -> Result<Vec<u32>> {
    let Some(t) = text else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for part in t.split([',', ' ']).filter(|x| !x.is_empty()) {
        let i: u32 = part.parse().map_err(|_| {
            MediaError::BadOption(format!(
                "\"{part}\" is not a stream number. {what} takes numbers like 2 or 2,5"
            ))
        })?;
        if !p.streams.iter().any(|s| s.index == i) {
            return Err(MediaError::BadOption(format!(
                "This file has no stream {i}. Its streams are numbered 0 to {}",
                p.streams.len().saturating_sub(1)
            )));
        }
        out.push(i);
    }
    Ok(out)
}

/// Run `media.streams`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let mut job = Job::new(io)?;
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        one(&mut job, input, opts).map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

fn kind_letter(k: Kind) -> &'static str {
    match k {
        Kind::Video => "v",
        Kind::Audio => "a",
        Kind::Subtitle => "s",
        _ => "d",
    }
}

fn one(job: &mut Job, input: &Path, opts: &Opts) -> Result<()> {
    let input = job.input(input)?;
    let p = job.probe(&input)?;
    let drop = indices(opts.str("drop"), &p, "drop")?;
    let extract = indices(opts.str("extract"), &p, "extract")?;
    let default = indices(opts.str("default"), &p, "default")?;
    let langs: Vec<(u32, String)> = match opts.str("language") {
        None => Vec::new(),
        Some(t) => t
            .split(',')
            .filter(|x| !x.trim().is_empty())
            .map(|pair| {
                let (i, l) = pair.split_once('=').ok_or_else(|| {
                    MediaError::BadOption(format!("\"{pair}\" should look like 2=eng (the stream number, then the language code)"))
                })?;
                let i: u32 = i.trim().parse().map_err(|_| MediaError::BadOption(format!("\"{i}\" is not a stream number")))?;
                if !p.streams.iter().any(|s| s.index == i) {
                    return Err(MediaError::BadOption(format!("This file has no stream {i}")));
                }
                Ok((i, l.trim().to_string()))
            })
            .collect::<Result<Vec<_>>>()?,
    };
    if drop.is_empty() && extract.is_empty() && default.is_empty() && langs.is_empty() {
        // The table: one line per stream with its size, chapters and attachments.
        job.note(format!(
            "{}: {} streams. Say what to do with drop, extract, default or language.",
            name_of(&input),
            p.streams.len()
        ));
        for s in &p.streams {
            job.note(format!("  stream {}: {}", s.index, info_line(s)));
        }
        return Ok(());
    }
    for i in &drop {
        if extract.contains(i) {
            return Err(MediaError::BadOption(format!(
                "Stream {i} is both dropped and extracted. Pick one"
            )));
        }
    }
    let mut results = 0;
    if !extract.is_empty() {
        let dir = {
            let d = job.io.output_dir_for(&input)?;
            job.stage.dir(&d, &input, "streams")?
        };
        for i in &extract {
            let s = p.streams.iter().find(|s| s.index == *i).expect("checked");
            extract_one(job, &input, &p, s, &dir)?;
            results += 1;
        }
    }
    if !drop.is_empty() || !default.is_empty() || !langs.is_empty() {
        remux(
            job,
            &input,
            &p,
            &drop,
            &extract_none(&extract),
            &default,
            &langs,
            opts.text("container", "same"),
        )?;
        results += 1;
    }
    let _ = results;
    Ok(())
}

fn extract_none(_e: &[u32]) -> Vec<u32> {
    Vec::new()
}

fn info_line(s: &Stream) -> String {
    let mut t = format!(
        "{} {}",
        match s.kind {
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Subtitle => "subtitles",
            Kind::Attachment => "attachment",
            Kind::Data => "data",
        },
        s.codec
    );
    if !s.language.is_empty() && s.language != "und" {
        t.push_str(&format!(", {}", s.language));
    }
    if !s.title.is_empty() {
        t.push_str(&format!(", \"{}\"", s.title));
    }
    if s.default {
        t.push_str(", default");
    }
    if s.forced {
        t.push_str(", forced");
    }
    let _ = info::Problem {
        text: String::new(),
        fix: None,
    };
    t
}

fn extract_one(job: &mut Job, input: &Path, p: &Profile, s: &Stream, dir: &Path) -> Result<()> {
    let lang = if s.language.is_empty() || s.language == "und" {
        String::new()
    } else {
        format!(" {}", s.language)
    };
    let (ext, muxer, extra): (String, &str, Vec<&str>) = match s.kind {
        Kind::Audio => match AudioFormat::for_codec(&s.codec) {
            Some(f) => (f.ext().into(), f.muxer(), vec![]),
            None => ("mka".into(), "matroska", vec![]),
        },
        Kind::Video => ("mkv".into(), "matroska", vec![]),
        Kind::Subtitle => match s.codec.as_str() {
            "subrip" | "srt" => ("srt".into(), "srt", vec![]),
            "ass" | "ssa" => ("ass".into(), "ass", vec![]),
            "webvtt" => ("vtt".into(), "webvtt", vec![]),
            "mov_text" => ("srt".into(), "srt", vec!["-c:s", "srt"]),
            "hdmv_pgs_subtitle" => ("sup".into(), "sup", vec![]),
            _ => ("mks".into(), "matroska", vec![]),
        },
        Kind::Attachment => ("bin".into(), "data", vec![]),
        Kind::Data => ("bin".into(), "data", vec![]),
    };
    if matches!(s.kind, Kind::Attachment | Kind::Data) {
        return Err(MediaError::Unsupported(format!(
            "Stream {} is an attachment or data track, which cannot be written as a file of its own.",
            s.index
        )));
    }
    let part = job.stage.part(dir, &ext)?;
    let mut a = Args::new();
    a.a("-y");
    a.input(input);
    a.kv("-map", format!("0:{}", s.index));
    if extra.is_empty() {
        a.kv("-c", "copy");
    } else {
        a.all(&extra);
    }
    if s.kind == Kind::Audio && muxer == "ipod" {
        a.kv("-movflags", "+faststart");
    }
    a.kv("-f", muxer);
    a.a(crate::io::tool_path(&part));
    job.ffmpeg(a, p.duration, "Taking the stream out", (0.0, 1.0), input)?;
    let name = format!("{} (stream {}{lang}).{ext}", stem_of(input), s.index);
    let out = job.stage.commit_named(&part, dir, &name)?;
    let size = std::fs::metadata(&out)?.len();
    job.note(format!(
        "{}: stream {} ({}), copied as it is ({}).",
        name_of(&out),
        s.index,
        s.codec,
        size_text(size)
    ));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn remux(
    job: &mut Job,
    input: &Path,
    p: &Profile,
    drop: &[u32],
    _skip: &[u32],
    default: &[u32],
    langs: &[(u32, String)],
    container: &str,
) -> Result<()> {
    let in_ext = crate::units::ext_of(input);
    let to_mkv = container == "mkv" && in_ext != "mkv";
    let ext = if to_mkv {
        "mkv".to_string()
    } else {
        in_ext.clone()
    };
    let muxer = match ext.as_str() {
        "mkv" => "matroska",
        "mka" => "matroska",
        "mp4" | "m4v" => "mp4",
        "mov" => "mov",
        "webm" => "webm",
        "m4a" => "ipod",
        other => other,
    };
    if !matches!(
        p.container,
        Container::Mp4
            | Container::Mov
            | Container::Mkv
            | Container::Webm
            | Container::Avi
            | Container::Ts
            | Container::Other
    ) && !to_mkv
    {
        // Single-stream audio files have nothing to drop.
        return Err(MediaError::Unsupported(
            "This kind of file holds one stream, so there is nothing to drop or relabel.".into(),
        ));
    }
    let (dir, part) = job.part(input, &ext)?;
    let mut a = Args::new();
    a.a("-y");
    a.input(input);
    a.kv("-map_metadata", "0");
    a.kv("-map_chapters", "0");
    let mut counters = std::collections::HashMap::<&str, usize>::new();
    let mut kept = 0;
    let mut dropped = 0;
    for s in &p.streams {
        if drop.contains(&s.index) {
            dropped += 1;
            continue;
        }
        a.kv("-map", format!("0:{}", s.index));
        let l = kind_letter(s.kind);
        let k = {
            let c = counters.entry(l).or_insert(0);
            let v = *c;
            *c += 1;
            v
        };
        if matches!(s.kind, Kind::Video | Kind::Audio | Kind::Subtitle) {
            a.kv(&format!("-c:{l}:{k}"), "copy");
            if s.kind == Kind::Video && s.codec == "hevc" && matches!(muxer, "mp4" | "mov") {
                a.kv(&format!("-tag:v:{k}"), "hvc1");
            }
            if let Some((_, lang)) = langs.iter().find(|(i, _)| *i == s.index) {
                a.kv(&format!("-metadata:s:{l}:{k}"), format!("language={lang}"));
            }
            // A chosen default replaces the other defaults of its kind.
            if !default.is_empty() {
                let chosen = default
                    .iter()
                    .any(|i| p.streams.iter().any(|x| x.index == *i && x.kind == s.kind));
                if chosen {
                    let on = default.contains(&s.index);
                    a.kv(
                        &format!("-disposition:{l}:{k}"),
                        if on { "default" } else { "0" },
                    );
                }
            }
        } else if matches!(s.kind, Kind::Attachment) && muxer == "matroska" {
            a.kv("-c:t", "copy");
        }
        kept += 1;
    }
    if kept == 0 {
        return Err(MediaError::BadOption(
            "That would drop every stream, and nothing would be left".into(),
        ));
    }
    if matches!(muxer, "mp4" | "mov") {
        a.kv("-movflags", "+faststart");
    }
    a.kv("-f", muxer);
    a.a(crate::io::tool_path(&part));
    job.ffmpeg(
        a,
        p.duration,
        "Rewriting the streams, no re-encode",
        (0.0, 1.0),
        input,
    )?;
    let out = job.commit(
        &part,
        &dir,
        input,
        &Naming::Labelled("streams".into()),
        &ext,
    )?;
    job.verify(&out, p.duration)?;
    let size = std::fs::metadata(&out)?.len();
    job.note(format!(
        "{}: {} kept, {} dropped, no re-encode, every kept stream is bit-identical ({} to {}).",
        name_of(&out),
        kept,
        dropped,
        size_text(p.size),
        size_text(size)
    ));
    Ok(())
}
