//! A JPEG sink that encodes strips of rows on several threads and joins them. Two uses: pictures
//! too large to hold (the trellis profile, for Compress), and speed on a picture too large to
//! hold (the fast profile, for Convert).
//!
//! The first use, in detail:  rows arrive, whole MCU rows are cut into strips,
//! and each strip is encoded as a small baseline JPEG with MozJPEG's trellis quantizer, on a few
//! threads at once. [`super::jpegopt::merge_and_optimize`] then joins the strips into one file
//! with optimal Huffman tables. Memory is a few strips of pixels and coefficients (about 9 bytes
//! a pixel of a strip, strips of about 1 megapixel) plus the compressed strips, which wait in
//! memory up to a limit and in a file beyond it.

use std::borrow::Cow;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::jpeg::{JpegOpts, JpegSink};
use super::jpegopt::StripSource;
use crate::error::{ImageError, Result};
use crate::pipeline::RowSink;
use crate::types::{Depth, Layout, Rows};

/// Compressed strips kept in memory before the rest go to a file.
pub const MEMORY_LIMIT: usize = 48 * 1024 * 1024;
/// Pixels a strip aims at.
const STRIP_PIXELS: usize = 1_000_000;
/// Rows of an MCU: 16 for 4:2:0, so strips are whole multiples of it.
const MCU_ROWS: usize = 16;

enum Loc {
    Mem(Vec<u8>),
    File { offset: u64, len: usize },
}

/// The encoded strips of a picture, top to bottom.
pub struct Strips {
    locs: Vec<Loc>,
    file: Option<Mutex<File>>,
    /// Width of the picture.
    pub width: u32,
    /// Height of the picture.
    pub height: u32,
    /// Bytes of all strips.
    pub bytes: u64,
    /// MCUs in every strip but the last (the restart interval that joins them).
    pub strip_mcus: usize,
}

impl StripSource for Strips {
    fn count(&self) -> usize {
        self.locs.len()
    }

    fn get(&self, i: usize) -> Result<Cow<'_, [u8]>> {
        match &self.locs[i] {
            Loc::Mem(v) => Ok(Cow::Borrowed(v)),
            Loc::File { offset, len } => {
                let mut f = self
                    .file
                    .as_ref()
                    .ok_or_else(|| ImageError::Failed("internal: strips lost their file".into()))?
                    .lock()
                    .map_err(|_| ImageError::Failed("internal: strip file lock".into()))?;
                f.seek(SeekFrom::Start(*offset))?;
                let mut buf = vec![0u8; *len];
                f.read_exact(&mut buf)?;
                Ok(Cow::Owned(buf))
            }
        }
    }
}

struct Job {
    index: usize,
    width: u32,
    rows: u32,
    layout: Layout,
    data: Vec<u8>,
    opts: JpegOpts,
}

type Done = (usize, Result<Vec<u8>>);

fn encode_job(job: Job) -> Result<Vec<u8>> {
    let mut sink = JpegSink::new(Vec::new(), job.opts);
    sink.begin(job.width, job.rows, job.layout, Depth::U8, None)?;
    sink.push(Rows::U8(&job.data))?;
    sink.finish()
}

/// Opens the scratch file for strips that do not fit in memory.
pub type Spill<'a> = &'a mut dyn FnMut() -> Result<File>;

/// What each strip is encoded with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// MozJPEG's trellis quantizer and optimized tables on each strip (about 9 bytes a pixel of
    /// the strip), then global optimal tables when the strips are joined. The best baseline
    /// file, and the one Compress uses.
    Best,
    /// libjpeg's plain quantizer and the standard tables, no coefficient buffer, strips joined
    /// with restart markers ([`super::jpegopt::join_restart`]): no second pass over the data at
    /// all, so the cost is the encode, split over the threads. What Convert uses above 24
    /// megapixels, where the whole picture cannot be held. Files are 2 to 17 percent larger than
    /// with optimized tables.
    Fast,
}

/// See the module documentation.
pub struct StripSink<'a> {
    template: JpegOpts,
    memory_limit: usize,
    spill: Spill<'a>,
    threads: usize,
    // Set at `begin`.
    width: u32,
    height: u32,
    layout: Layout,
    row_len: usize,
    strip_rows: usize,
    fixed_rows: Option<usize>,
    buf: Vec<u8>,
    sent: usize,
    jobs: Option<SyncSender<Job>>,
    results: Option<Receiver<Done>>,
    workers: Vec<JoinHandle<()>>,
    done: Vec<Option<Loc>>,
    mem_bytes: usize,
    total: u64,
    file: Option<File>,
    file_len: u64,
    error: Option<ImageError>,
}

