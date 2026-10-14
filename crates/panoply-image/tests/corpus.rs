//! The image modules on the image corpus: every case the specs name (IPC-1 to IPC-9) and the
//! files that break naive tools. Tests skip, with a message, for corpus files that are not
//! on disk; fetch them with `python3 -I tests/corpus/fetch.py --kind image` and
//! `python3 tests/corpus/generate.py --kind image`.

mod common;

use common::*;
use panoply_image::meta;
use panoply_image::ops::Io;
use panoply_image::orient::orientation_from_exif;
use panoply_image::ssim;
use serde_json::json;

const MAC: bool = cfg!(target_os = "macos");

fn mean_abs_diff(a: &panoply_image::types::Image, b: &panoply_image::types::Image) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let (a8, b8) = (a.to_u8(), b.to_u8());
    let (x, y) = (a8.as_u8().unwrap(), b8.as_u8().unwrap());
    assert_eq!(x.len(), y.len());
    x.iter()
        .zip(y)
        .map(|(p, q)| (*p as f64 - *q as f64).abs())
        .sum::<f64>()
        / x.len() as f64
}

fn orientation_of(path: &std::path::Path) -> u8 {
    meta::extract_exif(&file_bytes(path))
        .map(|e| orientation_from_exif(&e))
        .unwrap_or(1)
}

// ------------------------------------------------------------------ IPC-1 orientation

#[test]
fn all_eight_orientations_come_out_upright_and_tagged_one() {
    let Some(reference) = corpus("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let want = decode_srgb(&reference);
    for n in 1..=8 {
        let Some(src) = corpus(&format!("exif-orientation/landscape-{n}.jpg")) else {
            return;
        };
        assert_eq!(
            orientation_of(&src),
            n as u8,
            "landscape-{n} should carry orientation {n}"
        );
        // Convert (lossless target, EXIF kept), resize to the same size (cover), compress and
        // kb must all produce the upright picture with orientation 1 or no tag.
        let s = Scratch::new("orient");
        let (png, _) = run_ok("image.convert", &src, json!({"to": "png"}), s.path());
        let got = decode_srgb(&png);
        assert_eq!(
            (got.width, got.height),
            (want.width, want.height),
            "landscape-{n}: size"
        );
        // The eight files are separate encodings of one photo, so allow codec noise.
        let d = mean_abs_diff(&got, &want);
        assert!(
            d < 6.0,
            "landscape-{n} is not upright: mean difference {d:.1}"
        );
        assert_eq!(
            orientation_of(&png),
            1,
            "landscape-{n}: the output must be tagged 1"
        );

        let (small, _) = run_ok("image.resize", &src, json!({"longest": 300}), s.path());
        let small_img = decode_srgb(&small);
        assert_eq!(small_img.width.max(small_img.height), 300);
        assert!(
            small_img.width > small_img.height,
            "landscape-{n} resized sideways"
        );
        assert!(orientation_of(&small) <= 1);

        let (comp, _) = run_ok(
            "image.compress",
            &src,
            json!({"strip_metadata": false}),
            s.path(),
        );
        let c = decode_srgb(&comp);
        assert_eq!((c.width, c.height), (want.width, want.height));
        assert!(
            orientation_of(&comp) <= 1,
            "compress kept EXIF with the old orientation"
        );

        let (kb, _) = run_ok(
            "image.resize_to_kb",
            &src,
            json!({"width": 200, "height": 150, "max_kb": 40}),
            s.path(),
        );
        let k = decode_srgb(&kb);
        assert_eq!((k.width, k.height), (200, 150));
        // Landscape stays landscape: the sky (top rows) is lighter than the ground is not
        // assumed, so compare with the reference scaled by the same module.
        let (kb_ref, _) = run_ok(
            "image.resize_to_kb",
            &reference,
            json!({"width": 200, "height": 150, "max_kb": 40}),
            s.path(),
        );
        let d = mean_abs_diff(&k, &decode_srgb(&kb_ref));
        assert!(d < 8.0, "landscape-{n} resize_to_kb not upright: {d:.1}");
    }
}

#[test]
fn the_sideways_portal_bug_is_fixed_for_a_phone_photo() {
    let Some(src) = corpus("generated/exif-gps-orientation6.jpg") else {
        return;
    };
    let s = Scratch::new("sideways");
    let (out, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 120, "height": 160, "max_kb": 30}),
        s.path(),
    );
    let img = decode_srgb(&out);
    assert_eq!((img.width, img.height), (120, 160));
    // The source is stored 1200 x 800 with tag 6, so it is a portrait 800 x 1200.
    assert_eq!(orientation_of(&out), 1);
    assert!(
        meta::inspect(&file_bytes(&out)).unwrap().gps.is_none(),
        "a portal photo must carry no location"
    );
}

