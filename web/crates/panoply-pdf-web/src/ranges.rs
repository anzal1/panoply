//! Page ranges as people write them: `1-3, 7, 9-`, `last`, `odd`, `even`, `5-1` (reversed).

use crate::error::{PdfError, Result};

/// One comma-separated item, expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeItem {
    /// The item as the user wrote it, trimmed.
    pub label: String,
    /// Zero-based page indexes in the order the item lists them.
    pub pages: Vec<usize>,
}

/// The result of reading a range expression against a page count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    /// Items that selected at least one page.
    pub items: Vec<RangeItem>,
    /// Items that were wholly outside the document and left out.
    pub skipped: Vec<String>,
    /// Items with an end beyond the last page, clamped to it.
    pub clamped: Vec<String>,
}

impl Parsed {
    /// Every selected page, in order, with repeats.
    pub fn flat(&self) -> Vec<usize> {
        self.items
            .iter()
            .flat_map(|i| i.pages.iter().copied())
            .collect()
    }
}

fn bad(item: &str) -> PdfError {
    PdfError::BadInput(format!(
        "I could not read \"{item}\" as pages. Use page numbers and ranges like 1-3, 7, 9- or the words first, last, odd, even."
    ))
}

/// Read an endpoint: a number, `first`, or `last`. Returns a one-based page number, possibly
/// beyond `total` (clamped by the caller).
fn endpoint(tok: &str, total: usize, item: &str) -> Result<u64> {
    let t = tok.trim().to_ascii_lowercase();
    match t.as_str() {
        "first" => Ok(1),
        "last" => Ok(total as u64),
        _ => t.parse::<u64>().map_err(|_| bad(item)),
    }
}

/// Parse a range expression for a document of `total` pages. An empty expression selects nothing
/// (callers decide what that means). Page numbers are one-based in the text.
pub fn parse(text: &str, total: usize) -> Result<Parsed> {
    let mut out = Parsed::default();
    for raw in text.split([',', ';']) {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        let lower = item.to_ascii_lowercase();
        let pages: Vec<usize> = match lower.as_str() {
            "all" => (0..total).collect(),
            "odd" => (0..total).filter(|i| i % 2 == 0).collect(),
            "even" => (0..total).filter(|i| i % 2 == 1).collect(),
            _ => {
                let (a, b) = match lower.split_once('-') {
                    None => {
                        let n = endpoint(&lower, total, item)?;
                        (n, n)
                    }
                    Some((a, b)) => {
                        let a = if a.trim().is_empty() {
                            1
                        } else {
                            endpoint(a, total, item)?
                        };
                        let b = if b.trim().is_empty() {
                            total as u64
                        } else {
                            endpoint(b, total, item)?
                        };
                        (a, b)
                    }
                };
                if a == 0 || b == 0 {
                    return Err(PdfError::BadInput(format!(
                        "\"{item}\" starts at page 0. Pages are numbered from 1."
                    )));
                }
                let (lo, hi) = (a.min(b), a.max(b));
                if lo > total as u64 {
                    out.skipped.push(item.to_string());
                    continue;
                }
                let hi_c = hi.min(total as u64);
                if hi_c != hi {
                    out.clamped.push(item.to_string());
                }
                let mut v: Vec<usize> = ((lo - 1) as usize..hi_c as usize).collect();
                if a > b {
                    v.reverse();
                }
                v
            }
        };
        if pages.is_empty() {
            out.skipped.push(item.to_string());
        } else {
            out.items.push(RangeItem {
                label: item.to_string(),
                pages,
            });
        }
    }
    Ok(out)
}

/// A compact label for a list of pages, one-based: `3-7`, `1, 4, 9`, `2-4, 8`.
pub fn label(pages: &[usize]) -> String {
    let mut sorted: Vec<usize> = pages.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i;
        while j + 1 < sorted.len() && sorted[j + 1] == sorted[j] + 1 {
            j += 1;
        }
        if j == i {
            parts.push(format!("{}", sorted[i] + 1));
        } else if j == i + 1 {
            parts.push(format!("{}, {}", sorted[i] + 1, sorted[j] + 1));
        } else {
            parts.push(format!("{}-{}", sorted[i] + 1, sorted[j] + 1));
        }
        i = j + 1;
    }
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ranges_and_open_ends() {
        let p = parse("1-3, 7, 9-", 10).unwrap();
        let pages: Vec<Vec<usize>> = p.items.iter().map(|i| i.pages.clone()).collect();
        assert_eq!(pages, vec![vec![0, 1, 2], vec![6], vec![8, 9]]);
        assert!(p.skipped.is_empty() && p.clamped.is_empty());
    }

    #[test]
    fn keywords_and_reverse() {
        assert_eq!(parse("last", 5).unwrap().flat(), vec![4]);
        assert_eq!(parse("odd", 5).unwrap().flat(), vec![0, 2, 4]);
        assert_eq!(parse("even", 5).unwrap().flat(), vec![1, 3]);
        assert_eq!(parse("5-3", 6).unwrap().flat(), vec![4, 3, 2]);
        assert_eq!(parse("last-4", 6).unwrap().flat(), vec![5, 4, 3]);
        assert_eq!(parse("-2", 6).unwrap().flat(), vec![0, 1]);
    }

    #[test]
    fn out_of_range_is_clamped_or_skipped_and_listed() {
        let p = parse("4-99, 50", 10).unwrap();
        assert_eq!(p.flat(), (3..10).collect::<Vec<_>>());
        assert_eq!(p.clamped, vec!["4-99"]);
        assert_eq!(p.skipped, vec!["50"]);
    }

    #[test]
    fn nonsense_has_a_plain_error() {
        let e = parse("1-x", 10).unwrap_err().to_string();
        assert!(e.contains("could not read"), "{e}");
        assert!(parse("0", 10).is_err());
    }

    #[test]
    fn labels_collapse_runs() {
        assert_eq!(label(&[2, 3, 4, 5, 6]), "3-7");
        assert_eq!(label(&[0, 3, 8]), "1, 4, 9");
        assert_eq!(label(&[1, 2]), "2, 3");
    }
}
