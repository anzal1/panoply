use std::sync::Mutex;

use objc2_core_foundation::{
    CFBoolean, CFData, CFNumber, CFRetained, CFString, CFType, CGAffineTransform, CGPoint, CGRect,
    CGSize,
};
use objc2_core_graphics::{
    CGColorSpace, CGColorSpaceModel, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
    kCGColorSpaceDisplayP3, kCGColorSpaceSRGB,
};
use objc2_image_io::{
    CGImageSourceStatus, kCGImageAuxiliaryDataTypeDepth, kCGImageAuxiliaryDataTypeDisparity,
    kCGImageAuxiliaryDataTypeHDRGainMap, kCGImageAuxiliaryDataTypeISOGainMap,
    kCGImageAuxiliaryDataTypePortraitEffectsMatte, kCGImagePropertyDepth,
    kCGImagePropertyOrientation, kCGImageSourceCreateThumbnailFromImageAlways,
    kCGImageSourceCreateThumbnailFromImageIfAbsent, kCGImageSourceCreateThumbnailWithTransform,
    kCGImageSourceDecodeRequest, kCGImageSourceDecodeToSDR, kCGImageSourceShouldCache,
    kCGImageSourceShouldCacheImmediately, kCGImageSourceThumbnailMaxPixelSize,
};

use crate::cf::{self, PropDict, sys};
use crate::error::Error;
use crate::props::Properties;
use crate::raw::{self, RawImage};
use crate::sys as ffi;

/// The largest image, in pixels, this crate opens at all. This is a hard stop for dimension
/// bombs, not a memory promise: 400 megapixels is twice the largest tiled HEIC in the corpus
/// (200 MP). See `MAX_UNTILED_PIXELS` for the limit that matches the 300 MB budget.
pub const MAX_PIXELS: u64 = 400_000_000;

/// The largest image, in pixels, accepted when ImageIO cannot decode it in bands.
///
/// ImageIO decodes JPEG by strips and HEIC and AVIF grids by tile, so a band read only pays for
/// the band. Any other file (an AVIF without a grid, PNG, WebP, GIF and so on) is decoded whole
/// on the first read, at up to 8 bytes per pixel. 37.5 megapixels at 8 bytes is 300 MB, which is
/// the budget, so that is the limit for those files.
pub const MAX_UNTILED_PIXELS: u64 = 37_500_000;

/// The widest row this crate will draw. One band of one row must fit in memory.
pub const MAX_DIMENSION: u32 = 1 << 20;

/// What the file is. See the crate docs for each field.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// Stored pixel width of the primary image, before orientation.
    pub width: u32,
    /// Stored pixel height of the primary image, before orientation.
    pub height: u32,
    /// EXIF orientation 1 to 8 as ImageIO reports it.
    pub orientation: u8,
    /// True when the image has an alpha channel.
    pub has_alpha: bool,
    /// Bits per component as ImageIO reports it (8, 10, 16 and so on).
    pub bits_per_component: u8,
    /// Number of images in the file. The primary is index 0.
    pub frames: usize,
    /// True for animation sequences.
    pub is_animated: bool,
    /// Uniform type identifier, for example `public.heic`.
    pub uti: String,
    /// ICC bytes of the source color space, when it has any.
    pub icc: Option<Vec<u8>>,
    /// Name of the source color space when it is a named one.
    pub colorspace_name: Option<String>,
    /// PQ or HLG transfer, or a gain map is present.
    pub is_hdr: bool,
    /// A depth or disparity map is present.
    pub has_depth: bool,
    /// An HDR gain map is present.
    pub has_gain_map: bool,
    /// An Apple portrait effects matte is present.
    pub has_portrait_matte: bool,
}

/// Which RGB color space pixels are converted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    /// sRGB.
    Srgb,
    /// Display P3.
    DisplayP3,
    /// The image's own RGB space when it is usable, otherwise sRGB.
    Source,
}

/// What the decoder keeps per opened frame: the lazy image and what we read off it once.
struct FrameCtx {
    index: usize,
    image: CFRetained<CGImage>,
    geometry: FrameGeometry,
}

/// One decoded block of upright rows, kept so small sequential bands do not decode the same
/// tiles again and again.
struct Block {
    key: (usize, Space, bool),
    y_start: usize,
    rows: usize,
    data: Vec<u8>,
}

/// Where the CoreImage RAW developer stands for this file.
enum RawSlot {
    /// Not tried yet: the first read tries it.
    Untried,
    /// Developing through CoreImage.
    Ready(RawImage),
    /// CoreImage cannot take this file, or its size disagrees with ImageIO's: use ImageIO.
    Failed,
}

struct Inner {
    source: CFRetained<objc2_image_io::CGImageSource>,
    // Kept so ImageIO's view of the bytes stays alive as long as the source.
    _data: CFRetained<CFData>,
    banded: bool,
    frame: Option<FrameCtx>,
    block: Option<Block>,
    raw: RawSlot,
}

// SAFETY: ImageIO sources and CGImages are documented as safe to use from any thread, but not
// to be used concurrently without care. The same holds for the CoreImage filter and context in
// `RawSlot`. The type is only reachable through a `Mutex`, so all access is serialised and
// moving it between threads is sound.
unsafe impl Send for Inner {}

