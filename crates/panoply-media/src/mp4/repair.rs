//! Diagnosing and rebuilding broken MP4 files (spec 4.18).
//!
//! Three kinds of damage are handled here with Panoply's own code:
//!
//! - The index is fine but the media data is cut off (a partial download, a full disk): keep
//!   every sample that is really in the file, cut all tracks at the same time, write a new index.
//! - The index is missing (the recorder crashed, the file was copied before it finished): scan the
//!   media data for H.264 or HEVC pictures, using the parameter sets of a reference file from the
//!   same device (or those found in the stream itself), and hand the pictures to the helper to be
//!   muxed. Sound cannot be rebuilt this way: AAC frames carry no length, so without the index
//!   there is no way to tell where one ends, and the report says so.
//! - Anything else (fragments, other containers) is remuxed by the helper engine, which resyncs.

use std::io::{self, Write};

use super::boxes::{self, Layout};
use super::movie::{Movie, Track};
use super::poc;
use super::source::Source;
use super::write::{OutSample, OutTrack};

/// What is wrong with the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoovState {
    /// There is no `moov`.
    Missing,
    /// It starts but runs past the end of the file.
    Truncated,
    /// It is all there.
    Complete,
}

/// What the top level of the file says.
#[derive(Debug, Clone)]
pub struct Diagnosis {
    /// File length.
    pub len: u64,
    /// State of the index.
    pub moov: MoovState,
    /// The media data: `(payload start, end, cut off)` of the largest `mdat`.
    pub mdat: Option<(u64, u64, bool)>,
    /// True when the file has fragments.
    pub fragmented: bool,
    /// Where bytes that are not a box begin.
    pub garbage_at: Option<u64>,
    /// True when the file starts with a recognised `ftyp`.
    pub has_ftyp: bool,
    /// The brand.
    pub brand: String,
}

/// Read the top level of a file.
pub fn diagnose(src: &Source) -> io::Result<Diagnosis> {
    let l: Layout = boxes::scan(src)?;
    let moov = match l.find(b"moov") {
        None => MoovState::Missing,
        Some(m) if m.truncated => MoovState::Truncated,
        Some(_) => MoovState::Complete,
    };
    let mdat = l
        .all(b"mdat")
        .max_by_key(|b| b.size)
        .map(|b| (b.payload(), b.end(), b.truncated || b.end() >= src.len()));
    Ok(Diagnosis {
        len: src.len(),
        moov,
        mdat,
        fragmented: l.is_fragmented(),
        garbage_at: l.garbage_at,
        has_ftyp: l.find(b"ftyp").is_some(),
        brand: l.brands.first().cloned().unwrap_or_default(),
    })
}

/// How much of the media data an intact index actually points at.
#[derive(Debug, Clone, Copy)]
pub struct Intact {
    /// Samples the index lists.
    pub total: usize,
    /// Samples that lie fully inside the file.
    pub intact: usize,
    /// Seconds of the longest track that survive.
    pub seconds: f64,
    /// Seconds the index says.
    pub seconds_total: f64,
}

fn intact_count(t: &Track, len: u64) -> usize {
    t.samples
        .iter()
        .position(|s| s.offset + u64::from(s.size) > len)
        .unwrap_or(t.samples.len())
}

/// Count the samples that are really in the file.
pub fn intact(m: &Movie, len: u64) -> Intact {
    let mut total = 0;
    let mut ok = 0;
    let mut secs = 0.0f64;
    let mut secs_total = 0.0f64;
    for t in &m.tracks {
        let n = intact_count(t, len);
        total += t.samples.len();
        ok += n;
        let ts = f64::from(t.timescale.max(1));
        let end = |k: usize| t.samples[..k].iter().map(|s| f64::from(s.dur)).sum::<f64>() / ts;
        secs = secs.max(end(n));
        secs_total = secs_total.max(end(t.samples.len()));
    }
    Intact {
        total,
        intact: ok,
        seconds: secs,
        seconds_total: secs_total,
    }
}

