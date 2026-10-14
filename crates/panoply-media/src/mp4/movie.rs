//! The movie index: tracks and their sample tables, read from `moov`.
//!
//! Every sample of every track becomes one [`Sample`] with its file offset, size, decode time,
//! composition offset and sync flag, so cutting, rebuilding and repairing an MP4 are list
//! operations. Fragmented files (`moof`) are not read here; callers fall back to the helper
//! engine for those.

use super::boxes::{FourCc, be16, be32, be64, child, children};
use crate::error::{MediaError, Result};

/// One sample (a frame of video, a frame of audio, one subtitle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// Where its bytes are in the source file.
    pub offset: u64,
    /// How many bytes.
    pub size: u32,
    /// Duration in track time units.
    pub dur: u32,
    /// Decode time in track time units.
    pub dts: i64,
    /// Composition time minus decode time (nonzero with B-frames).
    pub cts_off: i32,
    /// True when decoding can start here.
    pub sync: bool,
    /// Which sample description (1 based).
    pub desc: u16,
}

impl Sample {
    /// Composition (presentation) time before the edit list.
    pub fn cts(&self) -> i64 {
        self.dts + i64::from(self.cts_off)
    }
}

/// One edit list entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit {
    /// Length in movie time units.
    pub duration: u64,
    /// Start in track time units, or -1 for an empty edit.
    pub media_time: i64,
    /// Rate as 16.16 fixed point (0x10000 is normal speed).
    pub rate: u32,
}

/// One track.
#[derive(Debug, Clone)]
pub struct Track {
    /// Track id.
    pub id: u32,
    /// `vide`, `soun`, `text`, `sbtl`, `tmcd` and so on.
    pub handler: FourCc,
    /// Time units per second.
    pub timescale: u32,
    /// Media duration in track time units.
    pub media_duration: u64,
    /// The whole `tkhd` box (matrix and size included, so rotation survives).
    pub tkhd: Vec<u8>,
    /// The whole `mdhd` box.
    pub mdhd: Vec<u8>,
    /// The whole `hdlr` box.
    pub hdlr: Vec<u8>,
    /// Raw boxes of `minf` other than the sample table (`vmhd`, `smhd`, `dinf`, ...).
    pub minf_head: Vec<u8>,
    /// Raw boxes of `mdia` other than `mdhd`, `hdlr` and `minf` (`elng`).
    pub mdia_extra: Vec<u8>,
    /// Raw boxes of `trak` other than `tkhd`, `edts` and `mdia` (`tref`, `udta`, `meta`).
    pub trak_extra: Vec<u8>,
    /// The sample description entries, each as a whole box.
    pub stsd: Vec<Vec<u8>>,
    /// The samples in decode order.
    pub samples: Vec<Sample>,
    /// The edit list (empty when the track has none).
    pub edits: Vec<Edit>,
}

impl Track {
    /// The code of the first sample description (`avc1`, `hvc1`, `mp4a`, ...).
    pub fn codec(&self) -> String {
        self.stsd
            .first()
            .filter(|e| e.len() >= 8)
            .map(|e| String::from_utf8_lossy(&e[4..8]).into_owned())
            .unwrap_or_default()
    }

    /// True for a picture track.
    pub fn is_video(&self) -> bool {
        &self.handler == b"vide"
    }

    /// True for a sound track.
    pub fn is_audio(&self) -> bool {
        &self.handler == b"soun"
    }

    /// Track duration in seconds from its samples.
    pub fn seconds(&self) -> f64 {
        let total: u64 = self.samples.iter().map(|s| u64::from(s.dur)).sum();
        total as f64 / f64::from(self.timescale.max(1))
    }

    /// Total bytes of sample data.
    pub fn bytes(&self) -> u64 {
        self.samples.iter().map(|s| u64::from(s.size)).sum()
    }

    /// Number of sync samples.
    pub fn sync_count(&self) -> usize {
        self.samples.iter().filter(|s| s.sync).count()
    }
}

/// The index of a file.
#[derive(Debug, Clone)]
pub struct Movie {
    /// Movie time units per second.
    pub timescale: u32,
    /// Movie duration in movie time units.
    pub duration: u64,
    /// The whole `mvhd` box.
    pub mvhd: Vec<u8>,
    /// The tracks.
    pub tracks: Vec<Track>,
    /// Raw boxes of `moov` other than `mvhd` and `trak` (`udta`, `meta`).
    pub extra: Vec<u8>,
}

