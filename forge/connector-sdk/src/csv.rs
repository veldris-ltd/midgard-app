//! Delimited records as RFC 4180 defines them: a field may be quoted with `"`, a quote
//! inside a quoted field is written `""`, and a quoted field may hold the delimiter and
//! line breaks. Records end at LF or CRLF. Anything else refuses the body rather than
//! guessing where a field ends. [FR-F-2]

use std::mem::take;

/// Split `body` into records of fields.
pub fn records(body: &str, delimiter: char) -> Result<Vec<Vec<String>>, String> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut line = 1usize;
    let mut started = false; // the current field has content or an opening quote
    let mut quoted = false; // the current field was quoted and its quote has closed
    let mut in_quotes = false;
    let mut chars = body.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    in_quotes = false;
                    quoted = true;
                }
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if !started => {
                in_quotes = true;
                started = true;
            }
            '"' => return Err(format!("line {line}: a quote inside an unquoted field")),
            c if c == delimiter => {
                record.push(take(&mut field));
                started = false;
                quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                record.push(take(&mut field));
                records.push(take(&mut record));
                started = false;
                quoted = false;
                line += 1;
            }
            _ if quoted => return Err(format!("line {line}: characters after a closing quote")),
            _ => {
                field.push(c);
                started = true;
            }
        }
    }
    if in_quotes {
        return Err(format!("line {line}: a quoted field is never closed"));
    }
    if started || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::records;

    fn fields(body: &str) -> Vec<Vec<String>> {
        records(body, ',').unwrap()
    }

    #[test]
    fn quoted_fields_hold_delimiters_quotes_and_line_breaks() {
        assert_eq!(
            fields("a,\"b,c\",\"say \"\"hi\"\"\"\r\n\"multi\nline\",,x\n"),
            [vec!["a", "b,c", "say \"hi\""], vec!["multi\nline", "", "x"],]
        );
        assert_eq!(fields("a,b"), [vec!["a", "b"]]);
        assert_eq!(fields("\n"), [vec![""]]);
        assert!(fields("").is_empty());
        assert_eq!(records("a;\"b;c\"", ';').unwrap(), [vec!["a", "b;c"]]);
    }

    #[test]
    fn malformed_quoting_is_refused() {
        for bad in ["a,b\"c", "\"abc", "\"ab\"c,d", "a,\"b\nc"] {
            assert!(records(bad, ',').is_err(), "{bad:?}");
        }
    }
}