/// The tracks of a movie cut to what is really in the file, all at the same time: the time at
/// which the first track runs out of data. Returns the tracks and the cut time in seconds.
pub fn truncated_tracks(m: &Movie, len: u64) -> (Vec<OutTrack>, f64) {
    let keep = |t: &Track| {
        t.is_video() || t.is_audio() || matches!(&t.handler, b"text" | b"sbtl" | b"subt")
    };
    let ts_of = |t: &Track| f64::from(t.timescale.max(1));
    // The earliest end among tracks that lost samples.
    let mut cut = f64::INFINITY;
    for t in m.tracks.iter().filter(|t| keep(t)) {
        let n = intact_count(t, len);
        if n < t.samples.len() {
            let end: f64 = t.samples[..n].iter().map(|s| f64::from(s.dur)).sum::<f64>() / ts_of(t);
            cut = cut.min(end);
        }
    }
    let mut out = Vec::new();
    for t in m.tracks.iter().filter(|t| keep(t)) {
        let n = intact_count(t, len);
        let mut samples = Vec::new();
        let mut at = 0.0f64;
        for s in &t.samples[..n] {
            // Keep a sample while it starts before the cut (video: and was decoded before it).
            if at >= cut {
                break;
            }
            samples.push(OutSample::from_source(0, s));
            at += f64::from(s.dur) / ts_of(t);
        }
        if samples.is_empty() {
            continue;
        }
        out.push(OutTrack {
            id: t.id,
            handler: t.handler,
            timescale: t.timescale,
            tkhd: t.tkhd.clone(),
            mdhd: t.mdhd.clone(),
            hdlr: t.hdlr.clone(),
            minf_head: t.minf_head.clone(),
            mdia_extra: t.mdia_extra.clone(),
            trak_extra: t.trak_extra.clone(),
            stsd: t.stsd.clone(),
            samples,
            // The old edit list is dropped: its lengths no longer match.
            edits: Vec::new(),
        });
    }
    (out, if cut.is_finite() { cut } else { 0.0 })
}

// ---------------------------------------------------------------------------------------------
// Rebuilding the pictures of a file with no index.

/// The two NAL-based video codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nal {
    /// H.264.
    Avc,
    /// H.265.
    Hevc,
}

/// What the decoder needs before the first picture.
#[derive(Debug, Clone)]
pub struct Params {
    /// The codec.
    pub codec: Nal,
    /// Bytes in the length prefix of every NAL in the media data.
    pub length_size: usize,
    /// Parameter set NAL units (VPS, SPS, PPS), each as a whole NAL including its header.
    pub sets: Vec<Vec<u8>>,
}

/// Read the parameter sets and length size from the `avcC` or `hvcC` of a track's first sample
/// description.
pub fn params_from_track(t: &Track) -> Option<Params> {
    let e = t.stsd.first()?;
    if e.len() <= 86 {
        return None;
    }
    for (typ, s, end) in boxes::children(&e[86..]) {
        let body = &e[86 + s..86 + end];
        match &typ {
            b"avcC" if body.len() > 7 => {
                let length_size = usize::from(body[4] & 3) + 1;
                let mut sets = Vec::new();
                let mut at = 5;
                let n_sps = usize::from(body[at] & 0x1F);
                at += 1;
                for _ in 0..n_sps {
                    let l = usize::from(boxes::be16(body, at));
                    sets.push(body.get(at + 2..at + 2 + l)?.to_vec());
                    at += 2 + l;
                }
                let n_pps = usize::from(*body.get(at)?);
                at += 1;
                for _ in 0..n_pps {
                    let l = usize::from(boxes::be16(body, at));
                    sets.push(body.get(at + 2..at + 2 + l)?.to_vec());
                    at += 2 + l;
                }
                return Some(Params {
                    codec: Nal::Avc,
                    length_size,
                    sets,
                });
            }
            b"hvcC" if body.len() > 23 => {
                let length_size = usize::from(body[21] & 3) + 1;
                let arrays = usize::from(body[22]);
                let mut at = 23;
                let mut sets = Vec::new();
                for _ in 0..arrays {
                    let n = usize::from(boxes::be16(body, at + 1));
                    at += 3;
                    for _ in 0..n {
                        let l = usize::from(boxes::be16(body, at));
                        sets.push(body.get(at + 2..at + 2 + l)?.to_vec());
                        at += 2 + l;
                    }
                }
                return Some(Params {
                    codec: Nal::Hevc,
                    length_size,
                    sets,
                });
            }
            _ => {}
        }
    }
    None
}

/// The largest NAL unit accepted when nothing is known about the stream.
const MAX_NAL: u64 = 16 << 20;

fn be_len(b: &[u8]) -> u64 {
    b.iter().fold(0u64, |a, x| (a << 8) | u64::from(*x))
}

