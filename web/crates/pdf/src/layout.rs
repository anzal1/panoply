//! Reading order and text output: columns before rows (an XY cut), paragraphs from line
//! spacing, plain text and Markdown.

use crate::types::{Line, OcrPage};

/// Put lines in reading order: split the page at the widest whitespace band (a gap between
/// rows first, then a gutter between columns), and read each part in turn.
pub fn reading_order(lines: Vec<Line>) -> Vec<Line> {
    if lines.len() < 2 {
        return lines;
    }
    let mut hs: Vec<f32> = lines.iter().map(|l| l.bbox.h).collect();
    hs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = hs[hs.len() / 2].max(1.0);
    let mut out = Vec::with_capacity(lines.len());
    cut(lines, median, 0, &mut out);
    out
}

/// A gap between rows must be at least this many median line heights to split on.
const ROW_GAP: f32 = 0.9;
/// A gutter between columns must be at least this many median line heights wide.
const COL_GAP: f32 = 1.8;

fn cut(mut lines: Vec<Line>, median: f32, depth: usize, out: &mut Vec<Line>) {
    if lines.len() < 2 || depth > 16 {
        sort_rows(&mut lines);
        out.append(&mut lines);
        return;
    }
    // Rows first: the widest empty band across the whole width.
    if let Some(at) = widest_gap(&lines, true, ROW_GAP * median) {
        let (a, b) = split_at(lines, true, at);
        cut(a, median, depth + 1, out);
        cut(b, median, depth + 1, out);
        return;
    }
    if let Some(at) = widest_gap(&lines, false, COL_GAP * median) {
        let (a, b) = split_at(lines, false, at);
        // A split that leaves one side with a lone line is a margin note or a stray mark, not
        // a column; read it in place by position instead of cutting.
        if a.len() > 1 && b.len() > 1 {
            cut(a, median, depth + 1, out);
            cut(b, median, depth + 1, out);
            return;
        }
        let mut all = a;
        all.extend(b);
        sort_rows(&mut all);
        out.append(&mut all);
        return;
    }
    sort_rows(&mut lines);
    out.append(&mut lines);
}

/// The coordinate in the middle of the widest gap (at least `min`) in the projection of the
/// lines onto the y axis (`rows`) or the x axis.
fn widest_gap(lines: &[Line], rows: bool, min: f32) -> Option<f32> {
    let mut spans: Vec<(f32, f32)> = lines
        .iter()
        .map(|l| {
            if rows {
                (l.bbox.y, l.bbox.bottom())
            } else {
                (l.bbox.x, l.bbox.right())
            }
        })
        .collect();
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut reach = spans[0].1;
    let mut best: Option<(f32, f32)> = None;
    for &(lo, hi) in &spans[1..] {
        if lo - reach >= min && best.is_none_or(|(g, _)| lo - reach > g) {
            best = Some((lo - reach, (lo + reach) / 2.0));
        }
        reach = reach.max(hi);
    }
    best.map(|(_, at)| at)
}

fn split_at(lines: Vec<Line>, rows: bool, at: f32) -> (Vec<Line>, Vec<Line>) {
    lines.into_iter().partition(|l| {
        let mid = if rows {
            l.bbox.y + l.bbox.h / 2.0
        } else {
            l.bbox.x + l.bbox.w / 2.0
        };
        mid < at
    })
}

/// Top to bottom, and left to right for lines on the same row. Rows are found by sweeping the
/// lines in order of their vertical centre: a line joins the current row when its centre is
/// within half a line height of the row's, so the comparison is always a plain sort by key.
fn sort_rows(lines: &mut Vec<Line>) {
    let centre = |l: &Line| l.bbox.y + l.bbox.h / 2.0;
    lines.sort_by(|a, b| centre(a).total_cmp(&centre(b)));
    let mut rows: Vec<(f32, f32, Vec<Line>)> = Vec::new(); // (mean centre, min height, lines)
    for l in lines.drain(..) {
        let c = centre(&l);
        match rows.last_mut() {
            Some((rc, rh, row)) if (c - *rc).abs() <= 0.5 * rh.min(l.bbox.h) => {
                *rc = (*rc * row.len() as f32 + c) / (row.len() + 1) as f32;
                *rh = rh.min(l.bbox.h);
                row.push(l);
            }
            _ => rows.push((c, l.bbox.h, vec![l])),
        }
    }
    for (_, _, mut row) in rows {
        row.sort_by(|a, b| a.bbox.x.total_cmp(&b.bbox.x));
        lines.extend(row);
    }
}

