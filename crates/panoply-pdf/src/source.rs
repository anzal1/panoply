//! Where PDF bytes come from. A file is read with positioned reads, never loaded whole, so a
//! 2 GB PDF costs the page tree and the objects in flight and nothing more.

use std::fs::File;
use std::io;

/// A random-access byte source: a file on disk, or bytes in memory (a repaired copy).
pub enum Source {
    /// A file, read with positioned reads.
    File {
        /// The open file.
        file: File,
        /// Its length when it was opened.
        len: u64,
    },
    /// Bytes in memory.
    Mem(Vec<u8>),
}

impl Source {
    /// Wrap an open file.
    pub fn from_file(file: File) -> io::Result<Source> {
        let len = file.metadata()?.len();
        Ok(Source::File { file, len })
    }

    /// Wrap bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Source {
        Source::Mem(bytes)
    }

    /// Length in bytes.
    pub fn len(&self) -> u64 {
        match self {
            Source::File { len, .. } => *len,
            Source::Mem(v) => v.len() as u64,
        }
    }

    /// True when there are no bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Read up to `buf.len()` bytes at `offset`. Returns how many were read (0 at the end).
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Source::File { file, .. } => read_at_file(file, offset, buf),
            Source::Mem(v) => {
                let start = usize::try_from(offset).unwrap_or(usize::MAX);
                if start >= v.len() {
                    return Ok(0);
                }
                let n = buf.len().min(v.len() - start);
                buf[..n].copy_from_slice(&v[start..start + n]);
                Ok(n)
            }
        }
    }

    /// Read up to `len` bytes at `offset` into a new vector (shorter at the end of the source).
    pub fn read_range(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        let avail = self.len().saturating_sub(offset);
        let want = (len as u64).min(avail) as usize;
        let mut out = vec![0u8; want];
        let mut got = 0;
        while got < want {
            let n = self.read_at(offset + got as u64, &mut out[got..])?;
            if n == 0 {
                break;
            }
            got += n;
        }
        out.truncate(got);
        Ok(out)
    }
}

#[cfg(unix)]
fn read_at_file(file: &File, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    loop {
        match file.read_at(buf, offset) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            r => return r,
        }
    }
}

#[cfg(windows)]
fn read_at_file(file: &File, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_read(buf, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_reads_clamp_at_the_end() {
        let s = Source::from_bytes(b"hello world".to_vec());
        assert_eq!(s.read_range(6, 100).unwrap(), b"world");
        assert_eq!(s.read_range(20, 4).unwrap(), b"");
        assert_eq!(s.len(), 11);
    }

    #[test]
    fn file_reads_match_mem() {
        let p = std::env::temp_dir().join(format!("panoply-src-{}", std::process::id()));
        std::fs::write(&p, b"0123456789").unwrap();
        let s = Source::from_file(File::open(&p).unwrap()).unwrap();
        assert_eq!(s.read_range(3, 4).unwrap(), b"3456");
        assert_eq!(s.read_range(8, 10).unwrap(), b"89");
        std::fs::remove_file(&p).unwrap();
    }
}
