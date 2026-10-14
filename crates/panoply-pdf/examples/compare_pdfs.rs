//! `compare_pdfs a.pdf b.pdf`: SSIM of the first, middle and last page of two documents,
//! drawn with pdfium at 100 dpi (the same measure as compress_report).
use std::fs::File;

use panoply_pdf::render::{RenderDoc, compare};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let da = RenderDoc::open(File::open(&a[1]).unwrap(), None).unwrap();
    let db = match RenderDoc::open(File::open(&a[2]).unwrap(), None) {
        Ok(d) => d,
        Err(e) => {
            println!("ERR open: {e}");
            return;
        }
    };
    if da.page_count() != db.page_count() {
        println!("PAGECOUNT {} {}", da.page_count(), db.page_count());
    }
    match compare(&da, &db, 100.0, 3) {
        Ok(l) => {
            let pages: Vec<String> = l
                .pages
                .iter()
                .map(|(i, s)| format!("p{}={:.4}", i + 1, s))
                .collect();
            println!("min={:.4} mean={:.4} {}", l.min, l.mean, pages.join(" "));
        }
        Err(e) => println!("ERR compare: {e}"),
    }
}
