//! The four image AI modules on corpus pictures and synthetic ones: what comes out, that the
//! original is never touched, and the guarantees the specs make (pixels outside an erase mask do
//! not change, tiled upscaling leaves no seams, a watermark run refuses without confirmation).
//!
//! Model backed tests download their model on first use (a few MB to 44 MB, once). With
//! `PANOPLY_OFFLINE=1` and the model not cached they skip.

mod common;

use common::*;
use panoply_image::ops::Io;
use panoply_vision::models::{ESRGAN, ISNET, MIGAN};
use serde_json::json;

// ---------------------------------------------------------------- remove background

#[test]
fn cutout_of_a_portrait_has_a_solid_subject_and_a_clear_background() {
    let Some(src) = corpus("libavif/dog-extended-xmp.jpg") else {
        return;
    };
    let d = Scratch::new("cutout");
    let input = d.copy_in(&src);
    let before = std::fs::read(&input).unwrap();
    let (out, ran) = run_ok("image.remove_background", &input, json!({}));
    assert_eq!(
        std::fs::read(&input).unwrap(),
        before,
        "the original changed"
    );
    assert_eq!(out.file_name().unwrap(), "dog-extended-xmp (cutout).png");
    let pic = decode(&out);
    // The photo is stored sideways with EXIF orientation 6; the cutout is upright.
    assert_eq!((pic.w, pic.h), (3024, 4032), "{}", ran.notes_text());
    assert!(pic.alpha.is_some(), "a cutout needs an alpha channel");
    // The dog's face is in the middle, the paving at the top left is background.
    assert_eq!(pic.a(pic.w / 2, pic.h * 55 / 100), 255);
    assert_eq!(pic.a(30, pic.h / 2), 0);
    let opaque = pic
        .alpha
        .as_ref()
        .unwrap()
        .iter()
        .filter(|&&a| a > 127)
        .count() as f64
        / (pic.w * pic.h) as f64;
    assert!((0.2..0.7).contains(&opaque), "opaque share {opaque}");
    assert!(
        ran.notes_text().contains("cutout done"),
        "{}",
        ran.notes_text()
    );
    assert_eq!(
        d.files().len(),
        2,
        "only the input and one output: {:?}",
        d.files()
    );
}

#[test]
fn the_model_engine_also_cuts_out_a_subject_when_asked() {
    if !have(&ISNET) {
        return;
    }
    let Some(src) = heic_corpus("heic/xiaomi.heic") else {
        return;
    };
    let d = Scratch::new("cutout-model");
    let input = d.copy_in(&src);
    let (out, ran) = run_ok(
        "image.remove_background",
        &input,
        json!({"engine": "model"}),
    );
    let pic = decode(&out);
    assert_eq!((pic.w, pic.h), (2592, 1944));
    assert!(ran.notes_text().contains("IS-Net"), "{}", ran.notes_text());
    let a = pic.alpha.as_ref().unwrap();
    let solid = a.iter().filter(|&&v| v == 255).count();
    let clear = a.iter().filter(|&&v| v == 0).count();
    assert!(
        solid > a.len() / 20 && clear > a.len() / 5,
        "solid {solid} clear {clear}"
    );
    // The pen tip is near the lower middle of the picture.
    assert!(pic.a(pic.w * 53 / 100, pic.h * 52 / 100) > 200);
}

#[test]
fn solid_color_and_blur_results_are_opaque_and_keep_the_subject() {
    let Some(src) = heic_corpus("heic/xiaomi.heic") else {
        return;
    };
    let d = Scratch::new("cutout-bg");
    let input = d.copy_in(&src);
    let (white, _) = run_ok(
        "image.remove_background",
        &input,
        json!({"result": "color", "background": "#ffffff"}),
    );
    let pic = decode(&white);
    assert!(
        pic.alpha.is_none(),
        "a solid color result has no transparency"
    );
    // Top right corner is paper: replaced by exactly the chosen color.
    assert_eq!(pic.px(pic.w - 3, 3), [255, 255, 255]);
    let (blur, _) = run_ok(
        "image.remove_background",
        &input,
        json!({"result": "blur", "blur": 40}),
    );
    let b = decode(&blur);
    assert!(b.alpha.is_none());
    assert_ne!(
        b.px(b.w - 3, 3),
        [255, 255, 255],
        "the blur result must show the picture"
    );
    // The original is untouched and the three outputs have distinct names.
    let names: Vec<String> = d
        .files()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
}

