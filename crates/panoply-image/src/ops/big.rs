//! `image.compress` for pictures too large to hold in memory.
//!
//! The in-memory path (`compress.rs`) keeps the picture and lets MozJPEG hold its coefficient
//! buffers, which is the smallest possible file but needs about 10 bytes a pixel, so it stops
//! near 24 megapixels inside the 300 MB cap. Phones shoot 48 to 200 megapixels, so above that
//! size this module takes over and never holds more than a few strips of rows:
//!
//! 1. **Sample.** One pass over the source collects a mosaic of tiles spread over the picture
//!    (the same sample the in-memory search uses), and the quality is searched on that.
//! 2. **Encode.** The source is decoded again, row by row, into strips that MozJPEG encodes with
//!    its trellis quantizer on a few threads, and the strips are joined with optimal Huffman
//!    tables (`encode::jpegopt`). That is the whole saving of the best mode except progressive
//!    scans, which need every coefficient at once: measured on a 49 MP photograph the file is
//!    0.2 percent larger than MozJPEG's progressive one.
//! 3. **Verify.** The source is decoded a third time next to the file just written, and both are
//!    scored against each other in bands (SSIM, luma and color, the worse of the two), on
//!    several threads. If the picture misses the target the quality moves up and 2 and 3 repeat.
//!
//! Nothing is refused for its size. What cannot be done in strips is replaced by what can, and the
//! result card says so: PNG is written in strips (without oxipng, which needs the whole picture),
//! and formats whose encoders take a whole picture (WebP, AVIF, HEIC, TIFF, GIF, reduced colors)
//! become JPEG, or PNG when the picture has transparency. A picture that an EXIF orientation
//! tag would turn by 90 degrees is not turned (that needs the whole picture): the tag stays, in a
//! small EXIF block, and the card says so.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::sync::{Arc, Mutex};

use crate::color::OutSpace;
use crate::decode::{self, Input, OpenOpts};
use crate::encode::jpeg::JpegOpts;
use crate::encode::jpegopt::{merge_and_optimize, optimize_huffman};
use crate::encode::jpegstrips::{StripSink, Strips};
use crate::encode::png::{self, Effort};
use crate::encode::{self, Embed};
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::pipeline::{Ctl, RowSink, StreamOut, StreamPlan, quiet, run_stream};
use crate::ssim::{Score, estimate_jpeg_quality};
use crate::types::{Depth, Image, Layout, Rows, Samples};

use super::common::*;
use super::compress::{Kind, Mode, kinds_for};
use super::options::Opts;
use super::search::{
    Grid, Sample, largest_fitting, looks_photographic, mosaic_grid, smallest_passing,
};
use super::{Io, NameSpec, Output, human_bytes};

/// Threads used for encoding strips and scoring bands.
fn threads() -> usize {
    if let Some(n) = std::env::var("PANOPLY_IMAGE_THREADS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
    {
        return n.clamp(1, 16);
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 4)
}

/// What the caller worked out before handing over.
pub(super) struct Args<'a> {
    pub io: &'a dyn Io,
    pub path: &'a Path,
    #[allow(dead_code)] // read by the overlapped convert path, wired next
    pub opts: &'a Opts,
    pub mode: Mode,
    pub out_space: OutSpace,
    pub animation: Animation,
    pub shootout: bool,
    pub forced: Option<Format>,
    pub strip: bool,
    pub lossy_png: Option<bool>,
}

/// The decoded rows of a picture whose decoder is slow or leaks memory per pass, kept in a file
/// (8 bit, color managed, flattened) so the later passes read it back instead of decoding the
/// source again. macOS HEIC decoding keeps memory for the life of the process for every tile it
/// decodes, measured at 130 MB for each full pass over a 33 MP grid with 3520 by 1600 tiles.
struct Scratch {
    out: Output,
    width: u32,
    height: u32,
    layout: Layout,
    icc: Option<Arc<Vec<u8>>>,
}

/// A [`RowReader`] over a [`Scratch`] file.
struct ScratchReader {
    info: crate::decode::SrcInfo,
    file: std::io::BufReader<File>,
    row_len: usize,
    left: usize,
}

impl crate::decode::RowReader for ScratchReader {
    fn info(&self) -> &crate::decode::SrcInfo {
        &self.info
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut crate::pipeline::RowBuf) -> Result<usize> {
        let crate::pipeline::RowBuf::U8(buf) = out else {
            return Err(ImageError::Failed(
                "internal: scratch rows are 8 bit".into(),
            ));
        };
        let n = max_rows.min(self.left);
        buf.clear();
        buf.resize(n * self.row_len, 0);
        std::io::Read::read_exact(&mut self.file, &mut buf[..]).map_err(ImageError::from)?;
        self.left -= n;
        Ok(n)
    }
}

