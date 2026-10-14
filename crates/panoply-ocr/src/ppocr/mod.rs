//! PP-OCRv5 mobile detection and recognition on ONNX Runtime: the engine that works on every
//! platform, and the only one that reads Devanagari (Hindi, Marathi, Nepali) here.
//!
//! Detection (`PP-OCRv5_mobile_det`, 4.8 MB) finds text lines on the whole page. Each line is
//! cut out straight (rotated by its measured angle) and read by a script specific recognizer
//! (`en`, `latin` or `devanagari`, about 8 MB each). Word positions come from where the
//! recognizer emitted each symbol. Models and the ONNX Runtime library are downloaded on first
//! use with pinned checksums ([`crate::models`]). The loaded sessions are dropped after 60
//! seconds without a job; the runtime library itself stays mapped until the process ends (it
//! cannot be unloaded), which is the same known exception as pdfium in `panoply-pdf`.

mod ctc;
mod db;
pub(crate) mod imgops;

pub use imgops::Oriented;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use panoply_core::CancelToken;

use crate::error::{OcrError, Result};
use crate::lang::{Lang, Script};
use crate::models::{self, Asset, Fetch};
use crate::types::{EngineKind, Line, OcrImage, OcrPage, Rect, Word};

pub use db::DbParams;

/// Which recognizer reads a set of languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Recognizer {
    /// English only.
    English,
    /// The Latin script languages other than (or with) English.
    Latin,
    /// Devanagari, which also reads the Latin letters mixed into Hindi text.
    Devanagari,
}

impl Recognizer {
    fn asset(self) -> &'static Asset {
        match self {
            Recognizer::English => &models::REC_EN,
            Recognizer::Latin => &models::REC_LATIN,
            Recognizer::Devanagari => &models::REC_DEVANAGARI,
        }
    }

    fn dict_text(self) -> &'static str {
        match self {
            Recognizer::English => include_str!("../../data/en.dict"),
            Recognizer::Latin => include_str!("../../data/latin.dict"),
            Recognizer::Devanagari => include_str!("../../data/devanagari.dict"),
        }
    }

    /// The recognizer for these languages, or `None` when PP-OCR has none that covers all of
    /// them. No languages (automatic) means English.
    pub fn for_langs(langs: &[Lang]) -> Option<Recognizer> {
        if langs.is_empty() {
            return Some(Recognizer::English);
        }
        if langs.iter().any(|l| l.script() == Script::Devanagari) {
            // Devanagari plus anything that is not Devanagari or Latin has no single model.
            return langs
                .iter()
                .all(|l| matches!(l.script(), Script::Devanagari | Script::Latin))
                .then_some(Recognizer::Devanagari);
        }
        if langs.iter().all(|l| l.script() == Script::Latin) {
            return Some(if langs.iter().all(|l| l.code() == "en") {
                Recognizer::English
            } else {
                Recognizer::Latin
            });
        }
        None
    }
}

/// How the detector is run. The defaults are tuned on the labelled set in
/// `docs/research/ocr-results.md`.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// Shortest long side the detector sees: smaller pictures are enlarged to this.
    pub min_long_side: u32,
    /// Longest long side: larger pages are shrunk to this.
    pub max_long_side: u32,
    /// DB post-process settings.
    pub db: DbParams,
    /// Lines whose confidence is below this are dropped as noise.
    pub drop_score: f32,
    /// Intra-op threads for ONNX Runtime.
    pub threads: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            min_long_side: 1280,
            max_long_side: env_num("PANOPLY_OCR_DET_SIDE").unwrap_or(1600),
            db: DbParams::default(),
            drop_score: 0.3,
            threads: default_threads(),
        }
    }
}

fn env_num(name: &str) -> Option<u32> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