/// Group lines into paragraphs: a new paragraph starts where the gap to the previous line is
/// clearly larger than normal line spacing, or the left edge jumps (an indent) after a line
/// that ended short.
pub fn paragraphs(lines: &[Line]) -> Vec<Vec<&Line>> {
    let mut out: Vec<Vec<&Line>> = Vec::new();
    let mut gaps: Vec<f32> = lines
        .windows(2)
        .map(|w| w[1].bbox.y - w[0].bbox.bottom())
        .filter(|g| g.is_finite())
        .collect();
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // The lower median: the gaps between paragraphs are the large ones and must not set the norm.
    let typical = gaps
        .get(gaps.len().saturating_sub(1) / 2)
        .copied()
        .unwrap_or(0.0)
        .max(0.0);
    let mut prev: Option<&Line> = None;
    for l in lines {
        let new_par = match prev {
            None => true,
            Some(p) => {
                let gap = l.bbox.y - p.bbox.bottom();
                let same_row = p.bbox.v_overlap(&l.bbox) > 0.5 * p.bbox.h.min(l.bbox.h);
                let line_h = p.bbox.h.min(l.bbox.h);
                // Reading jumped back up (the next column) or the gap is a blank line wide.
                gap < -line_h || (!same_row && gap > typical + 0.6 * line_h)
            }
        };
        if new_par {
            out.push(Vec::new());
        }
        out.last_mut().expect("a paragraph was just opened").push(l);
        prev = Some(l);
    }
    out
}