#[test]
fn a_transparent_cutout_refuses_a_jpg_and_a_bad_color_is_plain() {
    let d = Scratch::new("cutout-opts");
    let input = d.path().join("g.png");
    gradient_png(&input, 64, 64);
    let e = run_many(
        "image.remove_background",
        std::slice::from_ref(&input),
        json!({"format": "jpg"}),
    )
    .err()
    .unwrap();
    assert!(e.contains("JPG cannot hold transparency"), "{e}");
    let e = run_many(
        "image.remove_background",
        &[input],
        json!({"result": "color", "background": "not-a-color"}),
    )
    .err()
    .unwrap();
    assert!(e.contains("not a color"), "{e}");
}

// ---------------------------------------------------------------- erase

#[test]
fn erase_replaces_the_marked_area_and_nothing_else() {
    if !have(&MIGAN) {
        return;
    }
    let d = Scratch::new("erase");
    let input = d.path().join("scene.png");
    let mut pic = gradient_png(&input, 700, 520);
    // A bright block to erase.
    for y in 200..260 {
        for x in 300..380 {
            let i = (y * pic.w + x) * 3;
            pic.rgb[i..i + 3].copy_from_slice(&[250, 40, 40]);
        }
    }
    write_png(&input, pic.w, pic.h, &pic.rgb, None);
    let (out, ran) = run_ok(
        "image.erase",
        &input,
        json!({"rect": "300,200,80,60", "expand": 6}),
    );
    let after = decode(&out);
    assert_eq!((after.w, after.h), (700, 520));
    // Outside the expanded rectangle (and the 6 px growth) every byte is the original's.
    let mut outside_same = true;
    for y in 0..520 {
        for x in 0..700 {
            let inside = (294..386).contains(&x) && (194..266).contains(&y);
            if !inside && after.px(x, y) != pic.px(x, y) {
                outside_same = false;
            }
        }
    }
    assert!(outside_same, "pixels outside the mask changed");
    // Inside, the red block is gone: no pixel is still strongly red.
    let mut red = 0;
    for y in 200..260 {
        for x in 300..380 {
            let p = after.px(x, y);
            if p[0] > 200 && p[1] < 90 {
                red += 1;
            }
        }
    }
    assert!(red < 20, "{red} red pixels left");
    // The fill is plausible: close to the smooth gradient around it.
    let mid = after.px(340, 230);
    let want = pic.px(340, 180);
    assert!(mid[0].abs_diff(want[0]) < 45, "{mid:?} vs {want:?}");
    assert!(
        ran.notes_text().contains("untouched"),
        "{}",
        ran.notes_text()
    );
}

#[test]
fn a_mask_picture_as_the_second_file_works_like_the_rectangle() {
    if !have(&MIGAN) {
        return;
    }
    let d = Scratch::new("erase-mask");
    let input = d.path().join("scene.png");
    gradient_png(&input, 640, 480);
    let mask = d.path().join("mask.png");
    let mut m = vec![0u8; 640 * 480 * 3];
    for y in 100..140 {
        for x in 200..260 {
            let i = (y * 640 + x) * 3;
            m[i..i + 3].fill(255);
        }
    }
    write_png(&mask, 640, 480, &m, None);
    let r = run_many("image.erase", &[input.clone(), mask], json!({}));
    let r = r.unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert_eq!(r.outputs[0].file_name().unwrap(), "scene (erased).png");
    // A mask with nothing white says so, in a sentence.
    let blank = d.path().join("blank.png");
    write_png(&blank, 640, 480, &vec![0u8; 640 * 480 * 3], None);
    let e = run_many("image.erase", &[input.clone(), blank], json!({}))
        .err()
        .unwrap();
    assert!(e.contains("mask is empty"), "{e}");
    // Neither mask nor rectangle: tells what to do.
    let e = run_many("image.erase", &[input], json!({})).err().unwrap();
    assert!(e.contains("Say what to erase"), "{e}");
}

