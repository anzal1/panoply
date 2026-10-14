//! A streaming ZIP writer: stored entries (no compression), data descriptors, no temporary
//! file. The total length is known in advance, so browsers can show a progress bar.
//!
//! Zip64 records are written when the archive needs them (4 GiB or more, or 65,535 entries or
//! more), and the choice is made up front from the file sizes.

use std::io;
use std::path::PathBuf;

use bytes::Bytes;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

use crate::proto::civil;

const CHUNK: usize = 256 * 1024;
const U32_MAX: u64 = 0xFFFF_FFFF;

/// One file in the archive.
#[derive(Debug, Clone)]
pub struct ZipEntry {
    /// Name inside the archive, `/` separated.
    pub name: String,
    /// File to read.
    pub path: PathBuf,
    /// Size in bytes, as planned. A file that changes size while streaming aborts the stream.
    pub size: u64,
    /// Modification time, Unix seconds.
    pub mtime: i64,
}

/// A planned archive.
#[derive(Debug, Clone)]
pub struct ZipPlan {
    entries: Vec<ZipEntry>,
    zip64: bool,
}

fn name_bytes(e: &ZipEntry) -> Vec<u8> {
    e.name.trim_start_matches('/').as_bytes().to_vec()
}

impl ZipPlan {
    /// Plan an archive. `force_zip64` writes Zip64 records even when not needed (for tests).
    pub fn new(entries: Vec<ZipEntry>, force_zip64: bool) -> Self {
        let mut plan = ZipPlan {
            entries,
            zip64: false,
        };
        plan.zip64 = force_zip64
            || plan.entries.len() >= 0xFFFF
            || plan.entries.iter().any(|e| e.size >= U32_MAX)
            || plan.len_with(false) >= U32_MAX;
        plan
    }

    /// True when Zip64 records are used.
    pub fn is_zip64(&self) -> bool {
        self.zip64
    }

    fn len_with(&self, z64: bool) -> u64 {
        let (extra_local, desc, extra_central) = if z64 { (20, 24, 28) } else { (0, 16, 0) };
        let mut n = 0u64;
        for e in &self.entries {
            let nl = name_bytes(e).len() as u64;
            n += 30 + nl + extra_local + e.size + desc;
            n += 46 + nl + extra_central;
        }
        n + 22 + if z64 { 56 + 20 } else { 0 }
    }

    /// Exact length of the archive in bytes.
    pub fn total_len(&self) -> u64 {
        self.len_with(self.zip64)
    }