/// A lazy image decoder over one file held in memory.
pub struct Decoder {
    info: Info,
    props: Properties,
    primary: usize,
    count: usize,
    /// Tile size of the primary image when the file is a HEIF grid, in stored pixels.
    tile: Option<(u32, u32)>,
    /// True when ImageIO can decode this file band by band (JPEG strips, HEIF grid tiles).
    banded: bool,
    /// The primary image is camera RAW, which is developed through CoreImage when it can be.
    raw_file: bool,
    /// Upright size of the reduced picture this decoder serves, when it was opened with
    /// [`Reduce`] and the picture was large enough to be worth reducing.
    reduced: Option<(u32, u32)>,
    inner: Mutex<Inner>,
}

/// The smallest picture the caller can use, in pixels of the upright image. A decoder opened
/// with it may serve a smaller picture than the file holds (never smaller than this on either
/// side), decoded at that size instead of full size. That is what keeps a 33 MP 10 bit HEIF
/// grid or a 12 MP RAW inside the memory budget when the job only needs a thumbnail sized
/// result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reduce {
    /// Smallest length of the short side.
    pub min_short: u32,
    /// Smallest length of the long side.
    pub min_long: u32,
}

/// A small upright picture, 8 bit RGBA, straight from the file's own thumbnail or a reduced
/// decode.
#[derive(Debug, Clone)]
pub struct Thumb {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of RGBA, premultiplied, in sRGB.
    pub rgba: Vec<u8>,
    /// True when the picture is the thumbnail the file carries rather than a decode of the
    /// full picture.
    pub embedded: bool,
}

/// The tile edge most HEIC encoders use, and the nominal block height for the row cache.
const TILE: usize = 512;
/// The row cache never holds more than this many bytes.
const BLOCK_BYTES: usize = 96 << 20;

fn image_options() -> CFRetained<PropDict> {
    let f: &CFType = CFBoolean::new(false).as_ref();
    cf::make_dict(&[
        (sys!(kCGImageSourceShouldCache), f),
        (sys!(kCGImageSourceShouldCacheImmediately), f),
        // Ask for the SDR rendition explicitly, so a gain map is never applied behind our back.
        (
            sys!(kCGImageSourceDecodeRequest),
            sys!(kCGImageSourceDecodeToSDR).as_ref(),
        ),
    ])
}

fn orientation_of(props: &PropDict) -> u8 {
    match cf::get_i64(props, sys!(kCGImagePropertyOrientation)) {
        Some(v @ 1..=8) => v as u8,
        _ => 1,
    }
}

fn is_animated_uti(uti: &str) -> bool {
    matches!(
        uti,
        "com.compuserve.gif"
            | "public.heics"
            | "public.avis"
            | "org.webmproject.webp"
            | "public.png"
    )
}

fn alpha_of(image: &CGImage) -> bool {
    !matches!(
        CGImage::alpha_info(Some(image)),
        CGImageAlphaInfo::None | CGImageAlphaInfo::NoneSkipFirst | CGImageAlphaInfo::NoneSkipLast
    )
}

/// Markers of a gain map that ImageIO does not report through its auxiliary data API (Samsung
/// HEIC, Ultra HDR XMP). Only the first and last MiB are searched, where the HEIF `meta` box and
/// the XMP packet live.
fn bytes_hint_gain_map(data: &[u8]) -> bool {
    const NEEDLES: [&[u8]; 4] = [
        b"urn:com:samsung:photo:2024:aux:hdrgainmap",
        b"urn:com:apple:photo:2020:aux:hdrgainmap",
        b"urn:iso:std:iso:ts:21496",
        b"hdrgm:GainMapMax",
    ];
    const WINDOW: usize = 1 << 20;
    let head = &data[..data.len().min(WINDOW)];
    let tail = &data[data.len().saturating_sub(WINDOW)..];
    NEEDLES
        .iter()
        .any(|n| contains(head, n) || contains(tail, n))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    let Some((&first, rest)) = needle.split_first() else {
        return true;
    };
    let mut i = 0;
    while let Some(p) = hay[i..].iter().position(|&b| b == first) {
        let at = i + p;
        if hay.len() - at < needle.len() {
            return false;
        }
        if &hay[at + 1..at + needle.len()] == rest {
            return true;
        }
        i = at + 1;
    }
    false
}

impl Decoder {
    /// Opens an image from its bytes. Nothing is decoded yet.
    pub fn open(data: Vec<u8>) -> Result<Decoder, Error> {
        Self::open_reduced(data, None)
    }

