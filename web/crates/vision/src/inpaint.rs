//! Erase: the desktop crop, cluster and blend logic of image.erase, copied unchanged; the model run happens in the browser.
use crate::px::{Kernel, Rect, dilate, resize_u8};
use crate::tile::copy_out;
use crate::error::{ImageError, Result};

const MODEL_SIDE: usize = 512;
const PAD: usize = 128;
const MAX_CROP: usize = 1536;
const MERGE_GAP: usize = 96;

pub(crate) fn crop_for(b: Rect, expand: usize, w: usize, h: usize) -> Rect {
    let ext = b.w.max(b.h) + 2 * expand;
    let side = (ext + 2 * PAD).max(MODEL_SIDE).min(w).min(h);
    let cx = b.x + b.w / 2;
    let cy = b.y + b.h / 2;
    let x0 = cx.saturating_sub(side / 2).min(w - side);
    let y0 = cy.saturating_sub(side / 2).min(h - side);
    let mut r = Rect {
        x: x0,
        y: y0,
        w: side,
        h: side,
    };
    // The box itself plus the expansion must be inside (it is, unless the picture is tiny).
    let grown = b.grown(expand, w, h);
    if grown.x < r.x
        || grown.y < r.y
        || grown.x + grown.w > r.x + r.w
        || grown.y + grown.h > r.y + r.h
    {
        r = r.union(&grown);
    }
    r
}

pub fn clusters(mask: &[u8], w: usize, h: usize) -> Vec<Rect> {
    const CELL: usize = 16;
    let (gw, gh) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut marked = vec![false; gw * gh];
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] != 0 {
                marked[(y / CELL) * gw + x / CELL] = true;
            }
        }
    }
    let gap = MERGE_GAP.div_ceil(CELL);
    let mut parent: Vec<usize> = (0..gw * gh).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for cy in 0..gh {
        for cx in 0..gw {
            if !marked[cy * gw + cx] {
                continue;
            }
            for ny in cy.saturating_sub(gap)..=(cy + gap).min(gh - 1) {
                for nx in cx.saturating_sub(gap)..=(cx + gap).min(gw - 1) {
                    if marked[ny * gw + nx] {
                        let (a, b) = (
                            find(&mut parent, cy * gw + cx),
                            find(&mut parent, ny * gw + nx),
                        );
                        if a != b {
                            parent[a] = b;
                        }
                    }
                }
            }
        }
    }
    let mut boxes: std::collections::BTreeMap<usize, (usize, usize, usize, usize)> =
        std::collections::BTreeMap::new();
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] != 0 {
                let id = find(&mut parent, (y / CELL) * gw + x / CELL);
                let e = boxes.entry(id).or_insert((x, y, x, y));
                e.0 = e.0.min(x);
                e.1 = e.1.min(y);
                e.2 = e.2.max(x);
                e.3 = e.3.max(y);
            }
        }
    }
    boxes
        .into_values()
        .map(|(x0, y0, x1, y1)| Rect {
            x: x0,
            y: y0,
            w: x1 - x0 + 1,
            h: y1 - y0 + 1,
        })
        .collect()
}


/// One planned fill: the crop around a cluster and the grown mask inside it.
pub struct Plan {
    pub crop: Rect,
    pub grown: Vec<u8>,
}

/// What fill_one prepares for the model: planar pixels, the keep mask (255 keep, 0 erase), and the size it runs at.
pub struct Prepared {
    pub planar: Vec<u8>,
    pub keep: Vec<u8>,
    pub mw: usize,
    pub mh: usize,
    pub f: f64,
}

pub fn plan(mask: &[u8], w: usize, h: usize, expand: usize) -> (Vec<Plan>, Vec<u8>) {
    let boxes = clusters(mask, w, h);
    let mut changed = vec![0u8; w * h];
    let mut plans = Vec::new();
    for b in &boxes {
        let crop = crop_for(*b, expand, w, h);
        let win = copy_out(mask, w, 1, crop);
        let grown = dilate(&win, crop.w, crop.h, expand);
        for y in 0..crop.h {
            for x in 0..crop.w {
                if grown[y * crop.w + x] != 0 {
                    changed[(crop.y + y) * w + crop.x + x] = 1;
                }
            }
        }
        plans.push(Plan { crop, grown });
    }
    (plans, changed)
}

pub fn prepare(rgb: &[u8], w: usize, p: &Plan) -> Result<Prepared> {
    let (cw, ch) = (p.crop.w, p.crop.h);
    let pix = copy_out(rgb, w, 3, p.crop);
    let f = (cw.max(ch) as f64 / MAX_CROP as f64).max(1.0);
    let (mw, mh) = (((cw as f64 / f).round() as usize).max(1), ((ch as f64 / f).round() as usize).max(1));
    let (pix_in, mask_in): (Vec<u8>, Vec<u8>) = if f > 1.0 {
        let small_pix = resize_u8(&pix, cw, ch, 3, mw, mh, Kernel::Lanczos3)?;
        let m255: Vec<u8> = p.grown.iter().map(|&v| v * 255).collect();
        let sm = resize_u8(&m255, cw, ch, 1, mw, mh, Kernel::Bilinear)?;
        (small_pix, sm.iter().map(|&v| u8::from(v > 40)).collect())
    } else {
        (pix, p.grown.clone())
    };
    let n = mw * mh;
    let mut planar = vec![0u8; 3 * n];
    for (i, px) in pix_in.chunks_exact(3).enumerate() {
        planar[i] = px[0];
        planar[n + i] = px[1];
        planar[2 * n + i] = px[2];
    }
    let keep = mask_in.iter().map(|&v| if v != 0 { 0 } else { 255 }).collect();
    Ok(Prepared { planar, keep, mw, mh, f })
}

pub fn blend(rgb: &mut [u8], w: usize, p: &Plan, pr: &Prepared, res: &[u8]) -> Result<()> {
    let (cw, ch, n) = (p.crop.w, p.crop.h, pr.mw * pr.mh);
    if res.len() != 3 * n {
        return Err(ImageError::Failed(format!("The fill model returned the wrong size ({} values for {} x {}). Try again.", res.len(), pr.mw, pr.mh)));
    }
    let mut inter = vec![0u8; n * 3];
    for i in 0..n {
        inter[i * 3] = res[i];
        inter[i * 3 + 1] = res[n + i];
        inter[i * 3 + 2] = res[2 * n + i];
    }
    let filled = if pr.f > 1.0 { resize_u8(&inter, pr.mw, pr.mh, 3, cw, ch, Kernel::CatmullRom)? } else { inter };
    for y in 0..ch {
        for x in 0..cw {
            if p.grown[y * cw + x] != 0 {
                let d = ((p.crop.y + y) * w + p.crop.x + x) * 3;
                let s = (y * cw + x) * 3;
                rgb[d..d + 3].copy_from_slice(&filled[s..s + 3]);
            }
        }
    }
    Ok(())
}
