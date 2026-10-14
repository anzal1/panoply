//! Positioned reads of a file, so an MP4 of any size is read by pieces and never loaded whole.

use std::fs::File;
use std::io;
use std::path::Path;

/// A file read with positioned reads.
pub struct Source {
    file: File,
    len: u64,
}

impl Source {
    /// Open a file.
    pub fn open(path: &Path) -> io::Result<Source> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Source { file, len })
    }

    /// Length in bytes.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Read up to `buf.len()` bytes at `offset`; returns how many (0 at the end).
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        read_at_file(&self.file, offset, buf)
    }

    /// Read exactly `len` bytes at `offset`, or fail with `UnexpectedEof`.
    pub fn read_exact_at(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut out = vec![0u8; len];
        let mut got = 0;
        while got < len {
            let n = self.read_at(offset + got as u64, &mut out[got..])?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            got += n;
        }
        Ok(out)
    }

    /// Read up to `len` bytes at `offset` (shorter at the end of the file).
    pub fn read_up_to(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        let avail = self.len.saturating_sub(offset);
        let want = (len as u64).min(avail) as usize;
        self.read_exact_at(offset, want)
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
    fn reads_clamp_at_the_end() {
        let p = std::env::temp_dir().join(format!("panoply-mp4src-{}", std::process::id()));
        std::fs::write(&p, b"0123456789").unwrap();
        let s = Source::open(&p).unwrap();
        assert_eq!(s.read_up_to(3, 4).unwrap(), b"3456");
        assert_eq!(s.read_up_to(8, 10).unwrap(), b"89");
        assert!(s.read_exact_at(8, 10).is_err());
        assert_eq!(s.len(), 10);
        std::fs::remove_file(&p).unwrap();
    }
}
