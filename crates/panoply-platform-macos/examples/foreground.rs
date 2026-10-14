//! Runs Apple Vision's foreground instance mask on each picture given and writes
//! `<name>.mask.pgm` next to the scratch folder in argument 1. Prints timing.
//! Usage: foreground OUT_DIR file...
#[cfg(target_os = "macos")]
fn main() {
    use panoply_platform_macos::{
        Decoder, Space, foreground_instance_mask, foreground_mask_available,
    };
    let mut args = std::env::args().skip(1);
    let out_dir = std::path::PathBuf::from(args.next().expect("OUT_DIR"));
    println!("available: {}", foreground_mask_available());
    for path in args {
        let data = std::fs::read(&path).unwrap();
        let d = Decoder::open(data).unwrap();
        let g = d.frame_geometry(0).unwrap();
        let (w, h) = (g.width as usize, g.height as usize);
        let mut rgba = vec![0u8; w * h * 4];
        d.read_rows_u8(0, 0, h as u32, Space::Srgb, &mut rgba)
            .unwrap();
        let t = std::time::Instant::now();
        match foreground_instance_mask(&rgba, w as u32, h as u32, 4) {
            Ok(m) => {
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                let cov =
                    m.alpha.iter().filter(|&&a| a > 127).count() as f64 / m.alpha.len() as f64;
                let soft = m.alpha.iter().filter(|&&a| a > 8 && a < 247).count() as f64
                    / m.alpha.len() as f64;
                println!(
                    "{path}: {w}x{h} mask {}x{} instances {} coverage {:.3} soft {:.4} in {ms:.0} ms",
                    m.width, m.height, m.instances, cov, soft
                );
                let name = std::path::Path::new(&path)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let mut pgm = format!("P5\n{} {}\n255\n", m.width, m.height).into_bytes();
                pgm.extend_from_slice(&m.alpha);
                std::fs::write(out_dir.join(format!("{name}.mask.pgm")), pgm).unwrap();
            }
            Err(e) => println!("{path}: error {e}"),
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {}
