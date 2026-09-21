//! Resource quotas + priority scheduling for the compute mesh. Quota admission and
//! priority preemption keep the shared pool available under contention — critical
//! workloads run, best-effort work is throttled, never starved. [NFR-AVA-1]
//! [SAD_00 §6.2.7] [RT-43] [RC-43]
//!
//! A [`QuotaScheduler`] admits workloads against a fixed resource **quota** (the
//! namespace ceiling — the same pool `deploy/k8s/enclosure.yaml` expresses as a
//! Kubernetes `ResourceQuota`). The rules:
//!
//! - A workload that fits the currently-free resources is **admitted** and its
//!   resources are reserved.
//! - A workload that fits the quota but not the free remainder is **throttled**:
//!   parked in a **priority queue** rather than dropped.
//! - A workload whose request alone exceeds the whole quota is **rejected** — it
//!   could never fit, so it fails fast instead of queuing forever.
//! - When resources free up ([`complete`](QuotaScheduler::complete)), queued
//!   workloads are dispatched in **strict priority order** (highest first; FIFO by
//!   submission within a priority). Dispatch stops at the first queued workload
//!   that does not fit, so a lower-priority workload never overtakes a
//!   higher-priority one waiting for room.
//! - Submitting a **higher-priority** workload that does not fit may **preempt**
//!   just enough strictly-lower-priority running workloads (lowest priority, most
//!   recently started first) to make room; the preempted workloads return to the
//!   queue at their original submission order.
//!
//! Pure-std and deterministic: identical inputs give identical admission
//! decisions, so the policy is testable in isolation from any real orchestrator.

use std::collections::BTreeMap;

/// A workload identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkloadId(pub String);

/// The owning tenant/namespace (a label; the quota is the shared pool ceiling).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantId(pub String);

/// Scheduling priority. Higher variants preempt and are dispatched ahead of lower
/// ones. Declaration order defines the ordering (`Low` < `Normal` < `High` <
/// `System`), so derived `Ord` is the scheduling order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Best-effort / batch.
    Low,
    /// Default interactive workloads.
    Normal,
    /// Latency-sensitive or operator-important.
    High,
    /// Platform-critical (control plane, break-glass); preempts everything below.
    System,
}

/// A resource request or capacity: CPU (milli-cores), memory (MiB), whole GPUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Resources {
    /// CPU in milli-cores (1000 = one core), matching Kubernetes `cpu` requests.
    pub cpu_millis: u64,
    /// Memory in mebibytes.
    pub memory_mib: u64,
    /// Whole GPUs (`nvidia.com/gpu`), which are integer and non-oversubscribable.
    pub gpus: u32,
}

impl Resources {
    /// Construct a request/capacity.
    pub fn new(cpu_millis: u64, memory_mib: u64, gpus: u32) -> Self {
        Self {
            cpu_millis,
            memory_mib,
            gpus,
        }
    }

    /// True iff every dimension of `self` is ≤ the matching dimension of `cap`.
    pub fn fits_within(&self, cap: &Resources) -> bool {
        self.cpu_millis <= cap.cpu_millis
            && self.memory_mib <= cap.memory_mib
            && self.gpus <= cap.gpus
    }

    /// Componentwise addition, saturating (a quota is small; saturation only bites
    /// on absurd inputs and never silently under-counts usage).
    fn add(&self, o: &Resources) -> Resources {
        Resources {
            cpu_millis: self.cpu_millis.saturating_add(o.cpu_millis),
            memory_mib: self.memory_mib.saturating_add(o.memory_mib),
            gpus: self.gpus.saturating_add(o.gpus),
        }
    }

    /// Componentwise subtraction, saturating at zero.
    fn sub(&self, o: &Resources) -> Resources {
        Resources {
            cpu_millis: self.cpu_millis.saturating_sub(o.cpu_millis),
            memory_mib: self.memory_mib.saturating_sub(o.memory_mib),
            gpus: self.gpus.saturating_sub(o.gpus),
        }
    }
}

