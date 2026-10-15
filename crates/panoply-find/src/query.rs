//! The query engine.
//!
//! One keystroke runs up to three passes over one segment, cheapest first, and stops as soon as the
//! results cannot change:
//!
//! 1. **Prefix and exact.** `first` holds the first folded byte of every name, one byte per file, so
//!    `memchr` lists the names that start with the query's first byte at memory speed; the full prefix is
//!    then checked on those few. When this alone fills the result pool, nothing else can outrank it (the
//!    tier gaps are larger than every bonus) and the query ends here. That is why a one-letter query
//!    costs about the same as a ten-letter one.
//! 2. **Substring.** A SIMD search for the query over the whole folded-name buffer (zero bytes separate
//!    names, so a match never spans two). Hits are mapped back to their name with a cursor that only
//!    moves forward, so the cost is the hit, not a binary search.
//! 3. **Fuzzy.** Only if the pool is still short and the query has two letters or more: one AND per file
//!    against the 32-bit character mask, then a real subsequence match on the survivors.
//!
//! Ranking is `tier + bonuses`: exact name 10000, prefix 8000, word start 6000, substring 4000, fuzzy up
//! to 1000 plus its own score; bonuses (at most 1000 together) for how recently the file changed, how
//! shallow its path is and how much of the name the query covers. A caller-supplied `boost` (frecency:
//! opened often and lately) is added to the best few hundred candidates before the final sort.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use memchr::memmem;
use rayon::prelude::*;

use crate::fold::{fold, is_separator, mask_of_folded};
use crate::fuzzy;
use crate::index::Index;
use crate::segment::{F_DEAD, F_DIR, F_HIDDEN, F_ROOT, Segment};

const TIER_EXACT: u32 = 10_000;
const TIER_PREFIX: u32 = 8_000;
const TIER_BOUNDARY: u32 = 6_000;
const TIER_SUBSTRING: u32 = 4_000;
const TIER_FUZZY: u32 = 1_000;
const MODS_MAX: u32 = 1_000;
const SKIP: u32 = u32::MAX;

/// Bonus for how recently an entry changed, indexed by the bit length of its age in seconds.
const RECENCY: [u32; 33] = [
    400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 400, 380, 360, 340, 320, 290, 260,
    230, 200, 170, 140, 110, 80, 50, 25, 0, 0, 0, 0, 0, 0,
];

/// What a search may return.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// Files and folders.
    #[default]
    Any,
    /// Files only.
    Files,
    /// Folders only.
    Dirs,
}

/// Knobs for one search.
pub struct SearchOptions<'a> {
    /// Results to return.
    pub limit: usize,
    /// "Now" in seconds since the Unix epoch, for the recency bonus.
    pub now: u32,
    /// Include dotfiles and anything under a dot folder.
    pub include_hidden: bool,
    /// Files, folders or both.
    pub kind: Kind,
    /// Extra score for a path (frecency). Called for at most the best few hundred candidates.
    pub boost: Option<&'a (dyn Fn(&Path) -> u32 + Sync)>,
}

impl Default for SearchOptions<'_> {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs().min(u32::MAX as u64) as u32);
        SearchOptions {
            limit: 50,
            now,
            include_hidden: false,
            kind: Kind::Any,
            boost: None,
        }
    }
}

/// One result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Global entry id; read it back with [`Index::entry`].
    pub id: u32,
    /// Rank score, higher is better.
    pub score: u32,
}

struct Token {
    folded: Vec<u8>,
    raw: Vec<u8>,
    needle: Vec<u8>,
    case_sensitive: bool,
    path: bool,
    mask: u32,
}

struct Prepared {
    tokens: Vec<Token>,
    /// Tokens to drive a scan with, most selective (longest) first. A token that is a folder name finds
    /// only the folder, so the others are tried too when the first does not fill the page: for
    /// "documents report" the second scan, driven by "report", finds the files inside Documents.
    drivers: Vec<usize>,
    simple: bool,
}

