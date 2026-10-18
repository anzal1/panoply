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
            Preset::Aadhaar => r"\d{4}[ \-]\d{4}[ \-]\d{4}",
            Preset::Pan => r"[A-Z]{5}[0-9]{4}[A-Z]",
            Preset::Card => r"\d(?:[ \-]?\d){12,18}",
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
            Preset::Aadhaar => digits == 12,
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
            .map(|t| t.split_whitespace().map(regex::escape).collect::<Vec<_>>())
            .filter(|w| !w.is_empty())
            .map(|w| w.join(r"\s+"))
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
            // Not the middle of a longer number written in groups.
            let b: Vec<char> = text[..r.start].chars().rev().take(2).collect();
            let a: Vec<char> = text[r.end..].chars().take(2).collect();
            let sep = |c: char| c == ' ' || c == '-';
            let digit = |c: &char| c.is_ascii_digit();
            !b.first()
                .is_some_and(|c| digit(c) || (p == Preset::Phone && *c == '+'))
                && !a.first().is_some_and(digit)
                && !(b.first().is_some_and(|c| sep(*c)) && b.get(1).is_some_and(digit))
                && !(a.first().is_some_and(|c| sep(*c)) && a.get(1).is_some_and(digit))
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
            "Mail anzal@example.com, call +91 98765 43210, Aadhaar 2345 6789 0123, PAN ABCDE1234F, card 4111 1111 1111 1111, IBAN GB82 WEST 1234 5698 7654 32.",
        );
        let kinds: Vec<&str> = r.iter().map(|x| x.1).collect();
        assert_eq!(
            kinds,
            ["email", "phone", "aadhaar", "pan", "card", "iban"],
            "{r:?}"
        );
        assert_eq!(r[1].0, "+91 98765 43210");
        assert_eq!(r[2].0, "2345 6789 0123");
        // Failing the check digit, or the wrong shape, is not a match.
        assert!(found(&[], &all, "card 4111 1111 1111 1112").is_empty());
        assert!(found(&[], &all, "id ABCDE12345 and ABCD1234F").is_empty());
        assert!(found(&[], &all, "order 123456789012345678901").is_empty());
    }

    #[test]
    fn a_term_spans_words_and_wins_over_a_preset() {
        let r = found(
            &["john   smith", "2345 6789 0123"],
            &Preset::ALL,
            "To JOHN Smith, id 2345 6789 0123.",
        );
        assert_eq!(
            r,
            [
                ("JOHN Smith".into(), "term"),
                ("2345 6789 0123".into(), "term")
            ]
        );
    }

    #[test]
    fn preset_lists_parse() {
        assert_eq!(Preset::parse_list("").unwrap(), vec![]);
        assert_eq!(Preset::parse_list("all").unwrap().len(), 6);
        assert!(Preset::parse_list("email,bogus").is_err());
    }
}
