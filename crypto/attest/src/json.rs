//! A strict, read-only JSON parser.
//!
//! "Is this a CycloneDX document" cannot be answered by looking for substrings: a
//! truncated file, or one with the right words in the wrong places, contains them
//! too. So the SBOM gate parses, and it parses strictly — RFC 8259 and nothing
//! more lenient. `[AC-RF-SC-1b]` `[AC-B-4]`
//!
//! Strictness is the point, because the bytes are not yet trusted when this runs:
//!
//! - trailing data after the document is a rejection, as it is for the `.sig`
//!   envelope — bytes this parser ignores are bytes some other tool may read;
//! - a **duplicate key** is a rejection. Parsers disagree on which value wins, so
//!   a document with two `components` arrays would mean one thing to this gate
//!   and another to the scanner;
//! - nesting is bounded, so hostile input cannot exhaust the stack;
//! - there is no encoder here and no number conversion — nothing downstream needs
//!   either, and every feature is attack surface.

/// Deepest nesting accepted. mg-sbom's output is four levels deep.
const MAX_DEPTH: usize = 64;

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// A number, kept as its validated source text.
    Num(String),
    /// A string, unescaped.
    Str(String),
    /// An array.
    Arr(Vec<Json>),
    /// An object, in document order. Keys are unique.
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// An object's field, or `None` if this is not an object or has no such key.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The boolean, if this is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }
}

/// Parse one complete JSON document. Anything malformed is an error naming the
/// byte offset; there is no partial result.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        at: 0,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.at != p.b.len() {
        return Err(p.err("trailing data after the document"));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("{what} at byte {}", self.at)
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        match self.peek() {
            None => Err(self.err("unexpected end of document")),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
        }
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json, String> {
        if self.b[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(v)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.at += 1;
        let mut pairs: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.eat(b'}') {
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a string key"));
            }
            let key = self.string()?;
            if pairs.iter().any(|(k, _)| *k == key) {
                return Err(self.err(&format!("duplicate key {key:?}")));
            }
            self.ws();
            if !self.eat(b':') {
                return Err(self.err("expected ':'"));
            }
            self.ws();
            let v = self.value(depth + 1)?;
            pairs.push((key, v));
            self.ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                return Ok(Json::Obj(pairs));
            }
            return Err(self.err("expected ',' or '}'"));
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.at += 1;
        let mut items = Vec::new();
        self.ws();
        if self.eat(b']') {
            return Ok(Json::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth + 1)?);
            self.ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b']') {
                return Ok(Json::Arr(items));
            }
            return Err(self.err("expected ',' or ']'"));
        }
    }

    /// Parse a string; the cursor is on its opening quote.
    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while let Some(c) = self.peek() {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.at += 1;
            }
            // The input is a `&str` and the run only ever ends on an ASCII byte,
            // so it cannot split a multi-byte character.
            out.push_str(
                std::str::from_utf8(&self.b[start..self.at])
                    .map_err(|_| self.err("invalid UTF-8 in string"))?,
            );
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    self.escape(&mut out)?;
                }
                Some(_) => return Err(self.err("raw control character in string")),
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), String> {
        let c = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
        self.at += 1;
        match c {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let hi = self.hex4()?;
                let cp = if (0xD800..0xDC00).contains(&hi) {
                    if !(self.eat(b'\\') && self.eat(b'u')) {
                        return Err(self.err("unpaired surrogate"));
                    }
                    let lo = self.hex4()?;
                    if !(0xDC00..0xE000).contains(&lo) {
                        return Err(self.err("unpaired surrogate"));
                    }
                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                } else {
                    hi
                };
                // A lone low surrogate is not a scalar value and fails here.
                out.push(char::from_u32(cp).ok_or_else(|| self.err("invalid \\u escape"))?);
            }
            _ => return Err(self.err("invalid escape")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let s = self
            .b
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.err("truncated \\u escape"))?;
        let mut v = 0u32;
        for &c in s {
            let d = (c as char)
                .to_digit(16)
                .ok_or_else(|| self.err("invalid \\u escape"))?;
            v = v * 16 + d;
        }
        self.at += 4;
        Ok(v)
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        self.eat(b'-');
        match self.peek() {
            // A leading zero stands alone: `01` is not a number.
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.err("invalid number")),
        }
        if self.eat(b'.') && self.digits() == 0 {
            return Err(self.err("invalid number"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if !self.eat(b'+') {
                self.eat(b'-');
            }
            if self.digits() == 0 {
                return Err(self.err("invalid number"));
            }
        }
        Ok(Json::Num(
            String::from_utf8_lossy(&self.b[start..self.at]).into_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_value_kind() {
        let v = parse(r#" {"a": [1, -2.5e3, true, false, null], "b": {"c": "d"}} "#).unwrap();
        let a = v.get("a").and_then(Json::as_array).unwrap();
        assert_eq!(a.len(), 5);
        assert_eq!(a[0], Json::Num("1".into()));
        assert_eq!(a[1], Json::Num("-2.5e3".into()));
        assert_eq!(a[2].as_bool(), Some(true));
        assert_eq!(a[4], Json::Null);
        assert_eq!(
            v.get("b").and_then(|b| b.get("c")).and_then(Json::as_str),
            Some("d")
        );
    }

    #[test]
    fn unescapes_strings() {
        let v = parse(r#""a\"b\\c\/\né😀é""#).unwrap();
        assert_eq!(v.as_str(), Some("a\"b\\c/\n\u{e9}\u{1F600}é"));
    }

    #[test]
    fn rejects_truncation_at_every_length() {
        let full = r#"{"bomFormat": "CycloneDX", "components": [{"purl": "p"}], "n": 1}"#;
        for n in 0..full.len() {
            assert!(
                parse(&full[..n]).is_err(),
                "a {n}-byte prefix must not parse"
            );
        }
        assert!(parse(full).is_ok());
    }

    #[test]
    fn rejects_trailing_data() {
        assert!(parse("{} {}").is_err());
        assert!(parse("[1] x").is_err());
        assert!(parse("{}\n").is_ok(), "trailing whitespace is not data");
    }

    #[test]
    fn rejects_duplicate_keys() {
        let e = parse(r#"{"components": [], "components": [1]}"#).unwrap_err();
        assert!(e.contains("duplicate key"), "{e}");
        // The same key in sibling objects is not a duplicate.
        assert!(parse(r#"[{"a": 1}, {"a": 2}]"#).is_ok());
    }

    #[test]
    fn rejects_what_lenient_parsers_accept() {
        for bad in [
            "{'a': 1}",     // single quotes
            "{a: 1}",       // bare key
            "[1, 2,]",      // trailing comma
            r#"{"a": 1,}"#, // trailing comma
            "[01]",         // leading zero
            "[1.]",         // empty fraction
            "[+1]",         // explicit plus
            "[NaN]",        // not JSON
            "// c\n{}",     // comment
            "\"a\tb\"",     // raw control character
            r#""\x41""#,    // invalid escape
            r#""\ud83d""#,  // unpaired high surrogate
            r#""\ude00""#,  // lone low surrogate
            "",             // nothing at all
        ] {
            assert!(parse(bad).is_err(), "{bad:?} must not parse");
        }
    }

    #[test]
    fn bounds_nesting_depth() {
        let deep = "[".repeat(MAX_DEPTH + 2) + &"]".repeat(MAX_DEPTH + 2);
        assert!(parse(&deep).unwrap_err().contains("nesting too deep"));
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&ok).is_ok());
    }
}