/// A tiny big-endian bit reader for the first few fields of a slice header.
struct Bits<'a> {
    d: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.d.get(self.at / 8)?;
        let b = (byte >> (7 - self.at % 8)) & 1;
        self.at += 1;
        Some(u32::from(b))
    }

    /// Exp-Golomb, unsigned.
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 16 {
                return None;
            }
        }
        let mut v = 0u32;
        for _ in 0..zeros {
            v = (v << 1) | self.bit()?;
        }
        Some((1 << zeros) - 1 + v)
    }
}

/// The first fields of a slice header must look like a real one. Sound data that happens to
/// have the shape of a NAL header almost never also passes this.
fn slice_header_ok(codec: Nal, ty: u8, payload: &[u8]) -> bool {
    let mut b = Bits { d: payload, at: 0 };
    match codec {
        Nal::Avc => {
            let first_mb = b.ue();
            let slice_type = b.ue();
            let pps = b.ue();
            matches!((first_mb, slice_type, pps), (Some(0), Some(t), Some(p)) if t <= 9 && p <= 15)
        }
        Nal::Hevc => {
            if b.bit() != Some(1) {
                return false;
            }
            if (16..=23).contains(&ty) && b.bit().is_none() {
                return false;
            }
            matches!(b.ue(), Some(p) if p <= 63)
        }
    }
}

/// One NAL unit header at `p`: `(length, type, first_in_picture, is_vcl)`, or `None` when the
/// bytes cannot be a NAL of this codec. The first slice of a picture must also have a plausible
/// slice header.
fn nal_at(
    src: &Source,
    p: u64,
    end: u64,
    codec: Nal,
    ls: usize,
    cap: u64,
) -> io::Result<Option<(u64, u8, bool, bool)>> {
    if p + (ls + 3) as u64 > end {
        return Ok(None);
    }
    let h = src.read_up_to(p, ls + 10)?;
    if h.len() < ls + 3 {
        return Ok(None);
    }
    let len = be_len(&h[..ls]);
    if len < 2 || len > cap || p + ls as u64 + len > end {
        return Ok(None);
    }
    let n0 = h[ls];
    if n0 & 0x80 != 0 {
        return Ok(None);
    }
    Ok(match codec {
        Nal::Avc => {
            let ty = n0 & 0x1F;
            if !(1..=12).contains(&ty) {
                return Ok(None);
            }
            // nal_ref_idc: parameter sets and IDR pictures are always referenced; SEI and
            // access unit delimiters never are.
            let idc = (n0 >> 5) & 3;
            if matches!(ty, 5 | 7 | 8) && idc == 0 || matches!(ty, 6 | 9) && idc != 0 {
                return Ok(None);
            }
            let vcl = (1..=5).contains(&ty);
            let first = vcl && h[ls + 1] & 0x80 != 0;
            if first && !slice_header_ok(codec, ty, &h[ls + 1..]) {
                return Ok(None);
            }
            Some((len, ty, first, vcl))
        }
        Nal::Hevc => {
            let ty = (n0 >> 1) & 0x3F;
            let h1 = h[ls + 1];
            let layer = ((n0 & 1) << 5) | (h1 >> 3);
            let tid = h1 & 7;
            if layer != 0 || tid == 0 {
                return Ok(None);
            }
            let vcl = ty <= 31;
            let ok = vcl && (ty <= 9 || (16..=21).contains(&ty)) || (32..=40).contains(&ty);
            if !ok {
                return Ok(None);
            }
            let first = vcl && h[ls + 2] & 0x80 != 0;
            if first && !slice_header_ok(codec, ty, &h[ls + 2..]) {
                return Ok(None);
            }
            Some((len, ty, first, vcl))
        }
    })
}

fn starts_new_au(codec: Nal, ty: u8, vcl: bool, first: bool) -> bool {
    match codec {
        Nal::Avc => matches!(ty, 6..=9) || (vcl && first),
        Nal::Hevc => matches!(ty, 32..=35 | 39) || (vcl && first),
    }
}