    /// Like [`Decoder::open`], for a caller that will shrink the picture anyway. When the file
    /// is much larger than `reduce` asks for, rows come from a decode at a reduced size and
    /// [`Decoder::oriented_size`] reports that size ([`Decoder::full_oriented_size`] is the
    /// file's own). Otherwise this is exactly `open`.
    pub fn open_reduced(data: Vec<u8>, reduce: Option<Reduce>) -> Result<Decoder, Error> {
        if data.len() < 8 {
            return Err(Error::new("This file is too small to be an image."));
        }
        let byte_gain_map = bytes_hint_gain_map(&data);
        let ispe = heif_image_sizes(&data);
        let cfdata = CFData::from_bytes(&data);
        drop(data);
        let opts = image_options();
        let source = ffi::source_with_data(&cfdata, Some(cf::untyped(&opts)))
            .ok_or_else(|| Error::new("This file is not an image the system can read."))?;
        let count = ffi::source_count(&source);
        if count == 0 {
            return Err(Error::new("This file has no readable image in it."));
        }
        let primary = ffi::source_primary_index(&source).min(count - 1);
        if ffi::source_status_at(&source, primary) != CGImageSourceStatus::StatusComplete {
            return Err(Error::new("This image is damaged or cut short."));
        }
        let uti = ffi::source_uti(&source)
            .ok_or_else(|| Error::new("The system could not tell what kind of image this is."))?;
        let frame = open_frame(&source, primary)?;
        let image = &frame.image;
        let tile = guess_tile(
            &ispe,
            frame.geometry.stored_width,
            frame.geometry.stored_height,
        );
        let banded = uti == "public.jpeg" || tile.is_some();
        check_pixels(&frame.geometry, banded)?;
        let props_dict = ffi::source_props_at(&source, primary, Some(cf::untyped(&opts)));
        let g = frame.geometry;

        let reported_depth = CGImage::bits_per_component(Some(image)) as i64;
        let prop_depth = props_dict
            .as_deref()
            .and_then(|p| cf::get_i64(p, sys!(kCGImagePropertyDepth)))
            .unwrap_or(0);
        let bits_per_component = reported_depth.max(prop_depth).clamp(1, 255) as u8;

        let cs = CGImage::color_space(Some(image));
        let icc = cs
            .as_deref()
            .and_then(|c| CGColorSpace::icc_data(Some(c)))
            .map(|d| d.to_vec())
            .filter(|v| !v.is_empty());
        let colorspace_name = cs
            .as_deref()
            .and_then(|c| CGColorSpace::name(Some(c)))
            .map(|n| n.to_string());
        let cs_hdr = cs
            .as_deref()
            .map(|c| c.is_hdr() || c.is_pq_based() || c.is_hlg_based())
            .unwrap_or(false);

        let aux = |kind: &CFString| ffi::source_aux_exists(&source, primary, kind);
        let has_depth = aux(sys!(kCGImageAuxiliaryDataTypeDepth))
            || aux(sys!(kCGImageAuxiliaryDataTypeDisparity));
        let has_gain_map = aux(sys!(kCGImageAuxiliaryDataTypeHDRGainMap))
            || aux(sys!(kCGImageAuxiliaryDataTypeISOGainMap))
            || byte_gain_map;
        let has_portrait_matte = aux(sys!(kCGImageAuxiliaryDataTypePortraitEffectsMatte));

        let info = Info {
            width: g.stored_width,
            height: g.stored_height,
            orientation: g.orientation,
            has_alpha: g.has_alpha,
            bits_per_component,
            frames: count,
            is_animated: count > 1 && is_animated_uti(&uti),
            uti,
            icc,
            colorspace_name,
            is_hdr: cs_hdr || has_gain_map,
            has_depth,
            has_gain_map,
            has_portrait_matte,
        };
        let raw_file = raw::is_raw_uti(&info.uti);
        let mut decoder = Decoder {
            info,
            props: Properties::from_dict(props_dict),
            primary,
            count,
            tile,
            banded,
            raw_file,
            reduced: None,
            inner: Mutex::new(Inner {
                source,
                _data: cfdata,
                banded,
                frame: Some(frame),
                block: None,
                raw: RawSlot::Untried,
            }),
        };
        if let Some(r) = reduce {
            decoder.reduce(r);
        }
        Ok(decoder)
    }

