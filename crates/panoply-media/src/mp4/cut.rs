//! Planning a lossless cut of an MP4 from its sample tables (spec 4.1, SMART).
//!
//! Whole groups of pictures are copied bit for bit. Only the frames at the edges that cannot
//! start or end on a keyframe are re-encoded, and the pieces are spliced in one index. This
//! file is the pure part: it decides which samples are copied, which presentation intervals
//! must be re-encoded and where each piece sits on the new timeline. Running the helper and
//! writing the file is `ops::trim`.

use super::movie::{Edit, Sample, Track};
use crate::error::{MediaError, Result};

/// How presentation time follows from the stored times of one track.
///
/// `presentation(sample) = cts(sample) - media_time + lead`, in track time units, where `lead`
/// is an empty edit at the start (a track that begins late) and `media_time` is the start of the
/// one normal edit (which hides the picture delay of B-frames and audio priming).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeline {
    /// Track time units per second.
    pub ts: u32,
    /// Ticks of empty time before the first edit.
    pub lead: i64,
    /// Start of the normal edit in the media.
    pub media_time: i64,
}

impl Timeline {
    /// Presentation time of a sample in track ticks.
    pub fn pres(&self, s: &Sample) -> i64 {
        s.cts() - self.media_time + self.lead
    }

    /// Seconds for ticks.
    pub fn secs(&self, ticks: i64) -> f64 {
        ticks as f64 / f64::from(self.ts.max(1))
    }

    /// Ticks for seconds.
    pub fn ticks(&self, secs: f64) -> i64 {
        (secs * f64::from(self.ts.max(1))).round() as i64
    }
}

/// Read the edit list of a track. Panoply's own cut understands no edit list, or one normal
/// edit at normal speed with an optional empty edit before it. Anything else is left to the
/// helper engine.
pub fn timeline_of(t: &Track, movie_ts: u32) -> Result<Timeline> {
    let mut lead = 0i64;
    let mut media_time = 0i64;
    let mut normal = 0;
    for e in &t.edits {
        if e.media_time < 0 {
            if normal > 0 {
                return Err(complex());
            }
            lead += (e.duration as f64 * f64::from(t.timescale) / f64::from(movie_ts.max(1)))
                .round() as i64;
        } else {
            normal += 1;
            if normal > 1 || e.rate != 0x10000 {
                return Err(complex());
            }
            media_time = e.media_time;
        }
    }
    Ok(Timeline {
        ts: t.timescale,
        lead,
        media_time,
    })
}

fn complex() -> MediaError {
    MediaError::Unsupported(
        "This file has an edit list that Panoply's own MP4 code does not cut.".into(),
    )
}

/// One piece of the picture track of the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Copy these samples (decode indices `first..end`), skipping the leading pictures listed.
    Copy {
        /// First decode index.
        first: usize,
        /// One past the last decode index.
        end: usize,
        /// Decode indices not copied because they need frames before the cut.
        skip: Vec<usize>,
    },
    /// Re-encode the source frames whose presentation time is in `start..end` (source ticks).
    Encode {
        /// Presentation start, inclusive.
        start: i64,
        /// Presentation end, exclusive.
        end: i64,
    },
}

/// The picture side of one cut.
#[derive(Debug, Clone)]
pub struct VideoCut {
    /// The pieces in presentation order.
    pub pieces: Vec<Piece>,
    /// Presentation start of the first frame kept (source ticks).
    pub start: i64,
    /// Presentation end of the last frame kept (source ticks).
    pub end: i64,
    /// Frames copied without touching them.
    pub frames_copied: usize,
    /// Frames that are re-encoded.
    pub frames_encoded: usize,
}

/// Decode indices of the samples in presentation order.
pub fn presentation_order(t: &Track, tl: &Timeline) -> Vec<usize> {
    let mut v: Vec<usize> = (0..t.samples.len()).collect();
    v.sort_by_key(|&i| (tl.pres(&t.samples[i]), i));
    v
}

