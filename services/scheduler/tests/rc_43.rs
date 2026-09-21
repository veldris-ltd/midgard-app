//! RC-43: an over-quota workload is throttled/queued by priority. [SAD_00 §6.2.7]
//! [RT-43]
//!
//! Drives the [`QuotaScheduler`] through a realistic sequence: a quota pool fills,
//! further workloads are throttled into a priority queue (not dropped), a
//! high-priority arrival preempts best-effort work, and completions drain the
//! queue highest-priority-first.

use scheduler::{Admission, Priority, QuotaScheduler, Resources, TenantId, Workload, WorkloadId};

fn wl(id: &str, tenant: &str, prio: Priority, cpu: u64, mem: u64, gpu: u32) -> Workload {
    Workload {
        id: WorkloadId(id.into()),
        tenant: TenantId(tenant.into()),
        priority: prio,
        request: Resources::new(cpu, mem, gpu),
    }
}
fn ids(v: &[WorkloadId]) -> Vec<String> {
    v.iter().map(|w| w.0.clone()).collect()
}

#[test]
fn over_quota_workload_is_throttled_and_queued_by_priority() {
    // A namespace pool: 4 cores, 8 GiB, 1 GPU (mirrors an enclosure ResourceQuota).
    let mut s = QuotaScheduler::new(Resources::new(4000, 8192, 1));

    // Two High-priority pipeline jobs fill most of the pool. (High so the later
    // arrivals queue behind them rather than preempting — this test is about the
    // throttle+priority-queue path; preemption has its own test below.)
    assert!(matches!(
        s.submit(wl("ingest", "heimdall", Priority::High, 2000, 3072, 0)),
        Admission::Admitted { .. }
    ));
    assert!(matches!(
        s.submit(wl("fuse", "heimdall", Priority::High, 1500, 3072, 0)),
        Admission::Admitted { .. }
    ));
    assert_eq!(s.available(), Resources::new(500, 2048, 1));

    // Three more jobs all exceed the 500m free CPU → THROTTLED into the queue
    // rather than dropped, and ordered BY PRIORITY (High, then Normal, then Low).
    assert_eq!(
        s.submit(wl("nightly-batch", "ops", Priority::Low, 1000, 1024, 0)),
        Admission::Queued
    );
    assert_eq!(
        s.submit(wl("adhoc-query", "intel", Priority::Normal, 1000, 1024, 0)),
        Admission::Queued
    );
    assert_eq!(
        s.submit(wl("rush-triage", "intel", Priority::High, 1000, 1024, 0)),
        Admission::Queued
    );
    assert_eq!(
        ids(&s.queued_ids()),
        vec!["rush-triage", "adhoc-query", "nightly-batch"]
    );

    // "fuse" finishes, freeing 1500m/3072Mi. The queue drains highest-priority
    // first: "rush-triage" (High) then "adhoc-query" (Normal); the Low batch still
    // waits — a lower-priority job never overtakes a higher-priority one.
    let admitted = s.complete(&WorkloadId("fuse".into()));
    assert_eq!(ids(&admitted), vec!["rush-triage", "adhoc-query"]);
    assert_eq!(ids(&s.queued_ids()), vec!["nightly-batch"]);

    // Finally "ingest" finishes; the long-waiting Low batch is now admitted.
    let admitted = s.complete(&WorkloadId("ingest".into()));
    assert_eq!(ids(&admitted), vec!["nightly-batch"]);
    assert!(
        s.queued_ids().is_empty(),
        "queue fully drained once resources allowed"
    );
}

#[test]
fn critical_workload_preempts_best_effort_to_meet_priority() {
    // Priority scheduling's sharp edge: a System-critical workload that does not
    // fit evicts just enough lower-priority running work, which is requeued (not
    // lost) and readmitted when the critical job completes.
    let mut s = QuotaScheduler::new(Resources::new(2000, 2048, 0));
    assert!(matches!(
        s.submit(wl("batch-a", "ops", Priority::Low, 1000, 1024, 0)),
        Admission::Admitted { .. }
    ));
    assert!(matches!(
        s.submit(wl("batch-b", "ops", Priority::Normal, 1000, 1024, 0)),
        Admission::Admitted { .. }
    ));

    // Break-glass control-plane work needs 800m; evicting the Low batch alone
    // frees enough (lowest priority first, minimally) — the Normal job keeps running.
    match s.submit(wl("break-glass", "admin", Priority::System, 800, 512, 0)) {
        Admission::Admitted { preempted } => assert_eq!(ids(&preempted), vec!["batch-a"]),
        other => panic!("System workload must preempt to run, got {other:?}"),
    }
    assert_eq!(ids(&s.running_ids()), vec!["batch-b", "break-glass"]);
    assert_eq!(
        ids(&s.queued_ids()),
        vec!["batch-a"],
        "preempted work is requeued"
    );

    // When the critical job finishes, the preempted batch is readmitted.
    assert_eq!(
        ids(&s.complete(&WorkloadId("break-glass".into()))),
        vec!["batch-a"]
    );
}
