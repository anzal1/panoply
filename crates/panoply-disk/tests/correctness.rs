//! The scanner against an independent reference walk and against the traps real disks set:
//! hard links, sparse files, symlink loops, unreadable folders, APFS clones.

#![cfg(unix)]

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use panoply_disk::{BackendChoice, Kind, ScanOptions, SkipReason, flags, scan};

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p =
            std::env::temp_dir().join(format!("panoply-disk-{tag}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        Dir(p.canonicalize().unwrap())
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // Restore permissions so the cleanup can enter every folder.
        fn fix(p: &Path) {
            let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o755));
            if let Ok(rd) = fs::read_dir(p) {
                for e in rd.flatten() {
                    if e.file_type().is_ok_and(|t| t.is_dir()) {
                        fix(&e.path());
                    }
                }
            }
        }
        fix(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(p: &Path, len: usize) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let mut f = fs::File::create(p).unwrap();
    // Not compressible, so the allocation is real.
    let mut x = 0x9E37_79B9_7F4A_7C15u64 ^ len as u64;
    let mut buf = vec![0u8; len];
    for b in &mut buf {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = x as u8;
    }
    f.write_all(&buf).unwrap();
}

/// What `du` would add up: allocated bytes of every entry, hard links once, links not followed.
fn reference(root: &Path) -> u64 {
    fn walk(p: &Path, seen: &mut HashSet<(u64, u64)>, total: &mut u64) {
        let md = fs::symlink_metadata(p).unwrap();
        if md.is_file() && md.nlink() > 1 && !seen.insert((md.dev(), md.ino())) {
            return;
        }
        *total += md.blocks() * 512;
        if md.is_dir()
            && let Ok(rd) = fs::read_dir(p)
        {
            for e in rd.flatten() {
                walk(&e.path(), seen, total);
            }
        }
    }
    let mut total = 0;
    walk(root, &mut HashSet::new(), &mut total);
    total
}

fn opts(backend: BackendChoice) -> ScanOptions {
    ScanOptions {
        backend,
        clone_aware: false,
        ..ScanOptions::default()
    }
}

#[test]
fn matches_reference_walk_on_both_backends() {
    let d = Dir::new("ref");
    for i in 0..40 {
        write(&d.0.join(format!("a/b{}/f{i}.bin", i % 7)), 1000 + i * 977);
    }
    write(&d.0.join("big.dat"), 3_000_000);
    write(&d.0.join("zero"), 0);
    fs::create_dir_all(d.0.join("empty/deeper")).unwrap();
    write(&d.0.join("ünï/cødé/日本語.txt"), 5000);
    let want = reference(&d.0);
    for backend in [BackendChoice::Auto, BackendChoice::Portable] {
        let s = scan(&d.0, &opts(backend)).unwrap();
        assert_eq!(s.report.total_bytes, want, "{:?}", backend);
        assert_eq!(s.report.files, 40 + 3, "{:?}", backend);
        assert!(s.report.skipped.is_empty());
    }
}

#[test]
fn names_paths_and_aggregation() {
    let d = Dir::new("tree");
    write(&d.0.join("x/one.bin"), 100_000);
    write(&d.0.join("x/two.bin"), 50_000);
    write(&d.0.join("y/three.bin"), 10_000);
    let s = scan(
        &d.0,
        &ScanOptions {
            collapse_small_files: false,
            ..opts(BackendChoice::Auto)
        },
    )
    .unwrap();
    let t = &s.tree;
    let kids = t.children_by_size(t.root());
    assert_eq!(t.name(kids[0]), "x");
    assert_eq!(t.name(kids[1]), "y");
    let x = kids[0];
    let inner = t.children_by_size(x);
    assert_eq!(t.name(inner[0]), "one.bin");
    assert_eq!(t.path(inner[0]), d.0.join("x/one.bin"));
    let own = |p: &Path| fs::metadata(p).unwrap().blocks() * 512;
    assert_eq!(
        t.size(x),
        t.size(inner[0]) + t.size(inner[1]) + own(&d.0.join("x"))
    );
    assert_eq!(
        t.size(t.root()),
        t.size(kids[0]) + t.size(kids[1]) + own(&d.0)
    );
    assert_eq!(t.items(t.root()), 3);
    assert_eq!(t.kind(inner[0]), Kind::File);
    assert_eq!(t.largest(1, Kind::File), vec![inner[0]]);
    assert_eq!(t.parent(inner[0]), Some(x));
}