/// One picture (access unit) at `pos`: its length in bytes, or `None` when what is there is not
/// a complete picture.
fn au_at(
    src: &Source,
    pos: u64,
    end: u64,
    codec: Nal,
    ls: usize,
    cap: u64,
) -> io::Result<Option<u64>> {
    let mut p = pos;
    let mut seen_vcl = false;
    loop {
        if p >= end {
            return Ok(seen_vcl.then_some(p - pos));
        }
        let Some((len, ty, first, vcl)) = nal_at(src, p, end, codec, ls, cap)? else {
            return Ok(seen_vcl.then_some(p - pos));
        };
        if seen_vcl && starts_new_au(codec, ty, vcl, first) {
            return Ok(Some(p - pos));
        }
        if !seen_vcl && !vcl && p != pos && starts_new_au(codec, ty, vcl, first) {
            // A parameter set after parameter sets: still the same unit.
        }
        seen_vcl |= vcl;
        p += ls as u64 + len;
    }
}

/// The longest stretch of sound (or other data) that may sit between two pictures when nothing
/// is known about the file's layout.
pub const DEFAULT_MAX_GAP: u64 = 128 << 10;

/// The first position in `from..limit` where a picture starts.
fn next_picture(
    src: &Source,
    from: u64,
    limit: u64,
    to: u64,
    codec: Nal,
    ls: usize,
    cap: u64,
) -> io::Result<Option<u64>> {
    let mut p = from;
    let limit = limit.min(to);
    let mut buf: Vec<u8> = Vec::new();
    let mut buf_at = 0u64;
    while p < limit {
        if buf.is_empty() || p < buf_at || p + 8 > buf_at + buf.len() as u64 {
            buf_at = p;
            buf = src.read_up_to(p, 1 << 16)?;
            if buf.len() < 8 {
                return Ok(None);
            }
        }
        let i = (p - buf_at) as usize;
        // Cheap filter before the real check: a length that fits and a NAL header.
        let len = be_len(&buf[i..i + ls]);
        let n0 = buf[i + ls];
        let plausible = len >= 2
            && len <= cap
            && n0 & 0x80 == 0
            && match codec {
                Nal::Avc => (1..=12).contains(&(n0 & 0x1F)),
                Nal::Hevc => {
                    let t = (n0 >> 1) & 0x3F;
                    t <= 9 || (16..=21).contains(&t) || (32..=40).contains(&t)
                }
            };
        if plausible && au_at(src, p, to, codec, ls, cap)?.is_some_and(|l| l > 0) {
            return Ok(Some(p));
        }
        p += 1;
    }
    Ok(None)
}

/// What the scanner knows about the stream while it looks for the next picture.
#[derive(Clone, Copy)]
struct Shape {
    codec: Nal,
    ls: usize,
    cap: u64,
    max_gap: u64,
}

/// Like [`next_picture`], but when the numbering is known a picture must also carry a
/// `frame_num` that can follow the state: sound data that looks like a picture rarely does.
fn next_numbered(
    src: &Source,
    from: u64,
    limit: u64,
    to: u64,
    shape: Shape,
    numbering: Option<(poc::FrameNums, FrameState)>,
) -> io::Result<Option<u64>> {
    let Shape { codec, ls, cap, .. } = shape;
    let mut from = from;
    while from < limit {
        let Some(p) = next_picture(src, from, limit, to, codec, ls, cap)? else {
            return Ok(None);
        };
        let ok = match numbering {
            None => true,
            Some((rule, st)) => {
                let l = au_at(src, p, to, codec, ls, cap)?.unwrap_or(0);
                matches!(picture_frame_num(src, p, p + l, ls, rule)?,
                    Some((ty, _, f)) if st.allows(rule, ty, f))
            }
        };
        if ok {
            return Ok(Some(p));
        }
        from = p + 1;
    }
    Ok(None)
}

/// True when pictures resume at `pos`: a picture parses there, and after it (and after the
/// stretch of sound that may follow it) another picture is found, three times over.
fn chain_ok(
    src: &Source,
    pos: u64,
    end: u64,
    shape: Shape,
    hops: usize,
    mut numbering: Option<(poc::FrameNums, FrameState)>,
) -> io::Result<bool> {
    let Shape {
        codec,
        ls,
        cap,
        max_gap,
    } = shape;
    let mut cur = pos;
    for hop in 0..hops {
        let Some(l) = au_at(src, cur, end, codec, ls, cap)?.filter(|l| *l > 0) else {
            return Ok(false);
        };
        if let Some((rule, st)) = numbering {
            match picture_frame_num(src, cur, cur + l, ls, rule)? {
                Some((ty, idc, f)) if st.allows(rule, ty, f) => {
                    numbering = Some((rule, st.after(ty, idc, f)));
                }
                _ => return Ok(false),
            }
        }
        let after = cur + l;
        if after >= end {
            return Ok(true);
        }
        match next_numbered(src, after, after + max_gap, end, shape, numbering)? {
            Some(n) => cur = n,
            // Nothing follows: fine at the very end of the data, suspicious earlier.
            None => return Ok(hop >= 1 && end - after < max_gap),
        }
    }
    Ok(true)
}

