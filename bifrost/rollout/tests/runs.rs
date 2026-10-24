//! Live rollouts as a pure state machine, driven by a fake clock and fake reports: a
//! ring passes after its soak and the next starts; a ring not ready by its timeout, or
//! unhealthy after reporting ready, rolls every changed service back; a manual rollback
//! does the same; one run per channel; and a run resumes mid-ring from its saved state.
//! [FR-B-2] [NFR-AVA-1]

use std::collections::BTreeMap;

use bifrost_rollout::runs::{
    Event, InstallReport, Release, RingConfig, RingPhase, RingSpec, RolloutError, Rollouts,
    RunKind, RunState, ServiceStatus, DEFAULT_READY_TIMEOUT_SECS, DEFAULT_SOAK_SECS,
};

const SERVICES: [&str; 6] = [
    "forge-worker",
    "midgard-replay",
    "heimdall-feed",
    "heimdall-source",
    "mimir-server",
    "http-gateway",
];

fn release(version: &str) -> Release {
    Release {
        version: version.into(),
        artefacts: SERVICES
            .iter()
            .map(|s| (s.to_string(), version.to_string()))
            .collect(),
        created_by: "release-manager".into(),
        created_at: 0,
    }
}

/// A channel `stable` on 0.1.0 with releases 0.1.0 and 0.2.0.
fn setup() -> (Rollouts, RingConfig) {
    let config = RingConfig::suite_default();
    let mut r = Rollouts::new();
    r.seed_channel("stable", "0.1.0").unwrap();
    r.create_release(release("0.1.0"), &config).unwrap();
    r.create_release(release("0.2.0"), &config).unwrap();
    (r, config)
}

/// One installation's report: every service at `versions[service]` (default `base`),
/// healthy unless listed in `unhealthy`.
fn report(at: u64, base: &str, overrides: &[(&str, &str)], unhealthy: &[&str]) -> InstallReport {
    let mut services = BTreeMap::new();
    for s in SERVICES {
        let version = overrides
            .iter()
            .find(|(name, _)| *name == s)
            .map_or(base, |(_, v)| *v);
        services.insert(
            s.to_string(),
            ServiceStatus {
                version: version.into(),
                healthy: !unhealthy.contains(&s),
            },
        );
    }
    InstallReport {
        install_id: "mac-suite".into(),
        channel: "stable".into(),
        at,
        services,
    }
}

/// What a well-behaved agent reports: every service at its desired version, healthy.
fn obedient(r: &Rollouts, config: &RingConfig, at: u64) -> InstallReport {
    let desired = r.desired("stable", config).unwrap();
    InstallReport {
        install_id: "mac-suite".into(),
        channel: "stable".into(),
        at,
        services: desired
            .services
            .into_iter()
            .map(|(s, version)| {
                (
                    s,
                    ServiceStatus {
                        version,
                        healthy: true,
                    },
                )
            })
            .collect(),
    }
}

fn desired_of(r: &Rollouts, config: &RingConfig, service: &str) -> String {
    r.desired("stable", config).unwrap().services[service].clone()
}

#[test]
fn defaults_are_the_suite_rings() {
    let c = RingConfig::suite_default();
    c.validate().unwrap();
    let names: Vec<&str> = c.rings.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["canary", "ring-2", "ring-3", "last"]);
    assert_eq!(c.rings[0].services, ["forge-worker"]);
    assert_eq!(
        c.rings[1].services,
        ["midgard-replay", "heimdall-feed", "heimdall-source"]
    );
    assert_eq!(
        c.rings[3].services,
        ["http-gateway"],
        "the gateway goes last"
    );
    assert!(c
        .rings
        .iter()
        .all(|r| r.soak_secs == 60 && r.ready_timeout_secs == 180));

    let mut twice = c.clone();
    twice.rings[2].services.push("forge-worker".into());
    assert!(twice.validate().is_err());
    let mut empty = c.clone();
    empty.rings.push(RingSpec {
        services: vec![],
        ..RingSpec::new("x", &[])
    });
    assert!(empty.validate().is_err());
}

