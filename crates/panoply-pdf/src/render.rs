//! Drawing pages with pdfium: PDF to PNG and JPG, and the page renders the compression checks
//! compare.
//!
//! A page is drawn in horizontal strips of at most 16 MB, so a poster at 1200 DPI never needs a
//! bitmap over 64 MB, and PNG output is written strip by strip in constant memory. JPG output
//! needs the whole page in memory and is refused beyond 128 MB of pixels, with a plain message.

use std::fs::File;
use std::io::Write;

use panoply_core::CancelToken;
use pdfium_render::prelude::*;

use crate::error::{PdfError, Result};
use crate::pdfium::engine;

/// Largest strip drawn at once, in bytes of pixels.
const STRIP_BYTES: usize = 16 * 1024 * 1024;
/// Most pixel bytes held for one JPG page.
const JPG_PAGE_BYTES: usize = 128 * 1024 * 1024;
/// Longest side of a page image: JPEG stores sizes in 16 bits, and PNG tools give up well before.
const MAX_SIDE: u32 = 65_000;

/// Output format of a page image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// PNG, lossless.
    Png,
    /// JPEG.
    Jpg,
}

impl ImageFormat {
    /// File extension without the dot.
    pub fn ext(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpg => "jpg",
        }
    }
}

/// An open document, ready to draw.
pub struct RenderDoc {
    doc: Option<PdfDocument<'static>>,
}

impl Drop for RenderDoc {
    fn drop(&mut self) {
        // Closing a document calls into pdfium.
        let _guard = crate::pdfium::lock();
        self.doc.take();
    }
}

fn map_err(e: PdfiumError, has_password: bool) -> PdfError {
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            if has_password {
                PdfError::BadInput("That password did not open this PDF.".into())
            } else {
                PdfError::Encrypted
            }
        }
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            PdfError::Damaged("pdfium could not read the file".into())
        }
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::SecurityError) => {
            PdfError::Unsupported(
                "This PDF uses a security setting the PDF engine cannot open.".into(),
            )
        }
        other => PdfError::Engine(format!(
            "The PDF engine could not draw this file ({other:?})."
        )),
    }
}

impl RenderDoc {
    /// Open a file for drawing. `password` opens a password-protected file.
    pub fn open(file: File, password: Option<&str>) -> Result<RenderDoc> {
        let engine = engine()?;
        let _guard = crate::pdfium::lock();
        let doc = engine
            .load_pdf_from_reader(file, password)
            .map_err(|e| map_err(e, password.is_some()))?;
        Ok(RenderDoc { doc: Some(doc) })
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        let _guard = crate::pdfium::lock();
        self.pdf().pages().len().max(0) as usize
    }

