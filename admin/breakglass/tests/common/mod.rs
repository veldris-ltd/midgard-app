//! Passkey-enrolled administrators for the break-glass tests: every session here
//! comes from a verified WebAuthn assertion, as it does in the panel. [MG-SEC-07]
#![allow(dead_code)]

use admin_identity::{
    AdminAuthenticator, AdminChallenge, AdminId, AdminSession, IdentityError,
    SoftwareAdminAuthenticator,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

/// How fresh the approver's passkey re-authentication must be, in seconds.
pub const APPROVAL_MAX_AGE: u64 = 300;

pub struct SeedRng(pub u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

/// The panel's authenticator plus each administrator's own passkey.
pub struct Admins {
    pub auth: AdminAuthenticator,
    passkeys: Vec<(String, SoftwareAdminAuthenticator)>,
    rng: SeedRng,
}

impl Admins {
    /// Enrol a passkey for each name and register it with the authenticator.
    pub fn new(names: &[&str]) -> Self {
        let provider = fips_provider();
        let mut rng = SeedRng(77);
        let mut auth = AdminAuthenticator::new(fips_provider());
        let mut passkeys = Vec::new();
        for name in names {
            let key = SoftwareAdminAuthenticator::enrol(&provider, name, &mut rng).unwrap();
            auth.register(AdminId((*name).into()), key.credential().clone());
            passkeys.push(((*name).to_string(), key));
        }
        Self {
            auth,
            passkeys,
            rng,
        }
    }

    fn passkey(&self, name: &str) -> &SoftwareAdminAuthenticator {
        &self
            .passkeys
            .iter()
            .find(|(n, _)| n == name)
            .expect("an enrolled administrator")
            .1
    }

    /// Sign in as `claimed`, answering the challenge with `key_of`'s passkey and the
    /// given user-verification flag. Only a matching, verified assertion yields a
    /// session.
    pub fn try_sign_in(
        &mut self,
        claimed: &str,
        key_of: &str,
        user_verified: bool,
        now: u64,
    ) -> Result<AdminSession, IdentityError> {
        let challenge = AdminChallenge::sample(&mut self.rng);
        let assertion =
            self.passkey(key_of)
                .authenticate(self.auth.provider(), &challenge, user_verified)?;
        self.auth
            .sign_in(&AdminId(claimed.into()), &challenge, &assertion, now)
    }

    /// `name` signs in with their own passkey at `now`.
    pub fn sign_in(&mut self, name: &str, now: u64) -> AdminSession {
        self.try_sign_in(name, name, true, now)
            .expect("an administrator signs in with their own passkey")
    }
}