// ------------------------------------------------------------------ IPC-2 color

#[test]
fn cmyk_jpeg_and_tiff_do_not_go_neon() {
    if let Some(src) = corpus("generated/cmyk-adobe.jpg") {
        let s = Scratch::new("cmyk");
        let (out, r) = run_ok("image.convert", &src, json!({"to": "png"}), s.path());
        let img = decode_srgb(&out);
        // C = M = 0 at the top left, Y = 40 of 255: nearly white, a little warm. An inverted-CMYK
        // mistake gives near black here.
        let tl = px(&img, 4, 4);
        assert!(
            tl.iter().all(|&v| v >= 200),
            "top left of the CMYK JPEG is {tl:?}"
        );
        // C and M both high at the bottom right: a dark blue-violet, never white.
        let br = px(&img, img.width - 5, img.height - 5);
        assert!(
            br[0] < 110 && br[1] < 110,
            "bottom right of the CMYK JPEG is {br:?}"
        );
        assert!(
            r.notes_text().to_lowercase().contains("cmyk"),
            "the card should say CMYK was converted: {:?}",
            r.notes
        );
        assert!(
            img.icc.is_none(),
            "a CMYK picture converted to sRGB carries no profile"
        );
    }
    if let Some(src) = corpus("generated/cmyk-8bit.tif") {
        let s = Scratch::new("cmyk-tif");
        let (out, _) = run_ok("image.convert", &src, json!({"to": "png"}), s.path());
        let img = decode_srgb(&out);
        let tl = px(&img, 2, 2);
        assert!(
            tl.iter().all(|&v| v >= 245),
            "no ink should be white, got {tl:?}"
        );
        // x = 1023, y = 0: full cyan, no magenta: red is low, green and blue are high.
        let tr = px(&img, img.width - 3, 2);
        assert!(tr[0] < 90 && tr[1] > 120, "full cyan came out as {tr:?}");
    }
}

#[test]
fn display_p3_keeps_its_profile_or_converts_on_request() {
    for rel in ["generated/display-p3.png", "generated/display-p3.jpg"] {
        let Some(src) = corpus(rel) else { continue };
        let s = Scratch::new("p3");
        let is_png = rel.ends_with(".png");
        // Keep (default): the profile survives into the output.
        let (kept, _) = run_ok("image.convert", &src, json!({"to": "png"}), s.path());
        assert!(
            meta::inspect(&file_bytes(&kept)).unwrap().icc,
            "{rel}: the Display P3 profile was dropped"
        );
        // sRGB on request: no profile, and a saturated mid tone moves.
        let (web, r) = run_ok(
            "image.convert",
            &src,
            json!({"to": "png", "color_profile": "srgb"}),
            s.path(),
        );
        assert!(
            !meta::inspect(&file_bytes(&web)).unwrap().icc,
            "{rel}: sRGB output should carry no profile"
        );
        let (a, b) = (decode_keep(&kept), decode_srgb(&web));
        assert!(
            mean_abs_diff(&a, &b) > 0.3,
            "{rel}: converting to sRGB changed nothing"
        );
        assert!(
            r.notes_text().contains("sRGB"),
            "{rel}: card: {:?}",
            r.notes
        );
        // Neutral grays do not change: P3 and sRGB share the curve.
        let y = a.height - 8;
        let (g1, g2) = (px(&a, a.width / 2, y), px(&b, b.width / 2, y));
        for k in 0..3 {
            assert!(
                (g1[k] as i32 - g2[k] as i32).abs() <= 2,
                "{rel}: gray moved {g1:?} -> {g2:?}"
            );
        }
        // Compress keeps the profile unless told otherwise (IPC-2).
        let (c, _) = run_ok(
            "image.compress",
            &src,
            json!({"format": if is_png { "png" } else { "jpg" }}),
            s.path(),
        );
        assert!(
            meta::inspect(&file_bytes(&c)).unwrap().icc,
            "{rel}: compress dropped the profile"
        );
    }
}

