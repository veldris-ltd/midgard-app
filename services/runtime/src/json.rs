//! Minimal, dependency-free JSON value + encoder + parser.
//!
//! The repo deliberately excludes serde (no async/serde/HTTP framework). Only a
//! read-only JSON parser exists elsewhere (for LLM output); this module adds a
//! small self-contained value type with both an encoder (for responses) and a
//! permissive recursive-descent parser (for request bodies). The deployment files the
//! gateway and the mesh services load are parsed with it. [DD-6] [NFR-SEC-2]

use std::collections::BTreeMap;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// Integers are kept distinct so ids/counts encode without a trailing `.0`.
    Int(i64),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// Insertion-ordered object (a Vec of pairs) so encoded output is stable.
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Build an object from key/value pairs.
    pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
        Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    pub fn s(v: &str) -> Json {
        Json::Str(v.to_string())
    }

    pub fn arr_str(items: &[&str]) -> Json {
        Json::Arr(items.iter().map(|x| Json::Str(x.to_string())).collect())
    }

    // --- accessors (for parsed request bodies) ---

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(n) => Some(*n),
            Json::Num(n) => Some(*n as i64),
            _ => None,
        }
    }

    /// Object field as a `&str` (convenience).
    pub fn str_field(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.as_str())
    }

    /// Collect an object's string→string entries (for a params map).
    pub fn str_map(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        if let Json::Obj(pairs) = self {
            for (k, v) in pairs {
                if let Json::Str(s) = v {
                    out.insert(k.clone(), s.clone());
                }
            }
        }
        out
    }

    // --- encoding ---

    /// Serialise to a compact JSON string.
    pub fn encode(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => out.push_str(&n.to_string()),
            Json::Num(n) => {
                if n.is_finite() {
                    out.push_str(&n.to_string());
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => write_escaped(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    it.write(out);
                }
                out.push(']');
            }
            Json::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_escaped(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

// --- parsing (request bodies) ---

/// How deeply arrays and objects may nest. The parser recurses once per level, so an
/// unbounded depth would let a body of `[[[[…` exhaust the handler thread's stack and
/// abort the whole service; no request or deployment file needs more than a few levels.
pub const MAX_DEPTH: usize = 64;

/// Parse a JSON document. Permissive enough for the small request bodies the
/// console UIs send (objects of strings/numbers/bools/arrays/nested objects).
/// Nesting deeper than [`MAX_DEPTH`] is refused, and `\u` escapes must be exactly four
/// hex digits, with UTF-16 surrogates only in valid pairs.
pub fn parse(input: &str) -> Result<Json, String> {
    let mut p = Parser {
        b: input.as_bytes(),
        i: 0,
        depth: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err("trailing characters after JSON value".into());
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        match self.peek() {
            Some(b'{') | Some(b'[') => {
                if self.depth >= MAX_DEPTH {
                    return Err(format!("JSON nests deeper than {MAX_DEPTH} levels"));
                }
                self.depth += 1;
                let v = if self.peek() == Some(b'{') {
                    self.object()
                } else {
                    self.array()
                };
                self.depth -= 1;
                v
            }
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') | Some(b'f') => self.boolean(),
            Some(b'n') => self.null(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            other => Err(format!("unexpected token: {:?}", other.map(|c| c as char))),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.i += 1; // {
        let mut pairs = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err("expected object key".into());
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err("expected ':' after key".into());
            }
            self.i += 1;
            let val = self.value()?;
            pairs.push((key, val));
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b'}') => {
                    self.i += 1;
                    break;
                }
                _ => return Err("expected ',' or '}'".into()),
            }
        }
        Ok(Json::Obj(pairs))
    }

    fn array(&mut self) -> Result<Json, String> {
        self.i += 1; // [
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            let val = self.value()?;
            items.push(val);
            self.ws();
            match self.peek() {
                Some(b',') => {
                    self.i += 1;
                }
                Some(b']') => {
                    self.i += 1;
                    break;
                }
                _ => return Err("expected ',' or ']'".into()),
            }
        }
        Ok(Json::Arr(items))
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1; // opening quote
        let mut s = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                b'"' => return Ok(s),
                b'\\' => {
                    let e = self.peek().ok_or("unterminated escape")?;
                    self.i += 1;
                    match e {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        b'/' => s.push('/'),
                        b'n' => s.push('\n'),
                        b'r' => s.push('\r'),
                        b't' => s.push('\t'),
                        b'b' => s.push('\u{08}'),
                        b'f' => s.push('\u{0c}'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = match first {
                                0xD800..=0xDBFF => {
                                    // A high surrogate must be followed by `\u` + a low one.
                                    if self.b.get(self.i..self.i + 2) != Some(b"\\u") {
                                        return Err("lone surrogate in \\u escape".into());
                                    }
                                    self.i += 2;
                                    let low = self.hex4()?;
                                    if !(0xDC00..=0xDFFF).contains(&low) {
                                        return Err("lone surrogate in \\u escape".into());
                                    }
                                    0x10000 + ((first - 0xD800) << 10) + (low - 0xDC00)
                                }
                                0xDC00..=0xDFFF => {
                                    return Err("lone surrogate in \\u escape".into())
                                }
                                c => c,
                            };
                            s.push(char::from_u32(code).ok_or("bad \\u escape")?);
                        }
                        _ => return Err("bad escape".into()),
                    }
                }
                _ => {
                    // Push the raw byte; multi-byte UTF-8 continues on next bytes.
                    // Collect the full UTF-8 sequence starting at this byte.
                    let start = self.i - 1;
                    let len = utf8_len(c);
                    let end = (start + len).min(self.b.len());
                    if len > 1 {
                        self.i = end;
                    }
                    match std::str::from_utf8(&self.b[start..end]) {
                        Ok(chunk) => s.push_str(chunk),
                        Err(_) => s.push('\u{fffd}'),
                    }
                }
            }
        }
        Err("unterminated string".into())
    }

    /// Exactly four hex digits.
    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .b
            .get(self.i..self.i + 4)
            .filter(|d| d.iter().all(u8::is_ascii_hexdigit))
            .ok_or("\\u must be followed by four hex digits")?;
        self.i += 4;
        let text = std::str::from_utf8(digits).map_err(|_| "bad \\u escape")?;
        u32::from_str_radix(text, 16).map_err(|_| "bad \\u escape".to_string())
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        let mut is_float = false;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            match c {
                b'0'..=b'9' => self.i += 1,
                b'.' | b'e' | b'E' | b'+' | b'-' => {
                    is_float = true;
                    self.i += 1;
                }
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.b[start..self.i]).map_err(|_| "bad number")?;
        if is_float {
            text.parse::<f64>()
                .map(Json::Num)
                .map_err(|_| "bad number".into())
        } else {
            text.parse::<i64>()
                .map(Json::Int)
                .map_err(|_| "bad number".into())
        }
    }

    fn boolean(&mut self) -> Result<Json, String> {
        if self.b[self.i..].starts_with(b"true") {
            self.i += 4;
            Ok(Json::Bool(true))
        } else if self.b[self.i..].starts_with(b"false") {
            self.i += 5;
            Ok(Json::Bool(false))
        } else {
            Err("bad literal".into())
        }
    }

    fn null(&mut self) -> Result<Json, String> {
        if self.b[self.i..].starts_with(b"null") {
            self.i += 4;
            Ok(Json::Null)
        } else {
            Err("bad literal".into())
        }
    }
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else if first >> 3 == 0b11110 {
        4
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_object() {
        let j = Json::obj(vec![
            ("a", Json::Int(1)),
            ("b", Json::s("x\"y")),
            ("c", Json::Arr(vec![Json::Bool(true), Json::Null])),
        ]);
        let enc = j.encode();
        assert_eq!(enc, r#"{"a":1,"b":"x\"y","c":[true,null]}"#);
        let back = parse(&enc).unwrap();
        assert_eq!(back, j);
    }

    #[test]
    fn parses_request_body() {
        let v =
            parse(r#"{"objectType":"aircraft","maxResults":5,"humanGateToken":"HG-1"}"#).unwrap();
        assert_eq!(v.str_field("objectType"), Some("aircraft"));
        assert_eq!(v.get("maxResults").and_then(|x| x.as_i64()), Some(5));
        assert_eq!(v.str_field("humanGateToken"), Some("HG-1"));
    }

    #[test]
    fn parses_utf8_and_escapes() {
        let v = parse(r#"{"s":"ML-DSA ✓ · ok"}"#).unwrap();
        assert_eq!(v.str_field("s"), Some("ML-DSA ✓ · ok"));
    }

    #[test]
    fn deep_nesting_is_refused_not_a_stack_overflow() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(&ok).is_ok());
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
        assert!(parse(&deep).unwrap_err().contains("deeper than"));
        // Far past any stack: still an error, not an abort.
        let hostile = "[{\"a\":".repeat(200_000);
        assert!(parse(&hostile).unwrap_err().contains("deeper than"));
    }

    #[test]
    fn unicode_escapes_are_exact_and_surrogates_paired() {
        let v = parse(r#""é😀A""#).unwrap();
        assert_eq!(v.as_str(), Some("é😀A"));
        for bad in [
            r#""\ud83d""#,
            r#""\ud83dx""#,
            r#""\ud83dA""#,
            r#""\ude00""#,
            r#""\u12""#,
            r#""\u+123""#,
            r#""\u12g4""#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
