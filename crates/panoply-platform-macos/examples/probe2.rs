//! Reads a whole file in 64 row bands and prints alpha and timing statistics.
#[cfg(target_os = "macos")]
fn main() {
    use panoply_platform_macos::{Decoder, Space};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = &args[0];
    let frame: usize = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(0);
    let d = Decoder::open(std::fs::read(path).unwrap()).unwrap();
    println!(
        "headroom {:?} depth {:?} primary {:?}",
        d.properties().top_value("Headroom"),
        d.properties().top_value("Depth"),
        d.properties().top_value("PrimaryImage")
    );
    let g = d.frame_geometry(frame).unwrap();
    let (w, h) = (g.width as usize, g.height as usize);
    let t = std::time::Instant::now();
    let mut buf = vec![0u8; w * 64 * 4];
    let (mut amin, mut amax) = (255u8, 0u8);
    let mut sum = [0u64; 3];
    let mut y = 0;
    while y < h {
        let rows = 64.min(h - y);
        let b = &mut buf[..rows * w * 4];
        d.read_rows_u8(frame, y as u32, rows as u32, Space::Srgb, b)
            .unwrap();
        for px in b.chunks_exact(4) {
            amin = amin.min(px[3]);
            amax = amax.max(px[3]);
            for c in 0..3 {
                sum[c] += px[c] as u64;
            }
        }
        y += rows;
    }
    let n = (w * h) as u64;
    println!(
        "{path} frame {frame} {w}x{h} alpha {amin}..{amax} mean rgb {} {} {} in {:.0} ms",
        sum[0] / n,
        sum[1] / n,
        sum[2] / n,
        t.elapsed().as_secs_f64() * 1000.0
    );
}
#[cfg(not(target_os = "macos"))]
fn main() {}
