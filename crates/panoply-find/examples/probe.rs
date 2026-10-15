//! Times queries against a saved index: `probe <index file> <query>...`.

use std::time::Instant;

use panoply_find::{Index, SearchOptions, Segment};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seg = Segment::read_from(std::io::BufReader::new(
        std::fs::File::open(&args[0]).unwrap(),
    ))
    .unwrap();
    let idx = Index::from_segment(seg);
    let opts = SearchOptions {
        limit: 50,
        ..Default::default()
    };
    for q in &args[1..] {
        let mut t = Vec::new();
        let mut n = 0;
        let reps: usize = std::env::var("PROBE_REPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(7);
        for _ in 0..reps {
            let s = Instant::now();
            n = idx.search(q, &opts).len();
            t.push(s.elapsed().as_secs_f64() * 1e3);
        }
        println!(
            "{q:?}: {n} hits, ms per run {:?}",
            t.iter()
                .take(7)
                .map(|x| format!("{x:.2}"))
                .collect::<Vec<_>>()
        );
    }
}