    /// Switches this decoder to a reduced picture when `r` asks for much less than the file has.
    fn reduce(&mut self, r: Reduce) {
        let (w, h) = self.full_oriented_size();
        let (long, short) = (w.max(h), w.min(h));
        let scale = (f64::from(r.min_long) / f64::from(long))
            .max(f64::from(r.min_short) / f64::from(short));
        // A reduction below about a quarter off is not worth a second code path.
        if !(scale > 0.0 && scale < 0.75) || self.count == 0 {
            return;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if self.raw_file {
            if let Some(img) = RawImage::open(
                &inner._data,
                &self.info.uti,
                self.info.orientation,
                scale as f32,
            ) {
                self.reduced = Some(img.size());
                inner.raw = RawSlot::Ready(img);
                return;
            }
            // CoreImage cannot take this file: the thumbnail path below still works, and the
            // full size developer must not be tried on a reduced decoder.
            inner.raw = RawSlot::Failed;
        }
        let max_px = (f64::from(long) * scale).ceil() as u32 + 1;
        let opts = thumbnail_options(max_px, true);
        let Some(img) =
            ffi::source_thumbnail_at(&inner.source, self.primary, Some(cf::untyped(&opts)))
        else {
            return;
        };
        let (tw, th) = (CGImage::width(Some(&img)), CGImage::height(Some(&img)));
        let (Ok(tw), Ok(th)) = (u32::try_from(tw), u32::try_from(th)) else {
            return;
        };
        if tw == 0 || th == 0 || tw > MAX_DIMENSION || th > MAX_DIMENSION {
            return;
        }
        let geometry = FrameGeometry {
            stored_width: tw,
            stored_height: th,
            orientation: 1,
            width: tw,
            height: th,
            has_alpha: alpha_of(&img),
        };
        inner.block = None;
        inner.frame = Some(FrameCtx {
            index: self.primary,
            image: img,
            geometry,
        });
        inner.banded = false;
        drop(inner);
        self.tile = None;
        self.reduced = Some((tw, th));
    }

    /// What the file is.
    pub fn info(&self) -> &Info {
        &self.info
    }

    /// Size of the upright picture this decoder serves: the file's own size, or the reduced
    /// size when the decoder was opened with [`Reduce`] and chose to reduce.
    pub fn oriented_size(&self) -> (u32, u32) {
        self.reduced.unwrap_or_else(|| self.full_oriented_size())
    }

    /// Size of the file's own picture after orientation (width and height swapped for
    /// orientations 5 to 8), whatever this decoder serves.
    pub fn full_oriented_size(&self) -> (u32, u32) {
        oriented(self.info.width, self.info.height, self.info.orientation)
    }

    /// True when rows come from a reduced decode, not the full picture.
    pub fn is_reduced(&self) -> bool {
        self.reduced.is_some()
    }

    /// A copy of the primary image's property dictionaries, to pass to `encode`.
    pub fn properties(&self) -> Properties {
        self.props.clone()
    }

    /// True when ImageIO decodes this file in bands, so memory stays bounded for any size.
    /// False means the first read decodes the whole image (hence `MAX_UNTILED_PIXELS`).
    pub fn is_banded(&self) -> bool {
        self.banded
    }

    /// The ImageIO index of the primary image. Frame 0 is always this image.
    pub fn primary_source_index(&self) -> usize {
        self.primary
    }

    /// The property dictionaries of any frame.
    pub fn frame_properties(&self, frame: usize) -> Result<Properties, Error> {
        let idx = self.source_index(frame)?;
        let inner = self.lock()?;
        let opts = image_options();
        Ok(Properties::from_dict(ffi::source_props_at(
            &inner.source,
            idx,
            Some(cf::untyped(&opts)),
        )))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>, Error> {
        self.inner
            .lock()
            .map_err(|_| Error::new("The decoder is unusable."))
    }

    fn source_index(&self, frame: usize) -> Result<usize, Error> {
        if frame >= self.count {
            return Err(Error::new("That frame does not exist."));
        }
        Ok(if frame == 0 {
            self.primary
        } else if frame > self.primary {
            frame
        } else {
            frame - 1
        })
    }

    /// Stored size, orientation, alpha and upright size of any frame.
    pub fn frame_geometry(&self, frame: usize) -> Result<FrameGeometry, Error> {
        let idx = self.source_index(frame)?;
        let mut inner = self.lock()?;
        select_frame(&mut inner, idx)?;
        let ctx = inner
            .frame
            .as_ref()
            .ok_or_else(|| Error::new("The decoder is unusable."))?;
        Ok(ctx.geometry)
    }

    /// Draws rows `[y0, y0 + rows)` of the upright image as 8 bit RGBA, premultiplied alpha.
    /// `out` must hold `rows * width * 4` samples, where `width` is the oriented width of the
    /// frame. Opaque images get alpha 255.
    pub fn read_rows_u8(
        &self,
        frame: usize,
        y0: u32,
        rows: u32,
        space: Space,
        out: &mut [u8],
    ) -> Result<(), Error> {
        self.read_rows(frame, y0, rows, space, out, false)
    }

    /// Draws rows as 16 bit RGBA samples in native endianness, premultiplied alpha.
    pub fn read_rows_u16(
        &self,
        frame: usize,
        y0: u32,
        rows: u32,
        space: Space,
        out: &mut [u16],
    ) -> Result<(), Error> {
        self.read_rows(frame, y0, rows, space, ffi::u16_as_bytes_mut(out), true)
    }

    fn read_rows(
        &self,
        frame: usize,
        y0: u32,
        rows: u32,
        space: Space,
        out: &mut [u8],
        wide: bool,
    ) -> Result<(), Error> {
        let idx = self.source_index(frame)?;
        let mut inner = self.lock()?;
        select_frame(&mut inner, idx)?;
        let g = inner
            .frame
            .as_ref()
            .ok_or_else(|| Error::new("The decoder is unusable."))?
            .geometry;
        if self.raw_file
            && idx == self.primary
            && let Some(done) = self.read_raw_rows(&mut inner, g, y0, rows, space, out, wide)
        {
            return done;
        }
        let (w, h) = (g.width as usize, g.height as usize);
        let (y0, rows) = (y0 as usize, rows as usize);
        if y0.checked_add(rows).is_none_or(|end| end > h) {
            return Err(Error::new("Those rows are outside the image."));
        }
        let bpp = if wide { 8 } else { 4 };
        let row_bytes = w
            .checked_mul(bpp)
            .ok_or_else(|| Error::new("That band is too large."))?;
        if rows.checked_mul(row_bytes) != Some(out.len()) {
            return Err(Error::new(
                "The output buffer is the wrong size for those rows.",
            ));
        }
        if rows == 0 {
            return Ok(());
        }
        // Blocks line up with tile edges along the row axis: stored x for orientations 5 to 8.
        let tile_edge = match (self.tile, idx == self.primary) {
            (Some((tw, th)), true) => (if g.orientation >= 5 { tw } else { th }) as usize,
            _ => TILE,
        };
        let block_rows = block_rows_for(row_bytes, tile_edge);
        let image = inner
            .frame
            .as_ref()
            .map(|f| f.image.clone())
            .ok_or_else(|| Error::new("The decoder is unusable."))?;
        // A request at least as tall as a block already amortises tile decoding.
        if rows >= block_rows {
            return render(&image, g, y0, rows, space, wide, out);
        }
        // Otherwise serve from a cached block of whole tile rows.
        let reversed = matches!(g.orientation, 3 | 4 | 7 | 8);
        let offset = if reversed { h % block_rows } else { 0 };
        let key = (idx, space, wide);
        let (mut y, end) = (y0, y0 + rows);
        while y < end {
            let (bs, be) = block_bounds(y, h, block_rows, offset);
            let hit = inner
                .block
                .as_ref()
                .is_some_and(|b| b.key == key && b.y_start == bs && b.rows == be - bs);
            if !hit {
                let mut data = inner.block.take().map(|b| b.data).unwrap_or_default();
                data.clear();
                data.resize((be - bs) * row_bytes, 0);
                render(&image, g, bs, be - bs, space, wide, &mut data)?;
                inner.block = Some(Block {
                    key,
                    y_start: bs,
                    rows: be - bs,
                    data,
                });
            }
            let block = inner
                .block
                .as_ref()
                .ok_or_else(|| Error::new("The decoder is unusable."))?;
            let take = be.min(end) - y;
            let src = (y - bs) * row_bytes;
            let dst = (y - y0) * row_bytes;
            out[dst..dst + take * row_bytes]
                .copy_from_slice(&block.data[src..src + take * row_bytes]);
            y += take;
        }
        Ok(())
    }
}

impl Decoder {
    /// True when the file carries a thumbnail of its own that [`Decoder::thumbnail`] can use
    /// without developing the picture: a HEIF with a `thmb` reference, or a RAW (TIFF based
    /// containers keep one or more previews).
    pub fn has_embedded_thumbnail(&self) -> bool {
        if self.raw_file {
            return true;
        }
        if !matches!(
            self.info.uti.as_str(),
            "public.heic" | "public.heif" | "public.avci"
        ) {
            return false;
        }
        let Ok(inner) = self.lock() else {
            return false;
        };
        // The thumbnail link is a `thmb` entry in the `iref` box, which sits in the `meta`
        // box near the start of the file (or at the end for some writers).
        const WINDOW: usize = 1 << 20;
        let bytes = inner._data.to_vec();
        let head = &bytes[..bytes.len().min(WINDOW)];
        let tail = &bytes[bytes.len().saturating_sub(WINDOW)..];
        contains(head, b"thmb") || contains(tail, b"thmb")
    }

    /// A small upright picture of the file, 8 bit sRGB RGBA, at most `max_side` pixels on the
    /// long side (it is never enlarged past what the source has).
    ///
    /// The file's own thumbnail is used when it has one at least 90 percent of the size asked
    /// for, or of any size when `fast` is set (a first look that is refined later). That takes a
    /// few milliseconds whatever the picture size: a 200 MP HEIC answers in about 75 ms and a
    /// 12 MP RAW in about 100 ms. Otherwise the picture is decoded at the reduced size, which is
    /// much cheaper than a full decode and needs memory for the small result only.
    pub fn thumbnail(&self, max_side: u32, fast: bool) -> Result<Thumb, Error> {
        let max_side = max_side.clamp(1, 16_384);
        // Before the lock is taken: this looks at the file's bytes under the same lock.
        let has_thumbnail = self.has_embedded_thumbnail();
        let inner = self.lock()?;
        let mut best: Option<(CFRetained<CGImage>, bool)> = None;
        let mut fallback = None;
        if has_thumbnail {
            let opts = thumbnail_options(max_side, false);
            if let Some(t) =
                ffi::source_thumbnail_at(&inner.source, self.primary, Some(cf::untyped(&opts)))
            {
                let long = CGImage::width(Some(&t)).max(CGImage::height(Some(&t))) as f64;
                let (fw, fh) = self.full_oriented_size();
                let wanted = f64::from(max_side.min(fw.max(fh)));
                // RAW has nothing cheaper than its preview: developing it costs hundreds of MB.
                if fast || self.raw_file || long >= wanted * 0.9 {
                    best = Some((t, true));
                } else {
                    fallback = Some(t);
                }
            }
        }
        if best.is_none() {
            let opts = thumbnail_options(max_side, true);
            best = ffi::source_thumbnail_at(&inner.source, self.primary, Some(cf::untyped(&opts)))
                .map(|t| (t, false))
                .or_else(|| fallback.map(|t| (t, true)));
        }
        let (img, embedded) =
            best.ok_or_else(|| Error::new("The system could not make a preview of this image."))?;
        let (w, h) = (CGImage::width(Some(&img)), CGImage::height(Some(&img)));
        let (Ok(w32), Ok(h32)) = (u32::try_from(w), u32::try_from(h)) else {
            return Err(Error::new("This image is too large."));
        };
        if w32 == 0 || h32 == 0 || w32 > MAX_DIMENSION || h32 > MAX_DIMENSION {
            return Err(Error::new("This image has no pixels."));
        }
        let g = FrameGeometry {
            stored_width: w32,
            stored_height: h32,
            orientation: 1,
            width: w32,
            height: h32,
            has_alpha: alpha_of(&img),
        };
        let mut rgba = vec![0u8; w * h * 4];
        render(&img, g, 0, h, Space::Srgb, false, &mut rgba)?;
        Ok(Thumb {
            width: w32,
            height: h32,
            rgba,
            embedded,
        })
    }

    /// Reads rows through CoreImage when this is a RAW file it can develop. `None` means "use
    /// ImageIO instead": CoreImage cannot take the file or disagrees about its size.
    #[allow(clippy::too_many_arguments)]
    fn read_raw_rows(
        &self,
        inner: &mut Inner,
        g: FrameGeometry,
        y0: u32,
        rows: u32,
        space: Space,
        out: &mut [u8],
        wide: bool,
    ) -> Option<Result<(), Error>> {
        if matches!(inner.raw, RawSlot::Untried) {
            let img = RawImage::open(&inner._data, &self.info.uti, g.orientation, 1.0)
                .filter(|r| r.size() == (g.width, g.height));
            inner.raw = match img {
                Some(r) => RawSlot::Ready(r),
                None => RawSlot::Failed,
            };
        }
        let RawSlot::Ready(img) = &inner.raw else {
            return None;
        };
        let cs = inner
            .frame
            .as_ref()
            .and_then(|f| context_space(&f.image, space))?;
        let (w, h) = img.size();
        let (y0, rows) = (y0 as usize, rows as usize);
        let bpp = if wide { 8 } else { 4 };
        if y0.checked_add(rows).is_none_or(|end| end > h as usize)
            || rows.checked_mul(w as usize * bpp) != Some(out.len())
        {
            return Some(Err(Error::new("Those rows are outside the image.")));
        }
        if rows == 0 {
            return Some(Ok(()));
        }
        let r = img.render(y0, rows, &cs, wide, out);
        if r.is_ok() {
            force_opaque(out, wide);
        }
        Some(r)
    }
}

/// Options for `CGImageSourceCreateThumbnailAtIndex`: upright output, the SDR rendition, at most
/// `max_px` on the long side. `always` decodes the picture itself even when the file carries a
/// thumbnail; otherwise the file's own thumbnail is used when it has one.
fn thumbnail_options(max_px: u32, always: bool) -> CFRetained<PropDict> {
    let t: &CFType = CFBoolean::new(true).as_ref();
    let size = CFNumber::new_i64(i64::from(max_px));
    cf::make_dict(&[
        (
            if always {
                sys!(kCGImageSourceCreateThumbnailFromImageAlways)
            } else {
                sys!(kCGImageSourceCreateThumbnailFromImageIfAbsent)
            },
            t,
        ),
        (sys!(kCGImageSourceCreateThumbnailWithTransform), t),
        (sys!(kCGImageSourceShouldCacheImmediately), t),
        (sys!(kCGImageSourceThumbnailMaxPixelSize), size.as_ref()),
        (
            sys!(kCGImageSourceDecodeRequest),
            sys!(kCGImageSourceDecodeToSDR).as_ref(),
        ),
    ])
}

/// Rows per cached block for a given row size: a tile edge, halved for very wide images until
/// the cache fits `BLOCK_BYTES`. Halving keeps the block a divisor of the tile edge.
fn block_rows_for(row_bytes: usize, tile_edge: usize) -> usize {
    let cap = (BLOCK_BYTES / row_bytes.max(1)).max(1);
    let mut rows = tile_edge.max(1);
    while rows > cap && rows > 1 {
        rows = rows.div_ceil(2);
    }
    rows
}

/// Every `ispe` (image spatial extents) box found in the first and last MiB of a file. A HEIF
/// grid names its tile size this way, and ImageIO does not report it.
fn heif_image_sizes(data: &[u8]) -> Vec<(u32, u32)> {
    const WINDOW: usize = 1 << 20;
    let mut out = Vec::new();
    let tail_start = data
        .len()
        .saturating_sub(WINDOW)
        .max(data.len().min(WINDOW));
    for region in [&data[..data.len().min(WINDOW)], &data[tail_start..]] {
        let mut i = 0;
        while let Some(p) = region[i..].windows(4).position(|w| w == b"ispe") {
            let at = i + p + 4;
            if let Some(f) = region.get(at + 4..at + 12) {
                let w = u32::from_be_bytes([f[0], f[1], f[2], f[3]]);
                let h = u32::from_be_bytes([f[4], f[5], f[6], f[7]]);
                if out.len() < 64 {
                    out.push((w, h));
                }
            }
            i = at;
            if out.len() >= 64 {
                break;
            }
        }
    }
    out
}

/// The first listed extent that could be a tile of a `w` by `h` grid. Encoders write the tile
/// property first, ahead of the full size, thumbnails and gain map.
fn guess_tile(sizes: &[(u32, u32)], w: u32, h: u32) -> Option<(u32, u32)> {
    sizes.iter().copied().find(|&(tw, th)| {
        tw >= 16
            && th >= 16
            && tw <= w
            && th <= h
            && u64::from(tw) * u64::from(th) * 2 <= u64::from(w) * u64::from(h)
    })
}

/// The block `[start, end)` that holds row `y`. Blocks line up on tile edges counted from the
/// top, or from the bottom when `offset` is non zero (flipped orientations).
fn block_bounds(y: usize, h: usize, block_rows: usize, offset: usize) -> (usize, usize) {
    if offset > 0 && y < offset {
        return (0, offset);
    }
    let start = offset + ((y - offset) / block_rows) * block_rows;
    (start, (start + block_rows).min(h))
}

fn select_frame(inner: &mut Inner, idx: usize) -> Result<(), Error> {
    if inner.frame.as_ref().is_some_and(|f| f.index == idx) {
        return Ok(());
    }
    inner.block = None;
    let frame = open_frame(&inner.source, idx)?;
    check_pixels(&frame.geometry, inner.banded)?;
    inner.frame = Some(frame);
    Ok(())
}

fn open_frame(source: &objc2_image_io::CGImageSource, idx: usize) -> Result<FrameCtx, Error> {
    let opts = image_options();
    let image = ffi::source_image_at(source, idx, Some(cf::untyped(&opts)))
        .ok_or_else(|| Error::new("The system could not read this image."))?;
    let orientation = ffi::source_props_at(source, idx, Some(cf::untyped(&opts)))
        .as_deref()
        .map(orientation_of)
        .unwrap_or(1);
    let (sw, sh) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
    if sw == 0 || sh == 0 {
        return Err(Error::new("This image has no pixels."));
    }
    let (sw, sh) = match (u32::try_from(sw), u32::try_from(sh)) {
        (Ok(a), Ok(b)) if a <= MAX_DIMENSION && b <= MAX_DIMENSION => (a, b),
        _ => return Err(Error::new("This image is too large.")),
    };
    let (w, h) = oriented(sw, sh, orientation);
    let geometry = FrameGeometry {
        stored_width: sw,
        stored_height: sh,
        orientation,
        width: w,
        height: h,
        has_alpha: alpha_of(&image),
    };
    Ok(FrameCtx {
        index: idx,
        image,
        geometry,
    })
}

fn check_pixels(g: &FrameGeometry, banded: bool) -> Result<(), Error> {
    let pixels = u64::from(g.stored_width) * u64::from(g.stored_height);
    if pixels > MAX_PIXELS {
        return Err(Error::new("This image has too many pixels to open safely."));
    }
    if !banded && pixels > MAX_UNTILED_PIXELS {
        return Err(Error::new(
            "This image is too large for its format to be read within the memory budget.",
        ));
    }
    Ok(())
}

/// Draws one band of the upright image into `out` (which must be exactly the band).
fn render(
    image: &CGImage,
    g: FrameGeometry,
    y0: usize,
    rows: usize,
    space: Space,
    wide: bool,
    out: &mut [u8],
) -> Result<(), Error> {
    let (sw, sh) = (f64::from(g.stored_width), f64::from(g.stored_height));
    let (w, h) = (g.width as usize, g.height as usize);
    let bpp = if wide { 8 } else { 4 };
    let cs = context_space(image, space)
        .ok_or_else(|| Error::new("The system could not make a color space."))?;
    let alpha_bits = if g.has_alpha {
        CGImageAlphaInfo::PremultipliedLast
    } else {
        CGImageAlphaInfo::NoneSkipLast
    };
    let bitmap_info = alpha_bits.0
        | if wide {
            CGImageByteOrderInfo::Order16Little.0
        } else {
            0
        };
    out.fill(0);
    {
        let ctx = ffi::BitmapContext::new(
            out,
            w,
            rows,
            if wide { 16 } else { 8 },
            w * bpp,
            &cs,
            bitmap_info,
        )
        .ok_or_else(|| Error::new("The system could not make a drawing surface for this band."))?;
        let t = orientation_transform(g.orientation, sw, sh, h as f64);
        let shift = CGAffineTransform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            tx: 0.0,
            ty: (y0 + rows) as f64 - h as f64,
        };
        // Cut the band out of the stored image first. ImageIO then decodes only the tiles or
        // strips that overlap it instead of the whole picture.
        let (rx, ry, rw, rh) = stored_rect(g.orientation, sw, sh, w as f64, y0 as f64, rows as f64);
        let rect = CGRect {
            origin: CGPoint { x: rx, y: ry },
            size: CGSize {
                width: rw,
                height: rh,
            },
        };
        match ffi::crop_image(image, rect) {
            Some(part) => {
                let dest = CGRect {
                    origin: CGPoint {
                        x: rx,
                        y: sh - ry - rh,
                    },
                    size: CGSize {
                        width: rw,
                        height: rh,
                    },
                };
                ctx.draw_transformed(&part, shift, t, dest);
            }
            None => {
                let full = CGRect {
                    origin: CGPoint { x: 0.0, y: 0.0 },
                    size: CGSize {
                        width: sw,
                        height: sh,
                    },
                };
                ctx.draw_transformed(image, shift, t, full);
            }
        }
    }
    if !g.has_alpha {
        force_opaque(out, wide);
    }
    Ok(())
}

/// Geometry of one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameGeometry {
    /// Stored width.
    pub stored_width: u32,
    /// Stored height.
    pub stored_height: u32,
    /// EXIF orientation of this frame.
    pub orientation: u8,
    /// Upright width.
    pub width: u32,
    /// Upright height.
    pub height: u32,
    /// True when this frame has an alpha channel.
    pub has_alpha: bool,
}