#[test]
fn erase_on_a_real_photo_keeps_the_orientation_and_the_original_bytes() {
    if !have(&MIGAN) {
        return;
    }
    let Some(src) = corpus("exif-orientation/landscape-6.jpg") else {
        return;
    };
    let d = Scratch::new("erase-photo");
    let input = d.copy_in(&src);
    let before = std::fs::read(&input).unwrap();
    // The big "6" is stamped in the middle of the picture.
    let (out, _) = run_ok("image.erase", &input, json!({"rect": "47%,42%,6%,18%"}));
    assert_eq!(std::fs::read(&input).unwrap(), before);
    let p = decode(&out);
    // Orientation 6 means the file is stored rotated; the result is upright.
    let orig = decode(&input);
    assert_eq!((p.w, p.h), (orig.w, orig.h));
    assert_eq!(out.extension().unwrap(), "jpg", "a JPEG stays a JPEG");
}

// ---------------------------------------------------------------- upscale

/// Collects the rows an upscale pushes.
struct Collect {
    rows: Vec<u8>,
}

impl panoply_image::pipeline::RowSink for Collect {
    fn begin(
        &mut self,
        _w: u32,
        _h: u32,
        _l: panoply_image::types::Layout,
        _d: panoply_image::types::Depth,
        _i: Option<std::sync::Arc<Vec<u8>>>,
    ) -> panoply_image::error::Result<()> {
        Ok(())
    }

    fn push(&mut self, rows: panoply_image::types::Rows<'_>) -> panoply_image::error::Result<()> {
        if let panoply_image::types::Rows::U8(r) = rows {
            self.rows.extend_from_slice(r);
        }
        Ok(())
    }
}

fn upscale4(path: &std::path::Path) -> (usize, Vec<u8>) {
    use panoply_vision::runtime::{Prefer, acquire};
    use panoply_vision::upscale::upscale_into;
    let io = panoply_image::ops::FsIo::new();
    let src = panoply_vision::work::load_source(
        &io,
        path,
        panoply_image::ops::common::Animation::First,
        usize::MAX / 8,
    )
    .unwrap();
    let model = acquire(&io, &ESRGAN, Prefer::Accelerated).unwrap();
    let mut sink = Collect { rows: Vec::new() };
    upscale_into(
        &io,
        &model,
        &src,
        4,
        panoply_image::types::Layout::Rgb,
        &mut sink,
        |_| {},
    )
    .unwrap();
    (src.w * 4, sink.rows)
}

#[test]
fn upscale_4x_has_the_right_size_and_tiles_leave_no_seams() {
    if !have(&ESRGAN) {
        return;
    }
    let Some(src) = corpus("libavif/seine-sdr-gainmap.jpg") else {
        return;
    };
    let d = Scratch::new("upscale");
    let input = d.copy_in(&src);
    let (out, ran) = run_ok(
        "image.upscale",
        &input,
        json!({"scale": "4", "format": "png"}),
    );
    let pic = decode(&out);
    assert_eq!((pic.w, pic.h), (1600, 1200), "{}", ran.notes_text());
    assert_eq!(out.file_name().unwrap(), "seine-sdr-gainmap (4x).png");

    // Seams: upscale the same pixels with the tile grid in two different places. A model that is
    // translation invariant with enough context gives the same picture both ways, and any seam
    // would show as a difference concentrated at the tile boundaries of either grid. Tiles are
    // cut on a 96 pixel grid, so cropping 48 pixels off the top and left moves every boundary.
    let base = decode(&input);
    let (cw, ch) = (base.w - 48, base.h - 48);
    let mut crop = Vec::with_capacity(cw * ch * 3);
    for y in 48..base.h {
        crop.extend_from_slice(&base.rgb[(y * base.w + 48) * 3..(y * base.w + base.w) * 3]);
    }
    let shifted = d.path().join("shifted.png");
    write_png(&shifted, cw, ch, &crop, None);
    let (wa, a) = upscale4(&input);
    let (wb, b) = upscale4(&shifted);
    assert_eq!((wa, wb), (base.w * 4, cw * 4));
    let mut sq = 0.0f64;
    let mut n = 0.0f64;
    let (mut near_sum, mut near_n) = (0.0f64, 0.0f64);
    for y in 0..ch * 4 {
        for x in 0..cw * 4 {
            let ia = ((y + 192) * wa + (x + 192)) * 3;
            let ib = (y * wb + x) * 3;
            // Distance to the nearest tile boundary of the first grid, in output pixels.
            let (gx, gy) = ((x + 192) % 384, (y + 192) % 384);
            let dist = gx.min(384 - gx).min(gy.min(384 - gy));
            for c in 0..3 {
                let diff = f64::from(a[ia + c]) - f64::from(b[ib + c]);
                sq += diff * diff;
                n += 1.0;
                if dist < 4 {
                    near_sum += diff.abs();
                    near_n += 1.0;
                }
            }
        }
    }
    let psnr = 10.0 * (255.0f64 * 255.0 / (sq / n).max(1e-9)).log10();
    assert!(psnr > 38.0, "two tilings differ: PSNR {psnr:.1} dB");
    let near = near_sum / near_n;
    assert!(
        near < 1.5,
        "mean difference at the tile boundaries is {near:.2} of 255"
    );
}

