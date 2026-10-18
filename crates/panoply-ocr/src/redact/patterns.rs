//! Finding the things to redact in a page's text: the user's exact terms and the presets.
//!
//! Matching works on one string per page (words joined by a space), so a term or a number that
//! the page breaks over several words (`1234 5678 9012`) is still one match. Overlaps are
//! resolved in a fixed order: the user's own terms first, then the numbers, then the looser
//! shapes, so a 12 digit Aadhaar is never also reported as a phone number.

use std::ops::Range;

use regex::Regex;

/// A built in shape to look for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Preset {
    /// An email address.
    Email,
    /// A phone number of 10 to 13 digits, with or without a country code.
    Phone,
    /// An Indian Aadhaar number: 12 digits as 4-4-4.
    Aadhaar,
    /// An Indian PAN: `AAAAA9999A`.
    Pan,
    /// A payment card number of 13 to 19 digits that passes the Luhn check.
    Card,
    /// An IBAN that passes the mod 97 check.
    Iban,
}

impl Preset {
    /// Every preset.
    pub const ALL: [Preset; 6] = [
        Preset::Email,
        Preset::Phone,
        Preset::Aadhaar,
        Preset::Pan,
        Preset::Card,
        Preset::Iban,
    ];

    /// The word used on the command line and in options.
    pub fn id(self) -> &'static str {
        match self {
            Preset::Email => "email",
            Preset::Phone => "phone",
            Preset::Aadhaar => "aadhaar",
            Preset::Pan => "pan",
            Preset::Card => "card",
            Preset::Iban => "iban",
        }
    }

    /// A name for people.
    pub fn label(self) -> &'static str {
        match self {
            Preset::Email => "Email address",
            Preset::Phone => "Phone number",
            Preset::Aadhaar => "Aadhaar number",
            Preset::Pan => "PAN",
            Preset::Card => "Card number",
            Preset::Iban => "IBAN",
        }
    }

    /// Read one id.
    pub fn parse(s: &str) -> Option<Preset> {
        let s = s.trim().to_ascii_lowercase();
        Preset::ALL.into_iter().find(|p| p.id() == s)
    }

    /// Read a comma separated list. `all` is every preset, `none` and the empty string none.
    pub fn parse_list(s: &str) -> Result<Vec<Preset>, String> {
        let mut out = Vec::new();
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "none" => {}
                "all" => out.extend(Preset::ALL),
                other => out.push(Preset::parse(other).ok_or_else(|| {
                    format!(
                        "Unknown preset \"{other}\". Choose from email, phone, aadhaar, pan, card, iban, all."
                    )
                })?),
            }
        }
        out.sort_by_key(|p| p.id());
        out.dedup();
        Ok(out)
    }

    fn regex(self) -> &'static str {
        match self {
            Preset::Email => {
                r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}"
            }
            Preset::Phone => r"\+?\d[\d ().\-]{8,18}\d",
            Preset::Aadhaar => r"\d{4}[ \-\n]{1,2}\d{4}[ \-\n]{1,2}\d{4}",
            Preset::Pan => r"[A-Z]{5}[0-9]{4}[A-Z]",
            Preset::Card => r"\d(?:[ \-\n]{0,2}\d){12,18}",
            Preset::Iban => r"[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){2,7}(?: ?[A-Z0-9]{1,3})?",
        }
    }

    /// Order in which presets claim text: more specific first.
    fn priority(self) -> u8 {
        match self {
            Preset::Aadhaar => 0,
            Preset::Card => 1,
            Preset::Iban => 2,
            Preset::Pan => 3,
            Preset::Email => 4,
            Preset::Phone => 5,
        }
    }

    fn accepts(self, text: &str) -> bool {
        let digits = text.bytes().filter(u8::is_ascii_digit).count();
        match self {
            Preset::Email | Preset::Pan => true,
            Preset::Aadhaar => {
                digits == 12
                    && text
                        .bytes()
                        .next()
                        .is_some_and(|b| (b'2'..=b'9').contains(&b))
                    && verhoeff_ok(text)
            }
            Preset::Phone => (10..=13).contains(&digits),
            Preset::Card => (13..=19).contains(&digits) && luhn(text),
            Preset::Iban => iban_ok(text),
        }
    }
}

