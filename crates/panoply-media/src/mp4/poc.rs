//! Display order of H.264 and HEVC pictures, from their slice headers.
//!
//! A file whose index is lost has pictures in decode order only. Pictures with B-frames are
//! shown in a different order (picture order count, POC), and an index that ignores that makes
//! motion jump back and forth. Reading the POC from the first slice of every picture, with the
//! standard's rules for its high bits, gives the order without decoding a single frame.
//! Only the fields up to the POC are read, so this costs a few bytes per picture.

use std::collections::HashMap;

use super::repair::{Nal, Params};

/// Remove emulation prevention bytes (`00 00 03` becomes `00 00`) from a NAL payload.
pub fn unescape(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut zeros = 0;
    for &x in b {
        if zeros >= 2 && x == 3 {
            zeros = 0;
            continue;
        }
        out.push(x);
        zeros = if x == 0 { zeros + 1 } else { 0 };
    }
    out
}

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

    fn u(&mut self, n: usize) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.bit()?;
        }
        Some(v)
    }

    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1u32 << zeros) - 1 + self.u(zeros)?)
    }

    fn se(&mut self) -> Option<i32> {
        let k = self.ue()?;
        Some(if k % 2 == 1 {
            k.div_ceil(2) as i32
        } else {
            -((k / 2) as i32)
        })
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.at += n;
        (self.at <= self.d.len() * 8).then_some(())
    }
}

#[derive(Clone, Copy)]
struct AvcSps {
    log2_max_frame_num: usize,
    gaps: bool,
    poc_type: u32,
    log2_max_poc_lsb: usize,
    frame_mbs_only: bool,
}

#[derive(Clone, Copy)]
struct HevcSps {
    log2_max_poc_lsb: usize,
    separate_colour_plane: bool,
}

#[derive(Clone, Copy)]
struct HevcPps {
    dependent_slices: bool,
    output_flag_present: bool,
    extra_bits: usize,
}

fn parse_avc_sps(nal: &[u8]) -> Option<AvcSps> {
    let d = unescape(nal.get(1..)?);
    let mut b = Bits { d: &d, at: 0 };
    let profile = b.u(8)?;
    b.skip(16)?; // constraint flags, level
    b.ue()?; // sps id
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        let chroma = b.ue()?;
        if chroma == 3 {
            b.skip(1)?;
        }
        b.ue()?;
        b.ue()?;
        b.skip(1)?;
        if b.bit()? == 1 {
            let n = if chroma != 3 { 8 } else { 12 };
            for i in 0..n {
                if b.bit()? == 1 {
                    // scaling list: 16 or 64 entries of se(v)
                    let size = if i < 6 { 16 } else { 64 };
                    let (mut last, mut next) = (8i32, 8i32);
                    for _ in 0..size {
                        if next != 0 {
                            next = (last + b.se()? + 256) % 256;
                        }
                        last = if next == 0 { last } else { next };
                    }
                }
            }
        }
    }
    let log2_max_frame_num = b.ue()? as usize + 4;
    let poc_type = b.ue()?;
    let mut log2_max_poc_lsb = 0;
    match poc_type {
        0 => log2_max_poc_lsb = b.ue()? as usize + 4,
        1 => {
            b.skip(1)?;
            b.se()?;
            b.se()?;
            let n = b.ue()?;
            for _ in 0..n.min(256) {
                b.se()?;
            }
        }
        _ => {}
    }
    b.ue()?; // max_num_ref_frames
    let gaps = b.bit()? == 1; // gaps_in_frame_num_value_allowed_flag
    b.ue()?; // width
    b.ue()?; // height
    let frame_mbs_only = b.bit()? == 1;
    Some(AvcSps {
        log2_max_frame_num,
        gaps,
        poc_type,
        log2_max_poc_lsb,
        frame_mbs_only,
    })
}

