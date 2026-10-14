//! Writing an MP4 from sample lists: the index is rebuilt from scratch and the sample bytes are
//! copied from their sources in time order, in one pass, so memory stays flat for any length.
//!
//! The index goes first (`faststart`, spec MOOV), chunks are about half a second so players
//! that stream the file read audio and video together, offsets switch to 64 bit above 4 GB
//! (BIG), and a sample can point into any of several source files, which is how an edit is
//! spliced from re-encoded pieces and copied ones.

use std::io::{self, BufWriter, Write};

use super::boxes::{FourCc, bx, full};
use super::movie::{Edit, Sample};
use super::source::Source;

/// One sample of an output track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutSample {
    /// Which source file holds its bytes (index into the list given to [`write`]).
    pub src: usize,
    /// Where in that file.
    pub offset: u64,
    /// How many bytes.
    pub size: u32,
    /// Duration in track time units.
    pub dur: u32,
    /// Composition offset in track time units (never negative here).
    pub cts_off: i32,
    /// True for a sync sample.
    pub sync: bool,
    /// Sample description, 1 based, within this track's `stsd`.
    pub desc: u16,
}

impl OutSample {
    /// A sample taken from the source as it is.
    pub fn from_source(src: usize, s: &Sample) -> Self {
        OutSample {
            src,
            offset: s.offset,
            size: s.size,
            dur: s.dur,
            cts_off: s.cts_off,
            sync: s.sync,
            desc: s.desc,
        }
    }
}

/// One output track.
#[derive(Debug, Clone)]
pub struct OutTrack {
    /// Track id.
    pub id: u32,
    /// Handler code.
    pub handler: FourCc,
    /// Track time units per second.
    pub timescale: u32,
    /// `tkhd` template (the matrix, size and flags are kept; the duration is rewritten).
    pub tkhd: Vec<u8>,
    /// `mdhd` template (language and times are kept; the duration is rewritten).
    pub mdhd: Vec<u8>,
    /// `hdlr` box.
    pub hdlr: Vec<u8>,
    /// `minf` boxes other than the sample table.
    pub minf_head: Vec<u8>,
    /// `mdia` boxes other than the three above.
    pub mdia_extra: Vec<u8>,
    /// `trak` boxes other than the four above.
    pub trak_extra: Vec<u8>,
    /// Sample descriptions, each a whole box.
    pub stsd: Vec<Vec<u8>>,
    /// The samples in decode order.
    pub samples: Vec<OutSample>,
    /// The edit list, in movie time units.
    pub edits: Vec<Edit>,
}

/// The file-level parts.
#[derive(Debug, Clone)]
pub struct Output {
    /// The whole `ftyp` box.
    pub ftyp: Vec<u8>,
    /// Movie time units per second.
    pub timescale: u32,
    /// `mvhd` template.
    pub mvhd: Vec<u8>,
    /// Other `moov` children (`udta`, `meta`).
    pub extra: Vec<u8>,
}

/// What was written.
#[derive(Debug, Clone, Copy, Default)]
pub struct Written {
    /// Bytes of sample data copied.
    pub data_bytes: u64,
    /// Samples written.
    pub samples: u64,
    /// Total file size.
    pub file_bytes: u64,
}

fn put32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

fn rle<T: PartialEq + Copy>(items: impl Iterator<Item = T>) -> Vec<(u32, T)> {
    let mut out: Vec<(u32, T)> = Vec::new();
    for it in items {
        match out.last_mut() {
            Some((n, v)) if *v == it => *n += 1,
            _ => out.push((1, it)),
        }
    }
    out
}

/// A chunk: consecutive samples of one track written together.
#[derive(Debug, Clone, Copy)]
struct Chunk {
    track: usize,
    first: usize,
    count: usize,
    start: f64,
    bytes: u64,
}

const CHUNK_SECONDS: f64 = 0.5;
const MAX_CHUNK_SAMPLES: usize = 8192;

fn make_chunks(tracks: &[OutTrack]) -> Vec<Chunk> {
    let mut all = Vec::new();
    for (ti, t) in tracks.iter().enumerate() {
        let ts = f64::from(t.timescale.max(1));
        let mut i = 0usize;
        let mut dts = 0u64;
        while i < t.samples.len() {
            let first = i;
            let start = dts as f64 / ts;
            let desc = t.samples[i].desc;
            let mut dur = 0u64;
            let mut bytes = 0u64;
            while i < t.samples.len()
                && t.samples[i].desc == desc
                && i - first < MAX_CHUNK_SAMPLES
                && (dur as f64 / ts) < CHUNK_SECONDS
            {
                dur += u64::from(t.samples[i].dur);
                bytes += u64::from(t.samples[i].size);
                i += 1;
            }
            dts += dur;
            all.push(Chunk {
                track: ti,
                first,
                count: i - first,
                start,
                bytes,
            });
        }
    }
    all.sort_by(|a, b| {
        a.start
            .total_cmp(&b.start)
            .then(a.track.cmp(&b.track))
            .then(a.first.cmp(&b.first))
    });
    all
}

