//! Resize to exact KB (docs/specs/id-and-image.md 2.3 and 2.4): exact pixels, a size inside the
//! window under the stated reading of KB, the card, the name, padding as the last step.

mod common;

use common::*;
use panoply_image::meta;
use panoply_image::presets;
use serde_json::json;

fn has_marker(bytes: &[u8], m: u8) -> bool {
    bytes.windows(2).any(|w| w == [0xFF, m])
}

fn photo(rel: &str) -> Option<std::path::PathBuf> {
    corpus(rel)
}

#[test]
fn exact_pixels_inside_the_window_with_the_spec_card_and_name() {
    let Some(src) = photo("exif-orientation/landscape-1.jpg") else {
        return;
    };
    let s = Scratch::new("kb-card");
    let (out, r) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 200, "height": 230, "min_kb": 20, "max_kb": 50}),
        s.path(),
    );
    let img = decode_srgb(&out);
    assert_eq!((img.width, img.height), (200, 230));
    let n = std::fs::metadata(&out).unwrap().len();
    // Unknown reading of KB: inside both, so at least 20 x 1024 and at most 50 x 1000.
    assert!((20_480..=50_000).contains(&n), "{n} bytes");
    let name = out.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("landscape-1 (200x230, ") && name.ends_with("KB).jpg"),
        "{name}"
    );
    let card = r
        .notes
        .iter()
        .find(|l| l.contains("bytes ("))
        .expect("the card line");
    assert!(card.contains("200 x 230 px"), "{card}");
    assert!(
        card.contains(" KB or ") && card.contains(" KB)"),
        "both readings are printed: {card}"
    );
    assert!(
        card.contains("inside 20 to 50 KB under both readings"),
        "{card}"
    );
    let digits: String = n.to_string();
    let with_commas = format!(
        "{},{}",
        &digits[..digits.len() - 3],
        &digits[digits.len() - 3..]
    );
    assert!(
        card.contains(&with_commas),
        "exact bytes with a thousands separator: {card}"
    );
}

#[test]
fn both_readings_of_kb_are_honoured_separately() {
    let Some(src) = photo("libavif/seine-sdr-gainmap.jpg") else {
        return;
    };
    let s = Scratch::new("kb-basis");
    for (basis, lo, hi) in [
        ("1000", 20_000u64, 30_000u64),
        ("1024", 20_480, 30_720),
        ("both", 20_480, 30_000),
    ] {
        let (out, _) = run_ok(
            "image.resize_to_kb",
            &src,
            json!({"width": 300, "height": 200, "min_kb": 20, "max_kb": 30, "kb_basis": basis}),
            s.path(),
        );
        let n = std::fs::metadata(&out).unwrap().len();
        assert!(
            (lo..=hi).contains(&n),
            "basis {basis}: {n} is outside {lo} to {hi}"
        );
    }
    // An empty intersection is a conflict the person has to settle, not a silent guess.
    let e = run(
        "image.resize_to_kb",
        &src,
        json!({"width": 300, "height": 200, "min_kb": 48, "max_kb": 49}),
        s.path(),
    )
    .err()
    .unwrap();
    assert!(
        e.contains("no size is inside both") && e.contains("kb_basis"),
        "{e}"
    );
}