/// What a reference file says about the layout of the data to be scanned.
#[derive(Debug, Clone, Copy, Default)]
pub struct Hints {
    /// The largest picture in the reference.
    pub largest_picture: Option<u64>,
    /// The longest stretch of other data between two pictures in the reference, with margin.
    pub max_gap: Option<u64>,
    /// The `frame_num` rule of an H.264 stream. Pictures that resume after a stretch of other data
    /// must carry the next `frame_num`, which sound data that looks like a picture almost never
    /// does.
    pub frame_nums: Option<poc::FrameNums>,
}

impl Hints {
    /// Hints that come from the parameter sets alone (no reference file).
    pub fn from_params(p: &Params) -> Hints {
        Hints {
            frame_nums: poc::avc_frame_nums(p),
            ..Hints::default()
        }
    }
}

/// Read the layout hints from the video track of a reference file.
pub fn hints_from_track(t: &Track) -> Hints {
    let largest = t.samples.iter().map(|s| u64::from(s.size)).max();
    let mut v: Vec<(u64, u64)> = t
        .samples
        .iter()
        .map(|s| (s.offset, u64::from(s.size)))
        .collect();
    v.sort_unstable();
    let gap = v
        .windows(2)
        .map(|w| w[1].0.saturating_sub(w[0].0 + w[0].1))
        .max();
    Hints {
        largest_picture: largest,
        // Four times the longest stretch of sound the reference had between two pictures, and
        // never less than 16 KB: the broken file came from the same recorder.
        max_gap: gap.map(|g| (g * 4).max(16 << 10)),
        frame_nums: params_from_track(t).and_then(|p| poc::avc_frame_nums(&p)),
    }
}

/// The `(nal type, nal_ref_idc, frame_num)` of the first slice of the picture at `pos`.
fn picture_frame_num(
    src: &Source,
    pos: u64,
    end: u64,
    ls: usize,
    rule: poc::FrameNums,
) -> io::Result<Option<(u8, u8, u32)>> {
    let mut p = pos;
    for _ in 0..16 {
        if p + (ls + 2) as u64 > end {
            return Ok(None);
        }
        let h = src.read_up_to(p, ls + 24)?;
        if h.len() < ls + 2 {
            return Ok(None);
        }
        let n = be_len(&h[..ls]);
        let n0 = h[ls];
        let ty = n0 & 0x1F;
        if (1..=5).contains(&ty) {
            let nal = &h[ls..];
            return Ok(poc::avc_frame_num(rule, nal).map(|f| (ty, (n0 >> 5) & 3, f)));
        }
        p += ls as u64 + n;
    }
    Ok(None)
}

/// Where `frame_num` stands after the pictures found so far.
#[derive(Clone, Copy)]
struct FrameState {
    prev: u32,
    prev_ref: u32,
}

impl FrameState {
    fn after(self, ty: u8, idc: u8, f: u32) -> FrameState {
        FrameState {
            prev: f,
            prev_ref: if ty == 5 || idc != 0 {
                f
            } else {
                self.prev_ref
            },
        }
    }

    /// Can a picture with this `frame_num` follow?
    fn allows(self, rule: poc::FrameNums, ty: u8, f: u32) -> bool {
        if ty == 5 {
            return f == 0;
        }
        f == (self.prev_ref + 1) % (1 << rule.bits) || f == self.prev
    }
}

/// The pictures found in the media data.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    /// `(offset, length)` of each picture, in file order.
    pub pictures: Vec<(u64, u64)>,
    /// Bytes inside pictures.
    pub picture_bytes: u64,
    /// Bytes between pictures (sound, in a file that had some).
    pub other_bytes: u64,
    /// Where the recoverable data ended.
    pub stopped_at: u64,
}

