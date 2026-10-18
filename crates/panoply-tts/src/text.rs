//! Getting speakable text: plain text, Markdown with the markup removed, text from a PDF's own
//! text layer, and splitting long text into chunks of whole sentences.

use std::fs::File;
use std::path::Path;

use panoply_core::CancelToken;

use crate::error::{Result, TtsError};

/// The most characters sent to the voice in one piece. Apple and Windows both speak longer
/// text, but a chunk this size gives progress and a cancel point every few seconds of audio.
pub const MAX_CHUNK: usize = 500;

/// Read the text of a `.txt`, `.md` or `.pdf` file. Markdown loses its markup, a PDF gives the
/// text of every page that has a text layer, in page order.
pub fn read_file(path: &Path, cancel: &CancelToken) -> Result<String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => pdf_text(File::open(path)?, cancel),
        "md" | "markdown" => Ok(plain_from_markdown(&read_utf8(path)?)),
        "txt" | "text" | "" => read_utf8(path),
        other => Err(TtsError::BadInput(format!(
            "A .{other} file cannot be read aloud. Choose a .txt, .md or .pdf file, or paste the text."
        ))),
    }
}

fn read_utf8(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        TtsError::BadInput("This file is not UTF-8 text. Save it as UTF-8 and try again.".into())
    })
}

fn pdf_text(file: File, cancel: &CancelToken) -> Result<String> {
    let engine = panoply_pdf::pdfium::engine().map_err(|e| TtsError::Engine(e.to_string()))?;
    let _guard = panoply_pdf::pdfium::lock();
    let doc = engine.load_pdf_from_reader(file, None).map_err(|e| {
        TtsError::BadInput(format!("The PDF engine could not open this file ({e:?})."))
    })?;
    let mut out = String::new();
    for page in doc.pages().iter() {
        if cancel.is_cancelled() {
            return Err(TtsError::Cancelled);
        }
        if let Ok(text) = page.text() {
            out.push_str(text.all().trim());
            out.push_str("\n\n");
        }
    }
    if out.trim().is_empty() {
        return Err(TtsError::BadInput(
            "This PDF has no text layer to read. Run PDF OCR on it first.".into(),
        ));
    }
    Ok(out)
}

/// Markdown as the words a person would read: headings and list marks lose their symbols,
/// emphasis and code marks go, links keep their text, images and code blocks are dropped.
pub fn plain_from_markdown(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut in_fence = false;
    for raw in md.lines() {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let mut l = line.trim_start_matches('#').trim_start();
        let is_heading = line.starts_with('#');
        l = l.trim_start_matches("> ").trim_start_matches('>');
        let mut is_item = false;
        for mark in ["- ", "* ", "+ "] {
            if let Some(rest) = l.strip_prefix(mark) {
                l = rest;
                is_item = true;
                break;
            }
        }
        // A table row reads as its cells with commas, and the divider row not at all.
        let table_row;
        if line.starts_with('|') {
            if line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
                continue;
            }
            table_row = line
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(", ");
            l = &table_row;
            is_item = true;
        }
        let l = strip_inline(l);
        if l.is_empty() {
            out.push('\n');
            continue;
        }
        out.push_str(&l);
        // A heading, a list item or a table row is read as its own sentence.
        if (is_heading || is_item) && !l.ends_with(['.', '!', '?', ':']) {
            out.push('.');
        }
        out.push('\n');
    }
    out
}

fn strip_inline(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // ![alt](url) is dropped, [text](url) keeps its text.
        if (c == '!' && chars.get(i + 1) == Some(&'[')) || c == '[' {
            let start = if c == '!' { i + 2 } else { i + 1 };
            if let Some(close) = chars[start..].iter().position(|&x| x == ']') {
                let close = start + close;
                if chars.get(close + 1) == Some(&'(')
                    && let Some(end) = chars[close + 2..].iter().position(|&x| x == ')')
                {
                    if c == '[' {
                        out.extend(&chars[start..close]);
                    }
                    i = close + 2 + end + 1;
                    continue;
                }
            }
        }
        // An HTML tag (<br>, </p>, <img src=...>) is dropped; a lone less-than sign stays.
        if c == '<'
            && chars
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!'))
            && let Some(end) = chars[i..].iter().position(|&x| x == '>')
        {
            i += end + 1;
            out.push(' ');
            continue;
        }
        if !matches!(c, '*' | '`' | '~') && !(c == '_' && is_emphasis_underscore(&chars, i)) {
            out.push(c);
        }
        i += 1;
    }
    out.trim().to_string()
}

fn is_emphasis_underscore(chars: &[char], i: usize) -> bool {
    let before = i.checked_sub(1).and_then(|j| chars.get(j));
    let after = chars.get(i + 1);
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric());
    // snake_case keeps its underscore (and is read as it is); _emphasis_ loses it.
    !(word(before) && word(after))
}

/// Paragraphs of a text: blank lines separate them, and the single line breaks inside one (a
/// wrapped PDF, hard wrapped prose) become spaces. A lone newline stays a break only after
/// sentence-ending punctuation, where the voice would pause anyway.
fn paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if !cur.is_empty() {
            if cur.ends_with(['.', '!', '?', '।', '…', ':']) {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.push(' ');
            }
        }
        cur.push_str(&line);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Split text into chunks of whole sentences, none longer than `max` characters unless one
