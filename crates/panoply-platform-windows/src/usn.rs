//! Byte decoders and encoders for the NTFS USN journal. No system calls: the layouts are read with
//! explicit little endian offsets, so a short or damaged buffer yields fewer records, never a panic.

/// A file was created.
pub const REASON_FILE_CREATE: u32 = 0x0000_0100;
/// A file or folder was deleted.
pub const REASON_FILE_DELETE: u32 = 0x0000_0200;
/// The old name of a rename.
pub const REASON_RENAME_OLD_NAME: u32 = 0x0000_1000;
/// The new name of a rename.
pub const REASON_RENAME_NEW_NAME: u32 = 0x0000_2000;
/// File data was overwritten.
pub const REASON_DATA_OVERWRITE: u32 = 0x0000_0001;
/// File data was extended.
pub const REASON_DATA_EXTEND: u32 = 0x0000_0002;
/// File data was truncated.
pub const REASON_DATA_TRUNCATION: u32 = 0x0000_0004;
/// Attributes or times changed.
pub const REASON_BASIC_INFO_CHANGE: u32 = 0x0000_8000;
/// The file was closed; the record then carries every reason since it was opened.
pub const REASON_CLOSE: u32 = 0x8000_0000;

/// The reasons the index cares about: names, sizes and times.
pub const WANTED_REASONS: u32 = REASON_FILE_CREATE
    | REASON_FILE_DELETE
    | REASON_RENAME_OLD_NAME
    | REASON_RENAME_NEW_NAME
    | REASON_DATA_OVERWRITE
    | REASON_DATA_EXTEND
    | REASON_DATA_TRUNCATION
    | REASON_BASIC_INFO_CHANGE
    | REASON_CLOSE;

/// `FILE_ATTRIBUTE_DIRECTORY`.
pub const ATTRIBUTE_DIRECTORY: u32 = 0x10;

/// One decoded `USN_RECORD_V2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsnRecord {
    /// The file reference number of the file or folder.
    pub frn: u64,
    /// The file reference number of its parent folder.
    pub parent: u64,
    /// The update sequence number of this record.
    pub usn: i64,
    /// A bit set of the `REASON_*` flags.
    pub reason: u32,
    /// The file attributes.
    pub attributes: u32,
    /// The name (UTF-16 code units, not a path).
    pub name: Vec<u16>,
}

impl UsnRecord {
    /// True for a folder.
    pub fn is_dir(&self) -> bool {
        self.attributes & ATTRIBUTE_DIRECTORY != 0
    }

    /// True when this record can change what a path below the folder means (a folder renamed or
    /// deleted), so cached folder paths must be dropped.
    pub fn moves_paths(&self) -> bool {
        self.is_dir()
            && self.reason & (REASON_RENAME_OLD_NAME | REASON_RENAME_NEW_NAME | REASON_FILE_DELETE)
                != 0
    }
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// Decodes the output of `FSCTL_READ_USN_JOURNAL`: the next USN to ask for, then the records. Records of
/// a major version other than 2 are skipped by their length.
pub fn parse_records(buf: &[u8]) -> (i64, Vec<UsnRecord>) {
    let Some(next) = u64_at(buf, 0) else {
        return (0, Vec::new());
    };
    let mut out = Vec::new();
    let mut off = 8usize;
    while let Some(len) = u32_at(buf, off) {
        let len = len as usize;
        // A record is at least its fixed part and never runs past the buffer.
        if len < 60 || off + len > buf.len() {
            break;
        }
        let rec = &buf[off..off + len];
        if u16_at(rec, 4) == Some(2)
            && let Some(r) = decode_v2(rec)
        {
            out.push(r);
        }
        // Records are 8 byte aligned and their length already includes the padding.
        off += len;
    }
    (next as i64, out)
}

fn decode_v2(rec: &[u8]) -> Option<UsnRecord> {
    let name_len = u16_at(rec, 56)? as usize;
    let name_off = u16_at(rec, 58)? as usize;
    let raw = rec.get(name_off..name_off + name_len)?;
    let name = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(UsnRecord {
        frn: u64_at(rec, 8)?,
        parent: u64_at(rec, 16)?,
        usn: u64_at(rec, 24)? as i64,
        reason: u32_at(rec, 40)?,
        attributes: u32_at(rec, 52)?,
        name,
    })
}

/// The part of `USN_JOURNAL_DATA` this crate reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalData {
    /// Identifies this incarnation of the journal; a changed id means it was deleted and recreated.
    pub id: u64,
    /// The oldest USN still in the journal.
    pub first: i64,
    /// The USN the next change will get.
    pub next: i64,
    /// The lowest USN a read may start from.
    pub lowest_valid: i64,
}

/// Decodes the output of `FSCTL_QUERY_USN_JOURNAL`.
pub fn parse_journal_data(buf: &[u8]) -> Option<JournalData> {
    Some(JournalData {
        id: u64_at(buf, 0)?,
        first: u64_at(buf, 8)? as i64,
        next: u64_at(buf, 16)? as i64,
        lowest_valid: u64_at(buf, 24)? as i64,
    })
}

/// Size of `READ_USN_JOURNAL_DATA_V1` (it is 8 byte aligned).
pub const READ_REQUEST_SIZE: usize = 48;

