//! The searchable PDF writer: text layer, untouched pictures, skipped text pages.

mod common;

use std::collections::HashMap;

use common::*;
use panoply_core::CancelToken;
use panoply_ocr::searchable::PageStatus;
use panoply_ocr::{EngineKind, Ocr};
use panoply_pdf::obj::Obj;
use panoply_pdf::source::Source;
use sha2::{Digest, Sha256};

/// SHA-256 of the decoded bytes of every image XObject, by dimensions.
fn image_hashes(pdf: &[u8]) -> HashMap<(i64, i64), String> {
    let doc = panoply_pdf::Doc::open(Source::from_bytes(pdf.to_vec())).unwrap();
    let mut m = HashMap::new();
    for n in doc.used_objects() {
        if let Ok(Some((d, data))) = doc.stream_data(n)
            && d.name("Subtype") == Some(b"Image")
        {
            let key = (d.int("Width").unwrap(), d.int("Height").unwrap());
            m.insert(
                key,
                Sha256::digest(&data)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            );
        }
    }
    m
}

fn page_text(pdf: &[u8], index: i32) -> String {
    let engine = panoply_pdf::pdfium::engine().unwrap();
    let _g = panoply_pdf::pdfium::lock();
    let doc = engine.load_pdf_from_byte_slice(pdf, None).unwrap();
    let page = doc.pages().get(index).unwrap();
    // Not `all()`: it asks pdfium for the text inside the page rectangle, which on a page turned
    // by /Rotate compares unrotated positions with the rotated size and drops real characters.
    let text = page.text().unwrap();
    let chars = text.chars();
    chars.iter().filter_map(|c| c.unicode_char()).collect()
}

#[test]
fn a_scan_gets_a_text_layer_and_its_picture_is_untouched() {
    let (path, file) = picture_pdf(&labelled("en-1.png"), "layer");
    let original = std::fs::read(&path).unwrap();
    let Some((report, out)) = run_searchable(&file, &Ocr::new(), "en") else {
        return;
    };
    assert_eq!(report.pages, 1);
    assert_eq!(report.read, 1);
    assert!(report.words >= 35, "{}", report.words);
    assert!(report.bytes > 0 && report.bytes as usize == out.len());
    // Searchable: the text comes back out of the new file, in order.
    let text = page_text(&out, 0);
    assert!(text.contains("quick brown fox"), "{text}");
    assert!(text.contains("wizards jump quickly"), "{text}");
    // The picture is byte for byte the one that went in.
    assert_eq!(image_hashes(&original), image_hashes(&out));
    assert!(report.text.contains("lazy dog"));
    std::fs::remove_file(path).ok();
}

#[test]
fn hindi_text_is_searchable_through_the_unicode_map() {
    let (path, file) = picture_pdf(&labelled("hi-1.png"), "hindi");
    let Some((report, out)) = run_searchable(&file, &Ocr::only(EngineKind::PpOcr), "hi") else {
        return;
    };
    assert_eq!(report.read, 1);
    let text = page_text(&out, 0);
    assert!(text.contains("भारत एक विशाल देश"), "{text}");
    assert!(text.contains("शामिल"), "{text}");
    std::fs::remove_file(path).ok();
}

#[test]
fn pages_with_text_are_skipped_and_a_file_of_only_text_pages_writes_nothing() {
    // A one page PDF that already has text.
    let content =
        b"BT /F1 12 Tf 20 100 Td (This page already has a perfectly good text layer.) Tj ET";
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
    ];
    objs.push(
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content,
            b"\nendstream",
        ]
        .concat(),
    );
    objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(o);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let x = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    let path =
        std::env::temp_dir().join(format!("panoply-ocr-test-text-{}.pdf", std::process::id()));
    std::fs::write(&path, &pdf).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    // No engine is needed to find out there is nothing to do.
    let (report, out) = run_searchable(&file, &Ocr::only(EngineKind::PpOcr), "en").unwrap();
    assert_eq!(report.read, 0);
    assert_eq!(report.had_text, 1);
    assert_eq!(report.bytes, 0);
    assert!(out.is_empty());
    assert_eq!(report.per_page, vec![PageStatus::HadText]);
    std::fs::remove_file(path).ok();
}

