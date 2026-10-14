//! Times decode and encode of a synthetic 12 megapixel image. Usage: encodebench [FILE_TO_DECODE]
#[cfg(target_os = "macos")]
fn main() {
    use panoply_platform_macos::*;
    let (w, h) = (4000usize, 3000usize);
    let mut px = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            // Smooth gradients plus mild texture, so the encoder has real work to do.
            let t = ((x * 7 + y * 13) % 29) as u8;
            px.push(((x * 255 / w) as u8).saturating_add(t / 3));
            px.push(((y * 255 / h) as u8).saturating_add(t / 4));
            px.push((((x + y) * 255 / (w + h)) as u8).saturating_add(t / 2));
            px.push(255);
        }
    }
    let only = std::env::var("ONLY").ok();
    for format in [Format::Heic, Format::Avif] {
        if only.as_deref().is_some_and(|o| o != format!("{format:?}")) {
            continue;
        }
        let input = EncodeInput {
            width: w as u32,
            height: h as u32,
            pixels: Pixels::U8(&px),
            space: Space::Srgb,
        };
        let t = std::time::Instant::now();
        let r = encode(
            &input,
            format,
            &EncodeOptions {
                quality: 0.8,
                ..Default::default()
            },
        );
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match r {
            Ok(b) => {
                println!("{format:?} q0.8 12MP: {ms:.0} ms, {} bytes", b.len());
                let t = std::time::Instant::now();
                let d = Decoder::open(b).unwrap();
                let (ow, oh) = d.oriented_size();
                let mut buf = vec![0u8; ow as usize * 512 * 4];
                let mut y = 0;
                while y < oh {
                    let rows = 512.min(oh - y);
                    d.read_rows_u8(
                        0,
                        y,
                        rows,
                        Space::Srgb,
                        &mut buf[..ow as usize * rows as usize * 4],
                    )
                    .unwrap();
                    y += rows;
                }
                println!(
                    "  decode of that file, full, 512 row bands: {:.0} ms",
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
            Err(e) => println!("{format:?}: {e}"),
        }
    }
    if let Some(path) = std::env::args().nth(1) {
        let data = std::fs::read(&path).unwrap();
        let t = std::time::Instant::now();
        let d = Decoder::open(data).unwrap();
        let (ow, oh) = d.oriented_size();
        let mut buf = vec![0u8; ow as usize * 512 * 4];
        let mut y = 0;
        while y < oh {
            let rows = 512.min(oh - y);
            d.read_rows_u8(
                0,
                y,
                rows,
                Space::Srgb,
                &mut buf[..ow as usize * rows as usize * 4],
            )
            .unwrap();
            y += rows;
        }
        println!(
            "decode {path} {ow}x{oh} ({:.1} MP): {:.0} ms",
            f64::from(ow) * f64::from(oh) / 1e6,
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {}