/// A unit of work competing for quota.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workload {
    /// Unique id.
    pub id: WorkloadId,
    /// Owning tenant (label).
    pub tenant: TenantId,
    /// Scheduling priority.
    pub priority: Priority,
    /// Resources it needs while running.
    pub request: Resources,
}

/// The outcome of submitting a workload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Running now. `preempted` lists any workloads evicted to make room (empty in
    /// the common case).
    Admitted {
        /// Workloads evicted (returned to the queue) so this one could run.
        preempted: Vec<WorkloadId>,
    },
    /// Over the free remainder but within quota: parked in the priority queue.
    Queued,
    /// The request alone exceeds the whole quota; it can never fit.
    Rejected {
        /// Human-readable reason.
        reason: String,
    },
}

/// An entry with its submission sequence, for FIFO-within-priority ordering.
#[derive(Debug, Clone)]
struct Placed {
    seq: u64,
    workload: Workload,
}

/// A quota-bounded, priority-aware admission scheduler.
pub struct QuotaScheduler {
    quota: Resources,
    used: Resources,
    running: BTreeMap<WorkloadId, Placed>,
    queue: Vec<Placed>,
    seq: u64,
}

impl QuotaScheduler {
    /// A scheduler bounded by `quota` (the namespace resource ceiling).
    pub fn new(quota: Resources) -> Self {
        Self {
            quota,
            used: Resources::default(),
            running: BTreeMap::new(),
            queue: Vec::new(),
            seq: 0,
        }
    }

    /// Resources currently reserved by running workloads.
    pub fn used(&self) -> Resources {
        self.used
    }

    /// Resources still free within the quota.
    pub fn available(&self) -> Resources {
        self.quota.sub(&self.used)
    }

    /// Ids of running workloads (sorted).
    pub fn running_ids(&self) -> Vec<WorkloadId> {
        self.running.keys().cloned().collect()
    }

    /// Ids of queued workloads, in dispatch order (priority desc, then FIFO).
    pub fn queued_ids(&self) -> Vec<WorkloadId> {
        let mut q = self.queue.clone();
        Self::order(&mut q);
        q.into_iter().map(|p| p.workload.id).collect()
    }

    /// Submit a workload for admission. See [`Admission`] for the outcomes.
    pub fn submit(&mut self, workload: Workload) -> Admission {
        let seq = self.next_seq();

        // A request larger than the whole quota can never run — fail fast.
        if !workload.request.fits_within(&self.quota) {
            return Admission::Rejected {
                reason: format!(
                    "request {:?} exceeds the quota {:?}",
                    workload.request, self.quota
                ),
            };
        }

        // Fits the free remainder → admit immediately.
        if self.fits_free(&workload.request) {
            self.reserve(Placed { seq, workload });
            return Admission::Admitted {
                preempted: Vec::new(),
            };
        }

        // Doesn't fit free. Try to preempt strictly-lower-priority running work.
        if let Some(victims) = self.preemptible_for(&workload) {
            for v in &victims {
                self.evict_to_queue(v);
            }
            debug_assert!(self.fits_free(&workload.request));
            self.reserve(Placed { seq, workload });
            return Admission::Admitted { preempted: victims };
        }

        // Otherwise throttle: park in the priority queue.
        self.queue.push(Placed { seq, workload });
        Admission::Queued
    }

    /// Mark a running workload complete, releasing its resources, then dispatch as
    /// many queued workloads as now fit — in strict priority order. Returns the ids
    /// admitted from the queue as a result. Unknown ids release nothing.
    pub fn complete(&mut self, id: &WorkloadId) -> Vec<WorkloadId> {
        if let Some(p) = self.running.remove(id) {
            self.used = self.used.sub(&p.workload.request);
        }
        self.dispatch()
    }

    // --- internals ----------------------------------------------------------

    fn next_seq(&mut self) -> u64 {
        let s = self.seq;
        self.seq += 1;
        s
    }