#[test]
fn a_release_must_cover_exactly_the_ring_services_and_is_immutable() {
    let (mut r, config) = setup();
    let mut short = release("0.3.0");
    short.artefacts.remove("mimir-server");
    assert!(matches!(
        r.create_release(short, &config),
        Err(RolloutError::Invalid(m)) if m.contains("no artefact for mimir-server")
    ));
    let mut extra = release("0.3.0");
    extra.artefacts.insert("stray".into(), "0.3.0".into());
    assert!(matches!(
        r.create_release(extra, &config),
        Err(RolloutError::Invalid(m)) if m.contains("stray in no ring")
    ));
    assert!(matches!(
        r.create_release(release("0.2.0"), &config),
        Err(RolloutError::Conflict(_))
    ));
    assert!(matches!(
        r.create_release(release("bad version"), &config),
        Err(RolloutError::Invalid(_))
    ));
}

#[test]
fn rings_pass_in_order_after_their_soak_and_the_channel_moves() {
    let (mut r, config) = setup();
    let mut now = 1_000;
    let (id, events) = r.promote("stable", "0.2.0", &config, "rm", now).unwrap();
    assert_eq!(id, "run-1");
    assert!(matches!(&events[1], Event::RingStarted { ring, .. } if ring == "canary"));
    // Only the canary is raised.
    assert_eq!(desired_of(&r, &config, "forge-worker"), "0.2.0");
    assert_eq!(desired_of(&r, &config, "mimir-server"), "0.1.0");
    assert_eq!(desired_of(&r, &config, "http-gateway"), "0.1.0");

    // The agent has not installed yet: it reports the old version. Nothing happens.
    now += 10;
    assert!(r.tick(now, &[report(now, "0.1.0", &[], &[])]).is_empty());

    // Each ring: the agent follows its desired versions; the ring becomes ready, soaks
    // 60 s and passes, and the next ring starts.
    let mut passed = Vec::new();
    for _ in 0..200 {
        now += 10;
        let rep = obedient(&r, &config, now);
        for e in r.tick(now, &[rep]) {
            if let Event::RingPassed { ring, .. } = &e {
                passed.push(ring.clone());
            }
            if let Event::RingReady { .. } = &e {
                // Not passed in the same tick: it soaks first.
                assert!(r.run("run-1").unwrap().state == RunState::Running);
            }
        }
        if r.run("run-1").unwrap().state != RunState::Running {
            break;
        }
    }
    assert_eq!(passed, ["canary", "ring-2", "ring-3", "last"]);
    let run = r.run("run-1").unwrap();
    assert_eq!(run.state, RunState::Completed);
    assert!(run.rings.iter().all(|g| g.phase == RingPhase::Passed));
    // Each ring soaked at least its soak time.
    for g in &run.rings {
        assert!(g.ended_at.unwrap() - g.started_at.unwrap() >= DEFAULT_SOAK_SECS);
    }
    let channel = &r.channels["stable"];
    assert_eq!(channel.current, "0.2.0");
    assert_eq!(channel.previous.as_deref(), Some("0.1.0"));
    assert!(r.active_run("stable").is_none());
    assert!(r
        .desired("stable", &config)
        .unwrap()
        .services
        .values()
        .all(|v| v == "0.2.0"));
}

#[test]
fn soak_is_measured_from_readiness() {
    let (mut r, config) = setup();
    r.promote("stable", "0.2.0", &config, "rm", 0).unwrap();
    let canary_on = |at| report(at, "0.1.0", &[("forge-worker", "0.2.0")], &[]);
    let e = r.tick(30, &[canary_on(30)]);
    assert!(matches!(&e[..], [Event::RingReady { ring, .. }] if ring == "canary"));
    assert!(r.tick(89, &[canary_on(89)]).is_empty(), "59 s of soak");
    let e = r.tick(90, &[canary_on(90)]);
    assert!(matches!(&e[0], Event::RingPassed { ring, .. } if ring == "canary"));
    assert!(matches!(&e[1], Event::RingStarted { ring, .. } if ring == "ring-2"));
    assert_eq!(desired_of(&r, &config, "heimdall-source"), "0.2.0");
}

