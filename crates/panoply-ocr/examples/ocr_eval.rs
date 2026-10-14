//! Measures one engine on one input: character error rate against a label file, time for the
//! first (cold) and second (warm) read, and the words found. One line of JSON on stdout. Peak
//! memory is measured around this process by `testdata/run_eval.sh` (`/usr/bin/time -l`).
//!
//! ```text
//! ocr_eval --engine vision|ppocr|tesseract|tesseract-plain|ladder [--langs hi,en] [--label l.txt] [--words w.json] <image | file.pdf#PAGE>
//! ```

use std::time::Instant;

use panoply_core::CancelToken;
use panoply_ocr::{EngineKind, Fetch, Lang, Ocr, cer};

fn main() {
    let mut engine = "ladder".to_string();
    let mut langs = String::new();
    let mut label: Option<String> = None;
    let mut input: Option<String> = None;
    let mut dump = false;
    let mut save: Option<String> = None;
    let mut words_out: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--engine" => engine = args.next().unwrap_or_default(),
            "--langs" => langs = args.next().unwrap_or_default(),
            "--label" => label = args.next(),
            "--dump" => dump = true,
            "--save" => save = args.next(),
            "--words" => words_out = args.next(),
            other => input = Some(other.to_string()),
        }
    }
    let input = input.expect("an input file");
    let langs = Lang::parse_list(&langs).expect("languages");
    let plain_tesseract = engine == "tesseract-plain";
    let ocr = match engine.as_str() {
        "vision" => Ocr::only(EngineKind::AppleVision),
        "ppocr" => Ocr::only(EngineKind::PpOcr),
        "tesseract" | "tesseract-plain" => Ocr::only(EngineKind::Tesseract),
        _ => Ocr::new(),
    };
    let (path, page) = match input.split_once('#') {
        Some((p, n)) => (
            p.to_string(),
            Some(n.parse::<usize>().expect("page number") - 1),
        ),
        None => (input.clone(), None),
    };
    let t_load = Instant::now();
    let img = if let Some(index) = page {
        let file = std::fs::File::open(&path).expect("open pdf");
        let doc = panoply_ocr::input::PageSource::open(file).expect("render doc");
        doc.render(index, 300.0, 36_000_000, &CancelToken::new())
            .expect("render page")
            .0
    } else {
        let ext = std::path::Path::new(&path)
            .extension()
            .and_then(|e| e.to_str());
        panoply_ocr::input::load_image(std::fs::File::open(&path).expect("open image"), ext)
            .expect("load image")
    };
    let load_ms = t_load.elapsed().as_secs_f64() * 1000.0;
    if let Some(out) = &save {
        let file = std::fs::File::create(out).expect("create png");
        let mut enc = png::Encoder::new(file, img.width, img.height);
        enc.set_color(if img.channels == 1 {
            png::ColorType::Grayscale
        } else {
            png::ColorType::Rgb
        });
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .expect("png header")
            .write_image_data(&img.data)
            .expect("png data");
    }
    let mut notice = |s: &str| eprintln!("{s}");
    let mut fetch = Fetch::new(&mut notice);
    let cancel = CancelToken::new();
    let t0 = Instant::now();
    let read = |fetch: &mut Fetch<'_>| {
        if plain_tesseract {
            // Tesseract as it comes: its own page layout analysis, default segmentation mode.
            panoply_ocr::engine::tesseract::recognize(&img, &langs, fetch)
        } else {
            ocr.recognize(&img, &langs, fetch, &cancel)
        }
    };
    let first = read(&mut fetch);
    let cold_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let page = match first {
        Ok(p) => p,
        Err(e) => {
            println!(
                "{}",
                serde_json::json!({"input": input, "engine": engine, "error": e.to_string()})
            );
            return;
        }
    };
    let t1 = Instant::now();
    let _ = read(&mut fetch);
    let warm_ms = t1.elapsed().as_secs_f64() * 1000.0;
    if let Some(out) = &words_out {
        let words: Vec<_> = page
            .lines
            .iter()
            .enumerate()
            .flat_map(|(li, l)| {
                l.words.iter().map(move |w| {
                    serde_json::json!({"line": li, "text": w.text, "x": w.bbox.x, "y": w.bbox.y,
                        "w": w.bbox.w, "h": w.bbox.h, "conf": w.confidence})
                })
            })
            .collect();
        std::fs::write(out, serde_json::to_string(&words).expect("json")).expect("write words");
    }
    let text = page.text();
    if dump {
        println!("{text}");
    }
    let reference = label
        .as_ref()
        .map(|l| std::fs::read_to_string(l).expect("label"));
    let mut v = serde_json::json!({
        "input": input,
        "engine": page.engine.name(),
        "px": [img.width, img.height],
        "load_ms": load_ms.round(),
        "cold_ms": cold_ms.round(),
        "warm_ms": warm_ms.round(),
        "words": page.word_count(),
        "lines": page.lines.len(),
        "mean_conf": (f64::from(page.mean_confidence()) * 1000.0).round() / 1000.0,
    });
    if let Some(r) = reference {
        v["cer"] = serde_json::json!((cer::cer(&r, &text) * 10000.0).round() / 10000.0);
        v["wer"] = serde_json::json!((cer::wer(&r, &text) * 10000.0).round() / 10000.0);
    }
    println!("{v}");
}