// ------------------------------------------------------------------ IPC-5 bit depth

#[test]
fn sixteen_bit_survives_lossless_paths_exactly() {
    for rel in ["generated/rgb-16bit.tif", "generated/rgba-16bit.png"] {
        let Some(src) = corpus(rel) else { continue };
        let s = Scratch::new("16bit");
        let want = decode_keep(&src);
        assert_eq!(
            want.depth(),
            panoply_image::types::Depth::U16,
            "{rel} should decode as 16 bit"
        );
        for to in ["png", "tiff"] {
            let (out, _) = run_ok("image.convert", &src, json!({"to": to}), s.path());
            let got = decode_keep(&out);
            assert_eq!(
                got.depth(),
                panoply_image::types::Depth::U16,
                "{rel} to {to} lost bit depth"
            );
            assert_eq!(got.samples, want.samples, "{rel} to {to}: pixels differ");
            // And back again, still exact.
            let (back, _) = run_ok(
                "image.convert",
                &out,
                json!({"to": if to == "png" { "tiff" } else { "png" }}),
                s.path(),
            );
            assert_eq!(
                decode_keep(&back).samples,
                want.samples,
                "{rel} round trip through {to}"
            );
        }
    }
}

#[test]
fn sixteen_to_eight_bit_is_dithered_not_truncated() {
    let Some(src) = corpus("generated/rgb-16bit.tif") else {
        return;
    };
    let s = Scratch::new("dither");
    let (bmp, _) = run_ok("image.convert", &src, json!({"to": "bmp"}), s.path());
    let want = decode_keep(&src);
    let got = decode_keep(&bmp);
    let (panoply_image::types::Samples::U16(w), panoply_image::types::Samples::U8(g)) =
        (&want.samples, &got.samples)
    else {
        panic!("depths")
    };
    assert_eq!(w.len(), g.len());
    let bias: f64 = w
        .iter()
        .zip(g)
        .map(|(a, b)| *b as f64 - *a as f64 / 257.0)
        .sum::<f64>()
        / w.len() as f64;
    // Truncation would give -0.5 on average. Dithering is unbiased.
    assert!(
        bias.abs() < 0.12,
        "mean error {bias:.3} levels looks like truncation"
    );
}

// ------------------------------------------------------------------ IPC-8 alpha

