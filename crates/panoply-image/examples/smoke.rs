//! Decode every file given on the command line and print what came out.
use std::time::Instant;

use panoply_image::decode::{Input, OpenOpts, open};
use panoply_image::pipeline::{Ctl, ImagePlan, decode_to_image, quiet};

fn main() {
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    for path in std::env::args().skip(1) {
        let t = Instant::now();
        let file = std::fs::File::open(&path).unwrap();
        let ext = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_string());
        let res =
            open(Input::File(file), ext.as_deref(), &OpenOpts::default()).and_then(|mut r| {
                let info = r.info().clone();
                let (img, out) = decode_to_image(&mut *r, &ImagePlan::default(), &ctl)?;
                Ok((info, img, out))
            });
        match res {
            Ok((info, img, out)) => println!(
                "{:55} {:?} {}x{} o{} frames={} layout={:?} depth={:?} icc={} notes={:?} -> {}x{} {:?} {:.0}ms",
                path.rsplit('/').next().unwrap(),
                info.format,
                info.width,
                info.height,
                info.orientation,
                info.frames,
                info.layout,
                info.depth,
                info.color.icc.is_some(),
                out.notes,
                img.width,
                img.height,
                img.layout,
                t.elapsed().as_secs_f64() * 1000.0
            ),
            Err(e) => println!("{:55} ERR {}", path.rsplit('/').next().unwrap(), e),
        }
    }
}