#[test]
fn hard_links_count_once() {
    let d = Dir::new("hard");
    write(&d.0.join("orig.bin"), 500_000);
    fs::hard_link(d.0.join("orig.bin"), d.0.join("link1.bin")).unwrap();
    fs::create_dir_all(d.0.join("sub")).unwrap();
    fs::hard_link(d.0.join("orig.bin"), d.0.join("sub/link2.bin")).unwrap();
    let one = fs::metadata(d.0.join("orig.bin")).unwrap().blocks() * 512;
    let want = reference(&d.0); // the file once, plus the folders' own blocks where the filesystem has them
    for backend in [BackendChoice::Auto, BackendChoice::Portable] {
        let s = scan(&d.0, &opts(backend)).unwrap();
        assert_eq!(s.report.total_bytes, want, "{backend:?}");
        assert_eq!(s.report.hard_link_entries, 2);
        assert_eq!(s.report.hard_link_bytes_saved, 2 * one);
        let dups = (0..s.tree.len() as u32)
            .filter(|i| s.tree.flags(panoply_disk::NodeId(*i)) & flags::HARD_LINK_DUPLICATE != 0)
            .count();
        assert_eq!(dups, 2);
    }
}

#[test]
fn sparse_files_cost_what_they_occupy() {
    let d = Dir::new("sparse");
    let p = d.0.join("sparse.img");
    let f = fs::File::create(&p).unwrap();
    f.set_len(200 * 1024 * 1024).unwrap();
    drop(f);
    let md = fs::metadata(&p).unwrap();
    assert!(
        md.blocks() * 512 < md.len() / 4,
        "this filesystem has no sparse files"
    );
    let s = scan(&d.0, &opts(BackendChoice::Auto)).unwrap();
    assert_eq!(s.report.total_bytes, reference(&d.0));
    assert!(s.report.total_bytes < 10 * 1024 * 1024);
    assert_eq!(
        s.report.sparse_or_compressed,
        u64::from(md.blocks() * 512 + 65536 <= md.len())
    );
}

#[test]
fn symlinks_are_never_followed() {
    let d = Dir::new("links");
    write(&d.0.join("real/data.bin"), 200_000);
    symlink(&d.0, d.0.join("real/loop")).unwrap(); // a link to an ancestor
    symlink("/nonexistent/nowhere", d.0.join("dangling")).unwrap();
    symlink(d.0.join("real"), d.0.join("alias")).unwrap(); // a link to a folder
    let want = reference(&d.0);
    for backend in [BackendChoice::Auto, BackendChoice::Portable] {
        let s = scan(&d.0, &opts(backend)).unwrap();
        assert_eq!(s.report.total_bytes, want, "{backend:?}");
        assert_eq!(s.report.files, 4, "{backend:?}"); // data.bin + three links
        let links = (0..s.tree.len() as u32)
            .filter(|i| s.tree.kind(panoply_disk::NodeId(*i)) == Kind::Symlink)
            .count();
        assert_eq!(links, 3);
    }
}

#[test]
fn unreadable_folders_are_noted_not_fatal() {
    if unsafe_is_root() {
        return;
    }
    let d = Dir::new("denied");
    write(&d.0.join("ok/file.bin"), 100_000);
    write(&d.0.join("locked/secret.bin"), 100_000);
    fs::set_permissions(d.0.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
    for backend in [BackendChoice::Auto, BackendChoice::Portable] {
        let s = scan(&d.0, &opts(backend)).unwrap();
        assert_eq!(s.report.skipped.len(), 1, "{backend:?}");
        let sk = s.report.skipped[0];
        assert_eq!(s.tree.name(sk.node), "locked");
        assert!(matches!(
            sk.reason,
            SkipReason::Permission | SkipReason::FullDiskAccess
        ));
        assert_ne!(s.tree.flags(sk.node) & flags::UNREADABLE, 0);
        assert_eq!(s.report.files, 1);
    }
}

fn unsafe_is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false)
}

#[test]
fn a_missing_root_is_an_error() {
    assert!(scan(Path::new("/definitely/not/here"), &ScanOptions::default()).is_err());
}

#[test]
fn a_file_as_root_is_a_one_node_tree() {
    let d = Dir::new("file");
    write(&d.0.join("only.bin"), 12_345);
    let s = scan(&d.0.join("only.bin"), &ScanOptions::default()).unwrap();
    assert_eq!(s.tree.len(), 1);
    assert_eq!(
        s.report.total_bytes,
        fs::metadata(d.0.join("only.bin")).unwrap().blocks() * 512
    );
}

