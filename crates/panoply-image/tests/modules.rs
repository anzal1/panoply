//! What each module's spec promises, checked on small synthetic pictures and a few corpus files:
//! manifests, resize math (linear light, no ringing, every fit), size targets and the Resize to
//! exact KB card and naming.

mod common;

use common::*;
use panoply_image::types::{Image, Samples};
use serde_json::json;

fn gray_png(path: &std::path::Path, w: u32, h: u32, f: impl Fn(u32, u32) -> u8) {
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let g = f(x, y);
            v.extend_from_slice(&[g, g, g, 255]);
        }
    }
    write_rgba_png(path, w, h, &v);
}

fn mean(img: &Image) -> f64 {
    let a = img.to_u8();
    let v = a.as_u8().unwrap();
    let ch = a.layout.channels();
    let px: Vec<f64> = v.chunks_exact(ch).map(|p| p[0] as f64).collect();
    px.iter().sum::<f64>() / px.len() as f64
}

// ------------------------------------------------------------------ manifests match the specs

#[test]
fn manifests_say_what_the_specs_say() {
    use panoply_manifest::OptionType;
    let all = panoply_image::modules::modules();
    let by = |id: &str| {
        all.iter()
            .find(|m| m.manifest.module.id == id)
            .unwrap_or_else(|| panic!("{id}"))
            .manifest
            .clone()
    };

    let convert = by("image.convert");
    assert_eq!(
        convert.module.pattern.letter(),
        'A',
        "Convert is an instant transform"
    );
    assert_eq!(convert.options["to"].default, Some(json!("jpg")));
    assert_eq!(convert.options["keep_metadata"].default, Some(json!(true)));
    assert_eq!(
        convert.options["flatten_color"].default,
        Some(json!("#ffffff")),
        "alpha flattens to white by default (IPC-8)"
    );
    assert!(
        convert.options["quality"].default.is_none(),
        "each format picks its own default quality"
    );
    assert_eq!(
        convert.options["to"].values,
        ["jpg", "png", "webp", "avif", "heic", "tiff", "bmp", "gif"]
    );

    let compress = by("image.compress");
    assert_eq!(compress.module.pattern.letter(), 'B');
    assert_eq!(
        compress.options["target"].values,
        ["smallest", "under_size", "quality"]
    );
    assert_eq!(compress.options["target"].default, Some(json!("smallest")));
    assert_eq!(
        compress.options["ssim"].default,
        Some(json!(0.97)),
        "the published bar is SSIM 0.97"
    );
    assert_eq!(
        compress.options["strip_metadata"].default,
        Some(json!(true))
    );
    assert_eq!(compress.options["format"].default, Some(json!("keep")));
    assert_eq!(compress.options["max_size"].kind, OptionType::Size);
    assert_eq!(compress.options["lossy_png"].default, Some(json!("auto")));

    let resize = by("image.resize");
    assert_eq!(resize.module.pattern.letter(), 'B');
    assert_eq!(
        resize.options["mode"].values,
        ["longest", "pixels", "percent"]
    );
    assert_eq!(
        resize.options["enlarge"].default,
        Some(json!(false)),
        "Do not enlarge is on by default"
    );
    assert_eq!(resize.options["fit"].default, Some(json!("contain")));

    let kb = by("image.resize_to_kb");
    assert_eq!(kb.module.pattern.letter(), 'B');
    for k in [
        "preset",
        "width",
        "height",
        "min_kb",
        "max_kb",
        "kb_basis",
        "fit",
        "format",
        "background",
        "baseline",
        "pad_to_min",
    ] {
        assert!(kb.options.contains_key(k), "resize_to_kb has no option {k}");
    }
    assert_eq!(kb.options["kb_basis"].values, ["both", "1000", "1024"]);
    assert_eq!(kb.options["format"].values, ["jpg", "webp", "png"]);

    let strip = by("image.strip_metadata");
    assert_eq!(strip.module.pattern.letter(), 'I');
    assert_eq!(
        strip.options["remove"].default,
        Some(json!("private")),
        "strip private is the default for sharing"
    );

    for m in [&convert, &compress, &resize, &kb, &strip] {
        assert!(
            m.surfaces.cli
                && m.surfaces.mcp
                && m.surfaces.bar
                && m.surfaces.panel
                && m.surfaces.right_click
        );
        assert!(
            !m.capabilities.network,
            "{} must not use the network",
            m.module.id
        );
        assert!(m.module.id.starts_with("image."));
    }
}

