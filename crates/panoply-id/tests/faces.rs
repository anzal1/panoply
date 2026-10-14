//! The passport photo module on real portraits (public domain NASA portraits in the corpus).
//!
//! Needs a face engine: Apple Vision on macOS, YuNet elsewhere (fetched on first use).

mod common;

use common::*;
use panoply_id::face::{self, Engine, EnginePref};
use panoply_id::modules::Op;
use panoply_id::passport::{self, Status};
use panoply_id::rules;
use panoply_image::types::{Image, Samples};
use serde_json::json;
use std::path::{Path, PathBuf};

fn rgb(img: &Image) -> &[u8] {
    match &img.samples {
        Samples::U8(v) => v,
        _ => panic!("8 bit"),
    }
}

fn luma(img: &Image, x: usize, y: usize) -> f64 {
    let o = (y * img.width as usize + x) * 3;
    let v = rgb(img);
    0.2126 * f64::from(v[o]) + 0.7152 * f64::from(v[o + 1]) + 0.0722 * f64::from(v[o + 2])
}

#[test]
fn every_portrait_has_one_face_with_sane_landmarks() {
    if !engines_available() {
        return;
    }
    for name in PORTRAITS {
        let Some(p) = portrait(name) else { return };
        let img = read(&p);
        let (faces, engine) = face::detect(&img, EnginePref::Auto).unwrap();
        assert_eq!(
            faces.len(),
            1,
            "{name}: {} faces with {engine:?}",
            faces.len()
        );
        let f = &faces[0];
        assert!(f.eye_l.0 < f.eye_r.0, "{name}: eyes left to right");
        let eyes_y = (f.eye_l.1 + f.eye_r.1) / 2.0;
        assert!(
            eyes_y < f.nose.1 && f.nose.1 < f.mouth_l.1.min(f.mouth_r.1) + 2.0,
            "{name}: eyes, nose, mouth in order"
        );
        assert!(f.roll_deg().abs() < 8.0, "{name}: roll {}", f.roll_deg());
        assert!(f.confidence > 0.7, "{name}");
        eprintln!(
            "{name}: {engine:?} roll {:.1} turn {:.2} eyes {:?} mouth {:?}",
            f.roll_deg(),
            f.turn(),
            f.eye_open,
            f.mouth_open
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn the_head_proportions_the_estimate_uses_match_what_is_measured() {
    // Calibration: with the system's jaw line and subject mask, measure where the chin and the
    // top of the head sit in eye-to-mouth distances, and check the constants used when only five
    // landmarks are available (YuNet) agree.
    use panoply_id::passport::Params;
    use panoply_image::ops::FsIo;
    let rule = rules::find("US", None).unwrap();
    let (mut crowns, mut chins) = (Vec::new(), Vec::new());
    for name in PORTRAITS {
        let Some(p) = portrait(name) else { return };
        let up = panoply_id::imageio::load_upright(&FsIo::new(), &p, 4096, Some([255; 3])).unwrap();
        let pre = passport::prepare(up.image, rule, &Params::default()).unwrap();
        let pr = pre.proportions();
        eprintln!(
            "{name}: crown {:.2} chin {:.2} measured {}",
            pr.crown_per_em, pr.chin_per_em, pr.crown_measured
        );
        assert!(pr.crown_measured, "{name}: the subject mask gave no crown");
        assert!(
            (1.5..3.5).contains(&pr.crown_per_em),
            "{name}: crown {}",
            pr.crown_per_em
        );
        assert!(
            (0.6..1.4).contains(&pr.chin_per_em),
            "{name}: chin {}",
            pr.chin_per_em
        );
        crowns.push(pr.crown_per_em);
        chins.push(pr.chin_per_em);
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (mc, mh) = (mean(&crowns), mean(&chins));
    eprintln!(
        "mean crown {mc:.2} (constant {}), mean chin {mh:.2} (constant {})",
        face::CROWN_PER_EYE_MOUTH,
        face::CHIN_PER_EYE_MOUTH
    );
    assert!(
        (mc - face::CROWN_PER_EYE_MOUTH).abs() < 0.12,
        "crown constant {} vs measured {mc}",
        face::CROWN_PER_EYE_MOUTH
    );
    assert!(
        (mh - face::CHIN_PER_EYE_MOUTH).abs() < 0.08,
        "chin constant {} vs measured {mh}",
        face::CHIN_PER_EYE_MOUTH
    );
}

/// Measure the head in a finished photo without using our own geometry: the engine's chin, and
/// the top of the hair as the first row above the eyes that differs from the background.
fn measured_head_mm(img: &Image, bg: [u8; 3], height_mm: f64) -> Option<f64> {
    let (faces, _) = face::detect(img, EnginePref::Auto).ok()?;
    if faces.len() != 1 {
        return None;
    }
    let f = &faces[0];
    let chin_y = f
        .chin
        .map(|c| c.1)
        .unwrap_or_else(|| f.mouth_l.1 + (f.mouth_l.1 - f.eye_l.1) * 1.0);
    let (x0, x1) = (
        f.x as usize,
        ((f.x + f.w) as usize).min(img.width as usize - 1),
    );
    let eye_y = ((f.eye_l.1 + f.eye_r.1) / 2.0) as usize;
    let v = rgb(img);
    let differs = |x: usize, y: usize| {
        let o = (y * img.width as usize + x) * 3;
        (0..3)
            .map(|k| (i32::from(v[o + k]) - i32::from(bg[k])).abs())
            .sum::<i32>()
            > 40
    };
    let mut crown = None;
    for y in 0..eye_y {
        let n = (x0..x1).step_by(2).filter(|&x| differs(x, y)).count();
        if n * 12 >= (x1 - x0) / 2 {
            crown = Some(y);
            break;
        }
    }
    let crown = crown? as f64;
    Some((chin_y - crown) / f64::from(img.height) * height_mm)
}

#[test]
fn photos_for_every_country_hit_the_rule_to_within_a_millimetre() {
    if !engines_available() || !cfg!(target_os = "macos") {
        // The independent measurement needs the jaw line, which only Apple Vision gives.
        return;
    }
    let scratch = Scratch::new("rules");
    let mut worst: f64 = 0.0;
    for name in PORTRAITS {
        let Some(p) = portrait(name) else { return };
        // Three portraits through every rule, all seven through four of the common ones.
        let every = ["hoshide", "sunita", "fisher"].contains(&name);
        for rule in rules::all().iter().filter(|r| {
            every
                || ["us-passport", "in-passport", "uk-passport", "schengen-visa"]
                    .contains(&r.id.as_str())
        }) {
            let (out, notes) = run(
                Op::PassportPhoto,
                std::slice::from_ref(&p),
                json!({"country": rule.id, "report": true}),
                scratch.path(),
            )
            .unwrap_or_else(|e| panic!("{name} {}: {e}", rule.id));
            let jpg = out
                .iter()
                .find(|o| o.extension().is_some_and(|e| e == "jpg"))
                .unwrap();
            let img = read(jpg);
            let (w, h) = rule.pixels(None);
            assert_eq!(
                (img.width, img.height),
                (w, h),
                "{name} {}: pixel size",
                rule.id
            );
            if let Some(win) = rule.size_window(None).unwrap() {
                let n = std::fs::metadata(jpg).unwrap().len();
                assert!(
                    win.contains(n),
                    "{name} {}: {n} bytes outside {win:?}",
                    rule.id
                );
            }
            let (lo, hi) = rule.head_range_mm().unwrap();
            let mm = measured_head_mm(&img, rule.background_rgb(), rule.height_mm)
                .unwrap_or_else(|| panic!("{name} {}: could not measure the output", rule.id));
            let miss = (lo - mm).max(mm - hi).max(0.0);
            worst = worst.max(miss);
            assert!(
                miss <= 1.0,
                "{name} {}: head {mm:.2} mm, the rule asks {lo:.1} to {hi:.1} mm ({})",
                rule.id,
                notes
                    .iter()
                    .find(|n| n.contains("Head size"))
                    .cloned()
                    .unwrap_or_default()
            );
        }
    }
    eprintln!("worst miss against a head range: {worst:.2} mm");
}

#[test]
fn the_background_becomes_the_rules_colour() {
    // The corpus portraits are taken against flags, which only the system's subject mask can cut
    // out (the colour fallback says so instead, see the busy backdrop test below).
    if !cfg!(target_os = "macos") || !engines_available() {
        return;
    }
    let scratch = Scratch::new("bg");
    let Some(p) = portrait("fisher") else { return };
    for id in ["us-passport", "uk-passport"] {
        let rule = rules::find(id, None).unwrap();
        let (out, notes) = run(
            Op::PassportPhoto,
            std::slice::from_ref(&p),
            json!({"country": id}),
            scratch.path(),
        )
        .unwrap();
        let img = read(&out[0]);
        let bg = rule.background_rgb();
        // The four corners and the middle of the top edge are backdrop in a head and shoulders photo.
        for (x, y) in [
            (2usize, 2usize),
            (img.width as usize - 3, 2),
            (img.width as usize / 2, 2),
            (2, img.height as usize / 3),
        ] {
            let o = (y * img.width as usize + x) * 3;
            for (k, &want) in bg.iter().enumerate() {
                let d = (i32::from(rgb(&img)[o + k]) - i32::from(want)).abs();
                assert!(d <= 6, "{id} at ({x}, {y}) channel {k}: {d} off. {notes:?}");
            }
        }
        assert!(
            notes.iter().any(|n| n.contains("pass: Background")),
            "{id}: {notes:?}"
        );
    }
}

#[test]
fn india_lands_inside_the_portal_window_with_the_right_pixels() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("india");
    let Some(p) = portrait("jemison") else { return };
    let (out, notes) = run(
        Op::PassportPhoto,
        &[p],
        json!({"country": "IN"}),
        scratch.path(),
    )
    .unwrap();
    let img = read(&out[0]);
    assert_eq!((img.width, img.height), (630, 810));
    let n = std::fs::metadata(&out[0]).unwrap().len();
    // The Passport Seva preset: 10 to 250 KB, either reading of KB.
    assert!((10_240..=250_000).contains(&n), "{n} bytes");
    assert!(
        notes[0].contains("India passport photo, 35 x 45 mm, 630 x 810 px"),
        "{}",
        notes[0]
    );
    assert!(
        out[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("in-passport 630x810")
    );
}

#[test]
fn the_card_says_what_it_checked_and_what_it_did_not() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("card");
    let Some(p) = portrait("hoshide") else { return };
    let (_, notes) = run(
        Op::PassportPhoto,
        &[p],
        json!({"country": "US"}),
        scratch.path(),
    )
    .unwrap();
    let all = notes.join("\n");
    for needle in [
        "Head size",
        "Centred",
        "Head tilt",
        "Facing the camera",
        "Background",
        "Sharpness",
        "File",
        "not checked, please look yourself",
        "does not promise",
    ] {
        assert!(all.contains(needle), "missing {needle}: {all}");
    }
}

/// Make a rotated copy of a portrait on its own backdrop colour, as a JPEG in `dir`.
fn rotated_copy(
    src: &std::path::Path,
    degrees: f64,
    dir: &std::path::Path,
    name: &str,
) -> std::path::PathBuf {
    let img = read(src);
    let (w, h) = (f64::from(img.width), f64::from(img.height));
    let a = degrees.to_radians();
    let c = (w / 2.0, h / 2.0);
    let (s, co) = a.sin_cos();
    // Output pixel -> source: turn about the centre.
    let m = panoply_id::warp::Affine {
        a: co,
        b: -s,
        c: c.0 - co * c.0 + s * c.1,
        d: s,
        e: co,
        f: c.1 - s * c.0 - co * c.1,
    };
    let fill = rgb(&img)[0..3].to_vec();
    let (out, _) = panoply_id::warp::render(&img, &m, img.width, img.height, &fill).unwrap();
    let bytes = panoply_id::imageio::jpeg_bytes(&out, 92.0, false, 300).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn a_tilted_head_is_straightened_and_says_so() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("tilt");
    let Some(p) = portrait("thomas") else { return };
    let tilted = rotated_copy(&p, 11.0, scratch.path(), "tilted.jpg");
    let out_dir = Scratch::new("tilt-out");
    let (out, notes) = run(
        Op::PassportPhoto,
        std::slice::from_ref(&tilted),
        json!({"country": "US", "fix_tilt": true}),
        out_dir.path(),
    )
    .unwrap();
    let tilt_line = notes.iter().find(|n| n.contains("Head tilt")).unwrap();
    assert!(
        tilt_line.contains("pass") && tilt_line.contains("(fixed)"),
        "{tilt_line}"
    );
    let img = read(&out[0]);
    let (faces, _) = face::detect(&img, EnginePref::Auto).unwrap();
    assert_eq!(faces.len(), 1);
    // Apple Vision reads the eye line to within a degree or two. YuNet, the engine everywhere
    // else, reads it less exactly, and the reading depends on the CPU the model runs on: the
    // same photo came back as tilted 11 degrees on one CI machine and 4.4 on another, so the
    // straightened picture re-reads anywhere from level to 7 degrees off. Off macOS the check is
    // that the picture came out clearly closer to level than the 11 degrees it went in with.
    let limit = if cfg!(target_os = "macos") { 2.0 } else { 8.0 };
    assert!(
        faces[0].roll_deg().abs() < limit,
        "still tilted {:.1} degrees (limit {limit}); the first pass said: {tilt_line}",
        faces[0].roll_deg()
    );
    // Without the fix the photo is reported as it is: 11 degrees is past a 5 degree rule.
    let (_, notes) = run(
        Op::PassportPhoto,
        &[tilted],
        json!({"country": "US", "fix_tilt": false}),
        out_dir.path(),
    )
    .unwrap();
    let tilt_line = notes.iter().find(|n| n.contains("Head tilt")).unwrap();
    // On a CPU where YuNet reads only 4.4 of the 11 degrees the line is a warning, not a failure.
    let status = if cfg!(target_os = "macos") {
        "tilted.jpg: fail"
    } else {
        "tilted.jpg: "
    };
    assert!(tilt_line.starts_with(status), "{tilt_line}");
    assert!(!tilt_line.starts_with("tilted.jpg: pass"), "{tilt_line}");
    assert!(tilt_line.contains("can be fixed"), "{tilt_line}");
}

#[test]
fn a_dark_photo_is_brought_back() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("dark");
    let Some(p) = portrait("antonelli") else {
        return;
    };
    let img = read(&p);
    let mut dark = img.clone();
    if let Samples::U8(v) = &mut dark.samples {
        for p in v.iter_mut() {
            *p = ((f64::from(*p) / 255.0).powf(6.0) * 255.0) as u8;
        }
    }
    let path = scratch.path().join("dark.jpg");
    std::fs::write(
        &path,
        panoply_id::imageio::jpeg_bytes(&dark, 92.0, false, 300).unwrap(),
    )
    .unwrap();
    let out_dir = Scratch::new("dark-out");
    let (out, notes) = run(
        Op::PassportPhoto,
        std::slice::from_ref(&path),
        json!({"country": "US"}),
        out_dir.path(),
    )
    .unwrap();
    let line = notes.iter().find(|n| n.contains("Lighting")).unwrap();
    assert!(
        line.contains("(fixed)") && line.contains("too dark"),
        "{line}"
    );
    let img = read(&out[0]);
    let m = luma(&img, img.width as usize / 2, img.height as usize / 2);
    assert!(m > 60.0, "centre of the face is still {m}");
    let (_, notes) = run(
        Op::PassportPhoto,
        &[path],
        json!({"country": "US", "fix_exposure": false}),
        out_dir.path(),
    )
    .unwrap();
    let line = notes.iter().find(|n| n.contains("Lighting")).unwrap();
    assert!(
        line.contains("fail") && line.contains("too dark") && line.contains("can be fixed"),
        "{line}"
    );
}

/// The face engines to run a sharpness check on here: Apple Vision and YuNet on macOS (YuNet is
/// forced through the engine option), the one fallback engine elsewhere. Each entry is the value
/// of the `engine` option. A YuNet that cannot be fetched is skipped, as everywhere else.
fn engine_options() -> Vec<&'static str> {
    if !cfg!(target_os = "macos") {
        return if engines_available() {
            vec!["auto"]
        } else {
            vec![]
        };
    }
    let yunet = panoply_id::runtime::ensure_onnxruntime().is_ok()
        && panoply_id::runtime::ensure_model().is_ok();
    if !yunet && std::env::var_os("PANOPLY_REQUIRE_ENGINES").is_some() {
        panic!("YuNet could not be fetched and PANOPLY_REQUIRE_ENGINES is set");
    }
    if yunet {
        vec!["vision", "yunet"]
    } else {
        eprintln!("skipping the YuNet half: the YuNet engine could not be fetched here");
        vec!["vision"]
    }
}

/// The score on the Sharpness line of a passport run on `input` with `engine`, and the line.
fn sharpness_of(input: &Path, engine: &str, scratch: &Path) -> (f64, String) {
    let (_, notes) = run(
        Op::PassportPhoto,
        &[input.to_path_buf()],
        json!({"country": "US", "engine": engine}),
        scratch,
    )
    .unwrap();
    let line = notes
        .iter()
        .find(|n| n.contains("Sharpness"))
        .unwrap()
        .clone();
    let n: f64 = line
        .split("scores ")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    (n, line)
}

/// A heavy blur of a portrait: box filter of radius 14 px on the 2400 x 3000 original, saved as
/// a JPEG in `dir`.
fn blurred_copy(p: &Path, dir: &Path) -> PathBuf {
    let img = read(p);
    let (w, h) = (img.width as usize, img.height as usize);
    let mut data: Vec<f32> = rgb(&img).iter().map(|&v| f32::from(v)).collect();
    let r = 14usize;
    for _ in 0..2 {
        let mut tmp = data.clone();
        for y in 0..h {
            for x in 0..w {
                let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
                for k in 0..3 {
                    let s: f32 = (a..=b).map(|xx| data[(y * w + xx) * 3 + k]).sum();
                    tmp[(y * w + x) * 3 + k] = s / (b - a + 1) as f32;
                }
            }
        }
        for y in 0..h {
            let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
            for x in 0..w {
                for k in 0..3 {
                    let s: f32 = (a..=b).map(|yy| tmp[(yy * w + x) * 3 + k]).sum();
                    data[(y * w + x) * 3 + k] = s / (b - a + 1) as f32;
                }
            }
        }
    }
    let mut blurred = img.clone();
    blurred.samples = Samples::U8(data.iter().map(|&v| v as u8).collect());
    let path = dir.join("blurred.jpg");
    std::fs::write(
        &path,
        panoply_id::imageio::jpeg_bytes(&blurred, 92.0, false, 300).unwrap(),
    )
    .unwrap();
    path
}

#[test]
fn a_blurred_photo_fails_sharpness_and_a_sharp_one_passes() {
    let engines = engine_options();
    if engines.is_empty() {
        return;
    }
    let scratch = Scratch::new("blur");
    let Some(p) = portrait("hoshide") else { return };
    let blurred = blurred_copy(&p, scratch.path());
    for engine in engines {
        let (_, sharp) = sharpness_of(&p, engine, scratch.path());
        assert!(sharp.contains("pass: Sharpness"), "{engine}: {sharp}");
        let (_, line) = sharpness_of(&blurred, engine, scratch.path());
        eprintln!("{engine}: blurred copy: {line}");
        assert!(line.contains("fail: Sharpness"), "{engine}: {line}");
    }
}

#[test]
fn a_photo_too_small_for_the_frame_is_refused_with_the_reason() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("small");
    let Some(p) = portrait("hoshide") else { return };
    let img = read(&p);
    let small = panoply_image::ops::kb::resample_image(&img, 200, 250).unwrap();
    let path = scratch.path().join("tiny.jpg");
    std::fs::write(
        &path,
        panoply_id::imageio::jpeg_bytes(&small, 90.0, false, 72).unwrap(),
    )
    .unwrap();
    let e = run(
        Op::PassportPhoto,
        &[path],
        json!({"country": "IN"}),
        scratch.path(),
    )
    .err()
    .unwrap();
    assert!(e.contains("too small") && e.contains("pixels"), "{e}");
}

