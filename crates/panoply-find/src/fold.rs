//! Case and accent folding. The index stores a folded copy of every name next to the original, and a
//! query is folded the same way, so matching is a plain byte search.
//!
//! The fold is: ASCII letters lowercase; Latin-1 and Latin Extended-A letters lose their accent and
//! their case ("Cafe\u{301}", "cafe\u{301}" in NFD and "CAFÉ" in NFC all fold to "cafe", which is what
//! makes macOS NFD file names match what the user types); combining marks U+0300 to U+036F are dropped;
//! every other character is lowercased with the standard library. The folded form can be shorter than
//! the original, which is why the index keeps one offset table per buffer.

/// A 32-bit summary of which kinds of characters a name holds: bits 0 to 25 are the letters a to z,
/// bit 26 any digit, 27 a space, 28 an underscore, 29 a hyphen, 30 a dot, 31 anything else. A fuzzy
/// query can only match a name whose mask has every bit of the query's mask, so one AND rejects most
/// names before any real matching.
pub type CharMask = u32;

#[inline]
fn mask_bit(b: u8) -> CharMask {
    match b {
        b'a'..=b'z' => 1 << (b - b'a'),
        b'0'..=b'9' => 1 << 26,
        b' ' => 1 << 27,
        b'_' => 1 << 28,
        b'-' => 1 << 29,
        b'.' => 1 << 30,
        _ => 1 << 31,
    }
}

fn latin1_base(cp: u32) -> u8 {
    match cp {
        0xC0..=0xC6 | 0xE0..=0xE6 => b'a',
        0xC7 | 0xE7 => b'c',
        0xC8..=0xCB | 0xE8..=0xEB => b'e',
        0xCC..=0xCF | 0xEC..=0xEF => b'i',
        0xD0 | 0xF0 => b'd',
        0xD1 | 0xF1 => b'n',
        0xD2..=0xD6 | 0xD8 | 0xF2..=0xF6 | 0xF8 => b'o',
        0xD9..=0xDC | 0xF9..=0xFC => b'u',
        0xDD | 0xFD | 0xFF => b'y',
        0xDF => b's',
        _ => 0,
    }
}

fn latin_ext_a_base(cp: u32) -> u8 {
    match cp {
        0x100..=0x105 => b'a',
        0x106..=0x10D => b'c',
        0x10E..=0x111 => b'd',
        0x112..=0x11B => b'e',
        0x11C..=0x123 => b'g',
        0x124..=0x127 => b'h',
        0x128..=0x133 => b'i',
        0x134..=0x135 => b'j',
        0x136..=0x138 => b'k',
        0x139..=0x142 => b'l',
        0x143..=0x14B => b'n',
        0x14C..=0x153 => b'o',
        0x154..=0x159 => b'r',
        0x15A..=0x161 => b's',
        0x162..=0x167 => b't',
        0x168..=0x173 => b'u',
        0x174..=0x175 => b'w',
        0x176..=0x178 => b'y',
        0x179..=0x17E => b'z',
        0x17F => b's',
        _ => 0,
    }
}

/// Appends the folded form of `s` to `out` and returns its [`CharMask`].
pub fn fold_into(s: &str, out: &mut Vec<u8>) -> CharMask {
    let mut mask = 0;
    if s.is_ascii() {
        out.reserve(s.len());
        for &b in s.as_bytes() {
            let l = b.to_ascii_lowercase();
            mask |= mask_bit(l);
            out.push(l);
        }
        return mask;
    }
    let mut buf = [0u8; 4];
    for c in s.chars() {
        let cp = c as u32;
        if (0x300..=0x36F).contains(&cp) {
            continue;
        }
        let base = match cp {
            0x00..=0x7F => (c as u8).to_ascii_lowercase(),
            0xC0..=0xFF => latin1_base(cp),
            0x100..=0x17F => latin_ext_a_base(cp),
            _ => 0,
        };
        if base != 0 {
            mask |= mask_bit(base);
            out.push(base);
        } else if cp < 0x80 {
            mask |= mask_bit(cp as u8);
            out.push(cp as u8);
        } else {
            for l in c.to_lowercase() {
                out.extend_from_slice(l.encode_utf8(&mut buf).as_bytes());
            }
            mask |= 1 << 31;
        }
    }
    mask
}

/// The folded form of `s` as a new vector, and its mask.
pub fn fold(s: &str) -> (Vec<u8>, CharMask) {
    let mut v = Vec::with_capacity(s.len());
    let m = fold_into(s, &mut v);
    (v, m)
}

/// The [`CharMask`] of an already folded byte string.
pub fn mask_of_folded(folded: &[u8]) -> CharMask {
    folded
        .iter()
        .fold(0, |m, &b| m | if b < 0x80 { mask_bit(b) } else { 1 << 31 })
}

/// True for the bytes that start a new word inside a name: any ASCII byte that is not a letter or digit.
#[inline]
pub fn is_separator(b: u8) -> bool {
    b < 0x80 && !b.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_lowercases() {
        assert_eq!(fold("Quarterly_Report.PDF").0, b"quarterly_report.pdf");
    }

    #[test]
    fn nfd_and_nfc_agree() {
        let nfc = fold("Caf\u{e9}").0;
        let nfd = fold("Cafe\u{301}").0;
        assert_eq!(nfc, b"cafe");
        assert_eq!(nfd, b"cafe");
        assert_eq!(fold("CAF\u{c9}").0, b"cafe");
        assert_eq!(fold("Łódź").0, b"lodz");
    }

    #[test]
    fn other_scripts_lowercase() {
        assert_eq!(fold("ПРИВЕТ").0, "привет".as_bytes());
    }

    #[test]
    fn mask_covers_every_folded_byte() {
        let (f, m) = fold("My File-2.txt");
        for &b in &f {
            assert_ne!(m & mask_bit(b), 0);
        }
    }

    #[test]
    fn every_latin_letter_in_range_maps_to_ascii() {
        for cp in 0xC0u32..=0x17F {
            let ch = char::from_u32(cp).unwrap();
            let f = fold(&ch.to_string()).0;
            if cp == 0xD7 || cp == 0xF7 || cp == 0xDE || cp == 0xFE {
                continue;
            }
            assert!(
                f.iter().all(u8::is_ascii_lowercase),
                "U+{cp:04X} folded to {f:?}"
            );
        }
    }
}
