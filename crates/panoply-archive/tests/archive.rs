//! Round trips for every format and the malicious archives, all built here from bytes. Nothing is
//! downloaded and nothing is read from outside the scratch folder.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use panoply_archive::{
    ArchiveError, CreateOptions, ExtractReport, Format, Limits, create, detect, extract,
};
use panoply_core::CancelToken;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("panoply-archive-it-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

/// Bytes that do not compress: a xorshift stream.
fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut x = seed | 1;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

fn unpack(
    d: &Path,
    archive: &Path,
    limits: &Limits,
) -> (Result<ExtractReport, ArchiveError>, PathBuf) {
    let dest = d.join("out");
    let _ = fs::remove_dir_all(&dest);
    fs::create_dir_all(&dest).unwrap();
    let name = archive.file_name().unwrap().to_string_lossy().into_owned();
    let r = extract(
        File::open(archive).unwrap(),
        &name,
        &dest,
        limits,
        &CancelToken::new(),
        &mut |_, _| {},
    );
    (r, dest)
}

fn pack(inputs: &[PathBuf], out: &Path, format: Format) -> panoply_archive::CreateReport {
    let mut f = File::create(out).unwrap();
    create(
        inputs,
        &mut f,
        &CreateOptions { format, level: 6 },
        Path::new(""),
        &CancelToken::new(),
        &mut |_, _| {},
        &mut |p| {
            File::open(p).map_err(|e| ArchiveError::Io {
                path: p.into(),
                source: e,
            })
        },
    )
    .unwrap()
}

fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn go(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        names.sort();
        for p in names {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            let m = fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                out.push((format!("{rel}/"), vec![]));
                go(base, &p, out);
            } else if m.file_type().is_symlink() {
                out.push((
                    format!("{rel} -> {}", fs::read_link(&p).unwrap().display()),
                    vec![],
                ));
            } else {
                out.push((rel, fs::read(&p).unwrap()));
            }
        }
    }
    let mut v = Vec::new();
    go(root, root, &mut v);
    v
}

fn make_source(d: &Path) -> PathBuf {
    let src = d.join("project");
    fs::create_dir_all(src.join("deep/er")).unwrap();
    fs::create_dir_all(src.join("empty")).unwrap();
    fs::write(src.join("a.txt"), "alpha\n").unwrap();
    fs::write(src.join("deep/b.txt"), "").unwrap();
    fs::write(src.join("deep/er/big.bin"), noise(3 * 1024 * 1024 + 17, 7)).unwrap();
    fs::write(src.join("photo.jpg"), noise(4096, 9)).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("a.txt", src.join("link")).unwrap();
    src
}

