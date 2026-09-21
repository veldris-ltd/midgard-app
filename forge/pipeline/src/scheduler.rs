//! The scheduler adapter: batch + streaming trigger management for pipeline runs.
//! It decides *which* pipelines are due on a batch tick versus on a stream event;
//! the actual run reuses [`crate::run`]. It is deterministic and clock-free — a
//! caller drives it with ticks/events — so scheduling is reproducible and needs no
//! ambient time source (air-gap-friendly). [FR-F-5] [NFR-PRV-1]

/// How a pipeline is triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Run only on explicit request.
    Manual,
    /// Run on each batch tick (scheduled batch).
    Batch,
    /// Run on each inbound event / micro-batch (streaming).
    Stream,
}

/// A registration binding a pipeline id to a trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// The pipeline this schedule drives.
    pub pipeline_id: String,
    /// Its trigger.
    pub trigger: Trigger,
}

impl Schedule {
    /// A batch-triggered schedule.
    pub fn batch(pipeline_id: impl Into<String>) -> Self {
        Self {
            pipeline_id: pipeline_id.into(),
            trigger: Trigger::Batch,
        }
    }

    /// A stream-triggered schedule.
    pub fn stream(pipeline_id: impl Into<String>) -> Self {
        Self {
            pipeline_id: pipeline_id.into(),
            trigger: Trigger::Stream,
        }
    }

    /// A manual (on-request) schedule.
    pub fn manual(pipeline_id: impl Into<String>) -> Self {
        Self {
            pipeline_id: pipeline_id.into(),
            trigger: Trigger::Manual,
        }
    }
}

/// Holds pipeline schedules and answers "what is due now?" for each trigger source.
#[derive(Debug, Default)]
pub struct Scheduler {
    schedules: Vec<Schedule>,
}

impl Scheduler {
    /// An empty scheduler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a schedule.
    pub fn register(&mut self, schedule: Schedule) {
        self.schedules.push(schedule);
    }

    fn due(&self, trigger: Trigger) -> Vec<&str> {
        self.schedules
            .iter()
            .filter(|s| s.trigger == trigger)
            .map(|s| s.pipeline_id.as_str())
            .collect()
    }

    /// Pipelines due on a batch tick.
    pub fn due_on_tick(&self) -> Vec<&str> {
        self.due(Trigger::Batch)
    }

    /// Pipelines due on a stream event / micro-batch.
    pub fn due_on_event(&self) -> Vec<&str> {
        self.due(Trigger::Stream)
    }

    /// Manually-triggered pipelines (never fire on tick/event).
    pub fn manual(&self) -> Vec<&str> {
        self.due(Trigger::Manual)
    }
}