#[test]
fn the_host_registry_runs_all_five() {
    // Registered by the host so the CLI, the MCP server and the bench worker find them.
    let ids = [
        "image.convert",
        "image.compress",
        "image.resize",
        "image.resize_to_kb",
        "image.strip_metadata",
    ];
    for id in ids {
        assert!(
            panoply_image::modules::modules()
                .iter()
                .any(|m| m.manifest.module.id == id),
            "{id}"
        );
    }
}

// ------------------------------------------------------------------ resize

#[test]
fn resize_averages_in_linear_light() {
    let s = Scratch::new("linear");
    let src = s.path().join("checker.png");
    // One pixel black, one pixel white. Half the light is 188 on the sRGB scale, not 128.
    gray_png(&src, 64, 64, |x, y| if (x + y) % 2 == 0 { 0 } else { 255 });
    let (out, _) = run_ok(
        "image.resize",
        &src,
        json!({"mode": "pixels", "width": 16, "height": 16}),
        s.path(),
    );
    let m = mean(&decode_srgb(&out));
    assert!(
        (m - 188.0).abs() < 4.0,
        "mean {m:.1}: black and white must mix to about 188 in linear light, not 128"
    );
}

#[test]
fn resize_does_not_ring() {
    let s = Scratch::new("ring");
    let src = s.path().join("edge.png");
    gray_png(&src, 200, 40, |x, _| if x < 100 { 64 } else { 192 });
    let (out, _) = run_ok(
        "image.resize",
        &src,
        json!({"mode": "pixels", "width": 100, "height": 20}),
        s.path(),
    );
    let img = decode_srgb(&out);
    let v: Vec<u8> = (0..img.width).map(|x| px(&img, x, 10)[0]).collect();
    let (lo, hi) = (
        *v.iter().min().unwrap() as i32,
        *v.iter().max().unwrap() as i32,
    );
    // Overshoot is measured against the step height (128 levels): under 1 percent is 1.3 levels.
    assert!(
        lo >= 64 - 2 && hi <= 192 + 2,
        "ringing: values run from {lo} to {hi} for a 64 to 192 edge"
    );
}