#[test]
fn two_faces_ask_which_and_a_choice_works() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("two");
    let (Some(a), Some(b)) = (portrait("antonelli"), portrait("thomas")) else {
        return;
    };
    let (ia, ib) = (read(&a), read(&b));
    let fit = |im: &Image| panoply_image::ops::kb::resample_image(im, 1000, 1250).unwrap();
    let (fa, fb) = (fit(&ia), fit(&ib));
    let (sa, sb) = (rgb(&fa), rgb(&fb));
    let mut v = Vec::new();
    for y in 0..1250usize {
        v.extend_from_slice(&sa[y * 3000..(y + 1) * 3000]);
        v.extend_from_slice(&sb[y * 3000..(y + 1) * 3000]);
    }
    let both = Image {
        width: 2000,
        height: 1250,
        layout: ia.layout,
        samples: Samples::U8(v),
        icc: None,
    };
    let path = scratch.path().join("two.jpg");
    std::fs::write(
        &path,
        panoply_id::imageio::jpeg_bytes(&both, 92.0, false, 300).unwrap(),
    )
    .unwrap();
    let e = run(
        Op::PassportPhoto,
        std::slice::from_ref(&path),
        json!({"country": "US"}),
        scratch.path(),
    )
    .err()
    .unwrap();
    assert!(e.contains("2 faces") && e.contains("which one"), "{e}");
    let (out, _) = run(
        Op::PassportPhoto,
        std::slice::from_ref(&path),
        json!({"country": "US", "face": 2}),
        scratch.path(),
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    let e = run(
        Op::PassportPhoto,
        &[path],
        json!({"country": "US", "face": 3}),
        scratch.path(),
    )
    .err()
    .unwrap();
    assert!(e.contains("1 to 2"), "{e}");
}

