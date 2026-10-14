//! PDF objects, a lexer and parser for them, and a serializer.
//!
//! The parser works on a byte slice and is used three ways: on the head of an indirect object
//! (a dictionary or array, then maybe `stream`), on the decoded bytes of an object stream, and
//! on page content streams (operands and operators). It never allocates more than the objects
//! it returns, and nesting is bounded, so hostile files cannot blow the stack.

use std::collections::BTreeMap;

/// Deepest nesting of arrays and dictionaries the parser follows.
const MAX_DEPTH: usize = 64;

/// A PDF object. Streams are an indirect object with a dictionary and a byte range, see
/// [`crate::doc::Loaded`], so they are not a variant here.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Obj {
    /// `null`, and the value of anything missing.
    #[default]
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A real number.
    Real(f64),
    /// A name, without the leading slash, with `#xx` escapes decoded.
    Name(Vec<u8>),
    /// A string, escapes decoded.
    Str(Vec<u8>),
    /// An array.
    Array(Vec<Obj>),
    /// A dictionary.
    Dict(Dict),
    /// An indirect reference: object number and generation.
    Ref(u32, u16),
}

/// A dictionary. Keys are names without the slash. Sorted, so output is deterministic.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Dict(BTreeMap<Vec<u8>, Obj>);

impl Dict {
    /// An empty dictionary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a key.
    pub fn get(&self, key: &str) -> Option<&Obj> {
        self.0.get(key.as_bytes())
    }

    /// Insert or replace a key.
    pub fn set(&mut self, key: &str, value: Obj) {
        self.0.insert(key.as_bytes().to_vec(), value);
    }

    /// Remove a key, returning the old value.
    pub fn remove(&mut self, key: &str) -> Option<Obj> {
        self.0.remove(key.as_bytes())
    }

    /// True when the key exists.
    pub fn has(&self, key: &str) -> bool {
        self.0.contains_key(key.as_bytes())
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate over entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &Obj)> {
        self.0.iter().map(|(k, v)| (k.as_slice(), v))
    }

    /// Mutable iteration over the values.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut Obj> {
        self.0.values_mut()
    }

    /// Insert with a raw byte key.
    pub fn set_raw(&mut self, key: Vec<u8>, value: Obj) {
        self.0.insert(key, value);
    }

    /// The name stored under `key`, as text.
    pub fn name(&self, key: &str) -> Option<&[u8]> {
        self.get(key).and_then(Obj::as_name)
    }

    /// The integer stored under `key`.
    pub fn int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(Obj::as_i64)
    }
}

impl Obj {
    /// A name object from text.
    pub fn name(s: &str) -> Obj {
        Obj::Name(s.as_bytes().to_vec())
    }

    /// A reference to generation 0 of an object.
    pub fn r(num: u32) -> Obj {
        Obj::Ref(num, 0)
    }