    fn pdf(&self) -> &PdfDocument<'static> {
        self.doc
            .as_ref()
            .expect("a RenderDoc holds its document until dropped")
    }

    /// Size of a page in points after rotation.
    pub fn page_size(&self, index: usize) -> Result<(f32, f32)> {
        let _guard = crate::pdfium::lock();
        self.page_size_locked(index)
    }

    fn page_size_locked(&self, index: usize) -> Result<(f32, f32)> {
        let page = self.page(index)?;
        Ok((page.width().value, page.height().value))
    }

    fn page(&self, index: usize) -> Result<PdfPage<'_>> {
        self.pdf()
            .pages()
            .get(
                i32::try_from(index)
                    .map_err(|_| PdfError::BadInput("Page number too large.".into()))?,
            )
            .map_err(|e| map_err(e, false))
    }

    /// Pixel size of a page at a resolution.
    pub fn pixel_size(&self, index: usize, dpi: f32) -> Result<(u32, u32)> {
        let _guard = crate::pdfium::lock();
        self.pixel_size_locked(index, dpi)
    }

    fn pixel_size_locked(&self, index: usize, dpi: f32) -> Result<(u32, u32)> {
        let (w, h) = self.page_size_locked(index)?;
        let px = |pt: f32| ((pt * dpi / 72.0).round() as u32).max(1);
        Ok((px(w), px(h)))
    }

    /// Draw a whole page as 8-bit gray, scaled down if needed to stay under `max_pixels`.
    /// Returns width, height and the pixels.
    pub fn render_gray(
        &self,
        index: usize,
        dpi: f32,
        max_pixels: usize,
    ) -> Result<(usize, usize, Vec<u8>)> {
        let _guard = crate::pdfium::lock();
        let page = self.page(index)?;
        let (mut w, mut h) = {
            let (pw, ph) = (page.width().value, page.height().value);
            (
                ((pw * dpi / 72.0).round() as usize).max(1),
                ((ph * dpi / 72.0).round() as usize).max(1),
            )
        };
        if w * h > max_pixels {
            let f = (max_pixels as f64 / (w * h) as f64).sqrt();
            w = ((w as f64 * f) as usize).max(1);
            h = ((h as f64 * f) as usize).max(1);
        }
        // Drawn as colour and reduced to luma: pdfium's gray bitmaps are not worth the risk.
        let rgb = self.render_strip(&page, w as u32, h as u32, 0, h as u32)?;
        let out = crate::ssim::luma(&rgb, 3);
        Ok((w, h, out))
    }

    /// Draw rows `y0..y0 + rows` of a page of `w x h` pixels as RGB.
    fn render_strip(
        &self,
        page: &PdfPage<'_>,
        w: u32,
        h: u32,
        y0: u32,
        rows: u32,
    ) -> Result<Vec<u8>> {
        let mut bitmap = PdfBitmap::empty(w as Pixels, rows as Pixels, PdfBitmapFormat::BGR)
            .map_err(|e| map_err(e, false))?;
        let cfg = PdfRenderConfig::new()
            .set_fixed_size(w as Pixels, h as Pixels)
            .set_origin(0, -(y0 as Pixels))
            .set_format(PdfBitmapFormat::BGR)
            .clear_before_rendering(true)
            .set_clear_color(PdfColor::WHITE)
            .render_form_data(false);
        page.render_into_bitmap_with_config(&mut bitmap, &cfg)
            .map_err(|e| map_err(e, false))?;
        let raw = bitmap.as_raw_bytes();
        let stride = raw.len() / rows as usize;
        let mut rgb = Vec::with_capacity(w as usize * rows as usize * 3);
        for y in 0..rows as usize {
            for px in raw[y * stride..y * stride + w as usize * 3]
                .as_chunks::<3>()
                .0
                .iter()
            {
                rgb.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }
        Ok(rgb)
    }

    /// Draw one page to `out` as a PNG or JPG at `dpi`. Returns the image size in pixels.
    pub fn write_page_image(
        &self,
        index: usize,
        dpi: f32,
        format: ImageFormat,
        quality: u8,
        out: &mut dyn Write,
        cancel: &CancelToken,
    ) -> Result<(u32, u32)> {
        let _guard = crate::pdfium::lock();
        let page = self.page(index)?;
        let (w, h) = self.pixel_size_locked(index, dpi)?;
        if w > MAX_SIDE || h > MAX_SIDE {
            return Err(PdfError::BadInput(format!(
                "Page {} would be {w} x {h} pixels at {dpi} DPI, which is too large. Lower the resolution.",
                index + 1
            )));
        }
        let strip_rows = ((STRIP_BYTES / (w as usize * 3)).max(16) as u32).min(h);
        match format {
            ImageFormat::Png => {
                let mut enc = png::Encoder::new(&mut *out, w, h);
                enc.set_color(png::ColorType::Rgb);
                enc.set_depth(png::BitDepth::Eight);
                enc.set_compression(png::Compression::Fast);
                enc.set_filter(png::Filter::Adaptive);
                let ppm = (dpi as f64 / 0.0254).round() as u32;
                enc.set_pixel_dims(Some(png::PixelDimensions {
                    xppu: ppm,
                    yppu: ppm,
                    unit: png::Unit::Meter,
                }));
                let mut writer = enc.write_header().map_err(png_err)?;
                let mut stream = writer.stream_writer().map_err(png_err)?;
                let mut y = 0;
                while y < h {
                    if cancel.is_cancelled() {
                        return Err(PdfError::Cancelled);
                    }
                    let rows = strip_rows.min(h - y);
                    let rgb = self.render_strip(&page, w, h, y, rows)?;
                    stream.write_all(&rgb)?;
                    y += rows;
                }
                stream.finish().map_err(png_err)?;
            }
            ImageFormat::Jpg => {
                let bytes = w as usize * h as usize * 3;
                if bytes > JPG_PAGE_BYTES {
                    return Err(PdfError::BadInput(format!(
                        "Page {} is too large to save as a JPG at {dpi} DPI ({w} x {h} pixels). Lower the resolution or save it as a PNG.",
                        index + 1
                    )));
                }
                let mut rgb = Vec::with_capacity(bytes);
                let mut y = 0;
                while y < h {
                    if cancel.is_cancelled() {
                        return Err(PdfError::Cancelled);
                    }
                    let rows = strip_rows.min(h - y);
                    rgb.extend_from_slice(&self.render_strip(&page, w, h, y, rows)?);
                    y += rows;
                }
                let jpg = crate::jpeg::encode(
                    &rgb,
                    w as usize,
                    h as usize,
                    3,
                    crate::jpeg::EncodeOptions {
                        tables: crate::jpeg::Tables::Standard,
                        quality: quality.clamp(1, 100),
                        progressive: false,
                        subsample: quality < 90,
                    },
                )
                .ok_or_else(|| PdfError::Engine("The JPG encoder failed.".into()))?;
                out.write_all(&jpg)?;
            }
        }
        out.flush()?;
        Ok((w, h))
    }
}

