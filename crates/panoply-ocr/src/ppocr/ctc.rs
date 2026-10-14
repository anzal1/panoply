//! The recognizer's output: CTC decoding and word positions.
//!
//! The network gives, for each of `t` time steps along the crop, a probability for every
//! symbol (index 0 is the blank). Greedy decoding takes the best symbol per step, drops
//! repeats and blanks, and the steps where symbols were emitted say where along the crop each
//! symbol is, which is what gives word boxes without a second model.

use crate::error::{OcrError, Result};

/// The symbol list of a recognizer: blank first, then the dictionary, then a space.
#[derive(Debug, Clone)]
pub struct Dict {
    symbols: Vec<String>,
}

impl Dict {
    /// Build from the dictionary file text (one symbol per line) and the number of output
    /// classes the model has. PaddleOCR's list is `blank + dictionary (+ space)`, so a model
    /// with one class more than `dictionary + blank` has the space appended.
    pub fn new(text: &str, classes: usize) -> Result<Dict> {
        let mut symbols: Vec<String> = vec![String::new()];
        symbols.extend(text.lines().map(str::to_string));
        match classes.checked_sub(symbols.len()) {
            Some(0) => {}
            Some(1) => symbols.push(" ".into()),
            _ => {
                return Err(OcrError::Engine(format!(
                    "The text recognition model does not match its dictionary ({classes} outputs, {} symbols).",
                    symbols.len()
                )));
            }
        }
        Ok(Dict { symbols })
    }

    /// The symbol for a class index.
    pub fn get(&self, i: usize) -> Option<&str> {
        self.symbols.get(i).map(String::as_str)
    }

    /// Number of symbols including the blank (so never zero).
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.symbols.len()
    }
}

/// One emitted symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct Emit {
    /// The symbol text (a letter, a mark, or a space).
    pub text: String,
    /// The time step it was emitted at (the first of a run).
    pub step: usize,
    /// Its probability.
    pub prob: f32,
}

/// Greedy CTC decoding of `steps x classes` outputs. If the values are not already
/// probabilities (rows summing to 1) they are passed through a softmax first.
pub fn decode(out: &[f32], steps: usize, classes: usize, dict: &Dict) -> Vec<Emit> {
    debug_assert_eq!(out.len(), steps * classes);
    let probs = looks_like_probabilities(out, steps, classes);
    let mut emits = Vec::new();
    let mut prev = 0usize;
    for t in 0..steps {
        let row = &out[t * classes..(t + 1) * classes];
        let (mut best, mut bv) = (0usize, f32::MIN);
        for (i, &v) in row.iter().enumerate() {
            if v > bv {
                bv = v;
                best = i;
            }
        }
        let p = if probs { bv } else { softmax_at(row, best) };
        if best != 0
            && best != prev
            && let Some(sym) = dict.get(best)
        {
            emits.push(Emit {
                text: sym.to_string(),
                step: t,
                prob: p,
            });
        }
        prev = best;
    }
    emits
}

fn looks_like_probabilities(out: &[f32], steps: usize, classes: usize) -> bool {
    if steps == 0 {
        return true;
    }
    let row = &out[..classes];
    let sum: f32 = row.iter().sum();
    row.iter().all(|&v| (0.0..=1.0001).contains(&v)) && (sum - 1.0).abs() < 0.01
}

fn softmax_at(row: &[f32], i: usize) -> f32 {
    let m = row.iter().copied().fold(f32::MIN, f32::max);
    let sum: f32 = row.iter().map(|&v| (v - m).exp()).sum();
    (row[i] - m).exp() / sum
}

/// A word found in a decoded line: its text, where it sits along the crop (in time steps,
/// `start..end`) and its confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct WordSpan {
    /// The word.
    pub text: String,
    /// First step, a fraction of a step from the left edge of the symbols.
    pub start: f32,
    /// One past the last step.
    pub end: f32,
    /// Mean probability of its symbols.
    pub confidence: f32,
}

