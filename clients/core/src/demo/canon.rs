//! Canonical JSON: object keys sorted by byte order, no whitespace, numbers as their
//! validated source text, strings escaped minimally (RFC 8259). Two parses of equal
//! text encode to equal bytes. `[NFR-PQ-3]`
//!
//! "Canonical" here is textual, not numeric: a number keeps its source text, so `1E2`,
//! `1e2` and `100` give three different bodies (and three different signatures) although
//! they denote the same value. Nothing here normalises numbers; a producer that wants
//! one body per dataset must emit the same number text every time. The demo dataset
//! builder copies number text unchanged from the committed fixture files, which
//! `extract-fixtures.mjs` writes with `JSON.stringify`, and refuses any number not in
//! that one spelling (`super::build::number_text`).

use attest::json::Json;

/// Encode `v` canonically.
pub fn encode(v: &Json) -> String {
    let mut out = String::new();
    write(v, &mut out);
    out
}

fn write(v: &Json, out: &mut String) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(t) => out.push_str(t),
        Json::Str(s) => string(s, out),
        Json::Arr(items) => {
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Json::Obj(pairs) => {
            let mut sorted: Vec<&(String, Json)> = pairs.iter().collect();
            sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            out.push('{');
            for (i, (k, x)) in sorted.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                string(k, out);
                out.push(':');
                write(x, out);
            }
            out.push('}');
        }
    }
}

fn string(s: &str, out: &mut String) {
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

#[cfg(test)]
mod tests {
    use super::encode;
    use attest::json::parse;

    fn canon(text: &str) -> String {
        encode(&parse(text).expect("strict JSON"))
    }

    #[test]
    fn keys_are_sorted_by_byte_order_at_every_depth_and_whitespace_goes() {
        assert_eq!(
            canon(r#" { "b" : [ 1 , { "z" : null , "a" : true } ] , "a" : false , "B" : "x" } "#),
            r#"{"B":"x","a":false,"b":[1,{"a":true,"z":null}]}"#
        );
    }

    #[test]
    fn numbers_keep_their_source_text() {
        assert_eq!(canon("[-2.5e3,0,1E2,10.50]"), "[-2.5e3,0,1E2,10.50]");
    }

    #[test]
    fn strings_are_escaped_minimally() {
        assert_eq!(
            canon(r#"["A\/\"\\\n\r\t\u0001\u001fé "]"#),
            "[\"A/\\\"\\\\\\n\\r\\t\\u0001\\u001f\u{e9}\u{2028}\"]"
        );
    }

    #[test]
    fn non_ascii_keys_sort_by_utf8_bytes() {
        // U+FF21 (EF BC A1) sorts after U+00E9 (C3 A9) by bytes.
        assert_eq!(canon(r#"{"Ａ":1,"é":2}"#), r#"{"é":2,"Ａ":1}"#);
    }

    #[test]
    fn canonical_text_is_a_fixed_point() {
        let once = canon(r#"{"x":[{"q":"\u0000","p":[]}],"e":{}}"#);
        assert_eq!(canon(&once), once);
        assert_eq!(once, r#"{"e":{},"x":[{"p":[],"q":"\u0000"}]}"#);
    }
}
