//! EXIF orientation (IPC-1): reading it and applying all eight values, including the mirrored
//! 2, 4, 5 and 7.

use crate::types::{Image, Samples};

/// True when the orientation turns the picture by 90 degrees, so width and height swap.
pub fn swaps_dims(o: u8) -> bool {
    (5..=8).contains(&o)
}

/// Read the Orientation tag (0x0112) from the first IFD of a TIFF header block
/// (`II*\0` or `MM\0*`). Returns 1 when there is none or the block is damaged.
pub fn orientation_from_exif(exif: &[u8]) -> u8 {
    let le = match exif.get(0..4) {
        Some(b"II*\0") => true,
        Some(b"MM\0*") => false,
        _ => return 1,
    };
    let u16_at = |o: usize| -> Option<u16> {
        let b = exif.get(o..o + 2)?;
        Some(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |o: usize| -> Option<u32> {
        let b = exif.get(o..o + 4)?;
        Some(if le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    };
    let Some(ifd) = u32_at(4) else { return 1 };
    let ifd = ifd as usize;
    let Some(count) = u16_at(ifd) else { return 1 };
    for i in 0..count.min(512) as usize {
        let e = ifd + 2 + i * 12;
        if u16_at(e) == Some(0x0112) {
            // Type SHORT, count 1: the value sits in the first two bytes of the value field.
            return match u16_at(e + 8) {
                Some(v @ 1..=8) => v as u8,
                _ => 1,
            };
        }
    }
    1
}

/// Where output pixel `(x, y)` comes from in a source of size `w` by `h`.
#[inline]
fn source_of(o: u8, w: usize, h: usize, x: usize, y: usize) -> (usize, usize) {
    match o {
        2 => (w - 1 - x, y),
        3 => (w - 1 - x, h - 1 - y),
        4 => (x, h - 1 - y),
        5 => (y, x),
        6 => (y, h - 1 - x),
        7 => (w - 1 - y, h - 1 - x),
        8 => (w - 1 - y, x),
        _ => (x, y),
    }
}

/// The size of the picture after orientation.
pub fn oriented_size(o: u8, w: u32, h: u32) -> (u32, u32) {
    if swaps_dims(o) { (h, w) } else { (w, h) }
}

/// Fill `out` with upright rows `[y0, y0 + n)` of `img` stored with orientation `o`.
/// `out` holds `n` rows of the oriented width, in the image's layout and depth.
pub fn gather_rows(img: &Image, o: u8, y0: u32, n: u32, out: &mut Samples) {
    let (ow, oh) = oriented_size(o, img.width, img.height);
    debug_assert!(y0 + n <= oh);
    let (w, h) = (img.width as usize, img.height as usize);
    let ch = img.layout.channels();
    macro_rules! go {
        ($src:expr, $dst:expr) => {{
            for r in 0..n as usize {
                let y = y0 as usize + r;
                let drow = &mut $dst[r * ow as usize * ch..(r + 1) * ow as usize * ch];
                if o <= 1 {
                    drow.copy_from_slice(&$src[y * w * ch..(y + 1) * w * ch]);
                    continue;
                }
                for x in 0..ow as usize {
                    let (sx, sy) = source_of(o, w, h, x, y);
                    let s = (sy * w + sx) * ch;
                    drow[x * ch..(x + 1) * ch].copy_from_slice(&$src[s..s + ch]);
                }
            }
        }};
    }
    match (&img.samples, out) {
        (Samples::U8(s), Samples::U8(d)) => go!(s, d),
        (Samples::U16(s), Samples::U16(d)) => go!(s, d),
        _ => unreachable!("depth mismatch"),
    }
}

/// Apply orientation `o` to a whole image.
pub fn apply(img: Image, o: u8) -> Image {
    if o <= 1 || o > 8 {
        return img;
    }
    let (ow, oh) = oriented_size(o, img.width, img.height);
    let mut out = Image::new(ow, oh, img.layout, img.depth());
    out.icc = img.icc.clone();
    let mut samples = std::mem::replace(&mut out.samples, Samples::U8(Vec::new()));
    gather_rows(&img, o, 0, oh, &mut samples);
    out.samples = samples;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Depth, Layout};

    // A 3 by 2 image, pixel value = index + 1, so every move is visible.
    fn sample() -> Image {
        let mut img = Image::new(3, 2, Layout::Gray, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            v.copy_from_slice(&[1, 2, 3, 4, 5, 6]);
        }
        img
    }

    fn px(img: &Image) -> Vec<u8> {
        img.as_u8().unwrap().to_vec()
    }

    #[test]
    fn all_eight_orientations() {
        // Expected pictures (row major) for the 3 by 2 source
        //   1 2 3
        //   4 5 6
        let cases: [(u8, (u32, u32), [u8; 6]); 8] = [
            (1, (3, 2), [1, 2, 3, 4, 5, 6]),
            (2, (3, 2), [3, 2, 1, 6, 5, 4]),
            (3, (3, 2), [6, 5, 4, 3, 2, 1]),
            (4, (3, 2), [4, 5, 6, 1, 2, 3]),
            (5, (2, 3), [1, 4, 2, 5, 3, 6]),
            (6, (2, 3), [4, 1, 5, 2, 6, 3]),
            (7, (2, 3), [6, 3, 5, 2, 4, 1]),
            (8, (2, 3), [3, 6, 2, 5, 1, 4]),
        ];
        for (o, dims, want) in cases {
            let got = apply(sample(), o);
            assert_eq!((got.width, got.height), dims, "orientation {o}");
            assert_eq!(px(&got), want, "orientation {o}");
        }
    }

    #[test]
    fn reads_orientation_from_a_tiff_block() {
        // Little endian, one entry: tag 0x0112 SHORT 1 value 6.
        let mut b = b"II*\0\x08\0\0\0".to_vec();
        b.extend_from_slice(&[1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0]);
        assert_eq!(orientation_from_exif(&b), 6);
        assert_eq!(orientation_from_exif(b"junk"), 1);
        assert_eq!(orientation_from_exif(&[]), 1);
    }
}
