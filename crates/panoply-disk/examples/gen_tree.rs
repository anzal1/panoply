//! Make a synthetic tree for the benchmark: a million entries with the traps a real disk sets.
//!
//!     cargo run --release -p panoply-disk --example gen_tree -- <dir> [--entries 1000000] [--threads 8]
//!
//! Layout: about 12 files per folder on average with a heavy tail (a few folders hold thousands), folders
//! up to 10 levels deep, and file sizes that are mostly one block with a long tail up to tens of MiB.
//! On top of the plain files it adds hard links, symlinks (dangling and to an ancestor), sparse files,
//! APFS clones (macOS) and unreadable folders. Names and sizes come from a seeded hash, so the tree is the
//! same every time. `<dir>` must not exist.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(seed: u64, i: u64, salt: u64) -> f64 {
    (mix(seed ^ mix(i ^ (salt << 40))) >> 11) as f64 / (1u64 << 53) as f64
}

fn file_size(seed: u64, i: u64) -> usize {
    let u = unit(seed, i, 1);
    let v = unit(seed, i, 2);
    if u < 0.80 {
        (v * 4000.0) as usize
    } else if u < 0.95 {
        4096 + (v * 28_000.0) as usize
    } else if u < 0.999 {
        32_768 + (v * v * 480_000.0) as usize
    } else if u < 0.9999 {
        1_000_000 + (v * 4_000_000.0) as usize
    } else {
        5_000_000 + (v * 40_000_000.0) as usize
    }
}

const EXT: [&str; 12] = [
    "swift", "json", "png", "js", "txt", "o", "dat", "mp4", "pdf", "rs", "css", "log",
];

fn main() {
    let mut dir = None;
    let mut entries = 1_000_000usize;
    let mut threads = 8usize;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--entries" => entries = args.next().unwrap().parse().unwrap(),
            "--threads" => threads = args.next().unwrap().parse().unwrap(),
            _ => dir = Some(PathBuf::from(a)),
        }
    }
    let root = dir.expect("usage: gen_tree <dir> [--entries N] [--threads N]");
    assert!(!root.exists(), "{} already exists", root.display());
    let seed = 20_261_015u64;

    let hard_links = 2000usize;
    let symlinks = 1000usize;
    let sparse = 100usize;
    let clones = if cfg!(target_os = "macos") { 500 } else { 0 };
    let locked_dirs = 20usize;
    let locked_files = 10usize;
    let extras = hard_links + symlinks + sparse + clones + locked_dirs * locked_files;
    let plain = entries - extras;
    let dir_count = (plain / 12).max(1);

    // Folders: each hangs below an earlier folder, at most 10 levels deep.
    let mut parent = vec![0usize; dir_count];
    let mut depth = vec![0u8; dir_count];
    let mut paths: Vec<PathBuf> = Vec::with_capacity(dir_count);
    paths.push(root.clone());
    fs::create_dir_all(&root).unwrap();
    for i in 1..dir_count {
        let mut p = (unit(seed, i as u64, 3) * i as f64) as usize;
        while depth[p] >= 9 {
            p = parent[p];
        }
        parent[i] = p;
        depth[i] = depth[p] + 1;
        let path = paths[p].join(format!("d{i}"));
        fs::create_dir(&path).unwrap();
        paths.push(path);
    }
    eprintln!("{} folders", dir_count);

    let pool: Vec<u8> = (0..1 << 20).map(|i| mix(i as u64) as u8).collect();
    let written = AtomicU64::new(0);
    let next = AtomicU64::new(0);
    let chunk = 4096u64;
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let start = next.fetch_add(chunk, Ordering::Relaxed);
                    if start >= plain as u64 {
                        break;
                    }
                    for i in start..(start + chunk).min(plain as u64) {
                        // A heavy-tailed pick: low folder numbers receive many more files.
                        let u = unit(seed, i, 4);
                        let d = ((u * u * u) * dir_count as f64) as usize % dir_count;
                        let ext = EXT[(mix(i) % 12) as usize];
                        let path = paths[d].join(format!("f{i:07}.{ext}"));
                        let len = file_size(seed, i);
                        let mut f = fs::File::create(&path).unwrap();
                        let mut left = len;
                        let off = (mix(i) as usize) % (pool.len() / 2);
                        while left > 0 {
                            let n = left.min(pool.len() - off);
                            f.write_all(&pool[off..off + n]).unwrap();
                            left -= n;
                        }
                        written.fetch_add(len as u64, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    eprintln!(
        "{} plain files, {} MiB of content",
        plain,
        written.load(Ordering::Relaxed) >> 20
    );

    // Hard links: a second name for a file that exists, in another folder.
    let mut made = 0;
    let mut i = 0u64;
    while made < hard_links {
        let src_i = mix(i) % plain as u64;
        i += 1;
        let u = unit(seed, src_i, 4);
        let d = ((u * u * u) * dir_count as f64) as usize % dir_count;
        let ext = EXT[(mix(src_i) % 12) as usize];
        let src = paths[d].join(format!("f{src_i:07}.{ext}"));
        if fs::metadata(&src).map(|m| m.len()).unwrap_or(0) == 0 {
            continue;
        }
        let dst = paths[(mix(i) as usize) % dir_count].join(format!("hl{made:05}"));
        if fs::hard_link(&src, &dst).is_ok() {
            made += 1;
        }
    }

    // Symlinks: dangling, to an ancestor (a loop for anything that follows them), to a file.
    for k in 0..symlinks {
        let at = &paths[(mix(k as u64 + 77) as usize) % dir_count];
        let link = at.join(format!("ln{k:04}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            match k % 3 {
                0 => symlink("/nonexistent/target", &link).unwrap(),
                1 => symlink(&root, &link).unwrap(),
                _ => symlink(&paths[0], &link).unwrap(),
            }
        }
        #[cfg(not(unix))]
        let _ = link;
    }

    // Sparse files: 40 MiB long, one block written.
    for k in 0..sparse {
        let p = paths[(mix(k as u64 + 99) as usize) % dir_count].join(format!("sparse{k:03}.img"));
        let mut f = fs::File::create(&p).unwrap();
        f.set_len(40 << 20).unwrap();
        f.write_all(&pool[..4096]).unwrap();
    }

    // APFS clones of a 1 MiB file, made with the OS's own `cp -c`.
    #[cfg(target_os = "macos")]
    {
        let base = paths[0].join("clone-source.bin");
        fs::write(&base, &pool[..1 << 20]).unwrap();
        for k in 0..clones.saturating_sub(1) {
            let dst =
                paths[(mix(k as u64 + 555) as usize) % dir_count].join(format!("clone{k:04}.bin"));
            let ok = std::process::Command::new("cp")
                .arg("-c")
                .arg(&base)
                .arg(&dst)
                .status()
                .unwrap();
            assert!(ok.success());
        }
    }

    // Unreadable folders, made last so nothing else needs to enter them.
    #[cfg(unix)]
    for k in 0..locked_dirs {
        use std::os::unix::fs::PermissionsExt;
        let p = root.join(format!("locked{k:02}"));
        fs::create_dir(&p).unwrap();
        for j in 0..locked_files {
            fs::write(p.join(format!("secret{j}")), &pool[..5000]).unwrap();
        }
        fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
    }
    eprintln!("done: {} entries requested, extras {}", entries, extras);
}
