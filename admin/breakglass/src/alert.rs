//! Real-time break-glass alerting. The sink is a trait so production wires it to
//! the in-enclosure alert bus while tests use a deterministic recorder — no
//! network dependency either way. `[FR-AD-8]` `[AC-AD-12]`

/// The kind of break-glass alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// Emergency elevation was granted (raised immediately). `[FR-AD-8]`
    Granted,
    /// A grant reached its time-box and expired without a post-hoc review yet.
    ExpiredUnreviewed,
}

/// A break-glass alert delivered to security officers in real time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakGlassAlert {
    /// What happened.
    pub kind: AlertKind,
    /// The grant id.
    pub grant_id: String,
    /// The elevated principal.
    pub principal: String,
    /// Human-readable detail.
    pub detail: String,
}

/// A sink that receives break-glass alerts as they happen.
pub trait Alerter {
    /// Deliver an alert. Must not block on anything external.
    fn alert(&mut self, alert: BreakGlassAlert);
}

/// A deterministic in-memory alerter (tests / dev): records alerts in order.
#[derive(Debug, Default)]
pub struct RecordingAlerter {
    alerts: Vec<BreakGlassAlert>,
}

impl RecordingAlerter {
    /// A new empty recorder.
    pub fn new() -> Self {
        Self { alerts: Vec::new() }
    }

    /// The alerts raised so far, in order.
    pub fn alerts(&self) -> &[BreakGlassAlert] {
        &self.alerts
    }

    /// How many alerts of a given kind have been raised.
    pub fn count(&self, kind: AlertKind) -> usize {
        self.alerts.iter().filter(|a| a.kind == kind).count()
    }
}

impl Alerter for RecordingAlerter {
    fn alert(&mut self, alert: BreakGlassAlert) {
        self.alerts.push(alert);
    }
}

// ---- real in-enclosure alert bus + durable delivery -----------------------

/// A transient failure delivering to the alert bus (the bus is momentarily
/// unavailable). The [`DurableAlerter`] retries — an alert is never dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertBusError(pub String);

/// The in-enclosure alert bus a break-glass alerter publishes to. Production wires
/// this to the real security-officer notification channel; it stays in-enclosure
/// (no outbound network). Delivery may fail transiently; the alerter handles retry.
/// [FR-AD-8] [AC-AD-12]
pub trait AlertBus {
    /// Deliver one alert to subscribed security officers. `Err` means try again.
    fn deliver(&mut self, alert: &BreakGlassAlert) -> Result<(), AlertBusError>;
}

/// A durable [`Alerter`] over an [`AlertBus`]: every alert is enqueued and retained
/// until the bus **acknowledges** delivery, so a transient bus outage never drops a
/// break-glass alert (at-least-once delivery). Undelivered alerts stay pending and
/// are retried on [`DurableAlerter::flush`]. The pending queue is the delivery
/// durability guarantee; a deployment persists it through the same sealed store the
/// grants use for cross-restart durability. [FR-AD-8] [AC-AD-12] [RC-32]
pub struct DurableAlerter<B: AlertBus> {
    bus: B,
    pending: Vec<BreakGlassAlert>,
    delivered: usize,
}

impl<B: AlertBus> DurableAlerter<B> {
    /// Build a durable alerter over an alert bus.
    pub fn new(bus: B) -> Self {
        Self {
            bus,
            pending: Vec::new(),
            delivered: 0,
        }
    }

    /// Retry every pending (undelivered) alert; returns the running delivered count.
    pub fn flush(&mut self) -> usize {
        self.drain();
        self.delivered
    }

    /// How many alerts are still awaiting delivery (retained, not dropped).
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// How many alerts the bus has acknowledged.
    pub fn delivered(&self) -> usize {
        self.delivered
    }

    /// The underlying bus (for inspection).
    pub fn bus(&self) -> &B {
        &self.bus
    }

    fn drain(&mut self) {
        let mut still = Vec::new();
        for a in std::mem::take(&mut self.pending) {
            match self.bus.deliver(&a) {
                Ok(()) => self.delivered += 1,
                Err(_) => still.push(a),
            }
        }
        self.pending = still;
    }
}

impl<B: AlertBus> Alerter for DurableAlerter<B> {
    fn alert(&mut self, alert: BreakGlassAlert) {
        // Enqueue first (durable), then attempt immediate delivery; on failure the
        // alert remains pending for retry — never lost.
        self.pending.push(alert);
        self.drain();
    }
}

/// A security-officer inbox standing in for the real notification channel (tests /
/// dev): it records delivered alerts and can simulate a transient outage to exercise
/// durable retry. A real deployment replaces it with the actual SO channel.
#[derive(Debug, Default)]
pub struct SecurityOfficerBus {
    received: Vec<BreakGlassAlert>,
    fail_remaining: usize,
}

impl SecurityOfficerBus {
    /// A new, healthy bus.
    pub fn new() -> Self {
        Self::default()
    }

    /// Make the next `n` deliveries fail transiently (to test durable retry).
    pub fn fail_next(&mut self, n: usize) {
        self.fail_remaining = n;
    }

    /// The alerts security officers have actually received, in order.
    pub fn received(&self) -> &[BreakGlassAlert] {
        &self.received
    }
}

impl AlertBus for SecurityOfficerBus {
    fn deliver(&mut self, alert: &BreakGlassAlert) -> Result<(), AlertBusError> {
        if self.fail_remaining > 0 {
            self.fail_remaining -= 1;
            return Err(AlertBusError("alert bus momentarily unavailable".into()));
        }
        self.received.push(alert.clone());
        Ok(())
    }
}