#[test]
fn resize_modes_and_fits_give_the_sizes_they_promise() {
    let Some(src) = corpus("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let s = Scratch::new("modes");
    let size = |o: serde_json::Value| {
        let (p, _) = run_ok("image.resize", &src, o, s.path());
        let i = decode_srgb(&p);
        (i.width, i.height, p)
    };
    let (w0, h0, _) = size(json!({"mode": "percent", "percent": 100}));
    assert!(w0 > 0 && h0 > 0);
    let (lw, lh, p) = size(json!({"longest": 400}));
    assert_eq!(lw.max(lh), 400);
    assert!(
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .contains(&format!("({lw}x{lh})")),
        "name: {p:?}"
    );
    assert_eq!(p.extension().unwrap(), "jpg", "the format stays the same");
    let (pw, ph, _) = size(json!({"mode": "percent", "percent": 50}));
    assert_eq!(
        (pw, ph),
        (
            (w0 as f64 / 2.0).round() as u32,
            (h0 as f64 / 2.0).round() as u32
        )
    );
    let (a, b, _) = size(json!({"mode": "pixels", "width": 300}));
    assert_eq!(a, 300);
    assert!((b as f64 - 300.0 * h0 as f64 / w0 as f64).abs() <= 1.0);
    for fit in ["contain", "cover", "pad", "stretch"] {
        let (a, b, _) = size(json!({"mode": "pixels", "width": 300, "height": 300, "fit": fit}));
        match fit {
            "contain" => assert!(a == 300 && b < 300, "contain gave {a}x{b}"),
            _ => assert_eq!((a, b), (300, 300), "{fit}"),
        }
    }
    // Not enlarged by default, enlarged on request.
    let (a, _, _) = size(json!({"longest": w0.max(h0) * 2}));
    assert_eq!(a, w0.max(h0).min(a), "must not grow");
    assert!(a <= w0.max(h0));
    let (a, _, _) = size(json!({"longest": w0.max(h0) * 2, "enlarge": true}));
    assert_eq!(a, w0.max(h0) * 2);
    // Format change.
    let (_, _, p) = size(json!({"longest": 200, "format": "webp"}));
    assert_eq!(p.extension().unwrap(), "webp");
    // Missing size: a plain message.
    let e = run("image.resize", &src, json!({"mode": "percent"}), s.path())
        .err()
        .unwrap();
    assert!(e.contains("percent"), "{e}");
}

#[test]
fn resize_card_names_before_and_after() {
    let Some(src) = corpus("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let s = Scratch::new("card");
    let (_, r) = run_ok("image.resize", &src, json!({"longest": 300}), s.path());
    let line = r
        .notes
        .iter()
        .find(|l| l.contains(" to ") && l.contains(" x "))
        .expect("a size line");
    assert!(
        line.contains("smaller") || line.contains("larger"),
        "{line}"
    );
}

// ------------------------------------------------------------------ compress targets

fn len(p: &std::path::Path) -> u64 {
    std::fs::metadata(p).unwrap().len()
}

#[test]
fn under_a_size_lands_within_two_percent_below() {
    let cases = [
        "exif-orientation/landscape-1.jpg",
        "libavif/seine-sdr-gainmap.jpg",
        "libavif/dog-extended-xmp.jpg",
        "libavif/paris-exif-orientation-5.jpg",
    ];
    for rel in cases {
        let Some(src) = corpus(rel) else { continue };
        let s = Scratch::new("under");
        // A target that is reachable: 85 percent of what quality 92 gives for this picture.
        let (reference, _) = run_ok(
            "image.compress",
            &src,
            json!({"target": "quality", "quality": 92}),
            s.path(),
        );
        let target = len(&reference) * 85 / 100;
        let (out, _) = run_ok(
            "image.compress",
            &src,
            json!({"target": "under_size", "max_size": target}),
            s.path(),
        );
        let n = len(&out);
        assert!(n <= target, "{rel}: {n} bytes is over the {target} target");
        assert!(
            n as f64 >= target as f64 * 0.98,
            "{rel}: {n} bytes is more than 2% under the {target} target"
        );
    }
}

#[test]
fn an_unreachable_size_reports_the_floor() {
    let Some(src) = corpus("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let s = Scratch::new("floor");
    let e = run(
        "image.compress",
        &src,
        json!({"target": "under_size", "max_size": "1kb"}),
        s.path(),
    )
    .err()
    .unwrap();
    assert!(
        e.contains("smallest it gets") && e.contains("Make the picture smaller"),
        "{e}"
    );
    assert!(s.files().is_empty());
    let e = run(
        "image.compress",
        &src,
        json!({"target": "under_size"}),
        s.path(),
    )
    .err()
    .unwrap();
    assert!(e.contains("max_size"), "{e}");
}

/// A 2 MB photographic PNG: a smooth scene with sensor noise, far more than 5000 colors. Lossless PNG can
/// never reach 50 KB for it, so a size target has to move to a lossy format and say so.
fn photo_png(path: &std::path::Path, w: u32, h: u32) -> u64 {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 59) as i32 - 16 // -16..=15
    };
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let base = [
                110.0 + 90.0 * (fx * 3.1).sin() * (fy * 2.3).cos(),
                120.0 + 80.0 * (fy * 4.0 + fx).sin(),
                140.0 + 70.0 * ((fx + fy) * 2.7).cos(),
            ];
            for c in base {
                rgb.push((c as i32 + noise()).clamp(0, 255) as u8);
            }
        }
    }
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgb).unwrap();
    len(path)
}

