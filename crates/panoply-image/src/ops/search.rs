//! Quality search helpers shared by Compress and Resize to exact KB.
//!
//! A search tries many qualities. Encoding the whole picture each time is slow, so large
//! pictures are searched on a sample: a mosaic of tiles spread evenly over the picture, which
//! is an unbiased estimate of the whole-picture score. The caller always verifies the winner
//! on the whole picture before trusting it.

use crate::ssim::{Reference, Score};
use crate::types::{Image, Layout, Samples};

/// Pictures with at most this many pixels are searched whole.
pub const WHOLE_PIXELS: u64 = 1_500_000;
const TILE: u32 = 256;

/// The picture a search encodes, and how to score what comes back.
pub struct Sample {
    image: Image,
    refs: Vec<(Reference, (u32, u32))>,
    tile: (u32, u32),
    whole: bool,
}

/// Where the tiles of a mosaic lie in a picture.
#[derive(Debug, Clone)]
pub struct Grid {
    /// Tiles across.
    pub cols: u32,
    /// Tiles down.
    pub rows: u32,
    /// Width of one tile.
    pub tw: u32,
    /// Height of one tile.
    pub th: u32,
    /// Top left corner of each tile in the picture, row by row, left to right.
    pub origins: Vec<(u32, u32)>,
}

/// The tiles a search samples from a `w` by `h` picture, at positions that are multiples of 16
/// (the JPEG block grid for 4:2:0). `None` for a picture searched whole: at most
/// [`WHOLE_PIXELS`], or narrower or shorter than two tiles, unless `force` (a picture too large to
/// hold, which can only be searched on a sample).
pub fn mosaic_grid(w: u32, h: u32, force: bool) -> Option<Grid> {
    let px = w as u64 * h as u64;
    if !force && (px <= WHOLE_PIXELS || w < TILE * 2 || h < TILE * 2) {
        return None;
    }
    let (tw, th) = (TILE.min(w), TILE.min(h));
    let cols = (w / tw).clamp(1, 6);
    let rows = (h / th).clamp(1, 4);
    let mut origins = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let cx = ((c as u64 * 2 + 1) * w as u64 / (cols as u64 * 2)) as u32;
            let cy = ((r as u64 * 2 + 1) * h as u64 / (rows as u64 * 2)) as u32;
            let x = (cx.saturating_sub(tw / 2) / 16 * 16).min((w - tw) / 16 * 16);
            let y = (cy.saturating_sub(th / 2) / 16 * 16).min((h - th) / 16 * 16);
            origins.push((x, y));
        }
    }
    Some(Grid {
        cols,
        rows,
        tw,
        th,
        origins,
    })
}

impl Sample {
    /// Build the sample for `img`.
    pub fn new(img: &Image) -> Sample {
        let Some(g) = mosaic_grid(img.width, img.height, false) else {
            return Sample {
                image: img.clone(),
                refs: vec![(Reference::new(img), (0, 0))],
                tile: (img.width, img.height),
                whole: true,
            };
        };
        let tiles = g
            .origins
            .iter()
            .map(|&(x, y)| img.crop(x, y, g.tw, g.th))
            .collect();
        Sample::from_tiles(&g, tiles, img.icc.clone())
    }

    /// A sample made of the given tiles (one per origin of `g`, in the same order), for a
    /// picture that was never held whole.
    pub fn from_tiles(g: &Grid, tiles: Vec<Image>, icc: Option<std::sync::Arc<Vec<u8>>>) -> Sample {
        let first = &tiles[0];
        let mut mosaic = Image::new(g.cols * g.tw, g.rows * g.th, first.layout, first.depth());
        mosaic.icc = icc;
        let mut refs = Vec::new();
        for (i, tile) in tiles.iter().enumerate() {
            let (c, r) = (i as u32 % g.cols, i as u32 / g.cols);
            blit(&mut mosaic, tile, c * g.tw, r * g.th);
            refs.push((Reference::new(tile), (c * g.tw, r * g.th)));
        }
        Sample {
            image: mosaic,
            refs,
            tile: (g.tw, g.th),
            whole: false,
        }
    }

    /// The picture to encode while searching.
    pub fn image(&self) -> &Image {
        &self.image
    }

    /// True when the sample is the whole picture.
    pub fn is_whole(&self) -> bool {
        self.whole
    }

    /// Score a decoded version of [`Sample::image`].
    pub fn score(&self, decoded: &Image) -> Score {
        if self.whole {
            return self.refs[0].0.score(decoded);
        }
        let (mut l, mut c) = (0.0, 0.0);
        for (r, (x, y)) in &self.refs {
            let s = r.score(&decoded.crop(*x, *y, self.tile.0, self.tile.1));
            l += s.luma;
            c += s.chroma;
        }
        let n = self.refs.len() as f64;
        Score {
            luma: l / n,
            chroma: c / n,
        }
    }
}

fn blit(dst: &mut Image, src: &Image, x: u32, y: u32) {
    let ch = src.layout.channels();
    let (dw, sw) = (dst.width as usize, src.width as usize);
    match (&mut dst.samples, &src.samples) {
        (Samples::U8(d), Samples::U8(s)) => {
            for r in 0..src.height as usize {
                let doff = ((y as usize + r) * dw + x as usize) * ch;
                d[doff..doff + sw * ch].copy_from_slice(&s[r * sw * ch..(r + 1) * sw * ch]);
            }
        }
        (Samples::U16(d), Samples::U16(s)) => {
            for r in 0..src.height as usize {
                let doff = ((y as usize + r) * dw + x as usize) * ch;
                d[doff..doff + sw * ch].copy_from_slice(&s[r * sw * ch..(r + 1) * sw * ch]);
            }
        }
        _ => unreachable!("depth mismatch"),
    }
}

