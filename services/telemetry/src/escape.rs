//! Escaping for the two text formats telemetry emits: JSON log lines and Prometheus
//! label values. Both are complete over control characters, so no field a caller
//! supplies (a path, a username, a remote error) can break a line or forge another.
//! [RC-33] [AC-27.1] [AC-27.2] [VLD-ANB-MIDGARD-0033] [VLD-ANB-MIDGARD-0037]

/// `s` as the body of a JSON string (without the surrounding quotes). Escapes `"` and
/// `\`; `\n`, `\r` and `\t` by name; every other character below U+0020, and U+007F,
/// as `\u00XX`; and U+2028 / U+2029, which some JavaScript consumers treat as line
/// breaks, as `\u2028` / `\u2029`. RFC 8259 section 7.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

/// `s` as a Prometheus text-format label value (without the surrounding quotes). The
/// format's own escapes are kept (`\\`, `\"`, `\n`); every other control character is
/// replaced by U+FFFD, so an exposition line can never be broken or extended.
pub fn label_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escape_covers_every_c0_control_del_and_the_line_separators() {
        for code in (0u32..0x20).chain([0x7f]) {
            let c = char::from_u32(code).unwrap();
            let escaped = json_escape(&c.to_string());
            assert!(escaped.starts_with('\\'), "{code:#x} -> {escaped}");
            assert!(!escaped.chars().any(char::is_control), "{code:#x}");
        }
        assert_eq!(json_escape("\u{1b}"), "\\u001b");
        assert_eq!(json_escape("\u{0}"), "\\u0000");
        assert_eq!(json_escape("a\u{2028}b\u{2029}"), "a\\u2028b\\u2029");
        assert_eq!(json_escape("\"\\\r\n\t"), "\\\"\\\\\\r\\n\\t");
        assert_eq!(json_escape("plain é"), "plain é");
    }

    #[test]
    fn label_value_keeps_prometheus_escapes_and_replaces_other_controls() {
        assert_eq!(label_value("a\\b\"c\nd"), "a\\\\b\\\"c\\nd");
        assert_eq!(
            label_value("a\rb\u{1b}c\u{0}"),
            "a\u{fffd}b\u{fffd}c\u{fffd}"
        );
        assert_eq!(label_value("x\u{2028}"), "x\u{fffd}");
    }
}