#[test]
fn a_picture_with_no_face_says_so() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("noface");
    let (w, h) = (400u32, 300u32);
    let v: Vec<u8> = (0..w * h)
        .flat_map(|i| [(i % 251) as u8 / 3, 90, 140])
        .collect();
    let img = Image {
        width: w,
        height: h,
        layout: panoply_image::types::Layout::Rgb,
        samples: Samples::U8(v),
        icc: None,
    };
    let path = scratch.path().join("wall.jpg");
    std::fs::write(
        &path,
        panoply_id::imageio::jpeg_bytes(&img, 90.0, false, 72).unwrap(),
    )
    .unwrap();
    let e = run(
        Op::PassportPhoto,
        &[path],
        json!({"country": "US"}),
        scratch.path(),
    )
    .err()
    .unwrap();
    assert!(e.contains("No face was found"), "{e}");
}

#[test]
fn strict_writes_nothing_when_a_check_fails() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("strict");
    let Some(p) = portrait("thomas") else { return };
    let tilted = rotated_copy(&p, 14.0, scratch.path(), "t.jpg");
    let out = Scratch::new("strict-out");
    let e = run(
        Op::PassportPhoto,
        &[tilted],
        json!({"country": "US", "fix_tilt": false, "strict": true}),
        out.path(),
    )
    .err()
    .unwrap();
    assert!(
        e.contains("Nothing was written") && e.contains("Head tilt"),
        "{e}"
    );
    assert!(out.files().is_empty(), "{:?}", out.files());
}