#[test]
fn cancel_returns_a_partial_tree() {
    let d = Dir::new("cancel");
    for i in 0..50 {
        write(&d.0.join(format!("d{i}/f.bin")), 10);
    }
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let s = scan(
        &d.0,
        &ScanOptions {
            cancel: Some(flag),
            ..ScanOptions::default()
        },
    )
    .unwrap();
    assert!(s.report.cancelled);
}

#[test]
fn progress_counters_move() {
    let d = Dir::new("progress");
    for i in 0..30 {
        write(&d.0.join(format!("d{}/f{i}.bin", i % 5)), 3000);
    }
    let p = std::sync::Arc::new(panoply_disk::Progress::default());
    let s = scan(
        &d.0,
        &ScanOptions {
            progress: Some(p.clone()),
            ..ScanOptions::default()
        },
    )
    .unwrap();
    assert_eq!(p.files.load(std::sync::atomic::Ordering::Relaxed), 30);
    assert_eq!(p.dirs.load(std::sync::atomic::Ordering::Relaxed), 6);
    assert_eq!(s.report.files, 30);
}

/// APFS only: a clone shares its blocks, so the pair costs one file, and `du` would count it twice.
#[cfg(target_os = "macos")]
#[test]
fn apfs_clones_count_once() {
    let d = Dir::new("clone");
    write(&d.0.join("a/original.bin"), 2_000_000);
    let status = std::process::Command::new("cp")
        .args(["-c"])
        .arg(d.0.join("a/original.bin"))
        .arg(d.0.join("clone1.bin"))
        .status()
        .unwrap();
    assert!(status.success());
    std::process::Command::new("cp")
        .args(["-c"])
        .arg(d.0.join("a/original.bin"))
        .arg(d.0.join("clone2.bin"))
        .status()
        .unwrap();
    let one = fs::metadata(d.0.join("a/original.bin")).unwrap().blocks() * 512;
    let du_style = reference(&d.0);
    assert_eq!(du_style, 3 * one, "du-style counting sees three copies");

    let aware = scan(&d.0, &ScanOptions::default()).unwrap();
    assert_eq!(
        aware.report.total_bytes, one,
        "three clones occupy the space of one"
    );
    assert_eq!(aware.report.clone_bytes_saved, 2 * one);
    assert_eq!(
        aware.report.total_bytes + aware.report.clone_bytes_saved,
        du_style
    );

    let blind = scan(&d.0, &opts(BackendChoice::Auto)).unwrap();
    assert_eq!(blind.report.total_bytes, du_style);
}

/// A clone that has been edited keeps only the blocks it still shares.
#[cfg(target_os = "macos")]
#[test]
fn an_edited_clone_costs_its_own_blocks() {
    let d = Dir::new("clone-edit");
    write(&d.0.join("original.bin"), 4_000_000);
    std::process::Command::new("cp")
        .args(["-c"])
        .arg(d.0.join("original.bin"))
        .arg(d.0.join("copy.bin"))
        .status()
        .unwrap();
    // Rewrite the middle of the copy: those blocks are no longer shared.
    {
        use std::io::{Seek, SeekFrom};
        let mut f = fs::OpenOptions::new()
            .write(true)
            .open(d.0.join("copy.bin"))
            .unwrap();
        f.seek(SeekFrom::Start(1_000_000)).unwrap();
        f.write_all(&vec![0xAB; 1_000_000]).unwrap();
        f.sync_all().unwrap();
    }
    let du_style = reference(&d.0);
    let s = scan(&d.0, &ScanOptions::default()).unwrap();
    let saved = s.report.clone_bytes_saved;
    assert!(saved > 1_000_000 && saved < 4_000_000, "saved {saved}");
    assert_eq!(s.report.total_bytes + saved, du_style);
}

/// Every child of /System/Volumes is a separate volume (Data, VM, Preboot, ...). The bulk directory call
/// reports a mount point as the folder it covers, so only the mount table tells the scanner not to enter.
#[cfg(target_os = "macos")]
#[test]
fn other_volumes_are_not_entered() {
    let s = scan(Path::new("/System/Volumes"), &ScanOptions::default()).unwrap();
    assert!(
        s.report.other_volumes >= 2,
        "{} volumes skipped",
        s.report.other_volumes
    );
    assert_eq!(s.report.files, 0);
    let skipped = (0..s.tree.len() as u32)
        .filter(|i| s.tree.flags(panoply_disk::NodeId(*i)) & flags::OTHER_VOLUME != 0)
        .count();
    assert_eq!(skipped as u64, s.report.other_volumes);
}