    /// The name bytes, if this is a name.
    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }

    /// True when this is the name `s`.
    pub fn is_name(&self, s: &str) -> bool {
        self.as_name() == Some(s.as_bytes())
    }

    /// An integer, also accepting a real with no fraction.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Obj::Int(i) => Some(*i),
            Obj::Real(r) if r.fract() == 0.0 && r.abs() < 9.0e15 => Some(*r as i64),
            _ => None,
        }
    }

    /// Any number as a float.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Obj::Int(i) => Some(*i as f64),
            Obj::Real(r) => Some(*r),
            _ => None,
        }
    }

    /// The dictionary, if this is one.
    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            Obj::Dict(d) => Some(d),
            _ => None,
        }
    }

    /// The array, if this is one.
    pub fn as_array(&self) -> Option<&[Obj]> {
        match self {
            Obj::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The object number, if this is a reference.
    pub fn as_ref_num(&self) -> Option<u32> {
        match self {
            Obj::Ref(n, _) => Some(*n),
            _ => None,
        }
    }

    /// The string bytes, if this is a string.
    pub fn as_str_bytes(&self) -> Option<&[u8]> {
        match self {
            Obj::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Why parsing stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErr {
    /// The input ended in the middle of an object. More bytes might complete it.
    Eof,
    /// The bytes are not valid PDF syntax.
    Syntax,
}

/// One lexical item of a content stream or object body.
#[derive(Debug, PartialEq)]
pub enum Tok<'a> {
    /// An operand or object.
    Obj(Obj),
    /// A bare word: a content operator, or `obj`, `endobj`, `stream`, `R` and so on.
    Word(&'a [u8]),
    /// The end of input.
    End,
}

/// A cursor over bytes.
pub struct Lexer<'a> {
    /// The bytes being read.
    pub buf: &'a [u8],
    /// Current position.
    pub pos: usize,
    /// Recognise `n g R` as a reference. Off for content streams.
    pub refs: bool,
}

/// PDF white space: NUL, tab, line feed, form feed, carriage return, space.
pub fn is_ws(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn is_regular(b: u8) -> bool {
    !is_ws(b) && !is_delim(b)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

impl<'a> Lexer<'a> {
    /// A lexer at the start of `buf` that recognises references.
    pub fn new(buf: &'a [u8]) -> Self {
        Lexer {
            buf,
            pos: 0,
            refs: true,
        }
    }

    /// A lexer at `pos`.
    pub fn at(buf: &'a [u8], pos: usize) -> Self {
        Lexer {
            buf,
            pos,
            refs: true,
        }
    }

    /// A lexer for content streams.
    pub fn content(buf: &'a [u8]) -> Self {
        Lexer {
            buf,
            pos: 0,
            refs: false,
        }
    }

    /// Skip white space and comments.
    pub fn skip_ws(&mut self) {
        while self.pos < self.buf.len() {
            let b = self.buf[self.pos];
            if is_ws(b) {
                self.pos += 1;
            } else if b == b'%' {
                while self.pos < self.buf.len() && !matches!(self.buf[self.pos], 10 | 13) {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// The next token.
    pub fn next_tok(&mut self) -> Result<Tok<'a>, ParseErr> {
        self.tok(0)
    }

    /// Parse one object. A bare word that is not a keyword constant is a syntax error.
    pub fn object(&mut self) -> Result<Obj, ParseErr> {
        match self.tok(0)? {
            Tok::Obj(o) => Ok(o),
            Tok::Word(_) => Err(ParseErr::Syntax),
            Tok::End => Err(ParseErr::Eof),
        }
    }

    /// Read a bare word equal to `word`, or fail without moving.
    pub fn expect_word(&mut self, word: &[u8]) -> bool {
        let save = self.pos;
        self.skip_ws();
        match self.tok(0) {
            Ok(Tok::Word(w)) if w == word => true,
            _ => {
                self.pos = save;
                false
            }
        }
    }

    /// Read a non-negative integer token.
    pub fn uint(&mut self) -> Option<u64> {
        let save = self.pos;
        self.skip_ws();
        let start = self.pos;
        while self.pos < self.buf.len() && self.buf[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos == start || (self.pos < self.buf.len() && is_regular(self.buf[self.pos])) {
            self.pos = save;
            return None;
        }
        std::str::from_utf8(&self.buf[start..self.pos])
            .ok()?
            .parse()
            .ok()
    }

    fn tok(&mut self, depth: usize) -> Result<Tok<'a>, ParseErr> {
        self.skip_ws();
        let Some(&b) = self.buf.get(self.pos) else {
            return Ok(Tok::End);
        };
        match b {
            b'/' => {
                self.pos += 1;
                Ok(Tok::Obj(Obj::Name(self.name_bytes())))
            }
            b'(' => self.literal_string().map(|s| Tok::Obj(Obj::Str(s))),
            b'<' => {
                if self.buf.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    self.dict_body(depth).map(|d| Tok::Obj(Obj::Dict(d)))
                } else {
                    self.hex_string().map(|s| Tok::Obj(Obj::Str(s)))
                }
            }
            b'[' => {
                if depth >= MAX_DEPTH {
                    return Err(ParseErr::Syntax);
                }
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    match self.buf.get(self.pos) {
                        None => return Err(ParseErr::Eof),
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(Tok::Obj(Obj::Array(items)));
                        }
                        _ => match self.tok(depth + 1)? {
                            Tok::Obj(o) => items.push(o),
                            Tok::Word(w) if self.refs => {
                                // `endobj` or garbage inside an array: the array is broken.
                                let _ = w;
                                return Err(ParseErr::Syntax);
                            }
                            Tok::Word(_) => {} // operators inside arrays are ignored
                            Tok::End => return Err(ParseErr::Eof),
                        },
                    }
                }
            }
            b')' | b'>' | b']' | b'{' | b'}' => {
                // Stray delimiters: skip one byte so callers always make progress.
                self.pos += 1;
                Err(ParseErr::Syntax)
            }
            b'0'..=b'9' | b'+' | b'-' | b'.' => self.number(),
            _ => {
                let start = self.pos;
                while self.pos < self.buf.len() && is_regular(self.buf[self.pos]) {
                    self.pos += 1;
                }
                let w = &self.buf[start..self.pos];
                Ok(match w {
                    b"true" => Tok::Obj(Obj::Bool(true)),
                    b"false" => Tok::Obj(Obj::Bool(false)),
                    b"null" => Tok::Obj(Obj::Null),
                    _ => Tok::Word(w),
                })
            }
        }
    }

    fn number(&mut self) -> Result<Tok<'a>, ParseErr> {
        let start = self.pos;
        let mut i = self.pos;
        let b = self.buf;
        let mut neg = false;
        // Tolerate runs of signs such as `--5`.
        while i < b.len() && matches!(b[i], b'+' | b'-') {
            if b[i] == b'-' {
                neg = true;
            }
            i += 1;
        }
        let digits_start = i;
        let mut int_part: i128 = 0;
        let mut overflow = false;
        while i < b.len() && b[i].is_ascii_digit() {
            int_part = int_part * 10 + i128::from(b[i] - b'0');
            if int_part > i128::from(i64::MAX) {
                overflow = true;
                int_part = i128::from(i64::MAX);
            }
            i += 1;
        }
        let mut is_real = false;
        let mut frac = 0.0f64;
        if i < b.len() && b[i] == b'.' {
            is_real = true;
            i += 1;
            let mut scale = 0.1f64;
            while i < b.len() && b[i].is_ascii_digit() {
                frac += f64::from(b[i] - b'0') * scale;
                scale *= 0.1;
                i += 1;
            }
        }
        // A number runs into the next token only at a delimiter or white space. Words such as
        // `1.2abc` or `12R` in content are not numbers: consume the whole word.
        if i < b.len() && is_regular(b[i]) {
            while i < b.len() && is_regular(b[i]) {
                i += 1;
            }
            self.pos = i;
            return Ok(Tok::Word(&b[start..i]));
        }
        self.pos = i;
        if i == digits_start {
            // `+`, `-` alone: treat as zero.
            return Ok(Tok::Obj(Obj::Int(0)));
        }
        if is_real || overflow {
            let v = int_part as f64 + frac;
            let v = if neg { -v } else { v };
            return Ok(Tok::Obj(Obj::Real(v)));
        }
        let v = int_part as i64;
        let v = if neg { -v } else { v };
        if self.refs && !neg && v >= 0 {
            // Look ahead for `gen R`.
            let save = self.pos;
            if let Some(g) = self.uint()
                && g <= 65535
                && self.expect_word(b"R")
            {
                return Ok(Tok::Obj(Obj::Ref(v as u32, g as u16)));
            }
            self.pos = save;
        }
        Ok(Tok::Obj(Obj::Int(v)))
    }

    fn name_bytes(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while self.pos < self.buf.len() && is_regular(self.buf[self.pos]) {
            let c = self.buf[self.pos];
            if c == b'#'
                && let (Some(h), Some(l)) = (
                    self.buf.get(self.pos + 1).copied().and_then(hex_val),
                    self.buf.get(self.pos + 2).copied().and_then(hex_val),
                )
            {
                out.push(h << 4 | l);
                self.pos += 3;
                continue;
            }
            out.push(c);
            self.pos += 1;
        }
        out
    }

    fn literal_string(&mut self) -> Result<Vec<u8>, ParseErr> {
        // At '('.
        self.pos += 1;
        let mut out = Vec::new();
        let mut depth = 1usize;
        while self.pos < self.buf.len() {
            let c = self.buf[self.pos];
            self.pos += 1;
            match c {
                b'(' => {
                    depth += 1;
                    out.push(c);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(out);
                    }
                    out.push(c);
                }
                b'\\' => {
                    let Some(&e) = self.buf.get(self.pos) else {
                        return Err(ParseErr::Eof);
                    };
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'(' | b')' | b'\\' => out.push(e),
                        b'\r' => {
                            if self.buf.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut v = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.buf.get(self.pos) {
                                    Some(&d @ b'0'..=b'7') => {
                                        v = v * 8 + u32::from(d - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((v & 0xff) as u8);
                        }
                        other => out.push(other),
                    }
                }
                b'\r' => {
                    // A bare CR or CRLF inside a string is one LF.
                    if self.buf.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                _ => out.push(c),
            }
        }
        Err(ParseErr::Eof)
    }

    fn hex_string(&mut self) -> Result<Vec<u8>, ParseErr> {
        // At '<'.
        self.pos += 1;
        let mut out = Vec::new();
        let mut hi: Option<u8> = None;
        while self.pos < self.buf.len() {
            let c = self.buf[self.pos];
            self.pos += 1;
            if c == b'>' {
                if let Some(h) = hi {
                    out.push(h << 4);
                }
                return Ok(out);
            }
            if let Some(v) = hex_val(c) {
                match hi.take() {
                    Some(h) => out.push(h << 4 | v),
                    None => hi = Some(v),
                }
            }
        }
        Err(ParseErr::Eof)
    }

    fn dict_body(&mut self, depth: usize) -> Result<Dict, ParseErr> {
        if depth >= MAX_DEPTH {
            return Err(ParseErr::Syntax);
        }
        let mut d = Dict::new();
        loop {
            self.skip_ws();
            match self.buf.get(self.pos) {
                None => return Err(ParseErr::Eof),
                Some(b'>') => {
                    if self.buf.get(self.pos + 1) == Some(&b'>') {
                        self.pos += 2;
                        return Ok(d);
                    }
                    if self.pos + 1 >= self.buf.len() {
                        return Err(ParseErr::Eof);
                    }
                    self.pos += 1;
                }
                Some(b'/') => {
                    self.pos += 1;
                    let key = self.name_bytes();
                    self.skip_ws();
                    // A key with no value before `>>` or the next key is null.
                    match self.buf.get(self.pos) {
                        None => return Err(ParseErr::Eof),
                        Some(b'>') => d.set_raw(key, Obj::Null),
                        _ => match self.tok(depth + 1)? {
                            Tok::Obj(v) => d.set_raw(key, v),
                            Tok::Word(w) => {
                                // `endobj` or `stream` where a value should be: broken.
                                let _ = w;
                                return Err(ParseErr::Syntax);
                            }
                            Tok::End => return Err(ParseErr::Eof),
                        },
                    }
                }
                Some(_) => {
                    // Junk where a key should be: skip a token and carry on.
                    match self.tok(depth + 1) {
                        Ok(Tok::End) => return Err(ParseErr::Eof),
                        Ok(_) | Err(ParseErr::Syntax) => {}
                        Err(ParseErr::Eof) => return Err(ParseErr::Eof),
                    }
                }
            }
        }
    }
}

/// Parse a standalone object from bytes (used for object stream members and tests).
pub fn parse_object(buf: &[u8]) -> Result<Obj, ParseErr> {
    Lexer::new(buf).object()
}

// ---------------------------------------------------------------------------------------------
// Serializing

fn push_real(out: &mut Vec<u8>, r: f64) {
    if !r.is_finite() {
        out.push(b'0');
        return;
    }
    if r.abs() >= 1.0e15 {
        out.extend_from_slice(format!("{}", r as i64).as_bytes());
        return;
    }
    let s = format!("{r:.5}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" || s == "-0" {
        out.push(b'0');
    } else {
        out.extend_from_slice(s.as_bytes());
    }
}

/// Write a name with `#xx` escapes where PDF needs them.
pub fn push_name(out: &mut Vec<u8>, name: &[u8]) {
    out.push(b'/');
    for &b in name {
        if b <= 0x20 || b >= 0x7f || is_delim(b) || b == b'#' {
            out.extend_from_slice(format!("#{b:02X}").as_bytes());
        } else {
            out.push(b);
        }
    }
}

fn push_string(out: &mut Vec<u8>, s: &[u8]) {
    // Literal when mostly printable, hex otherwise.
    let printable = s.iter().filter(|&&b| (0x20..0x7f).contains(&b)).count();
    if s.is_empty() || printable * 4 >= s.len() * 3 {
        out.push(b'(');
        for &b in s {
            match b {
                b'(' | b')' | b'\\' => {
                    out.push(b'\\');
                    out.push(b);
                }
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                0x20..=0x7e => out.push(b),
                _ => out.extend_from_slice(format!("\\{b:03o}").as_bytes()),
            }
        }
        out.push(b')');
    } else {
        out.push(b'<');
        for &b in s {
            out.extend_from_slice(format!("{b:02x}").as_bytes());
        }
        out.push(b'>');
    }
}

/// Serialize an object.
pub fn write_obj(out: &mut Vec<u8>, o: &Obj) {
    match o {
        Obj::Null => out.extend_from_slice(b"null"),
        Obj::Bool(true) => out.extend_from_slice(b"true"),
        Obj::Bool(false) => out.extend_from_slice(b"false"),
        Obj::Int(i) => out.extend_from_slice(i.to_string().as_bytes()),
        Obj::Real(r) => push_real(out, *r),
        Obj::Name(n) => push_name(out, n),
        Obj::Str(s) => push_string(out, s),
        Obj::Array(a) => {
            out.push(b'[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                write_obj(out, x);
            }
            out.push(b']');
        }
        Obj::Dict(d) => write_dict(out, d),
        Obj::Ref(n, g) => out.extend_from_slice(format!("{n} {g} R").as_bytes()),
    }
}

/// Serialize a dictionary.
pub fn write_dict(out: &mut Vec<u8>, d: &Dict) {
    out.extend_from_slice(b"<<");
    for (k, v) in d.iter() {
        push_name(out, k);
        // A space is needed unless the value starts with a delimiter.
        if !matches!(v, Obj::Name(_) | Obj::Array(_) | Obj::Dict(_) | Obj::Str(_)) {
            out.push(b' ');
        }
        write_obj(out, v);
    }
    out.extend_from_slice(b">>");
}

/// Serialize to a new vector.
pub fn to_bytes(o: &Obj) -> Vec<u8> {
    let mut v = Vec::new();
    write_obj(&mut v, o);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Obj {
        parse_object(s.as_bytes()).unwrap()
    }

    #[test]
    fn scalars() {
        assert_eq!(p("42"), Obj::Int(42));
        assert_eq!(p("-7"), Obj::Int(-7));
        assert_eq!(p("3.5"), Obj::Real(3.5));
        assert_eq!(p(".5"), Obj::Real(0.5));
        assert_eq!(p("true"), Obj::Bool(true));
        assert_eq!(p("null"), Obj::Null);
        assert_eq!(p("/Na#20me"), Obj::Name(b"Na me".to_vec()));
    }

    #[test]
    fn strings_decode_escapes() {
        assert_eq!(p("(a\\(b\\)c)"), Obj::Str(b"a(b)c".to_vec()));
        assert_eq!(p("(x(y)z)"), Obj::Str(b"x(y)z".to_vec()));
        assert_eq!(p("(\\101\\n)"), Obj::Str(b"A\n".to_vec()));
        assert_eq!(p("<48 65 6c6C6f>"), Obj::Str(b"Hello".to_vec()));
        assert_eq!(p("<4>"), Obj::Str(vec![0x40]));
    }

    #[test]
    fn references_and_containers() {
        let o = p("<< /Type /Page /Kids [1 0 R 2 0 R] /Count 2 /MediaBox [0 0 612 792.5] >>");
        let d = o.as_dict().unwrap();
        assert!(d.get("Type").unwrap().is_name("Page"));
        assert_eq!(
            d.get("Kids").unwrap().as_array().unwrap()[1],
            Obj::Ref(2, 0)
        );
        assert_eq!(d.int("Count"), Some(2));
        // Two plain integers in an array are not a reference.
        assert_eq!(
            p("[1 0 3]"),
            Obj::Array(vec![Obj::Int(1), Obj::Int(0), Obj::Int(3)])
        );
    }

    #[test]
    fn truncated_input_reports_eof_and_junk_reports_syntax() {
        assert_eq!(parse_object(b"<< /A [1 2"), Err(ParseErr::Eof));
        assert_eq!(parse_object(b"(abc"), Err(ParseErr::Eof));
        assert!(parse_object(b"<< /A endobj").is_err());
    }

    #[test]
    fn deep_nesting_is_refused_not_overflowed() {
        let deep = "[".repeat(5000);
        assert!(parse_object(deep.as_bytes()).is_err());
    }

    #[test]
    fn round_trip() {
        let o = p("<< /A [1 2.25 (hi\\)) /N#41] /B << /C 3 0 R >> /D <00ff> >>");
        let bytes = to_bytes(&o);
        assert_eq!(parse_object(&bytes).unwrap(), o);
    }

    #[test]
    fn reals_never_use_exponents() {
        assert_eq!(to_bytes(&Obj::Real(0.000001)), b"0");
        assert_eq!(to_bytes(&Obj::Real(612.0)), b"612");
        assert_eq!(to_bytes(&Obj::Real(-1.5)), b"-1.5");
    }

    #[test]
    fn content_tokens() {
        let mut lx = Lexer::content(b"q 1 0 0 1 10 20 cm /Im0 Do Q");
        let mut words = Vec::new();
        let mut nums = 0;
        loop {
            match lx.next_tok().unwrap() {
                Tok::End => break,
                Tok::Word(w) => words.push(String::from_utf8_lossy(w).into_owned()),
                Tok::Obj(Obj::Int(_)) => nums += 1,
                Tok::Obj(_) => {}
            }
        }
        assert_eq!(words, ["q", "cm", "Do", "Q"]);
        assert_eq!(nums, 6);
    }

    #[test]
    fn comments_and_missing_values() {
        let o = p("<< /A 1 % note\n /B /C >>");
        assert_eq!(o.as_dict().unwrap().int("A"), Some(1));
        assert!(o.as_dict().unwrap().get("B").unwrap().is_name("C"));
    }
}