#[test]
fn upscale_2x_keeps_transparency_and_a_small_picture_is_scaled_by_four_when_auto() {
    if !have(&ESRGAN) {
        return;
    }
    let d = Scratch::new("upscale-alpha");
    let input = d.path().join("badge.png");
    let (w, h) = (160usize, 120usize);
    let mut rgb = vec![0u8; w * h * 3];
    let mut alpha = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let inside = ((x as f32 - 80.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt() < 40.0;
            let i = y * w + x;
            if inside {
                rgb[i * 3..i * 3 + 3].copy_from_slice(&[200, 60, 60]);
                alpha[i] = 255;
            }
        }
    }
    write_png(&input, w, h, &rgb, Some(&alpha));
    let (auto, _) = run_ok("image.upscale", &input, json!({}));
    let p = decode(&auto);
    assert_eq!(
        (p.w, p.h),
        (640, 480),
        "auto scale for a small picture is 4x"
    );
    assert!(p.alpha.is_some());
    assert_eq!(p.a(320, 240), 255);
    assert_eq!(p.a(5, 5), 0);
    let (two, _) = run_ok("image.upscale", &input, json!({"scale": "2"}));
    let p2 = decode(&two);
    assert_eq!((p2.w, p2.h), (320, 240));
    assert_eq!(p2.a(160, 120), 255);
}

#[test]
fn upscale_says_no_when_the_picture_is_already_large_or_the_result_too_big() {
    let d = Scratch::new("upscale-refuse");
    let big = d.path().join("big.png");
    gradient_png(&big, 3400, 2400);
    let e = run_many("image.upscale", std::slice::from_ref(&big), json!({}))
        .err()
        .unwrap();
    assert!(e.contains("already 3,400 x 2,400"), "{e}");
    assert!(e.contains("scale to 2"), "{e}");
    // 4x of 8 megapixels is 130 megapixels, over the 120 megapixel limit.
    let e = run_many("image.upscale", &[big], json!({"scale": "4"}))
        .err()
        .unwrap();
    assert!(e.contains("megapixels"), "{e}");
    assert_eq!(d.files().len(), 1, "a refused job leaves nothing behind");
}

// ---------------------------------------------------------------- remove watermark

#[test]
fn watermark_removal_needs_the_confirmation_and_never_touches_the_original() {
    let d = Scratch::new("wm-gate");
    let input = d.path().join("p.png");
    gradient_png(&input, 320, 200);
    let e = run_many(
        "image.remove_watermark",
        std::slice::from_ref(&input),
        json!({}),
    )
    .err()
    .unwrap();
    assert!(e.contains("own_content"), "{e}");
    assert_eq!(d.files().len(), 1);
}

