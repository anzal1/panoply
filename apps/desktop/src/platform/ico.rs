//! Reading the app icon out of the installer's `.ico` for the notification area.

/// The Windows icon the installer ships, with sizes from 16 to 128 pixels.
pub(crate) const PANOPLY_ICO: &[u8] = include_bytes!("../../packaging/windows/panoply.ico");

/// Decode the 32-bit BMP entry of the `.ico` closest to `want` pixels (at least `want` when there
/// is one) to RGBA. The directory is 6 bytes of header and 16 per entry.
pub(crate) fn decode(ico: &[u8], want: u32) -> Option<(Vec<u8>, u32, u32)> {
    let u16_at = |o: usize| ico.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| {
        ico.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let count = usize::from(u16_at(4)?);
    let mut best: Option<(u32, usize)> = None;
    for i in 0..count {
        let e = 6 + 16 * i;
        let size = u32_at(e + 8)? as usize;
        let off = u32_at(e + 12)? as usize;
        let head = ico.get(off..off + 40)?;
        // A BITMAPINFOHEADER (40) with 32 bits per pixel; PNG entries are skipped.
        if u32::from_le_bytes([head[0], head[1], head[2], head[3]]) != 40 {
            continue;
        }
        let w = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        let bpp = u16::from_le_bytes([head[14], head[15]]);
        if bpp != 32 || w == 0 || size < 40 + (w * w * 4) as usize {
            continue;
        }
        let better = match best {
            None => true,
            Some((bw, _)) => {
                if w >= want && bw < want {
                    true
                } else if w >= want && bw >= want {
                    w < bw
                } else {
                    bw < want && w > bw
                }
            }
        };
        if better {
            best = Some((w, off));
        }
    }
    let (w, off) = best?;
    let pixels = ico.get(off + 40..off + 40 + (w * w * 4) as usize)?;
    let mut rgba = vec![0u8; pixels.len()];
    // Rows are stored bottom-up as BGRA.
    for y in 0..w as usize {
        let src = (w as usize - 1 - y) * w as usize * 4;
        let dst = y * w as usize * 4;
        for x in 0..w as usize {
            let (s, d) = (src + x * 4, dst + x * 4);
            rgba[d] = pixels[s + 2];
            rgba[d + 1] = pixels[s + 1];
            rgba[d + 2] = pixels[s];
            rgba[d + 3] = pixels[s + 3];
        }
    }
    Some((rgba, w, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ico_decodes_to_a_small_icon() {
        let (rgba, w, h) = decode(PANOPLY_ICO, 16).expect("a 32-bit entry");
        assert_eq!((w, h), (16, 16));
        assert_eq!(rgba.len(), 16 * 16 * 4);
        assert!(rgba.chunks(4).any(|p| p[3] > 0), "not all transparent");
    }

    #[test]
    fn ico_picks_the_next_size_up() {
        let (_, w, _) = decode(PANOPLY_ICO, 30).expect("entry");
        assert_eq!(w, 32);
    }
}