#[test]
fn a_mixed_file_reads_the_scan_and_leaves_the_text_page_alone() {
    use panoply_pdf::pageops::{BuildOptions, Input, OutPage, build};
    // Page 1: a scan. Page 2: a page of text (the OCR'd copy of the first, so it has a layer).
    let (path, file) = picture_pdf(&labelled("en-2.png"), "mixed-a");
    let Some((_, searchable)) = run_searchable(&file, &Ocr::new(), "en") else {
        return;
    };
    let (path_b, _) = picture_pdf(&labelled("en-3.png"), "mixed-b");
    let a = panoply_pdf::open(&std::fs::File::open(&path_b).unwrap()).unwrap();
    let b = panoply_pdf::Doc::open(Source::from_bytes(searchable.clone())).unwrap();
    let (pa, pb) = (a.doc.pages().unwrap(), b.pages().unwrap());
    let inputs = [
        Input {
            doc: &a.doc,
            pages: pa.into(),
            title: "a".into(),
        },
        Input {
            doc: &b,
            pages: pb.into(),
            title: "b".into(),
        },
    ];
    let pages = [
        OutPage {
            input: 0,
            index: 0,
            rotate: 0,
        },
        OutPage {
            input: 1,
            index: 0,
            rotate: 0,
        },
    ];
    let mut mixed = Vec::new();
    build(
        &mut mixed,
        &inputs,
        &pages,
        &BuildOptions::default(),
        &CancelToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    let mixed_path =
        std::env::temp_dir().join(format!("panoply-ocr-test-mixed-{}.pdf", std::process::id()));
    std::fs::write(&mixed_path, &mixed).unwrap();
    let f = std::fs::File::open(&mixed_path).unwrap();
    let Some((report, out)) = run_searchable(&f, &Ocr::new(), "en") else {
        return;
    };
    assert_eq!((report.pages, report.read, report.had_text), (2, 1, 1));
    assert!(matches!(report.per_page[0], PageStatus::Read { .. }));
    assert_eq!(report.per_page[1], PageStatus::HadText);
    // Windows.Media.Ocr, the first engine on Windows, reads a hyphen between digits as a dash, so
    // the comparison treats the dashes alike.
    let first = page_text(&out, 0).replace(['\u{2013}', '\u{2014}'], "-");
    assert!(first.contains("Invoice 2026-0417"), "{first}");
    assert!(page_text(&out, 1).contains("Optical character recognition"));
    // Still two pages, and the second page's content is unchanged.
    let d = panoply_pdf::Doc::open(Source::from_bytes(out)).unwrap();
    assert_eq!(d.pages().unwrap().len(), 2);
    for p in [path, path_b, mixed_path] {
        std::fs::remove_file(p).ok();
    }
    let _ = Obj::Null;
}

#[test]
fn cancelling_stops_the_job() {
    let (path, file) = picture_pdf(&labelled("en-1.png"), "cancel");
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut out = Vec::new();
    let r = panoply_ocr::searchable::make_searchable(
        &file,
        &mut out,
        &panoply_ocr::searchable::Options::default(),
        &Ocr::new(),
        &mut panoply_ocr::Fetch::new(&mut |_| {}),
        &cancel,
        &mut |_, _| {},
    );
    assert!(matches!(r, Err(panoply_ocr::OcrError::Cancelled)));
    std::fs::remove_file(path).ok();
}

/// The picture rotated a quarter turn counter-clockwise, as a PNG.
fn sideways_png(id: &str) -> Vec<u8> {
    let img = image(id);
    let (w, h) = (img.width as usize, img.height as usize);
    let mut rot = vec![0u8; img.data.len()];
    for y in 0..h {
        for x in 0..w {
            // Counter-clockwise: the pixel at (x, y) goes to (y, w - 1 - x) in an h x w image.
            let (nx, ny) = (y, w - 1 - x);
            let src = (y * w + x) * 3;
            let dst = (ny * h + nx) * 3;
            rot[dst..dst + 3].copy_from_slice(&img.data[src..src + 3]);
        }
    }
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, h as u32, w as u32);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(&rot).unwrap();
    }
    out
}

