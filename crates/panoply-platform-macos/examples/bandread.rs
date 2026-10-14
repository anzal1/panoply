//! Reads an image in bands, the way the image engine does, and prints timing.
//! Usage: bandread FILE [band_rows] [max_bands] [u8|u16] [frame]
#[cfg(target_os = "macos")]
fn main() {
    use panoply_platform_macos::{Decoder, Space};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = &args[0];
    let band: usize = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(64);
    let max_bands: usize = args
        .get(2)
        .map(|s| s.parse().unwrap())
        .unwrap_or(usize::MAX);
    let wide = args.get(3).is_some_and(|s| s == "u16");
    let frame: usize = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(0);
    let passes: usize = std::env::var("BAND_PASSES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let shared = std::env::var_os("BAND_REUSE")
        .map(|_| Decoder::open(std::fs::read(path).unwrap()).unwrap());
    for pass in 0..passes {
        eprintln!("pass {pass}");
        let t0 = std::time::Instant::now();
        let own;
        let d = match &shared {
            Some(d) => d,
            None => {
                own = Decoder::open(std::fs::read(path).unwrap()).unwrap();
                &own
            }
        };
        let g = d.frame_geometry(frame).unwrap();
        let (w, h) = (g.width as usize, g.height as usize);
        eprintln!(
            "opened {w}x{h} in {:.1} ms",
            t0.elapsed().as_secs_f64() * 1000.0
        );
        let mut buf8 = vec![0u8; w * band * 4];
        let mut buf16 = vec![0u16; if wide { w * band * 4 } else { 0 }];
        let (mut y, mut n) = (0usize, 0usize);
        let t = std::time::Instant::now();
        let mut checksum = 0u64;
        while y < h && n < max_bands {
            let rows = band.min(h - y);
            let tb = std::time::Instant::now();
            if wide {
                let b = &mut buf16[..rows * w * 4];
                d.read_rows_u16(frame, y as u32, rows as u32, Space::Srgb, b)
                    .unwrap();
                checksum += b.iter().step_by(997).map(|&v| v as u64).sum::<u64>();
            } else {
                let b = &mut buf8[..rows * w * 4];
                d.read_rows_u8(frame, y as u32, rows as u32, Space::Srgb, b)
                    .unwrap();
                checksum += b.iter().step_by(997).map(|&v| v as u64).sum::<u64>();
            }
            if n < 3 || n % 50 == 0 {
                eprintln!(
                    "band {n} (rows {y}..{}) {:.1} ms",
                    y + rows,
                    tb.elapsed().as_secs_f64() * 1000.0
                );
            }
            if std::env::var_os("BAND_REPEAT").is_none() {
                y += rows;
            }
            n += 1;
        }
        if let Some(secs) = std::env::var("BAND_SLEEP")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            eprintln!("sleeping");
            std::thread::sleep(std::time::Duration::from_secs(secs));
        }
        eprintln!(
            "read {n} bands, {y} rows of {h} in {:.0} ms, checksum {checksum}",
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {}