fn patch_tkhd(tkhd: &[u8], duration: u64) -> Vec<u8> {
    let mut t = tkhd.to_vec();
    if t.len() >= 44 && t[8] == 1 {
        t[36..44].copy_from_slice(&duration.to_be_bytes());
    } else if t.len() >= 32 {
        t[28..32].copy_from_slice(&(duration.min(u64::from(u32::MAX)) as u32).to_be_bytes());
    }
    t
}

fn build_mdhd(template: &[u8], timescale: u32, duration: u64) -> Vec<u8> {
    // Keep creation and modification time and the language of the template.
    let (cm, lang) = if template.len() >= 12 && template[8] == 1 && template.len() >= 44 {
        let c = u64::from_be_bytes(template[12..20].try_into().unwrap_or([0; 8]))
            .min(u64::from(u32::MAX)) as u32;
        let m = u64::from_be_bytes(template[20..28].try_into().unwrap_or([0; 8]))
            .min(u64::from(u32::MAX)) as u32;
        ((c, m), u16::from_be_bytes([template[44], template[45]]))
    } else if template.len() >= 34 {
        (
            (
                u32::from_be_bytes(template[12..16].try_into().unwrap_or([0; 4])),
                u32::from_be_bytes(template[16..20].try_into().unwrap_or([0; 4])),
            ),
            u16::from_be_bytes([template[32], template[33]]),
        )
    } else {
        ((0, 0), 0x55C4)
    };
    let mut p = Vec::new();
    if duration > u64::from(u32::MAX) {
        p.extend_from_slice(&u64::from(cm.0).to_be_bytes());
        p.extend_from_slice(&u64::from(cm.1).to_be_bytes());
        put32(&mut p, timescale);
        p.extend_from_slice(&duration.to_be_bytes());
        p.extend_from_slice(&lang.to_be_bytes());
        p.extend_from_slice(&[0, 0]);
        full(b"mdhd", 1, 0, &p)
    } else {
        put32(&mut p, cm.0);
        put32(&mut p, cm.1);
        put32(&mut p, timescale);
        put32(&mut p, duration as u32);
        p.extend_from_slice(&lang.to_be_bytes());
        p.extend_from_slice(&[0, 0]);
        full(b"mdhd", 0, 0, &p)
    }
}

fn build_elst(edits: &[Edit]) -> Vec<u8> {
    let big = edits
        .iter()
        .any(|e| e.duration > u64::from(u32::MAX) || e.media_time > i64::from(i32::MAX));
    let mut p = Vec::new();
    put32(&mut p, edits.len() as u32);
    for e in edits {
        if big {
            p.extend_from_slice(&e.duration.to_be_bytes());
            p.extend_from_slice(&e.media_time.to_be_bytes());
        } else {
            put32(&mut p, e.duration as u32);
            put32(&mut p, e.media_time as i32 as u32);
        }
        put32(&mut p, e.rate);
    }
    bx(b"edts", &full(b"elst", u8::from(big), 0, &p))
}

