//! Tesseract through a `tesseract` program on the machine, for the languages the OS engines and
//! PP-OCR do not read (Bengali is the one that matters here).
//!
//! Panoply does not ship Tesseract's program. It looks for one in `PANOPLY_TESSERACT`, then on
//! the `PATH`, and brings its own checksum-pinned language data (Apache-2.0) so the answer does
//! not depend on whatever data the machine has. Words and their boxes come from the TSV output.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::{OcrError, Result};
use crate::lang::Lang;
use crate::models::{self, Asset, Fetch};
use crate::types::{EngineKind, Line, OcrImage, OcrPage, Rect, Word};

/// Languages with pinned data.
fn asset_for(l: &Lang) -> Option<&'static Asset> {
    match l.tesseract() {
        "ben" => Some(&models::TESSDATA_BEN),
        "eng" => Some(&models::TESSDATA_ENG),
        "hin" => Some(&models::TESSDATA_HIN),
        _ => None,
    }
}

/// The program, when there is one.
pub fn program() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = match std::env::var_os("PANOPLY_TESSERACT") {
        Some(p) => vec![PathBuf::from(p)],
        None => vec![PathBuf::from("tesseract")],
    };
    candidates.into_iter().find(|c| {
        Command::new(c)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// True when the program is there and every language has pinned data.
pub fn covers(langs: &[Lang]) -> bool {
    !langs.is_empty() && langs.iter().all(|l| asset_for(l).is_some()) && program().is_some()
}

/// Encode as PNG.
fn png_bytes(img: &OcrImage) -> Result<Vec<u8>> {
    png_raw(&img.data, img.width, img.height, img.channels)
}

fn png_raw(data: &[u8], width: u32, height: u32, channels: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(if channels == 1 {
            png::ColorType::Grayscale
        } else {
            png::ColorType::Rgb
        });
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc
            .write_header()
            .map_err(|e| OcrError::Engine(format!("Could not prepare the picture ({e}).")))?;
        w.write_image_data(data)
            .map_err(|e| OcrError::Engine(format!("Could not prepare the picture ({e}).")))?;
    }
    Ok(out)
}

/// Read one image.
pub fn recognize(img: &OcrImage, langs: &[Lang], fetch: &mut Fetch<'_>) -> Result<OcrPage> {
    let prog = program().ok_or_else(|| {
        OcrError::Unsupported(
            "Reading this language needs Tesseract, which is not installed. Install it, or set PANOPLY_TESSERACT to its program.".into(),
        )
    })?;
    // The pinned language files sit in the OCR cache folder under their Tesseract names, so that
    // folder is the data folder.
    let data_dir = models::cache_dir();
    let mut codes: Vec<&str> = Vec::new();
    for l in langs {
        let a = asset_for(l).ok_or_else(|| {
            OcrError::Unsupported(format!("Panoply has no Tesseract data for {l}."))
        })?;
        models::ensure(a, fetch)?;
        codes.push(l.tesseract());
    }
    let dir = std::env::temp_dir().join(format!("panoply-tess-{}-{}", std::process::id(), nonce()));
    std::fs::create_dir_all(&dir)?;
    let result = (|| -> Result<OcrPage> {
        let input = dir.join("page.png");
        std::fs::write(&input, png_bytes(img)?)?;
        let out = Command::new(&prog)
            .arg(&input)
            .arg("stdout")
            .arg("--tessdata-dir")
            .arg(&data_dir)
            .arg("-l")
            .arg(codes.join("+"))
            .args(["--psm", "3", "-c", "tessedit_create_tsv=1"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| OcrError::Engine(format!("Tesseract could not be started ({e}).")))?;
        if !out.status.success() {
            return Err(OcrError::Engine(
                "Tesseract could not read this picture.".into(),
            ));
        }
        let tsv = String::from_utf8_lossy(&out.stdout);
        Ok(OcrPage {
            width: img.width,
            height: img.height,
            lines: crate::layout::reading_order(parse_tsv(&tsv)),
            engine: EngineKind::Tesseract,
            languages: langs.to_vec(),
        })
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Height lines are straightened to before Tesseract reads them, and the white border around
/// each. Tesseract's models are happiest with text around this size (an x-height of 20 to 30
/// pixels) and a margin.
const LINE_H: usize = 72;
const LINE_PAD: usize = 20;

/// Read pictures of single lines, found by another engine's detector (the PP-OCR detector),
/// one Tesseract run for all of them (page segmentation mode 13, "raw line": mode 7 returned nothing at all for some clean Bengali lines, measured with Tesseract 5.5.3). Layout
/// analysis is Tesseract's weak point on small pictures (it can drop whole lines), and line
/// reading is its strong one, so the detector finds the lines and Tesseract reads them.
pub fn recognize_lines(
    img: &OcrImage,
    rects: &[crate::ppocr::Oriented],
    langs: &[Lang],
    fetch: &mut Fetch<'_>,
) -> Result<OcrPage> {
    let prog =
        program().ok_or_else(|| OcrError::Unsupported("Tesseract is not installed.".into()))?;
    let data_dir = models::cache_dir();
    let mut codes: Vec<&str> = Vec::new();
    for l in langs {
        let a = asset_for(l).ok_or_else(|| {
            OcrError::Unsupported(format!("Panoply has no Tesseract data for {l}."))
        })?;
        models::ensure(a, fetch)?;
        codes.push(l.tesseract());
    }
    let dir = std::env::temp_dir().join(format!("panoply-tess-{}-{}", std::process::id(), nonce()));
    std::fs::create_dir_all(&dir)?;
    let result = (|| -> Result<OcrPage> {
        let mut list = String::new();
        let mut sizes: Vec<(usize, usize)> = Vec::with_capacity(rects.len());
        for (i, r) in rects.iter().enumerate() {
            let (px, w, h) = crate::ppocr::imgops::gray_crop(img, r, LINE_H, LINE_PAD);
            let name = format!("l{i}.png");
            std::fs::write(dir.join(&name), png_raw(&px, w as u32, h as u32, 1)?)?;
            list.push_str(&name);
            list.push('\n');
            sizes.push((w, h));
        }
        std::fs::write(dir.join("list.txt"), list)?;
        let out = Command::new(&prog)
            .current_dir(&dir)
            .arg("list.txt")
            .arg("stdout")
            .arg("--tessdata-dir")
            .arg(&data_dir)
            .arg("-l")
            .arg(codes.join("+"))
            .args(["--psm", "13", "-c", "tessedit_create_tsv=1"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|e| OcrError::Engine(format!("Tesseract could not be started ({e}).")))?;
        if !out.status.success() {
            return Err(OcrError::Engine(
                "Tesseract could not read this picture.".into(),
            ));
        }
        let tsv = String::from_utf8_lossy(&out.stdout);
        // Words of each line picture, grouped by the page number of the TSV (one per picture).
        let mut by_page: Vec<Vec<Word>> = vec![Vec::new(); rects.len()];
        for row in tsv.lines().skip(1) {
            let f: Vec<&str> = row.splitn(12, '\t').collect();
            if f.len() < 12 || f[0] != "5" {
                continue;
            }
            let num = |i: usize| f[i].trim().parse::<f32>().ok();
            let (Some(page), Some(x), Some(w), Some(conf)) = (num(1), num(6), num(8), num(10))
            else {
                continue;
            };
            let text = f[11].trim();
            let idx = page as usize;
            if text.is_empty() || conf < 0.0 || idx == 0 || idx > rects.len() {
                continue;
            }
            let (r, (cw, _)) = (&rects[idx - 1], sizes[idx - 1]);
            // Position along the straightened line: remove the border, scale to the rectangle.
            let inner_w = (cw - 2 * LINE_PAD) as f32;
            let u0 = (x - LINE_PAD as f32) / inner_w * r.w - r.w / 2.0;
            let u1 = (x + w - LINE_PAD as f32) / inner_w * r.w - r.w / 2.0;
            let (bx, by, bw, bh) =
                crate::ppocr::imgops::span_bounds(r, u0.max(-r.w / 2.0), u1.min(r.w / 2.0));
            by_page[idx - 1].push(Word {
                text: text.to_string(),
                bbox: Rect {
                    x: bx,
                    y: by,
                    w: bw,
                    h: bh,
                },
                confidence: (conf / 100.0).clamp(0.0, 1.0),
            });
        }
        let lines: Vec<Line> = by_page.into_iter().filter_map(Line::from_words).collect();
        Ok(OcrPage {
            width: img.width,
            height: img.height,
            lines: crate::layout::reading_order(lines),
            engine: EngineKind::Tesseract,
            languages: langs.to_vec(),
        })
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Words of Tesseract's TSV output grouped into lines (by block, paragraph and line number).
pub fn parse_tsv(tsv: &str) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut key: Option<(u32, u32, u32)> = None;
    let mut cur: Vec<Word> = Vec::new();
    for row in tsv.lines().skip(1) {
        let f: Vec<&str> = row.splitn(12, '\t').collect();
        if f.len() < 12 || f[0] != "5" {
            continue;
        }
        let num = |i: usize| f[i].trim().parse::<f32>().ok();
        let (Some(b), Some(p), Some(l)) = (num(2), num(3), num(4)) else {
            continue;
        };
        let (Some(x), Some(y), Some(w), Some(h), Some(conf)) =
            (num(6), num(7), num(8), num(9), num(10))
        else {
            continue;
        };
        let text = f[11].trim();
        if text.is_empty() || conf < 0.0 {
            continue;
        }
        let k = (b as u32, p as u32, l as u32);
        if key != Some(k) {
            if let Some(line) = Line::from_words(std::mem::take(&mut cur)) {
                lines.push(line);
            }
            key = Some(k);
        }
        cur.push(Word {
            text: text.to_string(),
            bbox: Rect { x, y, w, h },
            confidence: (conf / 100.0).clamp(0.0, 1.0),
        });
    }
    if let Some(line) = Line::from_words(cur) {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tsv_words_group_into_lines() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
1\t1\t0\t0\t0\t0\t0\t0\t800\t600\t-1\t\n\
5\t1\t1\t1\t1\t1\t10\t20\t50\t14\t96.5\thello\n\
5\t1\t1\t1\t1\t2\t70\t20\t60\t14\t90\tworld\n\
5\t1\t1\t1\t2\t1\t10\t50\t40\t14\t30\tbye\n\
5\t1\t1\t1\t2\t2\t60\t50\t40\t14\t-1\t\n";
        let l = parse_tsv(tsv);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].text(), "hello world");
        assert_eq!(l[0].bbox.x, 10.0);
        assert!((l[0].words[0].confidence - 0.965).abs() < 1e-4);
        assert_eq!(l[1].text(), "bye");
    }

    #[test]
    fn only_languages_with_pinned_data_are_covered() {
        // Whether the program exists decides `covers`, but data coverage is fixed.
        assert!(asset_for(&Lang::parse("bn").unwrap()).is_some());
        assert!(asset_for(&Lang::parse("hi").unwrap()).is_some());
        assert!(asset_for(&Lang::parse("fr").unwrap()).is_none());
        assert!(!covers(&[]));
    }
}