/// Encodes a `READ_USN_JOURNAL_DATA_V1`: read from `start`, only the reasons in `mask`, block until at
/// least one byte of records exists (`BytesToWaitFor` 1, `Timeout` 0 means no timeout), and ask for
/// version 2 records only.
pub fn encode_read_request(start: i64, mask: u32, journal_id: u64) -> [u8; READ_REQUEST_SIZE] {
    let mut b = [0u8; READ_REQUEST_SIZE];
    b[0..8].copy_from_slice(&start.to_le_bytes());
    b[8..12].copy_from_slice(&mask.to_le_bytes());
    // ReturnOnlyOnClose (4 bytes) stays 0, Timeout (8 bytes) stays 0.
    b[24..32].copy_from_slice(&1u64.to_le_bytes());
    b[32..40].copy_from_slice(&journal_id.to_le_bytes());
    b[40..42].copy_from_slice(&2u16.to_le_bytes());
    b[42..44].copy_from_slice(&2u16.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(frn: u64, parent: u64, usn: i64, reason: u32, attrs: u32, name: &str) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let name_bytes = units.len() * 2;
        let len = (60 + name_bytes).div_ceil(8) * 8;
        let mut r = vec![0u8; len];
        r[0..4].copy_from_slice(&(len as u32).to_le_bytes());
        r[4..6].copy_from_slice(&2u16.to_le_bytes());
        r[8..16].copy_from_slice(&frn.to_le_bytes());
        r[16..24].copy_from_slice(&parent.to_le_bytes());
        r[24..32].copy_from_slice(&usn.to_le_bytes());
        r[40..44].copy_from_slice(&reason.to_le_bytes());
        r[52..56].copy_from_slice(&attrs.to_le_bytes());
        r[56..58].copy_from_slice(&(name_bytes as u16).to_le_bytes());
        r[58..60].copy_from_slice(&60u16.to_le_bytes());
        for (i, u) in units.iter().enumerate() {
            r[60 + i * 2..62 + i * 2].copy_from_slice(&u.to_le_bytes());
        }
        r
    }

    #[test]
    fn decodes_two_records() {
        let mut buf = 4242u64.to_le_bytes().to_vec();
        buf.extend(record(7, 5, 100, REASON_FILE_CREATE, 0x20, "a.txt"));
        buf.extend(record(
            9,
            5,
            200,
            REASON_RENAME_NEW_NAME | REASON_CLOSE,
            ATTRIBUTE_DIRECTORY,
            "d\u{e9}j\u{e0} vu",
        ));
        let (next, recs) = parse_records(&buf);
        assert_eq!(next, 4242);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].frn, 7);
        assert_eq!(recs[0].parent, 5);
        assert_eq!(recs[0].usn, 100);
        assert_eq!(String::from_utf16_lossy(&recs[0].name), "a.txt");
        assert!(!recs[0].is_dir());
        assert!(!recs[0].moves_paths());
        assert_eq!(String::from_utf16_lossy(&recs[1].name), "d\u{e9}j\u{e0} vu");
        assert!(recs[1].is_dir());
        assert!(recs[1].moves_paths());
    }

    #[test]
    fn damaged_buffers_never_panic() {
        assert_eq!(parse_records(&[]), (0, Vec::new()));
        assert_eq!(parse_records(&[1, 2, 3]), (0, Vec::new()));
        let mut buf = 5u64.to_le_bytes().to_vec();
        buf.extend(record(1, 2, 3, REASON_FILE_DELETE, 0, "x"));
        // Cut anywhere: the complete records before the cut survive, nothing panics.
        for cut in 0..buf.len() {
            let (_, recs) = parse_records(&buf[..cut]);
            assert!(recs.len() <= 1);
        }
        // A length that runs past the buffer stops the walk.
        let mut bad = 5u64.to_le_bytes().to_vec();
        bad.extend(record(1, 2, 3, 0, 0, "x"));
        bad[8..12].copy_from_slice(&9999u32.to_le_bytes());
        assert!(parse_records(&bad).1.is_empty());
        // A length of zero must not loop forever.
        let mut zero = 5u64.to_le_bytes().to_vec();
        zero.extend([0u8; 64]);
        assert!(parse_records(&zero).1.is_empty());
    }

    #[test]
    fn other_versions_are_skipped_by_length() {
        let mut buf = 1u64.to_le_bytes().to_vec();
        let mut v3 = record(1, 2, 3, REASON_FILE_CREATE, 0, "v3");
        v3[4..6].copy_from_slice(&3u16.to_le_bytes());
        buf.extend(v3);
        buf.extend(record(8, 5, 9, REASON_FILE_CREATE, 0, "v2"));
        let (_, recs) = parse_records(&buf);
        assert_eq!(recs.len(), 1);
        assert_eq!(String::from_utf16_lossy(&recs[0].name), "v2");
    }

    #[test]
    fn journal_data_and_request() {
        let mut q = Vec::new();
        for v in [11u64, 22, 33, 44, 55, 66, 77] {
            q.extend(v.to_le_bytes());
        }
        let d = parse_journal_data(&q).unwrap();
        assert_eq!(
            d,
            JournalData {
                id: 11,
                first: 22,
                next: 33,
                lowest_valid: 44
            }
        );
        assert!(parse_journal_data(&q[..20]).is_none());
        let r = encode_read_request(1000, WANTED_REASONS, 11);
        assert_eq!(&r[0..8], &1000i64.to_le_bytes());
        assert_eq!(&r[8..12], &WANTED_REASONS.to_le_bytes());
        assert_eq!(&r[24..32], &1u64.to_le_bytes());
        assert_eq!(&r[32..40], &11u64.to_le_bytes());
        assert_eq!(&r[40..44], &[2, 0, 2, 0]);
    }
}
