//! Print sheet: the desktop layout (guillotine packing of photos on paper, both paper orientations, optional rotation and cut marks)
//! and sheet drawing, copied from panoply-id sheet.rs. Encoding (JPEG, PNG, PDF) happens in the browser.
#![allow(dead_code)]
use std::collections::HashMap;

use crate::error::{ImageError, Result};
use crate::image::{Image, Layout, Samples, resample_image};

/// Paper.
#[derive(Debug, Clone, PartialEq)]
pub struct Paper {
    /// Name for the card.
    pub name: String,
    /// Width in millimetres (portrait).
    pub w_mm: f64,
    /// Height in millimetres (portrait).
    pub h_mm: f64,
}

/// A paper by name: `4x6`, `5x7`, `6x8`, `a4`, `a5`, `a6`, `letter`.
pub fn paper(name: &str) -> Result<Paper> {
    let n = name.trim().to_ascii_lowercase().replace([' ', '"'], "");
    let (label, w, h) = match n.as_str() {
        "4x6" | "4x6in" | "10x15" => ("4 x 6 in", 101.6, 152.4),
        "5x7" | "5x7in" | "13x18" => ("5 x 7 in", 127.0, 177.8),
        "6x8" | "6x8in" => ("6 x 8 in", 152.4, 203.2),
        "a4" => ("A4", 210.0, 297.0),
        "a5" => ("A5", 148.0, 210.0),
        "a6" => ("A6", 105.0, 148.0),
        "letter" | "us-letter" => ("Letter", 215.9, 279.4),
        _ => {
            return Err(ImageError::BadOption(format!(
                "\"{name}\" is not a paper size Panoply knows. Use 4x6, 5x7, 6x8, a4, a5, a6 or letter."
            )));
        }
    };
    Ok(Paper {
        name: label.to_string(),
        w_mm: w,
        h_mm: h,
    })
}

/// Parse `35x45`, `35 x 45 mm`, `3.5x4.5cm`, `1.5x2in` into millimetres. A bare pair is millimetres.
pub fn parse_size_mm(s: &str) -> Option<(f64, f64)> {
    let t = s.trim().to_ascii_lowercase().replace(' ', "");
    let (body, unit) = if let Some(b) = t.strip_suffix("mm") {
        (b, 1.0)
    } else if let Some(b) = t.strip_suffix("cm") {
        (b, 10.0)
    } else if let Some(b) = t.strip_suffix("in") {
        (b, 25.4)
    } else {
        (t.as_str(), 1.0)
    };
    let (a, b) = body.split_once('x')?;
    let (w, h): (f64, f64) = (a.parse().ok()?, b.parse().ok()?);
    (w > 5.0 / unit && h > 5.0 / unit && w * unit < 400.0 && h * unit < 400.0)
        .then_some((w * unit, h * unit))
}

/// One photo on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// Left edge from the paper's left edge, mm.
    pub x_mm: f64,
    /// Top edge from the paper's top edge, mm.
    pub y_mm: f64,
    /// Width on the paper (after any turn), mm.
    pub w_mm: f64,
    /// Height on the paper (after any turn), mm.
    pub h_mm: f64,
    /// The photo is turned a quarter turn.
    pub rotated: bool,
}

/// What to lay out.
#[derive(Debug, Clone)]
pub struct SheetSpec {
    /// The paper.
    pub paper: Paper,
    /// Photo size, width then height, mm.
    pub cell_mm: (f64, f64),
    /// Margin on every side, mm.
    pub margin_mm: f64,
    /// Gap between photos, mm.
    pub gutter_mm: f64,
    /// Number of photos; `None` fills the sheet.
    pub copies: Option<u32>,
    /// Corner ticks to cut along.
    pub cut_marks: bool,
    /// Photos may be turned a quarter turn to fit more.
    pub rotate: bool,
    /// Output resolution.
    pub dpi: u32,
}

const UNIT: f64 = 100.0; // pattern arithmetic in hundredths of a millimetre

fn to_u(v: f64) -> i64 {
    (v * UNIT).round() as i64
}

