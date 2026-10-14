//! A streaming `multipart/form-data` parser for the QR upload page.
//!
//! Fed arbitrary chunks, it never holds more than one delimiter's worth of file data back,
//! so a multi gigabyte upload streams straight to disk.

/// What the parser found.
#[derive(Debug, PartialEq, Eq)]
pub enum Part {
    /// A file part starts. The name is the raw `filename` the browser sent.
    FileStart(String),
    /// File content.
    Data(Vec<u8>),
    /// The current file part ended.
    FileEnd,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    /// Inside a body (or the preamble), looking for the next delimiter.
    Body,
    /// Just after a delimiter: `--` ends the message, CRLF starts a part.
    AfterDelim,
    /// Reading part headers.
    Headers,
    /// The closing delimiter was seen.
    Done,
}

/// Incremental parser.
pub struct Multipart {
    delim: Vec<u8>,
    buf: Vec<u8>,
    state: State,
    in_file: bool,
}

const MAX_HEADER_BYTES: usize = 16 * 1024;

impl Multipart {
    /// Start parsing a body with this boundary (from the `Content-Type` header).
    pub fn new(boundary: &str) -> Self {
        let mut delim = b"\r\n--".to_vec();
        delim.extend_from_slice(boundary.as_bytes());
        // The first boundary has no CRLF before it. Pretend there was one.
        Multipart {
            delim,
            buf: b"\r\n".to_vec(),
            state: State::Body,
            in_file: false,
        }
    }