fn oriented(w: u32, h: u32, orientation: u8) -> (u32, u32) {
    if (5..=8).contains(&orientation) {
        (h, w)
    } else {
        (w, h)
    }
}

fn force_opaque(out: &mut [u8], wide: bool) {
    if wide {
        for px in out.chunks_exact_mut(8) {
            px[6] = 0xff;
            px[7] = 0xff;
        }
    } else {
        for px in out.chunks_exact_mut(4) {
            px[3] = 0xff;
        }
    }
}

fn named_space(name: &CFString) -> Option<CFRetained<CGColorSpace>> {
    CGColorSpace::with_name(Some(name))
}

fn context_space(image: &CGImage, space: Space) -> Option<CFRetained<CGColorSpace>> {
    match space {
        Space::Srgb => named_space(sys!(kCGColorSpaceSRGB)),
        Space::DisplayP3 => named_space(sys!(kCGColorSpaceDisplayP3)),
        Space::Source => {
            let own = CGImage::color_space(Some(image));
            match own {
                Some(cs)
                    if CGColorSpace::model(Some(&cs)) == CGColorSpaceModel::RGB
                        && cs.supports_output()
                        && !cs.is_hdr()
                        && !cs.is_pq_based()
                        && !cs.is_hlg_based() =>
                {
                    Some(cs)
                }
                _ => named_space(sys!(kCGColorSpaceSRGB)),
            }
        }
    }
}