#[test]
fn round_trip_every_format() {
    let d = scratch("rt");
    let src = make_source(&d);
    let want = tree(&src);
    for (fmt, file) in [
        (Format::Zip, "p.zip"),
        (Format::TarGz, "p.tar.gz"),
        (Format::TarZst, "p.tar.zst"),
    ] {
        let arc = d.join(file);
        let rep = pack(std::slice::from_ref(&src), &arc, fmt);
        assert_eq!(rep.files, 4, "{fmt:?}");
        assert_eq!(detect(&mut File::open(&arc).unwrap(), file).unwrap(), fmt);
        let (r, dest) = unpack(&d, &arc, &Limits::default());
        let r = r.unwrap();
        assert_eq!(r.format, fmt);
        assert!(r.skipped.is_empty() && r.damaged.is_none(), "{r:?}");
        let got = tree(&dest.join("project"));
        assert_eq!(got, want, "{fmt:?} did not round trip");
    }
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn round_trip_plain_tar_gz_and_zst_single_files() {
    let d = scratch("single");
    let data = noise(300_000, 3);
    // tar
    let tarp = d.join("t.tar");
    {
        let mut b = tar::Builder::new(File::create(&tarp).unwrap());
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        b.append_data(&mut h, "x/data.bin", &data[..]).unwrap();
        b.finish().unwrap();
    }
    let (r, dest) = unpack(&d, &tarp, &Limits::default());
    assert_eq!(r.unwrap().format, Format::Tar);
    assert_eq!(fs::read(dest.join("x/data.bin")).unwrap(), data);
    // gz
    let gz = d.join("notes.txt.gz");
    let mut e =
        flate2::write::GzEncoder::new(File::create(&gz).unwrap(), flate2::Compression::new(6));
    e.write_all(&data).unwrap();
    e.finish().unwrap();
    let (r, dest) = unpack(&d, &gz, &Limits::default());
    assert_eq!(r.unwrap().format, Format::Gz);
    assert_eq!(fs::read(dest.join("notes.txt")).unwrap(), data);
    // zst
    let zs = d.join("blob.bin.zst");
    let mut e = zstd::stream::write::Encoder::new(File::create(&zs).unwrap(), 3).unwrap();
    e.write_all(&data).unwrap();
    e.finish().unwrap();
    let (r, dest) = unpack(&d, &zs, &Limits::default());
    assert_eq!(r.unwrap().format, Format::Zst);
    assert_eq!(fs::read(dest.join("blob.bin")).unwrap(), data);
    fs::remove_dir_all(&d).unwrap();
}

/// One tar entry with exactly the name and type given, however unsafe.
fn tar_entry(name: &[u8], typeflag: u8, link: &str, data: &[u8]) -> Vec<u8> {
    let mut h = [0u8; 512];
    h[..name.len()].copy_from_slice(name);
    h[100..108].copy_from_slice(b"0000644\0");
    h[108..116].copy_from_slice(b"0000000\0");
    h[116..124].copy_from_slice(b"0000000\0");
    h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
    h[136..148].copy_from_slice(b"00000000000\0");
    h[148..156].copy_from_slice(b"        ");
    h[156] = typeflag;
    h[157..157 + link.len()].copy_from_slice(link.as_bytes());
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let mut v = h.to_vec();
    v.extend_from_slice(data);
    v.resize(v.len().div_ceil(512) * 512, 0);
    v
}

fn tar_of(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut v: Vec<u8> = entries.concat();
    v.extend_from_slice(&[0u8; 1024]);
    v
}

fn skipped_names(r: &ExtractReport) -> Vec<&str> {
    r.skipped.iter().map(|s| s.name.as_str()).collect()
}

/// Every link under `root` must resolve inside `root`.
fn assert_no_escape(root: &Path) {
    for (name, _) in tree(root) {
        if let Some((link, _)) = name.split_once(" -> ") {
            let real = root.join(link).canonicalize();
            if let Ok(real) = real {
                assert!(real.starts_with(root), "{link} escapes to {real:?}");
            }
        }
    }
}

#[test]
fn zip_slip_is_rejected_in_zip_and_tar() {
    let d = scratch("slip");
    // zip
    let zp = d.join("slip.zip");
    {
        let mut z = zip::ZipWriter::new(File::create(&zp).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for n in [
            "../evil.txt",
            "a/../../evil2.txt",
            "..\\evil3.txt",
            "good.txt",
        ] {
            z.start_file(n, o).unwrap();
            z.write_all(b"x").unwrap();
        }
        z.finish().unwrap();
    }
    let (r, dest) = unpack(&d, &zp, &Limits::default());
    let r = r.unwrap();
    assert_eq!(r.files, 1, "{r:?}");
    assert_eq!(r.skipped.len(), 3, "{r:?}");
    assert!(dest.join("good.txt").exists());
    for e in ["evil.txt", "evil2.txt", "evil3.txt"] {
        assert!(!d.join(e).exists(), "{e} escaped into the parent");
        assert!(!d.join("out").join("..").join(e).exists());
    }
    // tar
    let tp = d.join("slip.tar");
    fs::write(
        &tp,
        tar_of(&[
            tar_entry(b"../evil.txt", b'0', "", b"x"),
            tar_entry(b"sub/../../evil2.txt", b'0', "", b"x"),
            tar_entry(b"good.txt", b'0', "", b"x"),
        ]),
    )
    .unwrap();
    let (r, dest) = unpack(&d, &tp, &Limits::default());
    let r = r.unwrap();
    assert_eq!(skipped_names(&r), ["../evil.txt", "sub/../../evil2.txt"]);
    assert!(r.skipped[0].reason.contains(".."));
    assert!(dest.join("good.txt").exists());
    assert!(!d.join("evil.txt").exists() && !d.join("evil2.txt").exists());
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn absolute_paths_are_rejected_in_zip_and_tar() {
    let d = scratch("abs");
    let target = d.join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    let abs = format!("{}/pwned.txt", target.display());
    let zp = d.join("abs.zip");
    {
        let mut z = zip::ZipWriter::new(File::create(&zp).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for n in [abs.as_str(), "C:\\Windows\\pwned.txt", "ok.txt"] {
            z.start_file(n, o).unwrap();
            z.write_all(b"x").unwrap();
        }
        z.finish().unwrap();
    }
    let (r, dest) = unpack(&d, &zp, &Limits::default());
    let r = r.unwrap();
    assert_eq!(r.skipped.len(), 2, "{r:?}");
    assert!(r.skipped[0].reason.contains("root of the disk"));
    assert!(!target.join("pwned.txt").exists());
    assert!(dest.join("ok.txt").exists());
    let tp = d.join("abs.tar");
    fs::write(
        &tp,
        tar_of(&[
            tar_entry(abs.as_bytes(), b'0', "", b"x"),
            tar_entry(b"ok.txt", b'0', "", b"x"),
        ]),
    )
    .unwrap();
    let (r, _) = unpack(&d, &tp, &Limits::default());
    assert_eq!(r.unwrap().skipped.len(), 1);
    assert!(!target.join("pwned.txt").exists());
    fs::remove_dir_all(&d).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinks_never_escape_the_destination() {
    let d = scratch("link");
    let outside = d.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let tp = d.join("links.tar");
    fs::write(
        &tp,
        tar_of(&[
            // Points up and out, absolute, and out through a deeper climb.
            tar_entry(b"up", b'2', "../outside", b""),
            tar_entry(b"abs", b'2', outside.to_str().unwrap(), b""),
            tar_entry(b"dir/deep", b'2', "../../outside", b""),
            // A safe link, then a file written through it: refused.
            tar_entry(b"sub/", b'5', "", b""),
            tar_entry(b"inner", b'2', "sub", b""),
            tar_entry(b"inner/through.txt", b'0', "", b"x"),
            // The "go down, then up" shape that fools a naive check.
            tar_entry(b"dot", b'2', ".", b""),
            tar_entry(b"sneaky", b'2', "dot/..", b""),
            // A file through a link that was refused is just a new folder inside.
            tar_entry(b"up/pwned.txt", b'0', "", b"x"),
        ]),
    )
    .unwrap();
    let (r, dest) = unpack(&d, &tp, &Limits::default());
    let r = r.unwrap();
    let skipped = skipped_names(&r);
    for n in ["up", "abs", "dir/deep", "inner/through.txt", "sneaky"] {
        assert!(skipped.contains(&n), "{n} should be refused: {skipped:?}");
    }
    assert_eq!(
        fs::read_dir(&outside).unwrap().count(),
        0,
        "something reached outside"
    );
    assert!(!dest.join("sub/through.txt").exists());
    assert_no_escape(&dest);
    // Same through a ZIP symlink entry.
    let zp = d.join("links.zip");
    {
        let mut z = zip::ZipWriter::new(File::create(&zp).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        z.add_symlink("evil", "../../outside", o).unwrap();
        z.add_symlink("evil2", "/etc/passwd", o).unwrap();
        z.start_file("fine.txt", o).unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
    }
    let (r, dest) = unpack(&d, &zp, &Limits::default());
    let r = r.unwrap();
    assert_eq!(r.skipped.len(), 2, "{r:?}");
    assert_eq!(r.links, 0);
    assert!(dest.join("fine.txt").exists());
    assert_no_escape(&dest);
    fs::remove_dir_all(&d).unwrap();
}

/// Small on purpose: the whole test suite runs on a capped number of threads.
const BOMB_BYTES: u64 = 64 * 1024 * 1024;

#[test]
fn a_zip_bomb_of_1000_to_1_is_stopped_before_anything_is_written() {
    let d = scratch("zipbomb");
    let zp = d.join("bomb.zip");
    {
        let mut z = zip::ZipWriter::new(File::create(&zp).unwrap());
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(9))
            .large_file(true);
        z.start_file("zeros.bin", o).unwrap();
        let mut zeros = io::repeat(0).take(BOMB_BYTES);
        io::copy(&mut zeros, &mut z).unwrap();
        z.finish().unwrap();
    }
    let packed = fs::metadata(&zp).unwrap().len();
    let ratio = BOMB_BYTES / packed;
    eprintln!("zip bomb: {BOMB_BYTES} bytes from {packed} bytes, {ratio}:1");
    assert!(
        ratio >= 1000,
        "the fixture must be a 1000:1 bomb, it is {ratio}:1"
    );
    // The default 1000:1 rule, with the "judge only after" floor lowered so a small fixture counts.
    let limits = Limits {
        ratio_floor: 8 * 1024 * 1024,
        ..Limits::default()
    };
    let (r, dest) = unpack(&d, &zp, &limits);
    match r {
        Err(ArchiveError::Bomb(m)) => {
            assert!(
                m.contains("to 1") && m.contains("raise the ratio limit"),
                "{m}"
            );
        }
        other => panic!("expected a bomb, got {other:?}"),
    }
    assert_eq!(
        fs::read_dir(&dest).unwrap().count(),
        0,
        "nothing may be written"
    );
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_streamed_bomb_is_stopped_while_unpacking() {
    let d = scratch("streambomb");
    // 128 MiB of zeros in a tar.zst: a few KB, so tens of thousands to 1. A tar gives no directory to
    // check first, so only the running ratio can stop it.
    let zp = d.join("bomb.tar.zst");
    {
        let enc = zstd::stream::write::Encoder::new(File::create(&zp).unwrap(), 1).unwrap();
        let mut b = tar::Builder::new(enc);
        let mut h = tar::Header::new_gnu();
        h.set_size(128 * 1024 * 1024);
        h.set_mode(0o644);
        let zeros = io::repeat(0).take(128 * 1024 * 1024);
        b.append_data(&mut h, "zeros.bin", zeros).unwrap();
        b.into_inner().unwrap().finish().unwrap();
    }
    let (r, dest) = unpack(&d, &zp, &Limits::default());
    match r {
        Err(ArchiveError::Bomb(m)) => assert!(m.contains("far bigger than it is"), "{m}"),
        other => panic!("expected a bomb, got {other:?}"),
    }
    let written: u64 = fs::read_dir(&dest)
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    assert!(
        written < 256 * 1024 * 1024,
        "wrote {written} bytes before stopping"
    );
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_total_size_cap_and_an_entry_cap_stop_the_job() {
    let d = scratch("caps");
    let zp = d.join("many.zip");
    {
        let mut z = zip::ZipWriter::new(File::create(&zp).unwrap());
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for i in 0..20 {
            z.start_file(format!("f{i}.bin"), o).unwrap();
            z.write_all(&noise(100_000, i + 1)).unwrap();
        }
        z.finish().unwrap();
    }
    let small = Limits {
        max_total: 1_000_000,
        ..Limits::default()
    };
    assert!(
        matches!(unpack(&d, &zp, &small).0, Err(ArchiveError::Bomb(m)) if m.contains("over the limit"))
    );
    let few = Limits {
        max_entries: 5,
        ..Limits::default()
    };
    assert!(matches!(
        unpack(&d, &zp, &few).0,
        Err(ArchiveError::Bomb(_))
    ));
    assert!(unpack(&d, &zp, &Limits::default()).0.is_ok());
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn nothing_that_exists_is_overwritten() {
    let d = scratch("noover");
    let tp = d.join("dup.tar");
    fs::write(
        &tp,
        tar_of(&[
            tar_entry(b"a.txt", b'0', "", b"from archive 1"),
            tar_entry(b"a.txt", b'0', "", b"from archive 2"),
            tar_entry(b"b.txt", b'0', "", b"b"),
        ]),
    )
    .unwrap();
    let dest = d.join("out");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("a.txt"), "mine, keep me").unwrap();
    let r = extract(
        File::open(&tp).unwrap(),
        "dup.tar",
        &dest,
        &Limits::default(),
        &CancelToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(r.renamed, 2);
    assert_eq!(
        fs::read_to_string(dest.join("a.txt")).unwrap(),
        "mine, keep me"
    );
    assert_eq!(
        fs::read_to_string(dest.join("a (2).txt")).unwrap(),
        "from archive 1"
    );
    assert_eq!(
        fs::read_to_string(dest.join("a (3).txt")).unwrap(),
        "from archive 2"
    );
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn cancel_stops_extract_and_create() {
    let d = scratch("cancel");
    let src = make_source(&d);
    let arc = d.join("c.tar.gz");
    pack(std::slice::from_ref(&src), &arc, Format::TarGz);
    let dest = d.join("out");
    fs::create_dir_all(&dest).unwrap();
    let token = CancelToken::new();
    let t2 = token.clone();
    let r = extract(
        File::open(&arc).unwrap(),
        "c.tar.gz",
        &dest,
        &Limits::default(),
        &token,
        &mut |_, _| t2.cancel(),
    );
    assert!(matches!(r, Err(ArchiveError::Cancelled)));
    let mut f = File::create(d.join("n.zip")).unwrap();
    let t = CancelToken::new();
    t.cancel();
    let r = create(
        &[src],
        &mut f,
        &CreateOptions {
            format: Format::Zip,
            level: 6,
        },
        Path::new(""),
        &t,
        &mut |_, _| {},
        &mut |p| {
            File::open(p).map_err(|e| ArchiveError::Io {
                path: p.into(),
                source: e,
            })
        },
    );
    assert!(matches!(r, Err(ArchiveError::Cancelled)));
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn a_cut_short_archive_keeps_what_was_readable_and_says_so() {
    let d = scratch("cut");
    let src = d.join("s");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("a.txt"), "first").unwrap();
    fs::write(src.join("b.txt"), "second").unwrap();
    fs::write(src.join("z.bin"), noise(2_000_000, 5)).unwrap();
    let arc = d.join("s.tar.gz");
    pack(std::slice::from_ref(&src), &arc, Format::TarGz);
    let bytes = fs::read(&arc).unwrap();
    let cut = d.join("cut.tar.gz");
    fs::write(&cut, &bytes[..bytes.len() * 6 / 10]).unwrap();
    let (r, dest) = unpack(&d, &cut, &Limits::default());
    let r = r.unwrap();
    assert!(
        r.damaged.as_deref().is_some_and(|m| m.contains("damaged")),
        "{r:?}"
    );
    assert_eq!(fs::read_to_string(dest.join("s/a.txt")).unwrap(), "first");
    // Nothing readable at all is an error.
    fs::write(
        d.join("junk.zip"),
        b"PK\x03\x04 this is not really a zip file",
    )
    .unwrap();
    assert!(matches!(
        unpack(&d, &d.join("junk.zip"), &Limits::default()).0,
        Err(ArchiveError::Corrupt(_))
    ));
    fs::remove_dir_all(&d).unwrap();
}

#[test]
fn unknown_files_and_encrypted_zips_get_plain_errors() {
    let d = scratch("unk");
    fs::write(d.join("a.txt"), "just text, no archive here").unwrap();
    match unpack(&d, &d.join("a.txt"), &Limits::default()).0 {
        Err(ArchiveError::Unsupported(m)) => assert!(m.contains("ZIP, TAR"), "{m}"),
        other => panic!("{other:?}"),
    }
    fs::write(d.join("a.7z"), b"7z\xbc\xaf\x27\x1c\x00\x04rest").unwrap();
    match unpack(&d, &d.join("a.7z"), &Limits::default()).0 {
        Err(ArchiveError::Unsupported(m)) => assert!(m.contains("7z"), "{m}"),
        other => panic!("{other:?}"),
    }
    fs::remove_dir_all(&d).unwrap();
}