#[test]
fn a_sheet_comes_straight_from_the_passport_run() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("sheet");
    let Some(p) = portrait("fisher") else { return };
    let (out, notes) = run(
        Op::PassportPhoto,
        &[p],
        json!({"country": "IN", "output": "both", "paper": "4x6"}),
        scratch.path(),
    )
    .unwrap();
    assert_eq!(out.len(), 2, "{out:?}");
    let sheet = out
        .iter()
        .find(|o| o.file_name().unwrap().to_string_lossy().contains("sheet"))
        .unwrap();
    let img = read(sheet);
    // 4 x 6 in at 300 DPI, fed landscape (that fits eight 35 x 45 mm photos).
    assert_eq!(
        (img.width.max(img.height), img.width.min(img.height)),
        (1800, 1200)
    );
    assert!(
        notes
            .iter()
            .any(|n| n.contains("8 photos, 35 x 45 mm each, 4 x 6 in, 300 DPI")),
        "{notes:?}"
    );
    let bytes = std::fs::read(sheet).unwrap();
    assert_eq!(panoply_id::imageio::jfif_dpi(&bytes), Some(300.0));
}

#[test]
fn the_yunet_fallback_agrees_with_the_os_engine() {
    // The same portraits through the fallback engine. Where the OS engine exists, the two must
    // agree on where the eyes are; the fallback must find exactly one face either way.
    if !engines_available() {
        return;
    }
    let (Some(_), true) = (
        portrait("hoshide"),
        panoply_id::runtime::ensure_onnxruntime().is_ok()
            && panoply_id::runtime::ensure_model().is_ok(),
    ) else {
        eprintln!("skipping: the YuNet engine could not be fetched here");
        return;
    };
    for name in PORTRAITS {
        let Some(p) = portrait(name) else { return };
        let img = read(&p);
        let (yn, e) = face::detect(&img, EnginePref::YuNet).unwrap();
        assert_eq!(e, Engine::YuNet);
        assert_eq!(yn.len(), 1, "{name}: YuNet found {} faces", yn.len());
        #[cfg(target_os = "macos")]
        {
            let (vs, _) = face::detect(&img, EnginePref::Vision).unwrap();
            let (a, b) = (&yn[0], &vs[0]);
            let tol = 0.25 * b.eye_dist();
            for (n, (p, q)) in [
                ("left eye", (a.eye_l, b.eye_l)),
                ("right eye", (a.eye_r, b.eye_r)),
                ("nose", (a.nose, b.nose)),
            ] {
                let d = face::dist(p, q);
                assert!(d < tol, "{name}: {n} differs by {d:.1} px (limit {tol:.1})");
            }
        }
    }
}

