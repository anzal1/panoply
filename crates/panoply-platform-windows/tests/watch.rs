//! Runs on the Windows CI runner only: the real watchers against a temporary folder.
#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use panoply_platform_windows::{Backend, Event, Options, start, start_with};

fn scratch(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("panoply-win-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

/// Waits until an event for `want` (or below it) arrives.
fn expect_change(rx: &Receiver<Event>, want: &Path) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(8) {
        if let Ok(Event::Changed(p) | Event::Rescan(p)) =
            rx.recv_timeout(Duration::from_millis(100))
            && p.starts_with(want)
        {
            return;
        }
    }
    panic!("no event for {}", want.display());
}

#[test]
fn the_default_is_directory_changes_and_reports_files() {
    let root = scratch("default");
    let (tx, rx) = mpsc::channel();
    let w = start(std::slice::from_ref(&root), move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    assert_eq!(w.backends().len(), 1);
    assert_eq!(w.backends()[0].1, Backend::DirectoryChanges);
    std::thread::sleep(Duration::from_millis(200));

    fs::write(root.join("one.txt"), b"x").unwrap();
    expect_change(&rx, &root.join("one.txt"));
    fs::rename(root.join("one.txt"), root.join("two.txt")).unwrap();
    expect_change(&rx, &root.join("two.txt"));
    fs::create_dir_all(root.join("a/b")).unwrap();
    fs::write(root.join("a/b/deep.txt"), b"x").unwrap();
    expect_change(&rx, &root.join("a"));
    fs::remove_file(root.join("two.txt")).unwrap();
    expect_change(&rx, &root.join("two.txt"));

    let t = Instant::now();
    drop(w);
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "stop took {:?}",
        t.elapsed()
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_missing_root_is_an_error_not_a_panic() {
    let gone = std::env::temp_dir().join("panoply-win-does-not-exist-xyz");
    assert!(start(&[gone], |_| {}).is_err());
}

/// The USN journal is opt in. When it is asked for it either works (an elevated runner, NTFS) or the
/// root falls back to ReadDirectoryChangesW; in both cases changes still arrive.
#[test]
fn the_usn_journal_opt_in_works_or_falls_back() {
    let root = scratch("usn");
    let (tx, rx) = mpsc::channel();
    let w = start_with(
        std::slice::from_ref(&root),
        Options { usn_journal: true },
        move |e| {
            let _ = tx.send(e);
        },
    )
    .unwrap();
    assert_eq!(w.backends().len(), 1);
    eprintln!(
        "backend with the journal asked for: {:?}",
        w.backends()[0].1
    );
    std::thread::sleep(Duration::from_millis(300));
    fs::write(root.join("journal.txt"), b"x").unwrap();
    expect_change(&rx, &root.join("journal.txt"));
    drop(w);
    let _ = fs::remove_dir_all(&root);
}
