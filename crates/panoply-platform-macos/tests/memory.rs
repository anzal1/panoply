//! Memory tests. They live in their own test binary and run one at a time so the resident set
//! size they read is not polluted by other tests.
#![cfg(target_os = "macos")]

use std::sync::Mutex;

use panoply_platform_macos::{Decoder, Space};

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/corpus/cache/image"
);
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn corpus(rel: &str) -> Option<Vec<u8>> {
    match std::fs::read(format!("{CORPUS}/{rel}")) {
        Ok(d) => Some(d),
        Err(_) => {
            eprintln!("skipping: {rel} is not in the corpus");
            None
        }
    }
}

fn open(rel: &str) -> Option<Decoder> {
    Some(Decoder::open(corpus(rel)?).unwrap_or_else(|e| panic!("{rel} should open: {e}")))
}

fn current_rss_kb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}

#[test]
fn dimension_bomb_fails_cleanly_and_never_allocates_the_bitmap() {
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(bytes) = corpus("libvips/avif-17000x17000.avif") else {
        return;
    };
    let before = current_rss_kb();
    let r = Decoder::open(bytes);
    let after = current_rss_kb();
    match r {
        Err(e) => eprintln!("bomb rejected: {e}"),
        Ok(d) => {
            // If it ever opens, one band must stay small.
            let (w, _) = d.oriented_size();
            let mut out = vec![0u8; w as usize * 4];
            let _ = d.read_rows_u8(0, 0, 1, Space::Srgb, &mut out);
        }
    }
    assert!(
        after.saturating_sub(before) < 100_000,
        "opening the bomb grew RSS by {} KB",
        after - before
    );
}

#[test]
fn huge_heic_reads_a_band_without_decoding_the_whole_image() {
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = open("heic/200mp-gainmap.heic") else {
        return;
    };
    let (w, h) = d.oriented_size();
    assert_eq!((w, h), (12240, 16320));
    let mut band = vec![0u8; w as usize * 64 * 4];
    let t = std::time::Instant::now();
    for y0 in [0u32, 8000, 16256] {
        d.read_rows_u8(0, y0, 64, Space::Srgb, &mut band).unwrap();
        assert!(band.chunks_exact(4).all(|p| p[3] == 255));
    }
    eprintln!("three bands of the 200 MP file in {:?}", t.elapsed());
    // Unbounded decode would be 1.6 GB of ImageIO bitmaps. Cropping keeps the whole test small.
    assert!(current_rss_kb() < 700_000, "RSS {} KB", current_rss_kb());
}

#[test]
fn open_and_read_loop_does_not_grow_memory() {
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    // JPEG is the clean case: nothing may accumulate.
    let Some(jpeg) = corpus("exif-orientation/landscape-6.jpg") else {
        return;
    };
    let run = |n: usize| {
        for _ in 0..n {
            let d = Decoder::open(jpeg.clone()).unwrap();
            let (w, h) = d.oriented_size();
            let mut out = vec![0u8; (w * 64 * 4) as usize];
            for y in (0..h.saturating_sub(64)).step_by(400) {
                d.read_rows_u8(0, y, 64, Space::Srgb, &mut out).unwrap();
            }
        }
    };
    run(20);
    let r0 = current_rss_kb();
    run(200);
    let r1 = current_rss_kb();
    eprintln!("jpeg loop: RSS {r0} KB -> {r1} KB over 200 opens");
    assert!(r1 < r0 + 30_000, "RSS climbed from {r0} to {r1} KB");
}

#[test]
fn heic_open_read_loop_memory_is_reported() {
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(heic) = corpus("heic/xiaomi.heic") else {
        return;
    };
    let run = |n: usize| {
        for _ in 0..n {
            let d = Decoder::open(heic.clone()).unwrap();
            let (w, _) = d.oriented_size();
            let mut out = vec![0u8; (w * 64 * 4) as usize];
            d.read_rows_u8(0, 512, 64, Space::Srgb, &mut out).unwrap();
        }
    };
    run(10);
    let r0 = current_rss_kb();
    run(200);
    let r1 = current_rss_kb();
    eprintln!(
        "heic loop: RSS {r0} KB -> {r1} KB over 200 opens ({:.0} KB per open)",
        (r1 as f64 - r0 as f64) / 200.0
    );
    // Documented ImageIO behaviour: hardware HEVC tile decodes retain some memory. Bound it.
    assert!(r1 < r0 + 400_000, "RSS climbed from {r0} to {r1} KB");
}
