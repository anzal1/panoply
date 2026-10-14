//! Debug: encode one strip of a picture in several MozJPEG configurations and report how
//! faithful each decode is.
use panoply_image::decode::{Input, OpenOpts, open};
use panoply_image::encode::jpeg::{JpegOpts, encode};
use panoply_image::ops::common::decode_mem;
use panoply_image::pipeline::{Ctl, ImagePlan, decode_to_image, quiet};
use panoply_image::types::Samples;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let file = std::fs::File::open(&a[0]).unwrap();
    let mut r = open(Input::File(file), Some("jpg"), &OpenOpts::default()).unwrap();
    let (img, _) = decode_to_image(&mut *r, &ImagePlan::default(), &ctl).unwrap();
    let y0: u32 = a[1].parse().unwrap();
    let rows: u32 = a[2].parse().unwrap();
    let q: f32 = a[3].parse().unwrap();
    let part = img.crop(0, y0, img.width, rows);
    let Samples::U8(want) = &part.samples else {
        panic!()
    };
    for (name, opts) in [
        (
            "trellis+baseline (optimize off)",
            JpegOpts {
                progressive: false,
                optimize: false,
                trellis: true,
                ..Default::default()
            },
        ),
        (
            "trellis+baseline+optimize",
            JpegOpts {
                progressive: false,
                optimize: true,
                trellis: true,
                ..Default::default()
            },
        ),
        ("progressive default", JpegOpts::default()),
        (
            "fastest baseline",
            JpegOpts {
                progressive: false,
                optimize: false,
                ..Default::default()
            },
        ),
    ] {
        let bytes = encode(&part, &JpegOpts { quality: q, ..opts }).unwrap();
        let dec = decode_mem(bytes.clone()).unwrap();
        let Samples::U8(got) = &dec.samples else {
            panic!()
        };
        let mse: f64 = want
            .iter()
            .zip(got)
            .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
            .sum::<f64>()
            / want.len() as f64;
        println!("{name:34} {:8} bytes, mse {mse:.1}", bytes.len());
    }
}
