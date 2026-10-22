use midgard_replay::shift::{is_time_key, shift_observation};
use service_runtime::json::parse;

#[test]
fn shifts_observed_at_and_time_attributes_only() {
    let o = parse(
        r#"{"entity":"E1","kind":"event-fire","observedAt":1792000000,
        "attrs":{"acq_time":1791990000,"issued_at":1791000000,"frp":3.5,"name":"x",
                 "valid_to":1792003600,"tiny_at":12,"lat":51.5}}"#,
    )
    .unwrap();
    let s = shift_observation(&o, 3600).encode();
    let v = parse(&s).unwrap();
    assert_eq!(v.get("observedAt").unwrap().as_i64(), Some(1792003600));
    let a = v.get("attrs").unwrap();
    assert_eq!(a.get("acq_time").unwrap().as_i64(), Some(1791993600));
    assert_eq!(a.get("issued_at").unwrap().as_i64(), Some(1791003600));
    assert_eq!(a.get("valid_to").unwrap().as_i64(), Some(1792007200));
    assert_eq!(a.get("tiny_at").unwrap().as_i64(), Some(12)); // not a Unix time
    assert_eq!(a.get("name").unwrap().as_str(), Some("x"));
    assert!(s.contains("\"frp\":3.5") && s.contains("\"lat\":51.5"));
}

#[test]
fn time_keys() {
    for k in [
        "pos_at",
        "event_time",
        "time_raised",
        "started",
        "etr",
        "valid_from",
        "published",
    ] {
        assert!(is_time_key(k), "{k}");
    }
    for k in ["lat", "frp", "timeline", "name"] {
        assert!(!is_time_key(k), "{k}");
    }
}