#[test]
fn not_ready_by_the_timeout_rolls_every_changed_service_back() {
    let (mut r, config) = setup();
    r.promote("stable", "0.2.0", &config, "rm", 0).unwrap();
    // The canary passes.
    let canary_on = |at| report(at, "0.1.0", &[("forge-worker", "0.2.0")], &[]);
    r.tick(10, &[canary_on(10)]);
    r.tick(70, &[canary_on(70)]);
    assert_eq!(r.run("run-1").unwrap().rings[1].phase, RingPhase::Waiting);
    assert_eq!(desired_of(&r, &config, "midgard-replay"), "0.2.0");

    // Ring 2: heimdall-feed never comes up on 0.2.0 (it keeps reporting 0.2.0, unhealthy).
    let stuck = |at| {
        report(
            at,
            "0.1.0",
            &[
                ("forge-worker", "0.2.0"),
                ("midgard-replay", "0.2.0"),
                ("heimdall-feed", "0.2.0"),
                ("heimdall-source", "0.2.0"),
            ],
            &["heimdall-feed"],
        )
    };
    let deadline = 70 + DEFAULT_READY_TIMEOUT_SECS;
    assert!(r.tick(deadline - 1, &[stuck(deadline - 1)]).is_empty());
    let events = r.tick(deadline, &[stuck(deadline)]);
    let reason = match &events[..] {
        [Event::RingFailed { ring, reason, .. }, Event::RolledBack { to, .. }] => {
            assert_eq!(ring, "ring-2");
            assert_eq!(to, "0.1.0");
            reason.clone()
        }
        other => panic!("{other:?}"),
    };
    assert!(reason.contains("not ready within 180 s"), "{reason}");
    assert!(
        reason.contains("heimdall-feed on mac-suite unhealthy"),
        "{reason}"
    );

    let run = r.run("run-1").unwrap();
    assert_eq!(run.state, RunState::RolledBack);
    assert_eq!(run.reason.as_deref(), Some(reason.as_str()));
    assert_eq!(run.rings[0].phase, RingPhase::RolledBack);
    assert_eq!(run.rings[1].phase, RingPhase::Failed);
    assert_eq!(run.rings[2].phase, RingPhase::Pending);
    // Every service, the passed canary included, is desired back on 0.1.0.
    assert!(r
        .desired("stable", &config)
        .unwrap()
        .services
        .values()
        .all(|v| v == "0.1.0"));
    assert_eq!(r.channels["stable"].current, "0.1.0");
    assert!(r.active_run("stable").is_none());
}

#[test]
fn no_reporting_installation_times_out_too() {
    let (mut r, config) = setup();
    r.promote("stable", "0.2.0", &config, "rm", 0).unwrap();
    assert!(r.tick(179, &[]).is_empty());
    let events = r.tick(180, &[]);
    assert!(
        matches!(&events[0], Event::RingFailed { reason, .. } if reason.contains("no installation on stable is reporting")),
        "{events:?}"
    );
}

#[test]
fn unhealthy_after_ready_rolls_back_at_once() {
    let (mut r, config) = setup();
    r.promote("stable", "0.2.0", &config, "rm", 0).unwrap();
    let on = |at, unhealthy: &[&str]| report(at, "0.1.0", &[("forge-worker", "0.2.0")], unhealthy);
    r.tick(10, &[on(10, &[])]);
    assert_eq!(r.run("run-1").unwrap().rings[0].phase, RingPhase::Soaking);
    let events = r.tick(20, &[on(20, &["forge-worker"])]);
    assert!(
        matches!(&events[0], Event::RingFailed { ring, reason, .. }
            if ring == "canary" && reason.contains("became unhealthy after reporting ready")),
        "{events:?}"
    );
    assert_eq!(r.run("run-1").unwrap().state, RunState::RolledBack);
    assert_eq!(desired_of(&r, &config, "forge-worker"), "0.1.0");

    // Going back to another version after ready is a failure too.
    r.promote("stable", "0.2.0", &config, "rm", 100).unwrap();
    r.tick(110, &[on(110, &[])]);
    let events = r.tick(120, &[report(120, "0.1.0", &[], &[])]);
    assert!(
        matches!(&events[0], Event::RingFailed { reason, .. } if reason.contains("went from 0.2.0 to 0.1.0")),
        "{events:?}"
    );
}

