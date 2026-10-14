//! Palette quantizer for lossy PNG and GIF, written for Panoply (imagequant is GPL and gifski is
//! AGPL, see ADR 0002).
//!
//! Method: count colors in a 5-5-5 bit grid (4 bits of alpha), split the grid cells by weighted
//! median cut in OKLab space (a perceptually even space), refine the palette with a few rounds
//! of k-means, then map pixels to the nearest palette entry, with serpentine Floyd-Steinberg
//! error diffusion when asked. A picture that already has no more colors than the palette
//! size keeps its exact colors and is not touched.

use std::collections::HashMap;

use crate::types::{Image, Layout, Samples};

/// The result: a palette of RGBA entries and one index per pixel.
#[derive(Debug, Clone)]
pub struct Quantized {
    /// Palette entries. Entries that are not fully opaque come first.
    pub palette: Vec<[u8; 4]>,
    /// One palette index per pixel, row major.
    pub indices: Vec<u8>,
    /// True when the picture had few enough colors to be stored exactly.
    pub exact: bool,
}

fn srgb_lin(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn lin_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}

/// sRGB bytes to OKLab (Björn Ottosson's matrices).
fn to_oklab(r: f32, g: f32, b: f32) -> [f32; 3] {
    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn from_oklab(lab: [f32; 3]) -> [u8; 3] {
    let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
    let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
    let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        lin_srgb(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        lin_srgb(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
        lin_srgb(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    ]
}

/// A point in the quantizer's space: OKLab plus alpha.
type P4 = [f32; 4];

fn dist2(a: &P4, b: &P4) -> f32 {
    (0..4).map(|i| (a[i] - b[i]) * (a[i] - b[i])).sum()
}

struct Cell {
    p: P4,
    weight: f32,
}

fn point(rgba: [u8; 4]) -> P4 {
    let lab = to_oklab(srgb_lin(rgba[0]), srgb_lin(rgba[1]), srgb_lin(rgba[2]));
    [lab[0], lab[1], lab[2], rgba[3] as f32 / 255.0]
}

fn to_rgba(p: P4) -> [u8; 4] {
    let rgb = from_oklab([p[0], p[1], p[2]]);
    [
        rgb[0],
        rgb[1],
        rgb[2],
        (p[3] * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Read an 8 bit image as RGBA.
fn rgba_pixels(img: &Image) -> Vec<[u8; 4]> {
    let Samples::U8(v) = &img.to_u8().samples else {
        unreachable!()
    };
    let ch = img.layout.channels();
    v.chunks_exact(ch)
        .map(|p| match img.layout {
            Layout::Gray => [p[0], p[0], p[0], 255],
            Layout::GrayAlpha => [p[0], p[0], p[0], p[1]],
            Layout::Rgb => [p[0], p[1], p[2], 255],
            Layout::Rgba => [p[0], p[1], p[2], p[3]],
        })
        .collect()
}

/// Reduce `img` to at most `max_colors` (2 to 256). `dither` is the error diffusion strength,
/// 0.0 for none and 1.0 for full Floyd-Steinberg.
pub fn quantize(img: &Image, max_colors: usize, dither: f32) -> Quantized {
    let max_colors = max_colors.clamp(2, 256);
    let px = rgba_pixels(img);
    let (w, h) = (img.width as usize, img.height as usize);
    // Exact path: few colors, keep them all.
    let mut unique: HashMap<u32, u8> = HashMap::new();
    let key = |c: [u8; 4]| u32::from_le_bytes(c);
    let mut exact = true;
    for &c in &px {
        if !unique.contains_key(&key(c)) {
            if unique.len() == max_colors {
                exact = false;
                break;
            }
            unique.insert(key(c), 0);
        }
    }
    if exact {
        let mut pal: Vec<[u8; 4]> = unique.keys().map(|k| k.to_le_bytes()).collect();
        sort_palette(&mut pal);
        let map: HashMap<u32, u8> = pal
            .iter()
            .enumerate()
            .map(|(i, c)| (key(*c), i as u8))
            .collect();
        let indices = px.iter().map(|c| map[&key(*c)]).collect();
        return Quantized {
            palette: pal,
            indices,
            exact: true,
        };
    }

    // Histogram in a 5-5-5-4 grid, keeping each cell's mean color.
    let mut cells: HashMap<u32, ([u64; 4], u32)> = HashMap::new();
    for &c in &px {
        let k = ((c[0] >> 3) as u32) << 13
            | ((c[1] >> 3) as u32) << 8
            | ((c[2] >> 3) as u32) << 3
            | (c[3] >> 5) as u32;
        let e = cells.entry(k).or_insert(([0; 4], 0));
        for (acc, &v) in e.0.iter_mut().zip(c.iter()) {
            *acc += v as u64;
        }
        e.1 += 1;
    }
    let cells: Vec<Cell> = cells
        .values()
        .map(|(s, n)| {
            let n64 = *n as u64;
            let mean = [
                (s[0] / n64) as u8,
                (s[1] / n64) as u8,
                (s[2] / n64) as u8,
                (s[3] / n64) as u8,
            ];
            Cell {
                p: point(mean),
                weight: *n as f32,
            }
        })
        .collect();

    let mut centers = median_cut(&cells, max_colors);
    for _ in 0..4 {
        kmeans_round(&cells, &mut centers);
    }
    let mut palette: Vec<[u8; 4]> = centers.iter().map(|c| to_rgba(*c)).collect();
    palette.dedup();
    sort_palette(&mut palette);
    let pal_points: Vec<P4> = palette.iter().map(|c| point(*c)).collect();

    // Nearest color lookup per 6-6-6 bit grid cell for opaque pixels, brute force otherwise.
    let opaque_idx: Vec<usize> = (0..pal_points.len())
        .filter(|&i| palette[i][3] == 255)
        .collect();
    let mut cache: Vec<u16> = vec![u16::MAX; 1 << 18];
    let nearest = |p: &P4, among: &[usize]| -> usize {
        let mut best = (f32::MAX, 0usize);
        for &i in among {
            let d = dist2(p, &pal_points[i]);
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1
    };
    let all: Vec<usize> = (0..pal_points.len()).collect();
    let mut lookup = |c: [i32; 3], a: u8| -> usize {
        let cl = [
            c[0].clamp(0, 255) as u8,
            c[1].clamp(0, 255) as u8,
            c[2].clamp(0, 255) as u8,
        ];
        if a == 255 && !opaque_idx.is_empty() {
            let k = ((cl[0] >> 2) as usize) << 12
                | ((cl[1] >> 2) as usize) << 6
                | (cl[2] >> 2) as usize;
            if cache[k] == u16::MAX {
                let centre = [(cl[0] | 3) - 2, (cl[1] | 3) - 2, (cl[2] | 3) - 2];
                cache[k] =
                    nearest(&point([centre[0], centre[1], centre[2], 255]), &opaque_idx) as u16;
            }
            cache[k] as usize
        } else {
            nearest(&point([cl[0], cl[1], cl[2], a]), &all)
        }
    };

    let mut indices = vec![0u8; w * h];
    if dither <= 0.0 {
        for (o, c) in indices.iter_mut().zip(&px) {
            *o = lookup([c[0] as i32, c[1] as i32, c[2] as i32], c[3]) as u8;
        }
    } else {
        // Serpentine Floyd-Steinberg on the RGB channels, with the error scaled by `dither`.
        let mut err = vec![[0f32; 3]; (w + 2) * 2];
        let row = |y: usize| (y % 2) * (w + 2);
        for y in 0..h {
            let ltr = y % 2 == 0;
            let cur = row(y);
            let nxt = row(y + 1);
            for e in &mut err[nxt..nxt + w + 2] {
                *e = [0.0; 3];
            }
            for step in 0..w {
                let x = if ltr { step } else { w - 1 - step };
                let c = px[y * w + x];
                let e = err[cur + x + 1];
                let adj = [
                    (c[0] as f32 + e[0]).round() as i32,
                    (c[1] as f32 + e[1]).round() as i32,
                    (c[2] as f32 + e[2]).round() as i32,
                ];
                let i = lookup(adj, c[3]);
                indices[y * w + x] = i as u8;
                let pc = palette[i];
                let diff = [
                    (adj[0].clamp(0, 255) as f32 - pc[0] as f32) * dither,
                    (adj[1].clamp(0, 255) as f32 - pc[1] as f32) * dither,
                    (adj[2].clamp(0, 255) as f32 - pc[2] as f32) * dither,
                ];
                let (fwd, back): (isize, isize) = if ltr { (1, -1) } else { (-1, 1) };
                let add = |buf: &mut [[f32; 3]], at: usize, f: f32| {
                    for k in 0..3 {
                        buf[at][k] += diff[k] * f;
                    }
                };
                add(
                    &mut err,
                    (cur as isize + x as isize + 1 + fwd) as usize,
                    7.0 / 16.0,
                );
                add(
                    &mut err,
                    (nxt as isize + x as isize + 1 + back) as usize,
                    3.0 / 16.0,
                );
                add(&mut err, nxt + x + 1, 5.0 / 16.0);
                add(
                    &mut err,
                    (nxt as isize + x as isize + 1 + fwd) as usize,
                    1.0 / 16.0,
                );
            }
        }
    }
    Quantized {
        palette,
        indices,
        exact: false,
    }
}

/// Non-opaque entries first (so a PNG tRNS chunk stays short), then by lightness.
fn sort_palette(pal: &mut [[u8; 4]]) {
    pal.sort_by(|a, b| {
        (a[3] == 255)
            .cmp(&(b[3] == 255))
            .then(a[3].cmp(&b[3]))
            .then_with(|| {
                let la = a[0] as u32 * 299 + a[1] as u32 * 587 + a[2] as u32 * 114;
                let lb = b[0] as u32 * 299 + b[1] as u32 * 587 + b[2] as u32 * 114;
                la.cmp(&lb)
            })
    });
}

fn weighted_mean(cells: &[Cell], idx: &[usize]) -> P4 {
    let total: f32 = idx.iter().map(|&i| cells[i].weight).sum::<f32>().max(1e-9);
    let mut m = [0f32; 4];
    for &i in idx {
        for (k, mk) in m.iter_mut().enumerate() {
            *mk += cells[i].p[k] * cells[i].weight;
        }
    }
    m.map(|v| v / total)
}

fn median_cut(cells: &[Cell], n: usize) -> Vec<P4> {
    let mut boxes: Vec<Vec<usize>> = vec![(0..cells.len()).collect()];
    while boxes.len() < n {
        // The box with the most weighted spread that can still be split.
        let mut pick = None;
        let mut best = 0f32;
        for (bi, b) in boxes.iter().enumerate() {
            if b.len() < 2 {
                continue;
            }
            let m = weighted_mean(cells, b);
            let spread: f32 = b
                .iter()
                .map(|&i| cells[i].weight * dist2(&cells[i].p, &m))
                .sum();
            if spread > best {
                best = spread;
                pick = Some(bi);
            }
        }
        let Some(bi) = pick else { break };
        let mut b = boxes.swap_remove(bi);
        // Split along the axis with the largest weighted variance, at the weighted median.
        let m = weighted_mean(cells, &b);
        let mut axis = 0;
        let mut axis_var = -1.0f32;
        for (k, &mk) in m.iter().enumerate() {
            let v: f32 = b
                .iter()
                .map(|&i| cells[i].weight * (cells[i].p[k] - mk).powi(2))
                .sum();
            if v > axis_var {
                axis_var = v;
                axis = k;
            }
        }
        b.sort_by(|&x, &y| cells[x].p[axis].total_cmp(&cells[y].p[axis]));
        let total: f32 = b.iter().map(|&i| cells[i].weight).sum();
        let mut run = 0.0;
        let mut cut = 1;
        for (k, &i) in b.iter().enumerate() {
            run += cells[i].weight;
            if run >= total / 2.0 {
                cut = (k + 1).clamp(1, b.len() - 1);
                break;
            }
        }
        let right = b.split_off(cut);
        boxes.push(b);
        boxes.push(right);
    }
    boxes.iter().map(|b| weighted_mean(cells, b)).collect()
}

fn kmeans_round(cells: &[Cell], centers: &mut [P4]) {
    let mut sums = vec![[0f32; 4]; centers.len()];
    let mut wts = vec![0f32; centers.len()];
    for c in cells {
        let mut best = (f32::MAX, 0usize);
        for (i, ctr) in centers.iter().enumerate() {
            let d = dist2(&c.p, ctr);
            if d < best.0 {
                best = (d, i);
            }
        }
        for (acc, &v) in sums[best.1].iter_mut().zip(c.p.iter()) {
            *acc += v * c.weight;
        }
        wts[best.1] += c.weight;
    }
    for (i, ctr) in centers.iter_mut().enumerate() {
        if wts[i] > 0.0 {
            *ctr = sums[i].map(|v| v / wts[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Depth;

    fn gradient(w: u32, h: u32) -> Image {
        let mut img = Image::new(w, h, Layout::Rgb, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let i = (y * w as usize + x) * 3;
                    v[i] = (x * 255 / w as usize) as u8;
                    v[i + 1] = (y * 255 / h as usize) as u8;
                    v[i + 2] = ((x + y) * 255 / (w + h) as usize) as u8;
                }
            }
        }
        img
    }

    #[test]
    fn few_colors_are_kept_exactly() {
        let mut img = Image::new(8, 8, Layout::Rgb, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            for (i, p) in v.chunks_exact_mut(3).enumerate() {
                p.copy_from_slice(if i % 3 == 0 {
                    &[255, 0, 0]
                } else if i % 3 == 1 {
                    &[1, 2, 3]
                } else {
                    &[9, 9, 9]
                });
            }
        }
        let q = quantize(&img, 16, 0.0);
        assert!(q.exact);
        assert_eq!(q.palette.len(), 3);
        for (i, idx) in q.indices.iter().enumerate() {
            let want = if i % 3 == 0 {
                [255, 0, 0, 255]
            } else if i % 3 == 1 {
                [1, 2, 3, 255]
            } else {
                [9, 9, 9, 255]
            };
            assert_eq!(q.palette[*idx as usize], want);
        }
    }

    #[test]
    fn a_gradient_reduces_to_the_requested_size_with_small_error() {
        let img = gradient(64, 64);
        let q = quantize(&img, 64, 0.0);
        assert!(q.palette.len() <= 64 && q.palette.len() > 16);
        let Samples::U8(v) = &img.samples else {
            unreachable!()
        };
        let mut worst = 0i32;
        let mut total = 0f64;
        for (i, p) in v.chunks_exact(3).enumerate() {
            let c = q.palette[q.indices[i] as usize];
            let d = (0..3)
                .map(|k| (p[k] as i32 - c[k] as i32).abs())
                .max()
                .unwrap();
            worst = worst.max(d);
            total += d as f64;
        }
        assert!(
            total / (64.0 * 64.0) < 16.0,
            "mean error {}",
            total / 4096.0
        );
        assert!(worst < 70, "worst {worst}");
    }

    #[test]
    fn dither_changes_indices_but_keeps_the_palette_size() {
        let img = gradient(48, 48);
        let a = quantize(&img, 16, 0.0);
        let b = quantize(&img, 16, 1.0);
        assert_eq!(a.palette, b.palette);
        assert_ne!(a.indices, b.indices);
    }

    #[test]
    fn transparency_survives() {
        let mut img = Image::new(16, 16, Layout::Rgba, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            for (i, p) in v.chunks_exact_mut(4).enumerate() {
                let a = if i % 4 == 0 { 0 } else { 255 };
                p.copy_from_slice(&[
                    (i * 7 % 256) as u8,
                    (i * 13 % 256) as u8,
                    (i * 29 % 256) as u8,
                    a,
                ]);
            }
        }
        let q = quantize(&img, 8, 0.0);
        assert!(q.palette[0][3] < 255, "transparent entry comes first");
        assert_eq!(q.palette[q.indices[0] as usize][3], 0);
        assert_eq!(q.palette[q.indices[1] as usize][3], 255);
    }
}