fn default_threads() -> usize {
    if let Some(n) = std::env::var("PANOPLY_OCR_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        && n > 0
    {
        return n;
    }
    std::thread::available_parallelism()
        .map(|n| n.get().min(4))
        .unwrap_or(2)
}

struct Loaded {
    det: Session,
    recs: HashMap<Recognizer, (Session, ctc::Dict)>,
    last_used: Instant,
}

static SLOT: Mutex<Option<Loaded>> = Mutex::new(None);
static IDLE_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(panoply_core::budgets::MODEL_IDLE_UNLOAD_S * 1000);
static REAPER: Mutex<bool> = Mutex::new(false);

/// How long loaded models are kept without a job. The default is the 60 seconds of the
/// budget; tests shorten it.
pub fn set_idle_timeout(d: Duration) {
    IDLE_MS.store(d.as_millis() as u64, std::sync::atomic::Ordering::Relaxed);
}

/// True while detection or recognition models are held in memory.
pub fn models_loaded() -> bool {
    SLOT.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// Drop the loaded models now.
pub fn unload() {
    *SLOT.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

fn ensure_reaper() {
    let mut running = REAPER.lock().unwrap_or_else(|p| p.into_inner());
    if *running {
        return;
    }
    *running = true;
    let spawned = std::thread::Builder::new()
        .name("panoply-ocr-idle".into())
        .spawn(|| {
            loop {
                let idle =
                    Duration::from_millis(IDLE_MS.load(std::sync::atomic::Ordering::Relaxed));
                std::thread::sleep(
                    (idle / 4).clamp(Duration::from_millis(20), Duration::from_secs(2)),
                );
                let mut slot = SLOT.lock().unwrap_or_else(|p| p.into_inner());
                match slot.as_ref() {
                    None => {
                        *REAPER.lock().unwrap_or_else(|p| p.into_inner()) = false;
                        return;
                    }
                    Some(l) if l.last_used.elapsed() >= idle => {
                        *slot = None;
                        *REAPER.lock().unwrap_or_else(|p| p.into_inner()) = false;
                        return;
                    }
                    Some(_) => {}
                }
            }
        });
    if spawned.is_err() {
        *running = false;
    }
}

fn ort_err(what: &str, e: impl std::fmt::Display) -> OcrError {
    OcrError::Engine(format!(
        "The text recognition engine failed while {what} ({e})."
    ))
}

fn init_runtime(fetch: &mut Fetch<'_>) -> Result<()> {
    static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    if let Some(r) = INIT.get() {
        return r.clone().map_err(OcrError::Engine);
    }
    let path = models::ensure_ort(fetch)?;
    let r = INIT.get_or_init(|| {
        match ort::init_from(&path) {
            Ok(b) => {
                b.with_name("panoply-ocr").commit();
                Ok(())
            }
            Err(e) => Err(format!(
                "The ONNX Runtime at {} could not be loaded ({e}). Delete it and Panoply will download it again.",
                path.display()
            )),
        }
    });
    r.clone().map_err(OcrError::Engine)
}

fn build_session(path: &std::path::Path, threads: usize) -> Result<Session> {
    Session::builder()
        .map_err(|e| ort_err("starting", e))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| ort_err("starting", e))?
        .with_intra_threads(threads)
        .map_err(|e| ort_err("starting", e))?
        .with_memory_pattern(false)
        .map_err(|e| ort_err("starting", e))?
        .commit_from_file(path)
        .map_err(|e| {
            ort_err(
                &format!(
                    "loading {}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
                e,
            )
        })
}

/// The PP-OCR engine. It holds only settings; the sessions live in a process wide cache that
/// is unloaded when idle.
#[derive(Debug, Clone, Default)]
pub struct PpOcr {
    /// How it is run.
    pub settings: Settings,
}

impl PpOcr {
    /// True when PP-OCR has a recognizer for every one of these languages.
    pub fn covers(langs: &[Lang]) -> bool {
        Recognizer::for_langs(langs).is_some()
    }

    /// Read one page image.
    pub fn recognize(
        &self,
        img: &OcrImage,
        langs: &[Lang],
        fetch: &mut Fetch<'_>,
        cancel: &CancelToken,
    ) -> Result<OcrPage> {
        let rec_kind = Recognizer::for_langs(langs).ok_or_else(|| {
            OcrError::Unsupported(
                "The built-in text recognizer cannot read this combination of languages.".into(),
            )
        })?;
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let det_path = models::ensure(&models::DET, fetch)?;
        let rec_path = models::ensure(rec_kind.asset(), fetch)?;
        init_runtime(fetch)?;
        let mut guard = SLOT.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            let det = build_session(&det_path, self.settings.threads)?;
            *guard = Some(Loaded {
                det,
                recs: HashMap::new(),
                last_used: Instant::now(),
            });
        }
        let loaded = guard.as_mut().expect("loaded just above");
        if let std::collections::hash_map::Entry::Vacant(slot) = loaded.recs.entry(rec_kind) {
            let session = build_session(&rec_path, self.settings.threads)?;
            let classes = output_classes(&session)?;
            let dict = ctc::Dict::new(rec_kind.dict_text(), classes)?;
            slot.insert((session, dict));
        }
        let result = self.run(loaded, rec_kind, img, langs, cancel);
        loaded.last_used = Instant::now();
        drop(guard);
        ensure_reaper();
        result
    }

    /// Find the text lines on a page image, without reading them. The rectangles are in pixels of
    /// `img`, each with the direction of its line. Used by engines that read lines but cannot
    /// find them as reliably (Tesseract on small pictures).
    pub fn detect(
        &self,
        img: &OcrImage,
        fetch: &mut Fetch<'_>,
        cancel: &CancelToken,
    ) -> Result<Vec<Oriented>> {
        let det_path = models::ensure(&models::DET, fetch)?;
        init_runtime(fetch)?;
        let mut guard = SLOT.lock().unwrap_or_else(|p| p.into_inner());
        if guard.is_none() {
            let det = build_session(&det_path, self.settings.threads)?;
            *guard = Some(Loaded {
                det,
                recs: HashMap::new(),
                last_used: Instant::now(),
            });
        }
        let loaded = guard.as_mut().expect("loaded just above");
        let r = self.detect_rects(loaded, img, cancel);
        loaded.last_used = Instant::now();
        drop(guard);
        ensure_reaper();
        r
    }

    fn detect_rects(
        &self,
        loaded: &mut Loaded,
        img: &OcrImage,
        cancel: &CancelToken,
    ) -> Result<Vec<Oriented>> {
        let s = &self.settings;
        let (iw, ih) = (img.width as usize, img.height as usize);
        // Detection size: the long side clamped to the configured range, both sides a
        // multiple of 32.
        let long = iw.max(ih) as f32;
        let target = long.clamp(
            s.min_long_side.min(s.max_long_side) as f32,
            s.max_long_side as f32,
        );
        let scale = target / long;
        let round32 = |v: f32| (((v / 32.0).round() as usize).max(1)) * 32;
        let (dw, dh) = (round32(iw as f32 * scale), round32(ih as f32 * scale));
        let trace = std::env::var_os("PANOPLY_OCR_TRACE").is_some();
        let t_det = Instant::now();
        let px = imgops::det_resize(img, dw, dh);
        let ch = usize::from(img.channels);
        // The page goes through the detector in horizontal strips with overlapping margins, and
        // the probability map is stitched from the strips' middles. Memory then follows the
        // strip, not the page: a full page at this size needs 500 MB or more of activations.
        let mut prob = vec![0f32; dw * dh];
        let in_name = loaded.det.inputs()[0].name().to_string();
        let mut y0 = 0usize;
        while y0 < dh {
            if cancel.is_cancelled() {
                return Err(OcrError::Cancelled);
            }
            let y1 = (y0 + STRIP_H).min(dh);
            // Rows kept from this strip: all of them at the page's own edges, and not the
            // margins, where the network sees a padded border, at the interior ones.
            let keep0 = if y0 == 0 { y0 } else { y0 + STRIP_MARGIN };
            let keep1 = if y1 == dh { y1 } else { y1 - STRIP_MARGIN };
            let input = imgops::det_tensor(&px, ch, dw, y0, y1);
            let tensor = Tensor::from_array(([1i64, 3, (y1 - y0) as i64, dw as i64], input))
                .map_err(|e| ort_err("preparing the page", e))?;
            let outputs = loaded
                .det
                .run(vec![(
                    std::borrow::Cow::from(in_name.clone()),
                    ort::session::SessionInputValue::from(tensor),
                )])
                .map_err(|e| ort_err("finding text", e))?;
            let (shape, data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| ort_err("finding text", e))?;
            if shape.num_elements() != dw * (y1 - y0) {
                return Err(OcrError::Engine(format!(
                    "The text detection model returned an unexpected size ({} values for {dw} x {}).",
                    shape.num_elements(),
                    y1 - y0
                )));
            }
            prob[keep0 * dw..keep1 * dw]
                .copy_from_slice(&data[(keep0 - y0) * dw..(keep1 - y0) * dw]);
            if y1 == dh {
                break;
            }
            y0 += STRIP_H - 2 * STRIP_MARGIN;
        }
        drop(px);
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let found = db::boxes(&prob, dw, dh, &s.db);
        drop(prob);
        if trace {
            eprintln!(
                "ocr trace: detection {dw}x{dh} {} boxes in {:?}",
                found.len(),
                t_det.elapsed()
            );
        }
        // Map-space rectangles back to the page image.
        let (sx, sy) = (iw as f32 / dw as f32, ih as f32 / dh as f32);
        let sm = (sx + sy) / 2.0;
        Ok(found
            .iter()
            .map(|b| Oriented {
                cx: b.rect.cx * sx,
                cy: b.rect.cy * sy,
                w: b.rect.w * sm,
                h: b.rect.h * sm,
                angle: b.rect.angle,
            })
            .collect())
    }

    fn run(
        &self,
        loaded: &mut Loaded,
        rec_kind: Recognizer,
        img: &OcrImage,
        langs: &[Lang],
        cancel: &CancelToken,
    ) -> Result<OcrPage> {
        let s = &self.settings;
        let trace = std::env::var_os("PANOPLY_OCR_TRACE").is_some();
        let rects = self.detect_rects(loaded, img, cancel)?;
        let t_rec = Instant::now();
        let (rec, dict) = loaded
            .recs
            .get_mut(&rec_kind)
            .map(|(r, d)| (r, &*d))
            .expect("recognizer loaded by the caller");
        let rec_in = rec.inputs()[0].name().to_string();
        // Every piece to read, with the width the recognizer will see it at. Pieces of similar
        // width are read together in one batch, which keeps the runtime's threads busy.
        let mut jobs: Vec<(usize, imgops::Oriented, usize)> = Vec::new();
        for rect in &rects {
            for piece in split_long(rect) {
                let ratio = piece.w / piece.h.max(1.0);
                let out_w = ((REC_H as f32 * ratio).ceil() as usize).clamp(16, REC_MAX_W);
                jobs.push((jobs.len(), piece, out_w));
            }
        }
        jobs.sort_by_key(|j| j.2);
        let mut slots: Vec<Option<Line>> = vec![None; jobs.len()];
        let mut at = 0;
        while at < jobs.len() {
            if cancel.is_cancelled() {
                return Err(OcrError::Cancelled);
            }
            let mut end = at + 1;
            while end < jobs.len()
                && end - at < BATCH_MAX_LINES
                && (end - at + 1) * jobs[end].2 <= BATCH_MAX_PIXELS
            {
                end += 1;
            }
            let batch = &jobs[at..end];
            let read = read_batch(rec, &rec_in, dict, img, batch, s.drop_score)?;
            for ((order, _, _), line) in batch.iter().zip(read) {
                slots[*order] = line;
            }
            at = end;
        }
        let lines: Vec<Line> = slots.into_iter().flatten().collect();
        if trace {
            eprintln!(
                "ocr trace: recognition of {} lines in {:?}",
                lines.len(),
                t_rec.elapsed()
            );
        }
        let lines = crate::layout::reading_order(merge_pieces(lines));
        Ok(OcrPage {
            width: img.width,
            height: img.height,
            lines,
            engine: EngineKind::PpOcr,
            languages: langs.to_vec(),
        })
    }
}

fn output_classes(session: &Session) -> Result<usize> {
    let out = session
        .outputs()
        .first()
        .ok_or_else(|| OcrError::Engine("The text recognition model has no output.".into()))?;
    // The class count is the last dimension of [batch, steps, classes].
    out.dtype()
        .tensor_shape()
        .and_then(|s| s.last().copied())
        .and_then(|c| usize::try_from(c).ok())
        .filter(|&c| c > 1)
        .ok_or_else(|| {
            OcrError::Engine("The text recognition model has an unexpected output.".into())
        })
}

/// Detector strip height and the rows at each interior edge that are dropped (both multiples of
/// 32). The margin is wider than the tallest line the detector handles at these sizes.
const STRIP_H: usize = 384;
const STRIP_MARGIN: usize = 64;

/// Recognizer input height.
const REC_H: usize = 48;
/// Widest recognizer input. Longer lines are cut into equal pieces.
const REC_MAX_W: usize = 3000;

/// A rectangle that would be wider than the recognizer takes, as equal pieces along its length.
fn split_long(r: &imgops::Oriented) -> Vec<imgops::Oriented> {
    let want = (REC_H as f32 * r.w / r.h.max(1.0)).ceil() as usize;
    let n = want.div_ceil(REC_MAX_W).max(1);
    if n == 1 {
        return vec![*r];
    }
    let (s, c) = r.angle.sin_cos();
    let len = r.w / n as f32;
    (0..n)
        .map(|i| {
            let off = (i as f32 + 0.5) * len - r.w / 2.0;
            imgops::Oriented {
                cx: r.cx + off * c,
                cy: r.cy + off * s,
                w: len,
                h: r.h,
                angle: r.angle,
            }
        })
        .collect()
}

/// Pieces of one long line come back as separate lines: join the ones that sit on the same
/// baseline and touch.
fn merge_pieces(lines: Vec<Line>) -> Vec<Line> {
    // Pieces are only ever made for lines wider than 62 recognizer-heights, and come out
    // adjacent in order, so joining neighbours that overlap vertically and abut horizontally
    // restores them without touching ordinary neighbouring columns (which have a gutter).
    let mut out: Vec<Line> = Vec::new();
    for l in lines {
        if let Some(prev) = out.last_mut()
            && prev.bbox.v_overlap(&l.bbox) > 0.8 * prev.bbox.h.min(l.bbox.h)
            && (l.bbox.x - prev.bbox.right()).abs() < 0.5 * prev.bbox.h
            && l.bbox.x >= prev.bbox.x
        {
            prev.words.extend(l.words);
            prev.bbox = prev.bbox.union(&l.bbox);
            continue;
        }
        out.push(l);
    }
    out
}

/// Most lines read in one batch, and most columns (batch size times the widest line's width).
/// One line at a time measured fastest on page sized inputs (batches of 3 and 8 were slower: the
/// padding to the widest line costs more than the larger runs save), so batching stays off; the
/// code is kept because narrow lines (screenshots, tables) may want it.
const BATCH_MAX_LINES: usize = 1;
const BATCH_MAX_PIXELS: usize = 9_000;

/// Read a batch of lines: crops padded on the right to the widest one (with the value the
/// network sees as mid gray, which is what PaddleOCR pads with), one run, and each line decoded
/// over its own share of the output steps.
fn read_batch(
    rec: &mut Session,
    input_name: &str,
    dict: &ctc::Dict,
    img: &OcrImage,
    batch: &[(usize, imgops::Oriented, usize)],
    drop_score: f32,
) -> Result<Vec<Option<Line>>> {
    let max_w = batch.iter().map(|j| j.2).max().unwrap_or(16);
    let plane = REC_H * max_w;
    let mut data = vec![0f32; batch.len() * 3 * plane];
    for (b, (_, rect, out_w)) in batch.iter().enumerate() {
        let crop = imgops::rec_tensor(img, rect, REC_H, *out_w);
        for c in 0..3 {
            for y in 0..REC_H {
                let src = &crop[(c * REC_H + y) * out_w..(c * REC_H + y + 1) * out_w];
                let dst = (b * 3 + c) * plane + y * max_w;
                data[dst..dst + out_w].copy_from_slice(src);
            }
        }
    }
    let tensor = Tensor::from_array(([batch.len() as i64, 3, REC_H as i64, max_w as i64], data))
        .map_err(|e| ort_err("preparing lines", e))?;
    let outputs = rec
        .run(vec![(
            std::borrow::Cow::from(input_name.to_string()),
            ort::session::SessionInputValue::from(tensor),
        )])
        .map_err(|e| ort_err("reading lines", e))?;
    let (shape, out) = outputs[0]
        .try_extract_tensor::<f32>()
        .map_err(|e| ort_err("reading lines", e))?;
    let dims: Vec<i64> = shape.iter().copied().collect();
    if dims.len() != 3 || dims[0] as usize != batch.len() {
        return Err(OcrError::Engine(
            "The text recognition model returned an unexpected shape.".into(),
        ));
    }
    let (steps_all, classes) = (dims[1] as usize, dims[2] as usize);
    if classes != dict.len() {
        return Err(OcrError::Engine(
            "The text recognition model does not match its dictionary.".into(),
        ));
    }
    let mut lines = Vec::with_capacity(batch.len());
    for (b, (_, rect, out_w)) in batch.iter().enumerate() {
        // The steps that belong to this line's own width.
        let steps = ((steps_all * out_w).div_ceil(max_w)).clamp(1, steps_all);
        let rows = &out[b * steps_all * classes..b * steps_all * classes + steps * classes];
        lines.push(line_from(rect, rows, steps, classes, dict, drop_score));
    }
    Ok(lines)
}

/// One decoded line as words with boxes in the page image.
fn line_from(
    rect: &imgops::Oriented,
    rows: &[f32],
    steps: usize,
    classes: usize,
    dict: &ctc::Dict,
    drop_score: f32,
) -> Option<Line> {
    let emits = ctc::decode(rows, steps, classes, dict);
    let spans = ctc::words(&emits, steps);
    if spans.is_empty() {
        return None;
    }
    let words: Vec<Word> = spans
        .iter()
        .map(|sp| {
            let u0 = sp.start / steps as f32 * rect.w - rect.w / 2.0;
            let u1 = sp.end / steps as f32 * rect.w - rect.w / 2.0;
            let (x, y, w, h) = imgops::span_bounds(rect, u0, u1);
            Word {
                text: sp.text.clone(),
                bbox: Rect { x, y, w, h },
                confidence: sp.confidence,
            }
        })
        .collect();
    let line = Line::from_words(words).expect("a line with spans has words");
    // Line score as PaddleOCR takes it: mean symbol probability.
    let n: usize = spans.iter().map(|w| w.text.chars().count()).sum();
    let score = spans
        .iter()
        .map(|w| w.confidence * w.text.chars().count() as f32)
        .sum::<f32>()
        / n.max(1) as f32;
    (score >= drop_score).then_some(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(codes: &str) -> Vec<Lang> {
        Lang::parse_list(codes).unwrap()
    }

    #[test]
    fn recognizers_follow_the_script() {
        assert_eq!(Recognizer::for_langs(&[]), Some(Recognizer::English));
        assert_eq!(Recognizer::for_langs(&l("en")), Some(Recognizer::English));
        assert_eq!(Recognizer::for_langs(&l("fr,en")), Some(Recognizer::Latin));
        assert_eq!(
            Recognizer::for_langs(&l("hi")),
            Some(Recognizer::Devanagari)
        );
        assert_eq!(
            Recognizer::for_langs(&l("hi,en")),
            Some(Recognizer::Devanagari)
        );
        assert_eq!(
            Recognizer::for_langs(&l("mr")),
            Some(Recognizer::Devanagari)
        );
        // No single PP-OCR model reads these: the ladder looks elsewhere.
        assert_eq!(Recognizer::for_langs(&l("bn")), None);
        assert_eq!(Recognizer::for_langs(&l("hi,bn")), None);
        assert_eq!(Recognizer::for_langs(&l("ja")), None);
    }

    #[test]
    fn the_embedded_dictionaries_have_the_pinned_models_sizes() {
        for (r, n) in [
            (Recognizer::English, 436),
            (Recognizer::Latin, 836),
            (Recognizer::Devanagari, 568),
        ] {
            assert_eq!(r.dict_text().lines().count(), n, "{r:?}");
            assert!(
                r.dict_text().lines().all(|l| l.chars().count() == 1),
                "{r:?}"
            );
        }
    }

    #[test]
    fn long_lines_are_cut_into_equal_pieces() {
        let r = imgops::Oriented {
            cx: 5000.0,
            cy: 100.0,
            w: 10000.0,
            h: 40.0,
            angle: 0.0,
        };
        let p = split_long(&r);
        // 10000 / 40 * 48 = 12000 wide at the recognizer height: four pieces of 3000.
        assert_eq!(p.len(), 4);
        assert!((p[0].cx - 1250.0).abs() < 1e-3 && (p[3].cx - 8750.0).abs() < 1e-3);
        assert!(p.iter().all(|q| (q.w - 2500.0).abs() < 1e-3));
        let short = imgops::Oriented { w: 400.0, ..r };
        assert_eq!(split_long(&short).len(), 1);
    }
}
