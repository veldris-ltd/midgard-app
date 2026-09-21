//! Durable, KMS-sealed identity directory. Each role [`RoleBinding`] is appended to
//! a [`SealedJournal`] (encrypted + `fsync`ed at rest) and replayed into an
//! [`IdentityDirectory`] on boot, so scoped/expiring role bindings survive a
//! restart. The role catalog is static configuration supplied at open. [FR-AD-2]
//! [RC-14]

use audit_writer::{SealedJournal, Sealer};

use crate::binding::{AdminId, ResourceScope, RoleBinding, Subsystem};
use crate::directory::IdentityDirectory;
use crate::error::IdentityError;
use crate::role::RoleCatalog;

const DOMAIN: &[u8] = b"midgard-admin-identity-v1";

/// An identity directory whose role bindings are durable and recovered on boot.
pub struct DurableIdentityDirectory {
    inner: IdentityDirectory,
    journal: SealedJournal,
}

impl DurableIdentityDirectory {
    /// Open (creating if absent), replaying persisted bindings over `catalog`.
    pub fn open(
        path: impl AsRef<std::path::Path>,
        sealer: Box<dyn Sealer>,
        catalog: RoleCatalog,
    ) -> Result<Self, IdentityError> {
        let (journal, records) = SealedJournal::open(path, sealer, DOMAIN)
            .map_err(|e| IdentityError::Io(e.to_string()))?;
        let mut inner = IdentityDirectory::new(catalog);
        for r in &records {
            inner.bind(decode_binding(r)?);
        }
        Ok(Self { inner, journal })
    }

    /// Grant a role binding (scoped, expiring), durably.
    pub fn bind(&mut self, binding: RoleBinding) -> Result<(), IdentityError> {
        self.journal
            .append(&encode_binding(&binding))
            .map_err(|e| IdentityError::Io(e.to_string()))?;
        self.inner.bind(binding);
        Ok(())
    }

    /// The recovered directory (all query methods: `has_capability`, `active_roles`,
    /// `risk_ceiling`, …).
    pub fn directory(&self) -> &IdentityDirectory {
        &self.inner
    }
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

fn encode_binding(b: &RoleBinding) -> Vec<u8> {
    let mut o = Vec::new();
    put_str(&mut o, &b.admin.0);
    put_str(&mut o, &b.role);
    put_str(&mut o, &b.scope.subsystem.0);
    put_str(&mut o, &b.scope.selector);
    o.extend_from_slice(&b.not_before.to_le_bytes());
    o.extend_from_slice(&b.not_after.to_le_bytes());
    o
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], IdentityError> {
        let e = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| IdentityError::Io("truncated binding record".into()))?;
        let s = &self.b[self.p..e];
        self.p = e;
        Ok(s)
    }
    fn u32(&mut self) -> Result<usize, IdentityError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn u64(&mut self) -> Result<u64, IdentityError> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }
    fn s(&mut self) -> Result<String, IdentityError> {
        let n = self.u32()?;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| IdentityError::Io("binding record invalid utf8".into()))
    }
}

fn decode_binding(b: &[u8]) -> Result<RoleBinding, IdentityError> {
    let mut r = R { b, p: 0 };
    let admin = AdminId(r.s()?);
    let role = r.s()?;
    let subsystem = Subsystem(r.s()?);
    let selector = r.s()?;
    let not_before = r.u64()?;
    let not_after = r.u64()?;
    if r.p != b.len() {
        return Err(IdentityError::Io("binding record trailing bytes".into()));
    }
    Ok(RoleBinding {
        admin,
        role,
        scope: ResourceScope {
            subsystem,
            selector,
        },
        not_before,
        not_after,
    })
}