    fn fits_free(&self, req: &Resources) -> bool {
        self.used.add(req).fits_within(&self.quota)
    }

    fn reserve(&mut self, placed: Placed) {
        self.used = self.used.add(&placed.workload.request);
        self.running.insert(placed.workload.id.clone(), placed);
    }

    fn evict_to_queue(&mut self, id: &WorkloadId) {
        if let Some(p) = self.running.remove(id) {
            self.used = self.used.sub(&p.workload.request);
            self.queue.push(p);
        }
    }

    /// The set of running workloads to preempt so `incoming` fits, or `None` if no
    /// admissible set exists. Only strictly-lower-priority workloads are eligible;
    /// evict lowest-priority (then most-recently-started) first, minimally.
    fn preemptible_for(&self, incoming: &Workload) -> Option<Vec<WorkloadId>> {
        let mut candidates: Vec<&Placed> = self
            .running
            .values()
            .filter(|p| p.workload.priority < incoming.priority)
            .collect();
        // Evict lowest priority first; within a priority, most recently started
        // (highest seq) first — cheapest, least-invested victims.
        candidates.sort_by(|a, b| {
            a.workload
                .priority
                .cmp(&b.workload.priority)
                .then(b.seq.cmp(&a.seq))
        });

        let mut freed = self.available();
        let mut victims = Vec::new();
        for c in candidates {
            if incoming.request.fits_within(&freed) {
                break;
            }
            freed = freed.add(&c.workload.request);
            victims.push(c.workload.id.clone());
        }
        if incoming.request.fits_within(&freed) {
            Some(victims)
        } else {
            None
        }
    }

    /// Dispatch queued workloads in strict priority order until the head no longer
    /// fits. Returns the ids admitted.
    fn dispatch(&mut self) -> Vec<WorkloadId> {
        let mut admitted = Vec::new();
        loop {
            Self::order(&mut self.queue);
            let Some(head) = self.queue.first() else {
                break;
            };
            if !self.fits_free(&head.workload.request) {
                // Strict priority: do not backfill past a higher-priority waiter.
                break;
            }
            let placed = self.queue.remove(0);
            let id = placed.workload.id.clone();
            self.reserve(placed);
            admitted.push(id);
        }
        admitted
    }

