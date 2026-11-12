//! Normalize Kingbase's Oracle JSON extensions before serde deserialization.
//!
//! This is deliberately not JSON5: comments, hexadecimal numbers and bare
//! string values are not accepted by Kingbase. serde_json still validates
//! escapes, Unicode, numeric range and nesting after normalization.

pub(super) fn normalize(raw: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(raw).ok()?;
    let mut parser = Parser {
        raw,
        text,
        pos: 0,
        out: Vec::with_capacity(raw.len()),
    };
    parser.value(0)?;
    parser.whitespace();
    (parser.pos == raw.len()).then_some(parser.out)
}

struct Parser<'a> {
    raw: &'a [u8],
    text: &'a str,
    pos: usize,
    out: Vec<u8>,
}

impl<'a> Parser<'a> {
    fn whitespace(&mut self) {
        while matches!(self.raw.get(self.pos), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        self.whitespace();
        if self.raw.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<()> {
        // Bound our own recursion independently of serde_json's recursion limit.
        if depth >= 128 {
            return None;
        }
        self.whitespace();
        match *self.raw.get(self.pos)? {
            b'{' => self.container(true, depth + 1),
            b'[' => self.container(false, depth + 1),
            b'"' | b'\'' => self.string(),
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number(),
            _ => {
                let token = self.word()?;
                let keyword: &[u8] = if token.eq_ignore_ascii_case(b"true") {
                    b"true"
                } else if token.eq_ignore_ascii_case(b"false") {
                    b"false"
                } else if token.eq_ignore_ascii_case(b"null") {
                    b"null"
                } else {
                    return None;
                };
                self.out.extend_from_slice(keyword);
                Some(())
            }
        }
    }

    fn container(&mut self, object: bool, depth: usize) -> Option<()> {
        let (open, close) = if object { (b'{', b'}') } else { (b'[', b']') };
        self.pos += 1;
        self.out.push(open);
        if !self.take(close) {
            loop {
                if object {
                    self.whitespace();
                    if matches!(self.raw.get(self.pos), Some(b'"' | b'\'')) {
                        self.string()?;
                    } else {
                        let key = self.word()?;
                        // These tokens are literals, not unquoted field names.
                        if [b"true".as_slice(), b"false", b"null"]
                            .iter()
                            .any(|keyword| key.eq_ignore_ascii_case(keyword))
                        {
                            return None;
                        }
                        self.out.push(b'"');
                        self.out.extend_from_slice(key);
                        self.out.push(b'"');
                    }
                    if !self.take(b':') {
                        return None;
                    }
                    self.out.push(b':');
                }
                self.value(depth)?;
                if self.take(close) {
                    break;
                }
                if !self.take(b',') {
                    return None;
                }
                // A single trailing comma is allowed for arrays and objects.
                if self.take(close) {
                    break;
                }
                self.out.push(b',');
            }
        }
        self.out.push(close);
        Some(())
    }

    fn word(&mut self) -> Option<&'a [u8]> {
        let start = self.pos;
        let text = self.text.get(start..)?;
        for (offset, ch) in text.char_indices() {
            if !(ch == '_' || ch.is_alphabetic() || (offset != 0 && ch.is_numeric())) {
                break;
            }
            self.pos = start + offset + ch.len_utf8();
        }
        (self.pos > start).then_some(&self.raw[start..self.pos])
    }

    fn string(&mut self) -> Option<()> {
        let quote = *self.raw.get(self.pos)?;
        self.pos += 1;
        self.out.push(b'"');
        loop {
            let byte = *self.raw.get(self.pos)?;
            self.pos += 1;
            if byte == quote {
                self.out.push(b'"');
                return Some(());
            }
            match byte {
                b'\\' => {
                    let escaped = *self.raw.get(self.pos)?;
                    self.pos += 1;
                    self.out.extend_from_slice(&[b'\\', escaped]);
                }
                b'"' => self.out.extend_from_slice(b"\\\""),
                0..=31 => return None,
                _ => self.out.push(byte),
            }
        }
    }

    fn digits(&mut self) -> &'a [u8] {
        let start = self.pos;
        while matches!(self.raw.get(self.pos), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        &self.raw[start..self.pos]
    }

    fn number(&mut self) -> Option<()> {
        if self.take(b'-') {
            self.out.push(b'-');
        } else {
            self.take(b'+');
        }
        // Do not permit whitespace between a sign and its number.
        let integer = self.digits();
        let has_integer = !integer.is_empty();
        let significant = integer.iter().position(|byte| *byte != b'0');
        let integer = significant.map_or(b"0".as_slice(), |start| &integer[start..]);
        self.out.extend_from_slice(integer);
        if self.raw.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            self.out.push(b'.');
            let fraction = self.digits();
            if fraction.is_empty() {
                if !has_integer {
                    return None;
                }
                self.out.push(b'0');
            } else {
                self.out.extend_from_slice(fraction);
            }
        } else if !has_integer {
            return None;
        }
        if matches!(self.raw.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            self.out.push(b'e');
            if let Some(sign @ (b'+' | b'-')) = self.raw.get(self.pos) {
                self.out.push(*sign);
                self.pos += 1;
            }
            let exponent = self.digits();
            if exponent.is_empty() {
                return None;
            }
            self.out.extend_from_slice(exponent);
        }
        Some(())
    }
}