/// Walk the media data from `from` to `to`, finding every picture and skipping what lies
/// between them. Stops at the first stretch from which no picture can be found again.
pub fn scan_pictures(
    src: &Source,
    from: u64,
    to: u64,
    codec: Nal,
    ls: usize,
    hints: Hints,
    cancel: &dyn Fn() -> bool,
) -> io::Result<Scan> {
    let mut out = Scan::default();
    let mut pos = from;
    // A NAL unit larger than three times the largest picture seen (or known from a reference
    // file) is sound data that happens to look like a length, not a picture.
    let mut biggest = hints.largest_picture.unwrap_or(0);
    let max_gap = hints.max_gap.unwrap_or(DEFAULT_MAX_GAP);
    let cap_for = |biggest: u64| -> u64 {
        if biggest == 0 {
            MAX_NAL
        } else {
            (biggest * 3).clamp(32 << 10, MAX_NAL)
        }
    };
    let mut state: Option<FrameState> = None;
    let note = |state: &mut Option<FrameState>, p: u64, l: u64| -> io::Result<()> {
        if let Some(rule) = hints.frame_nums
            && let Some((ty, idc, f)) = picture_frame_num(src, p, p + l, ls, rule)?
        {
            let base = state.unwrap_or(FrameState {
                prev: f,
                prev_ref: f,
            });
            *state = Some(base.after(ty, idc, f));
        }
        Ok(())
    };
    while pos < to {
        if cancel() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let cap = cap_for(biggest);
        if let Some(l) = au_at(src, pos, to, codec, ls, cap)?
            && l > 0
        {
            out.pictures.push((pos, l));
            out.picture_bytes += l;
            biggest = biggest.max(l);
            note(&mut state, pos, l)?;
            pos += l;
            continue;
        }
        // Not a picture: sound or damage. Look for the next place where pictures resume. A
        // candidate must carry the next frame_num when that is known; if nothing passes, the
        // search is repeated without that check (a stream that reset its numbering).
        let shape = Shape {
            codec,
            ls,
            cap,
            max_gap,
        };
        let mut found = None;
        for strict in [true, false] {
            if !strict && (state.is_none() || hints.frame_nums.is_none()) {
                break;
            }
            let mut from = pos + 1;
            let limit = (pos + max_gap).min(to);
            while from < limit {
                match next_picture(src, from, limit, to, codec, ls, cap)? {
                    Some(p) => {
                        let numbering = match (strict, hints.frame_nums, state) {
                            (true, Some(rule), Some(st)) => Some((rule, st)),
                            _ => None,
                        };
                        if chain_ok(src, p, to, shape, 4, numbering)? {
                            found = Some(p);
                            break;
                        }
                        from = p + 1;
                    }
                    None => break,
                }
            }
            if found.is_some() {
                break;
            }
        }
        match found {
            Some(p) => {
                out.other_bytes += p - pos;
                pos = p;
            }
            None => break,
        }
    }
    out.stopped_at = pos;
    Ok(out)
}

/// Write the pictures as an Annex B stream: parameter sets first, then each picture with its
/// length prefixes turned into start codes.
pub fn write_annexb(
    src: &Source,
    scan: &Scan,
    params: &Params,
    mut w: impl Write,
    cancel: &dyn Fn() -> bool,
) -> io::Result<()> {
    const START: [u8; 4] = [0, 0, 0, 1];
    for s in &params.sets {
        w.write_all(&START)?;
        w.write_all(s)?;
    }
    let ls = params.length_size;
    for (i, (off, len)) in scan.pictures.iter().enumerate() {
        if i % 64 == 0 && cancel() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let data = src.read_exact_at(*off, *len as usize)?;
        let mut p = 0usize;
        while p + ls <= data.len() {
            let n = be_len(&data[p..p + ls]) as usize;
            p += ls;
            if p + n > data.len() {
                break;
            }
            w.write_all(&START)?;
            w.write_all(&data[p..p + n])?;
            p += n;
        }
    }
    w.flush()
}

/// A rebuilt picture track.
pub struct Rebuilt {
    /// The track, ready for [`super::write::write`].
    pub track: OutTrack,
    /// False when the picture order could not be read and decode order was assumed.
    pub ordered: bool,
    /// Pictures in the track.
    pub pictures: usize,
}

fn first_slice(src: &Source, off: u64, len: u64, p: &Params) -> io::Result<Option<Vec<u8>>> {
    let ls = p.length_size as u64;
    let mut at = 0u64;
    for _ in 0..16 {
        if at + ls + 2 > len {
            return Ok(None);
        }
        let h = src.read_exact_at(off + at, p.length_size + 2)?;
        let n = be_len(&h[..p.length_size]);
        let first = h[p.length_size];
        let vcl = match p.codec {
            Nal::Avc => (1..=5).contains(&(first & 0x1F)),
            Nal::Hevc => ((first >> 1) & 0x3F) <= 31,
        };
        if vcl {
            let take = n.min(64) as usize;
            return Ok(Some(src.read_exact_at(off + at + ls, take)?));
        }
        at += ls + n;
    }
    Ok(None)
}

