//! Offline tests against the image corpus. Each test skips with a note when its file is absent.
#![cfg(target_os = "macos")]

use panoply_platform_macos::{
    Decoder, EncodeInput, EncodeOptions, Format, Pixels, PropValue, Properties, Space,
    can_decode_uti, can_encode, encode,
};

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/corpus/cache/image"
);

fn corpus(rel: &str) -> Option<Vec<u8>> {
    match std::fs::read(format!("{CORPUS}/{rel}")) {
        Ok(d) => Some(d),
        Err(_) => {
            eprintln!("skipping: {rel} is not in the corpus");
            None
        }
    }
}

fn open(rel: &str) -> Option<Decoder> {
    let d = corpus(rel)?;
    Some(Decoder::open(d).unwrap_or_else(|e| panic!("{rel} should open: {e}")))
}

/// Reads the whole upright image of a frame as RGBA8, in bands of `band` rows.
fn read_all(d: &Decoder, frame: usize, space: Space, band: u32) -> (usize, usize, Vec<u8>) {
    let g = d.frame_geometry(frame).unwrap();
    let (w, h) = (g.width as usize, g.height as usize);
    let mut out = vec![0u8; w * h * 4];
    let mut y = 0usize;
    while y < h {
        let rows = (band as usize).min(h - y);
        let slice = &mut out[y * w * 4..(y + rows) * w * 4];
        d.read_rows_u8(frame, y as u32, rows as u32, space, slice)
            .unwrap();
        y += rows;
    }
    (w, h, out)
}

fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(&x, &y)| f64::from(x.abs_diff(y)))
        .sum::<f64>()
        / a.len() as f64
}

fn max_abs_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| x.abs_diff(y))
        .max()
        .unwrap_or(0)
}

fn psnr_rgb(a: &[u8], b: &[u8]) -> f64 {
    let mut se = 0.0f64;
    let mut n = 0.0f64;
    for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for c in 0..3 {
            let d = f64::from(pa[c]) - f64::from(pb[c]);
            se += d * d;
            n += 1.0;
        }
    }
    let mse = se / n;
    if mse == 0.0 {
        99.0
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

/// A smooth RGBA image, straight alpha 255.
fn gradient(w: usize, h: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f64 / w as f64;
            let fy = y as f64 / h as f64;
            v.push((40.0 + 170.0 * fx) as u8);
            v.push((30.0 + 180.0 * fy) as u8);
            v.push((60.0 + 120.0 * (0.5 * fx + 0.5 * fy)) as u8);
            v.push(255);
        }
    }
    v
}