#[test]
fn a_quiet_agent_stops_the_soak_but_does_not_fail_it() {
    let (mut r, config) = setup();
    r.promote("stable", "0.2.0", &config, "rm", 0).unwrap();
    let on = |at| report(at, "0.1.0", &[("forge-worker", "0.2.0")], &[]);
    r.tick(10, &[on(10)]);
    // The last report is 31 s old at 41: it no longer counts.
    assert!(r.tick(41, &[on(10)]).is_empty());
    let ring = &r.run("run-1").unwrap().rings[0];
    assert_eq!(ring.phase, RingPhase::Waiting);
    assert_eq!(ring.deadline, Some(41 + 180), "a fresh readiness window");
    // It reports again and soaks a full minute from then.
    r.tick(50, &[on(50)]);
    assert!(r.tick(109, &[on(109)]).is_empty());
    assert!(matches!(
        &r.tick(110, &[on(110)])[0],
        Event::RingPassed { .. }
    ));
}

#[test]
fn a_manual_rollback_stops_the_run_and_returns_every_changed_service() {
    let (mut r, config) = setup();
    r.promote("stable", "0.2.0", &config, "rm", 0).unwrap();
    let on = |at| report(at, "0.1.0", &[("forge-worker", "0.2.0")], &[]);
    r.tick(10, &[on(10)]);
    r.tick(70, &[on(70)]);
    assert_eq!(desired_of(&r, &config, "heimdall-feed"), "0.2.0");

    assert!(matches!(
        r.rollback("stable", None, "  ", "rm", 80),
        Err(RolloutError::Invalid(_))
    ));
    assert!(matches!(
        r.rollback("stable", Some("0.2.0"), "x", "rm", 80),
        Err(RolloutError::Invalid(_))
    ));
    let (id, events) = r
        .rollback("stable", None, "bad dashboards", "rm", 80)
        .unwrap();
    assert_eq!(id, "run-1");
    assert!(
        matches!(&events[..], [Event::RolledBack { reason, to, .. }] if reason == "manual: bad dashboards" && to == "0.1.0")
    );
    let run = r.run("run-1").unwrap();
    assert_eq!(run.state, RunState::RolledBack);
    assert_eq!(run.ended_by.as_deref(), Some("rm"));
    assert_eq!(run.rings[0].phase, RingPhase::RolledBack);
    assert_eq!(run.rings[1].phase, RingPhase::RolledBack);
    assert!(r
        .desired("stable", &config)
        .unwrap()
        .services
        .values()
        .all(|v| v == "0.1.0"));
    // Ticks after it change nothing.
    assert!(r.tick(500, &[on(500)]).is_empty());
}

#[test]
fn a_manual_rollback_with_no_run_moves_the_channel_back() {
    let (mut r, config) = setup();
    assert!(matches!(
        r.rollback("stable", None, "why", "rm", 0),
        Err(RolloutError::Conflict(m)) if m.contains("no previous release")
    ));
    // Complete 0.2.0 quickly with a zero soak.
    let mut fast = config.clone();
    for g in &mut fast.rings {
        g.soak_secs = 0;
    }
    r.promote("stable", "0.2.0", &fast, "rm", 0).unwrap();
    for t in 1..10 {
        let rep = obedient(&r, &fast, t);
        r.tick(t, &[rep]);
    }
    assert_eq!(r.channels["stable"].current, "0.2.0");

    let (id, _) = r.rollback("stable", None, "regression", "rm", 20).unwrap();
    let run = r.run(&id).unwrap();
    assert_eq!(run.kind, RunKind::Rollback);
    assert_eq!(
        (run.from.as_str(), run.release.as_str()),
        ("0.2.0", "0.1.0")
    );
    assert_eq!(r.channels["stable"].current, "0.1.0");
    assert_eq!(r.channels["stable"].previous.as_deref(), Some("0.2.0"));
    assert_eq!(desired_of(&r, &config, "http-gateway"), "0.1.0");
    assert!(matches!(
        r.rollback("stable", Some("0.1.0"), "again", "rm", 21),
        Err(RolloutError::Conflict(_))
    ));
    assert!(matches!(
        r.rollback("stable", Some("9.9.9"), "x", "rm", 21),
        Err(RolloutError::NotFound(_))
    ));
    // And forward again.
    r.promote("stable", "0.2.0", &config, "rm", 30).unwrap();
}