    /// True once the closing delimiter has been read.
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// Feed the next chunk, appending findings to `out`.
    pub fn feed(&mut self, chunk: &[u8], out: &mut Vec<Part>) -> Result<(), &'static str> {
        if self.state == State::Done {
            return Ok(());
        }
        self.buf.extend_from_slice(chunk);
        loop {
            match self.state {
                State::Done => return Ok(()),
                State::Body => match find(&self.buf, &self.delim) {
                    Some(i) => {
                        if self.in_file {
                            if i > 0 {
                                out.push(Part::Data(self.buf[..i].to_vec()));
                            }
                            out.push(Part::FileEnd);
                            self.in_file = false;
                        }
                        self.buf.drain(..i + self.delim.len());
                        self.state = State::AfterDelim;
                    }
                    None => {
                        // Hold back a possible partial delimiter at the end.
                        let keep = self.delim.len() - 1;
                        if self.buf.len() > keep {
                            let n = self.buf.len() - keep;
                            if self.in_file {
                                out.push(Part::Data(self.buf[..n].to_vec()));
                            }
                            self.buf.drain(..n);
                        }
                        return Ok(());
                    }
                },
                State::AfterDelim => {
                    if self.buf.len() < 2 {
                        return Ok(());
                    }
                    if self.buf.starts_with(b"--") {
                        self.state = State::Done;
                        self.buf.clear();
                        return Ok(());
                    }
                    // Some clients put spaces before the CRLF: skip to it.
                    match find(&self.buf, b"\r\n") {
                        Some(i) if self.buf[..i].iter().all(|b| *b == b' ' || *b == b'\t') => {
                            self.buf.drain(..i + 2);
                            self.state = State::Headers;
                        }
                        Some(_) => return Err("malformed boundary line"),
                        None if self.buf.len() > 256 => return Err("malformed boundary line"),
                        None => return Ok(()),
                    }
                }
                State::Headers => match find(&self.buf, b"\r\n\r\n") {
                    Some(i) => {
                        let head = String::from_utf8_lossy(&self.buf[..i]).into_owned();
                        self.buf.drain(..i + 4);
                        if let Some(name) = filename_of(&head) {
                            out.push(Part::FileStart(name));
                            self.in_file = true;
                        }
                        self.state = State::Body;
                    }
                    None if self.buf.len() > MAX_HEADER_BYTES => {
                        return Err("part headers too large");
                    }
                    None => return Ok(()),
                },
            }
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let first = needle[0];
    let last = hay.len() - needle.len();
    let mut i = 0;
    while i <= last {
        i += hay[i..=last].iter().position(|b| *b == first)?;
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The file name from a part's headers, if it has one.
fn filename_of(head: &str) -> Option<String> {
    for line in head.split("\r\n") {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if !k.trim().eq_ignore_ascii_case("content-disposition") {
            continue;
        }
        // RFC 5987 form first: filename*=UTF-8''percent-encoded
        if let Some(i) = v.find("filename*=") {
            let rest = &v[i + 10..];
            let rest = rest.split(';').next().unwrap_or("").trim();
            if let Some((_, enc)) = rest.split_once("''") {
                return Some(crate::http::url_decode(&enc.replace('+', "%2B")));
            }
        }
        if let Some(i) = v.find("filename=") {
            let rest = v[i + 9..].trim_start();
            if let Some(q) = rest.strip_prefix('"') {
                // Browsers do not backslash-escape: a quote is sent as %22 and a Windows path
                // arrives with its backslashes as they are.
                let raw = q.split('"').next().unwrap_or("");
                return Some(
                    raw.replace("%22", "\"")
                        .replace("%0D", "")
                        .replace("%0A", ""),
                );
            }
            return Some(rest.split(';').next().unwrap_or("").trim().to_string());
        }
        // A form field, not a file.
        return None;
    }
    None
}

/// The boundary parameter of a `multipart/form-data` content type.
pub fn boundary_of(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    if !lower.trim_start().starts_with("multipart/form-data") {
        return None;
    }
    let i = lower.find("boundary=")?;
    let rest = &content_type[i + 9..];
    let b = match rest.strip_prefix('"') {
        Some(q) => q.split('"').next()?,
        None => rest.split(';').next()?.trim(),
    };
    (!b.is_empty() && b.len() <= 200).then(|| b.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(boundary: &str, file: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(b"preamble text\r\n");
        b.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nhello\r\n"
            )
            .as_bytes(),
        );
        b.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"a %22q%22.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
        b.extend_from_slice(file);
        b.extend_from_slice(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"f\"; filename=\"empty.txt\"\r\n\r\n\r\n--{boundary}--\r\nepilogue").as_bytes());
        b
    }

    fn run(chunks: &[&[u8]], boundary: &str) -> (Vec<(String, Vec<u8>)>, bool) {
        let mut p = Multipart::new(boundary);
        let mut out = Vec::new();
        for c in chunks {
            p.feed(c, &mut out).unwrap();
        }
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        for part in out {
            match part {
                Part::FileStart(n) => files.push((n, Vec::new())),
                Part::Data(d) => files.last_mut().unwrap().1.extend(d),
                Part::FileEnd => {}
            }
        }
        (files, p.is_done())
    }

    #[test]
    fn parses_at_every_split_point() {
        let boundary = "----WebKitFormBoundaryAbC123";
        // Content that looks like a delimiter.
        let mut file = b"start\r\n--".to_vec();
        file.extend_from_slice(b"----WebKitFormBoundaryAbC12"); // one char short
        file.extend_from_slice(b"\r\n--\r\n\r\nend");
        let whole = body(boundary, &file);
        let (expected, done) = run(&[&whole], boundary);
        assert!(done);
        assert_eq!(expected.len(), 2);
        assert_eq!(expected[0].0, "a \"q\".bin");
        assert_eq!(expected[0].1, file);
        assert_eq!(expected[1], ("empty.txt".to_string(), Vec::new()));
        for i in 0..=whole.len() {
            let (a, b) = whole.split_at(i);
            assert_eq!(
                run(&[a, b], boundary),
                (expected.clone(), true),
                "split at {i}"
            );
        }
        let singles: Vec<&[u8]> = whole.chunks(1).collect();
        assert_eq!(run(&singles, boundary), (expected, true));
    }

    #[test]
    fn boundary_and_filename_parsing() {
        assert_eq!(
            boundary_of("multipart/form-data; boundary=\"abc def\"").as_deref(),
            Some("abc def")
        );
        assert_eq!(
            boundary_of("multipart/form-data; boundary=xyz").as_deref(),
            Some("xyz")
        );
        assert_eq!(boundary_of("application/json"), None);
        assert_eq!(
            filename_of(
                "Content-Disposition: form-data; name=\"f\"; filename*=UTF-8''na%C3%AFve%20file.txt"
            )
            .as_deref(),
            Some("naïve file.txt")
        );
    }

    #[test]
    fn malformed_input_errors_instead_of_hanging() {
        let mut p = Multipart::new("b");
        let mut out = Vec::new();
        let r = p.feed(b"--bXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX", &mut out);
        assert!(r.is_err());
    }
}