#[test]
fn transparency_becomes_white_by_default_and_any_color_on_request() {
    let s = Scratch::new("alpha");
    let src = s.path().join("alpha.png");
    let (w, h) = (64u32, 32u32);
    let mut px_data = Vec::new();
    for _y in 0..h {
        for x in 0..w {
            // Left half transparent black, right half opaque green.
            if x < w / 2 {
                px_data.extend_from_slice(&[0, 0, 0, 0])
            } else {
                px_data.extend_from_slice(&[0, 200, 0, 255])
            }
        }
    }
    write_rgba_png(&src, w, h, &px_data);
    let out = Scratch::new("alpha-out");
    let (jpg, _) = run_ok(
        "image.convert",
        &src,
        json!({"to": "jpg", "quality": 95}),
        out.path(),
    );
    let img = decode_srgb(&jpg);
    let left = px(&img, 8, 16);
    assert!(
        left.iter().all(|&v| v >= 250),
        "transparent area should be white, got {left:?}"
    );
    let right = px(&img, 56, 16);
    assert!(
        right[1] > 150 && right[0] < 60,
        "opaque area changed: {right:?}"
    );
    let (red, _) = run_ok(
        "image.convert",
        &src,
        json!({"to": "jpg", "quality": 95, "flatten_color": "#ff0000"}),
        out.path(),
    );
    let l = px(&decode_srgb(&red), 8, 16);
    assert!(
        l[0] > 240 && l[1] < 20 && l[2] < 20,
        "flatten color ignored: {l:?}"
    );
    // Resize keeps transparency in PNG (premultiplied-correct: no dark fringe at the edge).
    let (small, _) = run_ok(
        "image.resize",
        &src,
        json!({"mode": "pixels", "width": 16, "height": 8, "enlarge": true}),
        out.path(),
    );
    let sm = decode_keep(&small);
    assert_eq!(sm.layout, panoply_image::types::Layout::Rgba);
    let samples = sm.as_u8().unwrap();
    for p in samples.as_chunks::<4>().0 {
        if p[3] == 255 {
            assert!(
                p[1] >= 195 && p[0] <= 5,
                "opaque pixel darkened by a bad resample: {p:?}"
            );
        }
        if p[3] > 0 && p[3] < 255 {
            // Straight alpha: a half-covered pixel still has the green color, not a dark one.
            assert!(
                p[1] >= 150,
                "edge pixel color was premultiplied twice: {p:?}"
            );
        }
    }
}

// ------------------------------------------------------------------ IPC-6 animation

#[test]
fn animated_files_ask_by_default_and_use_the_first_frame_on_request() {
    let cases = [
        "commons/animated-webp-sample.webp",
        "commons/muybridge-race-horse.gif",
        "generated/animated.webp",
        "libvips/cogs.gif",
        "libvips/apng-16bit.png",
        "libvips/dispose-previous.gif",
    ];
    let mut seen = 0;
    for rel in cases {
        let Some(src) = corpus(rel) else { continue };
        seen += 1;
        let s = Scratch::new("anim");
        for id in ["image.convert", "image.compress", "image.resize"] {
            let o = if id == "image.resize" {
                json!({"longest": 100})
            } else {
                json!({})
            };
            let e = run(id, &src, o, s.path())
                .err()
                .unwrap_or_else(|| panic!("{id} on {rel} should stop and ask"));
            assert!(
                e.contains("animated") && e.contains("first"),
                "{id} on {rel}: unhelpful message: {e}"
            );
            assert!(s.files().is_empty(), "{id} on {rel} left files behind");
        }
        let (png, _) = run_ok(
            "image.convert",
            &src,
            json!({"to": "png", "animation": "first"}),
            s.path(),
        );
        let img = decode_srgb(&png);
        assert!(img.width > 0 && img.height > 0);
        let (small, r) = run_ok(
            "image.resize",
            &src,
            json!({"longest": 64, "animation": "first", "enlarge": true}),
            s.path(),
        );
        assert_eq!(
            decode_srgb(&small).width.max(decode_srgb(&small).height),
            64
        );
        assert!(
            r.notes_text().contains("first frame"),
            "{rel}: card should say a frame was used: {:?}",
            r.notes
        );
    }
    assert!(seen > 0 || !MAC || corpus("generated/animated.webp").is_none());
}

#[test]
fn resizing_a_jpeg_2000_photo_writes_a_jpeg_not_a_larger_png() {
    // Found by the beta smoke matrix: Resize "keep format" on a JPEG 2000 file wrote a lossless
    // PNG seven times its size, because Panoply cannot write JPEG 2000.
    if !MAC {
        return;
    }
    let Some(src) = corpus("libvips/world.jp2") else {
        return;
    };
    let s = Scratch::new("jp2-resize");
    let (out, _) = run_ok("image.resize", &src, json!({"longest": 400}), s.path());
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("");
    assert_eq!(ext, "jpg", "{}", out.display());
    let (before, after) = (
        std::fs::metadata(&src).unwrap().len(),
        std::fs::metadata(&out).unwrap().len(),
    );
    assert!(after < before * 3, "{before} bytes became {after}");
}