/// Plan the picture side of keeping `start..end` (ticks, `end` exclusive, `None` to the end).
/// With `exact` the edges are re-encoded as needed; without it the cut snaps to keyframes.
pub fn plan_video(
    t: &Track,
    tl: &Timeline,
    start: i64,
    end: Option<i64>,
    exact: bool,
) -> Result<VideoCut> {
    let s = &t.samples;
    if s.is_empty() {
        return Err(MediaError::Corrupt("The video track has no frames.".into()));
    }
    let order = presentation_order(t, tl);
    let pres = |i: usize| tl.pres(&s[i]);
    let ks = order
        .iter()
        .position(|&i| pres(i) >= start)
        .ok_or_else(|| {
            MediaError::BadOption(
                "The start of the range is after the last frame of the video".into(),
            )
        })?;
    let ke = match end {
        None => order.len() - 1,
        Some(e) => order.iter().rposition(|&i| pres(i) < e).ok_or_else(|| {
            MediaError::BadOption("The range ends before the first frame of the video".into())
        })?,
    };
    if ke < ks {
        return Err(MediaError::BadOption(
            "The range is shorter than one frame, so nothing would be kept".into(),
        ));
    }
    let (f_s, f_e) = (order[ks], order[ke]);
    let first_pres = pres(f_s);
    let last_end = pres(f_e) + i64::from(s[f_e].dur);
    let syncs: Vec<usize> = (0..s.len()).filter(|&i| s[i].sync).collect();
    if syncs.is_empty() {
        return Err(MediaError::Corrupt(
            "The video has no keyframes to cut at.".into(),
        ));
    }
    if !exact {
        return plan_keyframes(t, tl, &order, &syncs, ks, ke);
    }
    // The first keyframe at or after the first wanted frame, in presentation time.
    let a = syncs
        .iter()
        .copied()
        .filter(|&i| pres(i) >= first_pres && pres(i) <= pres(f_e))
        .min_by_key(|&i| pres(i));
    let Some(a) = a else {
        return Ok(whole_encode(first_pres, last_end, ke - ks + 1));
    };
    // Extend across whole groups of pictures that end inside the range.
    let key_pres = pres(a);
    let mut mid_end = a;
    let mut idx = syncs.iter().position(|&x| x == a).unwrap_or(0);
    let first_boundary = syncs.get(idx + 1).copied().unwrap_or(s.len());
    loop {
        let boundary = syncs.get(idx + 1).copied().unwrap_or(s.len());
        // Every frame of this group must be inside the range. The first keyframe's leading
        // pictures are not copied (they need earlier frames), so they do not count here.
        let inside = (mid_end..boundary).all(|i| {
            let leading_of_first = i != a && i < first_boundary && pres(i) < key_pres;
            leading_of_first || (pres(i) + i64::from(s[i].dur) <= last_end && pres(i) >= first_pres)
        });
        if !inside {
            break;
        }
        mid_end = boundary;
        idx += 1;
        if boundary >= s.len() {
            break;
        }
    }
    if mid_end == a {
        return Ok(whole_encode(first_pres, last_end, ke - ks + 1));
    }
    // Leading pictures of the first copied keyframe cannot be decoded without earlier frames.
    let skip: Vec<usize> = (a + 1..mid_end).filter(|&i| pres(i) < key_pres).collect();
    let copied: Vec<usize> = (a..mid_end).filter(|i| !skip.contains(i)).collect();
    if copied.len() < 2 {
        return Ok(whole_encode(first_pres, last_end, ke - ks + 1));
    }
    let mid_start = copied.iter().map(|&i| pres(i)).min().unwrap_or(key_pres);
    let mid_stop = copied
        .iter()
        .map(|&i| pres(i) + i64::from(s[i].dur))
        .max()
        .unwrap_or(key_pres);
    let mut pieces = Vec::new();
    let mut encoded = 0usize;
    if first_pres < mid_start {
        pieces.push(Piece::Encode {
            start: first_pres,
            end: mid_start,
        });
        encoded += order
            .iter()
            .filter(|&&i| pres(i) >= first_pres && pres(i) < mid_start)
            .count();
    }
    pieces.push(Piece::Copy {
        first: a,
        end: mid_end,
        skip,
    });
    if last_end > mid_stop {
        pieces.push(Piece::Encode {
            start: mid_stop,
            end: last_end,
        });
        encoded += order
            .iter()
            .filter(|&&i| pres(i) >= mid_stop && pres(i) < last_end)
            .count();
    }
    Ok(VideoCut {
        pieces,
        start: first_pres.min(mid_start),
        end: last_end.max(mid_stop),
        frames_copied: copied.len(),
        frames_encoded: encoded,
    })
}