fn build_stbl(t: &OutTrack, chunks: &[(usize, usize, u64)], co64: bool) -> Vec<u8> {
    let mut stbl = Vec::new();
    let mut p = Vec::new();
    put32(&mut p, t.stsd.len() as u32);
    for e in &t.stsd {
        p.extend_from_slice(e);
    }
    stbl.extend(full(b"stsd", 0, 0, &p));
    // stts
    let runs = rle(t.samples.iter().map(|s| s.dur));
    let mut p = Vec::new();
    put32(&mut p, runs.len() as u32);
    for (n, d) in &runs {
        put32(&mut p, *n);
        put32(&mut p, *d);
    }
    stbl.extend(full(b"stts", 0, 0, &p));
    // ctts
    if t.samples.iter().any(|s| s.cts_off != 0) {
        let runs = rle(t.samples.iter().map(|s| s.cts_off));
        let mut p = Vec::new();
        put32(&mut p, runs.len() as u32);
        for (n, o) in &runs {
            put32(&mut p, *n);
            put32(&mut p, *o as u32);
        }
        stbl.extend(full(b"ctts", 0, 0, &p));
    }
    // stss
    if t.samples.iter().any(|s| !s.sync) {
        let idx: Vec<u32> = t
            .samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.sync)
            .map(|(i, _)| i as u32 + 1)
            .collect();
        let mut p = Vec::new();
        put32(&mut p, idx.len() as u32);
        for i in idx {
            put32(&mut p, i);
        }
        stbl.extend(full(b"stss", 0, 0, &p));
    }
    // stsz
    let mut p = Vec::new();
    let all_same = t
        .samples
        .first()
        .map(|f| t.samples.iter().all(|s| s.size == f.size))
        .unwrap_or(false);
    if all_same {
        put32(&mut p, t.samples[0].size);
        put32(&mut p, t.samples.len() as u32);
    } else {
        put32(&mut p, 0);
        put32(&mut p, t.samples.len() as u32);
        for s in &t.samples {
            put32(&mut p, s.size);
        }
    }
    stbl.extend(full(b"stsz", 0, 0, &p));
    // stsc: runs of (samples per chunk, description) over the track's chunks in decode order.
    let mut by_order: Vec<(usize, usize)> = chunks.iter().map(|c| (c.0, c.1)).collect();
    by_order.sort();
    let mut runs: Vec<(u32, u32, u32)> = Vec::new();
    for (ci, (first, count)) in by_order.iter().enumerate() {
        let desc = u32::from(t.samples[*first].desc);
        match runs.last() {
            Some(&(_, per, d)) if per == *count as u32 && d == desc => {}
            _ => runs.push((ci as u32 + 1, *count as u32, desc)),
        }
    }
    let mut p = Vec::new();
    put32(&mut p, runs.len() as u32);
    for (f, per, d) in &runs {
        put32(&mut p, *f);
        put32(&mut p, *per);
        put32(&mut p, *d);
    }
    stbl.extend(full(b"stsc", 0, 0, &p));
    // chunk offsets, in decode order of chunks
    let mut offs: Vec<(usize, u64)> = chunks.iter().map(|c| (c.0, c.2)).collect();
    offs.sort();
    let mut p = Vec::new();
    put32(&mut p, offs.len() as u32);
    for (_, o) in &offs {
        if co64 {
            p.extend_from_slice(&o.to_be_bytes());
        } else {
            put32(&mut p, *o as u32);
        }
    }
    stbl.extend(full(if co64 { b"co64" } else { b"stco" }, 0, 0, &p));
    bx(b"stbl", &stbl)
}

fn build_trak(t: &OutTrack, chunks: &[(usize, usize, u64)], co64: bool, mvts: u32) -> Vec<u8> {
    let media: u64 = t.samples.iter().map(|s| u64::from(s.dur)).sum();
    let track_dur_movie = if t.edits.is_empty() {
        (media as f64 * f64::from(mvts) / f64::from(t.timescale.max(1))).round() as u64
    } else {
        t.edits.iter().map(|e| e.duration).sum()
    };
    let mut trak = patch_tkhd(&t.tkhd, track_dur_movie);
    if !t.edits.is_empty() {
        trak.extend(build_elst(&t.edits));
    }
    let mut minf = t.minf_head.clone();
    minf.extend(build_stbl(t, chunks, co64));
    let mut mdia = build_mdhd(&t.mdhd, t.timescale, media);
    mdia.extend_from_slice(&t.hdlr);
    mdia.extend(bx(b"minf", &minf));
    mdia.extend_from_slice(&t.mdia_extra);
    trak.extend(bx(b"mdia", &mdia));
    trak.extend_from_slice(&t.trak_extra);
    bx(b"trak", &trak)
}

fn build_mvhd(template: &[u8], timescale: u32, duration: u64, next_id: u32) -> Vec<u8> {
    let mut m = template.to_vec();
    if m.len() >= 40 && m[8] == 1 {
        m[28..32].copy_from_slice(&timescale.to_be_bytes());
        m[32..40].copy_from_slice(&duration.to_be_bytes());
    } else if m.len() >= 28 {
        m[20..24].copy_from_slice(&timescale.to_be_bytes());
        m[24..28].copy_from_slice(&(duration.min(u64::from(u32::MAX)) as u32).to_be_bytes());
    }
    let n = m.len();
    if n >= 4 {
        m[n - 4..].copy_from_slice(&next_id.to_be_bytes());
    }
    m
}

