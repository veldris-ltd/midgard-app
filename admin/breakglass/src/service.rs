//! The break-glass service: grant → (active, time-boxed) → expire → forced review.

use std::collections::BTreeMap;

use admin_identity::AdminSession;

use crate::alert::{AlertKind, Alerter, BreakGlassAlert};
use crate::error::BreakGlassError;
use crate::grant::{BreakGlassGrant, BreakGlassState, GrantRequest, Review};

/// Manages break-glass grants over an injected clock and alert sink.
pub struct BreakGlassService<A: Alerter> {
    grants: BTreeMap<String, BreakGlassGrant>,
    alerter: A,
}

impl<A: Alerter> BreakGlassService<A> {
    /// Build a service over an alert sink.
    pub fn new(alerter: A) -> Self {
        Self {
            grants: BTreeMap::new(),
            alerter,
        }
    }

    /// Open a break-glass grant. Requires a justification and a non-zero time-box;
    /// raises a real-time `Granted` alert. `[FR-AD-8]`
    ///
    /// Two people, both authenticated: `requester` must be the session of the
    /// principal being elevated, and `approver` the session of a *different*
    /// administrator whose passkey re-authentication is within `approval_max_age`
    /// seconds of `now`. Sessions exist only after a verified passkey assertion
    /// ([`admin_identity::AdminAuthenticator::sign_in`]), so the identities compared
    /// and recorded are authenticated ones, never names anyone typed. `[MG-SEC-07]`
    pub fn grant(
        &mut self,
        req: GrantRequest,
        requester: &AdminSession,
        approver: &AdminSession,
        now: u64,
        approval_max_age: u64,
    ) -> Result<&BreakGlassGrant, BreakGlassError> {
        if requester.admin().0 != req.principal {
            return Err(BreakGlassError::SessionMismatch);
        }
        if !requester.has_mfa() || !approver.has_mfa() {
            return Err(BreakGlassError::MfaRequired);
        }
        if approver.admin() == requester.admin() {
            return Err(BreakGlassError::SelfApprovalDenied);
        }
        if !approver.has_fresh_reauth(now, approval_max_age) {
            return Err(BreakGlassError::StaleApproval);
        }
        if req.justification.trim().is_empty() {
            return Err(BreakGlassError::JustificationRequired);
        }
        if req.ttl_secs == 0 {
            return Err(BreakGlassError::InvalidTtl);
        }
        if self.grants.contains_key(&req.id) {
            return Err(BreakGlassError::DuplicateGrant(req.id));
        }

        let expires_at = now.saturating_add(req.ttl_secs);
        let grant = BreakGlassGrant {
            id: req.id.clone(),
            principal: req.principal.clone(),
            approver: approver.admin().0.clone(),
            scope: req.scope,
            justification: req.justification,
            granted_at: now,
            expires_at,
            review: None,
            expiry_alerted: false,
        };
        self.alerter.alert(BreakGlassAlert {
            kind: AlertKind::Granted,
            grant_id: req.id.clone(),
            principal: req.principal,
            detail: format!("break-glass granted until {expires_at}"),
        });
        self.grants.insert(req.id.clone(), grant);
        Ok(self.grants.get(&req.id).expect("just inserted"))
    }

    /// Rehydrate a previously-issued grant WITHOUT re-alerting — the durable store
    /// uses this to reload persisted grants on boot. [RC-14]
    pub fn restore_grant(&mut self, grant: BreakGlassGrant) {
        self.grants.insert(grant.id.clone(), grant);
    }

    /// The state of a grant at `now`.
    pub fn state(&self, id: &str, now: u64) -> Option<BreakGlassState> {
        self.grants.get(id).map(|g| g.state(now))
    }

    /// Whether a grant currently confers access.
    pub fn is_active(&self, id: &str, now: u64) -> bool {
        self.grants.get(id).is_some_and(|g| g.is_active(now))
    }

    /// A grant by id.
    pub fn get(&self, id: &str) -> Option<&BreakGlassGrant> {
        self.grants.get(id)
    }

    /// Sweep for grants that have passed their time-box without review, raising a
    /// one-time `ExpiredUnreviewed` alert for each. Returns the ids that now need a
    /// review. Idempotent: an already-alerted grant is not re-alerted. `[FR-AD-8]`
    pub fn sweep(&mut self, now: u64) -> Vec<String> {
        let mut newly = Vec::new();
        for g in self.grants.values_mut() {
            if g.state(now) == BreakGlassState::Expired && !g.expiry_alerted {
                g.expiry_alerted = true;
                self.alerter.alert(BreakGlassAlert {
                    kind: AlertKind::ExpiredUnreviewed,
                    grant_id: g.id.clone(),
                    principal: g.principal.clone(),
                    detail: "break-glass expired; post-hoc review required".to_string(),
                });
                newly.push(g.id.clone());
            }
        }
        newly
    }

    /// Every grant that has expired and not yet been reviewed — the outstanding
    /// review backlog the panel forces to be cleared. `[FR-AD-8]` `[AC-AD-8]`
    pub fn pending_reviews(&self, now: u64) -> Vec<&BreakGlassGrant> {
        self.grants
            .values()
            .filter(|g| g.state(now) == BreakGlassState::Expired)
            .collect()
    }

    /// Whether every grant that has ever been opened has been reviewed (nothing
    /// outstanding at `now`).
    pub fn all_reviewed(&self, now: u64) -> bool {
        self.pending_reviews(now).is_empty()
    }

    /// Record the mandatory post-hoc review of a grant. `[FR-AD-8]` `[AC-AD-8]`
    pub fn review(
        &mut self,
        id: &str,
        reviewer: &str,
        note: &str,
        now: u64,
    ) -> Result<&BreakGlassGrant, BreakGlassError> {
        let g = self
            .grants
            .get_mut(id)
            .ok_or_else(|| BreakGlassError::UnknownGrant(id.to_string()))?;
        if g.review.is_some() {
            return Err(BreakGlassError::AlreadyReviewed(id.to_string()));
        }
        g.review = Some(Review {
            reviewer: reviewer.to_string(),
            at: now,
            note: note.to_string(),
        });
        Ok(g)
    }

    /// The alert sink (for inspection).
    pub fn alerter(&self) -> &A {
        &self.alerter
    }

    /// Mutable access to the alert sink (e.g. to flush a durable alerter's retry
    /// queue). [RC-32]
    pub fn alerter_mut(&mut self) -> &mut A {
        &mut self.alerter
    }
}
