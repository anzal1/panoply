//! Benchmark for the index: `bench <root> [options]`.
//!
//!   --threads N        walker threads (default 8)
//!   --runs N           index builds to time (default 3)
//!   --no-mtime         skip the per-entry lstat
//!   --ignore           also build with the `ignore` crate's walker (needs --features ignore-walker)
//!   --exclude-dir P    never enter P (repeatable)
//!   --compare          time `rg --files` (the walker fd uses) and `mdfind -name` on the same root
//!   --queries N        samples per query class (default 2000)
//!
//! Prints index build time, memory, save and load time, per-keystroke latency (p50, p90, p99, max) for
//! prefix, substring and fuzzy queries of 1, 2, 3 and 8 characters, an exhaustive sweep of every one and
//! two character query, and a typing session.

use std::hint::black_box;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use panoply_find::{Exclusions, Index, SearchOptions, WalkOptions};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn rss_mb() -> f64 {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok();
    out.and_then(|o| {
        String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse::<f64>()
            .ok()
    })
    .map_or(0.0, |kb| kb / 1024.0)
}

fn pct(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() as f64 * p) as usize).min(sorted.len() - 1)]
}

fn ms(d: Duration) -> String {
    let us = d.as_secs_f64() * 1e6;
    if us < 1000.0 {
        format!("{us:7.0}us")
    } else {
        format!("{:6.2}ms", us / 1000.0)
    }
}