/// The most samples one track may have for Panoply's own MP4 code (about 190 MB of tables).
pub const MAX_SAMPLES_PER_TRACK: usize = 6_000_000;

fn unsupported(what: &str) -> MediaError {
    MediaError::Unsupported(format!(
        "This file's index uses {what}, which Panoply's own MP4 code does not read."
    ))
}

fn bad(what: &str) -> MediaError {
    MediaError::Corrupt(format!(
        "This file's index is damaged ({what}). Try Repair video."
    ))
}

/// Parse the payload of a `moov` box.
pub fn parse_movie(moov: &[u8]) -> Result<Movie> {
    let kids = children(moov);
    if kids.iter().any(|(t, _, _)| t == b"mvex") {
        return Err(unsupported("fragments"));
    }
    let mvhd_range = kids
        .iter()
        .find(|(t, _, _)| t == b"mvhd")
        .ok_or_else(|| bad("no movie header"))?;
    let mvhd = moov[mvhd_range.1 - 8..mvhd_range.2].to_vec();
    let (timescale, duration) = parse_mvhd(&moov[mvhd_range.1..mvhd_range.2])?;
    let mut tracks = Vec::new();
    let mut extra = Vec::new();
    for (t, s, e) in &kids {
        match t {
            b"trak" => tracks.push(parse_track(&moov[*s..*e])?),
            b"mvhd" => {}
            _ => extra.extend_from_slice(&moov[*s - 8..*e]),
        }
    }
    Ok(Movie {
        timescale,
        duration,
        mvhd,
        tracks,
        extra,
    })
}

fn parse_mvhd(p: &[u8]) -> Result<(u32, u64)> {
    if p.len() < 20 {
        return Err(bad("short movie header"));
    }
    if p[0] == 1 {
        if p.len() < 32 {
            return Err(bad("short movie header"));
        }
        Ok((be32(p, 20), be64(p, 24)))
    } else {
        Ok((be32(p, 12), u64::from(be32(p, 16))))
    }
}

fn parse_mdhd(p: &[u8]) -> Result<(u32, u64)> {
    parse_mvhd(p)
}

fn parse_track(trak: &[u8]) -> Result<Track> {
    let kids = children(trak);
    let mut tkhd = Vec::new();
    let mut edits = Vec::new();
    let mut mdia: &[u8] = &[];
    let mut trak_extra = Vec::new();
    for (t, s, e) in &kids {
        match t {
            b"tkhd" => tkhd = trak[*s - 8..*e].to_vec(),
            b"edts" => edits = parse_edts(&trak[*s..*e])?,
            b"mdia" => mdia = &trak[*s..*e],
            _ => trak_extra.extend_from_slice(&trak[*s - 8..*e]),
        }
    }
    if tkhd.len() < 24 {
        return Err(bad("a track has no header"));
    }
    let id = if tkhd[8] == 1 {
        be32(&tkhd, 28)
    } else {
        be32(&tkhd, 20)
    };
    let mut mdhd = Vec::new();
    let mut hdlr = Vec::new();
    let mut minf: &[u8] = &[];
    let mut mdia_extra = Vec::new();
    for (t, s, e) in children(mdia) {
        match &t {
            b"mdhd" => mdhd = mdia[s - 8..e].to_vec(),
            b"hdlr" => hdlr = mdia[s - 8..e].to_vec(),
            b"minf" => minf = &mdia[s..e],
            _ => mdia_extra.extend_from_slice(&mdia[s - 8..e]),
        }
    }
    if mdhd.len() < 12 || hdlr.len() < 20 {
        return Err(bad("a track has no media header"));
    }
    let (timescale, media_duration) = parse_mdhd(&mdhd[8..])?;
    let handler: FourCc = [hdlr[16], hdlr[17], hdlr[18], hdlr[19]];
    let mut minf_head = Vec::new();
    let mut stbl: &[u8] = &[];
    for (t, s, e) in children(minf) {
        if &t == b"stbl" {
            stbl = &minf[s..e];
        } else {
            minf_head.extend_from_slice(&minf[s - 8..e]);
        }
    }
    let (stsd, samples) = parse_stbl(stbl)?;
    Ok(Track {
        id,
        handler,
        timescale: timescale.max(1),
        media_duration,
        tkhd,
        mdhd,
        hdlr,
        minf_head,
        mdia_extra,
        trak_extra,
        stsd,
        samples,
        edits,
    })
}