/// Build the track of a file from its pictures and a reference track of the same device: the
/// reference supplies the sample description, the size and rotation, the time scale and the
/// frame duration; the pictures supply the data and, through their picture order counts, the
/// order they are shown in.
pub fn rebuild_track(
    src: &Source,
    scan: &Scan,
    params: &Params,
    reference: &Track,
    movie_ts: u32,
) -> io::Result<Rebuilt> {
    use super::cut::{Rel, place_pieces, start_edit};
    use super::poc::{Pic, PocReader, display_ranks};
    let n = scan.pictures.len();
    // The most common frame duration of the reference.
    let mut counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for s in &reference.samples {
        *counts.entry(s.dur).or_default() += 1;
    }
    let dur = counts
        .iter()
        .max_by_key(|(d, c)| (**c, **d))
        .map(|(d, _)| *d)
        .filter(|d| *d > 0)
        .unwrap_or_else(|| (reference.timescale / 30).max(1));
    let decode_order: Vec<Pic> = (0..n)
        .map(|i| Pic {
            clean_start: i == 0,
            sync: i == 0,
            poc: i as i64,
        })
        .collect();
    let (pics, ordered) = match PocReader::new(params) {
        None => (decode_order, false),
        // POC type 2: the standard says pictures are shown in the order they are stored.
        Some(r) if r.is_decode_order() => (decode_order, true),
        Some(mut r) => {
            let mut v: Vec<Pic> = Vec::with_capacity(n);
            for (off, len) in &scan.pictures {
                match first_slice(src, *off, *len, params)?.and_then(|nal| r.picture(&nal)) {
                    Some(p) => v.push(p),
                    None => break,
                }
            }
            if v.len() == n {
                (v, true)
            } else {
                (decode_order, false)
            }
        }
    };
    let ranks = display_ranks(&pics);
    let rel: Vec<Rel> = (0..n)
        .map(|i| Rel {
            dts: i as i64 * i64::from(dur),
            cts: i64::from(ranks[i]) * i64::from(dur),
            dur,
        })
        .collect();
    let (placed, shift) = place_pieces(&[rel]);
    let samples: Vec<OutSample> = scan
        .pictures
        .iter()
        .enumerate()
        .map(|(i, (off, len))| OutSample {
            src: 0,
            offset: *off,
            size: *len as u32,
            dur,
            cts_off: (placed[0][i].cts + shift - placed[0][i].dts).clamp(0, i64::from(i32::MAX))
                as i32,
            sync: pics[i].sync || i == 0,
            desc: 1,
        })
        .collect();
    let total = n as u64 * u64::from(dur);
    let edits = if shift > 0 {
        vec![start_edit(total, reference.timescale, movie_ts, shift)]
    } else {
        Vec::new()
    };
    Ok(Rebuilt {
        track: OutTrack {
            id: reference.id,
            handler: reference.handler,
            timescale: reference.timescale,
            tkhd: reference.tkhd.clone(),
            mdhd: reference.mdhd.clone(),
            hdlr: reference.hdlr.clone(),
            minf_head: reference.minf_head.clone(),
            mdia_extra: reference.mdia_extra.clone(),
            trak_extra: Vec::new(),
            stsd: reference.stsd.iter().take(1).cloned().collect(),
            samples,
            edits,
        },
        ordered,
        pictures: n,
    })
}

