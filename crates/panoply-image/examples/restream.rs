//! Re-encode a JPEG by streaming rows, in one of the encoder modes Compress can use above 24
//! megapixels, to measure memory and size.
//!
//! ```text
//! /usr/bin/time -l restream SRC.jpg OUT.jpg MODE QUALITY
//! MODE: progressive | baseline_trellis | baseline_trellis_recoded | baseline_plain
//! ```
use panoply_image::decode::{Input, OpenOpts, open};
use panoply_image::encode::jpeg::{JpegOpts, JpegSink};
use panoply_image::encode::jpegopt::optimize_huffman;
use panoply_image::pipeline::{Ctl, StreamPlan, quiet, run_stream};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (src, out, mode) = (&a[0], &a[1], a[2].as_str());
    let quality: f32 = a[3].parse().unwrap();
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let t = std::time::Instant::now();
    let file = std::fs::File::open(src).unwrap();
    let mut r = open(Input::File(file), Some("jpg"), &OpenOpts::default()).unwrap();
    let opts = match mode {
        "progressive" => JpegOpts {
            quality,
            ..Default::default()
        },
        "baseline_plain" => JpegOpts {
            quality,
            progressive: false,
            optimize: false,
            mozjpeg_tables: true,
            ..Default::default()
        },
        _ => JpegOpts {
            quality,
            progressive: false,
            optimize: false,
            trellis: true,
            ..Default::default()
        },
    };
    let mut sink = JpegSink::new(Vec::new(), opts);
    run_stream(&mut *r, &StreamPlan::default(), &ctl, &mut sink).unwrap();
    let mut bytes = sink.finish().unwrap();
    let stage1 = bytes.len();
    if mode == "baseline_trellis_recoded" {
        bytes = optimize_huffman(&bytes).unwrap();
    }
    std::fs::write(out, &bytes).unwrap();
    println!(
        "{mode} q{quality}: stage1 {stage1} final {} bytes in {:.1} s",
        bytes.len(),
        t.elapsed().as_secs_f64()
    );
}