fn parse_hevc_sps(nal: &[u8]) -> Option<HevcSps> {
    let d = unescape(nal.get(2..)?);
    let mut b = Bits { d: &d, at: 0 };
    b.skip(4)?; // vps id
    let max_sub = b.u(3)? as usize;
    b.skip(1)?;
    // profile_tier_level(1, max_sub)
    b.skip(88)?;
    b.skip(8)?; // general_level_idc
    let mut profile_present = [false; 8];
    let mut level_present = [false; 8];
    for i in 0..max_sub {
        profile_present[i] = b.bit()? == 1;
        level_present[i] = b.bit()? == 1;
    }
    if max_sub > 0 {
        for _ in max_sub..8 {
            b.skip(2)?;
        }
    }
    for i in 0..max_sub {
        if profile_present[i] {
            b.skip(88)?;
        }
        if level_present[i] {
            b.skip(8)?;
        }
    }
    b.ue()?; // sps id
    let chroma = b.ue()?;
    let separate = chroma == 3 && b.bit()? == 1;
    b.ue()?;
    b.ue()?;
    if b.bit()? == 1 {
        for _ in 0..4 {
            b.ue()?;
        }
    }
    b.ue()?;
    b.ue()?;
    let log2_max_poc_lsb = b.ue()? as usize + 4;
    Some(HevcSps {
        log2_max_poc_lsb,
        separate_colour_plane: separate,
    })
}

fn parse_hevc_pps(nal: &[u8]) -> Option<(u32, HevcPps)> {
    let d = unescape(nal.get(2..)?);
    let mut b = Bits { d: &d, at: 0 };
    let id = b.ue()?;
    b.ue()?; // sps id
    let dependent = b.bit()? == 1;
    let output = b.bit()? == 1;
    let extra = b.u(3)? as usize;
    Some((
        id,
        HevcPps {
            dependent_slices: dependent,
            output_flag_present: output,
            extra_bits: extra,
        },
    ))
}

/// How many bits an H.264 `frame_num` has, for checking that pictures follow each other the way
/// a real stream does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameNums {
    /// Bits in `frame_num`.
    pub bits: usize,
}

/// The `frame_num` rule of an H.264 stream, or `None` for other codecs and for streams that allow
/// gaps in `frame_num` (where consecutive numbers prove nothing).
pub fn avc_frame_nums(params: &Params) -> Option<FrameNums> {
    if params.codec != Nal::Avc {
        return None;
    }
    let sps = params
        .sets
        .iter()
        .find(|s| s.first().is_some_and(|h| h & 0x1F == 7))
        .and_then(|s| parse_avc_sps(s))?;
    (!sps.gaps).then_some(FrameNums {
        bits: sps.log2_max_frame_num,
    })
}

/// The `frame_num` in the first slice of a picture, `nal` being the slice NAL with its header.
pub fn avc_frame_num(rule: FrameNums, nal: &[u8]) -> Option<u32> {
    let d = unescape(nal.get(1..)?);
    let mut b = Bits { d: &d, at: 0 };
    b.ue()?;
    b.ue()?;
    b.ue()?;
    b.u(rule.bits)
}

/// What a picture's first slice says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pic {
    /// A picture that resets the order and needs nothing before it (IDR, or BLA in HEVC).
    pub clean_start: bool,
    /// Any intra random access point (a sync sample).
    pub sync: bool,
    /// Picture order count.
    pub poc: i64,
}

/// Reads picture order counts, one picture at a time, in decode order.
pub struct PocReader {
    codec: Nal,
    avc: Option<AvcSps>,
    hevc: Option<HevcSps>,
    pps: HashMap<u32, HevcPps>,
    prev_msb: i64,
    prev_lsb: i64,
}

impl PocReader {
    /// Build from the parameter sets of a reference file or of the stream. `None` when they cannot
    /// be read.
    pub fn new(params: &Params) -> Option<PocReader> {
        let mut r = PocReader {
            codec: params.codec,
            avc: None,
            hevc: None,
            pps: HashMap::new(),
            prev_msb: 0,
            prev_lsb: 0,
        };
        for s in &params.sets {
            match (params.codec, s.first()) {
                (Nal::Avc, Some(h)) if h & 0x1F == 7 => r.avc = parse_avc_sps(s),
                (Nal::Hevc, Some(h)) if (h >> 1) & 0x3F == 33 => r.hevc = parse_hevc_sps(s),
                (Nal::Hevc, Some(h)) if (h >> 1) & 0x3F == 34 => {
                    if let Some((id, p)) = parse_hevc_pps(s) {
                        r.pps.insert(id, p);
                    }
                }
                _ => {}
            }
        }
        (r.avc.is_some() || r.hevc.is_some()).then_some(r)
    }

    /// True when the picture order is the decode order for the whole stream (POC type 2).
    pub fn is_decode_order(&self) -> bool {
        self.avc.is_some_and(|s| s.poc_type == 2)
    }