/// The transform that takes stored image space (y up, `sw` by `sh`) to upright space
/// (y up, `h` tall) for an EXIF orientation. Derived in y down coordinates, then converted.
fn orientation_transform(orientation: u8, sw: f64, sh: f64, h: f64) -> CGAffineTransform {
    // x' = a x + c y + tx, y' = b x + d y + ty, with y down.
    let (a, b, c, d, tx, ty) = match orientation {
        2 => (-1.0, 0.0, 0.0, 1.0, sw, 0.0),
        3 => (-1.0, 0.0, 0.0, -1.0, sw, sh),
        4 => (1.0, 0.0, 0.0, -1.0, 0.0, sh),
        5 => (0.0, 1.0, 1.0, 0.0, 0.0, 0.0),
        6 => (0.0, 1.0, -1.0, 0.0, sh, 0.0),
        7 => (0.0, -1.0, -1.0, 0.0, sh, sw),
        8 => (0.0, -1.0, 1.0, 0.0, 0.0, sw),
        _ => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
    };
    // Convert to y up: Ys = sh - ys and Yu = h - yu.
    CGAffineTransform {
        a,
        b: -b,
        c: -c,
        d,
        tx: tx + c * sh,
        ty: h - d * sh - ty,
    }
}

/// Is this uniform type identifier readable by ImageIO on this system?
pub fn can_decode_uti(uti: &str) -> bool {
    ffi::source_type_identifiers().iter().any(|t| t == uti)
}