#[test]
fn the_fallback_makes_a_whole_photo_on_a_plain_backdrop() {
    // YuNet for the face and the plain-backdrop colour cut for the person, no OS engine. The
    // fixture is a portrait placed on a flat studio gradient (made with the system's subject
    // mask, so this runs where Apple Vision exists; elsewhere the fixture cannot be made).
    if !cfg!(target_os = "macos") || !engines_available() {
        return;
    }
    if panoply_id::runtime::ensure_onnxruntime().is_err()
        || panoply_id::runtime::ensure_model().is_err()
    {
        eprintln!("skipping: the YuNet engine could not be fetched here");
        return;
    }
    let scratch = Scratch::new("fallback");
    let Some(p) = portrait("fisher") else { return };
    let img = read(&p);
    let face = &face::detect(&img, EnginePref::Vision).unwrap().0[0];
    let matte = panoply_id::segment::compute(&img, face, EnginePref::Vision).unwrap();
    assert_eq!(matte.engine, panoply_id::segment::MatteEngine::Vision);
    let (w, h) = (img.width as usize, img.height as usize);
    let src = rgb(&img);
    let mut v = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let a = f64::from(matte.at(x as f64, y as f64)) / 255.0;
            let t = y as f64 / h as f64;
            let bg = [60.0 + 40.0 * t, 110.0 + 30.0 * t, 190.0 - 30.0 * t];
            for k in 0..3 {
                v[(y * w + x) * 3 + k] =
                    (f64::from(src[(y * w + x) * 3 + k]) * a + bg[k] * (1.0 - a)) as u8;
            }
        }
    }
    let fixture = Image {
        width: w as u32,
        height: h as u32,
        layout: img.layout,
        samples: Samples::U8(v),
        icc: None,
    };
    let path = scratch.path().join("plain.jpg");
    std::fs::write(
        &path,
        panoply_id::imageio::jpeg_bytes(&fixture, 93.0, false, 300).unwrap(),
    )
    .unwrap();
    let (out, notes) = run(
        Op::PassportPhoto,
        &[path],
        json!({"country": "US", "engine": "yunet", "cutout": "color"}),
        scratch.path(),
    )
    .unwrap();
    let all = notes.join("\n");
    assert!(
        !all.contains("Apple Vision"),
        "the OS engine must not have been used: {all}"
    );
    assert!(
        all.contains("YuNet") && all.contains("plain backdrop colour cut"),
        "{all}"
    );
    assert!(all.contains("pass: Background"), "{all}");
    let img = read(&out[0]);
    assert_eq!((img.width, img.height), (600, 600));
    for (x, y) in [(2usize, 2usize), (597, 2), (300, 2)] {
        let o = (y * 600 + x) * 3;
        for k in 0..3 {
            assert!(
                rgb(&img)[o + k] >= 240,
                "backdrop at ({x}, {y}) is not white"
            );
        }
    }
    // The head is where the rule wants it, found independently.
    let rule = rules::find("US", None).unwrap();
    let mm = measured_head_mm(&img, [255; 3], rule.height_mm).unwrap();
    let (lo, hi) = rule.head_range_mm().unwrap();
    assert!(
        mm >= lo - 1.5 && mm <= hi + 1.5,
        "head {mm:.1} mm outside {lo:.1} to {hi:.1}"
    );
}