    /// Read the picture whose first slice NAL (header included) is `nal`.
    pub fn picture(&mut self, nal: &[u8]) -> Option<Pic> {
        match self.codec {
            Nal::Avc => self.avc_picture(nal),
            Nal::Hevc => self.hevc_picture(nal),
        }
    }

    fn advance(&mut self, lsb: i64, max_lsb: i64, reset: bool, is_ref: bool) -> i64 {
        if reset {
            self.prev_msb = 0;
            self.prev_lsb = 0;
        }
        let msb = if lsb < self.prev_lsb && self.prev_lsb - lsb >= max_lsb / 2 {
            self.prev_msb + max_lsb
        } else if lsb > self.prev_lsb && lsb - self.prev_lsb > max_lsb / 2 {
            self.prev_msb - max_lsb
        } else {
            self.prev_msb
        };
        if is_ref {
            self.prev_msb = msb;
            self.prev_lsb = lsb;
        }
        msb + lsb
    }

    fn avc_picture(&mut self, nal: &[u8]) -> Option<Pic> {
        let sps = self.avc?;
        let ty = nal.first()? & 0x1F;
        let idc = (nal.first()? >> 5) & 3;
        let d = unescape(nal.get(1..)?);
        let mut b = Bits { d: &d, at: 0 };
        b.ue()?; // first_mb
        let slice_type = b.ue()?;
        b.ue()?; // pps id
        b.skip(sps.log2_max_frame_num)?;
        if !sps.frame_mbs_only && b.bit()? == 1 {
            b.skip(1)?;
        }
        let idr = ty == 5;
        if idr {
            b.ue()?;
        }
        let intra = slice_type % 5 == 2 || slice_type % 5 == 4;
        if sps.poc_type != 0 {
            return Some(Pic {
                clean_start: idr,
                sync: idr || intra && idr,
                poc: 0,
            });
        }
        let lsb = i64::from(b.u(sps.log2_max_poc_lsb)?);
        let poc = self.advance(lsb, 1i64 << sps.log2_max_poc_lsb, idr, idc != 0);
        Some(Pic {
            clean_start: idr,
            sync: idr,
            poc,
        })
    }

    fn hevc_picture(&mut self, nal: &[u8]) -> Option<Pic> {
        let sps = self.hevc?;
        let ty = (nal.first()? >> 1) & 0x3F;
        let tid = nal.get(1)? & 7;
        let d = unescape(nal.get(2..)?);
        let mut b = Bits { d: &d, at: 0 };
        let first = b.bit()? == 1;
        if !first {
            return None;
        }
        let irap = (16..=23).contains(&ty);
        if irap {
            b.skip(1)?;
        }
        let pps_id = b.ue()?;
        let pps = self.pps.get(&pps_id).copied().unwrap_or(HevcPps {
            dependent_slices: false,
            output_flag_present: false,
            extra_bits: 0,
        });
        // first_slice_segment_in_pic_flag is set, so no dependent flag and no address follow.
        b.skip(pps.extra_bits)?;
        b.ue()?; // slice_type
        if pps.output_flag_present {
            b.skip(1)?;
        }
        if sps.separate_colour_plane {
            b.skip(2)?;
        }
        let _ = pps.dependent_slices;
        let clean = matches!(ty, 16..=20);
        if ty == 19 || ty == 20 {
            return Some(Pic {
                clean_start: true,
                sync: true,
                poc: 0,
            })
            .inspect(|_| {
                self.prev_msb = 0;
                self.prev_lsb = 0;
            });
        }
        let lsb = i64::from(b.u(sps.log2_max_poc_lsb)?);
        // Sub-layer non-reference pictures (even types below 16) and RASL/RADL do not move the
        // reference point.
        let is_ref = tid == 0 && !(ty < 16 && ty % 2 == 0) && !(6..=9).contains(&ty);
        let poc = self.advance(lsb, 1i64 << sps.log2_max_poc_lsb, clean, is_ref);
        Some(Pic {
            clean_start: clean,
            sync: irap,
            poc,
        })
    }
}