#[test]
fn presets_set_everything_and_explicit_options_override() {
    let Some(src) = photo("libavif/dog-extended-xmp.jpg") else {
        return;
    };
    let s = Scratch::new("kb-preset");
    let (out, r) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"preset": "ssc-signature"}),
        s.path(),
    );
    let img = decode_srgb(&out);
    let p = presets::find("ssc-signature").unwrap();
    assert_eq!(Some((img.width, img.height)), p.pixels());
    let n = std::fs::metadata(&out).unwrap().len();
    assert!((10_240..=20_000).contains(&n), "{n}");
    let text = r.notes_text();
    assert!(
        text.contains("preset ssc-signature") && text.contains(&p.source_url),
        "the card must show where the rule came from: {text}"
    );
    assert!(
        text.contains("does not promise the portal accepts"),
        "{text}"
    );
    // Pixels given by hand win over the preset.
    let (out, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"preset": "ssc-signature", "width": 100, "height": 40}),
        s.path(),
    );
    let img = decode_srgb(&out);
    assert_eq!((img.width, img.height), (100, 40));
    // Presets are baseline JPEG with no metadata; without one the JPEG is progressive.
    let bytes = file_bytes(&out);
    assert!(
        has_marker(&bytes, 0xC0) && !has_marker(&bytes, 0xC2),
        "a preset must give baseline JPEG"
    );
    let (free, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 300, "height": 200, "max_kb": 40}),
        s.path(),
    );
    assert!(
        has_marker(&file_bytes(&free), 0xC2),
        "no preset: progressive"
    );
    let (forced, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 300, "height": 200, "max_kb": 40, "baseline": true}),
        s.path(),
    );
    assert!(has_marker(&file_bytes(&forced), 0xC0));
}

#[test]
fn portal_files_carry_no_metadata_and_no_color_profile() {
    let Some(src) = corpus("generated/exif-gps-orientation6.jpg") else {
        return;
    };
    let s = Scratch::new("kb-meta");
    let (out, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"preset": "ssc-signature"}),
        s.path(),
    );
    let r = meta::inspect(&file_bytes(&out)).unwrap();
    assert!(
        !r.exif_present && r.gps.is_none() && !r.icc && !r.xmp,
        "{r:?}"
    );
    if let Some(p3) = corpus("generated/display-p3.jpg") {
        let (out, _) = run_ok(
            "image.resize_to_kb",
            &p3,
            json!({"preset": "ssc-signature"}),
            s.path(),
        );
        let r = meta::inspect(&file_bytes(&out)).unwrap();
        assert!(!r.icc, "the profile must be gone");
    }
}

#[test]
fn a_minimum_the_pixels_cannot_reach_is_padded_and_the_card_says_so() {
    let Some(src) = corpus("libjpeg-turbo/testorig.jpg") else {
        return;
    };
    let s = Scratch::new("kb-pad");
    let (out, r) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"preset": "rrb-signature"}),
        s.path(),
    );
    let n = std::fs::metadata(&out).unwrap().len();
    assert!((30_720..=49_000).contains(&n), "{n}");
    assert!(
        r.notes_text().contains("comment padding"),
        "padding must be named: {:?}",
        r.notes
    );
    assert!(decode_srgb(&out).width == 140);
    let e = run(
        "image.resize_to_kb",
        &src,
        json!({"preset": "rrb-signature", "pad_to_min": false}),
        s.path(),
    )
    .err()
    .unwrap();
    assert!(e.contains("pad_to_min") && e.contains("minimum"), "{e}");
}

#[test]
fn a_maximum_the_picture_cannot_meet_at_exact_pixels_says_so() {
    let Some(src) = corpus("libavif/dog-extended-xmp.jpg") else {
        return;
    };
    let s = Scratch::new("kb-impossible");
    let e = run(
        "image.resize_to_kb",
        &src,
        json!({"width": 2000, "height": 2000, "max_kb": 5}),
        s.path(),
    )
    .err()
    .unwrap();
    assert!(
        e.contains("smallest this picture gets") && e.contains("both"),
        "{e}"
    );
    assert!(s.files().is_empty());
}

#[test]
fn free_size_mode_shrinks_only_as_far_as_it_must() {
    let Some(src) = corpus("commons/an-teallach-panorama.jpg") else {
        return;
    };
    let s = Scratch::new("kb-free");
    let (out, r) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"max_kb": 30, "kb_basis": "1000"}),
        s.path(),
    );
    let n = std::fs::metadata(&out).unwrap().len();
    assert!(n <= 30_000, "{n}");
    let img = decode_srgb(&out);
    assert!(
        img.width < 10179 && img.width > 300,
        "{}x{}",
        img.width,
        img.height
    );
    assert!(
        (img.width as f64 / img.height as f64 - 10179.0 / 1400.0).abs() < 0.2,
        "the shape must be kept"
    );
    assert!(r.notes_text().contains("reduced from"), "{:?}", r.notes);
}

