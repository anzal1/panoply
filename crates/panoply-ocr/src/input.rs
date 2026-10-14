//! Getting pixels to read: picture files and PDF pages.

use std::fs::File;
use std::io::{Seek, SeekFrom};

use panoply_core::CancelToken;

use crate::error::{OcrError, Result};
use crate::types::OcrImage;

/// Longest side of a picture handed to an engine. Bigger pictures are reduced on the way in.
pub const MAX_SIDE: u32 = 8192;

/// Most pixels of a picture handed to an engine, and of a page drawn for reading. On macOS it is
/// six million, A4 at about 250 dpi, where 10 point text is 35 pixels tall and reads as well as at
/// 300 dpi: Apple Vision's memory grows by about 20 MB per million pixels on top of 156 MB
/// (measured on a 31 MP photograph: 660 MB at full size), and this keeps a job inside the 300 MB
/// budget. The other engines have no such cost and keep the larger limit, so a page is read at
/// 300 dpi as before.
#[cfg(target_os = "macos")]
pub const MAX_PIXELS: usize = 6_000_000;

/// See the macOS value above.
#[cfg(not(target_os = "macos"))]
pub const MAX_PIXELS: usize = 36_000_000;

/// Read a picture file as an upright (EXIF orientation applied), sRGB, 8 bit image, with any
/// transparency flattened onto white.
pub fn load_image(mut file: File, ext_hint: Option<&str>) -> Result<OcrImage> {
    let side = reading_side(&file, ext_hint);
    file.seek(SeekFrom::Start(0))
        .map_err(|e| OcrError::BadInput(e.to_string()))?;
    let p = panoply_image::preview::preview(file, ext_hint, side, false)
        .map_err(|e| OcrError::BadInput(e.to_string()))?;
    OcrImage::from_rgba(p.width, p.height, &p.rgba)
        .ok_or_else(|| OcrError::BadInput("This picture could not be read as pixels.".into()))
}

/// The longest side to read a picture at: [`MAX_SIDE`], or less when the picture has more than
/// [`MAX_PIXELS`]. A file whose header cannot be read here is left to the reader, which says why.
fn reading_side(file: &File, ext_hint: Option<&str>) -> u32 {
    let Ok(probe) = file.try_clone() else {
        return MAX_SIDE;
    };
    let Ok(reader) = panoply_image::decode::open(
        panoply_image::decode::Input::File(probe),
        ext_hint,
        &panoply_image::decode::OpenOpts::default(),
    ) else {
        return MAX_SIDE;
    };
    let info = reader.info();
    side_for(info.full_width, info.full_height)
}

fn side_for(width: u32, height: u32) -> u32 {
    let px = f64::from(width) * f64::from(height);
    if px <= MAX_PIXELS as f64 {
        return MAX_SIDE;
    }
    let k = (MAX_PIXELS as f64 / px).sqrt();
    ((f64::from(width.max(height)) * k).floor() as u32).clamp(16, MAX_SIDE)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_sized_picture_is_read_whole_and_a_huge_one_is_brought_down() {
        // A4 at 200 dpi, and a 4 MP photo, are read as they are.
        assert_eq!(side_for(1654, 2339), MAX_SIDE);
        assert_eq!(side_for(2400, 1700), MAX_SIDE);
        // A 100 megapixel panorama comes down to the pixel cap, keeping its shape.
        let side = side_for(14_000, 7_200);
        let (w, h) = (f64::from(side), f64::from(side) * 7200.0 / 14000.0);
        assert!(w * h <= MAX_PIXELS as f64 * 1.001, "{w} x {h}");
        assert!(side > 3000, "{side}");
    }
}
