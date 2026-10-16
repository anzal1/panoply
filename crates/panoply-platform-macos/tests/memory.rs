//! Memory tests. They live in their own test binary and run one at a time so the resident set
//! size they read is not polluted by other tests.
#![cfg(target_os = "macos")]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

use panoply_platform_macos::{Decoder, Space};

/// Wraps the system allocator and, while armed, records the largest single request and the total
/// requested. It only sees Rust allocations; what ImageIO allocates behind our back is covered by
/// the dirty-memory footprint (see `Probe`).
struct CountingAlloc;

static ARMED: AtomicBool = AtomicBool::new(false);
static LARGEST: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);

fn note(size: usize) {
    if ARMED.load(Relaxed) {
        LARGEST.fetch_max(size, Relaxed);
        TOTAL.fetch_add(size, Relaxed);
    }
}

// SAFETY: every call is forwarded unchanged to `System`; the counters are only atomics, so the
// hooks never allocate.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

unsafe extern "C" {
    static mach_task_self_: u32;
    fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
}

/// Dirty memory the kernel charges to this process (`phys_footprint`). Unlike RSS it ignores clean
/// file pages, such as frameworks paged in on first use, so it does not move with machine load.
fn footprint_bytes() -> u64 {
    // `task_vm_info` up to and including `phys_footprint` (revision 1): a u64, two i32s, then 17
    // u64s, the last one being `phys_footprint`. 19 u64s in all.
    const TASK_VM_INFO: u32 = 22;
    let mut info = [0u64; 19];
    let mut count = (std::mem::size_of_val(&info) / 4) as u32;
    // SAFETY: `info` is writable for `count` 32 bit words and `task_info` writes at most that many.
    let kr = unsafe {
        task_info(
            mach_task_self_,
            TASK_VM_INFO,
            info.as_mut_ptr().cast(),
            &mut count,
        )
    };
    assert_eq!(kr, 0, "task_info failed");
    info[18]
}

/// What running a closure cost: the largest single Rust allocation, all Rust bytes requested, and the
/// highest dirty footprint above where it started, sampled every millisecond.
struct Probe {
    largest: usize,
    total: usize,
    footprint_peak: u64,
}

fn probe<R>(f: impl FnOnce() -> R) -> (R, Probe) {
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let sampler = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let base = footprint_bytes();
            let mut peak = 0;
            while !stop.load(Relaxed) {
                peak = peak.max(footprint_bytes().saturating_sub(base));
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            peak.max(footprint_bytes().saturating_sub(base))
        })
    };
    // Let the sampler take its baseline before anything is counted.
    std::thread::sleep(std::time::Duration::from_millis(20));
    LARGEST.store(0, Relaxed);
    TOTAL.store(0, Relaxed);
    ARMED.store(true, Relaxed);
    let out = f();
    ARMED.store(false, Relaxed);
    let (largest, total) = (LARGEST.load(Relaxed), TOTAL.load(Relaxed));
    stop.store(true, Relaxed);
    let footprint_peak = sampler.join().expect("sampler");
    (
        out,
        Probe {
            largest,
            total,
            footprint_peak,
        },
    )
}

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

/// The bomb is a 17000 x 17000 AVIF, so its RGBA bitmap would be 17000 * 17000 * 4 bytes.
const BOMB_WIDTH: usize = 17_000;
const BOMB_HEIGHT: usize = 17_000;
const BOMB_BITMAP: usize = BOMB_WIDTH * BOMB_HEIGHT * 4;

#[test]
fn dimension_bomb_fails_cleanly_and_never_allocates_the_bitmap() {
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let Some(bytes) = corpus("libvips/avif-17000x17000.avif") else {
        return;
    };
    let (r, p) = probe(|| {
        let r = Decoder::open(bytes);
        if let Ok(d) = &r {
            // If it ever opens, one band must stay small.
            let (w, _) = d.oriented_size();
            let mut out = vec![0u8; w as usize * 4];
            let _ = d.read_rows_u8(0, 0, 1, Space::Srgb, &mut out);
        }
        r
    });
    match &r {
        Err(e) => eprintln!("bomb rejected: {e}"),
        Ok(d) => assert_eq!(d.oriented_size(), (BOMB_WIDTH as u32, BOMB_HEIGHT as u32)),
    }
    eprintln!(
        "bomb open: largest allocation {} B, total {} B, footprint peak +{} B (bitmap would be {BOMB_BITMAP} B)",
        p.largest, p.total, p.footprint_peak
    );
    // No allocation anywhere near the bitmap, and the whole open costs a small fraction of it.
    assert!(
        p.largest < BOMB_BITMAP / 16,
        "one allocation of {} B is close to the {BOMB_BITMAP} B bitmap",
        p.largest
    );
    assert!(
        p.total < BOMB_BITMAP / 8,
        "opening the bomb allocated {} B in total",
        p.total
    );
    // ImageIO allocates through malloc and vm_allocate, which the counting allocator cannot see.
    // The bitmap would show up as dirty memory instead.
    assert!(
        p.footprint_peak < BOMB_BITMAP as u64 / 16,
        "opening the bomb grew the dirty footprint by {} B",
        p.footprint_peak
    );
}

#[test]
fn the_probe_sees_a_large_allocation() {
    // The bomb test passing only means something if the probe can fail. Allocate and touch 64 MiB.
    let _gate = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    const N: usize = 64 << 20;
    let (v, p) = probe(|| vec![1u8; N]);
    assert_eq!(v.len(), N);
    assert!(p.largest >= N, "largest {} B", p.largest);
    assert!(p.total >= N, "total {} B", p.total);
    assert!(
        p.footprint_peak >= (N as u64) * 3 / 4,
        "footprint saw only {} B of {N} B",
        p.footprint_peak
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