impl Scratch {
    fn reader(&mut self) -> Result<ScratchReader> {
        let mut f = self.out.file().try_clone().map_err(ImageError::from)?;
        f.seek(SeekFrom::Start(0)).map_err(ImageError::from)?;
        let mut info = crate::decode::SrcInfo::plain(
            Format::Png,
            self.width,
            self.height,
            self.layout,
            Depth::U8,
        );
        info.color_managed = true;
        info.managed_icc = self.icc.clone();
        info.upright = true;
        Ok(ScratchReader {
            info,
            file: std::io::BufReader::with_capacity(1 << 20, f),
            row_len: self.width as usize * self.layout.channels(),
            left: self.height as usize,
        })
    }
}

/// Writes the rows it forwards to a scratch file.
struct Tee<'a> {
    inner: &'a mut dyn RowSink,
    file: BufWriter<File>,
    icc: Option<Arc<Vec<u8>>>,
}

impl RowSink for Tee<'_> {
    fn begin(
        &mut self,
        w: u32,
        h: u32,
        layout: Layout,
        depth: Depth,
        icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        self.icc = icc.clone();
        self.inner.begin(w, h, layout, depth, icc)
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        if let Rows::U8(s) = rows {
            self.file.write_all(s).map_err(ImageError::from)?;
        }
        self.inner.push(rows)
    }
}

impl Args<'_> {
    fn reader(&self) -> Result<Loaded> {
        load(self.io, self.path, None, self.animation, self.out_space)
    }

    /// Run the source through the pipeline into `sink`. With `scratch`, the rows come from the
    /// scratch file (already in the form the JPEG plan makes) instead of the source.
    fn stream(
        &self,
        plan: &StreamPlan,
        scratch: Option<&std::cell::RefCell<Scratch>>,
        sink: &mut dyn RowSink,
        from: f32,
        to: f32,
        label: &str,
    ) -> Result<StreamOut> {
        let progress = |f: f32| self.io.progress(from + (to - from) * f, label);
        let ctl = Ctl {
            cancel: self.io.cancel(),
            progress: &progress,
        };
        if let Some(sc) = scratch {
            let mut reader = sc.borrow_mut().reader()?;
            let plan = StreamPlan {
                out_space: self.out_space,
                flatten: None,
                out_depth: Some(Depth::U8),
                ..Default::default()
            };
            return run_stream(&mut reader, &plan, &ctl, sink);
        }
        let mut loaded = self.reader()?;
        run_stream(&mut *loaded.reader, plan, &ctl, sink)
    }
}

// ------------------------------------------------------------------ pass 1: the sample

/// Width, height, layout and pixels of a finished band set.
type Finished = (u32, u32, Layout, Option<Arc<Vec<u8>>>);

/// Collects the tiles of a mosaic while rows stream by.
struct Collector {
    grid: Option<Grid>,
    tiles: Vec<Vec<u8>>,
    layout: Layout,
    width: usize,
    y: usize,
    out: Option<Finished>,
}

impl RowSink for Collector {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        if depth != Depth::U8 {
            return Err(ImageError::Failed(
                "internal: sampling needs 8 bit rows".into(),
            ));
        }
        let g = mosaic_grid(width, height, true)
            .ok_or_else(|| ImageError::Failed("internal: no sample grid".into()))?;
        let ch = layout.channels();
        self.tiles = g
            .origins
            .iter()
            .map(|_| vec![0u8; g.tw as usize * g.th as usize * ch])
            .collect();
        self.grid = Some(g);
        self.layout = layout;
        self.width = width as usize;
        self.out = Some((width, height, layout, icc));
        Ok(())
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        let Rows::U8(s) = rows else {
            return Err(ImageError::Failed(
                "internal: sampling needs 8 bit rows".into(),
            ));
        };
        let g = self.grid.as_ref().expect("begin was called");
        let ch = self.layout.channels();
        let rl = self.width * ch;
        let n = s.len() / rl.max(1);
        let (y0, y1) = (self.y, self.y + n);
        for (tile, &(ox, oy)) in self.tiles.iter_mut().zip(&g.origins) {
            let (oy, th, tw) = (oy as usize, g.th as usize, g.tw as usize);
            let (a, b) = (y0.max(oy), y1.min(oy + th));
            for y in a..b {
                let src = &s[(y - y0) * rl + ox as usize * ch..][..tw * ch];
                tile[(y - oy) * tw * ch..][..tw * ch].copy_from_slice(src);
            }
        }
        self.y = y1;
        Ok(())
    }
}

// ------------------------------------------------------------------ pass 3: lockstep scoring

/// Pixels in one scoring band. SSIM keeps several float planes per band, so the band is cut by
/// pixels, not rows, to keep a 16000 pixel wide picture inside the same memory as a narrow one.
const BAND_PIXELS: usize = 700_000;

