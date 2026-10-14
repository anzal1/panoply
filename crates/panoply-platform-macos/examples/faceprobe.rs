//! Runs Vision face landmarks and the subject mask on a picture and prints what comes back.
//! Usage: faceprobe FILE [max_side] [mask.pgm]
#[cfg(target_os = "macos")]
fn main() {
    use panoply_platform_macos::{Decoder, Space, faces};
    let args: Vec<String> = std::env::args().skip(1).collect();
    let d = Decoder::open(std::fs::read(&args[0]).unwrap()).unwrap();
    let g = d.frame_geometry(0).unwrap();
    let (w, h) = (g.width as usize, g.height as usize);
    let max_side: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1200);
    let mut rgba = vec![0u8; w * h * 4];
    d.read_rows_u8(0, 0, h as u32, Space::Srgb, &mut rgba)
        .unwrap();
    let s = (max_side as f64 / w.max(h) as f64).min(1.0);
    let (tw, th) = (
        (w as f64 * s).round() as usize,
        (h as f64 * s).round() as usize,
    );
    let mut rgb = vec![0u8; tw * th * 3];
    for y in 0..th {
        for x in 0..tw {
            let (sx, sy) = ((x as f64 / s) as usize, (y as f64 / s) as usize);
            let p = (sy.min(h - 1) * w + sx.min(w - 1)) * 4;
            rgb[(y * tw + x) * 3..][..3].copy_from_slice(&rgba[p..p + 3]);
        }
    }
    println!(
        "{tw}x{th} available={} mask={}",
        faces::available(),
        faces::subject_mask_available()
    );
    let t = std::time::Instant::now();
    let fs = faces::detect_faces(&rgb, tw as u32, th as u32).unwrap();
    println!(
        "faces: {} in {:.0} ms",
        fs.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    for f in &fs {
        println!("{f:#?}");
    }
    let t = std::time::Instant::now();
    let m = faces::subject_mask(&rgb, tw as u32, th as u32).unwrap();
    println!(
        "mask in {:.0} ms: {}",
        t.elapsed().as_secs_f64() * 1000.0,
        m.is_some()
    );
    if let (Some(m), Some(out)) = (m, args.get(2)) {
        let mut v = format!("P5\n{tw} {th}\n255\n").into_bytes();
        v.extend_from_slice(&m);
        std::fs::write(out, v).unwrap();
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {}