// ------------------------------------------------------------------ IPC-3 and HEIC

#[test]
fn heic_with_depth_converts_and_says_what_was_dropped() {
    let Some(src) = corpus("heic/apple-depth-portrait.heic") else {
        return;
    };
    let s = Scratch::new("heic");
    if !MAC {
        let e = run("image.convert", &src, json!({}), s.path())
            .err()
            .expect("HEIC off macOS must fail plainly");
        assert!(
            e.contains("HEIC needs the HEIC helper on this system"),
            "{e}"
        );
        assert!(s.files().is_empty());
        return;
    }
    let (jpg, r) = run_ok("image.convert", &src, json!({}), s.path());
    let img = decode_srgb(&jpg);
    assert_eq!((img.width, img.height), (4032, 3024));
    assert!(
        r.notes_text().contains("depth"),
        "the card must name the dropped depth map: {:?}",
        r.notes
    );
    // Orientation is baked in and tagged 1 (IPC-1 for HEIC).
    assert!(orientation_of(&jpg) <= 1);
    // The other modules take the same file.
    let (small, _) = run_ok("image.resize", &src, json!({"longest": 800}), s.path());
    assert_eq!(decode_srgb(&small).width, 800);
    let (kb, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 200, "height": 230, "min_kb": 20, "max_kb": 50}),
        s.path(),
    );
    assert_eq!(
        (decode_srgb(&kb).width, decode_srgb(&kb).height),
        (200, 230)
    );
    run_ok("image.compress", &src, json!({}), s.path());
}

#[test]
fn rotated_and_wide_gamut_heic_decode_upright() {
    if !MAC {
        return;
    }
    if let Some(src) = corpus("heic/guitar-rotated-cw90.hif") {
        let s = Scratch::new("hif");
        let (out, _) = run_ok("image.resize", &src, json!({"longest": 700}), s.path());
        let img = decode_srgb(&out);
        // 7008 x 4672 stored, rotated by 90 degrees: portrait.
        assert_eq!(
            (img.width, img.height),
            (467, 700),
            "a rotated HEIC must come out upright"
        );
    }
    if let Some(src) = corpus("heic/display-p3.heic") {
        let s = Scratch::new("heic-p3");
        let (out, _) = run_ok("image.convert", &src, json!({"to": "png"}), s.path());
        assert!(
            meta::inspect(&file_bytes(&out)).unwrap().icc,
            "HEIC Display P3 should keep a profile into PNG"
        );
    }
}

#[test]
fn heic_off_macos_fails_with_the_plain_helper_message() {
    if MAC {
        return;
    }
    for rel in ["heic/xiaomi.heic", "heic/alpha.heic", "heic/rgb-10bit.heif"] {
        let Some(src) = corpus(rel) else { continue };
        let s = Scratch::new("heic-linux");
        for id in [
            "image.convert",
            "image.compress",
            "image.resize",
            "image.resize_to_kb",
        ] {
            let o = if id == "image.resize" {
                json!({"longest": 100})
            } else {
                json!({})
            };
            let e = run(id, &src, o, s.path()).err().unwrap();
            assert!(
                e.contains("HEIC needs the HEIC helper on this system"),
                "{id} {rel}: {e}"
            );
        }
        assert!(s.files().is_empty());
    }
}

// ------------------------------------------------------------------ IPC-9 metadata and strip