fn solid(w: usize, h: usize, rgb: [u8; 3]) -> Vec<u8> {
    let mut v = Vec::with_capacity(w * h * 4);
    for _ in 0..w * h {
        v.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    v
}

fn encode_u8(w: usize, h: usize, px: &[u8], format: Format, space: Space, q: f32) -> Vec<u8> {
    let input = EncodeInput {
        width: w as u32,
        height: h as u32,
        pixels: Pixels::U8(px),
        space,
    };
    encode(
        &input,
        format,
        &EncodeOptions {
            quality: q,
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn info_for_every_heic_hif_avif_in_the_corpus() {
    let mut seen = 0;
    for dir in ["heic", "libavif", "libvips"] {
        let Ok(rd) = std::fs::read_dir(format!("{CORPUS}/{dir}")) else {
            eprintln!("skipping: {dir} is not in the corpus");
            continue;
        };
        let mut names: Vec<_> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let lower = name.to_lowercase();
            if !(lower.ends_with(".heic")
                || lower.ends_with(".hif")
                || lower.ends_with(".heif")
                || lower.ends_with(".avif"))
            {
                continue;
            }
            let data = std::fs::read(format!("{CORPUS}/{dir}/{name}")).unwrap();
            match Decoder::open(data) {
                Ok(d) => {
                    let i = d.info();
                    eprintln!(
                        "{dir}/{name}: {}x{} o{} alpha={} {}bpc frames={} anim={} {} hdr={} depth={} gain={} matte={} cs={:?} icc={:?} banded={}",
                        i.width,
                        i.height,
                        i.orientation,
                        i.has_alpha,
                        i.bits_per_component,
                        i.frames,
                        i.is_animated,
                        i.uti,
                        i.is_hdr,
                        i.has_depth,
                        i.has_gain_map,
                        i.has_portrait_matte,
                        i.colorspace_name,
                        i.icc.as_ref().map(Vec::len),
                        d.is_banded()
                    );
                    assert!(i.width > 0 && i.height > 0);
                    assert!((1..=8).contains(&i.orientation));
                    assert!(i.frames >= 1);
                    let (ow, oh) = d.oriented_size();
                    if i.orientation >= 5 {
                        assert_eq!((ow, oh), (i.height, i.width));
                    } else {
                        assert_eq!((ow, oh), (i.width, i.height));
                    }
                    assert!(!i.uti.is_empty());
                }
                Err(e) => {
                    eprintln!("{dir}/{name}: open error: {e}");
                    // Only the damaged file and the dimension bomb may fail.
                    assert!(
                        name == "corrupted.heic" || name == "avif-17000x17000.avif",
                        "{name} failed to open: {e}"
                    );
                }
            }
            seen += 1;
        }
    }
    eprintln!("checked {seen} files");
}

#[test]
fn info_values_for_known_files() {
    if let Some(d) = open("heic/display-p3.heic") {
        let i = d.info();
        assert_eq!((i.width, i.height, i.orientation), (4032, 3024, 6));
        assert_eq!(d.oriented_size(), (3024, 4032));
        assert_eq!(i.uti, "public.heic");
        assert_eq!(i.colorspace_name.as_deref(), Some("kCGColorSpaceDisplayP3"));
        assert!(i.icc.as_ref().is_some_and(|v| !v.is_empty()));
        assert!(!i.has_alpha);
    }
    if let Some(d) = open("heic/200mp-gainmap.heic") {
        let i = d.info();
        assert_eq!((i.width, i.height), (16320, 12240));
        assert!(i.has_gain_map, "the Samsung gain map must be detected");
        assert!(i.is_hdr);
        assert!(d.is_banded());
    }
    if let Some(d) = open("heic/apple-depth-portrait.heic") {
        let i = d.info();
        assert!(i.has_depth);
        assert!(i.has_gain_map);
        assert!(i.is_hdr);
    }
    if let Some(d) = open("heic/guitar-rotated-cw90.hif") {
        assert_eq!(d.info().orientation, 8);
        assert_eq!(d.info().bits_per_component, 10);
    }
}

#[test]
fn eight_orientations_decode_to_the_same_upright_picture() {
    let Some(base) = open("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let (bw, bh, reference) = read_all(&base, 0, Space::Srgb, 64);
    assert_eq!((bw, bh), (1800, 1200));
    for n in 1..=8 {
        let Some(d) = open(&format!("exif-orientation/landscape-{n}.jpg")) else {
            return;
        };
        assert_eq!(d.info().orientation, n as u8, "landscape-{n} orientation");
        assert_eq!(d.oriented_size(), (1800, 1200));
        let (w, h, px) = read_all(&d, 0, Space::Srgb, 64);
        assert_eq!((w, h), (1800, 1200));
        let mean = mean_abs_diff(&px, &reference);
        let psnr = psnr_rgb(&px, &reference);
        eprintln!("landscape-{n}: mean abs diff {mean:.3}, psnr {psnr:.1} dB");
        // The eight files are separate JPEG encodes of one scene, so chroma and quantisation
        // differ a little. A geometry error (wrong flip, off by one) would be far larger.
        assert!(
            mean < 5.0,
            "landscape-{n} differs from landscape-1 (mean {mean})"
        );
        assert!(psnr > 27.0, "landscape-{n} psnr {psnr}");
        // Zero shift must fit better than one pixel of shift either way.
        let shifted = |dx: isize| -> f64 {
            let mut tot = 0.0;
            let mut cnt = 0.0;
            for y in (8..h - 8).step_by(3) {
                for x in 8..w - 8 {
                    let sx = (x as isize + dx) as usize;
                    for c in 0..3 {
                        tot += f64::from(
                            px[(y * w + x) * 4 + c].abs_diff(reference[(y * w + sx) * 4 + c]),
                        );
                        cnt += 1.0;
                    }
                }
            }
            tot / cnt
        };
        let (m0, mm, mp) = (shifted(0), shifted(-1), shifted(1));
        assert!(
            m0 < mm && m0 < mp,
            "landscape-{n}: shift fits better ({m0:.2} vs {mm:.2}, {mp:.2})"
        );
    }
}

#[test]
fn heic_orientation_is_applied_and_bands_do_not_matter() {
    let Some(d) = open("heic/display-p3.heic") else {
        return;
    };
    // ImageIO hands back the stored 4032x3024 picture and reports orientation 6 separately.
    let g = d.frame_geometry(0).unwrap();
    assert_eq!((g.stored_width, g.stored_height), (4032, 3024));
    assert_eq!((g.width, g.height), (3024, 4032));
    let (_, _, whole) = read_all(&d, 0, Space::Srgb, 4032);
    for band in [64u32, 100, 513] {
        let (_, _, banded) = read_all(&d, 0, Space::Srgb, band);
        let max = max_abs_diff(&whole, &banded);
        let mean = mean_abs_diff(&whole, &banded);
        eprintln!("display-p3 band {band}: max diff {max}, mean {mean:.4}");
        assert!(
            mean < 0.5 && max <= 40,
            "band {band}: mean {mean}, max {max}"
        );
    }
}

#[test]
fn rotated_hif_reads_upright_and_consistently() {
    let Some(d) = open("heic/guitar-rotated-cw90.hif") else {
        return;
    };
    assert_eq!(d.oriented_size(), (4672, 7008));
    // Two bands from the top and the middle, compared with one read covering both.
    let w = 4672usize;
    let mut a = vec![0u8; w * 64 * 4];
    let mut b = vec![0u8; w * 64 * 4];
    d.read_rows_u8(0, 3000, 64, Space::Srgb, &mut a).unwrap();
    let mut big = vec![0u8; w * 128 * 4];
    d.read_rows_u8(0, 3000, 128, Space::Srgb, &mut big).unwrap();
    d.read_rows_u8(0, 3000, 64, Space::Srgb, &mut b).unwrap();
    assert_eq!(a, b, "the same band twice must be identical");
    let mean = mean_abs_diff(&a, &big[..w * 64 * 4]);
    assert!(mean < 0.5, "band vs larger read: {mean}");
    assert!(a.chunks_exact(4).any(|p| p[0] > 10));
}

fn clipped_channels(px: &[u8]) -> usize {
    px.chunks_exact(4)
        .map(|p| p[..3].iter().filter(|&&c| c == 0 || c == 255).count())
        .sum()
}

#[test]
fn display_p3_decodes_differently_into_srgb_than_into_p3() {
    // A real file first: the two spaces must give different numbers.
    if let Some(d) = open("heic/display-p3.heic") {
        let g = d.frame_geometry(0).unwrap();
        let rows = 64usize;
        let w = g.width as usize;
        let (mut s, mut p) = (vec![0u8; w * rows * 4], vec![0u8; w * rows * 4]);
        d.read_rows_u8(0, 2000, rows as u32, Space::Srgb, &mut s)
            .unwrap();
        d.read_rows_u8(0, 2000, rows as u32, Space::DisplayP3, &mut p)
            .unwrap();
        let mut src = vec![0u8; w * rows * 4];
        d.read_rows_u8(0, 2000, rows as u32, Space::Source, &mut src)
            .unwrap();
        eprintln!(
            "display-p3.heic band: mean diff sRGB vs P3 {:.3} (neutral paper, so small)",
            mean_abs_diff(&s, &p)
        );
        // Source on a P3 file is P3.
        assert!(
            mean_abs_diff(&src, &p) < 0.01,
            "Space::Source must match P3 for a P3 file"
        );
        // sRGB cannot hold P3 saturation, so it clips at least as much.
        assert!(clipped_channels(&s) >= clipped_channels(&p));
    }
    // A saturated P3 colour made synthetically. P3 (255, 64, 64) lies outside sRGB.
    for format in [Format::Heic, Format::Avif] {
        if !can_encode(format) {
            continue;
        }
        let px = solid(64, 64, [255, 64, 64]);
        let bytes = encode_u8(64, 64, &px, format, Space::DisplayP3, 0.95);
        let d = Decoder::open(bytes).unwrap();
        let (_, _, p3) = read_all(&d, 0, Space::DisplayP3, 64);
        let (_, _, srgb) = read_all(&d, 0, Space::Srgb, 64);
        let (cp, cs) = (
            &p3[(32 * 64 + 32) * 4..][..3],
            &srgb[(32 * 64 + 32) * 4..][..3],
        );
        eprintln!("{format:?}: P3 mode {cp:?}, sRGB mode {cs:?}");
        assert!(
            cp[0] >= 245 && (50..=80).contains(&cp[1]),
            "P3 round trip {cp:?}"
        );
        // In sRGB the red clips at 255 and green and blue move down (the colour is out of gamut).
        assert!(cs[0] >= 250);
        assert!(
            cs[1] + 15 < cp[1] && cs[2] + 5 < cp[2],
            "sRGB {cs:?} vs P3 {cp:?}"
        );
    }
}

#[test]
fn alpha_heic_keeps_alpha() {
    let Some(d) = open("heic/alpha.heic") else {
        return;
    };
    // The primary image of this file is a composite over opaque black. The image with
    // transparency is frame 1.
    assert!(!d.frame_geometry(0).unwrap().has_alpha);
    let g1 = d.frame_geometry(1).unwrap();
    assert!(g1.has_alpha);
    let (_, _, px) = read_all(&d, 1, Space::Srgb, 64);
    let (min, max) = px
        .chunks_exact(4)
        .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[3]), hi.max(p[3])));
    eprintln!("alpha.heic frame 1 alpha range {min}..{max}");
    assert!(min < 255 && max > 0 && min < max);
    // Premultiplied: colour never exceeds alpha.
    assert!(px.chunks_exact(4).all(|p| p[0] <= p[3].saturating_add(1)
        && p[1] <= p[3].saturating_add(1)
        && p[2] <= p[3].saturating_add(1)));
    // The frame 0 view is opaque.
    let (_, _, p0) = read_all(&d, 0, Space::Srgb, 64);
    assert!(p0.chunks_exact(4).all(|p| p[3] == 255));
}

#[test]
fn ten_bit_heif_reads_as_u16() {
    let Some(d) = open("heic/rgb-10bit.heif") else {
        return;
    };
    assert_eq!(d.info().bits_per_component, 10);
    let (w, h) = d.oriented_size();
    let (w, h) = (w as usize, h as usize);
    let mut wide = vec![0u16; w * h * 4];
    d.read_rows_u16(0, 0, h as u32, Space::Srgb, &mut wide)
        .unwrap();
    let mut narrow = vec![0u8; w * h * 4];
    d.read_rows_u8(0, 0, h as u32, Space::Srgb, &mut narrow)
        .unwrap();
    assert!(
        wide.iter().any(|&v| v > 255),
        "16 bit samples must use the wide range"
    );
    // Same picture: the 16 bit value divided by 257 is the 8 bit value, within rounding.
    let worst = wide
        .iter()
        .zip(&narrow)
        .map(|(&a, &b)| ((f64::from(a) / 257.0) - f64::from(b)).abs())
        .fold(0.0, f64::max);
    eprintln!("10 bit: worst u16 vs u8 difference {worst:.2} levels");
    assert!(worst < 2.5);
    assert!(wide.chunks_exact(4).all(|p| p[3] == 65535));
}

#[test]
fn encode_then_decode_round_trip_has_good_psnr() {
    let (w, h) = (256usize, 192usize);
    let px = gradient(w, h);
    let mut ran = 0;
    for format in [Format::Heic, Format::Avif] {
        if !can_encode(format) {
            eprintln!("skipping: {format:?} encode is not available");
            continue;
        }
        let bytes = encode_u8(w, h, &px, format, Space::Srgb, 0.9);
        let d = Decoder::open(bytes.clone()).unwrap();
        assert_eq!(
            d.info().uti,
            if format == Format::Heic {
                "public.heic"
            } else {
                "public.avif"
            }
        );
        assert_eq!((d.info().width, d.info().height), (w as u32, h as u32));
        let (_, _, out) = read_all(&d, 0, Space::Srgb, 64);
        let psnr = psnr_rgb(&px, &out);
        eprintln!("{format:?} q0.9: {} bytes, psnr {psnr:.1} dB", bytes.len());
        assert!(psnr > 35.0, "{format:?} psnr {psnr}");
        ran += 1;
    }
    eprintln!("round trips run: {ran}");
}

#[test]
fn sixteen_bit_encode_round_trip() {
    if !can_encode(Format::Heic) {
        return;
    }
    let (w, h) = (128usize, 96usize);
    let narrow = gradient(w, h);
    let wide: Vec<u16> = narrow.iter().map(|&v| u16::from(v) * 257).collect();
    let input = EncodeInput {
        width: w as u32,
        height: h as u32,
        pixels: Pixels::U16(&wide),
        space: Space::Srgb,
    };
    let bytes = encode(
        &input,
        Format::Heic,
        &EncodeOptions {
            quality: 0.9,
            ..Default::default()
        },
    )
    .unwrap();
    let d = Decoder::open(bytes).unwrap();
    let (_, _, out) = read_all(&d, 0, Space::Srgb, 64);
    assert!(psnr_rgb(&narrow, &out) > 35.0);
}

#[test]
fn icc_passes_through_encode() {
    let Some(src) = open("heic/display-p3.heic") else {
        return;
    };
    let icc = src
        .info()
        .icc
        .clone()
        .expect("display-p3.heic has an ICC profile");
    for format in [Format::Heic, Format::Avif] {
        if !can_encode(format) {
            continue;
        }
        let px = gradient(64, 64);
        let input = EncodeInput {
            width: 64,
            height: 64,
            pixels: Pixels::U8(&px),
            space: Space::Srgb,
        };
        let opts = EncodeOptions {
            quality: 0.8,
            icc: Some(&icc),
            ..Default::default()
        };
        let bytes = encode(&input, format, &opts).unwrap();
        let d = Decoder::open(bytes).unwrap();
        let out_icc = d
            .info()
            .icc
            .as_ref()
            .expect("output carries an ICC profile");
        assert!(!out_icc.is_empty());
        eprintln!(
            "{format:?}: ICC in {} bytes, out {} bytes, name {:?}",
            icc.len(),
            out_icc.len(),
            d.info().colorspace_name
        );
        assert_eq!(
            d.info().colorspace_name.as_deref(),
            Some("kCGColorSpaceDisplayP3")
        );
    }
}

#[test]
fn keep_properties_carries_exif_and_drop_gps_removes_gps() {
    if !can_encode(Format::Heic) {
        return;
    }
    let px = gradient(64, 64);
    let input = EncodeInput {
        width: 64,
        height: 64,
        pixels: Pixels::U8(&px),
        space: Space::Srgb,
    };

    // Properties built by hand, with GPS and a rotated orientation in TIFF.
    let props = Properties::from_sections(&[
        (
            "{TIFF}",
            &[
                ("Make", PropValue::Str("PanoplyCam".into())),
                ("Orientation", PropValue::Int(6)),
            ],
        ),
        (
            "{Exif}",
            &[
                ("UserComment", PropValue::Str("hello".into())),
                ("PixelXDimension", PropValue::Int(9999)),
            ],
        ),
        (
            "{GPS}",
            &[
                ("Latitude", PropValue::Float(12.5)),
                ("LatitudeRef", PropValue::Str("N".into())),
                ("Longitude", PropValue::Float(77.5)),
                ("LongitudeRef", PropValue::Str("E".into())),
            ],
        ),
    ]);
    assert!(props.has_section("{GPS}"));
    let with_gps = encode(
        &input,
        Format::Heic,
        &EncodeOptions {
            keep_properties: Some(&props),
            ..Default::default()
        },
    )
    .unwrap();
    let no_gps = encode(
        &input,
        Format::Heic,
        &EncodeOptions {
            keep_properties: Some(&props),
            drop_gps: true,
            ..Default::default()
        },
    )
    .unwrap();
    let plain = encode(&input, Format::Heic, &EncodeOptions::default()).unwrap();

    let a = Decoder::open(with_gps).unwrap().properties();
    let b = Decoder::open(no_gps).unwrap().properties();
    let c = Decoder::open(plain).unwrap().properties();
    eprintln!(
        "kept: {:?}\ndropped gps: {:?}\nplain: {:?}",
        a.section_names(),
        b.section_names(),
        c.section_names()
    );
    assert!(
        a.has_section("{Exif}") || a.has_section("{TIFF}"),
        "metadata should be carried"
    );
    assert_eq!(
        a.section_value("{TIFF}", "Make").as_deref(),
        Some("PanoplyCam")
    );
    assert_eq!(a.orientation(), Some(1), "orientation is forced to 1");
    assert!(a.has_section("{GPS}"), "GPS kept when not dropped");
    assert!(!b.has_section("{GPS}"), "GPS removed by drop_gps");
    assert_eq!(
        b.section_value("{TIFF}", "Make").as_deref(),
        Some("PanoplyCam")
    );
    assert!(!c.has_section("{GPS}") && c.section_value("{TIFF}", "Make").is_none());
    assert_ne!(
        a.section_value("{Exif}", "PixelXDimension").as_deref(),
        Some("9999")
    );

    // And from a real camera file: the rotated display-p3.heic loses its rotation.
    if let Some(src) = open("heic/display-p3.heic") {
        let out = encode(
            &input,
            Format::Heic,
            &EncodeOptions {
                keep_properties: Some(&src.properties()),
                ..Default::default()
            },
        )
        .unwrap();
        let d = Decoder::open(out).unwrap();
        assert_eq!(d.info().orientation, 1);
        assert_eq!(
            d.properties().section_value("{TIFF}", "Make"),
            src.properties().section_value("{TIFF}", "Make")
        );
        assert_eq!(d.info().width, 64);
    }
    if let Some(src) = open("heic/xiaomi.heic")
        && src.properties().has_section("{GPS}")
    {
        {
            let kept = encode(
                &input,
                Format::Heic,
                &EncodeOptions {
                    keep_properties: Some(&src.properties()),
                    ..Default::default()
                },
            )
            .unwrap();
            let dropped = encode(
                &input,
                Format::Heic,
                &EncodeOptions {
                    keep_properties: Some(&src.properties()),
                    drop_gps: true,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(
                Decoder::open(kept)
                    .unwrap()
                    .properties()
                    .has_section("{GPS}")
            );
            assert!(
                !Decoder::open(dropped)
                    .unwrap()
                    .properties()
                    .has_section("{GPS}")
            );
        }
    }
}

#[test]
fn opaque_flag_ignores_alpha() {
    if !can_encode(Format::Heic) {
        return;
    }
    let (w, h) = (64usize, 64usize);
    let mut px = gradient(w, h);
    for p in px.chunks_exact_mut(4) {
        p[3] = 40;
    }
    let input = EncodeInput {
        width: w as u32,
        height: h as u32,
        pixels: Pixels::U8(&px),
        space: Space::Srgb,
    };
    let bytes = encode(
        &input,
        Format::Heic,
        &EncodeOptions {
            quality: 0.9,
            opaque: true,
            ..Default::default()
        },
    )
    .unwrap();
    let d = Decoder::open(bytes).unwrap();
    assert!(!d.info().has_alpha);
    let (_, _, out) = read_all(&d, 0, Space::Srgb, 64);
    assert!(out.chunks_exact(4).all(|p| p[3] == 255));
    assert!(
        psnr_rgb(&gradient(w, h), &out) > 33.0,
        "RGB kept at full strength"
    );
}

#[test]
fn encode_alpha_round_trip_keeps_transparency() {
    if !can_encode(Format::Heic) {
        return;
    }
    let (w, h) = (64usize, 64usize);
    let mut px = gradient(w, h);
    for (i, p) in px.chunks_exact_mut(4).enumerate() {
        p[3] = if (i % w) < w / 2 { 255 } else { 0 };
    }
    let input = EncodeInput {
        width: w as u32,
        height: h as u32,
        pixels: Pixels::U8(&px),
        space: Space::Srgb,
    };
    let bytes = encode(
        &input,
        Format::Heic,
        &EncodeOptions {
            quality: 0.9,
            ..Default::default()
        },
    )
    .unwrap();
    let d = Decoder::open(bytes).unwrap();
    let mut seen_transparent = false;
    for f in 0..d.info().frames {
        let (_, _, out) = read_all(&d, f, Space::Srgb, 64);
        if out.chunks_exact(4).any(|p| p[3] < 40) {
            seen_transparent = true;
        }
    }
    assert!(
        seen_transparent,
        "the transparent half must stay transparent in some frame"
    );
}

#[test]
fn error_cases_are_plain_errors_not_panics() {
    assert!(Decoder::open(Vec::new()).is_err());
    assert!(Decoder::open(vec![1, 2, 3]).is_err());
    assert!(Decoder::open(vec![0xAB; 4096]).is_err());
    assert!(Decoder::open(b"not an image at all, just words".to_vec()).is_err());
    if let Some(bytes) = corpus("heic/corrupted.heic") {
        let e = Decoder::open(bytes)
            .err()
            .expect("corrupted.heic must not open");
        assert!(!e.to_string().is_empty());
    }
    // Truncations of real files at several lengths never panic.
    for rel in [
        "heic/xiaomi.heic",
        "exif-orientation/landscape-1.jpg",
        "libvips/truncated.jpg",
        "libvips/truncated.gif",
    ] {
        let Some(full) = corpus(rel) else { continue };
        for cut in [
            full.len(),
            full.len() * 9 / 10,
            full.len() / 2,
            full.len() / 10,
            20,
        ] {
            if let Ok(d) = Decoder::open(full[..cut].to_vec()) {
                let (w, h) = d.oriented_size();
                let rows = h.min(8);
                let mut out = vec![0u8; (w * rows * 4) as usize];
                let _ = d.read_rows_u8(0, 0, rows, Space::Srgb, &mut out);
            }
        }
    }
    // unsupported-depth.heic: open and read return a Result either way.
    if let Some(bytes) = corpus("heic/unsupported-depth.heic")
        && let Ok(d) = Decoder::open(bytes)
    {
        {
            let (w, h) = d.oriented_size();
            let mut out = vec![0u8; (w * h * 4) as usize];
            let r = d.read_rows_u8(0, 0, h, Space::Srgb, &mut out);
            eprintln!("unsupported-depth.heic read: {r:?}");
        }
    }
}

#[test]
fn api_misuse_returns_errors() {
    let Some(d) = open("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let mut small = vec![0u8; 16];
    assert!(
        d.read_rows_u8(0, 0, 4, Space::Srgb, &mut small).is_err(),
        "wrong buffer size"
    );
    let mut row = vec![0u8; 1800 * 4];
    assert!(
        d.read_rows_u8(0, 1200, 1, Space::Srgb, &mut row).is_err(),
        "row past the end"
    );
    assert!(
        d.read_rows_u8(0, u32::MAX, 1, Space::Srgb, &mut row)
            .is_err(),
        "overflowing rows"
    );
    assert!(
        d.read_rows_u8(5, 0, 1, Space::Srgb, &mut row).is_err(),
        "no such frame"
    );
    assert!(
        d.read_rows_u8(0, 0, 0, Space::Srgb, &mut []).is_ok(),
        "zero rows is a no-op"
    );
    let mut wide = vec![0u16; 3];
    assert!(d.read_rows_u16(0, 0, 1, Space::Srgb, &mut wide).is_err());
}

#[test]
fn encode_rejects_bad_input() {
    let px = gradient(8, 8);
    let ok = EncodeInput {
        width: 8,
        height: 8,
        pixels: Pixels::U8(&px),
        space: Space::Srgb,
    };
    let o = EncodeOptions::default();
    let zero = EncodeInput { width: 0, ..ok };
    assert!(encode(&zero, Format::Heic, &o).is_err());
    let short = EncodeInput { width: 9, ..ok };
    assert!(encode(&short, Format::Heic, &o).is_err());
    assert!(encode(&ok, Format::Heic, &EncodeOptions { quality: 1.5, ..o }).is_err());
    assert!(
        encode(
            &ok,
            Format::Heic,
            &EncodeOptions {
                icc: Some(b"not an icc profile"),
                ..o
            }
        )
        .is_err()
    );
    let huge = EncodeInput {
        width: 100_000,
        height: 100_000,
        pixels: Pixels::U8(&px),
        space: Space::Srgb,
    };
    assert!(encode(&huge, Format::Avif, &o).is_err());
}

#[test]
fn format_support_queries() {
    assert!(can_decode_uti("public.jpeg"));
    assert!(can_decode_uti("public.heic"));
    assert!(can_decode_uti("public.avif"));
    assert!(!can_decode_uti("com.example.nonsense"));
    eprintln!(
        "can_encode heic={} avif={}",
        can_encode(Format::Heic),
        can_encode(Format::Avif)
    );
}

#[test]
fn animated_and_multi_image_frames_read() {
    if let Some(d) = open("libavif/colors-animated-8bpc.avif") {
        eprintln!(
            "avif sequence: frames={} animated={}",
            d.info().frames,
            d.info().is_animated
        );
        assert!(d.info().frames > 1);
        assert!(d.info().is_animated);
        let (_, _, f0) = read_all(&d, 0, Space::Srgb, 64);
        let last = d.info().frames - 1;
        let (_, _, fl) = read_all(&d, last, Space::Srgb, 64);
        assert!(
            mean_abs_diff(&f0, &fl) > 1.0,
            "frames of an animation differ"
        );
    }
    if let Some(d) = open("libvips/cogs.gif") {
        assert!(d.info().frames > 1 && d.info().is_animated);
        let last = d.info().frames - 1;
        let (_, _, a) = read_all(&d, 0, Space::Srgb, 64);
        let (_, _, b) = read_all(&d, last, Space::Srgb, 64);
        assert!(!a.is_empty() && !b.is_empty());
    }
    if let Some(d) = open("heic/spatial-photo.heic") {
        assert_eq!(d.info().frames, 2);
        let (_, _, a) = read_all(&d, 0, Space::Srgb, 128);
        let (_, _, b) = read_all(&d, 1, Space::Srgb, 128);
        assert_eq!(a.len(), b.len());
    }
}

#[test]
fn other_formats_open_or_fail_cleanly() {
    for rel in [
        "libvips/world.jp2",
        "libvips/favicon.ico",
        "libvips/subsampled.tif",
        "libvips/apng-16bit.png",
        "libvips/ultra-hdr.jpg",
        "libvips/sample.exr",
        "libvips/logo.svg",
        "libavif/colors-hdr-p3-pq.avif",
        "libavif/seine-hdr-gainmap.avif",
        "libavif/grid-1x5.avif",
        "pngsuite/basn0g16.png",
    ] {
        let Some(bytes) = corpus(rel) else { continue };
        match Decoder::open(bytes) {
            Ok(d) => {
                let (w, h) = d.oriented_size();
                let rows = h.min(16);
                let mut out = vec![0u8; (w * rows * 4) as usize];
                let r = d.read_rows_u8(0, 0, rows, Space::Srgb, &mut out);
                eprintln!(
                    "{rel}: {} {}x{} hdr={} gain={} read={:?}",
                    d.info().uti,
                    w,
                    h,
                    d.info().is_hdr,
                    d.info().has_gain_map,
                    r.is_ok()
                );
            }
            Err(e) => eprintln!("{rel}: open error: {e}"),
        }
    }
}

#[test]
fn decoder_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Decoder>();
    assert_send_sync::<Properties>();
}
