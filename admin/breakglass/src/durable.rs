//! Durable, KMS-sealed break-glass record. Every grant and post-hoc review is
//! appended to a [`SealedJournal`] (encrypted + `fsync`ed at rest) and replayed
//! into a [`BreakGlassService`] on boot — so the emergency-elevation record, a
//! legal/forensic backbone, survives a restart. [FR-AD-8] [RC-14]

use admin_identity::AdminSession;
use audit_writer::{SealedJournal, Sealer};

use crate::alert::Alerter;
use crate::error::BreakGlassError;
use crate::grant::{BreakGlassGrant, GrantRequest, Review};
use crate::service::BreakGlassService;

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
            inner.restore_grant(decode_grant(r)?);
        }
        Ok(Self { inner, journal })
    }

    /// Open a break-glass grant (alerts), then durably persist it — including the
    /// authenticated approver. See [`BreakGlassService::grant`] for the two-person
    /// rule. [MG-SEC-07]
    pub fn grant(
        &mut self,
        req: GrantRequest,
        requester: &AdminSession,
        approver: &AdminSession,
        now: u64,
        approval_max_age: u64,
    ) -> Result<&BreakGlassGrant, BreakGlassError> {
        let id = req.id.clone();
        self.inner
            .grant(req, requester, approver, now, approval_max_age)?;
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
        self.inner.review(id, reviewer, note, now)?;
        let g = self.inner.get(id).expect("just reviewed").clone();
        self.journal
            .append(&encode_grant(&g))
            .map_err(|e| BreakGlassError::Io(e.to_string()))?;
        Ok(self.inner.get(id).expect("reviewed"))
    }

    /// The recovered service (reads: `state`, `is_active`, `get`).
    pub fn service(&self) -> &BreakGlassService<A> {
        &self.inner
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