/// Rows per scoring band for a picture `width` pixels wide: at most 128, at least 40 (a band
/// repeats 10 rows of the one before, so very short bands waste more than they save).
fn band_rows(width: usize) -> usize {
    (BAND_PIXELS / width.max(1)).clamp(40, 128)
}
/// A band repeats this many rows of the one before, so every SSIM window lies inside one band.
const OVERLAP: usize = 10;

enum Msg {
    Begin(u32, u32, Layout),
    Rows(Vec<u8>),
}

/// Decodes the written file and sends its rows to the scorer.
struct ChanSink {
    tx: SyncSender<Msg>,
}

impl RowSink for ChanSink {
    fn begin(
        &mut self,
        w: u32,
        h: u32,
        layout: Layout,
        depth: Depth,
        _: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        if depth != Depth::U8 {
            return Err(ImageError::Failed(
                "internal: scoring needs 8 bit rows".into(),
            ));
        }
        self.tx
            .send(Msg::Begin(w, h, layout))
            .map_err(|_| ImageError::Cancelled)
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        let Rows::U8(s) = rows else {
            return Err(ImageError::Failed(
                "internal: scoring needs 8 bit rows".into(),
            ));
        };
        self.tx
            .send(Msg::Rows(s.to_vec()))
            .map_err(|_| ImageError::Cancelled)
    }
}

/// A band of the original, the same band of the decoded file, its rows, its weight, the row
/// width in pixels and the layout.
type BandJob = (Vec<u8>, Vec<u8>, usize, f64, usize, Layout);

/// Receives the source rows, pairs them with the decoded rows, and hands bands to workers.
struct Pairer<'a> {
    rx: Receiver<Msg>,
    jobs: SyncSender<BandJob>,
    src: Vec<u8>,
    dec: Vec<u8>,
    layout: Layout,
    width: usize,
    scored: bool,
    decoder_done: bool,
    src_rows: usize,
    dec_rows: usize,
    cancel: &'a dyn Fn() -> bool,
}

impl Pairer<'_> {
    fn rl(&self) -> usize {
        (self.width * self.layout.channels()).max(1)
    }

    fn pump(&mut self, last: bool) -> Result<()> {
        loop {
            let rl = self.rl();
            let src_rows = self.src.len() / rl;
            // Pull decoded rows until they cover what the source has (or the decoder ends).
            while !self.decoder_done && self.dec.len() / rl < src_rows {
                match self.rx.recv() {
                    Ok(Msg::Rows(v)) => {
                        self.dec_rows += v.len() / rl;
                        self.dec.extend_from_slice(&v);
                    }
                    Ok(Msg::Begin(..)) => {}
                    Err(_) => self.decoder_done = true,
                }
                if (self.cancel)() {
                    return Err(ImageError::Cancelled);
                }
            }
            let rows = src_rows.min(self.dec.len() / rl);
            let ready = if last {
                rows > 0 && !(self.scored && rows <= OVERLAP)
            } else {
                rows >= band_rows(self.width)
            };
            if !ready {
                return Ok(());
            }
            let w = rows.saturating_sub(OVERLAP).max(1) as f64;
            let want = self.src[..rows * rl].to_vec();
            let got = self.dec[..rows * rl].to_vec();
            self.jobs
                .send((want, got, rows, w, self.width, self.layout))
                .map_err(|_| ImageError::Failed("internal: scoring stopped".into()))?;
            self.scored = true;
            if last {
                self.src.clear();
                self.dec.clear();
                return Ok(());
            }
            self.src.drain(..(rows - OVERLAP) * rl);
            self.dec.drain(..(rows - OVERLAP) * rl);
        }
    }

    /// After the last source row: score what is left and check both sides had the same rows.
    fn finish(&mut self) -> Result<()> {
        self.pump(true)?;
        // Count any decoded rows still in the channel.
        while !self.decoder_done {
            match self.rx.recv() {
                Ok(Msg::Rows(v)) => self.dec_rows += v.len() / self.rl(),
                Ok(Msg::Begin(..)) => {}
                Err(_) => self.decoder_done = true,
            }
        }
        if self.dec_rows != self.src_rows {
            return Err(ImageError::Failed(
                "internal: the encoded picture has a different height than the original".into(),
            ));
        }
        Ok(())
    }
}