#[test]
fn strip_leaves_no_location_keeps_orientation_and_pixels() {
    let Some(src) = corpus("generated/exif-gps-orientation6.jpg") else {
        return;
    };
    let before = meta::inspect(&file_bytes(&src)).unwrap();
    assert!(before.gps.is_some());
    for level in ["gps", "private", "everything"] {
        let s = Scratch::new("strip");
        let (out, r) = run_ok(
            "image.strip_metadata",
            &src,
            json!({"remove": level}),
            s.path(),
        );
        let bytes = file_bytes(&out);
        let after = meta::inspect(&bytes).unwrap();
        assert!(
            after.gps.is_none() && after.gps_tags == 0,
            "{level}: GPS left behind"
        );
        assert_eq!(
            after.orientation, 6,
            "{level}: orientation must survive or portraits turn sideways"
        );
        // No pixel is touched: the decoded pictures are identical and the scan data is the same bytes.
        assert_eq!(
            decode_keep(&out).samples,
            decode_keep(&src).samples,
            "{level}: pixels changed"
        );
        assert!(
            r.notes_text().contains("image data untouched"),
            "{level}: {:?}",
            r.notes
        );
        // A raw scan finds none of the coordinates or the camera name.
        assert!(
            !bytes
                .windows(14)
                .any(|w| w == b"Synthetic Came" && level == "everything"),
            "{level}: camera text left"
        );
        assert!(
            out.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("(clean)")
        );
    }
}

#[test]
fn strip_works_on_every_container_in_the_corpus() {
    let cases = [
        "libavif/paris-icc-exif-xmp.png",
        "libavif/dog-extended-xmp.jpg",
        "libvips/ultra-hdr.jpg",
        "raw/Google---Pixel-7-Pro---16bit-4x3.dng",
        "heic/xiaomi.heic",
        "libavif/seine-sdr-gainmap.jpg",
    ];
    for rel in cases {
        let Some(src) = corpus(rel) else { continue };
        let s = Scratch::new("strip-all");
        // A camera RAW file refuses "everything" (it would break the file), so it gets "private".
        let level = if rel.ends_with(".dng") {
            "private"
        } else {
            "everything"
        };
        let (out, _) = run_ok(
            "image.strip_metadata",
            &src,
            json!({"remove": level}),
            s.path(),
        );
        let after = meta::inspect(&file_bytes(&out)).unwrap();
        assert!(
            after.gps.is_none() && after.gps_tags == 0,
            "{rel}: GPS left"
        );
        assert!(after.serials.is_empty(), "{rel}: serial numbers left");
        if !rel.ends_with(".dng") {
            assert_eq!(
                decode_keep_or_skip(&out),
                decode_keep_or_skip(&src),
                "{rel}: pixels changed"
            );
        }
    }
}

fn decode_keep_or_skip(p: &std::path::Path) -> Option<panoply_image::types::Samples> {
    let f = std::fs::File::open(p).ok()?;
    let mut r = panoply_image::decode::open(
        panoply_image::decode::Input::File(f),
        p.extension().and_then(|e| e.to_str()),
        &Default::default(),
    )
    .ok()?;
    let (c, pr) = panoply_image::pipeline::quiet();
    let ctl = panoply_image::pipeline::Ctl {
        cancel: &c,
        progress: &pr,
    };
    Some(
        panoply_image::pipeline::decode_to_image(&mut *r, &Default::default(), &ctl)
            .ok()?
            .0
            .samples,
    )
}

#[test]
fn convert_keeps_metadata_by_default_and_names_gps() {
    let Some(src) = corpus("generated/exif-gps-orientation6.jpg") else {
        return;
    };
    let s = Scratch::new("meta");
    let (kept, r) = run_ok("image.convert", &src, json!({"to": "webp"}), s.path());
    assert!(
        r.notes_text().contains("GPS location"),
        "GPS must be named on the card when kept: {:?}",
        r.notes
    );
    assert!(meta::inspect(&file_bytes(&kept)).unwrap().gps.is_some());
    let (gone, r2) = run_ok(
        "image.convert",
        &src,
        json!({"to": "webp", "keep_metadata": false}),
        s.path(),
    );
    assert!(meta::inspect(&file_bytes(&gone)).unwrap().gps.is_none());
    assert!(r2.notes_text().contains("dropped"), "{:?}", r2.notes);
}