#[test]
fn small_files_fold_into_one_entry_per_folder_without_changing_a_total() {
    let d = Dir::new("small");
    for i in 0..300 {
        write(&d.0.join(format!("pkg{}/f{i}.js", i % 3)), 100 + i);
    }
    write(&d.0.join("pkg0/big.bin"), 200_000);
    fs::hard_link(d.0.join("pkg0/f0.js"), d.0.join("pkg1/linked.js")).unwrap();
    let want = reference(&d.0);

    let folded = scan(&d.0, &opts(BackendChoice::Auto)).unwrap();
    let flat = scan(
        &d.0,
        &ScanOptions {
            collapse_small_files: false,
            ..opts(BackendChoice::Auto)
        },
    )
    .unwrap();
    assert_eq!(folded.report.total_bytes, want);
    assert_eq!(flat.report.total_bytes, want);
    assert_eq!(
        folded.report.files, flat.report.files,
        "folded files still count one each"
    );
    assert_eq!(folded.report.files, 302);
    // 3 folders plus the root, one folded entry in each folder (pkg1 keeps its hard link as its own entry),
    // the big file, and the hard linked pair.
    assert!(folded.report.nodes < 20, "{} nodes", folded.report.nodes);
    assert!(flat.report.nodes > 300);

    let t = &folded.tree;
    let pkg2 = t.children(t.root()).find(|c| t.name(*c) == "pkg2").unwrap();
    let kids: Vec<_> = t.children(pkg2).collect();
    assert_eq!(kids.len(), 1);
    assert_eq!(t.kind(kids[0]), Kind::Small);
    assert_eq!(t.items(kids[0]), 100);
    assert_eq!(t.name(kids[0]), "100 small files");
    assert_eq!(
        t.path(kids[0]),
        d.0.join("pkg2"),
        "a folded entry points at its folder"
    );
    // The largest file is still found, and a folded entry is never listed as a file.
    let top = t.largest(3, Kind::File);
    assert_eq!(t.name(top[0]), "big.bin");
    assert!(top.iter().all(|id| t.kind(*id) == Kind::File));
}

/// Small clones (the Finder duplicates a folder this way) are settled from the clone ID alone, with no need to
/// open each file.
#[cfg(target_os = "macos")]
#[test]
fn small_apfs_clones_count_once() {
    let d = Dir::new("clone-small");
    write(&d.0.join("src/a.bin"), 100_000);
    std::fs::create_dir_all(d.0.join("copy")).unwrap();
    for name in ["x.bin", "y.bin", "z.bin"] {
        let ok = std::process::Command::new("cp")
            .arg("-c")
            .arg(d.0.join("src/a.bin"))
            .arg(d.0.join("copy").join(name))
            .status()
            .unwrap();
        assert!(ok.success());
    }
    let du_style = reference(&d.0);
    let s = scan(&d.0, &ScanOptions::default()).unwrap();
    assert!(s.report.clone_bytes_saved > 0);
    assert_eq!(s.report.total_bytes + s.report.clone_bytes_saved, du_style);
    let one = fs::metadata(d.0.join("src/a.bin")).unwrap().blocks() * 512;
    assert_eq!(
        s.report.total_bytes, one,
        "four copies occupy the space of one"
    );
}

/// Folded small clones (8 KiB each) still give their bytes back from the folder's folded entry.
#[cfg(target_os = "macos")]
#[test]
fn folded_small_clones_count_once() {
    let d = Dir::new("clone-folded");
    write(&d.0.join("src/a.bin"), 8_000);
    fs::create_dir_all(d.0.join("copy")).unwrap();
    for name in ["x.bin", "y.bin", "z.bin"] {
        let ok = std::process::Command::new("cp")
            .arg("-c")
            .arg(d.0.join("src/a.bin"))
            .arg(d.0.join("copy").join(name))
            .status()
            .unwrap();
        assert!(ok.success());
    }
    let du_style = reference(&d.0);
    let s = scan(&d.0, &ScanOptions::default()).unwrap();
    let one = fs::metadata(d.0.join("src/a.bin")).unwrap().blocks() * 512;
    assert_eq!(
        s.report.total_bytes, one,
        "four folded copies occupy the space of one"
    );
    assert_eq!(s.report.total_bytes + s.report.clone_bytes_saved, du_style);
    assert_eq!(s.report.files, 4);
}