#[test]
fn one_run_per_channel_and_promote_refusals() {
    let (mut r, config) = setup();
    assert!(matches!(
        r.promote("stable", "0.1.0", &config, "rm", 0),
        Err(RolloutError::Conflict(m)) if m.contains("already on 0.1.0")
    ));
    assert!(matches!(
        r.promote("stable", "9.9.9", &config, "rm", 0),
        Err(RolloutError::NotFound(_))
    ));
    assert!(matches!(
        r.promote("edge", "0.2.0", &config, "rm", 0),
        Err(RolloutError::NotFound(_))
    ));
    r.promote("stable", "0.2.0", &config, "rm", 0).unwrap();
    r.create_release(release("0.3.0"), &config).unwrap();
    assert!(matches!(
        r.promote("stable", "0.3.0", &config, "rm", 5),
        Err(RolloutError::Conflict(m)) if m.contains("one run per channel")
    ));
    // Another channel may run at the same time.
    r.seed_channel("edge", "0.1.0").unwrap();
    r.promote("edge", "0.3.0", &config, "rm", 5).unwrap();
    assert_eq!(r.active_run("edge").unwrap().id, "run-2");
    // Seeding a known channel does not move it.
    assert!(!r.seed_channel("stable", "0.3.0").unwrap());
    assert_eq!(r.channels["stable"].current, "0.1.0");
}

#[test]
fn a_run_resumes_mid_ring_from_its_saved_state() {
    let (mut r, config) = setup();
    r.promote("stable", "0.2.0", &config, "rm", 0).unwrap();
    let on = |at| report(at, "0.1.0", &[("forge-worker", "0.2.0")], &[]);
    r.tick(10, &[on(10)]);
    r.tick(70, &[on(70)]);
    let ring2 = |at| {
        report(
            at,
            "0.1.0",
            &[
                ("forge-worker", "0.2.0"),
                ("midgard-replay", "0.2.0"),
                ("heimdall-feed", "0.2.0"),
                ("heimdall-source", "0.2.0"),
            ],
            &[],
        )
    };
    r.tick(100, &[ring2(100)]);
    assert_eq!(r.run("run-1").unwrap().rings[1].phase, RingPhase::Soaking);

    // "Restart": the state is all that survives (the server persists it whole); the
    // reports do not.
    let mut resumed: Rollouts = r.clone();
    assert_eq!(resumed, r);
    // No report yet after the restart: soaking stops, and the deadline is fresh.
    assert!(resumed.tick(140, &[]).is_empty());
    let ring = &resumed.run("run-1").unwrap().rings[1];
    assert_eq!(ring.phase, RingPhase::Waiting);
    assert_eq!(ring.deadline, Some(140 + 180));
    assert_eq!(desired_of(&resumed, &config, "heimdall-feed"), "0.2.0");
    // The agent checks in again and the run carries on to the end.
    let mut now = 140;
    while resumed.active_run("stable").is_some() && now < 2_000 {
        now += 10;
        let rep = obedient(&resumed, &config, now);
        resumed.tick(now, &[rep]);
    }
    assert_eq!(resumed.run("run-1").unwrap().state, RunState::Completed);
    assert_eq!(resumed.channels["stable"].current, "0.2.0");
}
