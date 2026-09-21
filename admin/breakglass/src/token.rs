//! The scoped, time-boxed **capability elevation** a break-glass grant confers, and
//! the authority the PDP / subsystems consult to honour it. A grant is not just an
//! alert and an audit line — it issues a [`CapabilityToken`] that genuinely elevates
//! the grantee's access within a scope, and that elevation **auto-expires** at the
//! grant's time-box. A subsystem checks [`BreakGlassAuthority::elevates`] alongside
//! its normal deny-by-default policy, so emergency access is real and bounded.
//! [FR-AD-8] [RC-32]

use std::collections::BTreeMap;

/// A scoped, time-boxed elevation issued on a break-glass grant. It confers one
/// capability within one scope, and is only live within `[granted_at, expires_at)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityToken {
    /// The grant this token belongs to.
    pub grant_id: String,
    /// The elevated principal.
    pub principal: String,
    /// The scope the elevation applies to (e.g. `kms`).
    pub scope: String,
    /// The capability emergency-granted within the scope (e.g. `kms.key.rotate`).
    pub capability: String,
    /// When the elevation began (Unix seconds).
    pub granted_at: u64,
    /// When the elevation expires (Unix seconds, exclusive) — the time-box.
    pub expires_at: u64,
}

impl CapabilityToken {
    /// Whether this token is live at `now`.
    pub fn is_live(&self, now: u64) -> bool {
        now >= self.granted_at && now < self.expires_at
    }

    /// Whether this token elevates `principal` for `(scope, capability)` at `now`.
    pub fn covers(&self, principal: &str, scope: &str, capability: &str, now: u64) -> bool {
        self.is_live(now)
            && self.principal == principal
            && self.scope == scope
            && self.capability == capability
    }
}

/// The registry of live break-glass elevations. The PDP / subsystems call
/// [`elevates`](BreakGlassAuthority::elevates) as an override on top of normal policy:
/// deny-by-default still applies, but an active break-glass token grants the named
/// capability within scope until it expires. [FR-AD-8] [RC-32]
#[derive(Debug, Default)]
pub struct BreakGlassAuthority {
    tokens: BTreeMap<String, CapabilityToken>,
}

impl BreakGlassAuthority {
    /// An empty authority.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a grant's capability token.
    pub fn register(&mut self, token: CapabilityToken) {
        self.tokens.insert(token.grant_id.clone(), token);
    }

    /// Drop a grant's token (e.g. after expiry sweep). Elevation also lapses by time
    /// even without this — `elevates` checks the clock.
    pub fn revoke(&mut self, grant_id: &str) {
        self.tokens.remove(grant_id);
    }

    /// **The gate the PDP / subsystems honour**: does an active break-glass token
    /// elevate `principal` for `(scope, capability)` at `now`? Auto-expires by time.
    /// [RC-32]
    pub fn elevates(&self, principal: &str, scope: &str, capability: &str, now: u64) -> bool {
        self.tokens
            .values()
            .any(|t| t.covers(principal, scope, capability, now))
    }

    /// A grant's token, if registered.
    pub fn token(&self, grant_id: &str) -> Option<&CapabilityToken> {
        self.tokens.get(grant_id)
    }

    /// The number of tokens still live at `now`.
    pub fn active_count(&self, now: u64) -> usize {
        self.tokens.values().filter(|t| t.is_live(now)).count()
    }
}