impl RowSink for Pairer<'_> {
    fn begin(
        &mut self,
        w: u32,
        _h: u32,
        layout: Layout,
        depth: Depth,
        _: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        if depth != Depth::U8 {
            return Err(ImageError::Failed(
                "internal: scoring needs 8 bit rows".into(),
            ));
        }
        // The first message from the decoder says what the file is.
        match self.rx.recv() {
            Ok(Msg::Begin(dw, _dh, dl)) if dw == w && dl == layout => {}
            Ok(Msg::Begin(..)) => {
                return Err(ImageError::Failed(
                    "internal: the encoded picture has a different shape than the original".into(),
                ));
            }
            _ => {
                return Err(ImageError::Failed(
                    "internal: the written file could not be read back".into(),
                ));
            }
        }
        self.width = w as usize;
        self.layout = layout;
        Ok(())
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        let Rows::U8(s) = rows else {
            return Err(ImageError::Failed(
                "internal: scoring needs 8 bit rows".into(),
            ));
        };
        self.src_rows += s.len() / self.rl();
        self.src.extend_from_slice(s);
        self.pump(false)
    }
}

/// Score the file in `encoded` against the source, decoding both in step.
fn score_against_source(
    a: &Args<'_>,
    scratch: Option<&std::cell::RefCell<Scratch>>,
    encoded: File,
    plan: &StreamPlan,
    window: (f32, f32),
) -> Result<Score> {
    // Each scoring worker holds about 30 MB of filter planes at 16000 pixels wide, so half as many
    // as encoders keeps the check inside the budget.
    let n = (threads() / 2).max(1);
    std::thread::scope(|scope| -> Result<Score> {
        let (dtx, drx) = sync_channel::<Msg>(3);
        let decoder = scope.spawn(move || -> Result<()> {
            let mut reader = decode::open(Input::File(encoded), None, &OpenOpts::default())?;
            let (cancel, progress) = quiet();
            let ctl = Ctl {
                cancel: &cancel,
                progress: &progress,
            };
            let plan = StreamPlan {
                out_space: OutSpace::Keep,
                out_depth: Some(Depth::U8),
                ..Default::default()
            };
            run_stream(&mut *reader, &plan, &ctl, &mut ChanSink { tx: dtx }).map(|_| ())
        });
        let (jtx, jrx) = sync_channel::<BandJob>(n);
        let (rtx, rrx) = channel::<(f64, f64, f64)>();
        let jrx = Arc::new(Mutex::new(jrx));
        let mut workers = Vec::new();
        for _ in 0..n {
            let (jrx, rtx) = (jrx.clone(), rtx.clone());
            workers.push(scope.spawn(move || {
                loop {
                    let job = match jrx.lock() {
                        Ok(g) => g.recv(),
                        Err(_) => return,
                    };
                    let Ok((want, got, rows, weight, width, layout)) = job else {
                        return;
                    };
                    let mk = |v: Vec<u8>| Image {
                        width: width as u32,
                        height: rows as u32,
                        layout,
                        samples: Samples::U8(v),
                        icc: None,
                    };
                    let s = crate::ssim::score(&mk(want), &mk(got));
                    if rtx
                        .send((s.luma * weight, s.chroma * weight, weight))
                        .is_err()
                    {
                        return;
                    }
                }
            }));
        }
        drop(rtx);
        let cancel = || a.io.cancel().is_cancelled();
        let mut pairer = Pairer {
            rx: drx,
            jobs: jtx,
            src: Vec::new(),
            dec: Vec::new(),
            layout: Layout::Rgb,
            width: 0,
            scored: false,
            decoder_done: false,
            src_rows: 0,
            dec_rows: 0,
            cancel: &cancel,
        };
        let streamed = a
            .stream(
                plan,
                scratch,
                &mut pairer,
                window.0,
                window.1,
                "Checking the result",
            )
            .and_then(|_| pairer.finish());
        drop(pairer);
        let (mut luma, mut chroma, mut weight) = (0.0, 0.0, 0.0);
        for (l, c, w) in rrx.iter() {
            luma += l;
            chroma += c;
            weight += w;
        }
        for w in workers {
            let _ = w.join();
        }
        let decoded = decoder
            .join()
            .map_err(|_| ImageError::Failed("internal: the checker stopped".into()))?;
        // The reader's own error says more than "could not be read back".
        decoded?;
        streamed?;
        if weight == 0.0 {
            return Err(ImageError::Failed("internal: nothing was scored".into()));
        }
        Ok(Score {
            luma: luma / weight,
            chroma: chroma / weight,
        })
    })
}

// ------------------------------------------------------------------ candidates

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Jpeg,
    Png,
}

impl Target {
    fn format(self) -> Format {
        match self {
            Target::Jpeg => Format::Jpeg,
            Target::Png => Format::Png,
        }
    }
}

struct Cand {
    target: Target,
    out: Output,
    size: u64,
    quality: Option<f32>,
    score: Option<Score>,
    embed_notes: Carried,
}

