//! Tiling: walking a picture in square cores with a halo of context, so every per-pixel filter
//! and every model run stays within a fixed memory size however large the picture is.

use crate::px::Rect;

/// The cores of a `w x h` picture cut into squares of at most `core` pixels, left to right, top
/// to bottom.
pub fn cores(w: usize, h: usize, core: usize) -> Vec<Rect> {
    let core = core.max(1);
    let mut v = Vec::new();
    let mut y = 0;
    while y < h {
        let ch = core.min(h - y);
        let mut x = 0;
        while x < w {
            let cw = core.min(w - x);
            v.push(Rect { x, y, w: cw, h: ch });
            x += cw;
        }
        y += ch;
    }
    v
}

/// The core grown by a halo on every side, clamped to the picture.
pub fn window(core: Rect, halo: usize, w: usize, h: usize) -> Rect {
    core.grown(halo, w, h)
}

/// Copies the window `r` out of an interleaved picture with `ch` channels.
pub fn copy_out<T: Copy>(src: &[T], w: usize, ch: usize, r: Rect) -> Vec<T> {
    let mut out = Vec::with_capacity(r.w * r.h * ch);
    for y in r.y..r.y + r.h {
        out.extend_from_slice(&src[(y * w + r.x) * ch..(y * w + r.x + r.w) * ch]);
    }
    out
}

/// Writes `win` (a window of `win_rect` size) back into `dst`, but only the part inside `core`.
pub fn paste_core<T: Copy>(
    dst: &mut [T],
    w: usize,
    ch: usize,
    win: &[T],
    win_rect: Rect,
    core: Rect,
) {
    for y in core.y..core.y + core.h {
        let wy = y - win_rect.y;
        let wx = core.x - win_rect.x;
        let s = &win[(wy * win_rect.w + wx) * ch..(wy * win_rect.w + wx + core.w) * ch];
        dst[(y * w + core.x) * ch..(y * w + core.x + core.w) * ch].copy_from_slice(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cores_cover_the_picture_exactly_once() {
        let (w, h) = (1000usize, 700usize);
        let mut seen = vec![0u8; w * h];
        for c in cores(w, h, 384) {
            for y in c.y..c.y + c.h {
                for x in c.x..c.x + c.w {
                    seen[y * w + x] += 1;
                }
            }
        }
        assert!(seen.iter().all(|&v| v == 1));
    }

    #[test]
    fn windows_clamp_and_paste_roundtrips() {
        let (w, h) = (50usize, 40usize);
        let src: Vec<u8> = (0..w * h).map(|i| (i % 251) as u8).collect();
        let mut dst = vec![0u8; w * h];
        for c in cores(w, h, 16) {
            let win = window(c, 5, w, h);
            let data = copy_out(&src, w, 1, win);
            paste_core(&mut dst, w, 1, &data, win, c);
        }
        assert_eq!(src, dst);
    }
}
