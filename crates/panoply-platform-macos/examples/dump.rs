//! Writes a subsampled upright RGBA copy of an image as `W H` then raw bytes, for eyeballing.
//! Usage: dump FILE FRAME STEP OUT [srgb|p3|source]
#[cfg(target_os = "macos")]
fn main() {
    use panoply_platform_macos::{Decoder, Space};
    let a: Vec<String> = std::env::args().skip(1).collect();
    let d = Decoder::open(std::fs::read(&a[0]).unwrap()).unwrap();
    let frame: usize = a[1].parse().unwrap();
    let step: usize = a[2].parse().unwrap();
    let space = match a.get(4).map(String::as_str) {
        Some("p3") => Space::DisplayP3,
        Some("source") => Space::Source,
        _ => Space::Srgb,
    };
    let g = d.frame_geometry(frame).unwrap();
    let (w, h) = (g.width as usize, g.height as usize);
    let (ow, oh) = (w.div_ceil(step), h.div_ceil(step));
    let mut out = Vec::with_capacity(ow * oh * 4 + 16);
    out.extend_from_slice(format!("{ow} {oh}\n").as_bytes());
    let mut buf = vec![0u8; w * 64 * 4];
    let mut y = 0;
    while y < h {
        let rows = 64.min(h - y);
        let b = &mut buf[..rows * w * 4];
        d.read_rows_u8(frame, y as u32, rows as u32, space, b)
            .unwrap();
        for r in 0..rows {
            if (y + r) % step == 0 {
                for x in (0..w).step_by(step) {
                    out.extend_from_slice(&b[(r * w + x) * 4..(r * w + x) * 4 + 4]);
                }
            }
        }
        y += rows;
    }
    std::fs::write(&a[3], out).unwrap();
}
#[cfg(not(target_os = "macos"))]
fn main() {}
