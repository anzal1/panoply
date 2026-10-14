//! `cargo run -p panoply-pdf --example inspect -- file.pdf ...` prints what the engine sees.

use std::fs::File;

use panoply_pdf::{Doc, PdfError, source::Source};

fn main() {
    for path in std::env::args().skip(1) {
        let f = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        let strict = Doc::open(Source::from_file(f.try_clone().unwrap()).unwrap());
        let note = match &strict {
            Ok(_) => "opens as is".to_string(),
            Err(e) => format!("strict open failed: {e}"),
        };
        match panoply_pdf::open(&f) {
            Ok(o) => {
                let pages = o.doc.pages().map(|p| p.len()).unwrap_or(0);
                println!(
                    "{path}\n  {note}\n  repaired={} pages={pages} version={:?} encrypted={:?} signatures={:?} objects={}",
                    o.repaired,
                    o.doc.version(),
                    o.doc.encryption(),
                    o.doc.signatures(),
                    o.doc.object_count()
                );
            }
            Err(PdfError::Encrypted) => println!("{path}\n  {note}\n  encrypted"),
            Err(e) => println!("{path}\n  {note}\n  {e}"),
        }
    }
}