impl Prepared {
    fn parse(q: &str) -> Option<Prepared> {
        let mut tokens = Vec::new();
        for word in q.split_whitespace() {
            let word = word.replace('\\', "/");
            let path = word.contains('/');
            let (folded, _) = fold(&word);
            let needle: Vec<u8> = if path {
                folded
                    .rsplit(|&b| b == b'/')
                    .find(|s| !s.is_empty())
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default()
            } else {
                folded.clone()
            };
            let mask = mask_of_folded(&needle);
            tokens.push(Token {
                raw: word.as_bytes().to_vec(),
                case_sensitive: word.chars().any(char::is_uppercase),
                folded,
                needle,
                path,
                mask,
            });
        }
        let usable: Vec<usize> = (0..tokens.len())
            .filter(|&i| !tokens[i].needle.is_empty())
            .collect();
        let last = *usable.last()?;
        let mut rest: Vec<usize> = usable.iter().copied().filter(|&i| i != last).collect();
        rest.sort_by_key(|&i| (Reverse(tokens[i].needle.len()), i));
        let mut drivers = vec![last];
        drivers.extend(rest);
        let simple = tokens.len() == 1 && !tokens[0].path && !tokens[0].case_sensitive;
        Some(Prepared {
            tokens,
            drivers,
            simple,
        })
    }
}

struct Collector {
    heap: BinaryHeap<Reverse<u64>>,
    cap: usize,
    min: u64,
}

#[inline]
fn key(score: u32, gid: u32) -> u64 {
    ((score as u64) << 32) | (u32::MAX - gid) as u64
}

impl Collector {
    fn new(cap: usize) -> Self {
        Collector {
            heap: BinaryHeap::with_capacity(cap + 1),
            cap,
            min: 0,
        }
    }

    #[inline]
    fn push(&mut self, score: u32, gid: u32) {
        let k = key(score, gid);
        if self.heap.len() < self.cap {
            self.heap.push(Reverse(k));
            if self.heap.len() == self.cap {
                self.min = self.heap.peek().map_or(0, |r| r.0);
            }
        } else if k > self.min {
            self.heap.pop();
            self.heap.push(Reverse(k));
            self.min = self.heap.peek().map_or(0, |r| r.0);
        }
    }

    fn len(&self) -> usize {
        self.heap.len()
    }

    fn merge(&mut self, other: Collector) {
        for Reverse(k) in other.heap {
            self.push((k >> 32) as u32, u32::MAX - k as u32);
        }
    }

    fn full(&self) -> bool {
        self.heap.len() >= self.cap
    }

    fn min_score(&self) -> u32 {
        (self.min >> 32) as u32
    }

    fn into_hits(self) -> Vec<Hit> {
        let mut v: Vec<u64> = self.heap.into_iter().map(|r| r.0).collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v.into_iter()
            .map(|k| Hit {
                score: (k >> 32) as u32,
                id: u32::MAX - (k as u32),
            })
            .collect()
    }
}

struct Scanner<'a> {
    idx: &'a Index,
    seg: &'a Segment,
    q: &'a Prepared,
    opts: &'a SearchOptions<'a>,
    primary: usize,
    needle: &'a [u8],
}