#[test]
fn transparent_pictures_get_the_background() {
    let s = Scratch::new("kb-alpha");
    let src = s.path().join("t.png");
    let (w, h) = (80u32, 80u32);
    let mut v = Vec::new();
    for _ in 0..h * w {
        v.extend_from_slice(&[0, 0, 0, 0]);
    }
    write_rgba_png(&src, w, h, &v);
    let out = Scratch::new("kb-alpha-out");
    let (o, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 60, "height": 60, "max_kb": 10}),
        out.path(),
    );
    let p = px(&decode_srgb(&o), 30, 30);
    assert!(
        p.iter().all(|&c| c >= 250),
        "transparent area must be white: {p:?}"
    );
    let (o, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 60, "height": 60, "max_kb": 10, "background": "#0000ff"}),
        out.path(),
    );
    let p = px(&decode_srgb(&o), 30, 30);
    assert!(p[2] > 240 && p[0] < 20, "{p:?}");
}

#[test]
fn png_and_webp_targets_fit_too() {
    let Some(src) = corpus("libavif/paris-icc-exif-xmp.png") else {
        return;
    };
    let s = Scratch::new("kb-formats");
    let (w, _) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 200, "height": 200, "max_kb": 20, "kb_basis": "1000", "format": "webp"}),
        s.path(),
    );
    assert!(std::fs::metadata(&w).unwrap().len() <= 20_000);
    assert_eq!(w.extension().unwrap(), "webp");
    let (p, r) = run_ok(
        "image.resize_to_kb",
        &src,
        json!({"width": 200, "height": 200, "max_kb": 20, "kb_basis": "1000", "format": "png"}),
        s.path(),
    );
    assert!(
        std::fs::metadata(&p).unwrap().len() <= 20_000,
        "{:?}",
        r.notes
    );
    let e = run(
        "image.resize_to_kb",
        &src,
        json!({"width": 200, "height": 200, "max_kb": 20, "format": "gif"}),
        s.path(),
    )
    .err()
    .unwrap();
    assert!(
        e.contains("not right") || e.contains("must be one of"),
        "{e}"
    );
}

#[test]
fn every_preset_lands_in_its_window_under_both_readings() {
    // The full proof (20 photos) is the kb_proof example; this keeps three photos in the test run.
    let photos: Vec<_> = [
        "exif-orientation/landscape-3.jpg",
        "libavif/seine-sdr-gainmap.jpg",
        "libjpeg-turbo/testorig.jpg",
    ]
    .iter()
    .filter_map(|r| corpus(r))
    .collect();
    let s = Scratch::new("kb-all");
    let mut checked = 0;
    for p in presets::all().iter().filter(|p| p.has_size_rule()) {
        for basis in ["both", "1000", "1024"] {
            let (lo, hi) = match basis {
                "1000" => (p.min_bytes_claimed * 1000.0, p.max_bytes_claimed * 1000.0),
                "1024" => (p.min_bytes_claimed * 1024.0, p.max_bytes_claimed * 1024.0),
                _ => (p.min_bytes_claimed * 1024.0, p.max_bytes_claimed * 1000.0),
            };
            for photo in &photos {
                let (out, _) = run_ok(
                    "image.resize_to_kb",
                    photo,
                    json!({"preset": p.id, "kb_basis": basis}),
                    s.path(),
                );
                let n = std::fs::metadata(&out).unwrap().len() as f64;
                assert!(
                    n >= lo && n <= hi,
                    "{} [{basis}] on {}: {n} bytes is outside {lo} to {hi}",
                    p.id,
                    photo.display()
                );
                std::fs::remove_file(out).unwrap();
                checked += 1;
            }
        }
    }
    assert!(
        checked >= 15 * 3 || photos.is_empty(),
        "only {checked} cases ran"
    );
}
