//! The break-glass grant, its request, and its lifecycle state.

/// A request to open a break-glass grant. It names no approver: the approver is the
/// administrator whose passkey-authenticated session approves it. `[MG-SEC-07]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRequest {
    /// Stable grant id.
    pub id: String,
    /// The principal being elevated — the requester, whose authenticated session
    /// must be this principal's.
    pub principal: String,
    /// The scope the elevation applies to.
    pub scope: String,
    /// Mandatory written justification. `[FR-AD-8]`
    pub justification: String,
    /// Time-box duration in seconds; must be non-zero. `[FR-AD-8]`
    pub ttl_secs: u64,
}

/// The mandatory post-hoc review of a break-glass grant. `[FR-AD-8]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// Who reviewed it.
    pub reviewer: String,
    /// When (Unix seconds).
    pub at: u64,
    /// The reviewer's note.
    pub note: String,
}

/// An open (or historical) break-glass grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakGlassGrant {
    /// Stable grant id.
    pub id: String,
    /// The elevated principal: the authenticated requester.
    pub principal: String,
    /// The approver: the authenticated identity of the second administrator, taken
    /// from their passkey session, never from the request. `[MG-SEC-07]`
    pub approver: String,
    /// The scope.
    pub scope: String,
    /// The justification.
    pub justification: String,
    /// When it was granted (Unix seconds).
    pub granted_at: u64,
    /// When it expires (Unix seconds, exclusive) — the time-box. `[FR-AD-8]`
    pub expires_at: u64,
    /// The post-hoc review, once performed.
    pub review: Option<Review>,
    /// Whether an expiry alert has already been raised (so it fires once).
    pub(crate) expiry_alerted: bool,
}

/// Where a grant sits in its lifecycle at a given time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakGlassState {
    /// Within its time-box and not yet reviewed.
    Active,
    /// Past its time-box, still awaiting the mandatory review.
    Expired,
    /// Reviewed (terminal).
    Reviewed,
}

impl BreakGlassGrant {
    /// The grant's state at `now`.
    pub fn state(&self, now: u64) -> BreakGlassState {
        if self.review.is_some() {
            BreakGlassState::Reviewed
        } else if now >= self.expires_at {
            BreakGlassState::Expired
        } else {
            BreakGlassState::Active
        }
    }

    /// Whether the grant currently confers access (within its time-box).
    pub fn is_active(&self, now: u64) -> bool {
        self.review.is_none() && now < self.expires_at
    }
}
