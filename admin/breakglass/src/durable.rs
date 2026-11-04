//! Durable, KMS-sealed break-glass record. Every grant, early close, expiry alert
//! and post-hoc review is appended to a [`SealedJournal`] (encrypted + `fsync`ed at
//! rest) and replayed into a [`BreakGlassService`] on boot — so the
//! emergency-elevation record, a legal/forensic backbone, survives a restart.
//! [FR-AD-8] [RC-14]
//!
//! A record is the grant's whole current state, so a close is just the grant again
//! with its earlier `expires_at`. A record whose time-box is longer than
//! [`MAX_TTL_SECS`](crate::MAX_TTL_SECS) is refused on boot, as a malformed one is: the store does not
//! open, rather than restore a grant the service could never have issued.
//! [VLD-ANB-MIDGARD-0055]

use admin_identity::{AdminSession, IdentityDirectory};
use audit_writer::{SealedJournal, Sealer};

use crate::alert::Alerter;
use crate::error::BreakGlassError;
use crate::grant::{BreakGlassGrant, GrantRequest, Review};
use crate::service::{BreakGlassService, GrantStore};

const DOMAIN: &[u8] = b"midgard-admin-breakglass-v1";

/// A break-glass service whose grants + reviews are durable and recovered on boot.
pub struct DurableBreakGlass<A: Alerter> {
    inner: BreakGlassService<A>,
    journal: SealedJournal,
}

impl<A: Alerter> DurableBreakGlass<A> {
    /// Open (creating if absent), replaying persisted grants/reviews.
    pub fn open(
        path: impl AsRef<std::path::Path>,
        sealer: Box<dyn Sealer>,
        alerter: A,
    ) -> Result<Self, BreakGlassError> {
        let (journal, records) = SealedJournal::open(path, sealer, DOMAIN)
            .map_err(|e| BreakGlassError::Io(e.to_string()))?;
        let mut inner = BreakGlassService::new(alerter);
        for r in &records {
            // Last write per id wins (a grant then its later reviewed form).
            // A record past the four-hour cap fails the open, as a malformed one does.
            inner.restore_grant(decode_grant(r)?)?;
        }
        Ok(Self { inner, journal })
    }

    /// Open a break-glass grant (alerts), then durably persist it — including the
    /// authenticated approver. See [`BreakGlassService::grant`] for the two-person
    /// rule and the role check against `dir`. [MG-SEC-07]
    pub fn grant(
        &mut self,
        dir: &IdentityDirectory,
        req: GrantRequest,
        requester: &AdminSession,
        approver: &AdminSession,
        now: u64,
        approval_max_age: u64,
    ) -> Result<&BreakGlassGrant, BreakGlassError> {
        let id = req.id.clone();
        self.inner
            .grant(dir, req, requester, approver, now, approval_max_age)?;
        let g = self.inner.get(&id).expect("just granted").clone();
        self.journal
            .append(&encode_grant(&g))
            .map_err(|e| BreakGlassError::Io(e.to_string()))?;
        Ok(self.inner.get(&id).expect("granted"))
    }

    /// Record the mandatory post-hoc review, then durably persist it.
    pub fn review(
        &mut self,
        id: &str,
        reviewer: &str,
        note: &str,
        now: u64,
    ) -> Result<&BreakGlassGrant, BreakGlassError> {
        let before = self.inner.get(id).cloned();
        self.inner.review(id, reviewer, note, now)?;
        if let Err(e) = self.persist(id) {
            // Not recorded, so not reviewed: the grant goes back to how it was, and
            // the review can be retried rather than refused as already done.
            // [VLD-ANB-MIDGARD-0055]
            if let Some(before) = before {
                self.inner.restore_grant(before)?;
            }
            return Err(e);
        }
        Ok(self.inner.get(id).expect("reviewed"))
    }

    /// End a grant early (its time-box is cut to `now`), then durably persist it, so
    /// a closed grant is still closed after a restart. The record is the grant's
    /// usual one: its `expires_at` is simply the earlier time.
    ///
    /// Closing a grant already closed at or before `now` changes nothing and writes
    /// its record again, so a close whose write failed is retried by closing again.
    /// The grant stays closed in memory meanwhile, which fails closed.
    /// [VLD-ANB-MIDGARD-0055]
    pub fn close(&mut self, id: &str, now: u64) -> Result<&BreakGlassGrant, BreakGlassError> {
        self.inner.close(id, now)?;
        self.persist(id)?;
        Ok(self.inner.get(id).expect("closed"))
    }

