//! Time the first preview of every file given: the fast preview (the file's own thumbnail when
//! it has one) and the sharp one (as large as asked). Prints one line per file.
//!
//! ```text
//! cargo run --release -p panoply-image --example preview_bench -- 512 FILE...
//! ```
use std::time::Instant;

use panoply_image::preview::preview;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let max_side: u32 = a[0].parse().unwrap();
    let mut slowest = (0.0f64, String::new());
    println!(
        "{:44} {:>9} {:>20} {:>9} {:>20}",
        "file", "fast ms", "fast route", "sharp ms", "sharp route"
    );
    for path in &a[1..] {
        let name = path.rsplit('/').next().unwrap().to_string();
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_string);
        let mut cols = Vec::new();
        for fast in [true, false] {
            let f = std::fs::File::open(path).unwrap();
            let t = Instant::now();
            let r = preview(f, ext.as_deref(), max_side, fast);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            match r {
                Ok(p) => {
                    cols.push((
                        ms,
                        format!(
                            "{:?} {}x{}{}",
                            p.route,
                            p.width,
                            p.height,
                            if p.refine { " +" } else { "" }
                        ),
                    ));
                    if fast && ms > slowest.0 {
                        slowest = (ms, name.clone());
                    }
                }
                Err(e) => cols.push((
                    ms,
                    format!("ERR {}", e.to_string().chars().take(30).collect::<String>()),
                )),
            }
        }
        println!(
            "{:44} {:>9.1} {:>20} {:>9.1} {:>20}",
            name, cols[0].0, cols[0].1, cols[1].0, cols[1].1
        );
    }
    println!("slowest fast preview: {:.1} ms ({})", slowest.0, slowest.1);
}