impl Scanner<'_> {
    #[inline]
    fn accept(&self, e: usize) -> bool {
        let fl = self.seg.flags[e];
        if fl & (F_DEAD | F_ROOT) != 0 {
            return false;
        }
        if fl & F_HIDDEN != 0 && !self.opts.include_hidden {
            return false;
        }
        match self.opts.kind {
            Kind::Any => {}
            Kind::Files if fl & F_DIR != 0 => return false,
            Kind::Dirs if fl & F_DIR == 0 => return false,
            _ => {}
        }
        self.q.simple || self.accept_extras(e)
    }

    #[cold]
    fn accept_extras(&self, e: usize) -> bool {
        let gid = self.seg.base + e as u32;
        let mut path: Option<Vec<u8>> = None;
        for (ti, t) in self.q.tokens.iter().enumerate() {
            if t.path {
                let p = path.get_or_insert_with(|| self.idx.folded_path(gid));
                if memmem::find(p, &t.folded).is_none() {
                    return false;
                }
            } else if ti != self.primary
                && memmem::find(self.seg.f_name(e), &t.folded).is_none()
                && !self.idx.ancestor_contains(self.seg.parent[e], &t.folded)
            {
                return false;
            }
            if t.case_sensitive && !t.path && memmem::find(self.seg.o_name(e), &t.raw).is_none() {
                return false;
            }
        }
        true
    }

    #[inline]
    fn mods(&self, e: usize, name_len: usize) -> u32 {
        let depth = self.seg.depth[e] as u32;
        let shallow = 300u32.saturating_sub(depth * 30);
        let age = self.opts.now.saturating_sub(self.seg.mtime[e]);
        let rec = RECENCY[(32 - (age | 1).leading_zeros()) as usize];
        let cover = (self.needle.len() as u32 * 300) / (name_len as u32).max(1);
        shallow + rec + cover.min(300)
    }

    #[inline]
    fn flush(&self, coll: &mut Collector, e: usize, tier: u32) {
        if e != usize::MAX
            && tier != SKIP
            && !(coll.full() && tier + MODS_MAX < coll.min_score())
            && self.accept(e)
        {
            let len = (self.seg.f_off[e + 1] - self.seg.f_off[e] - 1) as usize;
            coll.push(tier + self.mods(e, len), self.seg.base + e as u32);
        }
    }

    fn run(&self, coll: &mut Collector) {
        let seg = self.seg;
        let needle = self.needle;
        let nl = needle.len();
        let n = seg.len();
        if n == 0 {
            return;
        }

        // Pass 1: exact and prefix, one byte per file.
        for e in memchr::memchr_iter(needle[0], &seg.first) {
            let off = seg.f_off[e] as usize;
            let len = seg.f_off[e + 1] as usize - 1 - off;
            if len < nl || &seg.f_names[off..off + nl] != needle {
                continue;
            }
            if !self.accept(e) {
                continue;
            }
            let tier = if len == nl { TIER_EXACT } else { TIER_PREFIX };
            coll.push(tier + self.mods(e, len), seg.base + e as u32);
        }
        if coll.full() && coll.min_score() > TIER_BOUNDARY + MODS_MAX {
            return;
        }

        // Pass 2: substring anywhere, SIMD over the folded buffer. Big segments are cut into slices at
        // name boundaries and scanned by the search pool; each slice keeps its own top results.
        let chunks = split_ranges(seg, PAR_MIN_BYTES);
        match (chunks.len() > 1).then(search_pool).flatten() {
            Some(pool) => {
                let parts: Vec<Collector> = pool.install(|| {
                    chunks
                        .par_iter()
                        .map(|&(lo, hi)| {
                            let mut c = Collector::new(coll.cap);
                            self.substring_range(lo, hi, &mut c);
                            c
                        })
                        .collect()
                });
                for c in parts {
                    coll.merge(c);
                }
            }
            None => self.substring_range(0, n, coll),
        }

        // Pass 3: fuzzy, only when the pool is still short.
        if coll.len() < self.opts.limit && nl >= 2 {
            match (chunks.len() > 1).then(search_pool).flatten() {
                Some(pool) => {
                    let parts: Vec<Collector> = pool.install(|| {
                        chunks
                            .par_iter()
                            .map(|&(lo, hi)| {
                                let mut c = Collector::new(coll.cap);
                                self.fuzzy_range(lo, hi, &mut c);
                                c
                            })
                            .collect()
                    });
                    for c in parts {
                        coll.merge(c);
                    }
                }
                None => self.fuzzy_range(0, n, coll),
            }
        }
    }

    /// Substring hits for the entries `lo..hi`.
    fn substring_range(&self, lo: usize, hi: usize, coll: &mut Collector) {
        let seg = self.seg;
        let base = seg.f_off[lo] as usize;
        let hay = &seg.f_names[base..seg.f_off[hi] as usize];
        let finder = memmem::Finder::new(self.needle);
        let mut e = lo;
        let mut cur = usize::MAX;
        let mut cur_tier = 0u32;
        for rel in finder.find_iter(hay) {
            let pos = base + rel;
            if seg.f_off[e + 1] as usize <= pos {
                e += 1;
                if seg.f_off[e + 1] as usize <= pos {
                    e += seg.f_off[e + 1..hi + 1].partition_point(|&o| (o as usize) <= pos);
                }
            }
            let off = seg.f_off[e] as usize;
            let tier = if pos == off {
                SKIP
            } else if is_separator(seg.f_names[pos - 1]) {
                TIER_BOUNDARY
            } else {
                TIER_SUBSTRING
            };
            if e != cur {
                self.flush(coll, cur, cur_tier);
                cur = e;
                cur_tier = tier;
            } else {
                cur_tier = cur_tier.max(tier);
            }
        }
        self.flush(coll, cur, cur_tier);
    }

    /// Fuzzy hits for the entries `lo..hi`: the character mask rejects most names with one AND, a real
    /// subsequence match runs on the rest.
    fn fuzzy_range(&self, lo: usize, hi: usize, coll: &mut Collector) {
        let seg = self.seg;
        let needle = self.needle;
        let qmask = self.q.tokens[self.primary].mask;
        let finder = memmem::Finder::new(needle);
        for e in lo..hi {
            if seg.mask[e] & qmask != qmask {
                continue;
            }
            let name = seg.f_name(e);
            if finder.find(name).is_some() {
                continue;
            }
            let Some(fs) = fuzzy::score(needle, name) else {
                continue;
            };
            if !self.accept(e) {
                continue;
            }
            let depth = seg.depth[e] as u32;
            let age = self.opts.now.saturating_sub(seg.mtime[e]);
            let rec = RECENCY[(32 - (age | 1).leading_zeros()) as usize];
            coll.push(
                TIER_FUZZY + fs + 300u32.saturating_sub(depth * 30) + rec,
                seg.base + e as u32,
            );
        }
    }
}

