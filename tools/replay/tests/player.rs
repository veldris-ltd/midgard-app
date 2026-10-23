use midgard_replay::player::{
    combine, log_line, Clock, Failures, Player, Sink, SystemClock, CHUNK_LINES,
};
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
    assert!(start.elapsed() < std::time::Duration::from_millis(3000));
}

/// Fails its first `fails` batches with one error, then succeeds.
struct Flaky {
    calls: RefCell<u64>,
    fails: u64,
}
impl Sink for Flaky {
    fn batch(&self, _: &str, _: &str) -> Result<String, String> {
        *self.calls.borrow_mut() += 1;
        if *self.calls.borrow() <= self.fails {
            Err("error: connection refused".into())
        } else {
            Ok("batch: 1 accepted, 0 refused, 0 unreadable".into())
        }
    }
    fn forge(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn failing_batches_are_still_all_attempted() {
    let rec = Recording {
        recorded_from: 1_000_000_000,
        duration: 100,
        events: (0..10).map(|i| batch(i * 10, "A")).collect(),
    };
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let sink = Flaky {
        calls: RefCell::new(0),
        fails: 7,
    };
    Player {
        rec: &rec,
        clock: &clock,
        sink: &sink,
    }
    .run(Some(1));
    assert_eq!(*sink.calls.borrow(), 10);
}

/// The messages `Failures` asks to log, with their fields, as "message k=v ...".
fn noted(f: &mut Failures, clock: &FakeClock, at: i64, r: Result<&str, &str>) -> Vec<String> {
    clock.sleep_until(at);
    f.note(clock.now(), &[("feed", "f".to_string())], r)
        .into_iter()
        .map(|(m, fields)| {
            let kv: Vec<String> = fields.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{m} {}", kv.join(" "))
        })
        .collect()
}

#[test]
fn a_repeated_failure_is_logged_once_then_counted_once_a_minute() {
    let clock = FakeClock(RefCell::new(0));
    let mut f = Failures::default();
    let err = Err("error: connection refused");
    assert_eq!(
        noted(&mut f, &clock, 0, err),
        ["batch sent feed=f result=FAILED error: connection refused"]
    );
    assert!(noted(&mut f, &clock, 10, err).is_empty());
    assert!(noted(&mut f, &clock, 30, err).is_empty());
    assert_eq!(
        noted(&mut f, &clock, 61, err),
        ["batches failing error=error: connection refused dropped=3"]
    );
    assert!(noted(&mut f, &clock, 70, err).is_empty());
    assert_eq!(
        noted(&mut f, &clock, 80, Ok("batch: 1 accepted")),
        [
            "batches recovered failed=5 dropped=1",
            "batch sent feed=f result=batch: 1 accepted"
        ]
    );
    assert_eq!(
        noted(&mut f, &clock, 90, Ok("batch: 2 accepted")),
        ["batch sent feed=f result=batch: 2 accepted"]
    );
}

#[test]
fn a_new_error_is_logged_at_once() {
    let clock = FakeClock(RefCell::new(0));
    let mut f = Failures::default();
    assert_eq!(noted(&mut f, &clock, 0, Err("a")).len(), 1);
    assert!(noted(&mut f, &clock, 1, Err("a")).is_empty());
    assert_eq!(
        noted(&mut f, &clock, 2, Err("b")),
        [
            "batches failing error=a dropped=1",
            "batch sent feed=f result=FAILED b"
        ]
    );
}

/// Records each batch call's line count; fails the calls numbered in `fail` (from 1).
struct Chunks {
    sizes: RefCell<Vec<usize>>,
    fail: Vec<usize>,
}
impl Sink for Chunks {
    fn batch(&self, _: &str, lines: &str) -> Result<String, String> {
        let n = lines.lines().count();
        self.sizes.borrow_mut().push(n);
        if self.fail.contains(&self.sizes.borrow().len()) {
            return Err("error: connection reset".into());
        }
        Ok(format!("batch: {n} accepted, 0 refused, 0 unreadable"))
    }
    fn forge(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

fn big_batch(n: usize) -> Recording {
    let lines: String = (0..n)
        .map(|i| obs(&format!("E{i}"), 1_000_000_000) + "\n")
        .collect();
    Recording {
        recorded_from: 1_000_000_000,
        duration: 10,
        events: vec![Event::Batch {
            offset: 0,
            feed: "f".into(),
            lines,
        }],
    }
}

#[test]
fn a_large_batch_is_sent_in_chunks() {
    assert_eq!(CHUNK_LINES, 2000);
    for (fail, calls) in [(vec![], 3), (vec![2], 3)] {
        let rec = big_batch(4500);
        let clock = FakeClock(RefCell::new(2_000_000_000));
        let sink = Chunks {
            sizes: RefCell::default(),
            fail,
        };
        Player {
            rec: &rec,
            clock: &clock,
            sink: &sink,
        }
        .run(Some(1));
        // Every chunk is tried, even after one fails.
        assert_eq!(*sink.sizes.borrow(), [2000, 2000, 500][..calls]);
    }
}

#[test]
fn chunk_results_combine_into_one() {
    let ok =
        |a: u64, r: u64, u: u64| Ok(format!("batch: {a} accepted, {r} refused, {u} unreadable"));
    assert_eq!(
        combine(vec![ok(1, 0, 0)]),
        Ok("batch: 1 accepted, 0 refused, 0 unreadable".to_string())
    );
    assert_eq!(
        combine(vec![ok(2000, 0, 0), ok(1999, 1, 0), ok(499, 0, 1)]),
        Ok("batch: 4498 accepted, 1 refused, 1 unreadable in 3 chunks".to_string())
    );
    assert_eq!(
        combine(vec![
            ok(2000, 0, 0),
            Err("error: reset".into()),
            ok(500, 0, 0)
        ]),
        Err("error: reset".to_string())
    );
    // A summary that is not one keeps every chunk's text.
    assert_eq!(
        combine(vec![ok(1, 0, 0), Ok("done".into())]),
        Ok("batch: 1 accepted, 0 refused, 0 unreadable; done".to_string())
    );
    assert_eq!(
        combine(vec![ok(1, 0, 0), Ok("batch: 2 accepted".into())]),
        Ok("batch: 1 accepted, 0 refused, 0 unreadable; batch: 2 accepted".to_string())
    );
}

#[test]
fn the_first_loop_starts_with_each_late_feeds_first_batch_dated_now() {
    // Feed "late" first reports 200 s in: without priming its layer would be empty for
    // the first minutes of a fresh start. Feed "f" (10 s in) is soon enough to wait for.
    let late = |offset: u64, entity: &str| Event::Batch {
        offset,
        feed: "late".into(),
        lines: obs(entity, 1_000_000_000 + offset as i64) + "\n",
    };
    let rec = Recording {
        recorded_from: 1_000_000_000,
        duration: 400,
        events: vec![batch(10, "A"), late(200, "L1"), late(300, "L2")],
    };
    let clock = FakeClock(RefCell::new(2_000_000_000));
    let log = Log::new();
    Player {
        rec: &rec,
        clock: &clock,
        sink: &log,
    }
    .run(Some(2));
    let got = log.got.borrow().clone();
    let second = 2_000_000_400;
    assert_eq!(
        got,
        vec![
            format!("late {}\n", obs("L1", 2_000_000_000)), // primed, dated the start
            format!("f {}\n", obs("A", 2_000_000_010)),
            format!("late {}\n", obs("L1", 2_000_000_200)),
            format!("late {}\n", obs("L2", 2_000_000_300)),
            // Later loops are not primed: the picture already has every feed.
            format!("f {}\n", obs("A", second + 10)),
            format!("late {}\n", obs("L1", second + 200)),
            format!("late {}\n", obs("L2", second + 300)),
        ]
    );
}
