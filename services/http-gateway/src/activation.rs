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
//!
//! It is bounded and throttled as a login is: every field is capped before anything
//! keeps or hashes it, each source address draws on the same allowance as its logins,
//! and an attempt is reserved under the throttle's lock before its code is checked.
//! [VLD-ANB-MIDGARD-0005] [VLD-ANB-MIDGARD-0019]
//!
//! The code is checked and the password hashed on a snapshot of the pending
//! activation, without the directory lock, so that work never holds up logins or
//! enrolments; the lock is taken again to commit, and only while the same activation
//! is still pending and unexpired. [NFR-SEC-1]

use std::net::SocketAddr;

use fabric::{LedgerStore, ObjectStore};
use iam::{Directory, PasswordHasher};

use crate::http::{Request, Response};
use crate::json::{parse, Json};
use crate::login_throttle::{Outcome, Refusal, MAX_PASSWORD_BYTES, MAX_USERNAME_BYTES};
use crate::strict_json::{object, only_keys, text};
use crate::{error_json, source_refused, Attempt, SecuredGateway};

/// The shortest password a principal may choose at activation, in characters.
pub const MIN_PASSWORD_CHARS: usize = 12;
/// The issuer named in the TOTP provisioning URI, as `mg-admin` names it.
const TOTP_ISSUER: &str = "Midgard";
/// The longest activation code accepted, in bytes: twice the 32 hex characters an
/// approval mints. [VLD-ANB-MIDGARD-0005]
pub const MAX_ACTIVATION_CODE_BYTES: usize = 64;

impl<S: ObjectStore, L: LedgerStore> SecuredGateway<S, L> {
    pub(crate) fn handle_activate(
        &self,
        req: &Request,
        peer: Option<SocketAddr>,
        now: u64,
    ) -> Response {
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
        // Every field is bounded before the throttle, the directory or the ledger sees
        // it, as a login's are. [VLD-ANB-MIDGARD-0005]
        if username.len() > MAX_USERNAME_BYTES
            || username.chars().any(char::is_control)
            || code.len() > MAX_ACTIVATION_CODE_BYTES
            || password.len() > MAX_PASSWORD_BYTES
        {
            return Response::json(
                400,
                error_json("activation fields exceed their limits").encode(),
            );
        }
        // A source past its allowance is refused before any code is checked; it draws
        // on the same allowance as the address's logins. [VLD-ANB-MIDGARD-0005]
        if self.over_source_allowance(req, peer).is_some() {
            return source_refused();
        }
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
        // The attempt is reserved under the throttle's lock before its code is checked,
        // so attempts arriving together cannot all pass a check made before any of them
        // is recorded. A locked name is refused before its code is checked.
        // [NFR-SEC-1] [VLD-ANB-MIDGARD-0019]
        let begun = self.throttle().begin_attempt(&username, now);
        match begun {
            Ok(()) => {}
            Err(Refusal::Locked { retry_after }) => {
                return self.record_login(
                    &username,
                    "ACTIVATE throttled",
                    throttled(retry_after),
                    now,
                );
            }
            Err(Refusal::Busy { retry_after }) => {
                return Response::json(
                    429,
                    error_json("an activation for this name is already being checked").encode(),
                )
                .with_retry_after(retry_after);
            }
        }
        // Released however the check ends, a panic included, so a slot cannot leak.
        let mut attempt = Attempt {
            throttle: &self.login_throttle,
            username: &username,
            now,
            finished: false,
        };

        // The pending activation as it stands, taken under the directory lock; the code
        // is checked and the password hashed after it is released.
        let pending = self
            .directory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .enrollment(&username)
            .and_then(|entry| {
                let activation = entry.credentials.activation.clone()?;
                Some((entry.credentials.clone(), activation))
            });
        let checked = match pending {
            Some((credentials, activation))
                if now < activation.expires_at
                    && self.hasher.verify(code.as_bytes(), &activation.code_hash) =>
            {
                let mut credentials = credentials;
                credentials.password_hash = self.hasher.hash(password.as_bytes());
                credentials.activation = None; // single use
                Some((credentials, activation.code_hash))
            }
            _ => {
                // An unknown name, no pending activation, an expired or wrong code:
                // the same work and the same answer for each.
                self.hasher.spend_verification(code.as_bytes());
                None
            }
        };
        self.between_check_and_commit();
        let outcome = match checked {
            None => Err(None),
            Some((credentials, code_hash)) => {
                let mut directory = self
                    .directory
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                // Committed only while the activation checked is still the one pending
                // and unexpired; one spent or replaced meanwhile is the uniform 401.
                let still_pending = directory
                    .enrollment(&username)
                    .and_then(|entry| entry.credentials.activation.as_ref())
                    .is_some_and(|activation| {
                        activation.code_hash == code_hash && now < activation.expires_at
                    });
                if still_pending {
                    let uri = credentials
                        .totp
                        .as_ref()
                        .map(|totp| totp.provisioning_uri(TOTP_ISSUER, &username));
                    match directory.update_credentials(&username, credentials) {
                        Ok(true) => Ok(uri),
                        Ok(false) => Err(None),
                        Err(e) => Err(Some(e)),
                    }
                } else {
                    Err(None)
                }
            }
        };

        let (event, response) = match outcome {
            Ok(uri) => {
                attempt.finish_attempt(Outcome::Success);
                let mut reply = vec![("activated", Json::s(&username))];
                if let Some(uri) = &uri {
                    reply.push(("otpauth", Json::s(uri)));
                }
                (
                    "ACTIVATE succeeded",
                    Response::json(200, Json::obj(reply).encode()),
                )
            }
            Err(Some(e)) => {
                attempt.finish_attempt(Outcome::Error);
                (
                    "ACTIVATE failed",
                    Response::json(
                        500,
                        error_json(&format!("the activation was not recorded: {e}")).encode(),
                    ),
                )
            }
            Err(None) => match attempt.finish_attempt(Outcome::Failure) {
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
