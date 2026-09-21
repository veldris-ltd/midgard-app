//! An access probe: one concrete (subject, resource, action, purpose) question the
//! dry-run evaluates under both the current and the proposed policy.

use policy::{AccessRequest, Action, Purpose, Resource, Subject};

/// A labelled access question. The label identifies the probe in the report (e.g.
/// "analyst reads MARKED report"); the rest is a full policy access request.
#[derive(Debug, Clone)]
pub struct AccessProbe {
    /// A human-readable label used in the [`crate::report::SimulationReport`].
    pub label: String,
    /// The requesting subject.
    pub subject: Subject,
    /// The target resource.
    pub resource: Resource,
    /// The action attempted.
    pub action: Action,
    /// The declared purpose.
    pub purpose: Purpose,
}

impl AccessProbe {
    /// Build a probe.
    pub fn new(
        label: impl Into<String>,
        subject: Subject,
        resource: Resource,
        action: Action,
        purpose: Purpose,
    ) -> Self {
        Self {
            label: label.into(),
            subject,
            resource,
            action,
            purpose,
        }
    }

    /// Borrow this probe as a policy [`AccessRequest`].
    pub fn as_request(&self) -> AccessRequest<'_> {
        AccessRequest {
            subject: &self.subject,
            resource: &self.resource,
            action: self.action.clone(),
            purpose: self.purpose.clone(),
        }
    }
}