#[test]
fn a_page_turned_by_rotate_gets_its_text_layer_the_right_way_up() {
    use panoply_pdf::images_in::{ImageOptions, PageSize, images_to_pdf};
    use panoply_pdf::pageops::{BuildOptions, Input, OutPage, build};
    let png_bytes = sideways_png("en-2");
    let mut scan = Vec::new();
    images_to_pdf(
        &mut scan,
        1,
        &mut |_| Ok(("s.png".into(), png_bytes.clone())),
        &ImageOptions {
            page: PageSize::Image,
            margin_mm: 0.0,
        },
        &CancelToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    // The page is shown a quarter turn clockwise, which makes the text upright again.
    let doc = panoply_pdf::Doc::open(Source::from_bytes(scan)).unwrap();
    let pages = doc.pages().unwrap();
    let inputs = [Input {
        doc: &doc,
        pages: pages.into(),
        title: "s".into(),
    }];
    let mut turned = Vec::new();
    build(
        &mut turned,
        &inputs,
        &[OutPage {
            input: 0,
            index: 0,
            rotate: 90,
        }],
        &BuildOptions::default(),
        &CancelToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    let path = std::env::temp_dir().join(format!(
        "panoply-ocr-test-turned-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, &turned).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let Some((report, out)) = run_searchable(&file, &Ocr::new(), "en") else {
        return;
    };
    assert_eq!(report.read, 1);
    let text = page_text(&out, 0);
    assert!(
        text.contains("Optical character recognition converts"),
        "{text} {report:?}"
    );
    // The text layer is placed where the words are on the displayed page. pdfium reports
    // character boxes in the page's own (unturned) space, where a page shown a quarter turn
    // clockwise has the reading direction along +y and later lines at larger x: the first
    // letter is near the start of the first line, the last full stop on a later line and
    // further along.
    let engine = panoply_pdf::pdfium::engine().unwrap();
    let _g = panoply_pdf::pdfium::lock();
    let d = engine.load_pdf_from_byte_slice(&out, None).unwrap();
    let page = d.pages().get(0).unwrap();
    let text_obj = page.text().unwrap();
    let all_chars = text_obj.chars();
    let chars: Vec<_> = all_chars.iter().collect();
    let first = chars
        .iter()
        .find(|c| c.unicode_char() == Some('O'))
        .unwrap()
        .loose_bounds()
        .unwrap();
    let last = chars
        .iter()
        .rev()
        .find(|c| c.unicode_char() == Some('.'))
        .unwrap()
        .loose_bounds()
        .unwrap();
    let shown_width = page.width().value.max(page.height().value);
    assert!(first.bottom().value < shown_width * 0.2, "{first:?}");
    assert!(
        last.left().value > first.left().value + 20.0,
        "later line, larger x: {first:?} {last:?}"
    );
    assert!(
        last.bottom().value > first.bottom().value,
        "further along the line: {first:?} {last:?}"
    );
    std::fs::remove_file(path).ok();
}

#[test]
fn a_long_file_keeps_every_page_and_flags_the_blank_ones() {
    use panoply_pdf::images_in::{ImageOptions, PageSize, images_to_pdf};
    // 70 blank pages: more than one page tree level (the writer groups above 64).
    let mut png_bytes = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png_bytes, 64, 64);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .unwrap()
            .write_image_data(&[255u8; 64 * 64 * 3])
            .unwrap();
    }
    let mut pdf = Vec::new();
    images_to_pdf(
        &mut pdf,
        70,
        &mut |_| Ok(("blank.png".into(), png_bytes.clone())),
        &ImageOptions {
            page: PageSize::Image,
            margin_mm: 0.0,
        },
        &CancelToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    let path =
        std::env::temp_dir().join(format!("panoply-ocr-test-long-{}.pdf", std::process::id()));
    std::fs::write(&path, &pdf).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let Some((report, out)) = run_searchable(&file, &Ocr::new(), "en") else {
        return;
    };
    assert_eq!((report.pages, report.read, report.no_text), (70, 70, 70));
    assert_eq!(report.flagged.len(), 70);
    let d = panoply_pdf::Doc::open(Source::from_bytes(out)).unwrap();
    assert_eq!(d.pages().unwrap().len(), 70);
    std::fs::remove_file(path).ok();
}
