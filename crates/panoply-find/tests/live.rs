//! Incremental updates: apply_path semantics without a watcher, compaction, and the real FSEvents loop.

use std::fs;
use std::path::{Path, PathBuf};

use panoply_find::{ApplyOutcome, Exclusions, Index, LiveIndex, SearchOptions, WalkOptions};

fn scratch(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir()).unwrap();
    let p = base.join(format!("panoply-live-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn touch(p: &Path) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"").unwrap();
}

fn live(root: &Path) -> LiveIndex {
    let o = WalkOptions {
        threads: 2,
        with_mtime: true,
        exclude: Exclusions::defaults(),
        portable_reader: false,
        bulk_reader: true,
    };
    let (idx, _) = Index::build(&[root.to_path_buf()], &o).unwrap();
    LiveIndex::new(idx, Exclusions::defaults())
}

fn find(l: &LiveIndex, q: &str) -> Vec<String> {
    let o = SearchOptions {
        limit: 10_000,
        include_hidden: true,
        ..Default::default()
    };
    l.index()
        .search(q, &o)
        .iter()
        .filter_map(|h| l.index().path(h.id))
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn add_update_remove() {
    let root = scratch("basic");
    touch(&root.join("a/one.txt"));
    touch(&root.join("a/two.txt"));
    let mut l = live(&root);
    assert!(find(&l, "three").is_empty());

    touch(&root.join("a/three.txt"));
    assert_eq!(l.apply_path(&root.join("a/three.txt")), ApplyOutcome::Added);
    assert_eq!(
        find(&l, "three"),
        vec![root.join("a/three.txt").to_string_lossy().into_owned()]
    );
    assert_eq!(
        l.apply_path(&root.join("a/three.txt")),
        ApplyOutcome::Updated
    );
    assert_eq!(find(&l, "three").len(), 1);

    fs::remove_file(root.join("a/one.txt")).unwrap();
    assert_eq!(l.apply_path(&root.join("a/one.txt")), ApplyOutcome::Removed);
    assert!(find(&l, "one.txt").is_empty());
    assert_eq!(
        l.apply_path(&root.join("a/never-existed")),
        ApplyOutcome::Ignored
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn new_nested_folder_removed_folder_and_rename() {
    let root = scratch("tree");
    touch(&root.join("keep/k.txt"));
    touch(&root.join("old/inner/deep/x.txt"));
    let mut l = live(&root);

    // A deep new folder reported by its leaf only: the parents are created on the way.
    touch(&root.join("n1/n2/n3/leaf.txt"));
    assert_eq!(
        l.apply_path(&root.join("n1/n2/n3/leaf.txt")),
        ApplyOutcome::Added
    );
    assert_eq!(find(&l, "leaf").len(), 1);
    assert_eq!(find(&l, "n2").len(), 1);
    // Reporting the top folder later must not duplicate anything.
    l.apply_path(&root.join("n1"));
    assert_eq!(find(&l, "leaf").len(), 1);

    // A folder removed with everything in it.
    fs::remove_dir_all(root.join("old")).unwrap();
    assert_eq!(l.apply_path(&root.join("old")), ApplyOutcome::Removed);
    l.finish_batch();
    assert!(find(&l, "x.txt").is_empty());
    assert!(find(&l, "deep").is_empty());

    // Rename: old path gone, new path appears with its contents.
    fs::rename(root.join("keep"), root.join("kept")).unwrap();
    l.apply_path(&root.join("kept"));
    l.apply_path(&root.join("keep"));
    l.finish_batch();
    assert_eq!(
        find(&l, "k.txt"),
        vec![root.join("kept/k.txt").to_string_lossy().into_owned()]
    );

    // Excluded folders are ignored.
    touch(&root.join("proj/node_modules/pkg/index.js"));
    assert_eq!(
        l.apply_path(&root.join("proj/node_modules/pkg/index.js")),
        ApplyOutcome::Ignored
    );
    assert!(find(&l, "index.js").is_empty());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn compaction_keeps_results_and_save_load_works() {
    let root = scratch("compact");
    for i in 0..300 {
        touch(&root.join(format!("d{}/file_{i}.txt", i % 9)));
    }
    let mut l = live(&root);
    for i in 300..360 {
        let p = root.join(format!("d{}/file_{i}.txt", i % 9));
        touch(&p);
        l.apply_path(&p);
    }
    for i in 0..40 {
        let p = root.join(format!("d{}/file_{i}.txt", i % 9));
        fs::remove_file(&p).unwrap();
        l.apply_path(&p);
    }
    fs::remove_dir_all(root.join("d3")).unwrap();
    l.apply_path(&root.join("d3"));
    l.finish_batch();
    let before = {
        let mut v = find(&l, "file_");
        v.sort();
        v
    };
    assert!(!before.is_empty());
    l.compact();
    let mut after = find(&l, "file_");
    after.sort();
    if before != after {
        let b: std::collections::BTreeSet<_> = before.iter().collect();
        let a: std::collections::BTreeSet<_> = after.iter().collect();
        panic!(
            "compaction changed results: only before {:?}, only after {:?}",
            b.difference(&a).take(3).collect::<Vec<_>>(),
            a.difference(&b).take(3).collect::<Vec<_>>()
        );
    }
    assert_eq!(l.index().segments().len(), 1);
    // Lookups still work on the compacted layout (blocks are rebuilt).
    let p = root.join("d1/file_361.txt");
    touch(&p);
    assert_eq!(l.apply_path(&p), ApplyOutcome::Added);
    assert_eq!(find(&l, "file_361").len(), 1);
    assert_eq!(
        l.apply_path(&root.join("d1/file_361.txt")),
        ApplyOutcome::Updated
    );
    // Save and reload.
    let f = root.join("index.bin");
    l.save(&f).unwrap();
    let seg = panoply_find::Segment::read_from(fs::File::open(&f).unwrap()).unwrap();
    let l2 = LiveIndex::new(Index::from_segment(seg), Exclusions::defaults());
    let mut again = find(&l2, "file_");
    again.sort();
    let mut now = find(&l, "file_");
    now.sort();
    assert_eq!(again, now);
    let _ = fs::remove_dir_all(&root);
}

#[cfg(target_os = "macos")]
#[test]
fn fsevents_keeps_the_index_fresh() {
    use std::sync::{Arc, RwLock};
    use std::time::{Duration, Instant};
    let root = scratch("fsevents");
    touch(&root.join("seed/a.txt"));
    let marker = panoply_find::watch::marker();
    let l = Arc::new(RwLock::new(live(&root)));
    let state = root.join("event-id.txt");
    let updater = panoply_find::watch::start(l.clone(), marker, Some(state.clone())).unwrap();
    std::thread::sleep(Duration::from_millis(400));

    let wait = |q: &str, want: usize| {
        let t = Instant::now();
        loop {
            let n = find(&l.read().unwrap(), q).len();
            if n == want {
                return t.elapsed();
            }
            assert!(
                t.elapsed() < Duration::from_secs(6),
                "query {q:?} still has {n} results, wanted {want}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    touch(&root.join("seed/fresh file.txt"));
    let created = wait("fresh file", 1);
    fs::rename(
        root.join("seed/fresh file.txt"),
        root.join("seed/renamed file.txt"),
    )
    .unwrap();
    wait("renamed file", 1);
    wait("fresh file", 0);
    fs::remove_dir_all(root.join("seed")).unwrap();
    wait("a.txt", 0);
    // A burst of many files is one batch.
    for i in 0..500 {
        touch(&root.join(format!("burst/f{i}.dat")));
    }
    let burst = wait(".dat", 500);
    eprintln!("created file visible after {created:?}; 500-file burst visible after {burst:?}");
    drop(updater);
    assert!(panoply_find::watch::read_event_id(&state).is_some_and(|id| id > 0));
    let _ = fs::remove_dir_all(&root);
}
