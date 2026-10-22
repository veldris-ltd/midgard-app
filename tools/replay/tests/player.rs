use midgard_replay::player::{log_line, Clock, Player, Sink, SystemClock};
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

/// Records what it is sent; each call can advance the clock (a slow sink, or the Mac
/// sleeping mid-loop): by `first` s on the first call, `each` s on every later one.
struct Log<'a> {
    got: RefCell<Vec<String>>,
    clock: Option<&'a FakeClock>,
    first: i64,
    each: i64,
}
impl Log<'_> {
    fn new() -> Log<'static> {
        Log {
            got: RefCell::default(),
            clock: None,
            first: 0,
            each: 0,
        }
    }
    fn advance(&self) {
        if let Some(c) = self.clock {
            let by = if self.got.borrow().len() == 1 {
                self.first
            } else {
                self.each
            };
            *c.0.borrow_mut() += by;
        }
    }
}
impl Sink for Log<'_> {
    fn batch(&self, feed: &str, lines: &str) -> Result<String, String> {
        self.got.borrow_mut().push(format!("{feed} {lines}"));
        self.advance();
        Ok("ok".into())
    }
    fn forge(&self, file: &str, csv: &str) -> Result<(), String> {
        self.got.borrow_mut().push(format!("forge {file} {csv}"));
        self.advance();
        Ok(())
    }
}

fn obs(entity: &str, at: i64) -> String {
    format!(r#"{{"entity":"{entity}","kind":"k","observedAt":{at},"attrs":{{}}}}"#)
}

fn batch(offset: u64, entity: &str) -> Event {
    Event::Batch {
        offset,
        feed: "f".into(),
        lines: obs(entity, 1_000_000_000 + offset as i64) + "\n",
    }
}

#[test]
fn plays_two_loops_with_times_shifted_to_each_loop() {
    let dir = std::env::temp_dir().join(format!("replay-player-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.csv"), "key,observed_at\nA,1000000100\n").unwrap();
    let rec = Recording {
        recorded_from: 1_000_000_000,
        duration: 100,
        events: vec![
            batch(10, "E"),
            Event::Forge {
                offset: 50,
                file: "a.csv".into(),
                path: dir.join("a.csv"),
            },
        ],
    };
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let log = Log::new();
    Player {
        rec: &rec,
        clock: &clock,
        sink: &log,
    }
    .run(Some(2));
    let _ = std::fs::remove_dir_all(&dir);
    let got = log.got.borrow().clone();
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
fn far_behind_skips_the_rest_of_the_loop_and_restarts_from_now() {
    let rec = Recording {
        recorded_from: 1_000_000_000,
        duration: 100,
        events: vec![batch(10, "A"), batch(20, "B"), batch(30, "C")],
    };
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let log = Log {
        clock: Some(&clock),
        first: 400,
        ..Log::new()
    };
    Player {
        rec: &rec,
        clock: &clock,
        sink: &log,
    }
    .run(Some(2));
    // Loop 0 played A at 2_000_000_010, then the clock jumped to 2_000_000_410: B and C
    // were 390 s late, so loop 1 started at 2_000_000_410 with its shift from there.
    let restart = 2_000_000_410;
    let got = log.got.borrow().clone();
    assert_eq!(
        got,
        vec![
            format!("f {}\n", obs("A", 2_000_000_010)),
            format!("f {}\n", obs("A", restart + 10)),
            format!("f {}\n", obs("B", restart + 20)),
            format!("f {}\n", obs("C", restart + 30)),
        ]
    );
    assert_eq!(clock.now(), restart + 30);
}

#[test]
fn a_little_late_still_plays_every_event_in_order() {
    let rec = Recording {
        recorded_from: 1_000_000_000,
        duration: 100,
        events: vec![batch(10, "A"), batch(11, "B"), batch(12, "C")],
    };
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let log = Log {
        clock: Some(&clock),
        first: 5,
        each: 5,
        ..Log::new()
    };
    Player {
        rec: &rec,
        clock: &clock,
        sink: &log,
    }
    .run(Some(1));
    let got = log.got.borrow().clone();
    assert_eq!(
        got,
        vec![
            format!("f {}\n", obs("A", 2_000_000_010)),
            format!("f {}\n", obs("B", 2_000_000_011)),
            format!("f {}\n", obs("C", 2_000_000_012)),
        ]
    );
}

#[test]
fn an_empty_recording_waits_for_each_loop_rather_than_spinning() {
    let rec = Recording {
        recorded_from: 1_000_000_000,
        duration: 100,
        events: vec![],
    };
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let log = Log::new();
    Player {
        rec: &rec,
        clock: &clock,
        sink: &log,
    }
    .run(Some(3));
    assert!(log.got.borrow().is_empty());
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

#[test]
fn system_clock_sleeps_to_the_instant_and_not_for_the_past() {
    let c = SystemClock;
    let start = std::time::Instant::now();
    c.sleep_until(c.now() - 10);
    assert!(start.elapsed() < std::time::Duration::from_millis(100));
    let t = c.now() + 1;
    c.sleep_until(t);
    assert!(c.now() >= t);
    assert!(start.elapsed() < std::time::Duration::from_millis(2100));
}
