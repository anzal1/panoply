//! Time a JPEG decode and the fast baseline encode (optimized tables) of one picture, one piece
//! against strips on threads.
use std::time::Instant;

use panoply_image::decode::{Input, OpenOpts, open};
use panoply_image::encode::jpeg::{JpegOpts, JpegSink};
use panoply_image::encode::jpegopt::merge_and_optimize;
use panoply_image::encode::jpegstrips::{Profile, StripSink};
use panoply_image::pipeline::{Ctl, ImagePlan, RowSink, decode_to_image, quiet};
use panoply_image::types::{Depth, Layout};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let t = Instant::now();
    let file = std::fs::File::open(&a[0]).unwrap();
    let mut r = open(Input::File(file), Some("jpg"), &OpenOpts::default()).unwrap();
    let (img, _) = decode_to_image(&mut *r, &ImagePlan::default(), &ctl).unwrap();
    println!(
        "decode {:.1} ms ({}x{})",
        t.elapsed().as_secs_f64() * 1000.0,
        img.width,
        img.height
    );
    let opts = JpegOpts {
        quality: 85.0,
        progressive: false,
        optimize: true,
        ..Default::default()
    };
    {
        use panoply_image::encode::jpeg::JpegThread;
        use panoply_image::pipeline::{StreamPlan, run_stream};
        struct S(Option<JpegThread>, JpegOpts);
        impl RowSink for S {
            fn begin(
                &mut self,
                w: u32,
                h: u32,
                l: Layout,
                _d: Depth,
                _i: Option<std::sync::Arc<Vec<u8>>>,
            ) -> panoply_image::error::Result<()> {
                self.0 = Some(JpegThread::start(self.1.clone(), w, h, l));
                Ok(())
            }
            fn push(
                &mut self,
                rows: panoply_image::types::Rows<'_>,
            ) -> panoply_image::error::Result<()> {
                if let panoply_image::types::Rows::U8(s) = rows {
                    self.0.as_mut().unwrap().push(s)?;
                }
                Ok(())
            }
        }
        for _ in 0..3 {
            let t = Instant::now();
            let file = std::fs::File::open(&a[0]).unwrap();
            let mut r = open(Input::File(file), Some("jpg"), &OpenOpts::default()).unwrap();
            let mut s = S(None, opts.clone());
            run_stream(&mut *r, &StreamPlan::default(), &ctl, &mut s).unwrap();
            let t_stream = t.elapsed().as_secs_f64() * 1000.0;
            let bytes = s.0.take().unwrap().finish().unwrap();
            println!(
                "overlapped decode+encode: decode done at {t_stream:.1} ms, file ready at {:.1} ms ({} bytes)",
                t.elapsed().as_secs_f64() * 1000.0,
                bytes.len()
            );
        }
    }
    for opt in [true, false] {
        let t = Instant::now();
        let mut s = JpegSink::new(
            Vec::new(),
            JpegOpts {
                optimize: opt,
                ..opts.clone()
            },
        );
        s.begin(img.width, img.height, Layout::Rgb, Depth::U8, None)
            .unwrap();
        s.push(img.rows(0, img.height)).unwrap();
        let one = s.finish().unwrap();
        println!(
            "optimize={opt}: {} bytes in {:.1} ms",
            one.len(),
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
    for round in 0..1 {
        let t = Instant::now();
        let mut s = JpegSink::new(Vec::new(), opts.clone());
        s.begin(img.width, img.height, Layout::Rgb, Depth::U8, None)
            .unwrap();
        s.push(img.rows(0, img.height)).unwrap();
        let one = s.finish().unwrap();
        let t_one = t.elapsed().as_secs_f64() * 1000.0;
        for threads in [2usize, 4] {
            let t = Instant::now();
            let mut spill =
                || -> Result<std::fs::File, panoply_image::error::ImageError> { panic!() };
            let mut sink =
                StripSink::with_profile(Profile::Fast, opts.clone(), threads, &mut spill)
                    .in_memory();
            sink.begin(img.width, img.height, Layout::Rgb, Depth::U8, None)
                .unwrap();
            sink.push(img.rows(0, img.height)).unwrap();
            let strips = sink.finish().unwrap();
            let t_enc = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let mut out = Vec::new();
            merge_and_optimize(&strips, img.height as usize, &mut out).unwrap();
            let t_merge = t.elapsed().as_secs_f64() * 1000.0;
            println!(
                "round {round}: one piece {t_one:.1} ms ({} bytes) | {threads} threads: strips {t_enc:.1} + merge {t_merge:.1} = {:.1} ms ({} bytes)",
                one.len(),
                t_enc + t_merge,
                out.len()
            );
        }
    }
}
