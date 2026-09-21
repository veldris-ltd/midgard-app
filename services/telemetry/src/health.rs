//! Liveness & readiness probes. **Liveness** answers "is the process running and not [NFR-PRV-1]
//! deadlocked" (a service that responds is live). **Readiness** answers "can it serve
//! traffic right now" — the AND of registered dependency checks (a KMS handle, a
//! sealed store, a warmed model). A service exposes `/health` (liveness) and `/ready`
//! (readiness); an orchestrator gates traffic on readiness. In-enclosure only. [RC-33]

use std::sync::Mutex;

/// The outcome of one dependency check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    /// The dependency name.
    pub name: String,
    /// Whether it is healthy.
    pub healthy: bool,
    /// Human-readable detail.
    pub detail: String,
}

/// A readiness dependency check. Must be non-blocking (no external I/O in the hot
/// path); it reports cached/derived health.
pub trait ReadinessCheck: Send + Sync {
    /// The dependency name.
    fn name(&self) -> &str;
    /// Evaluate the dependency's health right now.
    fn check(&self) -> bool;
    /// Optional detail for the report.
    fn detail(&self) -> String {
        String::new()
    }
}

/// A togglable readiness check (a dependency flips ready/not-ready at runtime).
pub struct Toggle {
    name: String,
    ready: Mutex<bool>,
}

impl Toggle {
    /// A check named `name`, initially `ready`.
    pub fn new(name: &str, ready: bool) -> Self {
        Self {
            name: name.to_string(),
            ready: Mutex::new(ready),
        }
    }
    /// Flip the readiness of this dependency.
    pub fn set(&self, ready: bool) {
        *self.ready.lock().expect("toggle lock") = ready;
    }
}

impl ReadinessCheck for Toggle {
    fn name(&self) -> &str {
        &self.name
    }
    fn check(&self) -> bool {
        *self.ready.lock().expect("toggle lock")
    }
    fn detail(&self) -> String {
        if self.check() {
            "ready".into()
        } else {
            "not ready".into()
        }
    }
}

/// A shared toggle can be registered as a check while a handle is kept to flip it at
/// runtime (a service holds the `Arc` and updates readiness as dependencies change).
impl ReadinessCheck for std::sync::Arc<Toggle> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn check(&self) -> bool {
        (**self).check()
    }
    fn detail(&self) -> String {
        (**self).detail()
    }
}

/// A health report: liveness plus per-dependency readiness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// Whether the process is live.
    pub live: bool,
    /// Whether every readiness dependency is healthy.
    pub ready: bool,
    /// Each dependency's result.
    pub checks: Vec<CheckResult>,
}

impl HealthReport {
    /// Render as compact JSON (no serde).
    pub fn to_json(&self) -> String {
        let checks: Vec<String> = self
            .checks
            .iter()
            .map(|c| {
                format!(
                    "{{\"name\":\"{}\",\"healthy\":{},\"detail\":\"{}\"}}",
                    esc(&c.name),
                    c.healthy,
                    esc(&c.detail)
                )
            })
            .collect();
        format!(
            "{{\"live\":{},\"ready\":{},\"checks\":[{}]}}",
            self.live,
            self.ready,
            checks.join(",")
        )
    }
}

/// The service's health registry: liveness is implicit (a served request proves the
/// process is live), readiness is the AND of registered checks.
#[derive(Default)]
pub struct HealthRegistry {
    checks: Vec<Box<dyn ReadinessCheck>>,
}

impl HealthRegistry {
    /// A new registry with no readiness dependencies (ready by default).
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a readiness dependency.
    pub fn register(&mut self, check: Box<dyn ReadinessCheck>) {
        self.checks.push(check);
    }

    /// The liveness report (always live if this is running).
    pub fn liveness(&self) -> HealthReport {
        HealthReport {
            live: true,
            ready: self.is_ready(),
            checks: Vec::new(),
        }
    }

    /// The readiness report: the AND of every dependency check, with detail.
    pub fn readiness(&self) -> HealthReport {
        let checks: Vec<CheckResult> = self
            .checks
            .iter()
            .map(|c| CheckResult {
                name: c.name().to_string(),
                healthy: c.check(),
                detail: c.detail(),
            })
            .collect();
        let ready = checks.iter().all(|c| c.healthy);
        HealthReport {
            live: true,
            ready,
            checks,
        }
    }

    /// Whether the service is ready to serve traffic.
    pub fn is_ready(&self) -> bool {
        self.checks.iter().all(|c| c.check())
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
