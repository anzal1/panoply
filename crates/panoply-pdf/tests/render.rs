//! pdfium: download once, draw pages, compare renders.

mod common;

use panoply_core::CancelToken;
use panoply_pdf::render::{ImageFormat, RenderDoc, compare};

#[test]
fn pdfium_renders_a_corpus_page_to_png_and_jpg() {
    let e = common::get("irs-p17-2022");
    let doc = RenderDoc::open(std::fs::File::open(&e.path).unwrap(), None).unwrap();
    assert!(doc.page_count() > 10);
    let s = common::Scratch::new("render");
    for (fmt, name) in [(ImageFormat::Png, "p.png"), (ImageFormat::Jpg, "p.jpg")] {
        let mut f = std::fs::File::create(s.path(name)).unwrap();
        let (w, h) = doc
            .write_page_image(0, 72.0, fmt, 85, &mut f, &CancelToken::new())
            .unwrap();
        assert!(
            (600..640).contains(&w) && (780..800).contains(&h),
            "{w}x{h}"
        );
        assert!(std::fs::metadata(s.path(name)).unwrap().len() > 2000);
    }
    let png = std::fs::read(s.path("p.png")).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let jpg = std::fs::read(s.path("p.jpg")).unwrap();
    assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
}

#[test]
fn a_document_compares_equal_to_itself_and_different_pages_differ() {
    let e = common::get("irs-p17-2022");
    let a = RenderDoc::open(std::fs::File::open(&e.path).unwrap(), None).unwrap();
    let b = RenderDoc::open(std::fs::File::open(&e.path).unwrap(), None).unwrap();
    let l = compare(&a, &b, 72.0, 4).unwrap();
    assert!(l.min > 0.999, "{l:?}");
    let (_, _, p0) = a.render_gray(0, 72.0, 5_000_000).unwrap();
    let (w, h, p5) = a.render_gray(5, 72.0, 5_000_000).unwrap();
    assert!(panoply_pdf::ssim::ssim_gray(&p0, &p5, w, h) < 0.95);
}

#[test]
fn an_encrypted_file_asks_for_a_password_and_opens_with_it() {
    let e = common::get("qpdf-enc-r3-rc4-128");
    let err = RenderDoc::open(std::fs::File::open(&e.path).unwrap(), None)
        .err()
        .unwrap();
    assert!(err.to_string().contains("password-protected"), "{err}");
    let ok = RenderDoc::open(std::fs::File::open(&e.path).unwrap(), e.password.as_deref());
    assert!(ok.is_ok(), "{:?}", ok.err());
    let wrong = RenderDoc::open(std::fs::File::open(&e.path).unwrap(), Some("nope"))
        .err()
        .unwrap();
    assert!(wrong.to_string().contains("did not open"), "{wrong}");
}

#[test]
fn drawing_from_several_threads_at_once_is_safe() {
    // pdfium is not thread safe: the engine serialises every call into it. Without that lock a
    // test binary like this one crashed with SIGSEGV within a few runs.
    let e = common::get("irs-p17-2022");
    let handles: Vec<_> = (0..4)
        .map(|t| {
            let path = e.path.clone();
            std::thread::spawn(move || {
                let doc = RenderDoc::open(std::fs::File::open(path).unwrap(), None).unwrap();
                for i in 0..6 {
                    let (w, h, px) = doc
                        .render_gray((t + i) % doc.page_count(), 40.0, 2_000_000)
                        .unwrap();
                    assert_eq!(px.len(), w * h);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}
