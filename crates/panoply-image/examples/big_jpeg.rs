//! Make a large, photographic JPEG by tiling a real photo, streaming rows so making it costs
//! about as much memory as the photo itself. Used to measure Compress above 24 megapixels.
//!
//! ```text
//! cargo run --release -p panoply-image --example big_jpeg -- SRC.jpg OUT.jpg COLS ROWS [quality] [orientation]
//! ```
//!
//! Every other tile is mirrored, so no two neighbours repeat exactly. `orientation` (1 to 8)
//! writes an EXIF orientation tag.

use panoply_image::decode::{Input, OpenOpts, open};
use panoply_image::encode::Embed;
use panoply_image::encode::jpeg::{JpegOpts, JpegSink};
use panoply_image::pipeline::{Ctl, ImagePlan, RowSink, decode_to_image, quiet};
use panoply_image::types::{Depth, Layout, Rows, Samples};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (src, out) = (&a[0], &a[1]);
    let cols: usize = a[2].parse().unwrap();
    let rows: usize = a[3].parse().unwrap();
    let quality: f32 = a.get(4).map(|s| s.parse().unwrap()).unwrap_or(85.0);
    let orientation: u8 = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(1);
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let file = std::fs::File::open(src).unwrap();
    let mut r = open(Input::File(file), Some("jpg"), &OpenOpts::default()).unwrap();
    let (img, _) = decode_to_image(&mut *r, &ImagePlan::default(), &ctl).unwrap();
    let (w, h) = (img.width as usize, img.height as usize);
    let Samples::U8(px) = &img.samples else {
        panic!("8 bit expected")
    };
    let (ow, oh) = (w * cols, h * rows);
    println!("writing {ow} x {oh} ({:.0} MP)", (ow * oh) as f64 / 1e6);
    let mut embed = Embed::default();
    if orientation > 1 {
        embed.exif = Some(panoply_image::meta::jpeg_orientation_only_exif(orientation));
    }
    let f = std::fs::File::create(out).unwrap();
    let mut sink = JpegSink::new(
        std::io::BufWriter::new(f),
        JpegOpts {
            quality,
            progressive: false,
            optimize: false,
            mozjpeg_tables: true,
            embed,
            ..Default::default()
        },
    );
    sink.begin(ow as u32, oh as u32, Layout::Rgb, Depth::U8, None)
        .unwrap();
    let mut line = vec![0u8; ow * 3];
    for ty in 0..rows {
        for y in 0..h {
            let sy = if ty % 2 == 1 { h - 1 - y } else { y };
            for tx in 0..cols {
                let dst = &mut line[tx * w * 3..(tx + 1) * w * 3];
                let srow = &px[sy * w * 3..(sy + 1) * w * 3];
                if tx % 2 == 1 {
                    for x in 0..w {
                        let s = (w - 1 - x) * 3;
                        dst[x * 3..x * 3 + 3].copy_from_slice(&srow[s..s + 3]);
                    }
                } else {
                    dst.copy_from_slice(srow);
                }
            }
            sink.push(Rows::U8(&line)).unwrap();
        }
    }
    sink.finish().unwrap();
    println!("{}", std::fs::metadata(out).unwrap().len());
}