fn whole_encode(start: i64, end: i64, frames: usize) -> VideoCut {
    VideoCut {
        pieces: vec![Piece::Encode { start, end }],
        start,
        end,
        frames_copied: 0,
        frames_encoded: frames,
    }
}

fn plan_keyframes(
    t: &Track,
    tl: &Timeline,
    order: &[usize],
    syncs: &[usize],
    ks: usize,
    ke: usize,
) -> Result<VideoCut> {
    let s = &t.samples;
    let pres = |i: usize| tl.pres(&s[i]);
    let f_s = order[ks];
    // The last keyframe at or before the first wanted frame (never lose what was asked for).
    let a = syncs
        .iter()
        .copied()
        .filter(|&i| pres(i) <= pres(f_s))
        .max_by_key(|&i| pres(i))
        .unwrap_or(syncs[0]);
    // Every sample decoded up to the last wanted frame: a decode-order prefix is self-contained.
    let last_decode = order[..=ke].iter().copied().max().unwrap_or(a);
    let end = last_decode + 1;
    let key_pres = pres(a);
    let skip: Vec<usize> = (a + 1..end).filter(|&i| pres(i) < key_pres).collect();
    let copied: Vec<usize> = (a..end).filter(|i| !skip.contains(i)).collect();
    let start = copied.iter().map(|&i| pres(i)).min().unwrap_or(key_pres);
    let stop = copied
        .iter()
        .map(|&i| pres(i) + i64::from(s[i].dur))
        .max()
        .unwrap_or(key_pres);
    Ok(VideoCut {
        pieces: vec![Piece::Copy {
            first: a,
            end,
            skip,
        }],
        start,
        end: stop,
        frames_copied: copied.len(),
        frames_encoded: 0,
    })
}

/// The window of a track that is not reordered (sound, subtitles) that covers presentation
/// time `start..end` (ticks of this track).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// First sample.
    pub first: usize,
    /// One past the last sample.
    pub end: usize,
    /// Ticks of the first sample to skip so the track starts exactly at `start`. Negative
    /// means the first sample begins after `start` by that many ticks (an empty lead).
    pub offset: i64,
}

/// Select the samples that overlap `start..end`.
pub fn plan_window(t: &Track, tl: &Timeline, start: i64, end: i64) -> Option<Window> {
    let s = &t.samples;
    let first = s
        .iter()
        .position(|x| tl.pres(x) + i64::from(x.dur) > start)?;
    if tl.pres(&s[first]) >= end {
        return None;
    }
    let last = s.iter().rposition(|x| tl.pres(x) < end)?;
    if last < first {
        return None;
    }
    Some(Window {
        first,
        end: last + 1,
        offset: start - tl.pres(&s[first]),
    })
}

/// One sample as placed on the new timeline of the picture track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// Decode time on the new timeline.
    pub dts: i64,
    /// Presentation time on the new timeline (before the start offset).
    pub cts: i64,
    /// Duration.
    pub dur: u32,
}

/// A piece of the picture track with its own relative times, ready to be placed.
#[derive(Debug, Clone)]
pub struct Rel {
    /// Decode time relative to the first sample of the piece.
    pub dts: i64,
    /// Presentation time relative to the first presented frame of the piece (at least 0).
    pub cts: i64,
    /// Duration.
    pub dur: u32,
}