fn run_class(idx: &Index, label: &str, queries: &[String], opts: &SearchOptions) {
    if queries.is_empty() {
        println!("{label:<22} (no samples)");
        return;
    }
    for q in queries.iter().take(300) {
        black_box(idx.search(q, opts));
    }
    // Each query runs three times back to back. `raw` is the first run; `best` is the fastest of the
    // three, which filters out the scheduler taking the core away mid-query (this machine is shared).
    let mut raw = Vec::with_capacity(queries.len());
    let mut best = Vec::with_capacity(queries.len());
    let mut results = 0usize;
    let mut slow: Vec<(Duration, &String)> = Vec::new();
    for q in queries {
        let mut b = Duration::MAX;
        for rep in 0..3 {
            let t = Instant::now();
            let r = idx.search(black_box(q), opts);
            let el = t.elapsed();
            if rep == 0 {
                raw.push(el);
                results += r.len();
            }
            b = b.min(el);
            black_box(&r);
        }
        best.push(b);
        slow.push((b, q));
    }
    slow.sort_by_key(|a| std::cmp::Reverse(a.0));
    raw.sort();
    best.sort();
    let over10 = raw.iter().filter(|t| t.as_millis() >= 10).count();
    println!(
        "{label:<22} n={:<5} raw p50 {} p99 {} max {} (>=10ms: {over10:<3}) | best-of-3 p50 {} p99 {} max {} | avg results {:.1}",
        raw.len(),
        ms(pct(&raw, 0.5)),
        ms(pct(&raw, 0.99)),
        ms(*raw.last().unwrap()),
        ms(pct(&best, 0.5)),
        ms(pct(&best, 0.99)),
        ms(*best.last().unwrap()),
        results as f64 / queries.len() as f64
    );
    if best.last().is_some_and(|m| m.as_millis() >= 5) {
        println!(
            "    slowest: {}",
            slow.iter()
                .take(4)
                .map(|(t, q)| format!("{q:?} {}", ms(*t).trim()))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(args.first().expect("usage: bench <root> [options]"));
    let mut threads = 8;
    let mut runs = 3;
    let mut with_mtime = true;
    let mut ignore = false;
    let mut compare = false;
    let mut portable = false;
    let mut nq = 2000usize;
    let mut save: Option<PathBuf> = None;
    let mut load: Option<PathBuf> = None;
    let mut skip_dirs: Vec<PathBuf> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--threads" => {
                threads = args[i + 1].parse().unwrap();
                i += 1;
            }
            "--runs" => {
                runs = args[i + 1].parse().unwrap();
                i += 1;
            }
            "--queries" => {
                nq = args[i + 1].parse().unwrap();
                i += 1;
            }
            "--exclude-dir" => {
                skip_dirs.push(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--save" => {
                save = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--load" => {
                load = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--no-mtime" => with_mtime = false,
            "--portable" => portable = true,
            "--ignore" => ignore = true,
            "--compare" => compare = true,
            other => panic!("unknown option {other}"),
        }
        i += 1;
    }

    let mut exclude = Exclusions::defaults();
    for d in skip_dirs {
        exclude = exclude.with_path(d);
    }
    let opts = WalkOptions {
        threads,
        with_mtime,
        exclude,
        portable_reader: portable,
    };
    println!(
        "root {}  threads {threads}  mtime {with_mtime}  rss at start {:.1} MB",
        root.display(),
        rss_mb()
    );

    let idx = if let Some(path) = &load {
        let t = Instant::now();
        let seg = panoply_find::Segment::read_from(std::io::BufReader::with_capacity(
            1 << 20,
            std::fs::File::open(path).unwrap(),
        ))
        .unwrap();
        println!(
            "loaded {} entries from {} in {:.0} ms, rss {:.1} MB, columns {:.1} MB",
            seg.len(),
            path.display(),
            t.elapsed().as_secs_f64() * 1e3,
            rss_mb(),
            seg.heap_bytes() as f64 / 1048576.0
        );
        Index::from_segment(seg)
    } else {
        // ---- build -----------------------------------------------------------------------------------
        let mut best: Option<(Index, Duration)> = None;
        for run in 0..runs {
            let t = Instant::now();
            let (idx, st) = Index::build(std::slice::from_ref(&root), &opts).unwrap();
            let total = t.elapsed();
            println!(
                "build #{run}: {:7.3}s total (walk {:.3}s + merge {:.3}s)  entries {}  dirs read {}  excluded {}  errors {}  rss {:.1} MB",
                total.as_secs_f64(),
                st.walk.as_secs_f64(),
                st.merge.as_secs_f64(),
                st.entries,
                st.dirs_read,
                st.dirs_excluded,
                st.errors,
                rss_mb()
            );
            if best.as_ref().is_none_or(|(_, b)| total < *b) {
                best = Some((idx, total));
            }
        }
        let (idx, best_time) = best.unwrap();
        println!(
            "best build {:.3}s ({:.0} entries/s)",
            best_time.as_secs_f64(),
            idx.len() as f64 / best_time.as_secs_f64()
        );

        #[cfg(feature = "ignore-walker")]
        if ignore {
            let mut times = Vec::new();
            for run in 0..runs {
                let (seg, t) = panoply_find::walk_ignore(&root, &opts).unwrap();
                println!(
                    "ignore-crate walk #{run}: {:7.3}s  entries {}  rss {:.1} MB",
                    t.as_secs_f64(),
                    seg.len(),
                    rss_mb()
                );
                times.push(t);
            }
            times.sort();
            println!("best ignore-crate build {:.3}s", times[0].as_secs_f64());
        }
        #[cfg(not(feature = "ignore-walker"))]
        if ignore {
            println!("--ignore needs --features ignore-walker");
        }

        // ---- memory ----------------------------------------------------------------------------------
        let n = idx.len();
        let heap = idx.heap_bytes();
        println!(
            "\nmemory: columns {:.1} MB for {n} entries = {:.1} bytes/entry (limit: 100 MB per 1M)",
            heap as f64 / 1048576.0,
            heap as f64 / n as f64
        );

        // ---- save and load ---------------------------------------------------------------------------
        let tmp =
            std::env::temp_dir().join(format!("panoply-find-bench-{}.idx", std::process::id()));
        let t = Instant::now();
        idx.segments()[0]
            .write_to(std::io::BufWriter::new(
                std::fs::File::create(&tmp).unwrap(),
            ))
            .unwrap();
        let wt = t.elapsed();
        let sz = std::fs::metadata(&tmp).unwrap().len();
        let t = Instant::now();
        let loaded = panoply_find::Segment::read_from(std::io::BufReader::with_capacity(
            1 << 20,
            std::fs::File::open(&tmp).unwrap(),
        ))
        .unwrap();
        let lt = t.elapsed();
        println!(
            "save {:.0} ms, file {:.1} MB, cold load {:.0} ms (spec: under 300 ms), loaded entries {}",
            wt.as_secs_f64() * 1e3,
            sz as f64 / 1048576.0,
            lt.as_secs_f64() * 1e3,
            loaded.len()
        );
        drop(loaded);
        let _ = std::fs::remove_file(&tmp);
        println!("rss with index resident: {:.1} MB", rss_mb());

        if let Some(path) = &save {
            idx.segments()[0]
                .write_to(std::io::BufWriter::new(
                    std::fs::File::create(path).unwrap(),
                ))
                .unwrap();
            println!("saved index to {}", path.display());
        }
        idx
    };
    let n = idx.len();

    // ---- queries ---------------------------------------------------------------------------------
    let sopts = SearchOptions {
        limit: 50,
        ..Default::default()
    };
    let mut rng = Rng(0xBADC0FFEE0DDF00D);
    let mut folded_names: Vec<Vec<u8>> = Vec::new();
    while folded_names.len() < 20_000 {
        let id = rng.below(n) as u32;
        if let Some(e) = idx.entry(id)
            && !e.is_dir
            && e.name.is_ascii()
            && e.name.len() >= 12
        {
            folded_names.push(e.name.to_ascii_lowercase().into_bytes());
        }
    }
    println!("\nper-keystroke latency, limit 50, single thread, warm (rows are query classes)");
    for l in [1usize, 2, 3, 8] {
        let mut prefix = Vec::new();
        let mut substr = Vec::new();
        let mut fuzzy = Vec::new();
        let mut miss = Vec::new();
        for _ in 0..nq {
            let name = &folded_names[rng.below(folded_names.len())];
            prefix.push(String::from_utf8_lossy(&name[..l]).into_owned());
            let start = 1 + rng.below(name.len() - l);
            substr.push(String::from_utf8_lossy(&name[start..start + l]).into_owned());
            if l >= 2 {
                // l letters of the name in order, with at least one skipped letter between two of them.
                for _ in 0..20 {
                    let mut pos: Vec<usize> = (0..l).map(|_| rng.below(name.len())).collect();
                    pos.sort();
                    pos.dedup();
                    if pos.len() == l && pos.windows(2).any(|w| w[1] > w[0] + 1) {
                        let q: Vec<u8> = pos.iter().map(|&p| name[p]).collect();
                        let q = String::from_utf8_lossy(&q).into_owned();
                        if !q.contains(' ') && !name.windows(l).any(|w| w == q.as_bytes()) {
                            fuzzy.push(q);
                            break;
                        }
                    }
                }
            }
            let m: String = (0..l)
                .map(|_| (b'a' + rng.below(26) as u8) as char)
                .collect();
            miss.push(m);
        }
        run_class(&idx, &format!("{l}-char prefix"), &prefix, &sopts);
        run_class(&idx, &format!("{l}-char substring"), &substr, &sopts);
        if l >= 2 {
            run_class(&idx, &format!("{l}-char fuzzy"), &fuzzy, &sopts);
        }
        run_class(&idx, &format!("{l}-char random letters"), &miss, &sopts);
        // Unlikely long query that matches nothing: the full-scan path plus fuzzy.
        if l == 8 {
            run_class(
                &idx,
                "8-char no match",
                &(0..nq)
                    .map(|_| {
                        (0..8)
                            .map(|_| (b'q' + rng.below(2) as u8) as char)
                            .collect()
                    })
                    .collect::<Vec<String>>(),
                &sopts,
            );
        }
    }

    // Exhaustive sweep: every 1 and 2 character query over a-z and 0-9, plus separators.
    let alphabet: Vec<char> = ('a'..='z')
        .chain('0'..='9')
        .chain(['_', '-', '.'])
        .collect();
    let mut sweep: Vec<(Duration, String)> = Vec::new();
    for &a in &alphabet {
        for _ in 0..3 {
            let q = a.to_string();
            let t = Instant::now();
            black_box(idx.search(&q, &sopts));
            sweep.push((t.elapsed(), q));
        }
        for &b in &alphabet {
            let q: String = [a, b].iter().collect();
            black_box(idx.search(&q, &sopts));
            let t = Instant::now();
            black_box(idx.search(&q, &sopts));
            sweep.push((t.elapsed(), q));
        }
    }
    sweep.sort();
    let d: Vec<Duration> = sweep.iter().map(|s| s.0).collect();
    println!(
        "\nexhaustive sweep, every 1 and 2 character query ({}): p50 {}  p99 {}  max {}  slowest: {}",
        sweep.len(),
        ms(pct(&d, 0.5)),
        ms(pct(&d, 0.99)),
        ms(*d.last().unwrap()),
        sweep
            .iter()
            .rev()
            .take(5)
            .map(|(t, q)| format!("{q:?} {}", ms(*t).trim()))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Typing session.
    println!("\ntyping \"quarterly report final\" one key at a time:");
    let phrase = "quarterly report final";
    for end in 1..=phrase.len() {
        let q = &phrase[..end];
        black_box(idx.search(q, &sopts));
        let t = Instant::now();
        let r = idx.search(q, &sopts);
        let el = t.elapsed();
        let top = r
            .first()
            .and_then(|h| idx.entry(h.id))
            .map_or("(none)".to_string(), |e| e.path.display().to_string());
        let short = if top.len() > 70 {
            format!("...{}", &top[top.len() - 67..])
        } else {
            top
        };
        println!("  {q:<24} {}  {:>3} hits  top: {short}", ms(el), r.len());
    }
    println!("\nranking checks:");
    for q in ["qrf", "launch plan", "wifi", "form-16", "Panoply Launch"] {
        let r = idx.search(q, &sopts);
        println!(
            "  {q:<16} -> {}",
            r.iter()
                .take(3)
                .filter_map(|h| idx.entry(h.id))
                .map(|e| e.name)
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }

    // ---- comparison ------------------------------------------------------------------------------
    if compare {
        println!("\ncomparison on the same root (each is one query; ours is the numbers above)");
        let rootstr = root.to_string_lossy().to_string();
        for q in ["quarterly", "launch"] {
            let mut ts = Vec::new();
            for _ in 0..3 {
                let t = Instant::now();
                let out = Command::new("rg")
                    .args([
                        "--files",
                        "--hidden",
                        "--no-ignore",
                        "-j",
                        "8",
                        "-g",
                        &format!("*{q}*"),
                        &rootstr,
                    ])
                    .output();
                ts.push((
                    t.elapsed(),
                    out.map_or(0, |o| o.stdout.iter().filter(|&&b| b == b'\n').count()),
                ));
            }
            ts.sort();
            println!(
                "  rg --files -g '*{q}*' (fd-style walk, 8 threads): best {:.2}s, median {:.2}s, {} matches",
                ts[0].0.as_secs_f64(),
                ts[1].0.as_secs_f64(),
                ts[0].1
            );
            let mut ts = Vec::new();
            for _ in 0..5 {
                let t = Instant::now();
                let out = Command::new("mdfind")
                    .args(["-onlyin", &rootstr, "-name", q])
                    .output();
                ts.push((
                    t.elapsed(),
                    out.map_or(0, |o| o.stdout.iter().filter(|&&b| b == b'\n').count()),
                ));
            }
            ts.sort();
            println!(
                "  mdfind -onlyin root -name {q}: median {:.0} ms (includes process start), {} matches",
                ts[2].0.as_secs_f64() * 1e3,
                ts[0].1
            );
        }
    }
}