#[test]
fn a_busy_backdrop_without_the_subject_mask_is_reported_not_painted_over() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("busy");
    let Some(p) = portrait("hoshide") else { return };
    // Two flags behind the head: the colour cut cannot separate that, and must say so.
    let (_, notes) = run(
        Op::PassportPhoto,
        &[p],
        json!({"country": "US", "cutout": "color"}),
        scratch.path(),
    )
    .unwrap();
    let line = notes.iter().find(|n| n.contains("Background")).unwrap();
    assert!(
        line.contains("fail: Background") && line.contains("too busy"),
        "{line}"
    );
}

#[test]
fn status_words_are_stable() {
    assert_eq!(Status::Pass.word(), "pass");
    assert_eq!(Status::Warn.word(), "warn");
    assert_eq!(Status::Fail.word(), "fail");
}

#[test]
fn sharpness_numbers_separate_real_portraits_from_a_blur() {
    // The thresholds in the module are set from these numbers; print them so a change to the
    // check shows up here. Every engine has to put every sharp portrait above the pass mark and
    // the blurred copy below the warn mark: the number is read on a region built from the eyes
    // and mouth, so it must not depend on which engine found the face.
    let engines = engine_options();
    if engines.is_empty() {
        return;
    }
    let scratch = Scratch::new("sharp-numbers");
    for engine in engines {
        let mut seen = Vec::new();
        for name in PORTRAITS {
            let Some(p) = portrait(name) else { return };
            let (n, line) = sharpness_of(&p, engine, scratch.path());
            eprintln!("{engine}: {name}: sharpness {n}");
            seen.push(n);
            assert!(n >= passport::SHARP_PASS, "{engine}: {name}: {line}");
        }
        assert_eq!(seen.len(), 7);
        let Some(p) = portrait("hoshide") else { return };
        let (n, line) = sharpness_of(&blurred_copy(&p, scratch.path()), engine, scratch.path());
        eprintln!("{engine}: blurred hoshide: sharpness {n}");
        assert!(n < passport::SHARP_WARN, "{engine}: {line}");
    }
}

