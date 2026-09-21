//! Action classes and their risk levels.
//!
//! Every administrative action belongs to a class, and every class carries a fixed
//! risk level 1..=5. The five high-risk classes named in FR-AD-5 — key rotation,
//! privilege escalation, model revocation, fleet-wide release, break-glass — plus
//! a change to the panel's own privilege logic (AC-AD-13) all sit at risk ≥ 4,
//! which is exactly the threshold at which [`crate::control::controls_for`]
//! demands a second distinct approver. `[FR-AD-5]` `[KDD-2]`

use crate::error::PrivError;

/// A risk level in the closed range 1..=5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RiskLevel(u8);

impl RiskLevel {
    /// Build a risk level, rejecting anything outside 1..=5.
    pub fn new(level: u8) -> Result<Self, PrivError> {
        if (1..=5).contains(&level) {
            Ok(Self(level))
        } else {
            Err(PrivError::InvalidRisk(level))
        }
    }

    /// The numeric level (1..=5).
    pub const fn level(self) -> u8 {
        self.0
    }
}

/// A class of administrative action. The class alone determines the risk level
/// and the capability required to *request* it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionClass {
    /// Read a dashboard / observability view.
    ReadDashboard,
    /// Edit a non-privileged config label or description.
    ConfigLabelEdit,
    /// Grant a narrow, scoped, expiring role binding.
    ScopedGrant,
    /// Deprecate (not revoke) a model.
    ModelDeprecate,
    /// Rotate a cryptographic key. **High-risk.** `[FR-AD-5]`
    KeyRotation,
    /// Escalate a privilege / grant a broad or powerful binding. **High-risk.**
    /// `[FR-AD-5]`
    PrivilegeEscalation,
    /// Revoke a model from the registry. **High-risk.** `[FR-AD-5]`
    ModelRevocation,
    /// A fleet-wide release. **High-risk.** `[FR-AD-5]`
    FleetWideRelease,
    /// Emergency break-glass elevation. **High-risk.** `[FR-AD-5]`
    BreakGlass,
    /// A change to the panel's own privilege logic. **High-risk**, and requires a
    /// security-officer approver. `[AC-AD-13]`
    PanelPrivilegeChange,
    /// Enrol a new principal who can log in to the enclosure. **High-risk**: it
    /// creates access, so a second, distinct IAM administrator must approve it.
    /// [RC-29]
    PrincipalEnroll,
}

impl ActionClass {
    /// The fixed risk level for this class.
    pub fn risk(self) -> RiskLevel {
        let l = match self {
            ActionClass::ReadDashboard => 1,
            ActionClass::ConfigLabelEdit => 2,
            ActionClass::ScopedGrant => 3,
            ActionClass::ModelDeprecate => 3,
            ActionClass::KeyRotation => 4,
            ActionClass::PrivilegeEscalation => 4,
            ActionClass::ModelRevocation => 4,
            ActionClass::FleetWideRelease => 5,
            ActionClass::BreakGlass => 5,
            ActionClass::PanelPrivilegeChange => 5,
            ActionClass::PrincipalEnroll => 4,
        };
        RiskLevel(l)
    }

    /// The capability an admin must hold to *request* this action class.
    pub fn required_capability(self) -> &'static str {
        match self {
            ActionClass::ReadDashboard => "config.read",
            ActionClass::ConfigLabelEdit => "config.write",
            ActionClass::ScopedGrant => "iam.binding.write",
            ActionClass::ModelDeprecate => "mimir.model.deprecate",
            ActionClass::KeyRotation => "kms.key.rotate",
            ActionClass::PrivilegeEscalation => "iam.binding.write",
            ActionClass::ModelRevocation => "mimir.model.revoke",
            ActionClass::FleetWideRelease => "bifrost.release.fleet",
            ActionClass::BreakGlass => "breakglass.request",
            ActionClass::PanelPrivilegeChange => "panel.privilege.write",
            ActionClass::PrincipalEnroll => "iam.user.write",
        }
    }

    /// A stable name for audit records.
    pub fn name(self) -> &'static str {
        match self {
            ActionClass::ReadDashboard => "read-dashboard",
            ActionClass::ConfigLabelEdit => "config-label-edit",
            ActionClass::ScopedGrant => "scoped-grant",
            ActionClass::ModelDeprecate => "model-deprecate",
            ActionClass::KeyRotation => "key-rotation",
            ActionClass::PrivilegeEscalation => "privilege-escalation",
            ActionClass::ModelRevocation => "model-revocation",
            ActionClass::FleetWideRelease => "fleet-wide-release",
            ActionClass::BreakGlass => "break-glass",
            ActionClass::PanelPrivilegeChange => "panel-privilege-change",
            ActionClass::PrincipalEnroll => "principal-enroll",
        }
    }

    /// Whether this class is high-risk (risk ≥ 4) and therefore requires a second
    /// distinct approver. `[FR-AD-5]`
    pub fn is_high_risk(self) -> bool {
        self.risk().level() >= 4
    }
}