/// Find parameter sets in the stream itself (an SPS and a PPS for H.264) for a file that has no
/// reference. Returns `None` when the stream carries none, as MP4 files usually do not.
pub fn params_from_stream(
    src: &Source,
    from: u64,
    to: u64,
    codec: Nal,
    ls: usize,
) -> io::Result<Option<Params>> {
    if codec != Nal::Avc {
        return Ok(None);
    }
    let mut sps = None;
    let mut pps = None;
    let mut pos = from;
    let end = to.min(from + (64 << 20));
    let mut seen = 0;
    while pos < end && seen < 20_000 {
        let Some((len, ty, _, _)) = nal_at(src, pos, to, codec, ls, MAX_NAL)? else {
            pos += 1;
            continue;
        };
        if ty == 7 && sps.is_none() {
            sps = Some(src.read_exact_at(pos + ls as u64, len as usize)?);
        } else if ty == 8 && pps.is_none() {
            pps = Some(src.read_exact_at(pos + ls as u64, len as usize)?);
        }
        if sps.is_some() && pps.is_some() {
            break;
        }
        pos += ls as u64 + len;
        seen += 1;
    }
    Ok(match (sps, pps) {
        (Some(s), Some(p)) => Some(Params {
            codec,
            length_size: ls,
            sets: vec![s, p],
        }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nal(ty: u8, first: bool, body: usize) -> Vec<u8> {
        // H.264: a 4-byte length, a header byte, a payload whose first bit says "first slice".
        let mut v = Vec::new();
        v.extend_from_slice(&((body + 2) as u32).to_be_bytes());
        v.push(0x60 | ty);
        // first_mb_in_slice = 0, slice_type = 7, pps id = 0 for the first slice of a picture;
        // first_mb_in_slice = 5 otherwise.
        v.extend_from_slice(if first { &[0x88, 0x80] } else { &[0x30, 0x80] });
        v.extend(std::iter::repeat_n(0x11u8, body.saturating_sub(1)));
        v
    }

    fn file(bytes: &[u8]) -> (std::path::PathBuf, Source) {
        let p = std::env::temp_dir().join(format!(
            "panoply-repair-{}-{}",
            std::process::id(),
            bytes.len()
        ));
        std::fs::write(&p, bytes).unwrap();
        let s = Source::open(&p).unwrap();
        (p, s)
    }

    #[test]
    fn pictures_are_found_between_stretches_of_sound() {
        let mut data = Vec::new();
        data.extend(nal(5, true, 100)); // IDR picture
        data.extend(nal(1, true, 40)); // P picture
        // Sixty bytes that are not a picture (sound): an invalid NAL header.
        data.extend(std::iter::repeat_n(0xFFu8, 60));
        data.extend(nal(1, true, 30));
        data.extend(nal(1, true, 30));
        data.extend(nal(1, true, 30));
        data.extend(nal(1, true, 30));
        let (p, s) = file(&data);
        let r = scan_pictures(&s, 0, s.len(), Nal::Avc, 4, Hints::default(), &|| false).unwrap();
        assert_eq!(r.pictures.len(), 6, "{:?}", r.pictures);
        assert_eq!(r.other_bytes, 60);
        assert_eq!(r.pictures[0], (0, 106));
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn a_picture_with_two_slices_is_one_picture() {
        let mut data = Vec::new();
        data.extend(nal(1, true, 50));
        data.extend(nal(1, false, 50)); // second slice of the same picture
        data.extend(nal(1, true, 50));
        data.extend(nal(1, true, 50));
        let (p, s) = file(&data);
        let r = scan_pictures(&s, 0, s.len(), Nal::Avc, 4, Hints::default(), &|| false).unwrap();
        assert_eq!(r.pictures.len(), 3);
        assert_eq!(r.pictures[0].1, 2 * 56);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn a_cut_off_last_picture_is_not_included() {
        let mut data = Vec::new();
        data.extend(nal(1, true, 50));
        data.extend(nal(1, true, 50));
        data.extend(nal(1, true, 50));
        let cut = data.len() - 20;
        data.truncate(cut);
        let (p, s) = file(&data);
        let r = scan_pictures(&s, 0, s.len(), Nal::Avc, 4, Hints::default(), &|| false).unwrap();
        assert_eq!(r.pictures.len(), 2);
        std::fs::remove_file(p).unwrap();
    }

    #[test]
    fn annex_b_output_has_start_codes_and_the_parameter_sets_first() {
        let mut data = Vec::new();
        data.extend(nal(5, true, 10));
        let (p, s) = file(&data);
        let r = scan_pictures(&s, 0, s.len(), Nal::Avc, 4, Hints::default(), &|| false).unwrap();
        let params = Params {
            codec: Nal::Avc,
            length_size: 4,
            sets: vec![vec![0x67, 1, 2], vec![0x68, 3]],
        };
        let mut out = Vec::new();
        write_annexb(&s, &r, &params, &mut out, &|| false).unwrap();
        assert_eq!(&out[..8], &[0, 0, 0, 1, 0x67, 1, 2, 0]);
        assert!(out.contains(&0x65));
        std::fs::remove_file(p).unwrap();
    }
}