/// Split a line's emitted symbols at spaces into words with spans.
///
/// A symbol emitted at step `t` covers about one symbol width around `t + 0.5`; the width is
/// the line's median spacing between symbols, so a word's box runs from half a symbol before
/// its first symbol to half a symbol after its last, and boxes of neighbouring words never
/// cross.
pub fn words(emits: &[Emit], steps: usize) -> Vec<WordSpan> {
    let glyphs: Vec<&Emit> = emits.iter().filter(|e| e.text != " ").collect();
    if glyphs.is_empty() {
        return Vec::new();
    }
    let mut gaps: Vec<f32> = glyphs
        .windows(2)
        .map(|w| (w[1].step - w[0].step) as f32)
        .filter(|&g| g > 0.0)
        .collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // The median gap between neighbouring symbols, limited to a sane range of the line width.
    let stride = gaps
        .get(gaps.len().saturating_sub(1) / 2)
        .copied()
        .unwrap_or(steps as f32 / glyphs.len().max(1) as f32)
        .clamp(1.0, (steps as f32 / 2.0).max(1.0));
    let mut out: Vec<WordSpan> = Vec::new();
    let mut cur: Vec<&Emit> = Vec::new();
    let flush = |cur: &mut Vec<&Emit>, out: &mut Vec<WordSpan>| {
        if cur.is_empty() {
            return;
        }
        let text: String = cur.iter().map(|e| e.text.as_str()).collect();
        let first = cur[0].step as f32 + 0.5 - 0.5 * stride;
        let last = cur[cur.len() - 1].step as f32 + 0.5 + 0.5 * stride;
        let confidence = cur.iter().map(|e| e.prob).sum::<f32>() / cur.len() as f32;
        out.push(WordSpan {
            text,
            start: first.max(0.0),
            end: last.min(steps as f32),
            confidence,
        });
        cur.clear();
    };
    for e in emits {
        if e.text == " " {
            flush(&mut cur, &mut out);
        } else {
            cur.push(e);
        }
    }
    flush(&mut cur, &mut out);
    // Neighbouring words must not overlap: split any overlap at the midpoint.
    for i in 1..out.len() {
        if out[i].start < out[i - 1].end {
            let mid = (out[i].start + out[i - 1].end) / 2.0;
            out[i - 1].end = mid;
            out[i].start = mid;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dict {
        // blank, a, b, c, then the appended space: 5 classes.
        Dict::new("a\nb\nc\n", 5).unwrap()
    }

    fn row(i: usize, p: f32, classes: usize) -> Vec<f32> {
        let mut r = vec![(1.0 - p) / (classes - 1) as f32; classes];
        r[i] = p;
        r
    }

    #[test]
    fn the_space_is_appended_when_the_model_has_one_more_class() {
        let d = dict();
        assert_eq!(d.len(), 5);
        assert_eq!(d.get(4), Some(" "));
        assert!(Dict::new("a\nb\n", 9).is_err());
        let no_space = Dict::new("a\nb\n", 3).unwrap();
        assert_eq!(no_space.len(), 3);
    }

    #[test]
    fn repeats_and_blanks_collapse() {
        // a a blank a b b blank c -> "a", "a", "b", "c" (blank between repeats separates them).
        let seq = [1, 1, 0, 1, 2, 2, 0, 3];
        let flat: Vec<f32> = seq.iter().flat_map(|&i| row(i, 0.9, 5)).collect();
        let e = decode(&flat, seq.len(), 5, &dict());
        let s: String = e.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(s, "aabc");
        assert_eq!(e[0].step, 0);
        assert_eq!(e[1].step, 3);
        assert!((e[0].prob - 0.9).abs() < 1e-5);
    }

    #[test]
    fn logits_are_softmaxed() {
        // Raw scores, not probabilities: class 1 clearly wins.
        let flat = vec![0.0, 5.0, 0.0, 0.0, 0.0];
        let e = decode(&flat, 1, 5, &dict());
        assert_eq!(e.len(), 1);
        assert!(e[0].prob > 0.9 && e[0].prob < 1.0, "{}", e[0].prob);
    }

    #[test]
    fn words_split_at_spaces_with_spans_and_confidence() {
        // ab c : a@2 b@4 space@6 c@8, 12 steps.
        let mk = |t: &str, step, prob| Emit {
            text: t.into(),
            step,
            prob,
        };
        let e = vec![
            mk("a", 2, 0.9),
            mk("b", 4, 0.7),
            mk(" ", 6, 0.99),
            mk("c", 8, 0.8),
        ];
        let w = words(&e, 12);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].text, "ab");
        assert!((w[0].confidence - 0.8).abs() < 1e-5);
        assert_eq!(w[1].text, "c");
        // Stride 2: "ab" runs from 2.5 - 1 to 4.5 + 1, and "c" from 8.5 - 1 to 8.5 + 1.
        assert!((w[0].start - 1.5).abs() < 1e-5 && (w[0].end - 5.5).abs() < 1e-5);
        assert!((w[1].start - 7.5).abs() < 1e-5 && (w[1].end - 9.5).abs() < 1e-5);
        assert!(w[0].end <= w[1].start);
    }

    #[test]
    fn a_line_with_no_symbols_has_no_words() {
        assert!(words(&[], 10).is_empty());
    }
}
