//! Pixel buffers and the small vocabulary the pipeline shares.

/// Channel layout of a pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One luminance channel.
    Gray,
    /// Luminance and alpha.
    GrayAlpha,
    /// Red, green, blue.
    Rgb,
    /// Red, green, blue, alpha.
    Rgba,
}

impl Layout {
    /// Samples per pixel.
    pub fn channels(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::GrayAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    /// True when the layout carries alpha.
    pub fn has_alpha(self) -> bool {
        matches!(self, Self::GrayAlpha | Self::Rgba)
    }

    /// True for the single-color layouts.
    pub fn is_gray(self) -> bool {
        matches!(self, Self::Gray | Self::GrayAlpha)
    }

    /// Number of color channels, without alpha.
    pub fn color_channels(self) -> usize {
        if self.is_gray() { 1 } else { 3 }
    }

    /// The same layout with alpha added or removed.
    pub fn with_alpha(self, alpha: bool) -> Self {
        match (self.is_gray(), alpha) {
            (true, false) => Self::Gray,
            (true, true) => Self::GrayAlpha,
            (false, false) => Self::Rgb,
            (false, true) => Self::Rgba,
        }
    }
}

/// Bits per sample inside the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// 8 bits.
    U8,
    /// 16 bits (also used for 10 and 12 bit sources).
    U16,
}

/// Sample storage.
#[derive(Debug, Clone, PartialEq)]
pub enum Samples {
    /// 8 bit samples.
    U8(Vec<u8>),
    /// 16 bit samples.
    U16(Vec<u16>),
}

