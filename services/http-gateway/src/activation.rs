//! `POST /api/v1/activate` — an approved principal activates their account.
//! [RC-29] [NFR-SEC-1] [AC-15.3]
//!
//! Dual-control approval mints a principal's credentials on the gateway and hands the
//! approver a single-use activation code, which they pass to the principal out of
//! band. The principal presents it here with the password they choose: the code is
//! checked in constant time against its stored hash and before its expiry, the
//! password is hashed and stored, the code is spent, and the reply carries the TOTP
//! provisioning URI, so the principal and nobody else holds the second factor. Every
//! failure is one uniform 401 that counts towards the name's lockout, exactly like a
//! login, and every attempt is in the audit ledger. [VLD-ANB-MIDGARD-0002]

use fabric::{LedgerStore, ObjectStore};
use iam::{Directory, PasswordHasher};

use crate::http::{Request, Response};
use crate::json::{parse, Json};
use crate::strict_json::{object, only_keys, text};
use crate::{error_json, SecuredGateway};

/// The shortest password a principal may choose at activation, in characters.
pub const MIN_PASSWORD_CHARS: usize = 12;
/// The issuer named in the TOTP provisioning URI, as `mg-admin` names it.
const TOTP_ISSUER: &str = "Midgard";

impl<S: ObjectStore, L: LedgerStore> SecuredGateway<S, L> {
    pub(crate) fn handle_activate(&self, req: &Request, now: u64) -> Response {
        let at = "the activation";
        let fields = parse(&req.body)
            .map_err(|_| "malformed activation body".to_string())
            .and_then(|body| {
                only_keys(
                    object(&body, at)?,
                    at,
                    &["username", "activationCode", "password"],
                )?;
                Ok((
                    text(&body, at, "username")?.to_string(),
                    text(&body, at, "activationCode")?.to_string(),
                    text(&body, at, "password")?.to_string(),
                ))
            });
        let (username, code, password) = match fields {
            Ok(fields) => fields,
            Err(e) => return Response::json(400, error_json(&e).encode()),
        };
        // A password too short to keep is refused before any work is spent on it.
        if password.chars().count() < MIN_PASSWORD_CHARS {
            return Response::json(
                400,
                error_json(&format!(
                    "the password must be at least {MIN_PASSWORD_CHARS} characters"
                ))
                .encode(),
            );
        }

        let throttled = |seconds: u64| {
            Response::json(
                429,
                error_json(&format!(
                    "too many failed attempts: try again in {seconds} seconds"
                ))
                .encode(),
            )
            .with_retry_after(seconds)
        };
        // A locked name is refused before its code is checked. [NFR-SEC-1]
        let locked = self.throttle().retry_after(&username, now);
        if let Some(seconds) = locked {
            return self.record_login(&username, "ACTIVATE throttled", throttled(seconds), now);
        }

        let outcome = {
            let mut directory = self
                .directory
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let pending = directory
                .enrollment(&username)
                .and_then(|entry| Some((entry, entry.credentials.activation.as_ref()?)));
            match pending {
                Some((entry, activation))
                    if now < activation.expires_at
                        && self.hasher.verify(code.as_bytes(), &activation.code_hash) =>
                {
                    let mut credentials = entry.credentials.clone();
                    credentials.password_hash = self.hasher.hash(password.as_bytes());
                    credentials.activation = None; // single use
                    let uri = credentials
                        .totp
                        .as_ref()
                        .map(|totp| totp.provisioning_uri(TOTP_ISSUER, &username));
                    match directory.update_credentials(&username, credentials) {
                        Ok(true) => Ok(uri),
                        Ok(false) => Err(None),
                        Err(e) => Err(Some(e)),
                    }
                }
                _ => {
                    // An unknown name, no pending activation, an expired or wrong code:
                    // the same work and the same answer for each.
                    self.hasher.spend_verification(code.as_bytes());
                    Err(None)
                }
            }
        };

        let (event, response) = match outcome {
            Ok(uri) => {
                self.throttle().record_success(&username);
                let mut reply = vec![("activated", Json::s(&username))];
                if let Some(uri) = &uri {
                    reply.push(("otpauth", Json::s(uri)));
                }
                (
                    "ACTIVATE succeeded",
                    Response::json(200, Json::obj(reply).encode()),
                )
            }
            Err(Some(e)) => (
                "ACTIVATE failed",
                Response::json(
                    500,
                    error_json(&format!("the activation was not recorded: {e}")).encode(),
                ),
            ),
            Err(None) => match self.throttle().record_failure(&username, now) {
                Some(seconds) => ("ACTIVATE refused, locked", throttled(seconds)),
                None => (
                    "ACTIVATE refused",
                    Response::json(401, error_json("invalid activation").encode()),
                ),
            },
        };
        self.record_login(&username, event, response, now)
    }
}
