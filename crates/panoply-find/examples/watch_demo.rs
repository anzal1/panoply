//! Live-update check on a big saved index: `watch_demo <index file> <root> [idle seconds]`.
//!
//! Loads the index, starts the FSEvents updater, creates and removes files under the root and times how
//! long each change takes to become searchable, then sits idle and reports the CPU time the process
//! used meanwhile (the spec asks for near zero).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use panoply_find::{Exclusions, Index, LiveIndex, SearchOptions, Segment};

fn cpu_and_rss() -> (f64, f64) {
    let out = Command::new("ps")
        .args(["-o", "time=,rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let mut it = text.split_whitespace();
    let t = it.next().unwrap_or("0:00.00");
    let (m, s) = t.split_once(':').unwrap_or(("0", "0"));
    let secs = m.parse::<f64>().unwrap_or(0.0) * 60.0 + s.parse::<f64>().unwrap_or(0.0);
    (
        secs,
        it.next().and_then(|r| r.parse::<f64>().ok()).unwrap_or(0.0) / 1024.0,
    )
}

fn count(l: &Arc<RwLock<LiveIndex>>, q: &str) -> usize {
    let g = l.read().unwrap();
    g.index()
        .search(
            q,
            &SearchOptions {
                limit: 100,
                include_hidden: true,
                ..Default::default()
            },
        )
        .len()
}

fn wait_for(l: &Arc<RwLock<LiveIndex>>, q: &str, want: usize) -> Duration {
    let t = Instant::now();
    while count(l, q) != want {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "{q:?} never reached {want} results"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    t.elapsed()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(&args[1]);
    let idle: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let t = Instant::now();
    let seg = Segment::read_from(std::io::BufReader::with_capacity(
        1 << 20,
        std::fs::File::open(&args[0]).unwrap(),
    ))
    .unwrap();
    let live = LiveIndex::new(Index::from_segment(seg), Exclusions::defaults());
    println!(
        "loaded {} entries in {:.0} ms",
        live.index().len(),
        t.elapsed().as_secs_f64() * 1e3
    );
    let shared = Arc::new(RwLock::new(live));
    let updater =
        panoply_find::watch::start(shared.clone(), panoply_find::watch::marker(), None).unwrap();
    std::thread::sleep(Duration::from_millis(500));

    let dir: &Path = &root.join("Documents/Work/Finance/2024");
    let f = dir.join("zzquokka_fresh_file.txt");
    std::fs::write(&f, b"x").unwrap();
    println!(
        "new file searchable after {:?}",
        wait_for(&shared, "zzquokka", 1)
    );
    let g = dir.join("zzquokka_renamed.txt");
    std::fs::rename(&f, &g).unwrap();
    println!(
        "rename searchable after {:?}",
        wait_for(&shared, "zzquokka_renamed", 1)
    );
    std::fs::remove_file(&g).unwrap();
    println!(
        "removal visible after {:?}",
        wait_for(&shared, "zzquokka", 0)
    );
    let d = root.join("zzquokka_dir");
    for i in 0..2000 {
        std::fs::create_dir_all(d.join(format!("sub{}", i % 20))).unwrap();
        std::fs::write(d.join(format!("sub{}/zzfile{i}.dat", i % 20)), b"x").unwrap();
    }
    println!(
        "2000-file burst searchable after {:?}",
        wait_for(&shared, "zzfile", 100)
    );
    std::fs::remove_dir_all(&d).unwrap();
    println!(
        "burst removal visible after {:?}",
        wait_for(&shared, "zzfile", 0)
    );

    std::thread::sleep(Duration::from_secs(3));
    let (cpu0, rss0) = cpu_and_rss();
    let t = Instant::now();
    std::thread::sleep(Duration::from_secs(idle));
    let (cpu1, rss1) = cpu_and_rss();
    println!(
        "idle {:.0} s: CPU time used {:.2} s ({:.4} percent of one core), rss {rss0:.1} -> {rss1:.1} MB",
        t.elapsed().as_secs_f64(),
        cpu1 - cpu0,
        (cpu1 - cpu0) / t.elapsed().as_secs_f64() * 100.0
    );
    drop(updater);
}