#[test]
fn a_photo_saved_as_png_goes_under_a_size_as_jpg_and_says_so() {
    let s = Scratch::new("photo-png");
    let src = s.path().join("photo.png");
    let before = photo_png(&src, 1400, 1000);
    assert!(before > 1_500_000, "the fixture is only {before} bytes");
    let out = Scratch::new("photo-png-out");
    let (c, r) = run_ok(
        "image.compress",
        &src,
        json!({"target": "under_size", "max_size": "50kb"}),
        out.path(),
    );
    let n = len(&c);
    assert!(n <= 50 * 1000, "{n} bytes is over 50 KB: {:?}", r.notes);
    assert_eq!(c.extension().and_then(|e| e.to_str()), Some("jpg"), "{c:?}");
    let notes = r.notes_text();
    assert!(
        notes.contains("PNG cannot get under") && notes.contains("changed format from PNG to JPEG"),
        "the change of format must be said: {:?}",
        r.notes
    );
    assert!(src.exists(), "the original stays");
}

#[test]
fn a_photo_png_with_the_format_asked_for_stays_a_png_and_reports_the_floor() {
    let s = Scratch::new("photo-png-keep");
    let src = s.path().join("photo.png");
    photo_png(&src, 700, 500);
    let out = Scratch::new("photo-png-keep-out");
    let e = run(
        "image.compress",
        &src,
        json!({"target": "under_size", "max_size": "10kb", "format": "png"}),
        out.path(),
    )
    .err()
    .unwrap();
    assert!(
        e.contains("cannot get under 10 KB") && e.contains("smallest it gets"),
        "{e}"
    );
    assert!(out.files().is_empty());
}

