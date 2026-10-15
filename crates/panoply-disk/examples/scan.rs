//! Scan a folder and print what the engine found: the benchmark front end.
//!
//!     cargo run --release -p panoply-disk --example scan -- <path> [--threads N] [--portable]
//!         [--no-clones] [--cross] [--top N]
//!
//! Wrap it in `/usr/bin/time -l` (macOS) or `-v` (Linux) for peak memory.

use std::path::PathBuf;
use std::time::Instant;

use panoply_disk::{BackendChoice, Kind, ScanOptions, scan};

fn main() {
    let mut path = None;
    let mut opts = ScanOptions::default();
    let mut top = 0usize;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--threads" => opts.threads = args.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--portable" => opts.backend = BackendChoice::Portable,
            "--no-clones" => opts.clone_aware = false,
            "--no-fold" => opts.collapse_small_files = false,
            "--cross" => opts.cross_volumes = true,
            "--top" => top = args.next().and_then(|v| v.parse().ok()).unwrap_or(10),
            _ => path = Some(PathBuf::from(a)),
        }
    }
    let path = path
        .expect("usage: scan <path> [--threads N] [--portable] [--no-clones] [--cross] [--top N]");
    let t = Instant::now();
    let s = scan(&path, &opts).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let wall = t.elapsed();
    let r = &s.report;
    println!("backend        {}", r.backend);
    println!("threads        {}", r.threads);
    println!(
        "scan time      {:.3} s (wall {:.3} s)",
        r.elapsed.as_secs_f64(),
        wall.as_secs_f64()
    );
    println!("files          {}", r.files);
    println!("folders        {}", r.dirs);
    println!("tree entries   {}", r.nodes);
    println!("allocated KiB  {}", r.total_bytes.div_ceil(1024));
    println!(
        "tree heap      {:.1} MiB ({:.1} bytes per entry)",
        s.tree.heap_bytes() as f64 / 1048576.0,
        s.tree.heap_bytes() as f64 / s.tree.len().max(1) as f64
    );
    println!(
        "hard links     {} entries, {} KiB not counted again",
        r.hard_link_entries,
        r.hard_link_bytes_saved / 1024
    );
    println!(
        "clones         {} KiB not counted again",
        r.clone_bytes_saved / 1024
    );
    println!("sparse/compr.  {}", r.sparse_or_compressed);
    println!("other volumes  {} not entered", r.other_volumes);
    println!(
        "unreadable     {} ({} need Full Disk Access)",
        r.skipped.len(),
        r.skipped_count(panoply_disk::SkipReason::FullDiskAccess)
    );
    if let Some(v) = &r.volume {
        println!(
            "volume         total {} GiB, free {} GiB, purgeable {:?} GiB, covers volume {}",
            v.total >> 30,
            v.free >> 30,
            v.purgeable.map(|p| p >> 30),
            v.covers_volume
        );
    }
    for id in s.tree.largest(top, Kind::File) {
        println!(
            "{:>12} KiB  {}",
            s.tree.size(id) / 1024,
            s.tree.path(id).display()
        );
    }
}