/// The lowest JPEG quality the default "smallest" target will pick.
///
/// SSIM on luma is blind to smooth regions when the rest of the picture is busy or dark: a 32 megapixel
/// night sky passed at 0.979 with quality 21 and showed 8 by 8 blocks and a banded halo at 100%
/// (docs/research/user-pass.md, UP-11). Without a floor the search settles at 10 to 21 on such pictures.
/// Quality 40 is where MozJPEG's blocks on a smooth gradient stop being visible at 100%.
pub const QUALITY_FLOOR: u32 = 40;

/// The lowest quality a search for `target` similarity may try, given the highest it will try (`hi`,
/// which stops a little above the quality the source was saved at, so a source saved below the floor
/// is never pushed above itself). A target under the default 0.97 was asked for by name: the person
/// chose a smaller file over a cleaner one, so there is no floor.
pub fn quality_floor(target: f64, hi: u32) -> u32 {
    const DEFAULT_TARGET: f64 = 0.97;
    const LOWEST: u32 = 10;
    if target + 1e-9 < DEFAULT_TARGET {
        LOWEST
    } else {
        QUALITY_FLOOR.min(hi).max(LOWEST)
    }
}

/// The smallest `q` in `[lo, hi]` for which `passes(q)` is true, assuming a higher quality never
/// passes less often. `hi` is taken to pass without being tried. Calls `passes` about
/// log2(hi - lo) times.
pub fn smallest_passing<E>(
    lo: u32,
    hi: u32,
    mut passes: impl FnMut(u32) -> Result<bool, E>,
) -> Result<u32, E> {
    let (mut lo, mut hi) = (lo, hi);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if passes(mid)? {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok(hi)
}

/// The largest `q` in `[lo, hi]` for which `fits(q)` is true, assuming a lower quality never
/// fits less often. Returns `None` when even `lo` does not fit.
pub fn largest_fitting<E>(
    lo: u32,
    hi: u32,
    mut fits: impl FnMut(u32) -> Result<bool, E>,
) -> Result<Option<u32>, E> {
    if !fits(lo)? {
        return Ok(None);
    }
    let (mut lo, mut hi) = (lo, hi);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if fits(mid)? {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    Ok(Some(lo))
}

/// True when the picture has the look of a photograph: many distinct colors.
pub fn looks_photographic(img: &Image) -> bool {
    use std::collections::HashSet;
    let ch = img.layout.channels();
    let Some(v) = img.as_u8() else { return true };
    let mut seen = HashSet::new();
    let step = (v.len() / ch / 60_000).max(1);
    for p in v.chunks_exact(ch).step_by(step) {
        let key = u32::from_le_bytes([
            p[0],
            *p.get(1).unwrap_or(&0),
            *p.get(2).unwrap_or(&0),
            if img.layout == Layout::Rgba { p[3] } else { 0 },
        ]);
        seen.insert(key);
        if seen.len() > 5000 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Depth;

    #[test]
    fn searches_find_the_edge() {
        let q = smallest_passing::<()>(1, 100, |q| Ok(q >= 37)).unwrap();
        assert_eq!(q, 37);
        assert_eq!(smallest_passing::<()>(1, 100, |_| Ok(false)).unwrap(), 100);
        assert_eq!(smallest_passing::<()>(1, 100, |_| Ok(true)).unwrap(), 1);
        assert_eq!(
            largest_fitting::<()>(1, 100, |q| Ok(q <= 64)).unwrap(),
            Some(64)
        );
        assert_eq!(largest_fitting::<()>(1, 100, |_| Ok(false)).unwrap(), None);
        assert_eq!(
            largest_fitting::<()>(1, 100, |_| Ok(true)).unwrap(),
            Some(100)
        );
    }

    #[test]
    fn the_floor_holds_at_the_default_and_never_passes_the_ceiling() {
        assert_eq!(quality_floor(0.97, 100), QUALITY_FLOOR);
        assert_eq!(quality_floor(0.99, 100), QUALITY_FLOOR);
        // A source saved at quality 20 searches up to 30 at most; the floor cannot sit above that.
        assert_eq!(quality_floor(0.97, 30), 30);
        // A named, lower similarity is a request for a smaller file.
        assert_eq!(quality_floor(0.9, 100), 10);
    }

    #[test]
    fn big_pictures_get_a_mosaic_that_scores_like_the_whole() {
        let mut img = Image::new(2000, 1500, Layout::Rgb, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            for (i, s) in v.iter_mut().enumerate() {
                *s = ((i * 7 + (i / 3000) * 11) % 251) as u8;
            }
        }
        let s = Sample::new(&img);
        assert!(!s.is_whole());
        assert!(s.image().width * s.image().height <= 1_600_000);
        let sc = s.score(s.image());
        assert!((sc.luma - 1.0).abs() < 1e-6);
    }

    #[test]
    fn small_pictures_are_searched_whole() {
        let img = Image::new(300, 200, Layout::Rgb, Depth::U8);
        assert!(Sample::new(&img).is_whole());
    }
}