impl Samples {
    /// Number of samples.
    pub fn len(&self) -> usize {
        match self {
            Self::U8(v) => v.len(),
            Self::U16(v) => v.len(),
        }
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The depth of the samples.
    pub fn depth(&self) -> Depth {
        match self {
            Self::U8(_) => Depth::U8,
            Self::U16(_) => Depth::U16,
        }
    }

    /// Zeroed samples.
    pub fn zeroed(depth: Depth, len: usize) -> Self {
        match depth {
            Depth::U8 => Self::U8(vec![0; len]),
            Depth::U16 => Self::U16(vec![0; len]),
        }
    }
}

/// A borrowed run of rows.
#[derive(Debug, Clone, Copy)]
pub enum Rows<'a> {
    /// 8 bit samples.
    U8(&'a [u8]),
    /// 16 bit samples.
    U16(&'a [u16]),
}

impl Rows<'_> {
    /// Number of samples.
    pub fn len(&self) -> usize {
        match self {
            Self::U8(v) => v.len(),
            Self::U16(v) => v.len(),
        }
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A whole image in memory. Alpha is straight (not premultiplied).
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Channel layout.
    pub layout: Layout,
    /// Pixel values, row major, tightly packed.
    pub samples: Samples,
    /// The profile the pixel values are in, when it is not sRGB. Encoders embed it.
    pub icc: Option<std::sync::Arc<Vec<u8>>>,
}

impl Image {
    /// A zeroed image.
    pub fn new(width: u32, height: u32, layout: Layout, depth: Depth) -> Self {
        let len = width as usize * height as usize * layout.channels();
        Self {
            width,
            height,
            layout,
            samples: Samples::zeroed(depth, len),
            icc: None,
        }
    }

    /// Bit depth of the samples.
    pub fn depth(&self) -> Depth {
        self.samples.depth()
    }

    /// Samples in one row.
    pub fn row_len(&self) -> usize {
        self.width as usize * self.layout.channels()
    }

    /// Bytes this image holds in memory.
    pub fn byte_size(&self) -> usize {
        match &self.samples {
            Samples::U8(v) => v.len(),
            Samples::U16(v) => v.len() * 2,
        }
    }

    /// Rows `[y, y + n)` as a borrowed run.
    pub fn rows(&self, y: u32, n: u32) -> Rows<'_> {
        let rl = self.row_len();
        let (a, b) = (y as usize * rl, (y + n) as usize * rl);
        match &self.samples {
            Samples::U8(v) => Rows::U8(&v[a..b]),
            Samples::U16(v) => Rows::U16(&v[a..b]),
        }
    }

    /// The 8 bit samples, when the image is 8 bit.
    pub fn as_u8(&self) -> Option<&[u8]> {
        match &self.samples {
            Samples::U8(v) => Some(v),
            Samples::U16(_) => None,
        }
    }

    /// True when every pixel is opaque (or the layout has no alpha).
    pub fn is_opaque(&self) -> bool {
        if !self.layout.has_alpha() {
            return true;
        }
        let ch = self.layout.channels();
        match &self.samples {
            Samples::U8(v) => v.chunks_exact(ch).all(|p| p[ch - 1] == 255),
            Samples::U16(v) => v.chunks_exact(ch).all(|p| p[ch - 1] == 65535),
        }
    }

    /// A copy of the rectangle `(x, y, w, h)`.
    pub fn crop(&self, x: u32, y: u32, w: u32, h: u32) -> Image {
        let ch = self.layout.channels();
        let (xs, ys, ws, hs) = (x as usize, y as usize, w as usize, h as usize);
        let rl = self.row_len();
        let samples = match &self.samples {
            Samples::U8(v) => {
                let mut out = Vec::with_capacity(ws * hs * ch);
                for r in ys..ys + hs {
                    out.extend_from_slice(&v[r * rl + xs * ch..r * rl + (xs + ws) * ch]);
                }
                Samples::U8(out)
            }
            Samples::U16(v) => {
                let mut out = Vec::with_capacity(ws * hs * ch);
                for r in ys..ys + hs {
                    out.extend_from_slice(&v[r * rl + xs * ch..r * rl + (xs + ws) * ch]);
                }
                Samples::U16(out)
            }
        };
        Image {
            width: w,
            height: h,
            layout: self.layout,
            samples,
            icc: self.icc.clone(),
        }
    }

    /// Drop the alpha channel (the caller has already flattened or checked it).
    pub fn without_alpha(&self) -> Image {
        if !self.layout.has_alpha() {
            return self.clone();
        }
        let ch = self.layout.channels();
        let keep = ch - 1;
        let samples = match &self.samples {
            Samples::U8(v) => {
                let mut out = Vec::with_capacity(v.len() / ch * keep);
                for p in v.chunks_exact(ch) {
                    out.extend_from_slice(&p[..keep]);
                }
                Samples::U8(out)
            }
            Samples::U16(v) => {
                let mut out = Vec::with_capacity(v.len() / ch * keep);
                for p in v.chunks_exact(ch) {
                    out.extend_from_slice(&p[..keep]);
                }
                Samples::U16(out)
            }
        };
        Image {
            width: self.width,
            height: self.height,
            layout: self.layout.with_alpha(false),
            samples,
            icc: self.icc.clone(),
        }
    }

    /// An 8 bit copy (16 bit samples are rounded, with no dither).
    pub fn to_u8(&self) -> Image {
        match &self.samples {
            Samples::U8(_) => self.clone(),
            Samples::U16(v) => Image {
                samples: Samples::U8(
                    v.iter()
                        .map(|&s| ((s as u32 * 255 + 32767) / 65535) as u8)
                        .collect(),
                ),
                ..self.clone()
            },
        }
    }

    /// The same picture at 8 bits, without copying when it already is (16 bit samples are
    /// rounded, with no dither).
    pub fn to_u8_owned(self) -> Image {
        match self.samples {
            Samples::U8(_) => self,
            Samples::U16(_) => self.to_u8(),
        }
    }

    /// This picture at 8 bits: borrowed when it already is, a rounded copy otherwise.
    pub fn as_u8_image(&self) -> std::borrow::Cow<'_, Image> {
        match self.samples {
            Samples::U8(_) => std::borrow::Cow::Borrowed(self),
            Samples::U16(_) => std::borrow::Cow::Owned(self.to_u8()),
        }
    }

    /// This picture with color channels (gray expanded): borrowed when it already has them.
    pub fn as_rgb_image(&self) -> std::borrow::Cow<'_, Image> {
        if self.layout.is_gray() {
            std::borrow::Cow::Owned(self.to_rgb_layout())
        } else {
            std::borrow::Cow::Borrowed(self)
        }
    }

    /// Expand gray to RGB (alpha is kept).
    pub fn to_rgb_layout(&self) -> Image {
        if !self.layout.is_gray() {
            return self.clone();
        }
        let alpha = self.layout.has_alpha();
        let ch = self.layout.channels();
        let new_layout = Layout::Rgb.with_alpha(alpha);
        macro_rules! expand {
            ($v:expr, $ctor:path) => {{
                let mut out = Vec::with_capacity($v.len() / ch * new_layout.channels());
                for p in $v.chunks_exact(ch) {
                    out.extend_from_slice(&[p[0], p[0], p[0]]);
                    if alpha {
                        out.push(p[1]);
                    }
                }
                $ctor(out)
            }};
        }
        let samples = match &self.samples {
            Samples::U8(v) => expand!(v, Samples::U8),
            Samples::U16(v) => expand!(v, Samples::U16),
        };
        Image {
            width: self.width,
            height: self.height,
            layout: new_layout,
            samples,
            icc: self.icc.clone(),
        }
    }
}