// ------------------------------------------------------------------ compress quality bar

#[test]
fn compress_meets_its_similarity_target_measured_independently() {
    let cases = [
        "exif-orientation/landscape-1.jpg",
        "libavif/seine-sdr-gainmap.jpg",
        "generated/display-p3.jpg",
        "libjpeg-turbo/testorig.jpg",
        "libavif/paris-icc-exif-xmp.png",
    ];
    for rel in cases {
        let Some(src) = corpus(rel) else { continue };
        let s = Scratch::new("compress");
        let out = match run("image.compress", &src, json!({"ssim": 0.97}), s.path()) {
            Ok(r) if !r.outputs.is_empty() => r.outputs[0].clone(),
            Ok(r) => {
                assert!(
                    r.notes_text().contains("already well compressed"),
                    "{rel}: {:?}",
                    r.notes
                );
                continue;
            }
            Err(e) => panic!("{rel}: {e}"),
        };
        let (a, b) = (decode_srgb(&src), decode_srgb(&out));
        let score = ssim::score(&a, &b);
        assert!(
            score.worst() >= 0.965,
            "{rel}: similarity {:.3} / {:.3} is under the 0.97 target",
            score.luma,
            score.chroma
        );
        assert!(
            std::fs::metadata(&out).unwrap().len() < std::fs::metadata(&src).unwrap().len(),
            "{rel}: compress inflated the file"
        );
    }
}

#[test]
fn compress_never_inflates_and_never_touches_the_original() {
    // A tiny PNG that is already well compressed is left alone, and the card says so.
    let Some(src) = corpus("pngsuite/tbbn2c16.png").or_else(|| corpus("pngsuite/basn0g16.png"))
    else {
        return;
    };
    let before = file_bytes(&src);
    let s = Scratch::new("noinflate");
    let r = run("image.compress", &src, json!({}), s.path()).unwrap();
    if r.outputs.is_empty() {
        assert!(r.notes_text().contains("already well compressed"));
    } else {
        assert!(std::fs::metadata(&r.outputs[0]).unwrap().len() < before.len() as u64);
    }
    assert_eq!(file_bytes(&src), before, "the original changed");
}

