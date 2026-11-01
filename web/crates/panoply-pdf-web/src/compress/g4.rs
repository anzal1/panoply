//! CCITT Group 4 (ITU-T T.6) for 1-bit pictures, through the `fax` crate (MIT, pure Rust).
//!
//! Samples are packed 8 to a byte, most significant bit first, each row padded to a byte, the way
//! a PDF stores a 1-bit image. A sample of 1 is a white pixel and 0 is black, which is what
//! `CCITTFaxDecode` with `/BlackIs1 false` (the default) hands back, so the samples a viewer sees
//! are the samples that went in, whatever `/Decode` and `/ImageMask` then make of them.
//!
//! Every stream this module returns has been decoded again and compared with its input row by
//! row, so a bug in the coder can cost a few bytes of effort but never a changed pixel.

use fax::{Color, VecWriter, decoder, encoder::Encoder};

/// Widest picture the coder takes (positions are 16 bits).
const MAX_SIDE: usize = 65_535;

fn bit(row: &[u8], x: usize) -> bool {
    row[x >> 3] & (0x80 >> (x & 7)) != 0
}

/// Positions where a row changes colour, starting from white (the form the decoder reports).
fn transitions(row: &[u8], w: usize) -> Vec<u16> {
    let mut out = Vec::new();
    let mut white = true;
    for x in 0..w {
        if bit(row, x) != white {
            out.push(x as u16);
            white = !white;
        }
    }
    out
}

/// Group 4 data for `h` rows of `w` samples, or `None` when the picture is too big for the coder
/// or the check after coding does not pass.
pub fn encode(bits: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let rb = w.div_ceil(8);
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || bits.len() != rb * h {
        return None;
    }
    // The crate asserts on its own tables; a panic must not take a job down.
    let coded = std::panic::catch_unwind(|| {
        let mut enc = Encoder::new(VecWriter::with_capacity(bits.len() * 2));
        for row in bits.chunks_exact(rb) {
            let pels = (0..w).map(|x| {
                if bit(row, x) {
                    Color::White
                } else {
                    Color::Black
                }
            });
            let Ok(()) = enc.encode_line(pels, w as u16);
        }
        let Ok(writer) = enc.finish();
        writer.finish()
    })
    .ok()?;
    verify(&coded, bits, w, h).then_some(coded)
}

/// Decode `coded` and compare every row with `bits`.
fn verify(coded: &[u8], bits: &[u8], w: usize, h: usize) -> bool {
    let rb = w.div_ceil(8);
    let mut y = 0usize;
    let mut same = true;
    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        decoder::decode_g4(coded.iter().copied(), w as u16, Some(h as u16), |t| {
            if y < h && same {
                same = t == transitions(&bits[y * rb..(y + 1) * rb], w).as_slice();
            }
            y += 1;
        })
    }));
    matches!(done, Ok(Some(()))) && same && y == h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(rows: &[Vec<bool>]) -> (Vec<u8>, usize, usize) {
        let (w, h) = (rows[0].len(), rows.len());
        let rb = w.div_ceil(8);
        let mut out = vec![0u8; rb * h];
        for (y, r) in rows.iter().enumerate() {
            for (x, &v) in r.iter().enumerate() {
                if v {
                    out[y * rb + (x >> 3)] |= 0x80 >> (x & 7);
                }
            }
        }
        (out, w, h)
    }

    #[test]
    fn text_like_pictures_round_trip_and_shrink() {
        let (w, h) = (613, 200);
        let mut s = 7u32;
        let rows: Vec<Vec<bool>> = (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| {
                        s = s.wrapping_mul(1103515245).wrapping_add(12345);
                        // Glyph-like blobs on white with a little speckle.
                        let blob = (x / 9 + y / 11) % 3 == 0 && (x % 9) < 5 && (y % 11) < 7;
                        !(blob || (s >> 16).is_multiple_of(997))
                    })
                    .collect()
            })
            .collect();
        let (bits, w, h) = pack(&rows);
        let g4 = encode(&bits, w, h).expect("coded");
        assert!(g4.len() < bits.len() / 4, "{} vs {}", g4.len(), bits.len());
    }

    #[test]
    fn blank_black_and_noisy_rows_survive() {
        let mut rows = vec![vec![true; 129]; 5];
        rows.push(vec![false; 129]);
        rows.push((0..129).map(|x| x % 2 == 0).collect());
        rows.push(vec![true; 129]);
        rows.push((0..129).map(|x| (x * 7) % 5 < 2).collect());
        let (bits, w, h) = pack(&rows);
        assert!(encode(&bits, w, h).is_some());
        // A very wide row needs make-up codes past 2560.
        let wide = vec![
            (0..6000)
                .map(|x| (x / 3000) % 2 == 0)
                .collect::<Vec<bool>>();
            3
        ];
        let (bits, w, h) = pack(&wide);
        assert!(encode(&bits, w, h).is_some());
    }

    #[test]
    fn a_wrong_size_is_refused() {
        assert!(encode(&[0u8; 10], 16, 4).is_none());
        assert!(encode(&[], 0, 0).is_none());
    }
}