fn build_moov(
    out: &Output,
    tracks: &[OutTrack],
    layout: &[Vec<(usize, usize, u64)>],
    co64: bool,
) -> Vec<u8> {
    let mut movie_dur = 0u64;
    for t in tracks {
        let media: u64 = t.samples.iter().map(|s| u64::from(s.dur)).sum();
        let d = if t.edits.is_empty() {
            (media as f64 * f64::from(out.timescale) / f64::from(t.timescale.max(1))).round() as u64
        } else {
            t.edits.iter().map(|e| e.duration).sum()
        };
        movie_dur = movie_dur.max(d);
    }
    let next_id = tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let mut moov = build_mvhd(&out.mvhd, out.timescale, movie_dur, next_id);
    for (i, t) in tracks.iter().enumerate() {
        moov.extend(build_trak(t, &layout[i], co64, out.timescale));
    }
    moov.extend_from_slice(&out.extra);
    bx(b"moov", &moov)
}

/// Write the file. `cancel` is polled between chunks and `progress` gets 0.0 to 1.0.
pub fn write(
    w: impl Write,
    out: &Output,
    tracks: &[OutTrack],
    sources: &[&Source],
    cancel: &dyn Fn() -> bool,
    progress: &dyn Fn(f32),
) -> io::Result<Written> {
    let chunks = make_chunks(tracks);
    let data_bytes: u64 = chunks.iter().map(|c| c.bytes).sum();
    let co64 = data_bytes + (1 << 20) > u64::from(u32::MAX);
    let mdat_hdr = if data_bytes + 8 > u64::from(u32::MAX) {
        16u64
    } else {
        8
    };
    // Pass 1: the index with placeholder offsets, to learn its size.
    let layout_for = |base: u64| -> Vec<Vec<(usize, usize, u64)>> {
        let mut per: Vec<Vec<(usize, usize, u64)>> = vec![Vec::new(); tracks.len()];
        let mut at = base;
        for c in &chunks {
            per[c.track].push((c.first, c.count, at));
            at += c.bytes;
        }
        per
    };
    let moov0 = build_moov(out, tracks, &layout_for(0), co64);
    let data_start = out.ftyp.len() as u64 + moov0.len() as u64 + mdat_hdr;
    let moov = build_moov(out, tracks, &layout_for(data_start), co64);
    debug_assert_eq!(moov.len(), moov0.len());
    let mut w = BufWriter::with_capacity(1 << 20, w);
    w.write_all(&out.ftyp)?;
    w.write_all(&moov)?;
    if mdat_hdr == 16 {
        w.write_all(&1u32.to_be_bytes())?;
        w.write_all(b"mdat")?;
        w.write_all(&(data_bytes + 16).to_be_bytes())?;
    } else {
        w.write_all(&((data_bytes + 8) as u32).to_be_bytes())?;
        w.write_all(b"mdat")?;
    }
    let mut done = 0u64;
    let mut samples = 0u64;
    let mut buf: Vec<u8> = Vec::new();
    for c in &chunks {
        if cancel() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let t = &tracks[c.track];
        let mut run: Option<(usize, u64, u64)> = None; // (src, offset, len)
        let flush = |run: &mut Option<(usize, u64, u64)>,
                     buf: &mut Vec<u8>,
                     w: &mut BufWriter<_>|
         -> io::Result<()> {
            if let Some((src, off, len)) = run.take() {
                buf.resize(len as usize, 0);
                let mut got = 0usize;
                while got < buf.len() {
                    let n = sources[src].read_at(off + got as u64, &mut buf[got..])?;
                    if n == 0 {
                        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
                    }
                    got += n;
                }
                w.write_all(buf)?;
            }
            Ok(())
        };
        for s in &t.samples[c.first..c.first + c.count] {
            match &mut run {
                Some((src, off, len))
                    if *src == s.src && *off + *len == s.offset && *len < (4 << 20) =>
                {
                    *len += u64::from(s.size);
                }
                _ => {
                    flush(&mut run, &mut buf, &mut w)?;
                    run = Some((s.src, s.offset, u64::from(s.size)));
                }
            }
            samples += 1;
        }
        flush(&mut run, &mut buf, &mut w)?;
        done += c.bytes;
        progress((done as f64 / data_bytes.max(1) as f64) as f32);
    }
    w.flush()?;
    Ok(Written {
        data_bytes: done,
        samples,
        file_bytes: data_start + done,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::boxes::{self, scan};
    use crate::mp4::movie::parse_movie;

    fn audio_track(sizes: &[u32], src: usize, base: u64) -> OutTrack {
        let mut off = base;
        let samples = sizes
            .iter()
            .map(|&s| {
                let o = OutSample {
                    src,
                    offset: off,
                    size: s,
                    dur: 1024,
                    cts_off: 0,
                    sync: true,
                    desc: 1,
                };
                off += u64::from(s);
                o
            })
            .collect();
        let mut tkhd = vec![0u8; 92];
        tkhd[0..4].copy_from_slice(&92u32.to_be_bytes());
        tkhd[4..8].copy_from_slice(b"tkhd");
        tkhd[8..12].copy_from_slice(&[0, 0, 0, 7]);
        tkhd[20..24].copy_from_slice(&1u32.to_be_bytes());
        let mut mdhd = vec![0u8; 32];
        mdhd[0..4].copy_from_slice(&32u32.to_be_bytes());
        mdhd[4..8].copy_from_slice(b"mdhd");
        mdhd[20..24].copy_from_slice(&48000u32.to_be_bytes());
        mdhd[28..30].copy_from_slice(&0x55C4u16.to_be_bytes());
        let mut hdlr = vec![0u8; 33];
        hdlr[0..4].copy_from_slice(&33u32.to_be_bytes());
        hdlr[4..8].copy_from_slice(b"hdlr");
        hdlr[16..20].copy_from_slice(b"soun");
        OutTrack {
            id: 1,
            handler: *b"soun",
            timescale: 48000,
            tkhd,
            mdhd,
            hdlr,
            minf_head: Vec::new(),
            mdia_extra: Vec::new(),
            trak_extra: Vec::new(),
            stsd: vec![bx(b"mp4a", &[0u8; 20])],
            samples,
            edits: Vec::new(),
        }
    }

    #[test]
    fn a_written_file_reads_back_with_the_same_samples_and_bytes() {
        let dir = std::env::temp_dir().join(format!("panoply-mp4write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src_path = dir.join("src.bin");
        let data: Vec<u8> = (0..255u8).cycle().take(600).collect();
        std::fs::write(&src_path, &data).unwrap();
        let src = Source::open(&src_path).unwrap();
        let t = audio_track(&[100, 200, 50, 250], 0, 0);
        let mut mvhd = vec![0u8; 108];
        mvhd[0..4].copy_from_slice(&108u32.to_be_bytes());
        mvhd[4..8].copy_from_slice(b"mvhd");
        let out = Output {
            ftyp: bx(b"ftyp", b"M4A \0\0\0\0M4A isom"),
            timescale: 1000,
            mvhd,
            extra: Vec::new(),
        };
        let path = dir.join("out.m4a");
        let f = std::fs::File::create(&path).unwrap();
        let w = write(
            f,
            &out,
            std::slice::from_ref(&t),
            &[&src],
            &|| false,
            &|_| {},
        )
        .unwrap();
        assert_eq!(w.data_bytes, 600);
        let read = Source::open(&path).unwrap();
        let layout = scan(&read).unwrap();
        let names: Vec<_> = layout.boxes.iter().map(|b| b.name()).collect();
        assert_eq!(names, ["ftyp", "moov", "mdat"], "index first");
        let moov = layout.find(b"moov").unwrap();
        let body = read
            .read_exact_at(moov.payload(), (moov.size - 8) as usize)
            .unwrap();
        let m = parse_movie(&body).unwrap();
        let tr = &m.tracks[0];
        assert_eq!(tr.samples.len(), 4);
        let mut copied = Vec::new();
        for s in &tr.samples {
            copied.extend(read.read_exact_at(s.offset, s.size as usize).unwrap());
        }
        assert_eq!(copied, data, "every sample byte arrives intact");
        assert_eq!(tr.samples[3].dts, 3072);
        let _ = boxes::fourcc_text(b"abcd");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cancel_stops_the_write() {
        let dir = std::env::temp_dir().join(format!("panoply-mp4cancel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.bin");
        std::fs::write(&p, vec![0u8; 100]).unwrap();
        let src = Source::open(&p).unwrap();
        let t = audio_track(&[50, 50], 0, 0);
        let out = Output {
            ftyp: bx(b"ftyp", b"M4A \0\0\0\0"),
            timescale: 1000,
            mvhd: vec![0u8; 108],
            extra: Vec::new(),
        };
        let r = write(Vec::new(), &out, &[t], &[&src], &|| true, &|_| {});
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::Interrupted);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