/// Plain text: paragraphs separated by a blank line, lines of a paragraph joined by a line
/// break when `keep_breaks` is set and by a space otherwise.
pub fn plain_text(page: &OcrPage, keep_breaks: bool) -> String {
    if keep_breaks {
        return page.text();
    }
    paragraphs(&page.lines)
        .iter()
        .map(|p| join_lines(p))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn join_lines(p: &[&Line]) -> String {
    let mut s = String::new();
    for l in p {
        let t = l.text();
        if s.is_empty() {
            s = t;
        } else if s.ends_with('-') && t.chars().next().is_some_and(char::is_lowercase) {
            // A word hyphenated at the end of a line is rejoined.
            s.pop();
            s.push_str(&t);
        } else {
            s.push(' ');
            s.push_str(&t);
        }
    }
    s
}

/// Markdown: a heading for a short line much taller than the body text, a list item for a
/// line that starts with a bullet or a number and a full stop, paragraphs for the rest.
pub fn markdown(page: &OcrPage) -> String {
    if page.lines.is_empty() {
        return String::new();
    }
    let mut hs: Vec<f32> = page.lines.iter().map(|l| l.bbox.h).collect();
    hs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let body = hs[hs.len() / 2].max(1.0);
    let mut out: Vec<String> = Vec::new();
    for p in paragraphs(&page.lines) {
        // A heading is a one line paragraph that is clearly taller than the body.
        if p.len() == 1 && p[0].words.len() <= 12 {
            let ratio = p[0].bbox.h / body;
            let level = if ratio >= 1.8 {
                Some(1)
            } else if ratio >= 1.4 {
                Some(2)
            } else if ratio >= 1.2 {
                Some(3)
            } else {
                None
            };
            if let Some(level) = level {
                out.push(format!("{} {}", "#".repeat(level), p[0].text()));
                continue;
            }
        }
        // List items: each line that begins with a marker is its own item.
        if p.iter().any(|l| list_marker(&l.text()).is_some()) {
            let mut items: Vec<String> = Vec::new();
            for l in &p {
                let t = l.text();
                match list_marker(&t) {
                    Some(rest) => items.push(format!("- {rest}")),
                    None => match items.last_mut() {
                        Some(last) => {
                            last.push(' ');
                            last.push_str(&t);
                        }
                        None => items.push(t),
                    },
                }
            }
            out.push(items.join("\n"));
            continue;
        }
        out.push(join_lines(&p));
    }
    out.join("\n\n")
}

/// The text after a bullet (`•`, `-`, `*`, `·`) or a number and a full stop or bracket (`3.`,
/// `3)`), when the line starts with one.
fn list_marker(t: &str) -> Option<String> {
    let t = t.trim_start();
    let mut chars = t.chars();
    let first = chars.next()?;
    if matches!(first, '•' | '·' | '▪' | '●' | '○' | '-' | '*' | '–') {
        let rest = chars.as_str();
        if rest.starts_with(' ') {
            return Some(rest.trim().to_string());
        }
        return None;
    }
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    if !digits.is_empty() && digits.len() <= 3 {
        let rest = &t[digits.len()..];
        if let Some(rest) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))
            && rest.starts_with(' ')
        {
            return Some(rest.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EngineKind, Rect, Word};

    fn line(text: &str, x: f32, y: f32, w: f32, h: f32) -> Line {
        Line::from_words(vec![Word {
            text: text.into(),
            bbox: Rect { x, y, w, h },
            confidence: 0.9,
        }])
        .unwrap()
    }

    fn page(lines: Vec<Line>) -> OcrPage {
        OcrPage {
            width: 1000,
            height: 1000,
            lines,
            engine: EngineKind::PpOcr,
            languages: vec![],
        }
    }

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(Line::text).collect()
    }

    #[test]
    fn two_columns_are_read_one_after_the_other() {
        // Left column lines at x 50, right column at x 550; interleaved by y.
        let ls = vec![
            line("L1", 50.0, 100.0, 400.0, 20.0),
            line("R1", 550.0, 102.0, 400.0, 20.0),
            line("L2", 50.0, 130.0, 400.0, 20.0),
            line("R2", 550.0, 133.0, 400.0, 20.0),
            line("L3", 50.0, 160.0, 400.0, 20.0),
            line("R3", 550.0, 158.0, 400.0, 20.0),
        ];
        let o = reading_order(ls);
        assert_eq!(texts(&o), vec!["L1", "L2", "L3", "R1", "R2", "R3"]);
    }

    #[test]
    fn a_full_width_heading_comes_before_the_columns() {
        let ls = vec![
            line("L1", 50.0, 200.0, 400.0, 20.0),
            line("Title", 50.0, 50.0, 900.0, 40.0),
            line("R1", 550.0, 200.0, 400.0, 20.0),
            line("L2", 50.0, 230.0, 400.0, 20.0),
            line("R2", 550.0, 230.0, 400.0, 20.0),
        ];
        let o = reading_order(ls);
        assert_eq!(texts(&o), vec!["Title", "L1", "L2", "R1", "R2"]);
    }

    #[test]
    fn single_column_is_top_to_bottom() {
        let ls = vec![
            line("c", 50.0, 300.0, 400.0, 20.0),
            line("a", 50.0, 100.0, 400.0, 20.0),
            line("b", 50.0, 140.0, 400.0, 20.0),
        ];
        assert_eq!(texts(&reading_order(ls)), vec!["a", "b", "c"]);
    }

    #[test]
    fn paragraphs_split_at_blank_lines_and_rejoin_hyphenated_words() {
        let p = page(vec![
            line(
                "the quick brown fox jumps over the lazy dog and runs to the long-",
                50.0,
                100.0,
                800.0,
                20.0,
            ),
            line("er road ahead", 50.0, 124.0, 300.0, 20.0),
            line("Second paragraph starts here", 50.0, 190.0, 500.0, 20.0),
        ]);
        let t = plain_text(&p, false);
        assert_eq!(
            t,
            "the quick brown fox jumps over the lazy dog and runs to the longer road ahead\n\nSecond paragraph starts here"
        );
        assert_eq!(plain_text(&p, true).lines().count(), 3);
    }

    #[test]
    fn markdown_finds_headings_and_list_items() {
        let p = page(vec![
            line("Report title", 50.0, 40.0, 400.0, 44.0),
            line("Some body text that goes on", 50.0, 120.0, 700.0, 22.0),
            line("more body text here", 50.0, 146.0, 600.0, 22.0),
            line("1. First thing", 50.0, 220.0, 300.0, 22.0),
            line("2. Second thing", 50.0, 246.0, 300.0, 22.0),
            line("• A bullet", 50.0, 272.0, 300.0, 22.0),
        ]);
        let m = markdown(&p);
        assert!(m.starts_with("# Report title\n\n"), "{m}");
        assert!(
            m.contains("Some body text that goes on more body text here"),
            "{m}"
        );
        assert!(
            m.contains("- First thing\n- Second thing\n- A bullet"),
            "{m}"
        );
    }
}
