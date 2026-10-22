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

/// Whether an attribute (or CSV column) holds a time to move.
pub fn is_time_key(key: &str) -> bool {
    key.ends_with("_at")
        || key.ends_with("_time")
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