/// Place pieces one after another. Decode time continues where the last piece stopped and
/// presentation time continues where the last piece stopped being shown, so the splice is
/// seamless even when the pieces have different picture delays. Returns the placed samples per
/// piece and the shift that must be applied (as an edit list start) so that no composition
/// offset is negative.
pub fn place_pieces(pieces: &[Vec<Rel>]) -> (Vec<Vec<Placed>>, i64) {
    let mut d = 0i64;
    let mut c = 0i64;
    let mut out = Vec::new();
    let mut min_off = i64::MAX;
    for p in pieces {
        let mut placed = Vec::with_capacity(p.len());
        let mut span = 0i64;
        let mut dec = 0i64;
        for r in p {
            let x = Placed {
                dts: d + r.dts,
                cts: c + r.cts,
                dur: r.dur,
            };
            min_off = min_off.min(x.cts - x.dts);
            span = span.max(r.cts + i64::from(r.dur));
            dec = dec.max(r.dts + i64::from(r.dur));
            placed.push(x);
        }
        d += dec;
        c += span;
        out.push(placed);
    }
    let shift = if min_off < 0 { -min_off } else { 0 };
    (out, shift)
}

/// The edit list that starts the track at the first presented frame.
pub fn start_edit(total_ticks: u64, track_ts: u32, movie_ts: u32, media_time: i64) -> Edit {
    Edit {
        duration: (total_ticks as f64 * f64::from(movie_ts) / f64::from(track_ts.max(1))).round()
            as u64,
        media_time,
        rate: 0x10000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed-GOP stream with two B-frames: decode order I P B B P B B ..., 25 frames per
    /// second, `gops` groups of `len` frames. Picture delay of two frames is hidden by an edit
    /// list that starts at media time 2.
    fn track(gops: usize, len: usize) -> (Track, Timeline) {
        let d = 40i64; // 25 fps at timescale 1000
        let mut samples = Vec::new();
        let mut dec = 0i64;
        let mut off = 1000u64;
        for g in 0..gops {
            let base = (g * len) as i64;
            // display order within the group: I=0, then P and the two B-frames before it.
            let mut order_disp: Vec<i64> = vec![0];
            let mut k = 1;
            while k < len as i64 {
                let p = (k + 2).min(len as i64 - 1);
                order_disp.push(p);
                for b in k..p {
                    order_disp.push(b);
                }
                k = p + 1;
            }
            for (j, disp) in order_disp.iter().enumerate() {
                let cts = (base + disp + 2) * d;
                samples.push(Sample {
                    offset: off,
                    size: 100,
                    dur: d as u32,
                    dts: dec,
                    cts_off: (cts - dec) as i32,
                    sync: j == 0,
                    desc: 1,
                });
                dec += d;
                off += 100;
            }
        }
        let t = Track {
            id: 1,
            handler: *b"vide",
            timescale: 1000,
            media_duration: dec as u64,
            tkhd: vec![],
            mdhd: vec![],
            hdlr: vec![],
            minf_head: vec![],
            mdia_extra: vec![],
            trak_extra: vec![],
            stsd: vec![],
            samples,
            edits: vec![Edit {
                duration: dec as u64,
                media_time: 2 * d,
                rate: 0x10000,
            }],
        };
        let tl = timeline_of(&t, 1000).unwrap();
        (t, tl)
    }

    #[test]
    fn the_edit_list_hides_the_picture_delay() {
        let (t, tl) = track(3, 10);
        assert_eq!(tl.media_time, 80);
        let first = presentation_order(&t, &tl)[0];
        assert_eq!(
            tl.pres(&t.samples[first]),
            0,
            "the first shown frame is at time zero"
        );
    }

    #[test]
    fn an_exact_cut_copies_whole_groups_and_encodes_only_the_edges() {
        let (t, tl) = track(6, 10); // 60 frames, keyframes every 10 frames (every 400 ms)
        // Keep 150 ms to 2150 ms: starts inside group 0, ends inside group 5.
        let cut = plan_video(&t, &tl, tl.ticks(0.15), Some(tl.ticks(2.15)), true).unwrap();
        assert_eq!(cut.pieces.len(), 3, "head, copy, tail: {:?}", cut.pieces);
        match &cut.pieces[1] {
            Piece::Copy { first, end, skip } => {
                assert!(skip.is_empty(), "closed groups have no leading pictures");
                assert!(*end > *first);
                assert!(t.samples[*first].sync, "the copy starts on a keyframe");
            }
            other => panic!("{other:?}"),
        }
        assert!(
            cut.frames_encoded < 20,
            "only the edge frames are re-encoded: {}",
            cut.frames_encoded
        );
        assert!(cut.frames_copied + cut.frames_encoded >= 50);
        // The result covers the range within a frame.
        assert!(
            (tl.secs(cut.start) - 0.16).abs() < 0.001,
            "{}",
            tl.secs(cut.start)
        );
        assert!(
            (tl.secs(cut.end) - 2.16).abs() < 0.05,
            "{}",
            tl.secs(cut.end)
        );
    }

    #[test]
    fn a_cut_on_keyframes_needs_no_encoding() {
        let (t, tl) = track(6, 10);
        let cut = plan_video(&t, &tl, tl.ticks(0.4), Some(tl.ticks(1.6)), true).unwrap();
        assert_eq!(cut.frames_encoded, 0, "{:?}", cut.pieces);
        assert_eq!(cut.pieces.len(), 1);
    }

    #[test]
    fn a_range_inside_one_group_is_one_encode() {
        let (t, tl) = track(6, 10);
        let cut = plan_video(&t, &tl, tl.ticks(0.45), Some(tl.ticks(0.7)), true).unwrap();
        assert_eq!(cut.pieces.len(), 1);
        assert!(matches!(cut.pieces[0], Piece::Encode { .. }));
        assert_eq!(cut.frames_copied, 0);
    }

    #[test]
    fn keyframe_mode_snaps_back_and_copies_everything() {
        let (t, tl) = track(6, 10);
        let cut = plan_video(&t, &tl, tl.ticks(0.55), Some(tl.ticks(1.3)), false).unwrap();
        assert_eq!(cut.frames_encoded, 0);
        assert_eq!(cut.pieces.len(), 1);
        // Snapped to the keyframe at 0.4 s, which is before the 0.55 s that was asked for.
        assert!(
            (tl.secs(cut.start) - 0.4).abs() < 0.001,
            "{}",
            tl.secs(cut.start)
        );
    }

    #[test]
    fn ranges_outside_the_video_are_plain_errors() {
        let (t, tl) = track(2, 10);
        assert!(plan_video(&t, &tl, tl.ticks(50.0), None, true).is_err());
        assert!(plan_video(&t, &tl, 0, Some(-5), true).is_err());
    }

    #[test]
    fn sound_windows_cut_inside_a_sample_with_an_offset() {
        let samples: Vec<Sample> = (0..100)
            .map(|i| Sample {
                offset: i * 50,
                size: 50,
                dur: 1024,
                dts: i as i64 * 1024,
                cts_off: 0,
                sync: true,
                desc: 1,
            })
            .collect();
        let t = Track {
            id: 2,
            handler: *b"soun",
            timescale: 48000,
            media_duration: 102400,
            tkhd: vec![],
            mdhd: vec![],
            hdlr: vec![],
            minf_head: vec![],
            mdia_extra: vec![],
            trak_extra: vec![],
            stsd: vec![],
            samples,
            edits: vec![],
        };
        let tl = timeline_of(&t, 1000).unwrap();
        let w = plan_window(&t, &tl, 48000, 96000).unwrap();
        assert_eq!(w.first, 46, "the sample that holds second 1.0");
        assert_eq!(w.offset, 48000 - 46 * 1024);
        assert!(w.end >= 93);
    }

    #[test]
    fn pieces_with_different_delays_splice_seamlessly() {
        // A copied piece with a two frame delay, then an encoded piece without one.
        let d = 40u32;
        let copy: Vec<Rel> = [(0, 0), (40, 120), (80, 40), (120, 80)]
            .iter()
            .map(|&(dts, cts)| Rel { dts, cts, dur: d })
            .collect();
        let enc: Vec<Rel> = (0..3)
            .map(|i| Rel {
                dts: i * 40,
                cts: i * 40,
                dur: d,
            })
            .collect();
        let (placed, shift) = place_pieces(&[copy, enc]);
        // The copy piece spans 4 frames of presentation (160) and 4 of decode (160).
        assert_eq!(placed[1][0].dts, 160);
        assert_eq!(
            placed[1][0].cts, 160,
            "the next piece is shown exactly where the last ended"
        );
        assert!(shift >= 0);
        let all: Vec<&Placed> = placed.iter().flatten().collect();
        assert!(
            all.iter().all(|p| p.cts + shift >= p.dts),
            "no negative composition offset"
        );
    }
}