/// Everything one picture's passes share.
struct Run<'a, 'b> {
    a: &'b Args<'a>,
    width: u32,
    height: u32,
    #[allow(dead_code)] // read by the overlapped convert path, wired next
    layout: Layout,
    /// Stream plan for JPEG: 8 bit, transparency flattened onto white.
    jpeg_plan: StreamPlan,
    scratch: Option<std::cell::RefCell<Scratch>>,
    passes: std::cell::Cell<u32>,
}

impl Run<'_, '_> {
    /// The progress window of the next pass over the source: each takes 40 percent of what is
    /// left, so progress only moves forward however many passes a picture needs.
    fn window(&self) -> (f32, f32) {
        let i = self.passes.get();
        self.passes.set(i + 1);
        let at = |n: i32| 0.12 + 0.86 * (1.0 - 0.6f32.powi(n));
        (at(i as i32), at(i as i32 + 1))
    }

    /// One full encode of the picture at quality `q`: strips, joined, written to a new file.
    fn jpeg_attempt(&self, q: f32, embed: &Embed) -> Result<(Output, u64)> {
        let a = self.a;
        let scratch: std::cell::RefCell<Option<Output>> = std::cell::RefCell::new(None);
        let mut spill = || -> Result<File> {
            let o = Output::create(a.io, a.path, &NameSpec::Label("working".into()), "tmp")?;
            let mut o = o;
            let f = o.file().try_clone().map_err(ImageError::from)?;
            *scratch.borrow_mut() = Some(o);
            Ok(f)
        };
        let (from, to) = self.window();
        let jo = JpegOpts {
            quality: q,
            subsample: true,
            embed: embed.clone(),
            ..Default::default()
        };
        let mut sink = StripSink::new(jo, threads(), &mut spill);
        a.stream(
            &self.jpeg_plan,
            self.scratch.as_ref(),
            &mut sink,
            from,
            to * 0.8 + from * 0.2,
            "Encoding",
        )?;
        let strips: Strips = sink.finish()?;
        let mut out = Output::create(a.io, a.path, &NameSpec::Label("compressed".into()), "jpg")?;
        {
            let f = out.file().try_clone().map_err(ImageError::from)?;
            let mut w = BufWriter::with_capacity(1 << 20, f);
            merge_and_optimize(&strips, self.height as usize, &mut w)?;
            w.flush().map_err(ImageError::from)?;
        }
        drop(strips);
        drop(scratch);
        let size = out.file().metadata().map(|m| m.len()).unwrap_or(0);
        check_cancel(a.io)?;
        Ok((out, size))
    }

    fn verify(&self, out: &mut Output) -> Result<Score> {
        let mut f = out.file().try_clone().map_err(ImageError::from)?;
        f.seek(SeekFrom::Start(0)).map_err(ImageError::from)?;
        let (from, to) = self.window();
        score_against_source(
            self.a,
            self.scratch.as_ref(),
            f,
            &self.jpeg_plan,
            (from, to),
        )
    }
}

/// The quality search on the sample. Encodes like the strips do (trellis, baseline) so the score
/// is the one the real file gets.
fn sample_bytes(sample: &Sample, q: f32, optimized: bool) -> Result<Vec<u8>> {
    let layout = sample.image().layout;
    let _ = layout;
    let bytes = encode::jpeg::encode(
        sample.image(),
        &JpegOpts {
            quality: q,
            progressive: false,
            optimize: true,
            trellis: true,
            subsample: true,
            ..Default::default()
        },
    )?;
    if optimized {
        optimize_huffman(&bytes)
    } else {
        Ok(bytes)
    }
}

/// Extra similarity asked of the search for a picture that is verified whole afterwards, so the
/// first full pass usually passes and no second one is needed.
const MARGIN: f64 = 0.001;