    /// Stream the archive into `tx`. Returns when everything was sent, the client went away
    /// (the channel closed), or a file could not be read.
    pub async fn write(self, tx: mpsc::Sender<io::Result<Bytes>>) -> io::Result<()> {
        let z64 = self.zip64;
        let mut offset = 0u64;
        let mut central: Vec<u8> = Vec::new();
        let mut buf = vec![0u8; CHUNK];
        let broken = || io::Error::new(io::ErrorKind::BrokenPipe, "the client went away");

        for e in &self.entries {
            let name = name_bytes(e);
            let (date, time) = dos_datetime(e.mtime);
            let local_offset = offset;

            let mut h = Vec::with_capacity(64 + name.len());
            h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            h.extend_from_slice(&(if z64 { 45u16 } else { 20 }).to_le_bytes());
            h.extend_from_slice(&0x0808u16.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(&time.to_le_bytes());
            h.extend_from_slice(&date.to_le_bytes());
            h.extend_from_slice(&0u32.to_le_bytes());
            let sizes32 = if z64 { 0xFFFF_FFFFu32 } else { 0 };
            h.extend_from_slice(&sizes32.to_le_bytes());
            h.extend_from_slice(&sizes32.to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&(if z64 { 20u16 } else { 0 }).to_le_bytes());
            h.extend_from_slice(&name);
            if z64 {
                h.extend_from_slice(&1u16.to_le_bytes());
                h.extend_from_slice(&16u16.to_le_bytes());
                h.extend_from_slice(&0u64.to_le_bytes());
                h.extend_from_slice(&0u64.to_le_bytes());
            }
            offset += h.len() as u64;
            tx.send(Ok(h.into())).await.map_err(|_| broken())?;

            let mut file = tokio::fs::File::open(&e.path).await?;
            let mut crc = crc32fast::Hasher::new();
            let mut got = 0u64;
            loop {
                let n = file.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                got += n as u64;
                if got > e.size {
                    break;
                }
                crc.update(&buf[..n]);
                tx.send(Ok(Bytes::copy_from_slice(&buf[..n])))
                    .await
                    .map_err(|_| broken())?;
            }
            if got != e.size {
                let err = io::Error::other(format!("{} changed while it was being zipped", e.name));
                let _ = tx.send(Err(io::Error::other(err.to_string()))).await;
                return Err(err);
            }
            offset += got;
            let crc = crc.finalize();

            let mut d = Vec::with_capacity(24);
            d.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
            d.extend_from_slice(&crc.to_le_bytes());
            if z64 {
                d.extend_from_slice(&got.to_le_bytes());
                d.extend_from_slice(&got.to_le_bytes());
            } else {
                d.extend_from_slice(&(got as u32).to_le_bytes());
                d.extend_from_slice(&(got as u32).to_le_bytes());
            }
            offset += d.len() as u64;
            tx.send(Ok(d.into())).await.map_err(|_| broken())?;

            // Central directory record, written at the end.
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&(0x0300u16 | if z64 { 45 } else { 20 }).to_le_bytes());
            central.extend_from_slice(&(if z64 { 45u16 } else { 20 }).to_le_bytes());
            central.extend_from_slice(&0x0808u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&time.to_le_bytes());
            central.extend_from_slice(&date.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            if z64 {
                central.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
                central.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
            } else {
                central.extend_from_slice(&(got as u32).to_le_bytes());
                central.extend_from_slice(&(got as u32).to_le_bytes());
            }
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&(if z64 { 28u16 } else { 0 }).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&(0o100_644u32 << 16).to_le_bytes());
            if z64 {
                central.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
            } else {
                central.extend_from_slice(&(local_offset as u32).to_le_bytes());
            }
            central.extend_from_slice(&name);
            if z64 {
                central.extend_from_slice(&1u16.to_le_bytes());
                central.extend_from_slice(&24u16.to_le_bytes());
                central.extend_from_slice(&got.to_le_bytes());
                central.extend_from_slice(&got.to_le_bytes());
                central.extend_from_slice(&local_offset.to_le_bytes());
            }
        }

        let cd_offset = offset;
        let cd_size = central.len() as u64;
        let count = self.entries.len() as u64;
        let mut tail = central;
        if z64 {
            let eocd64_offset = cd_offset + cd_size;
            tail.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
            tail.extend_from_slice(&44u64.to_le_bytes());
            tail.extend_from_slice(&(0x0300u16 | 45).to_le_bytes());
            tail.extend_from_slice(&45u16.to_le_bytes());
            tail.extend_from_slice(&0u32.to_le_bytes());
            tail.extend_from_slice(&0u32.to_le_bytes());
            tail.extend_from_slice(&count.to_le_bytes());
            tail.extend_from_slice(&count.to_le_bytes());
            tail.extend_from_slice(&cd_size.to_le_bytes());
            tail.extend_from_slice(&cd_offset.to_le_bytes());
            tail.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
            tail.extend_from_slice(&0u32.to_le_bytes());
            tail.extend_from_slice(&eocd64_offset.to_le_bytes());
            tail.extend_from_slice(&1u32.to_le_bytes());
        }
        tail.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        tail.extend_from_slice(&0u16.to_le_bytes());
        tail.extend_from_slice(&0u16.to_le_bytes());
        if z64 {
            tail.extend_from_slice(&0xFFFFu16.to_le_bytes());
            tail.extend_from_slice(&0xFFFFu16.to_le_bytes());
            tail.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
            tail.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        } else {
            tail.extend_from_slice(&(count as u16).to_le_bytes());
            tail.extend_from_slice(&(count as u16).to_le_bytes());
            tail.extend_from_slice(&(cd_size as u32).to_le_bytes());
            tail.extend_from_slice(&(cd_offset as u32).to_le_bytes());
        }
        tail.extend_from_slice(&0u16.to_le_bytes());
        tx.send(Ok(tail.into())).await.map_err(|_| broken())?;
        Ok(())
    }
}

/// MS-DOS date and time for a Unix timestamp (clamped to 1980 to 2107).
fn dos_datetime(secs: i64) -> (u16, u16) {
    let (y, m, d, hh, mm, ss) = civil(secs.max(0));
    if y < 1980 {
        return ((1 << 5) | 1, 0);
    }
    let y = (y.min(2107) - 1980) as u16;
    let date = (y << 9) | ((m as u16) << 5) | d as u16;
    let time = ((hh as u16) << 11) | ((mm as u16) << 5) | (ss as u16 / 2);
    (date, time)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_time_round() {
        // 2021-01-01 12:34:56 UTC
        let (date, time) = dos_datetime(1_609_504_496);
        assert_eq!(date, ((2021 - 1980) << 9) | (1 << 5) | 1);
        assert_eq!(time, (12 << 11) | (34 << 5) | 28);
        assert_eq!(dos_datetime(0).0, (1 << 5) | 1);
    }

    #[test]
    fn zip64_is_chosen_up_front() {
        let big = ZipEntry {
            name: "big".into(),
            path: PathBuf::new(),
            size: 5 << 30,
            mtime: 0,
        };
        assert!(ZipPlan::new(vec![big], false).is_zip64());
        let small = ZipEntry {
            name: "s".into(),
            path: PathBuf::new(),
            size: 10,
            mtime: 0,
        };
        assert!(!ZipPlan::new(vec![small.clone()], false).is_zip64());
        assert!(ZipPlan::new(vec![small], true).is_zip64());
    }
}