/// The display position of every picture, given their order counts in decode order. Pictures
/// are ranked by POC within each stretch that starts at a clean start.
pub fn display_ranks(pics: &[Pic]) -> Vec<u32> {
    let mut ranks = vec![0u32; pics.len()];
    let mut base = 0u32;
    let mut start = 0usize;
    while start < pics.len() {
        let mut end = start + 1;
        while end < pics.len() && !pics[end].clean_start {
            end += 1;
        }
        let mut idx: Vec<usize> = (start..end).collect();
        idx.sort_by_key(|&i| (pics[i].poc, i));
        for (rank, i) in idx.iter().enumerate() {
            ranks[*i] = base + rank as u32;
        }
        base += (end - start) as u32;
        start = end;
    }
    ranks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emulation_prevention_bytes_are_removed() {
        assert_eq!(unescape(&[1, 0, 0, 3, 1, 5]), vec![1, 0, 0, 1, 5]);
        assert_eq!(unescape(&[0, 0, 3]), vec![0, 0]);
        assert_eq!(unescape(&[7, 3, 3]), vec![7, 3, 3]);
    }

    #[test]
    fn exp_golomb_reads_the_standard_codes() {
        // 1 = 0, 010 = 1, 011 = 2, 00100 = 3, 00111 = 6
        let d = [0b1010_0110, 0b0100_0011, 0b1000_0000];
        let mut b = Bits { d: &d, at: 0 };
        assert_eq!(b.ue(), Some(0));
        assert_eq!(b.ue(), Some(1));
        assert_eq!(b.ue(), Some(2));
        assert_eq!(b.ue(), Some(3));
        assert_eq!(b.ue(), Some(6));
    }

    #[test]
    fn a_typical_ibbp_run_is_ranked_into_display_order() {
        // Decode order I P B B P B B with POCs 0 6 2 4 12 8 10 (the encoder's steps of two).
        let pics: Vec<Pic> = [0i64, 6, 2, 4, 12, 8, 10]
            .iter()
            .enumerate()
            .map(|(i, p)| Pic {
                clean_start: i == 0,
                sync: i == 0,
                poc: *p,
            })
            .collect();
        assert_eq!(display_ranks(&pics), vec![0, 3, 1, 2, 6, 4, 5]);
    }

    #[test]
    fn a_new_clean_start_begins_a_new_run() {
        let pics = vec![
            Pic {
                clean_start: true,
                sync: true,
                poc: 0,
            },
            Pic {
                clean_start: false,
                sync: false,
                poc: 4,
            },
            Pic {
                clean_start: false,
                sync: false,
                poc: 2,
            },
            Pic {
                clean_start: true,
                sync: true,
                poc: 0,
            },
            Pic {
                clean_start: false,
                sync: false,
                poc: 2,
            },
        ];
        assert_eq!(display_ranks(&pics), vec![0, 2, 1, 3, 4]);
    }

    #[test]
    fn the_high_bits_follow_the_wrap_of_the_low_bits() {
        let params = Params {
            codec: Nal::Avc,
            length_size: 4,
            // A baseline-profile SPS: profile 66, level 30, sps id 0, log2_max_frame_num 4,
            // poc type 0 with log2_max_poc_lsb 4 (16), one reference frame, progressive.
            sets: vec![vec![0x67, 66, 0, 30, 0xF4, 0xF0]],
        };
        let mut r = PocReader::new(&params).expect("the SPS parses");
        // An IDR with lsb 0, then reference pictures whose lsb climbs 8, 14 then wraps to 4.
        let mut pocs = Vec::new();
        for (i, lsb) in [0u32, 8, 14, 4].iter().enumerate() {
            let ty: u8 = if i == 0 { 5 } else { 1 };
            // slice header bits: first_mb ue(0)=1, slice_type ue(0)=1, pps ue(0)=1, frame_num u(4),
            // [idr_pic_id ue(0)=1 for IDR], poc lsb u(4)
            let mut bits: Vec<u8> = vec![1, 1, 1];
            bits.extend((0..4).rev().map(|k| ((i as u32 >> k) & 1) as u8));
            if i == 0 {
                bits.push(1);
            }
            bits.extend((0..4).rev().map(|k| ((lsb >> k) & 1) as u8));
            while !bits.len().is_multiple_of(8) {
                bits.push(0);
            }
            let mut nal = vec![0x60 | ty];
            for c in bits.chunks(8) {
                nal.push(c.iter().fold(0u8, |a, b| (a << 1) | b));
            }
            pocs.push(r.picture(&nal).expect("slice header parses").poc);
        }
        assert_eq!(
            pocs,
            vec![0, 8, 14, 20],
            "after 14 the lsb wraps to 4, which is 20 once the msb grows"
        );
    }
}