/// Segments with fewer name bytes than this are scanned on the calling thread.
const PAR_MIN_BYTES: usize = 2 << 20;

/// Entry ranges of about equal byte size, cut at name boundaries (one range when the segment is small).
fn split_ranges(seg: &Segment, min_bytes: usize) -> Vec<(usize, usize)> {
    let n = seg.len();
    let total = seg.f_names.len();
    let parts = search_pool().map_or(1, |p| p.current_num_threads());
    if total < min_bytes || parts <= 1 || n == 0 {
        return vec![(0, n)];
    }
    let mut cuts = vec![0usize];
    for t in 1..parts {
        let target = (total * t / parts) as u32;
        let e = seg.f_off.partition_point(|&o| o < target).min(n);
        if e > *cuts.last().unwrap_or(&0) {
            cuts.push(e);
        }
    }
    cuts.push(n);
    cuts.windows(2)
        .map(|w| (w[0], w[1]))
        .filter(|(a, b)| b > a)
        .collect()
}

/// The pool that scans big segments: up to 8 threads, parked (no CPU) between keystrokes.
fn search_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(8);
        if n < 2 {
            return None;
        }
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .thread_name(|i| format!("panoply-find-query-{i}"))
            .build()
            .ok()
    })
    .as_ref()
}

impl Index {
    /// Searches by file name. The query is split on spaces into tokens that must all match (in the
    /// name, or for the extra tokens in the path); a token with a `/` must appear in the path. An
    /// uppercase letter in a token makes that token case sensitive (smart case). Returns at most
    /// `opts.limit` hits, best first.
    pub fn search(&self, query: &str, opts: &SearchOptions<'_>) -> Vec<Hit> {
        let Some(q) = Prepared::parse(query) else {
            return Vec::new();
        };
        let mut coll = Collector::new((opts.limit * 4).max(128));
        for (i, &primary) in q.drivers.iter().enumerate() {
            let needle = q.tokens[primary].needle.as_slice();
            // A later driver only runs to fill a short page, and never as a one-letter scan of everything.
            if i > 0 && (coll.len() >= opts.limit || needle.len() < 2) {
                continue;
            }
            for seg in &self.segs {
                Scanner {
                    idx: self,
                    seg,
                    q: &q,
                    opts,
                    primary,
                    needle,
                }
                .run(&mut coll);
            }
        }
        let mut hits = coll.into_hits();
        if q.drivers.len() > 1 {
            // Two drivers can find the same entry; keep its best score.
            let mut seen = std::collections::HashSet::new();
            hits.retain(|h| seen.insert(h.id));
        }
        if let Some(boost) = opts.boost {
            for h in &mut hits {
                if let Some(p) = self.path(h.id) {
                    h.score += boost(&p);
                }
            }
            hits.sort_unstable_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        }
        hits.truncate(opts.limit);
        hits
    }
}
