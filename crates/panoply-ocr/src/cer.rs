//! Character error rate, the measure the quality bars are written in.
//!
//! CER is the Levenshtein distance between the reference and the engine's text, counted in
//! Unicode scalar values, divided by the length of the reference. Both texts are normalized
//! first: every run of white space (including line breaks) becomes one space and the ends are
//! trimmed, because reading order inside a paragraph is not what is being judged. Combining
//! marks count as characters, which is how Tesseract's own evaluation counts Devanagari and
//! Bengali.

/// Collapse white space and trim.
pub fn normalize(s: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(s.len());
    let mut space = true;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

/// Levenshtein distance between two sequences.
pub fn edit_distance<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(x != y);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Character error rate of `hypothesis` against `reference`, as a fraction (0.02 is 2%).
/// An empty reference gives 0.0 for an empty hypothesis and 1.0 otherwise.
pub fn cer(reference: &str, hypothesis: &str) -> f64 {
    let (r, h) = (normalize(reference), normalize(hypothesis));
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    edit_distance(&r, &h) as f64 / r.len() as f64
}

/// Word error rate: the same on whitespace separated words.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    let r: Vec<&str> = reference.split_whitespace().collect();
    let h: Vec<&str> = hypothesis.split_whitespace().collect();
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    edit_distance(&r, &h) as f64 / r.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_has_no_error() {
        assert_eq!(cer("hello world", "hello  world\n"), 0.0);
    }

    #[test]
    fn one_substitution_in_ten_characters() {
        assert!((cer("abcdefghij", "abcdefghix") - 0.1).abs() < 1e-9);
    }

    #[test]
    fn insertions_and_deletions_count() {
        assert!((cer("abcd", "abd") - 0.25).abs() < 1e-9);
        assert!((cer("abcd", "abxcd") - 0.25).abs() < 1e-9);
    }

    #[test]
    fn line_breaks_do_not_count_but_missing_text_does() {
        assert_eq!(cer("one two\nthree", "one two three"), 0.0);
        assert_eq!(cer("one two", ""), 1.0);
    }

    #[test]
    fn indic_text_counts_scalar_values() {
        // One mark missing from a six scalar word.
        let r = "नमस्ते";
        assert_eq!(normalize(r).len(), 6);
        assert!((cer(r, "नमसते") - 1.0 / 6.0).abs() < 1e-9);
    }

    #[test]
    fn word_error_rate() {
        assert!((wer("a b c d", "a x c d") - 0.25).abs() < 1e-9);
    }
}
