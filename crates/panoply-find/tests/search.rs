//! End to end: a real folder tree on disk, and a randomized check against brute force.

use std::fs;
use std::path::{Path, PathBuf};

use panoply_find::fold::fold;
use panoply_find::segment::{F_DIR, F_ROOT, NONE};
use panoply_find::{Exclusions, Index, Kind, SearchOptions, SegmentBuilder, WalkOptions};

fn scratch(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("panoply-find-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn touch(p: &Path) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"").unwrap();
}

fn names(idx: &Index, q: &str, opts: &SearchOptions) -> Vec<String> {
    idx.search(q, opts)
        .iter()
        .map(|h| idx.entry(h.id).unwrap().name)
        .collect()
}

fn opts() -> SearchOptions<'static> {
    SearchOptions {
        limit: 100,
        ..Default::default()
    }
}

fn build(root: &Path) -> Index {
    // A `.noindex` suffix on the root must not matter: the root is never excluded.
    let o = WalkOptions {
        threads: 4,
        with_mtime: true,
        exclude: Exclusions::defaults(),
        portable_reader: false,
        bulk_reader: true,
    };
    Index::build(&[root.to_path_buf()], &o).unwrap().0
}

#[test]
fn walks_ranks_and_excludes() {
    let root = scratch("tree.noindex");
    touch(&root.join("Documents/quarterly_report_final.xlsx"));
    touch(&root.join("Documents/report.pdf"));
    touch(&root.join("Documents/old/deep/deeper/my report notes.txt"));
    touch(&root.join("src/lib.rs"));
    touch(&root.join("src/main.rs"));
    touch(&root.join("proj/node_modules/left-pad/index.js"));
    touch(&root.join("proj/Cargo.toml"));
    touch(&root.join("proj/target/debug/proj"));
    touch(&root.join("other/target/keep.txt"));
    touch(&root.join("cache.noindex/inside.txt"));
    touch(&root.join("secret/.metadata_never_index"));
    touch(&root.join("secret/diary.txt"));
    touch(&root.join(".hidden/h.txt"));
    touch(&root.join("Cafe\u{301}.txt"));
    let idx = build(&root);

    // Exact and prefix beat substring; shallow beats deep inside a tier.
    let r = names(&idx, "report", &opts());
    assert_eq!(r[0], "report.pdf");
    assert!(r.contains(&"quarterly_report_final.xlsx".to_string()));
    assert!(r.contains(&"my report notes.txt".to_string()));

    // Prefix.
    assert_eq!(
        names(&idx, "quar", &opts()),
        vec!["quarterly_report_final.xlsx"]
    );
    // Fuzzy: abbreviation of the same file.
    assert!(names(&idx, "qrf", &opts()).contains(&"quarterly_report_final.xlsx".to_string()));
    // Exclusions: contents skipped, names kept.
    assert!(names(&idx, "left-pad", &opts()).is_empty());
    assert!(names(&idx, "node_modules", &opts()).contains(&"node_modules".to_string()));
    assert!(names(&idx, "inside", &opts()).is_empty());
    assert!(names(&idx, "diary", &opts()).is_empty());
    // A cargo target folder is skipped, a plain `target` folder is not.
    assert!(!names(&idx, "debug", &opts()).contains(&"debug".to_string()));
    assert_eq!(names(&idx, "keep", &opts()), vec!["keep.txt"]);
    // Hidden.
    assert!(names(&idx, "h.txt", &opts()).is_empty());
    let hidden = SearchOptions {
        include_hidden: true,
        ..opts()
    };
    assert_eq!(names(&idx, "h.txt", &hidden), vec!["h.txt"]);
    // Accent insensitive, both directions.
    assert_eq!(names(&idx, "cafe", &opts()).len(), 1);
    assert_eq!(names(&idx, "caf\u{e9}", &opts()).len(), 1);
    // Smart case: an uppercase letter is case sensitive.
    assert_eq!(names(&idx, "Cargo", &opts()), vec!["Cargo.toml"]);
    // Path token and multiple tokens.
    assert_eq!(names(&idx, "src/lib", &opts()), vec!["lib.rs"]);
    assert_eq!(names(&idx, "documents report", &opts()).len(), 3);
    assert_eq!(
        names(&idx, "deeper report", &opts()),
        vec!["my report notes.txt"]
    );
    // Kinds.
    let dirs = SearchOptions {
        kind: Kind::Dirs,
        ..opts()
    };
    assert!(names(&idx, "src", &dirs).contains(&"src".to_string()));
    let files = SearchOptions {
        kind: Kind::Files,
        ..opts()
    };
    assert!(!names(&idx, "src", &files).contains(&"src".to_string()));
    // Paths come back whole.
    let h = idx.search("lib.rs", &opts());
    assert_eq!(idx.path(h[0].id).unwrap(), root.join("src").join("lib.rs"));
    // The root is never a result.
    assert!(idx.search("panoply-find-test", &opts()).is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn save_and_load_roundtrip() {
    let root = scratch("roundtrip");
    for i in 0..200 {
        touch(&root.join(format!("d{}/file_{i}.txt", i % 7)));
    }
    let idx = build(&root);
    let mut bytes = Vec::new();
    idx.segments()[0].write_to(&mut bytes).unwrap();
    let back = panoply_find::Segment::read_from(&bytes[..]).unwrap();
    let idx2 = Index::from_segment(back);
    let a = idx.search("file_1", &opts());
    let b = idx2.search("file_1", &opts());
    assert_eq!(a, b);
    assert!(!a.is_empty());
    // A damaged file is rejected, not trusted.
    let mut broken = bytes.clone();
    broken[0] ^= 0xff;
    assert!(panoply_find::Segment::read_from(&broken[..]).is_err());
    let _ = fs::remove_dir_all(&root);
}

/// Tiny deterministic generator so the test needs no dependency.
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
}

#[test]
fn matches_brute_force() {
    let alphabet: Vec<char> = "abcde_-. XYZ1é".chars().collect();
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut b = SegmentBuilder::new(0);
    b.push(b"/root", b"/root", NONE, 0, 0, F_DIR | F_ROOT, 0);
    let mut all: Vec<String> = Vec::new();
    for _ in 0..4000 {
        let len = 1 + rng.below(9);
        let name: String = (0..len)
            .map(|_| alphabet[rng.below(alphabet.len())])
            .collect();
        if name.trim().is_empty() || name.starts_with('.') {
            continue;
        }
        let (f, m) = fold(&name);
        if f.is_empty() {
            continue;
        }
        b.push(name.as_bytes(), &f, 0, 1_700_000_000, m, 0, 1);
        all.push(name);
    }
    let idx = Index::from_segment(b.finish());
    let big = SearchOptions {
        limit: 100_000,
        now: 1_700_000_100,
        ..Default::default()
    };
    for _ in 0..400 {
        let len = 1 + rng.below(3);
        let q: String = (0..len).map(|_| alphabet[rng.below(5)]).collect();
        let (qf, _) = fold(&q);
        let got: std::collections::BTreeSet<String> = names(&idx, &q, &big).into_iter().collect();
        // Everything that contains the query as a substring (folded) must be returned.
        for n in &all {
            let nf = fold(n).0;
            let contains = nf.windows(qf.len()).any(|w| w == qf.as_slice());
            if contains {
                assert!(got.contains(n), "query {q:?} missed {n:?}");
            }
        }
        // And nothing returned may fail the subsequence test.
        for n in &got {
            let nf = fold(n).0;
            let mut it = nf.iter();
            assert!(
                qf.iter().all(|c| it.any(|x| x == c)),
                "query {q:?} returned non-match {n:?}"
            );
        }
    }
}

#[test]
fn ranking_prefers_exact_then_prefix_then_recent_then_shallow() {
    let mut b = SegmentBuilder::new(0);
    b.push(b"/r", b"/r", NONE, 0, 0, F_DIR | F_ROOT, 0);
    let now = 1_800_000_000u32;
    let rows: [(&str, u32, u8); 5] = [
        ("my-notes-archive.txt", now - 10, 1),
        ("notes.txt", now - 400 * 86_400, 6),
        ("notes.md", now - 86_400, 6),
        ("notes", now - 400 * 86_400, 9),
        ("notebook.txt", now - 86_400, 1),
    ];
    for (n, t, d) in rows {
        let (f, m) = fold(n);
        b.push(n.as_bytes(), &f, 0, t, m, 0, d);
    }
    let idx = Index::from_segment(b.finish());
    let o = SearchOptions {
        now,
        limit: 10,
        ..Default::default()
    };
    let got = names(&idx, "notes", &o);
    // Exact name first, then the prefix matches with the recent one ahead, then the substring match.
    assert_eq!(
        got,
        vec!["notes", "notes.md", "notes.txt", "my-notes-archive.txt"]
    );
}
