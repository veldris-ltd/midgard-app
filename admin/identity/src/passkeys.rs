//! Durable, KMS-sealed registry of admin passkeys. Each registration — an admin id and
//! the **public** half of their passkey — is appended to a [`SealedJournal`] and
//! replayed into an [`AdminAuthenticator`] on boot, so the admins who can sign in, and
//! the credentials that prove it, survive a restart. The secret half never comes here:
//! it stays with the admin. [RC-30] [RC-14]

use audit_writer::{SealedJournal, Sealer};
use pqc_agility::{CryptoProvider, SigId, VerifyingKey};

use crate::binding::AdminId;
use crate::error::IdentityError;
use crate::session::AdminAuthenticator;
use crate::webauthn::{RegisteredPasskey, PASSKEY_SIG_ALG};

const DOMAIN: &[u8] = b"midgard-admin-passkeys-v1";

/// An admin authenticator whose passkey registry is durable and recovered on boot.
pub struct DurableAdminPasskeys {
    authenticator: AdminAuthenticator,
    admins: Vec<AdminId>,
    journal: SealedJournal,
}

impl DurableAdminPasskeys {
    /// Open (creating if absent), replaying every registered passkey.
    pub fn open(
        path: impl AsRef<std::path::Path>,
        sealer: Box<dyn Sealer>,
        provider: CryptoProvider,
    ) -> Result<Self, IdentityError> {
        let (journal, records) = SealedJournal::open(path, sealer, DOMAIN)
            .map_err(|e| IdentityError::Io(e.to_string()))?;
        let mut authenticator = AdminAuthenticator::new(provider);
        let mut admins = Vec::new();
        for record in &records {
            let (admin, passkey) = decode(record)?;
            if !admins.contains(&admin) {
                admins.push(admin.clone());
            }
            authenticator.register(admin, passkey);
        }
        Ok(Self {
            authenticator,
            admins,
            journal,
        })
    }

    /// Register an admin's passkey, durably. A passkey of any algorithm but
    /// [`PASSKEY_SIG_ALG`] is refused. An admin who is already registered is refused
    /// too: replacing a credential is a rotation, not a registration.
    pub fn register(
        &mut self,
        admin: AdminId,
        passkey: RegisteredPasskey,
    ) -> Result<(), IdentityError> {
        if passkey.public_key.alg() != PASSKEY_SIG_ALG {
            return Err(IdentityError::WebAuthnRejected);
        }
        if self.admins.contains(&admin) {
            return Err(IdentityError::Io(format!(
                "admin {} already has a registered passkey",
                admin.0
            )));
        }
        self.journal
            .append(&encode(&admin, &passkey))
            .map_err(|e| IdentityError::Io(e.to_string()))?;
        self.admins.push(admin.clone());
        self.authenticator.register(admin, passkey);
        Ok(())
    }

    /// Whether `admin` has a registered passkey.
    pub fn is_registered(&self, admin: &AdminId) -> bool {
        self.admins.contains(admin)
    }

    /// The registered admins, in registration order.
    pub fn admins(&self) -> &[AdminId] {
        &self.admins
    }

    /// The authenticator over every registered passkey — the only way to obtain an
    /// [`AdminSession`](crate::AdminSession).
    pub fn authenticator(&self) -> &AdminAuthenticator {
        &self.authenticator
    }
}

fn put(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

fn encode(admin: &AdminId, passkey: &RegisteredPasskey) -> Vec<u8> {
    let mut o = Vec::new();
    put(&mut o, admin.0.as_bytes());
    put(&mut o, passkey.credential_id.as_bytes());
    o.push(passkey.public_key.alg().as_u8());
    put(&mut o, passkey.public_key.as_bytes());
    o
}

fn bad(what: &str) -> IdentityError {
    IdentityError::Io(format!("passkey record: {what}"))
}

/// A bounds-checked reader over one record.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], IdentityError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.b.len())
            .ok_or_else(|| bad("truncated"))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn field(&mut self) -> Result<&'a [u8], IdentityError> {
        let len = self.take(4)?;
        let n = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
        self.take(n)
    }

    fn text(&mut self, what: &str) -> Result<String, IdentityError> {
        String::from_utf8(self.field()?.to_vec()).map_err(|_| bad(what))
    }
}

fn decode(b: &[u8]) -> Result<(AdminId, RegisteredPasskey), IdentityError> {
    let mut c = Cursor { b, at: 0 };
    let admin = c.text("admin not UTF-8")?;
    let credential_id = c.text("credential id not UTF-8")?;
    let alg_tag = c.take(1)?[0];
    let alg = SigId::from_u8(alg_tag).ok_or_else(|| bad("unknown algorithm"))?;
    let public_key = VerifyingKey::new(alg, c.field()?.to_vec());
    if c.at != b.len() {
        return Err(bad("trailing bytes"));
    }
    Ok((
        AdminId(admin),
        RegisteredPasskey {
            credential_id,
            public_key,
        },
    ))
}
