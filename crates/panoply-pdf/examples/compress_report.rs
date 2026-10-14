//! `cargo run --release -p panoply-pdf --example compress_report [-- --under-mb 1] [--tier large]`
//!
//! Compresses every default-tier corpus PDF and prints Markdown tables: size before and after,
//! the lowest SSIM among recoded images, and the lowest SSIM of the pages drawn by pdfium before
//! and after. Outputs go to a temporary folder and are removed.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use panoply_core::CancelToken;
use panoply_pdf::PdfError;
use panoply_pdf::compress::{self, MIN_SAVING, Options, Target};
use panoply_pdf::render::{RenderDoc, compare};

struct Row {
    id: String,
    class: &'static str,
    license: String,
    before: u64,
    after: u64,
    images: usize,
    recoded: usize,
    kept_kinds: String,
    img_ssim: Option<f64>,
    page_min: Option<f64>,
    page_mean: Option<f64>,
    ms: u128,
    reached: Option<bool>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let under_mb: Option<f64> = args
        .iter()
        .position(|a| a == "--under-mb")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());
    let tier: String = args
        .iter()
        .position(|a| a == "--tier")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "default".into());
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    let toml: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.join("pdf.toml")).unwrap()).unwrap();
    let tmp = std::env::temp_dir().join(format!("panoply-compress-report-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut rows: Vec<Row> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for f in toml["file"].as_array().unwrap() {
        let id = f["id"].as_str().unwrap().to_string();
        let path = root.join("cache/pdf").join(f["file"].as_str().unwrap());
        if !path.is_file() || f.get("tier").and_then(|t| t.as_str()).unwrap_or("default") != tier {
            continue;
        }
        let tags: Vec<&str> = f["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| t.as_str()).collect())
            .unwrap_or_default();
        let license = f
            .get("license")
            .and_then(|l| l.as_str())
            .unwrap_or("")
            .to_string();
        let opened = match panoply_pdf::open(&File::open(&path).unwrap()) {
            Ok(o) => o,
            Err(e) => {
                skipped.push((id, e.to_string()));
                continue;
            }
        };
        if let Err(e) = opened.doc.require_unencrypted() {
            skipped.push((id, e.to_string()));
            continue;
        }
        let pages = match opened.doc.pages() {
            Ok(p) => p,
            Err(e) => {
                skipped.push((id, e.to_string()));
                continue;
            }
        };
        let out = tmp.join(format!("{id}.pdf"));
        let opts = Options {
            target: under_mb.map_or(Target::Smallest, |mb| Target::UnderBytes((mb * 1e6) as u64)),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let rep = match compress::compress(
            &opened.doc,
            &pages,
            &opts,
            &CancelToken::new(),
            &mut |_, _| {},
            Box::new(BufWriter::new(File::create(&out).unwrap())),
        ) {
            Ok(r) => r,
            Err(PdfError::NotPdf) => continue,
            Err(e) => {
                skipped.push((id, e.to_string()));
                continue;
            }
        };
        let ms = t.elapsed().as_millis();
        let (mut page_min, mut page_mean) = (None, None);
        if let (Ok(a), Ok(b)) = (
            RenderDoc::open(File::open(&path).unwrap(), None),
            RenderDoc::open(File::open(&out).unwrap(), None),
        ) && a.page_count() == b.page_count()
            && let Ok(l) = compare(&a, &b, 100.0, 6)
        {
            page_min = Some(l.min);
            page_mean = Some(l.mean);
        }
        let image_share = if rep.original_bytes == 0 {
            0.0
        } else {
            rep.image_bytes_before as f64 / rep.original_bytes as f64
        };
        let class = if tags.contains(&"scanned")
            || tags.contains(&"no-text-layer")
            || tags.contains(&"ccitt")
            || tags.contains(&"jbig2")
        {
            "scanned"
        } else if tags.contains(&"forms")
            || tags.contains(&"acroform")
            || tags.contains(&"fillable")
            || tags.contains(&"xfa")
        {
            "forms"
        } else if image_share >= 0.6 {
            "image-heavy"
        } else if image_share >= 0.15 {
            "mixed"
        } else {
            "text-heavy"
        };
        rows.push(Row {
            id,
            class,
            license,
            before: rep.original_bytes,
            after: rep.output_bytes,
            images: rep.images,
            recoded: rep.images_lossy + rep.images_lossless,
            kept_kinds: rep
                .images_kept
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", "),
            img_ssim: rep.min_image_ssim,
            page_min,
            page_mean,
            ms,
            reached: rep.target_reached,
        });
        let _ = std::fs::remove_file(&out);
    }
    let _ = std::fs::remove_dir_all(&tmp);

    println!(
        "| file | class | license | before | after | saved | images | recoded | images left alone | lowest image SSIM | lowest page SSIM | mean page SSIM | ms | target met |"
    );
    println!("|---|---|---|---:|---:|---:|---:|---:|---|---:|---:|---:|---:|---|");
    for r in &rows {
        let saved = 1.0 - r.after as f64 / r.before as f64;
        println!(
            "| {} | {} | {} | {} | {} | {:.1}%{} | {} | {} | {} | {} | {} | {} | {} | {} |",
            r.id,
            r.class,
            r.license,
            r.before,
            r.after,
            saved * 100.0,
            if saved < MIN_SAVING {
                " (original kept)"
            } else {
                ""
            },
            r.images,
            r.recoded,
            if r.kept_kinds.is_empty() {
                "-"
            } else {
                &r.kept_kinds
            },
            r.img_ssim.map_or("-".into(), |v| format!("{v:.3}")),
            r.page_min.map_or("-".into(), |v| format!("{v:.3}")),
            r.page_mean.map_or("-".into(), |v| format!("{v:.3}")),
            r.ms,
            r.reached.map_or("-", |b| if b { "yes" } else { "no" })
        );
    }
    let mut savings: Vec<f64> = rows
        .iter()
        .map(|r| (1.0 - r.after as f64 / r.before as f64).max(0.0))
        .collect();
    savings.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = if savings.is_empty() {
        0.0
    } else {
        savings[savings.len() / 2]
    };
    let total_before: u64 = rows.iter().map(|r| r.before).sum();
    let total_after: u64 = rows.iter().map(|r| r.after.min(r.before)).sum();
    let lost: Vec<&Row> = rows.iter().filter(|r| r.after >= r.before).collect();
    let small: Vec<&Row> = rows
        .iter()
        .filter(|r| (1.0 - r.after as f64 / r.before as f64) < MIN_SAVING)
        .collect();
    println!();
    println!(
        "files: {}  median saving {:.1}%  total {} -> {} bytes ({:.1}% smaller)",
        rows.len(),
        median * 100.0,
        total_before,
        total_after,
        (1.0 - total_after as f64 / total_before as f64) * 100.0
    );
    println!(
        "output larger than input: {:?}",
        lost.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    println!(
        "under 2% (original kept): {:?}",
        small.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    println!("skipped: {skipped:?}");
}