impl<'a> StripSink<'a> {
    /// A sink encoding with `template` (quality, subsampling, metadata for the first strip; the
    /// trellis flag is switched on and progressive scans off), using up to `threads` threads.
    pub fn new(template: JpegOpts, threads: usize, spill: Spill<'a>) -> Self {
        Self::with_profile(Profile::Best, template, threads, spill)
    }

    /// Like [`StripSink::new`] with a choice of [`Profile`].
    pub fn with_profile(
        profile: Profile,
        mut template: JpegOpts,
        threads: usize,
        spill: Spill<'a>,
    ) -> Self {
        template.progressive = false;
        match profile {
            Profile::Best => {
                // MozJPEG's trellis quantizer writes files that decode wrongly on some pictures
                // when the Huffman tables are not optimized (measured: a strip of a real
                // photograph decoded with a mean squared error of 1324 against 6), so a strip
                // always optimizes its own tables. The strip holds a full coefficient buffer for
                // the trellis anyway.
                template.optimize = true;
                template.trellis = true;
            }
            Profile::Fast => {
                // Standard tables and no coefficient buffer: a strip is encoded as it arrives.
                template.optimize = false;
                template.trellis = false;
            }
        }
        StripSink {
            template,
            memory_limit: MEMORY_LIMIT,
            spill,
            threads: threads.max(1),
            width: 0,
            height: 0,
            layout: Layout::Rgb,
            row_len: 0,
            strip_rows: MCU_ROWS,
            fixed_rows: None,
            buf: Vec::new(),
            sent: 0,
            jobs: None,
            results: None,
            workers: Vec::new(),
            done: Vec::new(),
            mem_bytes: 0,
            total: 0,
            file: None,
            file_len: 0,
            error: None,
        }
    }

    /// Keep every compressed strip in memory, however many there are (for a picture that is
    /// already in memory, whose file cannot be larger than the picture).
    pub fn in_memory(mut self) -> Self {
        self.memory_limit = usize::MAX;
        self
    }

    /// Use strips of about `rows` rows (rounded down to whole MCU rows), whatever the width.
    pub fn with_strip_rows(mut self, rows: usize) -> Self {
        self.fixed_rows = Some(rows.max(MCU_ROWS) / MCU_ROWS * MCU_ROWS);
        self
    }

    fn take_results(&mut self, block: bool) -> Result<()> {
        loop {
            let Some(rx) = &self.results else {
                return Ok(());
            };
            let msg = if block {
                match rx.recv() {
                    Ok(m) => m,
                    Err(_) => return Ok(()),
                }
            } else {
                match rx.try_recv() {
                    Ok(m) => m,
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(()),
                }
            };
            let (idx, res) = msg;
            let bytes = match res {
                Ok(b) => b,
                Err(e) => {
                    self.error.get_or_insert(e);
                    Vec::new()
                }
            };
            self.total += bytes.len() as u64;
            let loc = if self.mem_bytes + bytes.len() <= self.memory_limit && self.file.is_none() {
                self.mem_bytes += bytes.len();
                Loc::Mem(bytes)
            } else {
                if self.file.is_none() {
                    self.file = Some((self.spill)()?);
                }
                let f = self.file.as_mut().expect("opened above");
                f.seek(SeekFrom::Start(self.file_len))?;
                f.write_all(&bytes)?;
                let offset = self.file_len;
                self.file_len += bytes.len() as u64;
                Loc::File {
                    offset,
                    len: bytes.len(),
                }
            };
            self.done[idx] = Some(loc);
            if block && self.done.iter().all(Option::is_some) {
                return Ok(());
            }
        }
    }

    fn send(&mut self, rows: usize) -> Result<()> {
        let take = rows * self.row_len;
        let data: Vec<u8> = self.buf.drain(..take).collect();
        let index = self.sent;
        self.sent += 1;
        self.done.push(None);
        let mut opts = self.template.clone();
        if index > 0 {
            // Metadata lives in the first strip, whose header is the one the file keeps.
            opts.embed = Default::default();
        }
        let job = Job {
            index,
            width: self.width,
            rows: rows as u32,
            layout: self.layout,
            data,
            opts,
        };
        let tx = self.jobs.as_ref().expect("begin was called");
        tx.send(job)
            .map_err(|_| ImageError::Failed("internal: the strip encoders stopped".into()))?;
        self.take_results(false)?;
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        Ok(())
    }

    /// Finish and return the strips.
    pub fn finish(mut self) -> Result<Strips> {
        let left = self.buf.len() / self.row_len.max(1);
        if left > 0 {
            self.send(left)?;
        }
        self.jobs = None; // workers see the channel close and stop
        self.take_results(true)?;
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
        if let Some(e) = self.error.take() {
            return Err(e);
        }
        let locs = self
            .done
            .drain(..)
            .map(|l| l.ok_or_else(|| ImageError::Failed("internal: a strip was lost".into())))
            .collect::<Result<Vec<_>>>()?;
        Ok(Strips {
            locs,
            file: self.file.take().map(Mutex::new),
            width: self.width,
            height: self.height,
            bytes: self.total,
            strip_mcus: {
                // A 4:2:0 MCU is 16 by 16 pixels, a gray or 4:4:4 one 8 by 8.
                let m = if self.layout != Layout::Gray && self.template.subsample {
                    16
                } else {
                    8
                };
                (self.width as usize).div_ceil(m) * (self.strip_rows / m).max(1)
            },
        })
    }
}