    /// Order a queue by priority descending, then submission sequence ascending
    /// (FIFO within a priority).
    fn order(q: &mut [Placed]) {
        q.sort_by(|a, b| {
            b.workload
                .priority
                .cmp(&a.workload.priority)
                .then(a.seq.cmp(&b.seq))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wl(id: &str, prio: Priority, cpu: u64, mem: u64, gpu: u32) -> Workload {
        Workload {
            id: WorkloadId(id.into()),
            tenant: TenantId("t".into()),
            priority: prio,
            request: Resources::new(cpu, mem, gpu),
        }
    }
    fn ids(v: &[WorkloadId]) -> Vec<String> {
        v.iter().map(|w| w.0.clone()).collect()
    }

    #[test]
    fn admits_within_quota_and_reserves() {
        let mut s = QuotaScheduler::new(Resources::new(4000, 4096, 2));
        assert_eq!(
            s.submit(wl("a", Priority::Normal, 1000, 1024, 0)),
            Admission::Admitted { preempted: vec![] }
        );
        assert_eq!(s.used(), Resources::new(1000, 1024, 0));
        assert_eq!(s.available(), Resources::new(3000, 3072, 2));
    }

    #[test]
    fn over_quota_request_is_rejected_not_queued() {
        let mut s = QuotaScheduler::new(Resources::new(2000, 2048, 1));
        match s.submit(wl("huge", Priority::High, 8000, 1024, 0)) {
            Admission::Rejected { .. } => {}
            other => panic!("a request bigger than the quota must be rejected, got {other:?}"),
        }
        assert!(s.running_ids().is_empty() && s.queued_ids().is_empty());
    }

    #[test]
    fn over_capacity_workload_is_throttled_into_the_queue() {
        let mut s = QuotaScheduler::new(Resources::new(2000, 2048, 0));
        assert_eq!(
            s.submit(wl("a", Priority::Normal, 1500, 1024, 0)),
            Admission::Admitted { preempted: vec![] }
        );
        // Fits the quota (2000) but not the 500 free → queued, not dropped.
        assert_eq!(
            s.submit(wl("b", Priority::Normal, 1000, 512, 0)),
            Admission::Queued
        );
        assert_eq!(ids(&s.queued_ids()), vec!["b"]);
    }

    #[test]
    fn queue_drains_in_priority_order_on_completion() {
        let mut s = QuotaScheduler::new(Resources::new(1000, 4096, 0));
        // A top-priority job holds the CPU, so the queued arrivals cannot preempt
        // it — isolating the queue-drain behaviour from preemption.
        assert!(matches!(
            s.submit(wl("run", Priority::System, 1000, 512, 0)),
            Admission::Admitted { .. }
        ));
        assert_eq!(
            s.submit(wl("low", Priority::Low, 600, 512, 0)),
            Admission::Queued
        );
        assert_eq!(
            s.submit(wl("high", Priority::High, 600, 512, 0)),
            Admission::Queued
        );
        assert_eq!(
            s.submit(wl("norm", Priority::Normal, 600, 512, 0)),
            Admission::Queued
        );
        // Queue order: high, then norm, then low (FIFO within priority).
        assert_eq!(ids(&s.queued_ids()), vec!["high", "norm", "low"]);
        // Freeing 1000 CPU admits only "high" (600); "norm" would need another 600.
        let admitted = s.complete(&WorkloadId("run".into()));
        assert_eq!(ids(&admitted), vec!["high"]);
        assert_eq!(
            ids(&s.queued_ids()),
            vec!["norm", "low"],
            "lower priority never overtakes higher"
        );
    }

    #[test]
    fn higher_priority_preempts_lower_running_work() {
        let mut s = QuotaScheduler::new(Resources::new(1000, 1024, 0));
        assert!(matches!(
            s.submit(wl("batch", Priority::Low, 1000, 1024, 0)),
            Admission::Admitted { .. }
        ));
        // System work arrives, doesn't fit → preempts the Low batch.
        match s.submit(wl("ctl", Priority::System, 800, 512, 0)) {
            Admission::Admitted { preempted } => assert_eq!(ids(&preempted), vec!["batch"]),
            other => panic!("System workload should preempt Low, got {other:?}"),
        }
        assert_eq!(ids(&s.running_ids()), vec!["ctl"]);
        assert_eq!(
            ids(&s.queued_ids()),
            vec!["batch"],
            "preempted work is requeued, not lost"
        );
        // When ctl finishes, the requeued batch is re-admitted.
        assert_eq!(ids(&s.complete(&WorkloadId("ctl".into()))), vec!["batch"]);
    }

    #[test]
    fn equal_priority_does_not_preempt() {
        let mut s = QuotaScheduler::new(Resources::new(1000, 1024, 0));
        assert!(matches!(
            s.submit(wl("a", Priority::Normal, 1000, 1024, 0)),
            Admission::Admitted { .. }
        ));
        // Same priority cannot preempt — it queues.
        assert_eq!(
            s.submit(wl("b", Priority::Normal, 500, 512, 0)),
            Admission::Queued
        );
        assert_eq!(ids(&s.running_ids()), vec!["a"]);
    }

    #[test]
    fn gpus_are_a_hard_integer_constraint() {
        let mut s = QuotaScheduler::new(Resources::new(8000, 8192, 1));
        assert!(matches!(
            s.submit(wl("g1", Priority::Normal, 1000, 1024, 1)),
            Admission::Admitted { .. }
        ));
        // Only one GPU in the pool; a second GPU workload must queue despite ample CPU/mem.
        assert_eq!(
            s.submit(wl("g2", Priority::Normal, 1000, 1024, 1)),
            Admission::Queued
        );
        assert_eq!(s.available(), Resources::new(7000, 7168, 0));
    }
}
