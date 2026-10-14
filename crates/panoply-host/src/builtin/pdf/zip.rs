//! A small streaming ZIP writer (stored entries, no compression) for "put the pictures in a
//! ZIP". PNG and JPG are already compressed, so storing them is the right choice. Each entry is
//! written once, with its CRC and size in a data descriptor after the bytes, so a page image goes
//! straight from the renderer to the file without being held in memory.

use std::io::{self, Write};

struct Entry {
    name: String,
    crc: u32,
    size: u32,
    offset: u32,
}

/// Writes a ZIP archive to `W`.
pub struct Zip<W: Write> {
    out: W,
    pos: u64,
    entries: Vec<Entry>,
}

fn crc_table() -> &'static [u32; 256] {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    })
}

/// Counts and checksums what passes through.
struct Tee<'a, W: Write> {
    out: &'a mut W,
    crc: u32,
    size: u64,
}

impl<W: Write> Write for Tee<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.out.write(buf)?;
        let t = crc_table();
        let mut c = !self.crc;
        for &b in &buf[..n] {
            c = t[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
        }
        self.crc = !c;
        self.size += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

fn too_big() -> io::Error {
    io::Error::other(
        "The ZIP would be larger than 4 GB or have more than 65,535 files. Turn the ZIP off, or pick fewer pages.",
    )
}

impl<W: Write> Zip<W> {
    /// Start an archive.
    pub fn new(out: W) -> Self {
        Zip {
            out,
            pos: 0,
            entries: Vec::new(),
        }
    }

    /// Add a file. `fill` writes its bytes.
    pub fn add<E: From<io::Error>>(
        &mut self,
        name: &str,
        fill: impl FnOnce(&mut dyn Write) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.entries.len() >= 65_535 || self.pos > u64::from(u32::MAX) - (1 << 20) {
            return Err(too_big().into());
        }
        let offset = self.pos as u32;
        let mut head = Vec::with_capacity(30 + name.len());
        head.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        head.extend_from_slice(&20u16.to_le_bytes());
        head.extend_from_slice(&0x0808u16.to_le_bytes()); // data descriptor, UTF-8 names
        head.extend_from_slice(&0u16.to_le_bytes()); // stored
        head.extend_from_slice(&0u16.to_le_bytes()); // time
        head.extend_from_slice(&0x0021u16.to_le_bytes()); // date: 1980-01-01
        head.extend_from_slice(&[0; 12]); // crc, sizes: in the descriptor
        head.extend_from_slice(&(name.len() as u16).to_le_bytes());
        head.extend_from_slice(&0u16.to_le_bytes());
        head.extend_from_slice(name.as_bytes());
        self.out.write_all(&head)?;
        self.pos += head.len() as u64;
        let mut tee = Tee {
            out: &mut self.out,
            crc: 0,
            size: 0,
        };
        fill(&mut tee)?;
        let (crc, size) = (tee.crc, tee.size);
        if size > u64::from(u32::MAX) {
            return Err(too_big().into());
        }
        let mut desc = Vec::with_capacity(16);
        desc.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
        desc.extend_from_slice(&crc.to_le_bytes());
        desc.extend_from_slice(&(size as u32).to_le_bytes());
        desc.extend_from_slice(&(size as u32).to_le_bytes());
        self.out.write_all(&desc)?;
        self.pos += size + desc.len() as u64;
        self.entries.push(Entry {
            name: name.to_string(),
            crc,
            size: size as u32,
            offset,
        });
        Ok(())
    }

    /// Write the directory and give the output back.
    pub fn finish(mut self) -> io::Result<W> {
        let start = self.pos;
        let mut dir = Vec::new();
        for e in &self.entries {
            dir.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            dir.extend_from_slice(&20u16.to_le_bytes());
            dir.extend_from_slice(&20u16.to_le_bytes());
            dir.extend_from_slice(&0x0808u16.to_le_bytes());
            dir.extend_from_slice(&0u16.to_le_bytes());
            dir.extend_from_slice(&0u16.to_le_bytes());
            dir.extend_from_slice(&0x0021u16.to_le_bytes());
            dir.extend_from_slice(&e.crc.to_le_bytes());
            dir.extend_from_slice(&e.size.to_le_bytes());
            dir.extend_from_slice(&e.size.to_le_bytes());
            dir.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            dir.extend_from_slice(&[0; 12]); // extra, comment, disk, internal and external attributes
            dir.extend_from_slice(&e.offset.to_le_bytes());
            dir.extend_from_slice(e.name.as_bytes());
        }
        let mut end = Vec::new();
        end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        end.extend_from_slice(&[0; 4]);
        end.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        end.extend_from_slice(&(dir.len() as u32).to_le_bytes());
        end.extend_from_slice(&(start as u32).to_le_bytes());
        end.extend_from_slice(&0u16.to_le_bytes());
        self.out.write_all(&dir)?;
        self.out.write_all(&end)?;
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_known_check_value() {
        let mut sink = Vec::new();
        let mut tee = Tee {
            out: &mut sink,
            crc: 0,
            size: 0,
        };
        tee.write_all(b"123456789").unwrap();
        assert_eq!(tee.crc, 0xCBF4_3926);
        assert_eq!(tee.size, 9);
    }

    #[test]
    fn archive_has_a_directory_with_every_entry() {
        let mut z = Zip::new(Vec::new());
        z.add::<io::Error>("a.txt", |w| w.write_all(b"hello"))
            .unwrap();
        z.add::<io::Error>("b.txt", |w| w.write_all(b"")).unwrap();
        let bytes = z.finish().unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]), 2);
    }
}
