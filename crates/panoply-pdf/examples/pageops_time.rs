//! `pageops_time in.pdf [iterations]`: time the identity copy in both catalog modes.
use std::fs::File;

use panoply_core::CancelToken;
use panoply_pdf::pageops::{BuildOptions, CatalogMode, Input, OutPage, build};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let n: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(5);
    let opened = panoply_pdf::open(&File::open(&a[1]).unwrap()).unwrap();
    let pages = opened.doc.pages().unwrap();
    let plan: Vec<OutPage> = (0..pages.len())
        .map(|index| OutPage {
            input: 0,
            index,
            rotate: 0,
        })
        .collect();
    for mode in [CatalogMode::Subset, CatalogMode::Full] {
        let t = std::time::Instant::now();
        let mut bytes = 0;
        for _ in 0..n {
            let inputs = [Input {
                doc: &opened.doc,
                pages: pages.as_slice().into(),
                title: "t".into(),
            }];
            let r = build(
                std::io::BufWriter::new(std::io::sink()),
                &inputs,
                &plan,
                &BuildOptions {
                    mode,
                    ..Default::default()
                },
                &CancelToken::new(),
                &mut |_, _| {},
            )
            .unwrap();
            bytes = r.bytes;
        }
        println!(
            "{mode:?}: {:.1} ms per copy, {} pages, {bytes} bytes out",
            t.elapsed().as_secs_f64() * 1000.0 / n as f64,
            pages.len()
        );
    }
}
