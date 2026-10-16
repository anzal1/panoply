//! Generates a synthetic home-folder-shaped tree for the benchmark: `gen_tree <root> [files] [threads] [seed]`.
//!
//! Shape: a heavy-tailed number of files per folder (mean about 12), folders nested up to 12 deep, names
//! in the styles real folders have (snake_case, PascalCase, names with spaces, camera and screenshot
//! names, versioned names, accented and CJK names, dotfiles), a realistic extension mix, and modified
//! times spread over eight years with most of the mass recent. A handful of known "needle" files are
//! planted so the benchmark can check ranking. Files are empty; the tree costs inodes, not space.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn gauss(&mut self) -> f64 {
        let (u1, u2) = (self.unit().max(1e-12), self.unit());
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

const WORDS: &[&str] = &[
    "report",
    "invoice",
    "budget",
    "meeting",
    "notes",
    "draft",
    "final",
    "summary",
    "project",
    "design",
    "photo",
    "holiday",
    "family",
    "travel",
    "receipt",
    "tax",
    "contract",
    "resume",
    "letter",
    "proposal",
    "plan",
    "roadmap",
    "sales",
    "marketing",
    "backup",
    "archive",
    "screenshot",
    "wallpaper",
    "music",
    "podcast",
    "lecture",
    "slides",
    "thesis",
    "paper",
    "data",
    "results",
    "analysis",
    "model",
    "config",
    "settings",
    "index",
    "main",
    "utils",
    "helpers",
    "test",
    "tests",
    "build",
    "release",
    "deploy",
    "server",
    "client",
    "api",
    "schema",
    "migration",
    "readme",
    "license",
    "changelog",
    "todo",
    "ideas",
    "journal",
    "recipe",
    "workout",
    "playlist",
    "movie",
    "episode",
    "season",
    "chapter",
    "volume",
    "scan",
    "form",
    "statement",
    "policy",
    "manual",
    "guide",
    "tutorial",
    "demo",
    "prototype",
    "mockup",
    "wireframe",
    "logo",
    "icon",
    "banner",
    "poster",
    "flyer",
    "brochure",
    "catalog",
    "inventory",
    "order",
    "shipping",
    "customer",
    "vendor",
    "payroll",
    "timesheet",
    "expense",
    "quarterly",
    "annual",
    "monthly",
    "weekly",
    "daily",
    "review",
    "feedback",
    "survey",
    "interview",
    "offer",
    "template",
    "sample",
    "example",
    "snippet",
    "script",
    "module",
    "package",
    "vendor",
    "assets",
    "images",
    "videos",
    "fonts",
    "styles",
    "components",
    "pages",
    "routes",
    "models",
    "views",
    "controllers",
    "services",
    "hooks",
    "store",
    "state",
    "reducer",
    "action",
    "query",
    "mutation",
    "resolver",
    "worker",
    "queue",
    "cache",
    "session",
    "token",
    "auth",
    "login",
    "signup",
    "profile",
    "account",
    "billing",
    "invoice2",
    "dashboard",
    "chart",
    "table",
    "export",
];
const SYL: &[&str] = &[
    "ka", "to", "mi", "ra", "ne", "so", "lu", "vi", "da", "po", "en", "ar", "ix", "on", "el", "us",
    "or", "an", "it", "ex",
];
const EXTS: &[(&str, u32)] = &[
    ("jpg", 14),
    ("png", 9),
    ("pdf", 9),
    ("txt", 6),
    ("rs", 4),
    ("ts", 6),
    ("js", 6),
    ("json", 6),
    ("md", 5),
    ("docx", 4),
    ("xlsx", 3),
    ("mp3", 3),
    ("mp4", 2),
    ("py", 4),
    ("html", 3),
    ("css", 3),
    ("zip", 2),
    ("log", 3),
    ("csv", 3),
    ("c", 2),
    ("h", 2),
    ("yaml", 2),
    ("toml", 1),
    ("heic", 3),
    ("mov", 1),
    ("svg", 2),
    ("tsx", 3),
    ("sh", 1),
    ("pptx", 1),
    ("epub", 1),
    ("", 1),
];
const ACCENTED: &[&str] = &[
    "Café menu",
    "résumé_final",
    "Zoë birthday",
    "naïve_bayes",
    "Łódź trip",
    "São Paulo",
    "über_alles",
    "日本語メモ",
    "Привет мир",
    "señor_notes",
];
const TOP: &[&str] = &[
    "Projects",
    "Documents",
    "Downloads",
    "Pictures",
    "Music",
    "Work",
    "Archive",
    "Desktop",
    "Movies",
    "Code",
    "Notes",
    "School",
];