/// The Luhn check over the digits of `s`.
pub fn luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if digits.len() < 2 {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// The Verhoeff check Aadhaar numbers carry in their last digit.
pub fn verhoeff_ok(s: &str) -> bool {
    const D: [[u8; 10]; 10] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 2, 3, 4, 0, 6, 7, 8, 9, 5],
        [2, 3, 4, 0, 1, 7, 8, 9, 5, 6],
        [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
        [4, 0, 1, 2, 3, 9, 5, 6, 7, 8],
        [5, 9, 8, 7, 6, 0, 4, 3, 2, 1],
        [6, 5, 9, 8, 7, 1, 0, 4, 3, 2],
        [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
        [8, 7, 6, 5, 9, 3, 2, 1, 0, 4],
        [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
    ];
    const P: [[u8; 10]; 8] = [
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        [1, 5, 7, 6, 2, 8, 3, 0, 9, 4],
        [5, 8, 0, 3, 7, 9, 6, 1, 4, 2],
        [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
        [9, 4, 5, 3, 1, 2, 6, 8, 7, 0],
        [4, 2, 8, 6, 5, 7, 3, 9, 0, 1],
        [2, 7, 9, 3, 8, 0, 6, 4, 1, 5],
        [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
    ];
    let mut c = 0usize;
    for (i, d) in s
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .enumerate()
    {
        c = usize::from(D[c][usize::from(P[i % 8][d as usize])]);
    }
    c == 0
}

/// The IBAN mod 97 check, and a plausible length.
pub fn iban_ok(s: &str) -> bool {
    let c: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !(15..=34).contains(&c.len()) {
        return false;
    }
    let mut rem: u32 = 0;
    for ch in c[4..].iter().chain(c[..4].iter()) {
        let v = match ch.to_digit(36) {
            Some(v) => v,
            None => return false,
        };
        rem = if v < 10 {
            (rem * 10 + v) % 97
        } else {
            (rem * 100 + v) % 97
        };
    }
    rem == 1
}

/// What was found: where in the page text, and what kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Byte range in the text.
    pub range: Range<usize>,
    /// `term` or a preset id.
    pub kind: &'static str,
}

/// The compiled patterns for one job.
pub struct Matcher {
    terms: Option<Regex>,
    presets: Vec<(Preset, Regex)>,
}

impl Matcher {
    /// Compile the terms (matched ignoring case, any run of white space inside a term matches
    /// any run of white space) and the presets.
    pub fn new(terms: &[String], presets: &[Preset]) -> Result<Matcher, String> {
        let mut parts: Vec<String> = terms
            .iter()
            .map(|t| t.split_whitespace().collect::<Vec<_>>())
            .filter(|w| !w.is_empty())
            .map(|w| {
                let body = w
                    .iter()
                    .map(|x| regex::escape(x))
                    .collect::<Vec<_>>()
                    .join(r"\s+");
                // A term made of letters or digits must not match inside a longer word.
                let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
                let first = w.first().and_then(|x| x.chars().next());
                let last = w.last().and_then(|x| x.chars().next_back());
                format!(
                    "{}{body}{}",
                    if word(first) { r"\b" } else { "" },
                    if word(last) { r"\b" } else { "" }
                )
            })
            .collect();
        parts.sort_by_key(|p| std::cmp::Reverse(p.len()));
        parts.dedup();
        let terms = if parts.is_empty() {
            None
        } else {
            Some(
                Regex::new(&format!("(?i)(?:{})", parts.join("|")))
                    .map_err(|e| format!("A term could not be read ({e})."))?,
            )
        };
        let mut ps: Vec<Preset> = presets.to_vec();
        ps.sort_by_key(|p| p.priority());
        ps.dedup();
        let presets = ps
            .into_iter()
            .map(|p| Regex::new(p.regex()).map(|r| (p, r)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("A preset could not be built ({e})."))?;
        Ok(Matcher { terms, presets })
    }

    /// True when there is nothing to look for.
    pub fn is_empty(&self) -> bool {
        self.terms.is_none() && self.presets.is_empty()
    }

    /// Every hit in `text`, sorted by position, none overlapping another.
    pub fn find(&self, text: &str) -> Vec<Hit> {
        let mut hits: Vec<Hit> = Vec::new();
        let free = |hits: &[Hit], r: &Range<usize>| {
            !hits
                .iter()
                .any(|h| h.range.start < r.end && r.start < h.range.end)
        };
        if let Some(re) = &self.terms {
            for m in re.find_iter(text) {
                hits.push(Hit {
                    range: m.range(),
                    kind: "term",
                });
            }
        }
        for (p, re) in &self.presets {
            let mut from = 0;
            while let Some(m) = re.find_at(text, from) {
                let r = m.range();
                // A match may start inside a longer run of digits; the regex engine cannot
                // look behind, so check the edges here and move on by one character.
                let ok = bounded(text, &r, *p) && p.accepts(m.as_str()) && free(&hits, &r);
                if ok {
                    from = r.end;
                    hits.push(Hit {
                        range: r,
                        kind: p.id(),
                    });
                } else {
                    from = text[r.start..]
                        .chars()
                        .next()
                        .map_or(r.end, |c| r.start + c.len_utf8());
                }
            }
        }
        hits.sort_by_key(|h| h.range.start);
        hits
    }
}

/// The characters on each side of the match must not extend it.
fn bounded(text: &str, r: &Range<usize>, p: Preset) -> bool {
    let before = text[..r.start].chars().next_back();
    let after = text[r.end..].chars().next();
    match p {
        Preset::Aadhaar | Preset::Card | Preset::Phone => {
            // Not the middle of a longer number written in groups: look past up to two
            // separators on each side for another digit.
            let b: Vec<char> = text[..r.start].chars().rev().take(3).collect();
            let a: Vec<char> = text[r.end..].chars().take(3).collect();
            let continues = |v: &[char]| {
                v.iter()
                    .position(|c| *c != ' ' && *c != '-')
                    .is_some_and(|k| v[k].is_ascii_digit())
            };
            !continues(&b) && !continues(&a) && !(p == Preset::Phone && b.first() == Some(&'+'))
        }
        Preset::Pan | Preset::Iban => {
            !before.is_some_and(|c| c.is_ascii_alphanumeric())
                && !after.is_some_and(|c| c.is_ascii_alphanumeric())
        }
        Preset::Email => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(terms: &[&str], presets: &[Preset], text: &str) -> Vec<(String, &'static str)> {
        let t: Vec<String> = terms.iter().map(|s| s.to_string()).collect();
        Matcher::new(&t, presets)
            .unwrap()
            .find(text)
            .into_iter()
            .map(|h| (text[h.range].to_string(), h.kind))
            .collect()
    }

    #[test]
    fn the_checks_are_real() {
        assert!(luhn("4111 1111 1111 1111"));
        assert!(!luhn("4111 1111 1111 1112"));
        assert!(iban_ok("GB82 WEST 1234 5698 7654 32"));
        assert!(!iban_ok("GB82 WEST 1234 5698 7654 33"));
    }

    #[test]
    fn presets_find_their_shape_and_not_look_alikes() {
        let all = Preset::ALL;
        let r = found(
            &[],
            &all,
            "Mail anzal@example.com, call +91 98765 43210, Aadhaar 2345 6789 0124, PAN ABCDE1234F, card 4111 1111 1111 1111, IBAN GB82 WEST 1234 5698 7654 32.",
        );
        let kinds: Vec<&str> = r.iter().map(|x| x.1).collect();
        assert_eq!(
            kinds,
            ["email", "phone", "aadhaar", "pan", "card", "iban"],
            "{r:?}"
        );
        assert_eq!(r[1].0, "+91 98765 43210");
        assert_eq!(r[2].0, "2345 6789 0124");
        // Failing the check digit, or the wrong shape, is not a match.
        assert!(found(&[], &all, "card 4111 1111 1111 1112").is_empty());
        assert!(found(&[], &all, "id ABCDE12345 and ABCD1234F").is_empty());
        assert!(found(&[], &all, "order 123456789012445678901").is_empty());
    }

    #[test]
    fn a_term_spans_words_and_wins_over_a_preset() {
        let r = found(
            &["john   smith", "2345 6789 0124"],
            &Preset::ALL,
            "To JOHN Smith, id 2345 6789 0124.",
        );
        assert_eq!(
            r,
            [
                ("JOHN Smith".into(), "term"),
                ("2345 6789 0124".into(), "term")
            ]
        );
    }

    #[test]
    fn aadhaar_needs_its_check_digit_and_wrapped_numbers_are_found() {
        assert!(verhoeff_ok("2363"));
        assert!(verhoeff_ok("234567890124"));
        assert!(!verhoeff_ok("234567890123"));
        let p = [Preset::Aadhaar, Preset::Card];
        // A wrong check digit, or a first digit of 0 or 1, is not an Aadhaar.
        assert!(found(&[], &p, "id 2345 6789 0123 here").is_empty());
        assert!(found(&[], &p, "id 0345 6789 0124 here").is_empty());
        // Separators of one or two characters (a hyphen at a line end, then a space).
        let r = found(
            &[],
            &p,
            "id 2345 6789- 0124 here, card 4111 1111- 1111 1111.",
        );
        let kinds: Vec<&str> = r.iter().map(|x| x.1).collect();
        assert_eq!(kinds, ["aadhaar", "card"], "{r:?}");
        assert_eq!(found(&[], &p, "id 3456 7890 1238.").len(), 1);
        // A line break between groups (kept as a line feed in the page text) joins them, and
        // digits on the next line after a line break do not extend a number.
        let r = found(&[], &p, "id 2345 6789\n0124 end");
        assert_eq!(r.len(), 1, "{r:?}");
        assert!(found(&[], &p, "id 2345 6789 0124\n5678").len() == 1);
    }

    #[test]
    fn terms_respect_word_edges() {
        assert!(found(&["raj"], &[], "Rajesh and Raja").is_empty());
        assert_eq!(found(&["raj"], &[], "Hi Raj, bye").len(), 1);
        assert_eq!(found(&["a+b"], &[], "x a+b y").len(), 1);
    }

    #[test]
    fn preset_lists_parse() {
        assert_eq!(Preset::parse_list("").unwrap(), vec![]);
        assert_eq!(Preset::parse_list("all").unwrap().len(), 6);
        assert!(Preset::parse_list("email,bogus").is_err());
    }
}