#[test]
fn the_fallback_model_unloads_when_idle() {
    if !engines_available() {
        return;
    }
    if panoply_id::runtime::ensure_onnxruntime().is_err()
        || panoply_id::runtime::ensure_model().is_err()
    {
        eprintln!("skipping: the YuNet engine could not be fetched here");
        return;
    }
    let Some(p) = portrait("hoshide") else { return };
    // This test owns the process-wide session: nothing else in this binary may run YuNet at the
    // same moment, so it only checks "loaded after use, gone after the idle time" with a short idle.
    panoply_id::runtime::set_idle_unload(std::time::Duration::from_millis(800));
    let img = read(&p);
    let (faces, e) = face::detect(&img, EnginePref::YuNet).unwrap();
    assert_eq!((faces.len(), e), (1, Engine::YuNet));
    let t = std::time::Instant::now();
    while panoply_id::runtime::session_loaded() && t.elapsed() < std::time::Duration::from_secs(10)
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panoply_id::runtime::set_idle_unload(std::time::Duration::from_secs(60));
    assert!(
        !panoply_id::runtime::session_loaded(),
        "the session was still loaded after the idle time"
    );
}

#[test]
fn the_json_report_carries_every_check_with_its_number() {
    if !engines_available() {
        return;
    }
    let scratch = Scratch::new("json");
    let Some(p) = portrait("fisher") else { return };
    let (out, _) = run(
        Op::PassportPhoto,
        &[p],
        json!({"country": "GB", "report": true}),
        scratch.path(),
    )
    .unwrap();
    let report = out
        .iter()
        .find(|o| o.extension().is_some_and(|e| e == "json"))
        .expect("a report file");
    assert!(
        report
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("uk-passport check")
    );
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
    assert_eq!(v["rule"], "uk-passport");
    assert_eq!(
        (v["width_px"].as_u64(), v["height_px"].as_u64()),
        (Some(600), Some(750))
    );
    let ids: Vec<&str> = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    for want in [
        "face",
        "head_size",
        "centered",
        "tilt",
        "pose",
        "resolution",
        "framing",
        "lighting",
        "background",
        "sharpness",
        "color",
        "file",
    ] {
        assert!(ids.contains(&want), "{want} is missing from {ids:?}");
    }
    let head = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "head_size")
        .unwrap();
    assert_eq!(head["status"], "pass");
    assert!(
        head["detail"].as_str().unwrap().contains("29.0 to 34.0 mm"),
        "{head}"
    );
    assert!(v["head_mm"].as_f64().unwrap() > 29.0);
    assert!(
        v["unchecked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|u| u.as_str().unwrap().contains("glasses"))
    );
}

#[test]
fn a_photo_with_the_top_of_the_head_cut_off_is_flagged() {
    // Where the top of the head is estimated (no subject mask) the cut-off point is only as good
    // as the estimate, so the exact wording is checked where it is measured.
    if !cfg!(target_os = "macos") || !engines_available() {
        return;
    }
    let scratch = Scratch::new("cut");
    let Some(p) = portrait("hoshide") else { return };
    let img = read(&p);
    // Take the top 17 percent away: the crown sits near 12 percent from the top.
    let cut = (f64::from(img.height) * 0.17) as u32;
    let cropped = img.crop(0, cut, img.width, img.height - cut);
    let path = scratch.path().join("cut.jpg");
    std::fs::write(
        &path,
        panoply_id::imageio::jpeg_bytes(&cropped, 92.0, false, 300).unwrap(),
    )
    .unwrap();
    let (_, notes) = run(
        Op::PassportPhoto,
        &[path],
        json!({"country": "US"}),
        scratch.path(),
    )
    .unwrap();
    let line = notes.iter().find(|n| n.contains("Framing")).unwrap();
    assert!(
        line.contains("fail: Framing") && line.contains("top of the head is outside"),
        "{line}"
    );
}