/// Best guillotine packing of boxes `a` (and `b` when given) into `u` by `v` (hundredths of mm).
/// Returns pieces as `(x, y, use_b)` with the box's own size.
fn guillotine(u: i64, v: i64, a: (i64, i64), b: Option<(i64, i64)>) -> Vec<(i64, i64, bool)> {
    let pieces: Vec<(i64, i64)> = std::iter::once(a).chain(b).collect();
    let patterns = |limit: i64, sizes: Vec<i64>| -> Vec<i64> {
        let mut set = vec![0i64];
        let mut i = 0;
        while i < set.len() {
            for &s in &sizes {
                let n = set[i] + s;
                if n <= limit && !set.contains(&n) {
                    set.push(n);
                }
            }
            i += 1;
        }
        set.sort_unstable();
        set
    };
    let xs = patterns(u, pieces.iter().map(|p| p.0).collect());
    let ys = patterns(v, pieces.iter().map(|p| p.1).collect());
    let floor_idx = |vals: &[i64], x: i64| -> usize {
        match vals.binary_search(&x) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    };
    #[derive(Clone, Copy)]
    enum Pick {
        None,
        Piece(bool),
        Vert(usize),
        Horz(usize),
    }
    let mut best: HashMap<(usize, usize), (u32, Pick)> = HashMap::new();
    // Fill by increasing size so sub-results exist.
    for xi in 0..xs.len() {
        for yi in 0..ys.len() {
            let (x, y) = (xs[xi], ys[yi]);
            let mut cur = (0u32, Pick::None);
            for (k, p) in pieces.iter().enumerate() {
                if p.0 <= x && p.1 <= y && cur.0 < 1 {
                    cur = (1, Pick::Piece(k == 1));
                }
            }
            for ci in 1..xi {
                let rest = floor_idx(&xs, x - xs[ci]);
                let n = best[&(ci, yi)].0 + best[&(rest, yi)].0;
                if n > cur.0 {
                    cur = (n, Pick::Vert(ci));
                }
            }
            for ci in 1..yi {
                let rest = floor_idx(&ys, y - ys[ci]);
                let n = best[&(xi, ci)].0 + best[&(xi, rest)].0;
                if n > cur.0 {
                    cur = (n, Pick::Horz(ci));
                }
            }
            best.insert((xi, yi), cur);
        }
    }
    let mut out = Vec::new();
    fn walk(
        best: &HashMap<(usize, usize), (u32, Pick)>,
        xs: &[i64],
        ys: &[i64],
        floor_idx: &dyn Fn(&[i64], i64) -> usize,
        (xi, yi): (usize, usize),
        (ox, oy): (i64, i64),
        out: &mut Vec<(i64, i64, bool)>,
    ) {
        match best[&(xi, yi)].1 {
            Pick::None => {}
            Pick::Piece(b) => out.push((ox, oy, b)),
            Pick::Vert(ci) => {
                let (x, cx) = (xs[xi], xs[ci]);
                walk(best, xs, ys, floor_idx, (ci, yi), (ox, oy), out);
                let rest = floor_idx(xs, x - cx);
                walk(best, xs, ys, floor_idx, (rest, yi), (ox + cx, oy), out);
            }
            Pick::Horz(ci) => {
                let (y, cy) = (ys[yi], ys[ci]);
                walk(best, xs, ys, floor_idx, (xi, ci), (ox, oy), out);
                let rest = floor_idx(ys, y - cy);
                walk(best, xs, ys, floor_idx, (xi, rest), (ox, oy + cy), out);
            }
        }
    }
    let (xi, yi) = (xs.len() - 1, ys.len() - 1);
    walk(&best, &xs, &ys, &floor_idx, (xi, yi), (0, 0), &mut out);
    out
}

/// Where the photos go, and the paper's size as fed (turned when that fitted more). Fills the
/// sheet; `copies` is applied by the caller.
pub fn layout_on(spec: &SheetSpec) -> Result<(Vec<Placement>, (f64, f64))> {
    let (cw, ch) = spec.cell_mm;
    let g = spec.gutter_mm;
    let mut best: Vec<Placement> = Vec::new();
    let mut best_paper = (spec.paper.w_mm, spec.paper.h_mm);
    // The paper may be fed either way round; try both and keep the better (portrait on a tie).
    for turned_paper in [false, true] {
        let (pw, ph) = if turned_paper {
            (spec.paper.h_mm, spec.paper.w_mm)
        } else {
            (spec.paper.w_mm, spec.paper.h_mm)
        };
        let (u, v) = (pw - 2.0 * spec.margin_mm + g, ph - 2.0 * spec.margin_mm + g);
        if u <= 0.0 || v <= 0.0 {
            continue;
        }
        let a = (to_u(cw + g), to_u(ch + g));
        let b = (spec.rotate && (cw - ch).abs() > 0.05).then_some((to_u(ch + g), to_u(cw + g)));
        let pieces = guillotine(to_u(u), to_u(v), a, b);
        if pieces.len() > best.len() {
            best_paper = (pw, ph);
            best = pieces
                .into_iter()
                .map(|(x, y, rot)| {
                    let (w, h) = if rot { (ch, cw) } else { (cw, ch) };
                    Placement {
                        x_mm: spec.margin_mm + x as f64 / UNIT,
                        y_mm: spec.margin_mm + y as f64 / UNIT,
                        w_mm: w,
                        h_mm: h,
                        rotated: rot,
                    }
                })
                .collect();
        }
    }
    best.sort_by(|a, b| {
        (a.y_mm.round() as i64, a.x_mm.round() as i64)
            .cmp(&(b.y_mm.round() as i64, b.x_mm.round() as i64))
    });
    Ok((best, best_paper))
}

