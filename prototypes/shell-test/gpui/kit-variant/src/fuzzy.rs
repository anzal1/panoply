//! Fuzzy scoring: same constants and best-alignment DP as the SwiftUI reference.

pub struct IndexEntry {
    pub order: usize,
    pub name_lower: Vec<char>,
    pub boundary: Vec<bool>,
    pub subtitle_lower: String,
    pub category_lower: String,
}

const BASE: i32 = 10;
const CONSECUTIVE: i32 = 12;
const WORD_START: i32 = 10;
const PREFIX: i32 = 14;
const GAP_PENALTY: i32 = 2;
const GAP_CAP: i32 = 8;
const LEAD_PENALTY: i32 = 1;
const LEAD_CAP: i32 = 6;

pub fn lower_chars(s: &str) -> Vec<char> {
    s.to_lowercase().chars().collect()
}

pub fn boundaries(name: &str) -> Vec<bool> {
    let u: Vec<char> = name.chars().collect();
    let mut out = vec![false; u.len()];
    for j in 0..u.len() {
        if j == 0 {
            out[j] = true;
            continue;
        }
        let (p, c) = (u[j - 1], u[j]);
        if !p.is_alphanumeric() {
            out[j] = true;
            continue;
        }
        if p.is_lowercase() && c.is_uppercase() {
            out[j] = true;
        }
    }
    out
}

/// Best-alignment subsequence match. Returns score and matched char indices.
pub fn matches(q: &[char], e: &IndexEntry) -> Option<(i32, Vec<usize>)> {
    let m = q.len();
    let t = &e.name_lower;
    let n = t.len();
    if m == 0 {
        return Some((0, vec![]));
    }
    if m > n {
        return None;
    }
    const NONE: i32 = i32::MIN;
    let mut dp = vec![NONE; m * n];
    let mut par = vec![usize::MAX; m * n];
    for j in 0..n {
        if t[j] == q[0] {
            let mut s = BASE;
            if e.boundary[j] {
                s += WORD_START;
            }
            if j == 0 {
                s += PREFIX;
            }
            s -= ((j as i32) * LEAD_PENALTY).min(LEAD_CAP);
            dp[j] = s;
        }
    }
    for i in 1..m {
        if i >= n {
            return None;
        }
        for j in i..n {
            if t[j] != q[i] {
                continue;
            }
            let mut best = NONE;
            let mut bk = usize::MAX;
            for k in (i - 1)..j {
                let prev = dp[(i - 1) * n + k];
                if prev == NONE {
                    continue;
                }
                let s = if k == j - 1 {
                    prev + CONSECUTIVE
                } else {
                    prev - (((j - k - 1) as i32) * GAP_PENALTY).min(GAP_CAP)
                };
                if s > best {
                    best = s;
                    bk = k;
                }
            }
            if best == NONE {
                continue;
            }
            let mut s = best + BASE;
            if e.boundary[j] {
                s += WORD_START;
            }
            dp[i * n + j] = s;
            par[i * n + j] = bk;
        }
    }
    let mut best_end = usize::MAX;
    let mut best_score = NONE;
    for j in 0..n {
        let s = dp[(m - 1) * n + j];
        if s != NONE && s > best_score {
            best_score = s;
            best_end = j;
        }
    }
    if best_end == usize::MAX {
        return None;
    }
    let mut idx = vec![0usize; m];
    let mut j = best_end;
    for i in (0..m).rev() {
        idx[i] = j;
        if i > 0 {
            j = par[i * n + j];
        }
    }
    Some((best_score, idx))
}