    /// Sweep for newly expired grants (alerting once each), then durably persist
    /// each, so the one-time expiry alert is not raised again after a restart.
    pub fn sweep(&mut self, now: u64) -> Result<Vec<String>, BreakGlassError> {
        let expired = self.inner.sweep(now);
        for id in &expired {
            self.persist(id)?;
        }
        Ok(expired)
    }

    /// The recovered service (reads: `state`, `is_active`, `get`).
    pub fn service(&self) -> &BreakGlassService<A> {
        &self.inner
    }

    /// Mutable access to the alert sink (e.g. to flush a durable alerter).
    pub fn alerter_mut(&mut self) -> &mut A {
        self.inner.alerter_mut()
    }

    fn persist(&mut self, id: &str) -> Result<(), BreakGlassError> {
        let g = self.inner.get(id).expect("known grant");
        self.journal
            .append(&encode_grant(g))
            .map_err(|e| BreakGlassError::Io(e.to_string()))
    }
}

impl<A: Alerter> GrantStore<A> for DurableBreakGlass<A> {
    fn grant(
        &mut self,
        dir: &IdentityDirectory,
        req: GrantRequest,
        requester: &AdminSession,
        approver: &AdminSession,
        now: u64,
        approval_max_age: u64,
    ) -> Result<&BreakGlassGrant, BreakGlassError> {
        DurableBreakGlass::grant(self, dir, req, requester, approver, now, approval_max_age)
    }

    fn sweep(&mut self, now: u64) -> Result<Vec<String>, BreakGlassError> {
        DurableBreakGlass::sweep(self, now)
    }

    fn review(
        &mut self,
        id: &str,
        reviewer: &str,
        note: &str,
        now: u64,
    ) -> Result<&BreakGlassGrant, BreakGlassError> {
        DurableBreakGlass::review(self, id, reviewer, note, now)
    }

    fn close(&mut self, id: &str, now: u64) -> Result<&BreakGlassGrant, BreakGlassError> {
        DurableBreakGlass::close(self, id, now)
    }

    fn service(&self) -> &BreakGlassService<A> {
        &self.inner
    }

    fn alerter_mut(&mut self) -> &mut A {
        DurableBreakGlass::alerter_mut(self)
    }
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

fn encode_grant(g: &BreakGlassGrant) -> Vec<u8> {
    let mut o = Vec::new();
    put_str(&mut o, &g.id);
    put_str(&mut o, &g.principal);
    put_str(&mut o, &g.approver);
    put_str(&mut o, &g.scope);
    put_str(&mut o, &g.justification);
    o.extend_from_slice(&g.granted_at.to_le_bytes());
    o.extend_from_slice(&g.expires_at.to_le_bytes());
    match &g.review {
        Some(rev) => {
            o.push(1);
            put_str(&mut o, &rev.reviewer);
            o.extend_from_slice(&rev.at.to_le_bytes());
            put_str(&mut o, &rev.note);
        }
        None => o.push(0),
    }
    o.push(g.expiry_alerted as u8);
    o
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BreakGlassError> {
        let e = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| BreakGlassError::Io("truncated grant record".into()))?;
        let s = &self.b[self.p..e];
        self.p = e;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, BreakGlassError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<usize, BreakGlassError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn u64(&mut self) -> Result<u64, BreakGlassError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn s(&mut self) -> Result<String, BreakGlassError> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| BreakGlassError::Io("grant record invalid utf8".into()))
    }
}

fn decode_grant(b: &[u8]) -> Result<BreakGlassGrant, BreakGlassError> {
    let mut r = R { b, p: 0 };
    let id = r.s()?;
    let principal = r.s()?;
    let approver = r.s()?;
    let scope = r.s()?;
    let justification = r.s()?;
    let granted_at = r.u64()?;
    let expires_at = r.u64()?;
    let review = match r.u8()? {
        0 => None,
        1 => Some(Review {
            reviewer: r.s()?,
            at: r.u64()?,
            note: r.s()?,
        }),
        _ => return Err(BreakGlassError::Io("bad review tag".into())),
    };
    let expiry_alerted = r.u8()? != 0;
    if r.p != b.len() {
        return Err(BreakGlassError::Io("grant record trailing bytes".into()));
    }
    Ok(BreakGlassGrant {
        id,
        principal,
        approver,
        scope,
        justification,
        granted_at,
        expires_at,
        review,
        expiry_alerted,
    })
}