impl RowSink for StripSink<'_> {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        _icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        if depth != Depth::U8 || layout.has_alpha() {
            return Err(ImageError::Failed(
                "internal: JPEG needs 8 bit rows without alpha".into(),
            ));
        }
        if width > super::jpeg::MAX_SIDE || height > super::jpeg::MAX_SIDE {
            return Err(ImageError::TooLarge(format!(
                "JPEG files cannot be larger than {} pixels on a side, and this picture is {width} x {height}. Use PNG, or make the picture smaller.",
                super::jpeg::MAX_SIDE
            )));
        }
        self.width = width;
        self.height = height;
        self.layout = layout;
        self.row_len = width as usize * layout.channels();
        let rows = (STRIP_PIXELS / width.max(1) as usize).clamp(MCU_ROWS, 512);
        self.strip_rows = self.fixed_rows.unwrap_or(rows / MCU_ROWS * MCU_ROWS);
        let (jtx, jrx) = sync_channel::<Job>(self.threads);
        let (rtx, rrx) = sync_channel::<Done>(self.threads * 4);
        let jrx = Arc::new(Mutex::new(jrx));
        for _ in 0..self.threads {
            let (jrx, rtx) = (jrx.clone(), rtx.clone());
            self.workers.push(std::thread::spawn(move || {
                loop {
                    let job = match jrx.lock() {
                        Ok(g) => g.recv(),
                        Err(_) => return,
                    };
                    let Ok(job) = job else { return };
                    let index = job.index;
                    let res = encode_job(job);
                    if rtx.send((index, res)).is_err() {
                        return;
                    }
                }
            }));
        }
        self.jobs = Some(jtx);
        self.results = Some(rrx);
        Ok(())
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        let Rows::U8(s) = rows else {
            return Err(ImageError::Failed("internal: JPEG needs 8 bit rows".into()));
        };
        self.buf.extend_from_slice(s);
        // Every strip but the last is a whole number of MCU rows; the rest waits for `finish`.
        while self.buf.len() / self.row_len.max(1) >= self.strip_rows {
            self.send(self.strip_rows)?;
        }
        Ok(())
    }
}

impl Drop for StripSink<'_> {
    fn drop(&mut self) {
        // Close the job channel so any worker still waiting stops; they hold no borrowed data.
        self.jobs = None;
        self.results = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::jpegopt::merge_and_optimize;
    use crate::types::{Image, Samples};

    fn gray(w: u32, h: u32) -> Image {
        let mut img = Image::new(w, h, Layout::Gray, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            let mut s = 7u32;
            for (i, p) in v.iter_mut().enumerate() {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                *p = ((i % w as usize) as u32 * 255 / w + (s >> 27)) as u8;
            }
        }
        img
    }

    #[test]
    fn rows_in_any_batches_become_strips_that_join_into_the_picture() {
        let (w, h) = (130u32, 197u32);
        let img = gray(w, h);
        let mut spill = || -> Result<File> { Err(ImageError::Failed("no spill expected".into())) };
        let mut sink = StripSink::new(
            JpegOpts {
                quality: 80.0,
                ..Default::default()
            },
            3,
            &mut spill,
        )
        .with_strip_rows(48);
        sink.begin(w, h, Layout::Gray, Depth::U8, None).unwrap();
        // Odd batch sizes, so strips straddle pushes.
        let mut y = 0;
        for n in [1u32, 50, 7, 64, 33, 42] {
            sink.push(img.rows(y, n)).unwrap();
            y += n;
        }
        assert_eq!(y, h);
        let strips = sink.finish().unwrap();
        assert_eq!(strips.count(), 5, "197 rows in strips of 48");
        let mut want = Vec::new();
        for i in 0..strips.count() {
            let part = crate::ops::common::decode_mem(strips.get(i).unwrap().into_owned()).unwrap();
            if let Samples::U8(v) = part.samples {
                want.extend(v);
            }
        }
        let mut joined = Vec::new();
        merge_and_optimize(&strips, h as usize, &mut joined).unwrap();
        let got = crate::ops::common::decode_mem(joined).unwrap();
        assert_eq!((got.width, got.height), (w, h));
        let Samples::U8(got) = got.samples else {
            unreachable!()
        };
        assert_eq!(got, want);
    }
}
