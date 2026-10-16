//! Subsequence ("fuzzy") matching, run only on the few names the character-mask prefilter lets through.
//!
//! `needle` and `hay` are both folded. A name matches when every needle byte appears in it in order.
//! The score rewards what people mean when they type an abbreviation: characters that sit right after
//! each other, characters that start a word (after `_ - . space`) and a match that starts at the head
//! of the name. It punishes spread. The match window is found the way fzf's first algorithm does: a
//! forward pass finds the earliest end, a backward pass from that end finds the latest start, so the
//! window is the tightest one that ends first.

use crate::fold::is_separator;

/// Highest score [`score`] returns.
pub const MAX_SCORE: u32 = 1000;

/// Scores `needle` as a subsequence of `hay`, or `None` when it is not one.
pub fn score(needle: &[u8], hay: &[u8]) -> Option<u32> {
    if needle.is_empty() {
        return Some(0);
    }
    // Forward: earliest end.
    let mut j = 0;
    let mut end = 0;
    for (i, &b) in hay.iter().enumerate() {
        if b == needle[j] {
            j += 1;
            if j == needle.len() {
                end = i;
                break;
            }
        }
    }
    if j < needle.len() {
        return None;
    }
    // Backward: latest start that still fits before `end`.
    let mut j = needle.len();
    let mut start = end;
    let mut i = end as isize;
    while i >= 0 {
        if hay[i as usize] == needle[j - 1] {
            j -= 1;
            if j == 0 {
                start = i as usize;
                break;
            }
        }
        i -= 1;
    }
    // Score the window greedily from its start.
    let mut s: i32 = 0;
    let mut prev: Option<usize> = None;
    let mut j = 0;
    for (i, &b) in hay.iter().enumerate().take(end + 1).skip(start) {
        if j < needle.len() && b == needle[j] {
            s += 16;
            if i == 0 {
                s += 24;
            } else if is_separator(hay[i - 1]) {
                s += 14;
            }
            match prev {
                Some(p) if p + 1 == i => s += 12,
                Some(p) => s -= 3 + (i - p - 1).min(8) as i32,
                None => {}
            }
            prev = Some(i);
            j += 1;
        }
    }
    // Prefer matches that cover more of a short name.
    s += (needle.len() * 40 / hay.len().max(1)) as i32;
    Some(s.clamp(1, MAX_SCORE as i32) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_required() {
        assert!(score(b"qrf", b"quarterly_report_final.xlsx").is_some());
        assert!(score(b"qzf", b"quarterly_report_final.xlsx").is_none());
        assert!(score(b"abc", b"acb").is_none());
    }

    #[test]
    fn boundaries_and_runs_win() {
        let tight = score(b"rep", b"report.pdf").unwrap();
        let spread = score(b"rep", b"red_elephant.pdf").unwrap();
        assert!(tight > spread);
        let boundary = score(b"qr", b"q1_report.txt").unwrap();
        let buried = score(b"qr", b"aqzzzzr.txt").unwrap();
        assert!(boundary > buried);
    }
}