/// The stored image rectangle (y down pixels) that holds upright rows `[y0, y0 + rows)`.
fn stored_rect(o: u8, sw: f64, sh: f64, w: f64, y0: f64, rows: f64) -> (f64, f64, f64, f64) {
    let (a, b, c, d, tx, ty) = match o {
        2 => (-1.0, 0.0, 0.0, 1.0, sw, 0.0),
        3 => (-1.0, 0.0, 0.0, -1.0, sw, sh),
        4 => (1.0, 0.0, 0.0, -1.0, 0.0, sh),
        5 => (0.0, 1.0, 1.0, 0.0, 0.0, 0.0),
        6 => (0.0, 1.0, -1.0, 0.0, sh, 0.0),
        7 => (0.0, -1.0, -1.0, 0.0, sh, sw),
        8 => (0.0, -1.0, 1.0, 0.0, 0.0, sw),
        _ => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
    };
    let corners = [(0.0, y0), (w, y0), (0.0, y0 + rows), (w, y0 + rows)];
    let (mut x0, mut y0s, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (ux, uy) in corners {
        let x = a * (ux - tx) + b * (uy - ty);
        let y = c * (ux - tx) + d * (uy - ty);
        x0 = x0.min(x);
        y0s = y0s.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    (x0, y0s, x1 - x0, y1 - y0s)
}