/// word is. A paragraph break always ends a chunk, so a voice never runs two paragraphs
/// together. Wrapped lines inside a paragraph are one run of text.
pub fn chunks(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in paragraphs(text) {
        let mut cur = String::new();
        for sentence in sentences(&para) {
            if !cur.is_empty() && cur.chars().count() + 1 + sentence.chars().count() > max {
                out.push(std::mem::take(&mut cur));
            }
            if sentence.chars().count() > max {
                for piece in split_long(&sentence, max) {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                    out.push(piece);
                }
                continue;
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(&sentence);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out.into_iter()
        .filter(|c| c.chars().any(char::is_alphanumeric))
        .collect()
}

/// Sentences of one paragraph: after `.`, `!`, `?`, `।` or `…` followed by a space, unless the
/// word before the dot is a short abbreviation or a number.
fn sentences(para: &str) -> Vec<String> {
    let chars: Vec<char> = para.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    for i in 0..chars.len() {
        let c = chars[i];
        if !matches!(c, '.' | '!' | '?' | '।' | '…') {
            continue;
        }
        let at_end = i + 1 == chars.len();
        if !at_end && chars[i + 1] != ' ' {
            continue;
        }
        if c == '.' && !at_end && is_abbreviation(&chars[start..i]) {
            continue;
        }
        let s: String = chars[start..=i].iter().collect();
        out.push(s.trim().to_string());
        start = i + 1;
    }
    if start < chars.len() {
        let s: String = chars[start..].iter().collect();
        if !s.trim().is_empty() {
            out.push(s.trim().to_string());
        }
    }
    out
}

fn is_abbreviation(before: &[char]) -> bool {
    let word: String = before
        .iter()
        .rev()
        .take_while(|c| !c.is_whitespace())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let lower = word.to_lowercase();
    matches!(
        lower.as_str(),
        "mr" | "mrs"
            | "ms"
            | "dr"
            | "prof"
            | "sr"
            | "jr"
            | "st"
            | "vs"
            | "etc"
            | "e.g"
            | "i.e"
            | "no"
            | "a.m"
            | "p.m"
            | "u.s"
    ) || (word.chars().count() == 1 && word.chars().all(char::is_alphabetic))
}

fn split_long(sentence: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in sentence.split(' ') {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > max {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk_and_paragraphs_split() {
        assert_eq!(
            chunks("Hello there. How are you?", 500),
            ["Hello there. How are you?"]
        );
        assert_eq!(chunks("One.\n\nTwo.", 500), ["One.", "Two."]);
    }

    #[test]
    fn chunks_end_on_sentences_and_respect_the_limit() {
        let text = "Alpha beta gamma. Delta epsilon zeta. Eta theta iota.";
        let c = chunks(text, 30);
        assert_eq!(
            c,
            [
                "Alpha beta gamma.",
                "Delta epsilon zeta.",
                "Eta theta iota."
            ]
        );
        assert!(c.iter().all(|s| s.chars().count() <= 30));
    }

    #[test]
    fn abbreviations_and_initials_do_not_end_a_sentence() {
        let c = sentences("Dr. Rao met J. Smith at 5 p.m. today. Then left.");
        assert_eq!(c, ["Dr. Rao met J. Smith at 5 p.m. today.", "Then left."]);
    }

    #[test]
    fn one_huge_sentence_is_cut_at_words() {
        let text = "word ".repeat(300);
        let c = chunks(&text, 100);
        assert!(c.len() > 10 && c.iter().all(|s| s.chars().count() <= 100));
    }

    #[test]
    fn markdown_keeps_the_words() {
        let md = "# Title\n\nSome *bold* and `code` with a [link](http://x.y) and ![pic](a.png).\n\n```\nlet x = 1;\n```\n- item one\n- item_two\n";
        let t = plain_from_markdown(md);
        assert!(t.contains("Title."));
        assert!(t.contains("Some bold and code with a link and ."));
        assert!(!t.contains("let x"));
        assert!(t.contains("item one") && t.contains("item_two"));
    }

    #[test]
    fn wrapped_lines_are_one_paragraph_and_only_blank_lines_split() {
        let wrapped =
            "This sentence is wrapped\nacross three short\nlines and ends here.\n\nNext paragraph.";
        assert_eq!(
            chunks(wrapped, 500),
            [
                "This sentence is wrapped across three short lines and ends here.",
                "Next paragraph."
            ]
        );
        // A lone newline after a full stop stays a break.
        assert_eq!(chunks("One.\nTwo.", 500), ["One.", "Two."]);
        assert_eq!(paragraphs("a\nb\n\nc"), ["a b", "c"]);
    }

    #[test]
    fn markdown_tables_html_and_list_items_read_cleanly() {
        let md = "<p>Intro<br/>text</p>\n\n| Name | Age |\n|------|-----|\n| Ann | 30 |\n\n- first\n- second\n";
        let t = plain_from_markdown(md);
        assert!(
            !t.contains('<') && !t.contains('|') && !t.contains("---"),
            "{t}"
        );
        assert!(t.contains("Name, Age.") && t.contains("Ann, 30."), "{t}");
        let c = chunks(&t, 500);
        assert!(
            c.iter().any(|x| x.contains("first.")) && c.iter().any(|x| x.contains("second.")),
            "{c:?}"
        );
    }

    #[test]
    fn a_hindi_danda_ends_a_sentence() {
        let c = chunks("यह पहला वाक्य है। यह दूसरा वाक्य है।", 20);
        assert_eq!(c.len(), 2);
    }
}
