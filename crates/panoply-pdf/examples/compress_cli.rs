//! `compress_cli in.pdf out.pdf [under_mb]`: one compression, with the report.
use std::fs::File;
use std::io::BufWriter;

use panoply_core::CancelToken;
use panoply_pdf::compress::{self, Options, Target};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let opened = panoply_pdf::open(&File::open(&a[1]).unwrap()).unwrap();
    let pages = opened.doc.pages().unwrap();
    let mut opts = Options::default();
    if let Some(mb) = a.get(3) {
        opts.target = Target::UnderBytes((mb.parse::<f64>().unwrap() * 1_000_000.0) as u64);
    }
    let t = std::time::Instant::now();
    let rep = compress::compress(
        &opened.doc,
        &pages,
        &opts,
        &CancelToken::new(),
        &mut |f, l| eprint!("\r{:>3.0}% {l:<24}", f * 100.0),
        Box::new(BufWriter::new(File::create(&a[2]).unwrap())),
    )
    .unwrap();
    eprintln!();
    println!("{rep:#?}");
    println!("{} ms", t.elapsed().as_millis());
}