pub(super) fn run(a: &Args<'_>, loaded: Loaded) -> Result<Option<PathBuf>> {
    let io = a.io;
    let started = std::time::Instant::now();
    let info = loaded.info.clone();
    let name = loaded.name.clone();
    let before = loaded.file_len;
    drop(loaded);
    let o = if info.upright { 1 } else { info.orientation };
    let has_alpha = info.has_alpha;
    let (sw, sh) = (info.width, info.height);
    let (uw, uh) = crate::orient::oriented_size(o, sw, sh);
    let megapixels = uw as f64 * uh as f64 / 1e6;
    let mut notes: Vec<String> = Vec::new();

    let src_bytes = source_bytes(io, a.path);
    let src_meta = SourceMeta::read_for(src_bytes.as_deref(), Format::Jpeg);
    let est_q = if info.format == Format::Jpeg {
        src_bytes.as_deref().and_then(estimate_jpeg_quality)
    } else {
        None
    };
    drop(src_bytes);

    // Pass 1: the sample.
    let jpeg_plan = StreamPlan {
        out_space: a.out_space,
        flatten: has_alpha.then_some([255, 255, 255]),
        out_depth: Some(Depth::U8),
        ..Default::default()
    };
    let mut collector = Collector {
        grid: None,
        tiles: Vec::new(),
        layout: Layout::Rgb,
        width: 0,
        y: 0,
        out: None,
    };
    // A source that costs a lot to decode more than once is decoded once into a scratch file.
    let slow_source = matches!(
        info.format,
        Format::Heic | Format::Avif | Format::Jxl | Format::Jp2 | Format::Raw | Format::Exr
    ) && a.forced != Some(Format::Png);
    let mut scratch_out: Option<Output> = None;
    let sample_out = if slow_source {
        let mut out = Output::create(io, a.path, &NameSpec::Label("working".into()), "tmp")?;
        let file = out.file().try_clone().map_err(ImageError::from)?;
        let mut tee = Tee {
            inner: &mut collector,
            file: BufWriter::with_capacity(1 << 20, file),
            icc: None,
        };
        let r = a.stream(&jpeg_plan, None, &mut tee, 0.0, 0.1, "Reading")?;
        tee.file.flush().map_err(ImageError::from)?;
        scratch_out = Some(out);
        r
    } else {
        a.stream(&jpeg_plan, None, &mut collector, 0.0, 0.1, "Reading")?
    };
    let (width, height, layout, icc) = collector.out.clone().expect("begin was called");
    let grid = collector.grid.clone().expect("begin was called");
    let ch = layout.channels();
    let tiles: Vec<Image> = collector
        .tiles
        .drain(..)
        .map(|v| Image {
            width: grid.tw,
            height: grid.th,
            layout,
            samples: Samples::U8(v),
            icc: None,
        })
        .collect();
    let _ = ch;
    let sample = Sample::from_tiles(&grid, tiles, icc.clone());
    notes.extend(sample_out.notes.iter().cloned());
    let photographic = looks_photographic(sample.image());

    // What can be written in strips.
    let kinds = kinds_for(
        a.forced,
        info.format,
        photographic,
        has_alpha,
        a.lossy_png,
        false,
        a.shootout || (a.forced.is_none() && !super::compress::can_write(info.format)),
    );
    let mut targets: Vec<Target> = Vec::new();
    let mut tried: Vec<String> = Vec::new();
    for k in &kinds {
        let t = match k {
            Kind::Jpeg => Some(Target::Jpeg),
            Kind::PngLossless => Some(Target::Png),
            Kind::PngLossy => {
                tried.push("reduced colors is only offered up to 8 megapixels".into());
                None
            }
            other => {
                if a.shootout {
                    tried.push(format!("{} skipped at this size", other.label()));
                    None
                } else {
                    let sub = if has_alpha { Target::Png } else { Target::Jpeg };
                    notes.push(format!(
                        "{} cannot be written a strip at a time, and this picture is {megapixels:.0} megapixels, so it was saved as {} instead",
                        other.format().label(),
                        sub.format().label()
                    ));
                    Some(sub)
                }
            }
        };
        if let Some(t) = t
            && !targets.contains(&t)
        {
            targets.push(t);
        }
    }
    // Photographs are never smallest as PNG, and a PNG of 100 megapixels takes a while.
    if targets.len() > 1 && photographic {
        targets.retain(|t| *t != Target::Png);
        tried.push("PNG skipped (a photograph)".into());
    }
    if width > crate::encode::jpeg::MAX_SIDE || height > crate::encode::jpeg::MAX_SIDE {
        if targets.contains(&Target::Jpeg) {
            notes.push(format!(
                "JPEG files cannot be larger than {} pixels on a side, so this picture was saved as PNG",
                crate::encode::jpeg::MAX_SIDE
            ));
        }
        targets = vec![Target::Png];
    }
    if targets.is_empty() {
        targets.push(if has_alpha { Target::Png } else { Target::Jpeg });
    }

    let run = Run {
        a,
        width,
        height,
        layout,
        jpeg_plan: jpeg_plan.clone(),
        scratch: scratch_out.map(|out| {
            std::cell::RefCell::new(Scratch {
                out,
                width,
                height,
                layout,
                icc: icc.clone(),
            })
        }),
        passes: std::cell::Cell::new(0),
    };
    let target_format = |t: Target| t.format();
    let embed_for = |t: Target| -> Carried {
        let mut c = carry(
            &src_meta,
            &info,
            width,
            height,
            icc.as_ref(),
            !a.strip,
            true,
            target_format(t),
        );
        if o > 1 && !info.upright {
            // The picture stays as stored; the tag says how to show it.
            let exif = c
                .embed
                .exif
                .as_deref()
                .and_then(|e| crate::meta::exif_with_orientation(e, o))
                .unwrap_or_else(|| crate::meta::jpeg_orientation_only_exif(o));
            c.embed.exif = Some(exif);
            c.kept.push(format!(
                "orientation tag ({})",
                super::convert::orientation_words(o)
            ));
        }
        c
    };

    let mut cands: Vec<Cand> = Vec::new();
    for (n, t) in targets.iter().enumerate() {
        check_cancel(io)?;
        if n > 0 && started.elapsed() > super::compress::SHOOTOUT_BUDGET * 6 {
            tried.push(format!("{} skipped (time budget)", t.format().label()));
            continue;
        }
        let carried = embed_for(*t);
        let res = match t {
            Target::Jpeg => jpeg_candidate(&run, &sample, a.mode, est_q, &carried),
            Target::Png => png_candidate(&run, &carried),
        };
        match res {
            Ok(mut c) => {
                c.embed_notes = carried;
                cands.push(c);
            }
            Err(ImageError::Cancelled) => return Err(ImageError::Cancelled),
            Err(e) => {
                if targets.len() == 1 {
                    return Err(e);
                }
                tried.push(format!("{} not possible ({e})", t.format().label()));
            }
        }
    }
    if let Mode::UnderSize(max) = a.mode {
        cands.retain(|c| c.size <= max);
    }
    let Some(best_i) = cands
        .iter()
        .enumerate()
        .min_by_key(|(_, c)| c.size)
        .map(|(i, _)| i)
    else {
        return Err(ImageError::Failed(format!(
            "{name} could not be compressed with these settings. Try a larger size or another format."
        )));
    };
    let best = cands.swap_remove(best_i);
    if !cands.is_empty() || !tried.is_empty() {
        let mut rows: Vec<String> = std::iter::once(&best)
            .chain(cands.iter())
            .map(|c| format!("{} {}", c.target.format().label(), human_bytes(c.size)))
            .collect();
        rows.sort();
        io.note(format!("{name}: tried {}", rows.join(", ")));
        for t in &tried {
            io.note(format!("{name}: {t}"));
        }
    }
    drop(cands);

    let after = best.size;
    let target_given = !matches!(a.mode, Mode::Smallest(_));
    let asked_other_format = a.forced.is_some_and(|f| f != info.format);
    if after >= before && !target_given && !asked_other_format {
        io.note(format!(
            "{name}: already well compressed. The best result was {} against the original {}, so nothing was written and the original stays as it is",
            human_bytes(after),
            human_bytes(before)
        ));
        return Ok(None);
    }
    let fmt = best.target.format();
    let out_path = best.out.commit();
    let mut line = size_change_line(&name, before, after);
    if let Some(s) = best.score {
        line.push_str(&format!(
            ", similarity {:.3} (color {:.3})",
            s.luma, s.chroma
        ));
    }
    if let Some(q) = best.quality {
        line.push_str(&format!(", quality {}", q.round() as u32));
    }
    io.note(line);
    io.note(format!(
        "{name}: wrote {}",
        out_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    if fmt != info.format {
        io.note(format!(
            "{name}: changed format from {} to {}",
            info.format.label(),
            fmt.label()
        ));
    }
    if let Some(l) = kept_dropped_line(&best.embed_notes) {
        io.note(format!("{name}: {l}"));
    }
    let how = match best.target {
        Target::Jpeg => {
            "in strips of rows (MozJPEG's trellis quantizer on each, then optimized Huffman tables for the whole file). Progressive scans need the whole picture at once, so they were not used"
        }
        Target::Png => {
            "in strips of rows, without the extra pass of oxipng, which needs the whole picture at once"
        }
    };
    io.note(format!(
        "{name}: this picture is {megapixels:.0} megapixels, more than fits in the memory limit at once, so it was compressed {how}"
    ));
    if o > 1 && !info.upright {
        io.note(format!(
            "{name}: kept the orientation tag ({}) instead of turning the picture, which needs the whole picture in memory",
            super::convert::orientation_words(o)
        ));
    }
    if info.frames > 1 {
        io.note(format!("{name}: used the first frame of an animation"));
    }
    for n in notes {
        io.note(format!("{name}: {n}"));
    }
    Ok(Some(out_path))
}

fn png_candidate(run: &Run<'_, '_>, carried: &Carried) -> Result<Cand> {
    let a = run.a;
    let mut out = Output::create(a.io, a.path, &NameSpec::Label("compressed".into()), "png")?;
    let f = out.file().try_clone().map_err(ImageError::from)?;
    let mut sink = png::stream(
        BufWriter::with_capacity(1 << 20, f),
        &carried.embed,
        Effort::Balanced,
    );
    let plan = StreamPlan {
        out_space: a.out_space,
        ..Default::default()
    };
    let (from, to) = run.window();
    a.stream(&plan, None, &mut sink, from, to, "Writing PNG")?;
    sink.finish()?;
    let size = out.file().metadata().map(|m| m.len()).unwrap_or(0);
    Ok(Cand {
        target: Target::Png,
        out,
        size,
        quality: None,
        score: None,
        embed_notes: Carried::default(),
    })
}

fn jpeg_candidate(
    run: &Run<'_, '_>,
    sample: &Sample,
    mode: Mode,
    est_q: Option<f32>,
    carried: &Carried,
) -> Result<Cand> {
    let a = run.a;
    let embed = &carried.embed;
    let (qlo, qhi_all) = (10u32, 100u32);
    let mk = |out: Output, size: u64, q: f32, score: Option<Score>| Cand {
        target: Target::Jpeg,
        out,
        size,
        quality: Some(q),
        score,
        embed_notes: Carried::default(),
    };
    match mode {
        Mode::Quality(q) => {
            let q = q.clamp(1, 100) as f32;
            let (mut out, size) = run.jpeg_attempt(q, embed)?;
            let score = run.verify(&mut out).ok();
            Ok(mk(out, size, q, score))
        }
        Mode::Smallest(target) => {
            let hi = est_q
                .map(|e| (e as u32 + 8).min(100))
                .unwrap_or(qhi_all)
                .max(30);
            let mut q = smallest_passing(qlo, hi, |q| {
                check_cancel(a.io)?;
                let bytes = sample_bytes(sample, q as f32, false)?;
                let dec = decode_mem(bytes)?;
                Ok::<bool, ImageError>(sample.score(&dec).worst() >= target + MARGIN)
            })?;
            let mut tries = 0;
            loop {
                let (mut out, size) = run.jpeg_attempt(q as f32, embed)?;
                let score = run.verify(&mut out)?;
                if score.worst() >= target || q >= hi || tries >= 4 {
                    return Ok(mk(out, size, q as f32, Some(score)));
                }
                drop(out);
                q = (q + 1 + tries / 2).min(hi);
                tries += 1;
            }
        }
        Mode::UnderSize(max) => {
            // Predict the size of the whole file from the sample, then correct the prediction with
            // what each real pass produces. Quality moves in sixteenth steps, like the
            // in-memory search, so the file lands close under the size asked for.
            const STEPS: u32 = 16;
            let at = |i: u32| i as f32 / STEPS as f32;
            let scale = run.width as f64 * run.height as f64
                / (sample.image().width as f64 * sample.image().height as f64);
            let overhead = (embed.icc.as_ref().map_or(0, Vec::len)
                + embed.exif.as_ref().map_or(0, Vec::len)
                + embed.xmp.as_ref().map_or(0, Vec::len)) as f64;
            let predict = |i: u32| -> Result<f64> {
                let b = sample_bytes(sample, at(i), true)?;
                Ok(b.len() as f64 * scale + overhead)
            };
            let mut k = 1.0f64;
            let mut best: Option<(Output, u64, f32)> = None;
            let mut tried_at: Vec<u32> = Vec::new();
            let mut floor = None;
            for _ in 0..6 {
                check_cancel(a.io)?;
                let found = largest_fitting(qlo * STEPS, qhi_all * STEPS, |i| {
                    check_cancel(a.io)?;
                    Ok::<bool, ImageError>(predict(i)? * k <= max as f64)
                })?;
                let Some(i) = found else {
                    // Even the lowest quality is predicted over the size: try it for real.
                    if !tried_at.contains(&(qlo * STEPS)) {
                        let (out, size) = run.jpeg_attempt(at(qlo * STEPS), embed)?;
                        floor = Some(size);
                        tried_at.push(qlo * STEPS);
                        if size <= max {
                            best = Some((out, size, at(qlo * STEPS)));
                        }
                    }
                    break;
                };
                if tried_at.contains(&i) {
                    break;
                }
                tried_at.push(i);
                let (out, size) = run.jpeg_attempt(at(i), embed)?;
                let pred = predict(i)?;
                if size <= max && best.as_ref().is_none_or(|b| at(i) > b.2) {
                    let close = (max - size) as f64 / max as f64 <= 0.02;
                    best = Some((out, size, at(i)));
                    if close {
                        break;
                    }
                }
                k = size as f64 / pred.max(1.0) * 0.995;
            }
            let Some((mut out, size, q)) = best else {
                let floor = floor.map(human_bytes).unwrap_or_else(|| "more".into());
                return Err(ImageError::Failed(format!(
                    "JPG cannot get this picture under {}: the smallest it gets at its current size is {floor}. Make the picture smaller first, or allow a larger size.",
                    human_bytes(max)
                )));
            };
            let score = run.verify(&mut out).ok();
            Ok(mk(out, size, q, score))
        }
    }
}