fn parse_edts(edts: &[u8]) -> Result<Vec<Edit>> {
    let Some(elst) = child(edts, b"elst") else {
        return Ok(Vec::new());
    };
    if elst.len() < 8 {
        return Err(bad("short edit list"));
    }
    let version = elst[0];
    let n = be32(elst, 4) as usize;
    let mut out = Vec::with_capacity(n);
    let mut at = 8;
    for _ in 0..n {
        let (dur, mt, rate_at) = if version == 1 {
            if at + 20 > elst.len() {
                break;
            }
            (be64(elst, at), be64(elst, at + 8) as i64, at + 16)
        } else {
            if at + 12 > elst.len() {
                break;
            }
            (
                u64::from(be32(elst, at)),
                i64::from(be32(elst, at + 4) as i32),
                at + 8,
            )
        };
        let rate = be32(elst, rate_at);
        at = rate_at + 4;
        out.push(Edit {
            duration: dur,
            media_time: mt,
            rate,
        });
    }
    Ok(out)
}

fn parse_stbl(stbl: &[u8]) -> Result<(Vec<Vec<u8>>, Vec<Sample>)> {
    let mut stsd_raw: &[u8] = &[];
    let mut stss: Option<&[u8]> = None;
    let empty: &[u8] = &[];
    let (mut stts, mut ctts, mut stsz) = (empty, empty, empty);
    let (mut stsc, mut stco, mut co64) = (empty, empty, empty);
    for (t, s, e) in children(stbl) {
        let body = &stbl[s..e];
        match &t {
            b"stsd" => stsd_raw = body,
            b"stts" => stts = body,
            b"ctts" => ctts = body,
            b"stss" => stss = Some(body),
            b"stsz" => stsz = body,
            b"stz2" => return Err(unsupported("compact sample sizes")),
            b"stsc" => stsc = body,
            b"stco" => stco = body,
            b"co64" => co64 = body,
            _ => {}
        }
    }
    // Sample descriptions.
    let mut stsd = Vec::new();
    if stsd_raw.len() >= 8 {
        let n = be32(stsd_raw, 4) as usize;
        let mut at = 8;
        for _ in 0..n {
            if at + 8 > stsd_raw.len() {
                break;
            }
            let size = be32(stsd_raw, at) as usize;
            if size < 8 || at + size > stsd_raw.len() {
                break;
            }
            stsd.push(stsd_raw[at..at + size].to_vec());
            at += size;
        }
    }
    // Sizes.
    if stsz.len() < 12 {
        return Ok((stsd, Vec::new()));
    }
    let fixed = be32(stsz, 4);
    let count = be32(stsz, 8) as usize;
    // Uncompressed audio in QuickTime stores every sample frame as a sample. Past this many the
    // tables would not stay small, so such files go to the helper engine instead.
    if count > MAX_SAMPLES_PER_TRACK {
        return Err(unsupported(
            "more samples than its own tables can hold in memory (uncompressed audio)",
        ));
    }
    let sizes: Vec<u32> = if fixed != 0 {
        vec![fixed; count]
    } else {
        let avail = (stsz.len() - 12) / 4;
        (0..count.min(avail))
            .map(|i| be32(stsz, 12 + i * 4))
            .collect()
    };
    let count = sizes.len();
    // Decode times.
    let mut dts = Vec::with_capacity(count);
    let mut durs = Vec::with_capacity(count);
    if stts.len() >= 8 {
        let n = be32(stts, 4) as usize;
        let mut t = 0i64;
        'outer: for i in 0..n {
            let at = 8 + i * 8;
            if at + 8 > stts.len() {
                break;
            }
            let (c, d) = (be32(stts, at) as usize, be32(stts, at + 4));
            for _ in 0..c {
                if dts.len() >= count {
                    break 'outer;
                }
                dts.push(t);
                durs.push(d);
                t += i64::from(d);
            }
        }
    }
    let have_time = dts.len();
    // Composition offsets.
    let mut offs: Vec<i32> = Vec::new();
    if ctts.len() >= 8 {
        let n = be32(ctts, 4) as usize;
        'o: for i in 0..n {
            let at = 8 + i * 8;
            if at + 8 > ctts.len() {
                break;
            }
            let (c, v) = (be32(ctts, at) as usize, be32(ctts, at + 4) as i32);
            for _ in 0..c {
                if offs.len() >= count {
                    break 'o;
                }
                offs.push(v);
            }
        }
    }
    // Sync samples.
    let mut sync = vec![stss.is_none(); count];
    if let Some(s) = stss
        && s.len() >= 8
    {
        let n = be32(s, 4) as usize;
        for i in 0..n {
            let at = 8 + i * 4;
            if at + 4 > s.len() {
                break;
            }
            let k = be32(s, at) as usize;
            if k >= 1 && k <= count {
                sync[k - 1] = true;
            }
        }
    }
    // Chunks.
    let chunk_offsets: Vec<u64> = if co64.len() >= 8 {
        let n = be32(co64, 4) as usize;
        (0..n)
            .take_while(|i| 8 + i * 8 + 8 <= co64.len())
            .map(|i| be64(co64, 8 + i * 8))
            .collect()
    } else if stco.len() >= 8 {
        let n = be32(stco, 4) as usize;
        (0..n)
            .take_while(|i| 8 + i * 4 + 4 <= stco.len())
            .map(|i| u64::from(be32(stco, 8 + i * 4)))
            .collect()
    } else {
        Vec::new()
    };
    let runs: Vec<(u32, u32, u32)> = if stsc.len() >= 8 {
        let n = be32(stsc, 4) as usize;
        (0..n)
            .take_while(|i| 8 + i * 12 + 12 <= stsc.len())
            .map(|i| {
                (
                    be32(stsc, 8 + i * 12),
                    be32(stsc, 12 + i * 12),
                    be32(stsc, 16 + i * 12),
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut samples = Vec::with_capacity(count.min(have_time));
    let mut k = 0usize;
    for (ci, &base) in chunk_offsets.iter().enumerate() {
        let chunk_no = (ci + 1) as u32;
        let run = runs.iter().rev().find(|r| r.0 <= chunk_no);
        let Some(&(_, per, desc)) = run else { continue };
        let mut off = base;
        for _ in 0..per {
            if k >= have_time {
                break;
            }
            samples.push(Sample {
                offset: off,
                size: sizes[k],
                dur: durs[k],
                dts: dts[k],
                cts_off: offs.get(k).copied().unwrap_or(0),
                sync: sync[k],
                desc: desc.clamp(1, u32::from(u16::MAX)) as u16,
            });
            off += u64::from(sizes[k]);
            k += 1;
        }
        if k >= have_time {
            break;
        }
    }
    let _ = be16;
    Ok((stsd, samples))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4::boxes::{bx, full};

    #[test]
    fn a_tiny_index_round_trips_through_the_parser() {
        // One audio-like track with three samples in two chunks.
        let stsd = full(
            b"stsd",
            0,
            0,
            &[&1u32.to_be_bytes()[..], &bx(b"mp4a", &[0u8; 20])].concat(),
        );
        let stts = full(
            b"stts",
            0,
            0,
            &[
                &1u32.to_be_bytes()[..],
                &3u32.to_be_bytes(),
                &1024u32.to_be_bytes(),
            ]
            .concat(),
        );
        let stsz = full(
            b"stsz",
            0,
            0,
            &[
                &0u32.to_be_bytes()[..],
                &3u32.to_be_bytes(),
                &10u32.to_be_bytes(),
                &20u32.to_be_bytes(),
                &30u32.to_be_bytes(),
            ]
            .concat(),
        );
        let stsc = full(
            b"stsc",
            0,
            0,
            &[
                &2u32.to_be_bytes()[..],
                &1u32.to_be_bytes(),
                &2u32.to_be_bytes(),
                &1u32.to_be_bytes(),
                &2u32.to_be_bytes(),
                &1u32.to_be_bytes(),
                &1u32.to_be_bytes(),
            ]
            .concat(),
        );
        let stco = full(
            b"stco",
            0,
            0,
            &[
                &2u32.to_be_bytes()[..],
                &100u32.to_be_bytes(),
                &500u32.to_be_bytes(),
            ]
            .concat(),
        );
        let stbl = bx(b"stbl", &[stsd, stts, stsz, stsc, stco].concat());
        let (d, s) = parse_stbl(&stbl[8..]).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].offset, s[1].offset, s[2].offset), (100, 110, 500));
        assert_eq!((s[0].dts, s[1].dts, s[2].dts), (0, 1024, 2048));
        assert!(
            s.iter().all(|x| x.sync),
            "no stss means every sample is a sync sample"
        );
    }

    #[test]
    fn fragmented_files_are_refused_plainly() {
        let moov = [bx(b"mvhd", &[0u8; 100]), bx(b"mvex", &[])].concat();
        let err = parse_movie(&moov).unwrap_err();
        assert!(err.to_string().contains("fragments"));
    }
}
