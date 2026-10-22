use midgard_replay::player::{log_line, Clock, Player, Sink};
use midgard_replay::recording::{Event, Recording};
use service_runtime::json::{parse, Json};
use std::cell::RefCell;

struct FakeClock(RefCell<i64>);
impl Clock for FakeClock {
    fn now(&self) -> i64 {
        *self.0.borrow()
    }
    fn sleep_until(&self, t: i64) {
        let mut n = self.0.borrow_mut();
        if t > *n {
            *n = t;
        }
    }
}
#[derive(Default)]
struct Log(RefCell<Vec<String>>);
impl Sink for Log {
    fn batch(&self, feed: &str, lines: &str) -> Result<String, String> {
        self.0.borrow_mut().push(format!("{feed} {lines}"));
        Ok("ok".into())
    }
    fn forge(&self, file: &str, csv: &str) -> Result<(), String> {
        self.0.borrow_mut().push(format!("forge {file} {csv}"));
        Ok(())
    }
}

#[test]
fn plays_two_loops_with_times_shifted_to_each_loop() {
    let dir = std::env::temp_dir().join(format!("replay-player-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.csv"), "key,observed_at\nA,1000000100\n").unwrap();
    let rec = Recording {
        recorded_from: 1_000_000_000,
        duration: 100,
        events: vec![
            Event::Batch {
                offset: 10,
                feed: "f".into(),
                observations: vec![parse(
                    r#"{"entity":"E","kind":"k","observedAt":1000000010,"attrs":{}}"#,
                )
                .unwrap()],
            },
            Event::Forge {
                offset: 50,
                file: "a.csv".into(),
                path: dir.join("a.csv"),
            },
        ],
    };
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let log = Log::default();
    Player {
        rec: &rec,
        clock: &clock,
        sink: &log,
    }
    .run(Some(2));
    let got = log.0.borrow().clone();
    assert_eq!(
        got,
        vec![
            r#"f {"entity":"E","kind":"k","observedAt":2000000010,"attrs":{}}"#.to_string() + "\n",
            "forge a.csv key,observed_at\nA,2000000100\n".to_string(),
            r#"f {"entity":"E","kind":"k","observedAt":2000000110,"attrs":{}}"#.to_string() + "\n",
            "forge a.csv key,observed_at\nA,2000000200\n".to_string(),
        ]
    );
    assert_eq!(clock.now(), 2_000_000_150); // waited for the second loop's last event
}

#[test]
fn an_empty_recording_waits_for_each_loop_rather_than_spinning() {
    let rec = Recording {
        recorded_from: 1_000_000_000,
        duration: 100,
        events: vec![],
    };
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let log = Log::default();
    Player {
        rec: &rec,
        clock: &clock,
        sink: &log,
    }
    .run(Some(3));
    assert!(log.0.borrow().is_empty());
    assert_eq!(clock.now(), 2_000_000_200); // the third loop's start
}

#[test]
fn a_log_line_is_one_valid_json_object() {
    let line = log_line(
        "say \"hi\"\n",
        &[("result", "FAILED \"x\"\\y\n".to_string())],
    );
    assert!(!line.contains('\n'));
    let j = parse(&line).unwrap();
    assert_eq!(j.str_field("message"), Some("say \"hi\"\n"));
    assert_eq!(j.str_field("result"), Some("FAILED \"x\"\\y\n"));
    assert!(matches!(j, Json::Obj(_)));
}