fn png_err(e: png::EncodingError) -> PdfError {
    match e {
        png::EncodingError::IoError(e) => PdfError::from(e),
        other => PdfError::Engine(format!("The PNG encoder failed ({other}).")),
    }
}

/// How alike two documents look when drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Likeness {
    /// SSIM of each compared page, in the order compared.
    pub pages: Vec<(usize, f64)>,
    /// Mean of the page scores.
    pub mean: f64,
    /// Lowest page score.
    pub min: f64,
}

/// Draw the same pages of two documents at `dpi` and compare them with SSIM. At most
/// `max_pages` pages are compared, spread evenly with the first and last always included.
pub fn compare(a: &RenderDoc, b: &RenderDoc, dpi: f32, max_pages: usize) -> Result<Likeness> {
    let n = a.page_count().min(b.page_count());
    if n == 0 {
        return Err(PdfError::BadInput("There are no pages to compare.".into()));
    }
    let picks: Vec<usize> = if n <= max_pages {
        (0..n).collect()
    } else {
        let mut v: Vec<usize> = (0..max_pages)
            .map(|i| i * (n - 1) / (max_pages - 1))
            .collect();
        v.dedup();
        v
    };
    let mut pages = Vec::new();
    for i in picks {
        let (w, h, pa) = a.render_gray(i, dpi, 12_000_000)?;
        let (w2, h2, pb) = b.render_gray(i, dpi, 12_000_000)?;
        let s = if (w, h) == (w2, h2) {
            crate::ssim::ssim_gray(&pa, &pb, w, h)
        } else {
            // The page size changed: that is a difference in itself.
            0.0
        };
        pages.push((i, s));
    }
    let mean = pages.iter().map(|p| p.1).sum::<f64>() / pages.len() as f64;
    let min = pages.iter().map(|p| p.1).fold(1.0, f64::min);
    Ok(Likeness { pages, mean, min })
}
