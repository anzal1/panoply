//! Plain words to intent. "make this under 50 kb", "heic to jpg", "shrink pdf".
//!
//! Small on purpose: three things are understood (a size target, a conversion, and search words
//! with a few everyday synonyms). Anything else is ordinary fuzzy search over tool names.

use crate::units::parse_size;

/// A size the result should fit under.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeTarget {
    /// The size in bytes.
    pub bytes: u64,
    /// The words that said it, as typed: "under 50 kb".
    pub phrase: String,
}

/// A conversion asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Convert {
    /// Source extension, when the query names one.
    pub from: Option<String>,
    /// Target extension.
    pub to: String,
}

/// What a query means.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Intent {
    /// A size target.
    pub size: Option<SizeTarget>,
    /// A conversion.
    pub convert: Option<Convert>,
    /// Search words left after the intent phrases and filler are removed, synonyms applied.
    pub words: Vec<String>,
}

const STOP: &[&str] = &[
    "a", "an", "the", "this", "that", "it", "my", "me", "file", "files", "please", "make", "i",
    "want", "need", "can", "you", "of", "for", "with", "and", "just", "photo's",
];
const QUALIFIERS: &[&str] = &[
    "under", "below", "less", "than", "smaller", "max", "maximum", "most", "within", "upto", "up",
    "to", "at", "around", "about", "below",
];
const UNITS: &[&str] = &["b", "k", "kb", "m", "mb", "g", "gb"];

fn synonym(w: &str) -> &str {
    match w {
        "smaller" | "shrink" | "reduce" | "lighter" | "slim" | "compressed" => "compress",
        "combine" | "join" | "stitch" | "merged" => "merge",
        "cut" | "divide" | "separate" | "break" => "split",
        "photo" | "photos" | "picture" | "pictures" | "pic" | "pics" => "image",
        "rotate" | "turn" => "rotate",
        other => other,
    }
}

/// True for an extension the conversion graph knows.
fn is_format(ext: &str) -> bool {
    !ext.is_empty()
        && panoply_convert::Graph::shared()
            .format_by_ext(ext)
            .is_some()
}

fn tokenize(query: &str) -> Vec<String> {
    let q = query
        .to_lowercase()
        .replace("->", " to ")
        .replace('\u{2192}', " to ");
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut last_digit: Option<bool> = None;
    for c in q.chars() {
        let digit = c.is_ascii_digit() || c == '.';
        if c.is_whitespace() || (c.is_ascii_punctuation() && c != '.' && c != '\'') {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            last_digit = None;
            continue;
        }
        if let Some(ld) = last_digit
            && ld != digit
            && !cur.is_empty()
        {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
        last_digit = Some(digit);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// True while a query is plainly still being typed: only qualifier words, numbers and sizes so far
/// ("under", "under 5", "under 50 k"), or a conversion that has no target yet ("heic to"). Such a
/// query has nothing to match until the next keystroke, and is not an answer of "no tool".
pub fn is_unfinished(query: &str) -> bool {
    let t = tokenize(query);
    let Some(last) = t.last() else {
        return false;
    };
    if t.len() >= 2 && matches!(last.as_str(), "to" | "into") {
        return true;
    }
    const BIG_UNITS: &[&str] = &["bytes", "kilobytes", "megabytes", "gigabytes"];
    t.iter().all(|w| {
        let numeric = w.chars().all(|c| c.is_ascii_digit() || c == '.')
            && w.chars().any(|c| c.is_ascii_digit());
        numeric
            || STOP.iter().any(|s| s.starts_with(w.as_str()))
            || QUALIFIERS.iter().any(|q| q.starts_with(w.as_str()))
            || UNITS.contains(&w.as_str())
            || BIG_UNITS.iter().any(|u| u.starts_with(w.as_str()))
    })
}

/// Understand a query.
pub fn parse(query: &str) -> Intent {
    let tokens = tokenize(query);
    let mut used = vec![false; tokens.len()];
    let mut intent = Intent::default();

    // A size: a number then a unit, with the qualifier words just before it.
    for i in 0..tokens.len().saturating_sub(1) {
        let is_num = tokens[i].chars().all(|c| c.is_ascii_digit() || c == '.')
            && tokens[i].chars().any(|c| c.is_ascii_digit());
        if is_num && UNITS.contains(&tokens[i + 1].as_str()) {
            let text = format!("{}{}", tokens[i], tokens[i + 1]);
            let Some(bytes) = parse_size(&text) else {
                continue;
            };
            let mut start = i;
            while start > 0 && QUALIFIERS.contains(&tokens[start - 1].as_str()) && i - start < 3 {
                start -= 1;
            }
            for u in &mut used[start..=i + 1] {
                *u = true;
            }
            intent.size = Some(SizeTarget {
                bytes,
                phrase: tokens[start..=i + 1].join(" "),
            });
            break;
        }
    }

    // A conversion: "<from> to <to>", "to <to>", "as <to>", "into <to>".
    for i in 0..tokens.len() {
        if used[i] || !matches!(tokens[i].as_str(), "to" | "into" | "as") {
            continue;
        }
        let Some(to) = tokens.get(i + 1).filter(|t| !used[i + 1] && is_format(t)) else {
            continue;
        };
        let from = i
            .checked_sub(1)
            .filter(|&p| !used[p] && is_format(&tokens[p]))
            .map(|p| tokens[p].clone());
        used[i] = true;
        used[i + 1] = true;
        if let Some(p) = i.checked_sub(1).filter(|_| from.is_some()) {
            used[p] = true;
        }
        intent.convert = Some(Convert {
            from,
            to: to.clone(),
        });
        break;
    }

    intent.words = tokens
        .iter()
        .enumerate()
        .filter(|(i, t)| !used[*i] && !STOP.contains(&t.as_str()))
        .map(|(_, t)| synonym(t).to_string())
        .collect();
    intent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_target_in_plain_words() {
        let i = parse("make this under 50 kb");
        let s = i.size.unwrap();
        assert_eq!(s.bytes, 50_000);
        assert_eq!(s.phrase, "under 50 kb");
        assert!(i.words.is_empty());
        let i = parse("compress to 2mb");
        assert_eq!(i.size.unwrap().bytes, 2_000_000);
        assert_eq!(i.words, vec!["compress"]);
        assert_eq!(
            parse("pdf smaller than 1.5 MB").size.unwrap().bytes,
            1_500_000
        );
    }

    #[test]
    fn a_conversion_with_and_without_a_source() {
        let c = parse("heic to jpg").convert.unwrap();
        assert_eq!(
            c,
            Convert {
                from: Some("heic".into()),
                to: "jpg".into()
            }
        );
        let c = parse("convert to png").convert.unwrap();
        assert_eq!(
            c,
            Convert {
                from: None,
                to: "png".into()
            }
        );
        assert_eq!(parse("docx -> pdf").convert.unwrap().to, "pdf");
        assert_eq!(parse("make it as webp").convert.unwrap().to, "webp");
    }

    #[test]
    fn to_before_a_size_is_not_a_conversion() {
        let i = parse("resize to 50kb");
        assert!(i.convert.is_none());
        assert_eq!(i.size.unwrap().bytes, 50_000);
    }

    #[test]
    fn synonyms_and_filler() {
        assert_eq!(parse("shrink my photos").words, vec!["compress", "image"]);
        assert_eq!(parse("join pdfs").words, vec!["merge", "pdfs"]);
        assert_eq!(parse("").words, Vec::<String>::new());
    }

    #[test]
    fn unknown_formats_do_not_convert() {
        assert!(parse("heic to banana").convert.is_none());
    }
}
