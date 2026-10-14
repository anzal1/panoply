//! Debug: stream a JPEG through the strip encoder and join the strips.
use panoply_image::decode::{Input, OpenOpts, open};
use panoply_image::encode::jpeg::JpegOpts;
use panoply_image::encode::jpegopt::{StripSource, merge_and_optimize, optimize_huffman};
use panoply_image::encode::jpegstrips::StripSink;
use panoply_image::pipeline::{Ctl, StreamPlan, quiet, run_stream};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let file = std::fs::File::open(&a[0]).unwrap();
    let mut r = open(Input::File(file), Some("jpg"), &OpenOpts::default()).unwrap();
    let h = r.info().height as usize;
    let mut spill =
        || -> Result<std::fs::File, panoply_image::error::ImageError> { panic!("no spill") };
    let mut sink = StripSink::new(
        JpegOpts {
            quality: a[1].parse().unwrap(),
            ..Default::default()
        },
        2,
        &mut spill,
    );
    run_stream(&mut *r, &StreamPlan::default(), &ctl, &mut sink).unwrap();
    let strips = sink.finish().unwrap();
    println!("{} strips, {} bytes", strips.count(), strips.bytes);
    for i in [0usize, 1, 14, 15] {
        std::fs::write(format!("/tmp/strip{i}.jpg"), &*strips.get(i).unwrap()).unwrap();
        let b = strips.get(i).unwrap();
        let mut j = 2;
        while j + 4 < b.len() {
            let m = b[j + 1];
            let l = u16::from_be_bytes([b[j + 2], b[j + 3]]) as usize;
            if m == 0xC4 {
                let seg = &b[j + 4..j + 2 + l];
                println!(
                    "strip {i} DHT seg len {l}: class/id {:02x} bits {:?}",
                    seg[0],
                    &seg[1..17]
                );
            }
            if m == 0xDA {
                break;
            }
            j += 2 + l;
        }
    }
    for i in 0..strips.count() {
        let b = strips.get(i).unwrap();
        match optimize_huffman(&b) {
            Ok(o) => println!("strip {i}: {} -> {}", b.len(), o.len()),
            Err(e) => println!("strip {i}: {} ERR {e}", b.len()),
        }
    }
    let all: Vec<Vec<u8>> = (0..strips.count())
        .map(|i| strips.get(i).unwrap().into_owned())
        .collect();
    for k in 2..=all.len() {
        let mut o = Vec::new();
        if let Err(e) = merge_and_optimize(&all[..k], 16 * k, &mut o) {
            println!("first failure with {k} strips: {e}");
            for j in 0..k - 1 {
                let mut o = Vec::new();
                let pair = [all[j].clone(), all[k - 1].clone()];
                if merge_and_optimize(&pair[..], 32, &mut o).is_err() {
                    println!("  pair ({j}, {}) fails", k - 1);
                }
            }
            break;
        }
    }
    let mut out = Vec::new();
    match merge_and_optimize(&strips, h, &mut out) {
        Ok(()) => println!("joined {}", out.len()),
        Err(e) => println!("join ERR {e}"),
    }
}