/// The placements alone (see [`layout_on`]).
pub fn layout(spec: &SheetSpec) -> Result<Vec<Placement>> {
    Ok(layout_on(spec)?.0)
}

/// The result: pixels, where things went, and how true to size.
pub struct Sheet {
    /// The sheet.
    pub image: Image,
    /// The paper as fed (turned when that fitted more).
    pub paper_mm: (f64, f64),
    /// Paper name.
    pub paper_name: String,
    /// The photos placed.
    pub placements: Vec<Placement>,
    /// Photo size, mm.
    pub cell_mm: (f64, f64),
    /// Largest difference between a photo's pixel size and its millimetres, mm.
    pub size_error_mm: f64,
    /// How many photos were turned.
    pub rotated: usize,
    /// Distinct source photos used.
    pub photos: usize,
    /// Notes for the card.
    pub notes: Vec<String>,
}

impl Sheet {
    /// "8 photos, 35 x 45 mm each, A4, 300 DPI, 0.0 mm size error."
    pub fn card_line(&self, dpi: u32) -> String {
        let n = self.placements.len();
        format!(
            "{n} photo{}, {} x {} mm each, {}, {dpi} DPI, {:.1} mm size error.",
            if n == 1 { "" } else { "s" },
            trim(self.cell_mm.0),
            trim(self.cell_mm.1),
            self.paper_name,
            self.size_error_mm
        )
    }
}

