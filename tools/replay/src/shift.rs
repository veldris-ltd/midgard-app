//! Moving a recording's times to now. [design §2]
use service_runtime::json::Json;

/// Unix seconds between these bounds are times; anything else is left alone.
const MIN_UNIX: i64 = 1_000_000_000;
const MAX_UNIX: i64 = 4_000_000_000;
const NAMED: &[&str] = &[
    "started",
    "ended",
    "etr",
    "valid_from",
    "valid_to",
    "published",
];

/// Whether an attribute or CSV column holds a time to move: one rule for both.
pub fn is_time_key(key: &str) -> bool {
    key.ends_with("_at")
        || key.ends_with("_time")
        || key.ends_with("_start")
        || key.ends_with("_end")
        || key.starts_with("time_")
        || NAMED.contains(&key)
}

fn shift_value(v: &Json, shift: i64) -> Json {
    match v {
        Json::Int(n) if (MIN_UNIX..=MAX_UNIX).contains(n) => Json::Int(n + shift),
        Json::Num(f) if (MIN_UNIX as f64..=MAX_UNIX as f64).contains(f) => {
            Json::Num(f + shift as f64)
        }
        other => other.clone(),
    }
}

/// A copy of the observation with `observedAt` and its time attributes moved by `shift` s.
pub fn shift_observation(o: &Json, shift: i64) -> Json {
    let Json::Obj(pairs) = o else {
        return o.clone();
    };
    Json::Obj(
        pairs
            .iter()
            .map(|(k, v)| {
                let v = match (k.as_str(), v) {
                    ("observedAt", v) => shift_value(v, shift),
                    ("attrs", Json::Obj(attrs)) => Json::Obj(
                        attrs
                            .iter()
                            .map(|(ak, av)| {
                                (
                                    ak.clone(),
                                    if is_time_key(ak) {
                                        shift_value(av, shift)
                                    } else {
                                        av.clone()
                                    },
                                )
                            })
                            .collect(),
                    ),
                    (_, v) => v.clone(),
                };
                (k.clone(), v)
            })
            .collect(),
    )
}

/// One CSV record: its raw fields (quotes kept as written) and the line terminator that
/// ended it (`"\r\n"`, `"\n"`, or `""` for a final record without one).
struct Record<'a> {
    fields: Vec<&'a str>,
    end: &'a str,
}

/// Split CSV text into records, on commas and newlines outside quotes. Joining each
/// record's fields with ',' and appending its terminator gives back the input exactly.
fn records(text: &str) -> Vec<Record<'_>> {
    let (mut out, mut fields, mut start, mut quoted) = (Vec::new(), Vec::new(), 0usize, false);
    let b = text.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'"' => quoted = !quoted,
            b',' if !quoted => {
                fields.push(&text[start..i]);
                start = i + 1;
            }
            b'\n' if !quoted => {
                let cut = if i > start && b[i - 1] == b'\r' {
                    i - 1
                } else {
                    i
                };
                fields.push(&text[start..cut]);
                out.push(Record {
                    fields: std::mem::take(&mut fields),
                    end: &text[cut..=i],
                });
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < text.len() || !fields.is_empty() {
        fields.push(&text[start..]);
        out.push(Record { fields, end: "" });
    }
    out
}

/// The Unix time a field holds, if it is made only of ASCII digits and in range.
fn unix_time(field: &str) -> Option<i64> {
    if field.is_empty() || !field.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    field
        .parse::<i64>()
        .ok()
        .filter(|n| (MIN_UNIX..=MAX_UNIX).contains(n))
}

/// The CSV with whole-number Unix times in its time columns moved by `shift` seconds;
/// every other byte, line terminators included, is kept as written.
pub fn shift_csv(text: &str, shift: i64) -> String {
    let recs = records(text);
    let Some(header) = recs.first() else {
        return text.to_string();
    };
    let cols: Vec<usize> = header
        .fields
        .iter()
        .enumerate()
        .filter(|(_, h)| is_time_key(h.trim_matches('"')))
        .map(|(i, _)| i)
        .collect();
    if cols.is_empty() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 64);
    for (r, rec) in recs.iter().enumerate() {
        for (i, f) in rec.fields.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            match unix_time(f) {
                Some(n) if r > 0 && cols.contains(&i) => out.push_str(&(n + shift).to_string()),
                _ => out.push_str(f),
            }
        }
        out.push_str(rec.end);
    }
    out
}