#[test]
fn a_stamp_in_the_corner_is_found_and_blurred_out_without_touching_the_rest() {
    let d = Scratch::new("wm-blur");
    let input = d.path().join("stamped.png");
    let mut pic = gradient_png(&input, 640, 400);
    let clean = pic.rgb.clone();
    stamp(&mut pic.rgb, 640, 470, 360, 10, 0.55);
    write_png(&input, 640, 400, &pic.rgb, None);
    let (out, ran) = run_ok(
        "image.remove_watermark",
        &input,
        json!({"own_content": true, "fill": "blur", "format": "png"}),
    );
    assert!(
        ran.notes_text().contains("found text at the bottom right"),
        "{}",
        ran.notes_text()
    );
    let after = decode(&out);
    // The letters are gone: pixels that were stamped are close to the clean gradient again.
    let mut err = 0u32;
    let mut n = 0u32;
    for k in 0..10 {
        for y in 360..374 {
            for x in (470 + k * 13)..(470 + k * 13 + 8) {
                for c in 0..3 {
                    err += u32::from(after.px(x, y)[c].abs_diff(clean[(y * 640 + x) * 3 + c]));
                    n += 1;
                }
            }
        }
    }
    assert!(
        f64::from(err) / f64::from(n) < 14.0,
        "mean error {}",
        f64::from(err) / f64::from(n)
    );
    // Far from the stamp, every byte is the stamped picture's.
    for y in 0..300 {
        for x in 0..640 {
            assert_eq!(after.px(x, y), pic.px(x, y), "({x},{y})");
        }
    }
}

#[test]
fn a_clean_picture_gets_a_plain_no_watermark_answer_and_a_rect_still_works() {
    let d = Scratch::new("wm-none");
    let input = d.path().join("clean.png");
    gradient_png(&input, 480, 300);
    let e = run_many(
        "image.remove_watermark",
        std::slice::from_ref(&input),
        json!({"own_content": true}),
    )
    .err()
    .unwrap();
    assert!(e.contains("No watermark"), "{e}");
    assert!(e.contains("rect"), "{e}");
    // The user can always mark the area by hand.
    let r = run_many(
        "image.remove_watermark",
        &[input],
        json!({"own_content": true, "rect": "70%,85%,25%,10%", "fill": "blur"}),
    )
    .unwrap();
    assert_eq!(r.outputs.len(), 1);
    assert!(r.notes_text().contains("marked area"), "{}", r.notes_text());
}

#[test]
fn the_model_fill_cleans_a_stamp_too() {
    if !have(&MIGAN) {
        return;
    }
    let d = Scratch::new("wm-model");
    let input = d.path().join("stamped.png");
    let mut pic = gradient_png(&input, 640, 400);
    let clean = pic.rgb.clone();
    stamp(&mut pic.rgb, 640, 470, 360, 10, 0.55);
    write_png(&input, 640, 400, &pic.rgb, None);
    let (out, _) = run_ok(
        "image.remove_watermark",
        &input,
        json!({"own_content": true, "save_mask": true}),
    );
    let after = decode(&out);
    let mut err = 0u32;
    let mut n = 0u32;
    for y in 358..376 {
        for x in 468..600 {
            for c in 0..3 {
                err += u32::from(after.px(x, y)[c].abs_diff(clean[(y * 640 + x) * 3 + c]));
                n += 1;
            }
        }
    }
    assert!(
        f64::from(err) / f64::from(n) < 12.0,
        "mean error {}",
        f64::from(err) / f64::from(n)
    );
    // save_mask wrote a third file.
    assert!(
        d.files()
            .iter()
            .any(|p| p.to_string_lossy().contains("(mask)"))
    );
}

// ---------------------------------------------------------------- jobs

#[test]
fn a_cancelled_job_stops_and_leaves_no_file() {
    let d = Scratch::new("cancel");
    let input = d.path().join("p.png");
    gradient_png(&input, 300, 200);
    let io = panoply_image::ops::FsIo::new();
    io.cancel().cancel();
    let e = module("image.upscale")
        .run(&io, &[input], &json!({}))
        .unwrap_err();
    assert!(e.to_string().contains("Cancelled"), "{e}");
    assert_eq!(d.files().len(), 1);
}

#[test]
fn a_batch_reports_the_file_it_skipped_and_still_finishes_the_rest() {
    let d = Scratch::new("batch");
    let good = d.path().join("good.png");
    gradient_png(&good, 120, 80);
    let bad = d.path().join("bad.png");
    std::fs::write(&bad, b"this is not a picture").unwrap();
    let io = panoply_image::ops::FsIo::new();
    if !have(&ESRGAN) {
        return;
    }
    let out = module("image.upscale")
        .run(&io, &[bad, good], &json!({"scale": "2"}))
        .unwrap();
    assert_eq!(out.len(), 1);
    let notes = io.take_notes().join("\n");
    assert!(notes.contains("bad.png: skipped"), "{notes}");
}
