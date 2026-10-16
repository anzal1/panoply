//! Decoder for the buffer `ReadDirectoryChangesW` fills with `FILE_NOTIFY_INFORMATION` records. No
//! system calls, so it is tested on every platform.

/// A file or folder was added.
pub const ACTION_ADDED: u32 = 1;
/// A file or folder was removed.
pub const ACTION_REMOVED: u32 = 2;
/// A file or folder was modified.
pub const ACTION_MODIFIED: u32 = 3;
/// The old name of a rename.
pub const ACTION_RENAMED_OLD: u32 = 4;
/// The new name of a rename.
pub const ACTION_RENAMED_NEW: u32 = 5;

/// One change report: what happened and the name relative to the watched folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// One of the `ACTION_*` values.
    pub action: u32,
    /// The path relative to the watched folder (UTF-16 code units, `\` separated).
    pub name: Vec<u16>,
}

/// Decodes `buf`, the first `bytes_returned` bytes of the completed read. A damaged buffer yields the
/// records before the damage.
pub fn parse_notices(buf: &[u8]) -> Vec<Notice> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while let Some(head) = buf.get(off..off + 12) {
        let next = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let action = u32::from_le_bytes([head[4], head[5], head[6], head[7]]);
        let name_len = u32::from_le_bytes([head[8], head[9], head[10], head[11]]) as usize;
        let Some(raw) = buf.get(off + 12..off + 12 + name_len) else {
            break;
        };
        out.push(Notice {
            action,
            name: raw
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect(),
        });
        if next == 0 {
            break;
        }
        off += next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(action: u32, name: &str, last: bool) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let name_bytes = units.len() * 2;
        let len = (12 + name_bytes).div_ceil(4) * 4;
        let mut r = vec![0u8; len];
        let next = if last { 0 } else { len as u32 };
        r[0..4].copy_from_slice(&next.to_le_bytes());
        r[4..8].copy_from_slice(&action.to_le_bytes());
        r[8..12].copy_from_slice(&(name_bytes as u32).to_le_bytes());
        for (i, u) in units.iter().enumerate() {
            r[12 + i * 2..14 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        r
    }

    #[test]
    fn decodes_a_chain() {
        let mut b = rec(ACTION_RENAMED_OLD, "a\\old.txt", false);
        b.extend(rec(ACTION_RENAMED_NEW, "a\\new.txt", false));
        b.extend(rec(ACTION_ADDED, "z", true));
        let n = parse_notices(&b);
        assert_eq!(n.len(), 3);
        assert_eq!(n[0].action, ACTION_RENAMED_OLD);
        assert_eq!(String::from_utf16_lossy(&n[1].name), "a\\new.txt");
        assert_eq!(n[2].action, ACTION_ADDED);
    }

    #[test]
    fn names_keep_non_bmp_characters_and_an_odd_byte_is_dropped() {
        // A character outside the BMP is two UTF-16 units; a stray last byte is ignored.
        let b = rec(ACTION_ADDED, "a\u{1F4C4}b", true);
        let n = parse_notices(&b);
        assert_eq!(String::from_utf16_lossy(&n[0].name), "a\u{1F4C4}b");
        let mut odd = b.clone();
        let len = u32::from_le_bytes([odd[8], odd[9], odd[10], odd[11]]) + 1;
        odd[8..12].copy_from_slice(&len.to_le_bytes());
        odd.push(0);
        assert_eq!(parse_notices(&odd)[0].name.len(), n[0].name.len());
    }

    #[test]
    fn a_next_offset_that_goes_nowhere_ends_the_walk() {
        let mut b = rec(ACTION_ADDED, "x", false);
        // Points far past the end: the first record is kept and the walk stops.
        b[0..4].copy_from_slice(&100_000u32.to_le_bytes());
        assert_eq!(parse_notices(&b).len(), 1);
    }

    #[test]
    fn damage_and_empty() {
        assert!(parse_notices(&[]).is_empty());
        let b = rec(ACTION_MODIFIED, "file", true);
        for cut in 0..b.len() {
            assert!(parse_notices(&b[..cut]).len() <= 1);
        }
        // A name length past the end stops the walk.
        let mut bad = b.clone();
        bad[8..12].copy_from_slice(&9999u32.to_le_bytes());
        assert!(parse_notices(&bad).is_empty());
    }
}