#[test]
fn quality_target_and_format_shootout() {
    let Some(src) = corpus("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let s = Scratch::new("shoot");
    let (lo, _) = run_ok(
        "image.compress",
        &src,
        json!({"target": "quality", "quality": 30}),
        s.path(),
    );
    let (hi, _) = run_ok(
        "image.compress",
        &src,
        json!({"target": "quality", "quality": 90}),
        s.path(),
    );
    assert!(len(&lo) < len(&hi));
    let (best, r) = run_ok(
        "image.compress",
        &src,
        json!({"format": "smallest"}),
        s.path(),
    );
    assert!(
        r.notes_text().contains("tried"),
        "the shootout must show what it tried: {:?}",
        r.notes
    );
    assert!(["jpg", "webp", "png", "avif"].contains(&best.extension().unwrap().to_str().unwrap()));
    let (w, _) = run_ok("image.compress", &src, json!({"format": "webp"}), s.path());
    assert_eq!(w.extension().unwrap(), "webp");
}

/// The quality the card names: "..., quality 21".
fn quality_in(notes: &str) -> u32 {
    let after = notes
        .split("quality ")
        .nth(1)
        .expect("the card names a quality");
    after
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

/// UP-11: luma SSIM passes smooth, blocky results on pictures that are mostly busy or dark, and without
/// a floor these two settle at 18 and 10. The default stops at 40; a similarity asked for by name is a
/// request for a smaller file and is not held to it.
#[test]
fn the_default_target_never_goes_under_the_quality_floor() {
    for rel in ["libvips/ultra-hdr.jpg", "generated/display-p3.jpg"] {
        let Some(src) = corpus(rel) else { continue };
        let s = Scratch::new("floor");
        let (_, r) = run_ok("image.compress", &src, json!({}), s.path());
        let q = quality_in(&r.notes_text());
        assert!(q >= 40, "{rel}: the default settled at quality {q}");
        let (_, r) = run_ok("image.compress", &src, json!({"ssim": 0.9}), s.path());
        let q = quality_in(&r.notes_text());
        assert!(
            q < 40,
            "{rel}: an asked-for 0.9 should be free of the floor, got {q}"
        );
    }
}

#[test]
fn flat_art_png_gets_fewer_colors_and_photos_do_not() {
    let s = Scratch::new("lossy-png");
    let flat = s.path().join("flat.png");
    gray_png(&flat, 256, 256, |x, y| ((x / 32 + y / 32) % 4 * 60) as u8);
    let out = Scratch::new("lossy-png-out");
    let (c, r) = run_ok(
        "image.compress",
        &flat,
        json!({"strip_metadata": false}),
        out.path(),
    );
    assert!(len(&c) <= len(&flat));
    assert!(
        r.notes_text().contains("smaller") || r.notes_text().contains("already"),
        "{:?}",
        r.notes
    );
    let Some(photo) = corpus("libavif/paris-icc-exif-xmp.png") else {
        return;
    };
    let (_, r) = run_ok("image.compress", &photo, json!({}), out.path());
    assert!(
        !r.notes_text().contains("colors,"),
        "a photograph must not be reduced to a palette: {:?}",
        r.notes
    );
}

// ------------------------------------------------------------------ convert defaults

#[test]
fn convert_uses_each_formats_default_quality() {
    let Some(src) = corpus("libavif/seine-sdr-gainmap.jpg") else {
        return;
    };
    let s = Scratch::new("defq");
    let (a, _) = run_ok("image.convert", &src, json!({"to": "jpg"}), s.path());
    let (b, _) = run_ok(
        "image.convert",
        &src,
        json!({"to": "jpg", "quality": 85}),
        s.path(),
    );
    assert_eq!(file_bytes(&a), file_bytes(&b), "JPG default is quality 85");
    let (a, _) = run_ok("image.convert", &src, json!({"to": "webp"}), s.path());
    let (b, _) = run_ok(
        "image.convert",
        &src,
        json!({"to": "webp", "quality": 80}),
        s.path(),
    );
    assert_eq!(file_bytes(&a), file_bytes(&b), "WebP default is quality 80");
    // Lossless WebP is pixel exact.
    let (l, _) = run_ok(
        "image.convert",
        &src,
        json!({"to": "webp", "lossless": true}),
        s.path(),
    );
    let (want, got) = (decode_srgb(&src), decode_srgb(&l));
    assert_eq!(want.samples, got.samples, "lossless WebP changed pixels");
    // PNG is lossless too.
    let (p, _) = run_ok("image.convert", &src, json!({"to": "png"}), s.path());
    assert_eq!(decode_srgb(&p).samples, want.samples);
    let Samples::U8(_) = &want.samples else {
        panic!("8 bit expected")
    };
}

#[test]
fn bad_options_are_plain_sentences() {
    let Some(src) = corpus("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let s = Scratch::new("badopts");
    for (id, o, word) in [
        ("image.convert", json!({"to": "doc"}), "to"),
        ("image.convert", json!({"quality": 500}), "quality"),
        ("image.compress", json!({"nope": 1}), "nope"),
        (
            "image.resize_to_kb",
            json!({"preset": "no-such-preset"}),
            "ssc-photo",
        ),
        (
            "image.resize_to_kb",
            json!({"width": 200}),
            "both width and height",
        ),
        ("image.convert", json!({"flatten_color": "mauve"}), "color"),
    ] {
        let e = run(id, &src, o.clone(), s.path())
            .err()
            .unwrap_or_else(|| panic!("{id} {o} should fail"));
        assert!(e.contains(word), "{id} {o}: {e}");
        assert!(
            e.ends_with('.') || e.ends_with(')'),
            "{id}: not a sentence: {e}"
        );
    }
    assert!(s.files().is_empty());
}