fn word(r: &mut Rng) -> String {
    if r.below(6) == 0 {
        let n = 2 + r.below(3);
        (0..n).map(|_| SYL[r.below(SYL.len())]).collect()
    } else {
        WORDS[r.below(WORDS.len())].to_string()
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| {
        f.to_uppercase().collect::<String>() + c.as_str()
    })
}

fn ext(r: &mut Rng) -> &'static str {
    let total: u32 = EXTS.iter().map(|e| e.1).sum();
    let mut x = r.below(total as usize) as u32;
    for (e, w) in EXTS {
        if x < *w {
            return e;
        }
        x -= w;
    }
    "txt"
}

fn file_name(r: &mut Rng) -> String {
    let e = ext(r);
    let dot = if e.is_empty() {
        String::new()
    } else {
        format!(".{e}")
    };
    match r.below(100) {
        0..=29 => format!("{}_{}{dot}", word(r), word(r)),
        30..=44 => format!("{}{}{dot}", cap(&word(r)), cap(&word(r))),
        45..=54 => format!("{} {} {}{dot}", cap(&word(r)), word(r), 1 + r.below(30)),
        55..=59 => format!("IMG_{:04}.jpg", r.below(10000)),
        60..=62 => format!("DSC{:05}.JPG", r.below(100000)),
        63..=65 => format!(
            "Screenshot 20{:02}-{:02}-{:02} at {:02}.{:02}.{:02}.png",
            18 + r.below(8),
            1 + r.below(12),
            1 + r.below(28),
            r.below(24),
            r.below(60),
            r.below(60)
        ),
        66..=75 => format!("{}-{}-{}{dot}", word(r), word(r), r.below(100)),
        76..=85 => format!("{}{dot}", word(r)),
        86..=92 => format!("{}{}{dot}", word(r), r.below(1000)),
        93..=95 => format!("{}_v{}{dot}", word(r), 1 + r.below(9)),
        96..=97 => format!("{}{dot}", ACCENTED[r.below(ACCENTED.len())]),
        _ => format!(".{}{dot}", word(r)),
    }
}

fn dir_name(r: &mut Rng) -> String {
    match r.below(10) {
        0 => [
            "src", "lib", "docs", "assets", "tests", "build", "old", "tmp",
        ][r.below(8)]
        .to_string(),
        1..=5 => word(r),
        6..=7 => format!("{}-{}", word(r), word(r)),
        8 => format!("{} {}", cap(&word(r)), word(r)),
        _ => format!("{}{}", word(r), 2015 + r.below(11)),
    }
}