fn trim(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

fn px(mm: f64, dpi: u32) -> u32 {
    (mm / 25.4 * f64::from(dpi)).round().max(1.0) as u32
}

fn rotate_cw(img: &Image) -> Image {
    let (w, h) = (img.width as usize, img.height as usize);
    let Samples::U8(v) = &img.samples else {
        return img.clone();
    };
    let ch = img.layout.channels();
    let mut out = vec![0u8; v.len()];
    for y in 0..h {
        for x in 0..w {
            // (x, y) goes to (h - 1 - y, x) in an image h wide and w tall.
            let o = (x * h + (h - 1 - y)) * ch;
            let i = (y * w + x) * ch;
            out[o..o + ch].copy_from_slice(&v[i..i + ch]);
        }
    }
    Image {
        width: h as u32,
        height: w as u32,
        layout: img.layout,
        samples: Samples::U8(out),
        icc: img.icc.clone(),
    }
}

/// Fit a photo to exactly `w` by `h` pixels. A photo of a different shape is cut to the shape
/// from the middle (never stretched).
fn fit_photo(img: &Image, w: u32, h: u32, notes: &mut Vec<String>) -> Result<Image> {
    let (iw, ih) = (img.width, img.height);
    let (a, b) = (f64::from(iw) / f64::from(ih), f64::from(w) / f64::from(h));
    let mut src = img.clone();
    if (a - b).abs() / b > 0.01 {
        let (cw, chh) = if a > b {
            (((f64::from(ih) * b).round() as u32).max(1), ih)
        } else {
            (iw, ((f64::from(iw) / b).round() as u32).max(1))
        };
        src = img.crop((iw - cw) / 2, (ih - chh) / 2, cw, chh);
        let msg = "a photo was a different shape from the print size, so it was cut to shape from the middle (not stretched)".to_string();
        if !notes.contains(&msg) {
            notes.push(msg);
        }
    }
    resample_image(&src, w, h)
}

/// Draw the sheet.
pub fn make(photos: &[Image], spec: &SheetSpec) -> Result<Sheet> {
    if photos.is_empty() {
        return Err(ImageError::BadOption(
            "There is no photo to put on the sheet.".into(),
        ));
    }
    let (mut placements, (pw, ph)) = layout_on(spec)?;
    if placements.is_empty() {
        return Err(ImageError::Failed(format!(
            "A {} x {} mm photo does not fit on {} with a {} mm margin. Use bigger paper or a smaller margin.",
            trim(spec.cell_mm.0),
            trim(spec.cell_mm.1),
            spec.paper.name,
            trim(spec.margin_mm)
        )));
    }
    let max = placements.len();
    let mut notes = Vec::new();
    let want = match spec.copies {
        Some(c) => {
            if c as usize > max {
                return Err(ImageError::Failed(format!(
                    "Only {max} photo{} of {} x {} mm fit on {}. Ask for {max} or fewer, or use bigger paper.",
                    if max == 1 { "" } else { "s" },
                    trim(spec.cell_mm.0),
                    trim(spec.cell_mm.1),
                    spec.paper.name
                )));
            }
            c as usize
        }
        None => {
            // Equal copies of every photo.
            if photos.len() > 1 && max >= photos.len() {
                max - max % photos.len()
            } else {
                max
            }
        }
    };
    placements.truncate(want);
    let (sw, sh) = (px(pw, spec.dpi), px(ph, spec.dpi));
    let mut canvas = vec![255u8; sw as usize * sh as usize * 3];
    let (cw, ch) = (px(spec.cell_mm.0, spec.dpi), px(spec.cell_mm.1, spec.dpi));
    let mut fitted: Vec<Image> = Vec::new();
    for p in photos {
        fitted.push(fit_photo(p, cw, ch, &mut notes)?);
    }
    let mut size_error: f64 = 0.0;
    let mut rotated = 0;
    for (i, pl) in placements.iter().enumerate() {
        let photo = &fitted[i % fitted.len()];
        let turned;
        let img = if pl.rotated {
            rotated += 1;
            turned = rotate_cw(photo);
            &turned
        } else {
            photo
        };
        let (x, y) = (px(pl.x_mm, spec.dpi), px(pl.y_mm, spec.dpi));
        blit(&mut canvas, sw, sh, img, x, y);
        let (wmm, hmm) = (
            f64::from(img.width) / f64::from(spec.dpi) * 25.4,
            f64::from(img.height) / f64::from(spec.dpi) * 25.4,
        );
        size_error = size_error
            .max((wmm - pl.w_mm).abs())
            .max((hmm - pl.h_mm).abs());
        if spec.cut_marks {
            marks(&mut canvas, (sw, sh), (x, y, img.width, img.height), spec);
        }
    }
    if pw > ph && spec.paper.w_mm < spec.paper.h_mm {
        notes.push(format!(
            "the sheet is laid out sideways (landscape) because that fits more {} x {} mm photos; print it on landscape paper, or let the printer turn it",
            trim(spec.cell_mm.0),
            trim(spec.cell_mm.1)
        ));
    }
    if rotated > 0 {
        notes.push(format!(
            "{rotated} photo{} turned a quarter turn to fit more on the sheet",
            if rotated == 1 { " was" } else { "s were" }
        ));
    }
    let image = Image {
        width: sw,
        height: sh,
        layout: Layout::Rgb,
        samples: Samples::U8(canvas),
        icc: None,
    };
    Ok(Sheet {
        image,
        paper_mm: (pw, ph),
        paper_name: spec.paper.name.clone(),
        placements,
        cell_mm: spec.cell_mm,
        size_error_mm: size_error,
        rotated,
        photos: photos.len(),
        notes,
    })
}

fn blit(canvas: &mut [u8], sw: u32, sh: u32, img: &Image, x: u32, y: u32) {
    let Samples::U8(v) = &img.samples else { return };
    let ch = img.layout.channels();
    for row in 0..img.height {
        let dy = y + row;
        if dy >= sh {
            break;
        }
        let w = img.width.min(sw.saturating_sub(x)) as usize;
        let s = row as usize * img.width as usize * ch;
        let d = (dy as usize * sw as usize + x as usize) * 3;
        if ch == 3 {
            canvas[d..d + w * 3].copy_from_slice(&v[s..s + w * 3]);
        } else if ch == 4 {
            // Flatten on white (IPC-8).
            for i in 0..w {
                let a = u32::from(v[s + i * 4 + 3]);
                for c in 0..3 {
                    let f = u32::from(v[s + i * 4 + c]);
                    canvas[d + i * 3 + c] = ((f * a + 255 * (255 - a) + 127) / 255) as u8;
                }
            }
        }
    }
}

/// Thin corner ticks outside a photo's corners.
fn marks(
    canvas: &mut [u8],
    (sw, sh): (u32, u32),
    (x, y, w, h): (u32, u32, u32, u32),
    spec: &SheetSpec,
) {
    let len = px(spec.gutter_mm.clamp(1.5, 2.5), spec.dpi) as i64;
    let (x0, y0, x1, y1) = (
        i64::from(x),
        i64::from(y),
        i64::from(x + w),
        i64::from(y + h),
    );
    let mut dot = |px_: i64, py: i64| {
        if px_ >= 0 && py >= 0 && px_ < i64::from(sw) && py < i64::from(sh) {
            let o = (py as usize * sw as usize + px_ as usize) * 3;
            canvas[o..o + 3].copy_from_slice(&[90, 90, 90]);
        }
    };
    for k in 1..=len {
        // Horizontal ticks on the lines through the top and bottom edges, outside the photo.
        for &(ex, dir) in &[(x0, -1i64), (x1 - 1, 1)] {
            dot(ex + dir * k, y0);
            dot(ex + dir * k, y1 - 1);
        }
        // Vertical ticks on the lines through the left and right edges.
        for &(ey, dir) in &[(y0, -1i64), (y1 - 1, 1)] {
            dot(x0, ey + dir * k);
            dot(x1 - 1, ey + dir * k);
        }
    }
}

