//! Getting pixels to read: picture files and PDF pages.

use std::fs::File;

use panoply_core::CancelToken;

use crate::error::{OcrError, Result};
use crate::types::OcrImage;

/// Longest side of a picture handed to an engine. Bigger pictures are reduced on the way in.
pub const MAX_SIDE: u32 = 8192;

/// Read a picture file as an upright (EXIF orientation applied), sRGB, 8 bit image, with any
/// transparency flattened onto white.
pub fn load_image(file: File, ext_hint: Option<&str>) -> Result<OcrImage> {
    let p = panoply_image::preview::preview(file, ext_hint, MAX_SIDE, false)
        .map_err(|e| OcrError::BadInput(e.to_string()))?;
    OcrImage::from_rgba(p.width, p.height, &p.rgba)
        .ok_or_else(|| OcrError::BadInput("This picture could not be read as pixels.".into()))
}

/// A PDF opened for drawing pages to read.
///
/// Pages are drawn with pdfium's own scaling (`scale_page_by_factor`), not the fixed size and
/// strip origin that `panoply-pdf`'s page-to-picture code uses: on layered scans (JPEG 2000
/// background with a JBIG2 or JPEG mask, as the Internet Archive writes them) the fixed size
/// path draws the page at one pixel per point in the corner of the bitmap, which reads as an
/// almost empty page. Measured on `ia-scan-public-lands.pdf` page 15.
pub struct PageSource {
    doc: Option<pdfium_render::prelude::PdfDocument<'static>>,
}

impl Drop for PageSource {
    fn drop(&mut self) {
        // Closing a document calls into pdfium.
        let _guard = panoply_pdf::pdfium::lock();
        self.doc.take();
    }
}

impl PageSource {
    /// Open a PDF file.
    pub fn open(file: File) -> Result<PageSource> {
        let engine = panoply_pdf::pdfium::engine()?;
        let _guard = panoply_pdf::pdfium::lock();
        let doc = engine.load_pdf_from_reader(file, None).map_err(|e| {
            OcrError::BadInput(format!("The PDF engine could not open this file ({e:?})."))
        })?;
        Ok(PageSource { doc: Some(doc) })
    }

    /// Draw a page as 8 bit gray at `dpi`, scaled down if it would exceed `max_pixels`.
    /// Returns the image and the scale in pixels per point actually used.
    pub fn render(
        &self,
        index: usize,
        dpi: f32,
        max_pixels: usize,
        cancel: &CancelToken,
    ) -> Result<(OcrImage, f32)> {
        use pdfium_render::prelude::*;
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let _guard = panoply_pdf::pdfium::lock();
        let doc = self
            .doc
            .as_ref()
            .expect("a PageSource holds its document until dropped");
        let page = doc
            .pages()
            .get(
                i32::try_from(index)
                    .map_err(|_| OcrError::BadInput("Page number too large.".into()))?,
            )
            .map_err(|e| {
                OcrError::BadInput(format!(
                    "The PDF engine could not open page {} ({e:?}).",
                    index + 1
                ))
            })?;
        let (pw, ph) = (page.width().value.max(1.0), page.height().value.max(1.0));
        let mut scale = dpi / 72.0;
        let px = |k: f32| ((pw * k).round() as usize).max(1) * ((ph * k).round() as usize).max(1);
        if px(scale) > max_pixels {
            scale *= (max_pixels as f32 / px(scale) as f32).sqrt();
        }
        let cfg = PdfRenderConfig::new()
            .scale_page_by_factor(scale)
            .set_format(PdfBitmapFormat::BGR)
            .clear_before_rendering(true)
            .set_clear_color(PdfColor::WHITE)
            .render_form_data(false);
        let bmp = page.render_with_config(&cfg).map_err(|e| {
            OcrError::Engine(format!(
                "The PDF engine could not draw page {} ({e:?}).",
                index + 1
            ))
        })?;
        let (w, h) = (bmp.width() as usize, bmp.height() as usize);
        let raw = bmp.as_raw_bytes();
        let stride = raw.len() / h.max(1);
        let mut gray = Vec::with_capacity(w * h);
        for y in 0..h {
            for p in raw[y * stride..y * stride + w * 3].chunks_exact(3) {
                // BGR to Rec. 601 luma.
                gray.push(
                    ((u32::from(p[2]) * 299 + u32::from(p[1]) * 587 + u32::from(p[0]) * 114) / 1000)
                        as u8,
                );
            }
        }
        let img = OcrImage::gray(w as u32, h as u32, gray)
            .ok_or_else(|| OcrError::Engine("The page could not be drawn for reading.".into()))?;
        Ok((img, w as f32 / pw))
    }
}