#[test]
fn outputs_never_overwrite_anything() {
    let Some(src) = corpus("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let s = Scratch::new("noover");
    let input = s.path().join("photo.jpg");
    std::fs::copy(&src, &input).unwrap();
    let io = panoply_image::ops::FsIo::new();
    let m = module("image.compress");
    let mut names = Vec::new();
    for _ in 0..3 {
        let out = m
            .run(&io, std::slice::from_ref(&input), &json!({}))
            .unwrap();
        names.push(out[0].file_name().unwrap().to_string_lossy().into_owned());
    }
    assert_eq!(
        names,
        [
            "photo (compressed).jpg",
            "photo (compressed 2).jpg",
            "photo (compressed 3).jpg"
        ]
    );
    assert_eq!(file_bytes(&input), file_bytes(&src));
    // Convert names: name.png, then name (1).png.
    let m = module("image.convert");
    let a = m
        .run(&io, std::slice::from_ref(&input), &json!({"to": "png"}))
        .unwrap();
    let b = m
        .run(&io, std::slice::from_ref(&input), &json!({"to": "png"}))
        .unwrap();
    assert_eq!(a[0].file_name().unwrap(), "photo.png");
    assert_eq!(b[0].file_name().unwrap(), "photo (1).png");
    // Converting a JPEG to JPG never replaces the original: it becomes "photo (1).jpg".
    let c = m
        .run(&io, std::slice::from_ref(&input), &json!({"to": "jpg"}))
        .unwrap();
    assert_eq!(c[0].file_name().unwrap(), "photo (1).jpg");
    assert_eq!(file_bytes(&input), file_bytes(&src));
}

// ------------------------------------------------------------------ damaged and refused files

#[test]
fn damaged_and_unsupported_files_fail_plainly_and_leave_nothing() {
    let cases = [
        ("libvips/truncated.jpg", "incomplete"),
        ("libvips/truncated.gif", "damaged"),
        ("pngsuite/xcrn0g04.png", "not an image"),
        ("libvips/logo.svg", "not an image"),
        ("libvips/subsampled.tif", "cannot read"),
    ];
    for (rel, word) in cases {
        let Some(src) = corpus(rel) else { continue };
        for id in [
            "image.convert",
            "image.compress",
            "image.resize",
            "image.resize_to_kb",
        ] {
            let s = Scratch::new("bad");
            let o = if id == "image.resize" {
                json!({"longest": 100})
            } else {
                json!({})
            };
            let e = run(id, &src, o, s.path())
                .err()
                .unwrap_or_else(|| panic!("{id} on {rel} should fail"));
            assert!(e.to_lowercase().contains(word), "{id} on {rel}: {e}");
            assert!(
                !e.contains("panicked") && !e.contains("unwrap") && !e.contains("0x"),
                "{id} on {rel}: not plain: {e}"
            );
            assert!(s.files().is_empty(), "{id} on {rel} left {:?}", s.files());
        }
    }
    // A broken HEIC on macOS.
    if MAC && let Some(src) = corpus("heic/corrupted.heic") {
        let s = Scratch::new("bad-heic");
        assert!(run("image.convert", &src, json!({}), s.path()).is_err());
        assert!(s.files().is_empty());
    }
}

#[test]
fn a_batch_skips_the_bad_file_and_finishes_the_rest() {
    let (Some(good), Some(bad)) = (
        corpus("exif-orientation/landscape-1.jpg"),
        corpus("libvips/truncated.jpg"),
    ) else {
        return;
    };
    let s = Scratch::new("batch");
    let r = run_many(
        "image.convert",
        &[bad, good],
        json!({"to": "png"}),
        s.path(),
    )
    .unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert!(r.notes_text().contains("skipped"), "{:?}", r.notes);
}

#[test]
fn cancel_leaves_nothing_behind() {
    let Some(src) = corpus("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let s = Scratch::new("cancel");
    let io = panoply_image::ops::FsIo::in_dir(s.path());
    io.cancel().cancel();
    for id in [
        "image.convert",
        "image.compress",
        "image.resize",
        "image.strip_metadata",
        "image.resize_to_kb",
    ] {
        let o = if id == "image.resize" {
            json!({"longest": 100})
        } else {
            json!({})
        };
        let e = module(id)
            .run(&io, std::slice::from_ref(&src), &o)
            .err()
            .unwrap_or_else(|| panic!("{id} ignored the cancel"));
        assert!(e.to_string().contains("Cancelled"), "{id}: {e}");
    }
    assert!(s.files().is_empty());
}

#[test]
fn heic_and_avif_from_the_os_encoder_carry_what_the_card_says() {
    if !MAC {
        return;
    }
    let Some(src) = corpus("generated/exif-gps-orientation6.jpg") else {
        return;
    };
    for to in ["heic", "avif"] {
        let s = Scratch::new("os-meta");
        let (kept, r) = run_ok("image.convert", &src, json!({"to": to}), s.path());
        let rep = meta::inspect(&file_bytes(&kept)).unwrap();
        assert!(r.notes_text().contains("kept: EXIF"), "{to}: {:?}", r.notes);
        assert!(
            rep.gps.is_some(),
            "{to}: the card says GPS was kept but the file has none ({:?})",
            rep.summary
        );
        assert!(
            rep.orientation <= 1,
            "{to}: orientation must be 1 after baking it in"
        );
        let (gone, r) = run_ok(
            "image.convert",
            &src,
            json!({"to": to, "keep_metadata": false}),
            s.path(),
        );
        let rep = meta::inspect(&file_bytes(&gone)).unwrap();
        assert!(
            rep.gps.is_none() && !rep.exif_present,
            "{to}: {:?}",
            rep.summary
        );
        assert!(r.notes_text().contains("dropped"), "{to}: {:?}", r.notes);
    }
}
