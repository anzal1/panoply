//! True redaction: text pages and scans. Nothing of a redacted string may survive in the text
//! pdfium extracts, in a decoded stream or string, or in the raw bytes.

mod common;

use std::collections::HashSet;
use std::fs::File;

use common::with_fetch;
use panoply_core::CancelToken;
use panoply_ocr::Ocr;
use panoply_ocr::redact::{self, Match, Options, Preset, Where};
use panoply_pdf::obj::Obj;
use panoply_pdf::source::Source;

const EMAIL: &str = "anzal@example.com";
const PHONE: &str = "98765 43210";
const AADHAAR: &str = "2345 6789 0124";

/// A PDF with a classic xref table from numbered object bodies (streams included in the body).
fn build(objs: &[(u32, Vec<u8>)], trailer: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let max = objs.iter().map(|o| o.0).max().unwrap();
    let mut offs = vec![0usize; max as usize + 1];
    for (n, body) in objs {
        offs[*n as usize] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for o in offs.iter().skip(1) {
        if *o == 0 {
            out.extend_from_slice(b"0000000000 00000 f \n");
        } else {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R {trailer} >>\nstartxref\n{xref}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    out
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

/// Two pages of Helvetica. Page 1 holds the secrets; the document info, a note annotation and an
/// attached file hold them too. Then an incremental update is appended that edits the title.
fn text_pdf() -> Vec<u8> {
    let p1 = format!(
        "BT /F1 14 Tf 40 700 Td (Contact Anzal at {EMAIL} or call {PHONE}.) Tj 0 -30 Td (Aadhaar {AADHAAR} on file.) Tj ET"
    );
    let p2 = "BT /F1 14 Tf 40 700 Td (Quarterly report, nothing private here.) Tj ET";
    let objs: Vec<(u32, Vec<u8>)> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles << /Names [(notes.txt) 9 0 R] >> >> >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R /Annots [8 0 R] >>".to_vec()),
        (4, stream("", p1.as_bytes())),
        (5, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>".to_vec()),
        (6, stream("", p2.as_bytes())),
        (7, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
        (8, format!("<< /Type /Annot /Subtype /Text /Rect [10 10 30 30] /Contents (call {PHONE} back) >>").into_bytes()),
        (9, b"<< /Type /Filespec /F (notes.txt) /EF << /F 10 0 R >> >>".to_vec()),
        (10, stream("/Type /EmbeddedFile", format!("mail {EMAIL} please").as_bytes())),
        (11, format!("<< /Title (Notes for {EMAIL}) /Author (A. Writer) >>").into_bytes()),
    ];
    let mut pdf = build(&objs, "/Info 11 0 R");
    // Incremental update: a new revision of the info dictionary; the old one stays in the file.
    let prev = pdf.windows(9).rposition(|w| w == b"startxref").unwrap();
    let old_xref: usize = String::from_utf8_lossy(&pdf[prev + 10..])
        .lines()
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let off = pdf.len();
    pdf.extend_from_slice(b"11 0 obj\n<< /Title (Notes) /Author (A. Writer) /Subject (kept for anzal@example.com) >>\nendobj\n");
    let x = pdf.len();
    pdf.extend_from_slice(format!("xref\n11 1\n{off:010} 00000 n \ntrailer\n<< /Size 12 /Root 1 0 R /Info 11 0 R /Prev {old_xref} >>\nstartxref\n{x}\n%%EOF\n").as_bytes());
    pdf
}

fn tmp(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("panoply-redact-{tag}-{}.pdf", std::process::id()))
}

fn pdfium_text(pdf: &[u8], page: i32) -> String {
    let engine = panoply_pdf::pdfium::engine().unwrap();
    let _g = panoply_pdf::pdfium::lock();
    let doc = engine.load_pdf_from_byte_slice(pdf, None).unwrap();
    let p = doc.pages().get(page).unwrap();
    let t = p.text().unwrap();
    t.chars().iter().filter_map(|c| c.unicode_char()).collect()
}

/// An independent check, not `redact::verify`: decode every stream and string in the file.
fn leaks_in(pdf: &[u8], secrets: &[&str]) -> Vec<String> {
    let mut bad = Vec::new();
    let low = String::from_utf8_lossy(pdf).to_lowercase();
    for s in secrets {
        if low.contains(&s.to_lowercase()) {
            bad.push(format!("raw bytes contain {s}"));
        }
    }
    let doc = panoply_pdf::Doc::open(Source::from_bytes(pdf.to_vec())).unwrap();
    for n in doc.used_objects() {
        if let Ok(Some((_, data))) = doc.stream_data(n) {
            let t = String::from_utf8_lossy(&data).to_lowercase();
            for s in secrets {
                if t.contains(&s.to_lowercase()) {
                    bad.push(format!("object {n} decoded stream contains {s}"));
                }
            }
        }
    }
    bad
}

fn run(
    file: &File,
    opts: &Options,
    tweak: impl Fn(&mut Vec<Match>),
) -> Option<(redact::Found, redact::Report, Vec<u8>)> {
    let ocr = Ocr::new();
    let cancel = CancelToken::new();
    let found = with_fetch(|f| redact::find(file, opts, &ocr, f, &cancel, &mut |_, _| {}))?;
    let found_words = found.scan_words.clone();
    let mut all = found.matches.clone();
    tweak(&mut all);
    let mut out = Vec::new();
    let report = with_fetch(|f| {
        redact::apply(
            file,
            &mut out,
            opts,
            &all,
            &found_words,
            &ocr,
            f,
            &cancel,
            &mut |_, _| {},
        )
    })?;
    Some((found, report, out))
}

#[test]
fn a_text_pdf_loses_its_text_info_annotation_attachment_and_history() {
    let path = tmp("text");
    let input = text_pdf();
    std::fs::write(&path, &input).unwrap();
    // The incremental history really is in the input.
    assert!(String::from_utf8_lossy(&input).contains("Notes for anzal@example.com"));
    let file = File::open(&path).unwrap();
    let opts = Options {
        presets: vec![Preset::Email, Preset::Phone, Preset::Aadhaar],
        ..Options::default()
    };
    let (found, report, out) = run(&file, &opts, |_| {}).unwrap();

    // The preview lists every match with its place.
    for m in &found.matches {
        println!(
            "match #{}: {} {:?} at {:?} boxes={}",
            m.id,
            m.kind,
            m.text,
            m.at,
            m.rects.len()
        );
    }
    let on_page: Vec<&Match> = found
        .matches
        .iter()
        .filter(|m| m.at == Where::Page(0))
        .collect();
    assert_eq!(on_page.len(), 3, "email, phone and aadhaar on page 1");
    assert!(on_page.iter().all(|m| !m.rects.is_empty()));
    assert!(found.matches.iter().any(|m| matches!(
        m.at,
        Where::Object {
            what: "annotation",
            ..
        }
    )));
    assert!(found.matches.iter().any(|m| matches!(
        m.at,
        Where::Object {
            what: "document info",
            ..
        }
    )));
    assert!(found.matches.iter().any(|m| matches!(
        m.at,
        Where::Object {
            what: "attachment",
            ..
        }
    )));

    // 1. pdfium finds none of the redacted strings.
    let t1 = pdfium_text(&out, 0);
    let t2 = pdfium_text(&out, 1);
    println!("page 1 text after: {t1:?}");
    println!("page 2 text after: {t2:?}");
    for s in [EMAIL, PHONE, AADHAAR, "example.com", "98765", "2345"] {
        assert!(!t1.contains(s) && !t2.contains(s), "pdfium still finds {s}");
    }
    // 2. The raw bytes and decoded streams hold none either.
    let bad = leaks_in(
        &out,
        &[EMAIL, PHONE, AADHAAR, "anzal@example", "98765", "6789 0124"],
    );
    println!("independent byte and stream check: {bad:?}");
    assert!(bad.is_empty(), "{bad:?}");
    let reopened = File::open(&path).unwrap();
    std::fs::write(&path, &out).unwrap();
    drop(reopened);
    let leaks = redact::verify(&File::open(&path).unwrap(), &report.needles).unwrap();
    assert!(leaks.is_empty(), "{leaks:?}");
    // 3. Unredacted text is still extractable, on the redacted page and the untouched one.
    assert!(t1.contains("Contact") && t1.contains("on file"), "{t1:?}");
    assert!(
        t2.contains("Quarterly report, nothing private here."),
        "{t2:?}"
    );
    // 4. Same number of pages, and a fresh file with one cross reference section.
    let doc = panoply_pdf::Doc::open(Source::from_bytes(out.clone())).unwrap();
    assert_eq!(doc.pages().unwrap().len(), 2);
    assert_eq!(
        String::from_utf8_lossy(&out).matches("startxref").count(),
        1,
        "no incremental update"
    );
    assert_eq!(report.redacted_pages, vec![0]);
    println!("dropped objects: {:?}", report.dropped);
    assert!(report.dropped.iter().any(|d| d.1 == "annotation"));
    assert!(report.dropped.iter().any(|d| d.1 == "attachment"));
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_unticked_match_stays() {
    let path = tmp("untick");
    std::fs::write(&path, text_pdf()).unwrap();
    let file = File::open(&path).unwrap();
    let opts = Options {
        presets: vec![Preset::Email, Preset::Phone, Preset::Aadhaar],
        ..Options::default()
    };
    let (_, _, out) = run(&file, &opts, |all| {
        let keep: HashSet<&str> = [PHONE].into_iter().collect();
        for m in all.iter_mut() {
            if keep.contains(m.text.as_str()) {
                m.selected = false;
            }
        }
    })
    .unwrap();
    let t1 = pdfium_text(&out, 0);
    assert!(
        t1.contains(PHONE),
        "the unticked phone number is still there: {t1:?}"
    );
    assert!(!t1.contains(EMAIL) && !t1.contains(AADHAAR), "{t1:?}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_image_only_pdf_is_redacted_in_its_pixels() {
    // Make a scan: draw the text page, take its picture, and build a PDF that is only that picture.
    let src = tmp("scan-src");
    std::fs::write(&src, text_pdf()).unwrap();
    let rd = panoply_pdf::render::RenderDoc::open(File::open(&src).unwrap(), None).unwrap();
    let mut png = Vec::new();
    rd.write_page_image(
        0,
        200.0,
        panoply_pdf::render::ImageFormat::Png,
        90,
        &mut png,
        &CancelToken::new(),
    )
    .unwrap();
    drop(rd);
    let scan = tmp("scan");
    panoply_pdf::images_in::images_to_pdf(
        std::io::BufWriter::new(File::create(&scan).unwrap()),
        1,
        &mut |_| Ok(("scan.png".into(), png.clone())),
        &panoply_pdf::images_in::ImageOptions {
            page: panoply_pdf::images_in::PageSize::Image,
            margin_mm: 0.0,
        },
        &CancelToken::new(),
        &mut |_, _| {},
    )
    .unwrap();
    let input = std::fs::read(&scan).unwrap();
    assert!(
        pdfium_text(&input, 0).trim().is_empty(),
        "the scan has no text layer"
    );
    let file = File::open(&scan).unwrap();
    let opts = Options {
        presets: vec![Preset::Email, Preset::Phone, Preset::Aadhaar],
        ..Options::default()
    };
    let Some((found, report, out)) = run(&file, &opts, |_| {}) else {
        return;
    };
    for m in &found.matches {
        println!(
            "scan match #{}: {} {:?} at {:?}",
            m.id, m.kind, m.text, m.at
        );
    }
    assert_eq!(found.scanned, vec![0], "read with OCR");
    assert!(
        found.matches.iter().any(|m| m.kind == "email"),
        "OCR found the email"
    );
    let t = pdfium_text(&out, 0);
    println!("scan text layer after: {t:?}");
    for s in ["example.com", "98765", "2345 6789", "anzal@"] {
        assert!(!t.contains(s), "pdfium still finds {s}");
    }
    assert!(leaks_in(&out, &["example.com", "98765", "2345 6789"]).is_empty());
    assert!(
        t.contains("Contact") || t.contains("Aadhaar"),
        "the unredacted words stay searchable: {t:?}"
    );
    // The pixels are gone: read the new picture again.
    let outp = tmp("scan-out");
    std::fs::write(&outp, &out).unwrap();
    let src_pages = panoply_ocr::input::PageSource::open(File::open(&outp).unwrap()).unwrap();
    let (img, _) = src_pages
        .render(0, 200.0, 36_000_000, &CancelToken::new())
        .unwrap();
    let again = with_fetch(|f| Ocr::new().recognize(&img, &[], f, &CancelToken::new())).unwrap();
    let reread = again.text();
    println!("OCR of the redacted picture: {reread:?}");
    println!(
        "picture {}x{}, {} words read",
        img.width,
        img.height,
        again.word_count()
    );
    assert!(
        again.word_count() > 3,
        "the redrawn page is still readable: {reread:?}"
    );
    assert!(
        !reread.contains("example.com") && !reread.contains("98765"),
        "{reread:?}"
    );
    let doc = panoply_pdf::Doc::open(Source::from_bytes(out)).unwrap();
    assert_eq!(doc.pages().unwrap().len(), 1);
    assert_eq!(report.redacted_pages, vec![0]);
    for p in [src, scan, outp] {
        std::fs::remove_file(p).ok();
    }
}

/// A one page PDF of Helvetica text lines, from `(x, y, text)` runs, with extra objects.
fn simple_pdf(
    runs: &[(i32, i32, &str)],
    extra: Vec<(u32, Vec<u8>)>,
    catalog_extra: &str,
    annots: &str,
) -> Vec<u8> {
    let mut content = String::from("BT /F1 18 Tf\n");
    let (mut px, mut py) = (0, 0);
    for (x, y, t) in runs {
        content.push_str(&format!("{} {} Td ({t}) Tj\n", x - px, y - py));
        (px, py) = (*x, *y);
    }
    content.push_str("ET");
    let mut objs: Vec<(u32, Vec<u8>)> = vec![
        (1, format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (
            3,
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R {annots} >>").into_bytes(),
        ),
        (4, stream("", content.as_bytes())),
        (7, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()),
    ];
    objs.extend(extra);
    build(&objs, "")
}

fn ids(found: &redact::Found) -> Vec<String> {
    found.matches.iter().map(|m| m.kind.clone()).collect()
}

/// The gray picture of a page of a PDF held in memory, 200 dpi, and its scale in pixels a point.
fn page_picture(pdf: &[u8], tag: &str) -> (panoply_ocr::OcrImage, f32) {
    let p = tmp(tag);
    std::fs::write(&p, pdf).unwrap();
    let src = panoply_ocr::input::PageSource::open(File::open(&p).unwrap()).unwrap();
    let r = src
        .render(0, 200.0, 36_000_000, &CancelToken::new())
        .unwrap();
    std::fs::remove_file(&p).ok();
    r
}

#[test]
fn a_form_field_value_is_not_burned_into_the_page_picture() {
    // P0: an Aadhaar in a filled text field, and an email in the body that makes the page
    // get redrawn. The field's appearance must not show in the new picture.
    let widget = format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (id) /V ({AADHAAR}) /Rect [100 500 420 540] /F 4 /DA (/Helv 0 Tf 0 g) /AP << /N 9 0 R >> >>"
    );
    let ap = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 320 40] /Resources << /Font << /Helv 7 0 R >> >>",
        format!("/Tx BMC q BT /Helv 24 Tf 4 12 Td ({AADHAAR}) Tj ET Q EMC").as_bytes(),
    );
    let pdf = simple_pdf(
        &[(40, 700, "Contact anzal@example.com today")],
        vec![(8, widget.into_bytes()), (9, ap)],
        "/AcroForm << /Fields [8 0 R] /DA (/Helv 0 Tf 0 g) /DR << /Font << /Helv 7 0 R >> >> >>",
        "/Annots [8 0 R]",
    );
    let path = tmp("form");
    std::fs::write(&path, &pdf).unwrap();
    let file = File::open(&path).unwrap();
    let opts = Options {
        presets: vec![Preset::Email, Preset::Aadhaar],
        ..Options::default()
    };
    let (found, _, out) = run(&file, &opts, |_| {}).unwrap();
    assert!(
        found
            .matches
            .iter()
            .any(|m| m.at == Where::Page(0) && m.kind == "email")
    );
    assert!(found.matches.iter().any(|m| matches!(
        m.at,
        Where::Object {
            what: "form field",
            ..
        }
    ) && m.kind == "aadhaar"));
    assert!(leaks_in(&out, &[AADHAAR, "2345 6789", "example.com"]).is_empty());
    // The field's place on the picture holds no drawn text: only solid black or white, no
    // mid grays from anti-aliased digits.
    let (img, scale) = page_picture(&out, "form-out");
    let w = img.width as usize;
    let (x0, x1) = ((104.0 * scale) as usize, (416.0 * scale) as usize);
    let (y0, y1) = (
        ((792.0 - 536.0) * scale) as usize,
        ((792.0 - 504.0) * scale) as usize,
    );
    let mut mid = 0usize;
    for y in y0..y1 {
        for x in x0..x1 {
            let v = img.data[y * w + x];
            if (60..=200).contains(&v) {
                mid += 1;
            }
        }
    }
    assert!(mid < 20, "{mid} mid gray pixels where the form field was");
    // And read back, no digits of it.
    let Some(again) = with_fetch(|f| Ocr::new().recognize(&img, &[], f, &CancelToken::new()))
    else {
        return;
    };
    let text = again.text().replace(' ', "");
    assert!(!text.contains("6789") && !text.contains("2345"), "{text:?}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn numbers_wrapped_over_two_lines_are_found_and_removed() {
    // P1: pdfium puts a line end between the groups.
    let pdf = simple_pdf(
        &[
            (40, 700, "Aadhaar 2345 6789"),
            (40, 680, "0124 on file."),
            (40, 640, "Card 4111 1111"),
            (40, 620, "1111 1111 ok."),
        ],
        vec![],
        "",
        "",
    );
    let path = tmp("wrap");
    std::fs::write(&path, &pdf).unwrap();
    let file = File::open(&path).unwrap();
    let opts = Options {
        presets: vec![Preset::Aadhaar, Preset::Card],
        ..Options::default()
    };
    let (found, report, out) = run(&file, &opts, |_| {}).unwrap();
    let kinds = ids(&found);
    assert!(kinds.contains(&"aadhaar".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"card".to_string()), "{kinds:?}");
    let t = pdfium_text(&out, 0);
    for s in ["2345", "6789", "0124", "4111", "1111"] {
        assert!(!t.contains(s), "{s} survived: {t:?}");
    }
    assert!(t.contains("on file."), "{t:?}");
    std::fs::write(&path, &out).unwrap();
    let written = File::open(&path).unwrap();
    let leaks = redact::verify(&written, &report.needles).unwrap();
    assert!(leaks.is_empty(), "{leaks:?}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_kids_of_a_dropped_field_go_with_it() {
    // P1: a field with two widgets whose appearances draw the value in pieces the matcher
    // cannot read as one number. Dropping the field must take the widgets.
    let parent = format!("<< /FT /Tx /T (id) /V ({AADHAAR}) /Kids [21 0 R 22 0 R] >>");
    let kid = |y: i32| {
        format!("<< /Type /Annot /Subtype /Widget /Parent 20 0 R /Rect [100 {y} 300 {}] /F 4 /AP << /N 23 0 R >> >>", y + 30)
            .into_bytes()
    };
    let ap = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 200 30] /Resources << /Font << /Helv 7 0 R >> >>",
        b"BT /Helv 14 Tf 2 8 Td (2345) Tj (x) Tj (6789) Tj (y) Tj (0124) Tj ET",
    );
    let pdf = simple_pdf(
        &[(40, 700, "Reach anzal@example.com")],
        vec![
            (20, parent.into_bytes()),
            (21, kid(500)),
            (22, kid(400)),
            (23, ap),
        ],
        "/AcroForm << /Fields [20 0 R] >>",
        "/Annots [21 0 R 22 0 R]",
    );
    let path = tmp("kids");
    std::fs::write(&path, &pdf).unwrap();
    let file = File::open(&path).unwrap();
    let opts = Options {
        presets: vec![Preset::Email, Preset::Aadhaar],
        ..Options::default()
    };
    let (_, report, out) = run(&file, &opts, |_| {}).unwrap();
    let dropped: Vec<u32> = report.dropped.iter().map(|d| d.0).collect();
    for n in [20, 21, 22] {
        assert!(dropped.contains(&n), "object {n} not dropped: {dropped:?}");
    }
    assert!(leaks_in(&out, &["6789) tj", AADHAAR]).is_empty());
    // No field list with holes, and no widgets left anywhere.
    let doc = panoply_pdf::Doc::open(Source::from_bytes(out)).unwrap();
    let cat = doc.catalog().unwrap();
    if let Some(af) = cat.get("AcroForm").and_then(|a| doc.resolve_dict(a)) {
        let fields = af
            .get("Fields")
            .and_then(|f| doc.resolve_array(f))
            .unwrap_or_default();
        assert!(fields.is_empty(), "{fields:?}");
    }
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_name_tree_keeps_its_pairs_when_one_attachment_is_dropped() {
    // P1: two attached files, one holds an email. The other must survive with its name.
    let spec = |name: &str, f: u32| {
        format!("<< /Type /Filespec /F ({name}) /EF << /F {f} 0 R >> >>").into_bytes()
    };
    let pdf = simple_pdf(
        &[(40, 700, "Hello there")],
        vec![
            (9, spec("a.txt", 10)),
            (
                10,
                stream("/Type /EmbeddedFile", b"mail anzal@example.com please"),
            ),
            (11, spec("b.txt", 12)),
            (12, stream("/Type /EmbeddedFile", b"nothing to see")),
        ],
        "/Names << /EmbeddedFiles << /Names [(a.txt) 9 0 R (b.txt) 11 0 R] >> >>",
        "",
    );
    let path = tmp("names");
    std::fs::write(&path, &pdf).unwrap();
    let file = File::open(&path).unwrap();
    let opts = Options {
        presets: vec![Preset::Email],
        ..Options::default()
    };
    let (_, report, out) = run(&file, &opts, |_| {}).unwrap();
    let dropped: Vec<u32> = report.dropped.iter().map(|d| d.0).collect();
    assert!(dropped.contains(&9) && dropped.contains(&10), "{dropped:?}");
    assert!(
        !dropped.contains(&11) && !dropped.contains(&12),
        "{dropped:?}"
    );
    assert!(leaks_in(&out, &["anzal@example.com"]).is_empty());
    let doc = panoply_pdf::Doc::open(Source::from_bytes(out)).unwrap();
    let cat = doc.catalog().unwrap();
    let names = cat.get("Names").and_then(|n| doc.resolve_dict(n)).unwrap();
    let ef = names
        .get("EmbeddedFiles")
        .and_then(|n| doc.resolve_dict(n))
        .unwrap();
    let arr = ef.get("Names").and_then(|n| doc.resolve_array(n)).unwrap();
    assert_eq!(arr.len(), 2, "pairs stay pairs: {arr:?}");
    assert_eq!(arr[0].as_str_bytes(), Some(&b"b.txt"[..]));
    let fnum = doc
        .resolve_dict(&arr[1])
        .and_then(|s| s.get("EF").and_then(|e| doc.resolve_dict(e)))
        .and_then(|e| e.get("F").and_then(Obj::as_ref_num))
        .expect("the surviving file spec points at its file");
    let (_, data) = doc.stream_data(fnum).unwrap().unwrap();
    assert!(String::from_utf8_lossy(&data).contains("nothing to see"));
    std::fs::remove_file(&path).ok();
}

#[test]
fn bookmarks_are_joined_up_around_a_dropped_one() {
    let pdf = simple_pdf(
        &[(40, 700, "Hello there")],
        vec![
            (
                30,
                b"<< /Type /Outlines /First 31 0 R /Last 33 0 R /Count 3 >>".to_vec(),
            ),
            (
                31,
                b"<< /Title (Intro) /Parent 30 0 R /Next 32 0 R >>".to_vec(),
            ),
            (
                32,
                b"<< /Title (Mail anzal@example.com) /Parent 30 0 R /Prev 31 0 R /Next 33 0 R >>"
                    .to_vec(),
            ),
            (
                33,
                b"<< /Title (End) /Parent 30 0 R /Prev 32 0 R >>".to_vec(),
            ),
        ],
        "/Outlines 30 0 R",
        "",
    );
    let path = tmp("outline");
    std::fs::write(&path, &pdf).unwrap();
    let file = File::open(&path).unwrap();
    let opts = Options {
        presets: vec![Preset::Email],
        ..Options::default()
    };
    let (_, report, out) = run(&file, &opts, |_| {}).unwrap();
    assert!(
        report
            .dropped
            .iter()
            .any(|d| d.0 == 32 && d.1 == "bookmark"),
        "{:?}",
        report.dropped
    );
    let doc = panoply_pdf::Doc::open(Source::from_bytes(out)).unwrap();
    let cat = doc.catalog().unwrap();
    let root = cat
        .get("Outlines")
        .and_then(|o| doc.resolve_dict(o))
        .expect("outline kept");
    let first = root.get("First").unwrap().clone();
    let a = doc.resolve_dict(&first).unwrap();
    assert_eq!(
        a.get("Title").and_then(Obj::as_str_bytes),
        Some(&b"Intro"[..])
    );
    let next = a
        .get("Next")
        .expect("Intro points to the next kept item")
        .clone();
    let c = doc.resolve_dict(&next).unwrap();
    assert_eq!(
        c.get("Title").and_then(Obj::as_str_bytes),
        Some(&b"End"[..])
    );
    assert!(c.get("Next").is_none());
    assert_eq!(c.get("Prev").and_then(Obj::as_ref_num), first.as_ref_num());
    assert_eq!(
        root.get("Last").and_then(Obj::as_ref_num),
        next.as_ref_num()
    );
    assert_eq!(root.get("Count").and_then(Obj::as_i64), Some(2));
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_kept_annotation_stays_visible_on_a_redacted_page() {
    // P1: a stamp or highlight that holds no match must not vanish when its page is redrawn.
    let annot =
        b"<< /Type /Annot /Subtype /Square /Rect [100 300 200 350] /F 4 /AP << /N 9 0 R >> >>"
            .to_vec();
    let ap = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 100 50]",
        b"0.5 g 0 0 100 50 re f",
    );
    let pdf = simple_pdf(
        &[(40, 700, "Contact anzal@example.com today")],
        vec![(8, annot), (9, ap)],
        "",
        "/Annots [8 0 R]",
    );
    let path = tmp("annot");
    std::fs::write(&path, &pdf).unwrap();
    let file = File::open(&path).unwrap();
    let opts = Options {
        presets: vec![Preset::Email],
        ..Options::default()
    };
    let (_, report, out) = run(&file, &opts, |_| {}).unwrap();
    assert_eq!(report.redacted_pages, vec![0]);
    let (img, scale) = page_picture(&out, "annot-out");
    let w = img.width as usize;
    let (cx, cy) = ((150.0 * scale) as usize, ((792.0 - 325.0) * scale) as usize);
    let (mut sum, mut n) = (0u32, 0u32);
    for y in cy - 10..cy + 10 {
        for x in cx - 10..cx + 10 {
            sum += u32::from(img.data[y * w + x]);
            n += 1;
        }
    }
    let mean = sum / n;
    assert!(
        (90..=170).contains(&mean),
        "the annotation's gray is drawn, mean {mean}"
    );
    std::fs::remove_file(&path).ok();
}