struct Dir {
    rel: PathBuf,
    depth: usize,
    files: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(
        args.get(1)
            .expect("usage: gen_tree <root> [files] [threads] [seed]"),
    );
    let target: usize = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);
    let threads: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8);
    let seed: u64 = args
        .get(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_F00D);
    let mut r = Rng(seed | 1);

    // Plan.
    let mut dirs: Vec<Dir> = TOP
        .iter()
        .map(|t| Dir {
            rel: PathBuf::from(t),
            depth: 0,
            files: 0,
        })
        .collect();
    let mut total = 0usize;
    let mut siblings: HashSet<PathBuf> = dirs.iter().map(|d| d.rel.clone()).collect();
    while total < target {
        let n = dirs.len();
        let pi = if r.below(2) == 0 {
            n - 1 - r.below(n.min(3000))
        } else {
            r.below(n)
        };
        if dirs[pi].depth >= 11 {
            continue;
        }
        let mut rel = dirs[pi].rel.join(dir_name(&mut r));
        let mut k = 1;
        while !siblings.insert(rel.clone()) {
            rel = dirs[pi].rel.join(format!("{}_{k}", dir_name(&mut r)));
            k += 1;
        }
        let files = ((1.8 + 1.2 * r.gauss()).exp() as usize)
            .clamp(0, 2500)
            .min(target - total);
        total += files;
        dirs.push(Dir {
            rel,
            depth: dirs[pi].depth + 1,
            files,
        });
    }
    // Top folders hold files too.
    let extra = (target / 400).min(target - total);
    dirs[0].files += extra;
    total += extra;
    eprintln!("planned {} folders, {} files", dirs.len(), total);

    fs::create_dir_all(&root).unwrap();
    let now = SystemTime::now();
    let t0 = Instant::now();
    // Folders first, so a file can never take the name of a folder that is still to be made.
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..threads {
            let (dirs, next, root) = (&dirs, &next, &root);
            s.spawn(move || {
                while let Some(d) = dirs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    fs::create_dir_all(root.join(&d.rel)).unwrap();
                }
            });
        }
    });
    eprintln!("folders made in {:.1}s", t0.elapsed().as_secs_f64());
    let next = AtomicUsize::new(0);
    let made = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for t in 0..threads {
            let (dirs, next, made, root) = (&dirs, &next, &made, &root);
            s.spawn(move || {
                let mut r = Rng(seed ^ (0x9E37_79B9_7F4A_7C15u64.wrapping_mul(t as u64 + 1)) | 1);
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(d) = dirs.get(i) else { break };
                    let p = root.join(&d.rel);
                    for _ in 0..d.files {
                        // create_new fails on a name that exists, also one that differs only by case on a
                        // case-insensitive volume, or that is a folder; then try a numbered name.
                        let mut name = file_name(&mut r);
                        let mut k = 1;
                        let f = loop {
                            match OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(p.join(&name))
                            {
                                Ok(f) => break f,
                                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                                    name = format!("{k}_{}", file_name(&mut r));
                                    k += 1;
                                }
                                Err(e) => panic!("{}: {e}", p.join(&name).display()),
                            }
                        };
                        // Eight years of history, most of it recent.
                        let days = match r.below(10) {
                            0..=3 => r.unit() * 90.0,
                            4..=7 => 90.0 + r.unit() * 640.0,
                            _ => 730.0 + r.unit() * 2190.0,
                        };
                        let _ = f.set_modified(now - Duration::from_secs((days * 86400.0) as u64));
                    }
                    let m = made.fetch_add(d.files, Ordering::Relaxed) + d.files;
                    if i % 4000 == 0 {
                        eprintln!("  {m} files, {:.0}s", t0.elapsed().as_secs_f64());
                    }
                }
            });
        }
    });

    // Needles with known names, for the ranking checks.
    let needles: &[(&str, i64)] = &[
        ("Documents/Work/Finance/2024/quarterly_report_final.xlsx", 3),
        (
            "Documents/Work/Finance/2019/quarterly_report_final_OLD.xlsx",
            2200,
        ),
        ("Projects/panoply/docs/Panoply Launch Plan.md", 1),
        ("Downloads/launch-plan-draft.pdf", 40),
        ("Desktop/wifi password.txt", 5),
        ("Documents/Taxes/2023/form-16.pdf", 300),
    ];
    for (rel, age_days) in needles {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let f = File::create(&p).unwrap();
        let _ = f.set_modified(now - Duration::from_secs(*age_days as u64 * 86400));
    }
    fs::write(root.join(".gen-done"), format!("{total}\n")).unwrap();
    eprintln!(
        "done: {} files in {:.1}s",
        made.load(Ordering::Relaxed),
        t0.elapsed().as_secs_f64()
    );
}
