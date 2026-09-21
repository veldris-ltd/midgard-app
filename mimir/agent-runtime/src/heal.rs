//! Self-healing supervisor (A.4).
//!
//! On a failure or degradation signal, the supervisor chooses a bounded,
//! auditable recovery — retry / reroute / rollback / halt — so the runtime
//! recovers automatically (AC-MI-5). It never repeats an identical failing action
//! more than the retry budget, and escalates to a human on anything that could
//! change externally-visible state or that looks like a security event. The A.4
//! prompt (`mimir_prompts::A4_SELF_HEALING`) is the contract; [`SelfHealing`] is
//! the deterministic reference supervisor. [FR-MI-6]

/// What went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// A transient, likely-retryable error.
    Transient,
    /// The model/inference backend is unavailable.
    ModelDown,
    /// A tool call failed.
    ToolFailed,
    /// The same action keeps failing (a loop).
    LoopDetected,
    /// The failure looks like an attack / security event.
    SecurityEvent,
}

/// A recovery action the supervisor may choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Retry the same action.
    Retry,
    /// Route to a different model.
    RerouteModel,
    /// Route to a different tool.
    RerouteTool,
    /// Roll back and abandon the step.
    Rollback,
    /// Stop; no safe recovery.
    Halt,
}

impl RecoveryAction {
    /// The A.4 contract string.
    pub fn as_str(self) -> &'static str {
        match self {
            RecoveryAction::Retry => "retry",
            RecoveryAction::RerouteModel => "reroute_model",
            RecoveryAction::RerouteTool => "reroute_tool",
            RecoveryAction::Rollback => "rollback",
            RecoveryAction::Halt => "halt",
        }
    }
}

/// The context handed to the supervisor.
#[derive(Debug, Clone)]
pub struct FailureContext {
    /// What failed.
    pub kind: FailureKind,
    /// The recoveries permitted here.
    pub allowed_recoveries: Vec<RecoveryAction>,
    /// The retry budget for this step.
    pub retry_budget: u32,
    /// Attempts already made.
    pub attempts: u32,
    /// Whether the failing step could change externally-visible state.
    pub externally_visible: bool,
}

impl FailureContext {
    fn allows(&self, a: RecoveryAction) -> bool {
        self.allowed_recoveries.contains(&a)
    }
}

/// The supervisor's chosen recovery (mirrors the A.4 JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// Short diagnosis.
    pub diagnosis: String,
    /// The chosen action.
    pub recovery: RecoveryAction,
    /// The reroute target, if any.
    pub target: Option<String>,
    /// Remaining attempts after this decision.
    pub max_attempts_remaining: i64,
    /// Whether a human must sign off.
    pub requires_human: bool,
}

/// The supervisor seam. The real A.4 model plugs in behind this.
pub trait RecoverySupervisor: Send + Sync {
    /// Choose a recovery for `fc`.
    fn recover(&self, fc: &FailureContext) -> Recovery;
}

/// The deterministic reference supervisor implementing A.4's rules.
#[derive(Debug, Default, Clone, Copy)]
pub struct SelfHealing;

impl SelfHealing {
    /// Build the supervisor.
    pub fn new() -> Self {
        Self
    }
}

impl RecoverySupervisor for SelfHealing {
    fn recover(&self, fc: &FailureContext) -> Recovery {
        let remaining = fc.retry_budget as i64 - fc.attempts as i64;
        // A security event is never auto-recovered: halt and escalate.
        if fc.kind == FailureKind::SecurityEvent {
            return Recovery {
                diagnosis: "failure indicates a security event".to_string(),
                recovery: RecoveryAction::Halt,
                target: None,
                max_attempts_remaining: 0,
                requires_human: true,
            };
        }

        // Any externally-visible recovery requires a human, regardless of kind.
        let requires_human = fc.externally_visible;

        let recovery = match fc.kind {
            FailureKind::SecurityEvent => unreachable!(),
            FailureKind::ModelDown => {
                if fc.allows(RecoveryAction::RerouteModel) {
                    RecoveryAction::RerouteModel
                } else {
                    RecoveryAction::Halt
                }
            }
            FailureKind::LoopDetected => {
                // Never keep retrying a loop.
                if fc.allows(RecoveryAction::Rollback) {
                    RecoveryAction::Rollback
                } else {
                    RecoveryAction::Halt
                }
            }
            FailureKind::Transient | FailureKind::ToolFailed => {
                // Retry only while budget remains and it is permitted; otherwise
                // reroute the tool, else roll back, else halt.
                if remaining > 0 && fc.allows(RecoveryAction::Retry) {
                    RecoveryAction::Retry
                } else if fc.allows(RecoveryAction::RerouteTool) {
                    RecoveryAction::RerouteTool
                } else if fc.allows(RecoveryAction::Rollback) {
                    RecoveryAction::Rollback
                } else {
                    RecoveryAction::Halt
                }
            }
        };

        let diagnosis = match fc.kind {
            FailureKind::ModelDown => "inference backend unavailable",
            FailureKind::LoopDetected => "repeated identical failure; not retrying",
            FailureKind::Transient => "transient error; bounded retry",
            FailureKind::ToolFailed => "tool call failed; bounded recovery",
            FailureKind::SecurityEvent => unreachable!(),
        }
        .to_string();

        Recovery {
            diagnosis,
            recovery,
            target: None,
            max_attempts_remaining: remaining.max(0),
            requires_human,
        }
    }
}
