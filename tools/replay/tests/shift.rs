use midgard_replay::shift::{is_time_key, shift_csv, shift_observation};
use service_runtime::json::{parse, Json};

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
        "period_start",
        "period_end",
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

#[test]
fn shifts_time_columns_and_keeps_every_other_byte() {
    let csv = "key,message,time_raised,severity,observed_at\n\
               A,\"Rivers Bure, Ant and \"\"Thurne\"\"\",1792153093,3,\n\
               B,plain,1792000000,2,1792000100\n";
    let out = shift_csv(csv, 60);
    assert_eq!(
        out,
        "key,message,time_raised,severity,observed_at\n\
                     A,\"Rivers Bure, Ant and \"\"Thurne\"\"\",1792153153,3,\n\
                     B,plain,1792000060,2,1792000160\n"
    );
}

#[test]
fn csv_without_time_columns_is_unchanged() {
    let csv = "key,name,intensity\nX,North,120\n";
    assert_eq!(shift_csv(csv, 999), csv);
}

#[test]
fn crlf_csv_keeps_every_line_terminator() {
    let csv = "key,time_raised\r\nA,1792000000\r\nB,\"x\r\ny\"\r\n";
    assert_eq!(
        shift_csv(csv, 60),
        "key,time_raised\r\nA,1792000060\r\nB,\"x\r\ny\"\r\n"
    );
}

#[test]
fn last_record_without_newline_stays_without_one() {
    let csv = "key,observed_at\nA,1792000000\nB,1792000001";
    assert_eq!(
        shift_csv(csv, 1),
        "key,observed_at\nA,1792000001\nB,1792000002"
    );
}

#[test]
fn only_all_digit_fields_are_shifted() {
    let csv = "key,time_raised\nA,+1792000000\nB, 1792000000\n";
    assert_eq!(shift_csv(csv, 60), csv);
}

#[test]
fn period_start_column_is_shifted() {
    let csv = "key,name,period_start,wind_pct\r\nCI-1,North,1792686600,99.8\r\n";
    assert_eq!(
        shift_csv(csv, 60),
        "key,name,period_start,wind_pct\r\nCI-1,North,1792686660,99.8\r\n"
    );
}

#[test]
fn fractional_time_attribute_moves_by_the_shift() {
    let o = parse(r#"{"observedAt":1792000000,"attrs":{"etr":1792000000.5}}"#).unwrap();
    let v = shift_observation(&o, 60);
    match v.get("attrs").unwrap().get("etr") {
        Some(Json::Num(f)) => assert_eq!(*f, 1792000060.5),
        other => panic!("etr not a number: {other:?}"),
    }
}

#[test]
fn observed_at_out_of_range_is_unchanged() {
    let o = parse(r#"{"observedAt":12,"attrs":{}}"#).unwrap();
    assert_eq!(
        shift_observation(&o, 60)
            .get("observedAt")
            .unwrap()
            .as_i64(),
        Some(12)
    );
}

#[test]
fn overflowing_shift_leaves_values_unchanged() {
    let o = parse(r#"{"observedAt":1792000000,"attrs":{}}"#).unwrap();
    let v = shift_observation(&o, i64::MAX);
    assert_eq!(v.get("observedAt").unwrap().as_i64(), Some(1792000000));
    let csv = "key,observed_at\nA,1792000000\n";
    assert_eq!(shift_csv(csv, i64::MAX), csv);
}

#[test]
fn empty_csv_is_unchanged() {
    assert_eq!(shift_csv("", 60), "");
}

#[test]
fn header_cells_are_never_shifted() {
    let csv = "key,1792000000_at\nA,1792000000\n";
    assert_eq!(shift_csv(csv, 60), "key,1792000000_at\nA,1792000060\n");
    let csv = "key,observed_at\n";
    assert_eq!(shift_csv(csv, 60), csv);
}

#[test]
fn quoted_header_column_is_shifted() {
    let csv = "\"key\",\"observed_at\"\nA,1792000000\n";
    assert_eq!(
        shift_csv(csv, 60),
        "\"key\",\"observed_at\"\nA,1792000060\n"
    );
}

#[test]
fn bom_prefixed_time_header_is_shifted() {
    let csv = "\u{feff}time_raised,key\n1792000000,A\n";
    assert_eq!(
        shift_csv(csv, 60),
        "\u{feff}time_raised,key\n1792000060,A\n"
    );
    let csv = "\u{feff}\"observed_at\" ,key\n1792000000,A\n";
    assert_eq!(
        shift_csv(csv, 60),
        "\u{feff}\"observed_at\" ,key\n1792000060,A\n"
    );
}
