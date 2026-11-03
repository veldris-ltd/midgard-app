//! # http-gateway — the browser-facing HTTP/JSON edge
//!
//! A runnable service that exposes the `GatewayClient` contract (see
//! `@midgard/sdk`) over plain HTTP/JSON so the console UIs can reach real data
//! from a real process. It is std-only — a thread-per-connection `TcpListener`,
//! a hand-rolled HTTP/1.1 parser, and a hand-rolled JSON codec — mirroring the
//! PQ-mTLS `gateway`'s lean-std transport and the repo's no-async/serde posture.
//!
//! In production this edge terminates the browser session and forwards to the
//! enclosure over PQ-mTLS. Its object-fabric and governance reads and writes go
//! through the real `fabric` and `governance` crates (see `enclosure.rs`); only the
//! static platform read models in `data.rs` remain fixed data. CORS is locked to an
//! origin allowlist. `[DD-6] [NFR-SEC-1]`

use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod admin_api;
pub mod boot;
pub mod cli;
pub mod data;
pub mod enclosure;
pub mod http;
pub mod import_file;
pub mod login_throttle;
pub mod mesh_sync;
pub mod ontology_file;
pub mod revocations;
pub mod state_dir;
/// The JSON codec and the policy file, shared with the mesh services.
pub use service_runtime::{json, policy_file, ListenerLimits, CONNECTION_STACK};
use service_runtime::{strict_json, BoundedListener};

use audit_writer::Sealer;
use data::State;
use enclosure::{EdgeError, Enclosure};
use fabric::{InMemoryLedger, InMemoryStore, LedgerStore, ObjectStore};
use gateway::RateLimiter;
use governance::{GovContext, Governance};
use http::{read_request_within, refuse_at_once, Incoming, Request, Response};
use iam::{
    verify_token, AuthError, Credentials, Directory, DurableDirectory, Enrollment, Idp,
    InMemoryDirectory, IssueError, MfaProof, PasswordHasher, PasswordUpgrade, Pbkdf2Hasher,
    TokenClaims, UpgradingAuthenticator, UserRecord,
};
use json::{parse, Json};
use login_throttle::{Outcome, Refusal};
use policy::{
    Action, ClassificationLevel, Clearance, Compartment, DefaultPdp, Effect, Marking, PolicySet,
    Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, OsRng, VerifyingKey};
use revocations::Revocations;
use std::time::Instant;
use telemetry::{AvailabilitySlo, HealthRegistry, MetricsRegistry, Toggle};

/// The availability objective the gateway is instrumented against (99.9%).
const AVAILABILITY_OBJECTIVE: f64 = 0.999;

/// Issued-token lifetime (seconds) minted by the `/api/v1/login` endpoint. [RC-37]
const TOKEN_TTL: u64 = 3_600;
/// The declared purpose stamped on a login-issued token.
const LOGIN_PURPOSE: &str = "operations";

/// The dev gateway's demo operator credentials (dev/offline only — production wires a
/// real IdP directory over the enclosure connection). [RC-37]
///
/// They stay compiled into every build but are reachable only through
/// [`SecuredGateway::dev`], which only `http-gateway --dev` starts, on a loopback
/// address and with no override. A gateway served from a state directory has no
/// built-in login. [MG-SEC-06] [VLD-ANB-MIDGARD-0032]
pub const DEMO_USERNAME: &str = "operator";
pub const DEMO_PASSWORD: &str = "midgard-demo";
/// The dev gateway's demo approver, the second person a human-gated action needs.
pub const DEMO_APPROVER_USERNAME: &str = "approver";
pub const DEMO_APPROVER_PASSWORD: &str = "midgard-approve";

/// Where logins are looked up: in memory for development, a durable KMS-sealed
/// directory for a deployment. Either way a login authenticates through
/// `iam::Authenticator` — password, then any enrolled second factor. [RC-29] [RC-37]
enum Logins {
    InMemory(InMemoryDirectory),
    Durable(DurableDirectory),
}

impl Directory for Logins {
    fn enrollment(&self, subject: &str) -> Option<&Enrollment> {
        match self {
            Logins::InMemory(directory) => directory.enrollment(subject),
            Logins::Durable(directory) => directory.enrollment(subject),
        }
    }

    fn replace_password_hash(
        &mut self,
        subject: &str,
        current: &str,
        new_hash: String,
    ) -> Result<bool, String> {
        match self {
            Logins::InMemory(directory) => {
                directory.replace_password_hash(subject, current, new_hash)
            }
            Logins::Durable(directory) => {
                directory.replace_password_hash(subject, current, new_hash)
            }
        }
    }
}

/// The **secured** HTTP/JSON edge: it verifies the PQ-signed IdP bearer token on
/// every request, authorises + audits each through the governance spine, and locks
/// CORS to a configured origin allowlist. Public health is the only unauthenticated
/// route. It is also **observable**: `/metrics` (Prometheus), `/api/v1/health`
/// (liveness), and `/api/v1/ready` (readiness), with request metrics and an
/// availability SLO whose breach is visible in a scrape — all in-enclosure. [RC-24]
/// [RC-33] [NFR-SEC-1] [NFR-PRV-1]
///
/// Generic over the enclosure's object store and audit ledger, both in memory by
/// default; a deployment serves durable, KMS-sealed ones. [AC-M-4] [RC-09]
pub struct SecuredGateway<S: ObjectStore = InMemoryStore, L: LedgerStore = InMemoryLedger> {
    state: State,
    provider: CryptoProvider,
    issuer_vk: VerifyingKey,
    /// The real object fabric, which owns the single governance spine: every edge
    /// admission decision and every fabric decision lands in one PQ-signed ledger.
    enclosure: Mutex<Enclosure<S, L>>,
    allowed_origins: Vec<String>,
    /// Object-type → resource classification, for read-up enforcement by the PDP.
    markings: BTreeMap<String, ClassificationLevel>,
    // ---- login / token issuance (RT-37) ----
    /// The IdP that mints login tokens — present only when this gateway is configured
    /// to terminate the browser session (`dev()` / a provisioned gateway). Its key is
    /// the same one `issuer_vk` verifies, so a minted token is accepted downstream.
    idp: Option<Idp>,
    /// Login principals: token attributes, password hash and any second factor. Behind
    /// a lock because an approved enrolment writes to it while logins read it.
    directory: Mutex<Logins>,
    /// Failed logins per username, and the lockouts they earn. [NFR-SEC-1]
    login_throttle: Mutex<login_throttle::LoginThrottle>,
    /// Login and administration attempts per source address, refused past their
    /// allowance before any credential is checked; `None` when turned off.
    /// [VLD-ANB-MIDGARD-0005]
    login_sources: Option<RateLimiter>,
    /// Token ids signed out before their expiry, refused on every request. In memory
    /// unless a state directory journals them. Locks are taken revocations, then
    /// directory, then enclosure. [VLD-ANB-MIDGARD-0021]
    revocations: Mutex<Revocations>,
    /// Administration — dual-controlled enrolment — where a durable directory is served.
    admin: Option<Mutex<admin_api::AdminControl>>,
    /// The ontology records synced over the mesh are checked against, where a state
    /// directory declares one. [FR-F-4]
    ontology: Option<ontology_file::Ontology>,
    /// The highest marking a mesh sync may write: every synced record's marking must be
    /// dominated by it. Configuration, not a certificate attribute. [FR-F-4]
    /// [VLD-ANB-MIDGARD-0001]
    sync_clearance: Clearance,
    /// Password hasher for login verification (Argon2id/PBKDF2 seam, RT-29).
    hasher: Pbkdf2Hasher,
    // ---- observability (in-enclosure; nothing egresses) [RC-33] ----
    metrics: Arc<MetricsRegistry>,
    health: HealthRegistry,
    readiness: Arc<Toggle>,
    availability: Mutex<AvailabilitySlo>,
}

impl<L: LedgerStore> SecuredGateway<InMemoryStore, L> {
    /// Build a secured edge over an in-memory object fabric. `issuer_vk` is the IdP's
    /// verifying key (the offline JWKS); `governance` runs the PDP and audits every
    /// decision.
    pub fn new(
        provider: CryptoProvider,
        issuer_vk: VerifyingKey,
        governance: Governance<L>,
        allowed_origins: Vec<String>,
    ) -> Self {
        Self::with_enclosure(
            provider,
            issuer_vk,
            Enclosure::new(governance),
            allowed_origins,
        )
    }
}

impl<S: ObjectStore, L: LedgerStore> SecuredGateway<S, L> {
    /// Build a secured edge over an explicit enclosure — the seam a deployment uses to
    /// serve a durable, KMS-sealed object store and audit ledger. [AC-M-4] [RC-09]
    pub fn with_enclosure(
        provider: CryptoProvider,
        issuer_vk: VerifyingKey,
        enclosure: Enclosure<S, L>,
        allowed_origins: Vec<String>,
    ) -> Self {
        let readiness = Arc::new(Toggle::new("gateway", true));
        let mut health = HealthRegistry::new();
        health.register(Box::new(readiness.clone()));
        let metrics = Arc::new(MetricsRegistry::new());
        metrics.describe(
            "http_requests_total",
            "HTTP requests by matched route template (path) and status",
        );
        metrics.describe(
            "http_request_latency_ms",
            "HTTP request latency in milliseconds by matched route template (path)",
        );
        metrics.describe(
            telemetry::DROPPED_SERIES,
            "metric series not recorded because their family was at its series cap",
        );
        Self {
            state: State::new(),
            provider,
            issuer_vk,
            enclosure: Mutex::new(enclosure),
            allowed_origins,
            markings: BTreeMap::new(),
            idp: None,
            directory: Mutex::new(Logins::InMemory(InMemoryDirectory::new())),
            login_throttle: Mutex::new(login_throttle::LoginThrottle::new()),
            login_sources: Some(RateLimiter::new(
                login_throttle::LOGIN_SOURCE_BURST,
                login_throttle::LOGIN_SOURCE_PER_SEC,
            )),
            revocations: Mutex::new(Revocations::default()),
            admin: None,
            ontology: None,
            // The highest band with no compartments: existing syncs keep working, and a
            // compartmented object needs an explicit grant.
            sync_clearance: Clearance {
                level: ClassificationLevel::TOP_SECRET,
                compartments: BTreeSet::new(),
            },
            hasher: Pbkdf2Hasher::production(),
            metrics,
            health,
            readiness,
            availability: Mutex::new(AvailabilitySlo::new(
                "gateway_availability",
                AVAILABILITY_OBJECTIVE,
            )),
        }
    }

    /// Enable the `/api/v1/login` endpoint on this gateway: hold the token-minting IdP
    /// (its key must be the one `issuer_vk` verifies) and the password hasher used to
    /// check credentials. Returns self for chaining. [RC-37]
    pub fn with_idp(mut self, idp: Idp, hasher: Pbkdf2Hasher) -> Self {
        self.idp = Some(idp);
        self.hasher = hasher;
        self
    }

    /// Allow each source address `burst` login and administration attempts at once,
    /// refilling at `per_sec` a second, in place of [`login_throttle::LOGIN_SOURCE_BURST`]
    /// and [`login_throttle::LOGIN_SOURCE_PER_SEC`]. A burst of 0 turns the per-source
    /// limit off. [VLD-ANB-MIDGARD-0005]
    pub fn with_login_source_limit(mut self, burst: f64, per_sec: f64) -> Self {
        self.login_sources = (burst > 0.0).then(|| RateLimiter::new(burst, per_sec));
        self
    }

    /// The failed-login throttle's size: the names it tracks and the bytes their keys
    /// hold. [VLD-ANB-MIDGARD-0005]
    pub fn login_throttle_stats(&self) -> (usize, usize) {
        let throttle = self.throttle();
        (throttle.tracked(), throttle.key_bytes())
    }

    /// Provision a login principal (username + password + token attributes) with no
    /// second factor. The password is hashed with this gateway's hasher; only a login
    /// that passes the authenticator mints a token. Fails if a durable directory
    /// cannot record the enrollment. [RC-37] [RC-29]
    pub fn provision_login(
        &mut self,
        username: &str,
        password: &str,
        user: UserRecord,
    ) -> Result<(), String> {
        let enrollment = Enrollment {
            user,
            credentials: Credentials {
                password_hash: self.hasher.hash(password.as_bytes()),
                ..Credentials::default()
            },
        };
        match self
            .directory
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            Logins::InMemory(directory) => {
                directory.enroll(username, enrollment);
                Ok(())
            }
            Logins::Durable(directory) => directory
                .enroll(username, enrollment)
                .map_err(|e| e.to_string()),
        }
    }

    /// Authenticate logins against a durable, KMS-sealed identity directory instead of
    /// the in-memory one, so enrolled principals and their second factors survive a
    /// restart. [RC-29]
    pub fn with_directory(mut self, directory: DurableDirectory) -> Self {
        self.directory = Mutex::new(Logins::Durable(directory));
        self
    }

    /// Keep signed-out token ids in the sealed journal at `path`, so a revocation
    /// survives a restart as the issuer key does. Revocations whose token has already
    /// expired are purged as it opens. Fails if the journal does not open or a record
    /// does not authenticate. [VLD-ANB-MIDGARD-0021]
    pub fn with_revocation_journal(
        mut self,
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
    ) -> Result<Self, String> {
        let revocations = Revocations::open(path, sealer, now_secs())
            .map_err(|e| format!("the revocation journal does not open: {e}"))?;
        self.revocations = Mutex::new(revocations);
        Ok(self)
    }

    /// Revoked tokens not yet purged: what a request is checked against. For tests.
    #[doc(hidden)]
    pub fn revoked_count(&self) -> usize {
        self.revocations().len()
    }

    /// Records in the revocation journal, purged or not; `None` without one. For tests.
    #[doc(hidden)]
    pub fn revocation_journal_len(&self) -> Option<u64> {
        self.revocations().journal_len()
    }

    /// Remove a login principal from an in-memory directory, as de-enrolment would.
    /// For tests: the durable directory has no removal yet. [VLD-ANB-MIDGARD-0021]
    #[doc(hidden)]
    pub fn remove_login(&self, username: &str) -> bool {
        match &mut *self
            .directory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
        {
            Logins::InMemory(directory) => directory.remove(username).is_some(),
            Logins::Durable(_) => false,
        }
    }

    /// Serve administration — online enrolment of principals under dual control — over
    /// `admin`. Enrolment writes to the durable directory, so it is only meaningful on a
    /// gateway serving one. [FR-AD-5] [RC-29]
    pub fn with_admin(mut self, admin: admin_api::AdminControl) -> Self {
        self.admin = Some(Mutex::new(admin));
        self
    }

    /// The administration this gateway serves, if any, for in-process inspection
    /// (outstanding challenges, the challenge ceiling). [VLD-ANB-MIDGARD-0020]
    pub fn admin_control(&self) -> Option<std::sync::MutexGuard<'_, admin_api::AdminControl>> {
        self.admin.as_ref().map(|admin| {
            admin
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        })
    }

    /// Check records synced over the mesh against `ontology` — the one registered at
    /// boot. Without it, nothing can be synced. [FR-F-4]
    pub fn with_ontology(mut self, ontology: ontology_file::Ontology) -> Self {
        self.ontology = Some(ontology);
        self
    }

    /// Let records synced over the mesh carry markings up to `clearance`, its band and
    /// compartments. By default a sync may write any band but no compartment.
    /// [FR-F-4] [VLD-ANB-MIDGARD-0001]
    pub fn with_sync_clearance(mut self, clearance: Clearance) -> Self {
        self.sync_clearance = clearance;
        self
    }

    /// `POST /api/v1/login {username, password}` → a PQ-signed bearer token. Verifies
    /// the password (constant-time) and mints a token via the IdP; a wrong credential
    /// yields a uniform 401 (no user enumeration). The returned `token` is the hex
    /// bearer the console sends on subsequent requests. [RC-37] [RC-29]
    ///
    /// Fields over their limits are refused before anything is kept or recorded, and a
    /// source address past its allowance is refused before any password is checked.
    /// Throttled attempts are coalesced on the ledger. [VLD-ANB-MIDGARD-0005] Each
    /// attempt is reserved in the throttle before its password is checked and released
    /// after, so concurrent attempts cannot outrun the lockout. [VLD-ANB-MIDGARD-0019]
    fn handle_login(&self, req: &Request, peer: Option<SocketAddr>, now: u64) -> Response {
        let idp = match &self.idp {
            Some(i) => i,
            None => {
                return Response::json(
                    501,
                    error_json("login is not configured on this edge").encode(),
                )
            }
        };
        let body = match parse(&req.body) {
            Ok(j) => j,
            Err(_) => return Response::json(400, error_json("malformed login body").encode()),
        };
        let username = body.str_field("username").unwrap_or_default();
        let password = body.str_field("password").unwrap_or_default();
        let totp = body.str_field("totp");

        // Every field is bounded before the throttle, the directory or the ledger sees
        // it, so no login can make the edge keep or record more than these limits.
        // [VLD-ANB-MIDGARD-0005]
        if username.len() > login_throttle::MAX_USERNAME_BYTES
            || username.chars().any(char::is_control)
            || password.len() > login_throttle::MAX_PASSWORD_BYTES
            || totp.is_some_and(|code| code.chars().count() > login_throttle::MAX_TOTP_CHARS)
        {
            return Response::json(400, error_json("login fields exceed their limits").encode());
        }
        // A source past its allowance is refused before any password is checked; the
        // ledger hears of it at most once a minute, with the count since.
        if let Some(source) = self.over_source_allowance(peer) {
            self.count_login("source_throttled");
            let refused = source_refused();
            let record = self.throttle().note_source_throttled(&source, now);
            let event = match record {
                None => return refused,
                Some(0) => "LOGIN source throttled".to_string(),
                Some(n) => format!("LOGIN source throttled, {n} attempts suppressed"),
            };
            let resource = format!("source:{source}");
            return self.record_login_at(username, &event, &resource, refused, now);
        }

        let mfa = match totp {
            Some(code) => MfaProof::Totp(code.to_string()),
            None => MfaProof::None,
        };

        let unauthorized = || Response::json(401, error_json("invalid credentials").encode());
        // The attempt is reserved under the throttle's lock before its password is
        // checked, so attempts arriving together cannot all pass a check made before
        // any of them is recorded: no more than the free failures and one more are
        // checked for a name before it locks, and no more than MAX_IN_FLIGHT at once.
        // [VLD-ANB-MIDGARD-0019] A name locked by repeated failures is refused before
        // its password is checked, so guessing gains nothing while the lock holds.
        // [NFR-SEC-1] Only the lock's first such attempt is recorded; the rest, and
        // busy refusals, are counted onto the name's next recorded event.
        // [VLD-ANB-MIDGARD-0005]
        let begun = self.throttle().begin_attempt(username, now);
        match begun {
            Ok(()) => {}
            Err(Refusal::Locked { retry_after: wait }) => {
                self.count_login("throttled");
                let first = self.throttle().note_throttled(username);
                if !first {
                    return name_locked(wait);
                }
                return self.record_login(username, "LOGIN throttled", name_locked(wait), now);
            }
            Err(Refusal::Busy { retry_after: wait }) => {
                self.count_login("busy");
                return name_busy(wait);
            }
        }
        // Released however the check ends, a panic included, so a slot cannot leak.
        let mut attempt = Attempt {
            throttle: &self.login_throttle,
            username,
            now,
            finished: false,
        };

        // Password, then any enrolled second factor, then the PQ-signed token bound to
        // the principal's attributes — one path, every check fail-closed. [RC-29] A
        // legacy or weaker password hash is replaced with Argon2id once every check has
        // passed. [MG-SEC-11]
        let outcome = {
            let mut directory = self
                .directory
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            UpgradingAuthenticator::new(idp, &mut *directory, &self.hasher).authenticate(
                username,
                password.as_bytes(),
                mfa,
                Purpose(LOGIN_PURPOSE.into()),
                now,
                TOKEN_TTL,
            )
        };
        let upgrade = match &outcome {
            Ok(login) => match &login.upgrade {
                PasswordUpgrade::Upgraded => Some("PASSWORD rehashed to Argon2id".to_string()),
                PasswordUpgrade::NotRecorded(e) => {
                    Some(format!("PASSWORD rehash not recorded: {e}"))
                }
                PasswordUpgrade::Current | PasswordUpgrade::Superseded => None,
            },
            Err(_) => None,
        };
        // The throttled attempts kept off the ledger while the name was locked are named
        // on this, its next recorded event; taken before a success clears the name.
        // [VLD-ANB-MIDGARD-0005]
        let suppressed = self.throttle().take_suppressed(username);
        let (event, response) = match outcome.map(|login| login.token) {
            Ok(token) => {
                attempt.finish_attempt(Outcome::Success);
                (
                    "LOGIN succeeded",
                    Response::json(
                        200,
                        Json::obj(vec![
                            ("token", Json::s(&hex_encode(&token))),
                            ("subject", Json::s(username)),
                            (
                                "expiresAt",
                                Json::s(&now.saturating_add(TOKEN_TTL).to_string()),
                            ),
                        ])
                        .encode(),
                    ),
                )
            }
            // Not a failure of the credentials: the slot is released, nothing counted.
            Err(AuthError::Issue(IssueError::Token(e))) => {
                attempt.finish_attempt(Outcome::Error);
                (
                    "LOGIN failed",
                    Response::json(
                        500,
                        error_json(&format!("token issuance failed: {e}")).encode(),
                    ),
                )
            }
            // An unknown principal, a wrong password, a missing or wrong second factor,
            // or a purpose the principal does not hold: one uniform 401, so a caller
            // cannot tell which check refused them. The failure counts towards a lock.
            Err(_) => match attempt.finish_attempt(Outcome::Failure) {
                Some(seconds) => ("LOGIN refused, locked", name_locked(seconds)),
                None => ("LOGIN refused", unauthorized()),
            },
        };
        self.count_login(match event {
            "LOGIN succeeded" => "succeeded",
            "LOGIN refused, locked" => "locked",
            "LOGIN refused" => "refused",
            _ => "failed",
        });
        let event = match suppressed {
            0 => event.to_string(),
            n => format!("{event}, {n} throttled attempts suppressed"),
        };
        let response = match upgrade {
            Some(upgrade) => self.record_login(username, &upgrade, response, now),
            None => response,
        };
        self.record_login(username, &event, response, now)
    }

    /// Count a login attempt by its outcome in `login_attempts_total`.
    /// [VLD-ANB-MIDGARD-0005]
    fn count_login(&self, outcome: &str) {
        self.metrics
            .inc("login_attempts_total", &[("outcome", outcome)]);
    }

    /// Take one login or administration attempt from `peer`'s source allowance: the
    /// source address if it is past it, to be refused with [`source_refused`]; `None`
    /// if it is within it, or the source is unknown (an in-process caller) or the
    /// limit off. [VLD-ANB-MIDGARD-0005]
    fn over_source_allowance(&self, peer: Option<SocketAddr>) -> Option<String> {
        let (Some(sources), Some(peer)) = (&self.login_sources, peer) else {
            return None;
        };
        let source = peer.ip().to_string();
        (!sources.allow(&source)).then_some(source)
    }

    /// Every attempt is recorded in the same ledger as every access decision, so a
    /// password spray or a stolen credential leaves lineage. A login the ledger cannot
    /// record does not complete: no token leaves. [AC-M-4] [RC-29]
    fn record_login(&self, username: &str, event: &str, response: Response, now: u64) -> Response {
        self.record_login_at(username, event, "session", response, now)
    }

    /// [`Self::record_login`] against `resource`: a sign-out names the token it ended.
    /// A response the ledger cannot record becomes a 500. [AC-M-4]
    fn record_login_at(
        &self,
        username: &str,
        event: &str,
        resource: &str,
        response: Response,
        now: u64,
    ) -> Response {
        match self.lock().governance_mut().record_event(
            &audit_actor(username),
            event,
            resource,
            now,
        ) {
            Ok(_) => response,
            Err(e) => Response::json(500, error_json(&format!("audit failure: {e}")).encode()),
        }
    }

    /// `POST /api/v1/logout` with the bearer → 204: the presented token is revoked
    /// until its expiry and refused from then on, here and after a restart of a
    /// gateway that journals revocations. Only the holder of a valid token can revoke
    /// it, so a second sign-out with the same token is a 401 like any other use. A
    /// revocation the journal cannot record is a 500 and the token stays valid; a
    /// sign-out is recorded in the ledger as `LOGOUT`, naming the token id.
    /// [VLD-ANB-MIDGARD-0021] [AC-M-4]
    fn handle_logout(&self, req: &Request, now: u64) -> Response {
        let claims = match self.authenticate(req, now) {
            Ok(claims) => claims,
            Err(resp) => return resp,
        };
        let revoked = self
            .revocations()
            .revoke(claims.token_id, claims.expires_at);
        if let Err(e) = revoked {
            return Response::json(
                500,
                error_json(&format!("sign-out not recorded: {e}")).encode(),
            );
        }
        self.record_login_at(
            &claims.subject,
            "LOGOUT",
            &format!("token:{}", claims.token_id_hex()),
            Response::json(204, String::new()),
            now,
        )
    }

    /// The revocation list, recovering from poisoning like [`Self::lock`].
    fn revocations(&self) -> std::sync::MutexGuard<'_, Revocations> {
        self.revocations
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The failed-login throttle, recovering from poisoning like [`Self::lock`].
    fn throttle(&self) -> std::sync::MutexGuard<'_, login_throttle::LoginThrottle> {
        self.login_throttle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `POST /api/v1/admin/*` — administration, authenticated by an administrator's
    /// passkey assertion rather than a bearer token. Every request and approval, and
    /// every refusal, is recorded in the governance ledger beside the privilege
    /// service's own. Locks are taken administration → directory → enclosure.
    /// [FR-AD-5] [RC-29] [AC-M-4]
    ///
    /// Each source address draws on the same allowance as its logins, so a refused
    /// address cannot turn to the administration routes. [VLD-ANB-MIDGARD-0005]
    fn handle_admin(&self, req: &Request, peer: Option<SocketAddr>, now: u64) -> Response {
        if self.over_source_allowance(peer).is_some() {
            return source_refused();
        }
        let Some(control) = &self.admin else {
            return Response::json(
                404,
                error_json("administration is not served by this gateway").encode(),
            );
        };
        let body = match parse(&req.body) {
            Ok(body) => body,
            Err(_) => return Response::json(400, error_json("malformed body").encode()),
        };
        if req.path == "/api/v1/admin/challenge" {
            return self.handle_admin_challenge(control, &body, now);
        }

        let outcome = {
            let mut admin = control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match req.path.as_str() {
                "/api/v1/admin/enrolments" => {
                    let directory = self
                        .directory
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let enrolled = |subject: &str| directory.enrollment(subject).is_some();
                    admin
                        .request_enrolment(&body, &enrolled, now)
                        .map(|(json, actor)| {
                            let subject = json.str_field("subject").unwrap_or_default().to_string();
                            (json, Some((actor, "ENROLL requested", subject)))
                        })
                }
                "/api/v1/admin/enrolments/approve" => {
                    let mut directory = self
                        .directory
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let mut enrol = |subject: &str, enrollment: Enrollment| match &mut *directory {
                        Logins::Durable(d) => {
                            d.enroll(subject, enrollment).map_err(|e| e.to_string())
                        }
                        Logins::InMemory(_) => {
                            Err("this gateway serves no durable identity directory".to_string())
                        }
                    };
                    admin
                        .approve_enrolment(&body, &mut enrol, now)
                        .map(|(json, actor, subject)| {
                            (json, Some((actor, "ENROLL approved", subject)))
                        })
                }
                "/api/v1/admin/enrolments/list" => {
                    admin.list_enrolments(&body, now).map(|json| (json, None))
                }
                _ => {
                    return Response::json(404, error_json("no such administration route").encode())
                }
            }
        };

        let (response, event) = match outcome {
            Ok((json, event)) => (
                Response::ok(json.encode()),
                event.map(|(actor, action, subject)| {
                    (actor, action.to_string(), format!("principal:{subject}"))
                }),
            ),
            Err(e) => (
                Response::json(e.status, error_json(&e.message).encode()),
                Some((
                    audit_actor(body.str_field("admin").unwrap_or_default()),
                    format!("ADMIN refused ({})", e.status),
                    req.path.clone(),
                )),
            ),
        };
        match event {
            None => response,
            Some((actor, action, resource)) => {
                match self
                    .lock()
                    .governance_mut()
                    .record_event(&actor, &action, &resource, now)
                {
                    Ok(_) => response,
                    Err(e) => {
                        Response::json(500, error_json(&format!("audit failure: {e}")).encode())
                    }
                }
            }
        }
    }

    /// `POST /api/v1/admin/challenge` — a challenge bound to the administrator `body`
    /// names. The route is unauthenticated, so only a refusal at the global ceiling
    /// reaches the ledger, and that at most once a minute with the count since.
    /// [VLD-ANB-MIDGARD-0020]
    fn handle_admin_challenge(
        &self,
        control: &Mutex<admin_api::AdminControl>,
        body: &Json,
        now: u64,
    ) -> Response {
        let (refusal, record) = {
            let mut admin = control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match admin.challenge(body, now) {
                Ok(json) => return Response::ok(json.encode()),
                Err(e) if e.status == 429 => {
                    let record = admin.note_challenge_refusal(now);
                    (e, record)
                }
                Err(e) => return Response::json(e.status, error_json(&e.message).encode()),
            }
        };
        let response = Response::json(refusal.status, error_json(&refusal.message).encode());
        let Some(suppressed) = record else {
            return response;
        };
        let event = match suppressed {
            0 => "ADMIN challenge refused".to_string(),
            n => format!("ADMIN challenge refused, {n} refusals suppressed"),
        };
        match self.lock().governance_mut().record_event(
            &audit_actor(body.str_field("admin").unwrap_or_default()),
            &event,
            "/api/v1/admin/challenge",
            now,
        ) {
            Ok(_) => response,
            Err(e) => Response::json(500, error_json(&format!("audit failure: {e}")).encode()),
        }
    }

    /// Flip the gateway's readiness (a dependency became un/available). Liveness is
    /// unaffected. [RC-33]
    pub fn set_ready(&self, ready: bool) {
        self.readiness.set(ready);
    }

    /// The registry `/metrics` renders, for a listener serving this gateway to count
    /// its connections in. [VLD-ANB-MIDGARD-0004]
    pub fn metrics_registry(&self) -> Arc<MetricsRegistry> {
        Arc::clone(&self.metrics)
    }

    /// The Prometheus-format metrics snapshot (also what `/metrics` serves).
    pub fn metrics_text(&self) -> String {
        // Refresh SLO gauges so the scrape observes the current availability + any
        // breach, then render everything.
        self.availability
            .lock()
            .expect("slo lock")
            .export(&self.metrics);
        self.metrics.render()
    }

    /// Classify an object type (its resource marking) for read-up enforcement.
    pub fn with_marking(mut self, object_type: &str, level: ClassificationLevel) -> Self {
        self.markings.insert(object_type.to_string(), level);
        self
    }

    /// Register a minimal object type in this gateway's fabric — an id, a title and an
    /// optional `name` — for callers that only need the type to exist. Panics on a
    /// duplicate id, like any other builder misuse.
    pub fn with_object_type(mut self, id: &str, title: &str) -> Self {
        self.enclosure
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .register_named_type(id, title, now_secs())
            .expect("object type registers");
        self
    }

    /// Lock the enclosure, **recovering from poisoning** rather than panicking: a panic
    /// in one request handler must not take every later connection down with it.
    /// [RT-52] [NFR-SEC-1]
    fn lock(&self) -> std::sync::MutexGuard<'_, Enclosure<S, L>> {
        self.enclosure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl SecuredGateway {
    /// A fail-closed dev gateway for the runnable binary: a fresh dev IdP, a
    /// deny-by-default PDP, and a real object fabric seeded with a small, illustrative
    /// schema and data set (`Enclosure::seed_dev`). Production injects the real IdP
    /// verifying key and policy over the enclosure connection.
    pub fn dev() -> Self {
        let mut rng = OsRng;
        let idp = iam::Idp::generate("dev-idp", &mut rng).expect("dev IdP keygen");
        // The dev PDP permits the demo operator to Read/Apply at the edge and to run
        // each seeded fabric action, so a logged-in console can act. [RC-37]
        let governance = Governance::new(DefaultPdp::new(dev_login_policy()), &mut rng)
            .expect("dev governance keygen");
        let issuer_vk = idp.verifying_key().clone();
        let mut gw = Self::new(
            fips_provider(),
            issuer_vk,
            governance,
            vec!["http://localhost:5173".to_string()],
        )
        .with_idp(idp, Pbkdf2Hasher::new(4096)); // lower work factor for a snappy dev login
        gw.provision_login(DEMO_USERNAME, DEMO_PASSWORD, demo_operator())
            .expect("the dev login provisions in memory");
        gw.provision_login(
            DEMO_APPROVER_USERNAME,
            DEMO_APPROVER_PASSWORD,
            demo_approver(),
        )
        .expect("the dev approver provisions in memory");
        gw.lock()
            .seed_dev(LOGIN_PURPOSE, now_secs())
            .expect("dev enclosure seeds");
        gw
    }
}

impl<S: ObjectStore, L: LedgerStore> SecuredGateway<S, L> {
    /// Handle one request: authenticate, authorise + audit, serve — with the CORS
    /// grant attached only for an allowlisted origin. Instrumented: every request is
    /// counted by route+status, its latency observed, and the availability SLO
    /// updated (a 5xx counts against availability; a correct 401/403 does not). The
    /// `path` label is the matched route template ([`route_label`]), never the request
    /// path, so a client cannot add series. [RC-33] [VLD-ANB-MIDGARD-0003]
    pub fn handle(&self, req: &Request, now: u64) -> Response {
        self.handle_from(req, None, now)
    }

    /// [`SecuredGateway::handle`] for a request that arrived from `peer`, the client's
    /// address where it is known, without a change to [`Request`]. The login and
    /// administration routes limit attempts by it; with `None` that limit is skipped.
    /// [VLD-ANB-MIDGARD-0004] [VLD-ANB-MIDGARD-0005]
    pub fn handle_from(&self, req: &Request, peer: Option<SocketAddr>, now: u64) -> Response {
        let start = Instant::now();
        let cors = self.cors_origin(req);
        let resp = self.handle_inner(req, peer, now).with_allow_origin(cors);
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        let status = resp.status.to_string();
        let route = route_label(&req.method, &req.path);
        self.metrics.inc(
            "http_requests_total",
            &[("path", route), ("status", &status)],
        );
        self.metrics
            .observe("http_request_latency_ms", &[("path", route)], elapsed_ms);
        // Availability = served without a server error. Auth/authz denials are the
        // correct outcome, not an outage.
        let served_ok = resp.status < 500;
        self.availability
            .lock()
            .expect("slo lock")
            .record(served_ok);
        resp
    }

    fn cors_origin(&self, req: &Request) -> Option<String> {
        self.allowed_origin(req.origin.as_deref())
    }

    /// `origin`, if it is on the allowlist.
    fn allowed_origin(&self, origin: Option<&str>) -> Option<String> {
        origin
            .filter(|o| self.allowed_origins.iter().any(|a| a == o))
            .map(str::to_string)
    }

    fn handle_inner(&self, req: &Request, peer: Option<SocketAddr>, now: u64) -> Response {
        // CORS preflight.
        if req.method == "OPTIONS" {
            return Response::json(204, String::new());
        }
        // Public liveness — the only unauthenticated data route (a 200 proves the
        // process is live).
        if req.method == "GET" && (req.path == "/" || req.path == "/api/v1/health") {
            return route(req, &self.state);
        }
        // Public observability endpoints (unauthenticated): a scraper and the
        // orchestrator inside the enclosure poll these. Nothing egresses. [RC-33]
        if req.method == "GET" {
            match req.path.as_str() {
                "/api/v1/ready" => {
                    // Readiness: 200 if every dependency is healthy, else 503.
                    let report = self.health.readiness();
                    let code = if report.ready { 200 } else { 503 };
                    return Response::json(code, report.to_json());
                }
                "/metrics" => {
                    return Response::text(200, &self.metrics_text());
                }
                _ => {}
            }
        }
        // Public login: exchange credentials for a PQ-signed token (unauthenticated —
        // it is how a browser session obtains its bearer). [RC-37]
        // Administration: authenticated by passkey assertions, not a bearer token.
        if req.method == "POST" && req.path.starts_with("/api/v1/admin/") {
            return self.handle_admin(req, peer, now);
        }
        if req.method == "POST" && req.path == "/api/v1/login" {
            return self.handle_login(req, peer, now);
        }
        // Sign-out: authenticated by the very token it revokes. [VLD-ANB-MIDGARD-0021]
        if req.method == "POST" && req.path == "/api/v1/logout" {
            return self.handle_logout(req, now);
        }

        // 1. Authenticate: verify the PQ-signed IdP token or reject.
        let claims = match self.authenticate(req, now) {
            Ok(c) => c,
            Err(resp) => return resp, // 401
        };

        // 2. Authorise + audit the request against the resource it targets.
        let (id, level, action) = self.resource_action(req);
        match self.authorize(&claims, &id, level, action, now) {
            Ok(audit_seq) => self.serve(req, &claims, audit_seq, now), // permitted → serve
            Err(resp) => resp,                                         // 403, already audited
        }
    }

    /// Serve a request that has passed authentication and edge admission. Fabric and
    /// governance routes are answered by the enclosure, scoped to the caller; anything
    /// else falls through to the static platform read models. [AC-M-4]
    fn serve(&self, req: &Request, claims: &TokenClaims, audit_seq: u64, now: u64) -> Response {
        let ctx = gov_context(claims);
        let result = match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/api/v1/object-types") => Ok(self.lock().object_types(&ctx, now)),
            ("POST", "/api/v1/query") => parse(&req.body)
                .map_err(|e| EdgeError::bad_request(&e))
                .and_then(|body| search_of(&body))
                .and_then(|search| self.lock().query(&ctx, search, now)),
            ("POST", "/api/v1/actions") => parse(&req.body)
                .map_err(|e| EdgeError::bad_request(&e))
                .and_then(|body| {
                    let action = body.str_field("actionType").unwrap_or("").to_string();
                    let params = body.get("params").map(Json::str_map).unwrap_or_default();
                    let token = body.str_field("humanGateToken").map(str::to_string);
                    self.lock()
                        .apply_action(&ctx, &action, params, token.as_deref(), now)
                }),
            ("GET", "/api/v1/whoami") => Ok(whoami_json(claims)),
            ("GET", "/api/v1/approvals") => Ok(self.lock().approvals(&ctx, now)),
            ("POST", "/api/v1/approvals") => parse(&req.body)
                .map_err(|e| EdgeError::bad_request(&e))
                .and_then(|body| {
                    let action = body.str_field("actionType").unwrap_or("").to_string();
                    let target = body.str_field("target").unwrap_or("").to_string();
                    self.lock().request_approval(&ctx, &action, &target, now)
                }),
            ("POST", path) if approval_decision(path).is_some() => {
                let (id, approve) = approval_decision(path).expect("matched above");
                self.lock().decide_approval(&ctx, id, approve, now)
            }
            ("POST", "/api/v1/search-around") => parse(&req.body)
                .map_err(|e| EdgeError::bad_request(&e))
                .and_then(|body| {
                    let seed = body.str_field("seedId").unwrap_or("").to_string();
                    let hops = body.get("hops").and_then(|v| v.as_i64()).unwrap_or(2);
                    let limit = body.get("limit").and_then(Json::as_i64);
                    self.lock().search_around(&ctx, &seed, hops, limit, now)
                }),
            ("GET", "/api/v1/audit") => {
                let limit = req
                    .query_param("limit")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(50);
                Ok(self.lock().audit(&self.provider, limit))
            }
            ("GET", "/api/v1/policies") => Ok(self.lock().policies()),
            ("POST", "/api/v1/policy/simulate") => parse(&req.body)
                .map_err(|e| EdgeError::bad_request(&e))
                .and_then(|body| {
                    let purpose = body.str_field("purpose").unwrap_or("").to_string();
                    let marking = body.str_field("marking").unwrap_or("").to_string();
                    self.lock()
                        .simulate_policy(&ctx, &purpose, &marking, audit_seq)
                }),
            _ => return route(req, &self.state),
        };
        match result {
            Ok(json) => Response::ok(json.encode()),
            Err(e) => Response::json(e.status, e.body.encode()),
        }
    }

    /// The verified claims of the request's bearer token, or a 401. Past the signature
    /// and validity window, a token signed out is refused, and so is one whose subject
    /// is no longer enrolled on a gateway that issues its own tokens and so holds the
    /// directory they come from (a verify-only edge has none to consult). Each lock is
    /// released before the next is taken, and none is held while serving. Neither
    /// refusal writes a ledger row, so an unauthenticated caller cannot make the
    /// ledger grow; both are counted in `/metrics`. [VLD-ANB-MIDGARD-0021] [NFR-SEC-1]
    fn authenticate(&self, req: &Request, now: u64) -> Result<TokenClaims, Response> {
        let token = req
            .bearer()
            .ok_or_else(|| Response::json(401, error_json("missing bearer token").encode()))?;
        let bytes = hex_decode(token)
            .map_err(|_| Response::json(401, error_json("malformed bearer token").encode()))?;
        let claims = verify_token(&self.provider, &self.issuer_vk, &bytes, now).map_err(|e| {
            Response::json(401, error_json(&format!("token rejected: {e:?}")).encode())
        })?;
        let revoked = self.revocations().is_revoked(&claims.token_id, now);
        if revoked {
            return Err(self.refuse_session("token revoked"));
        }
        let enrolled = self.idp.is_none()
            || self
                .directory
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .enrollment(&claims.subject)
                .is_some();
        if !enrolled {
            return Err(self.refuse_session("subject not enrolled"));
        }
        Ok(claims)
    }

    /// A 401 for a token that verifies but no longer opens a session, counted by
    /// reason in `/metrics`. [VLD-ANB-MIDGARD-0021]
    fn refuse_session(&self, reason: &str) -> Response {
        self.metrics
            .inc("http_session_refusals_total", &[("reason", reason)]);
        Response::json(401, error_json(reason).encode())
    }

    /// The (resource id, marking level, action) a request targets.
    fn resource_action(&self, req: &Request) -> (String, ClassificationLevel, &'static str) {
        match (req.method.as_str(), req.path.as_str()) {
            // Both name the object type they read under the fabric's resource-id
            // scheme, so a type-scoped rule holds on either: a query names it, and a
            // search-around takes it from its seed (`<type>/*` or `<type>/<key>`).
            // [VLD-ANB-MIDGARD-0023]
            ("POST", "/api/v1/query") => {
                let ot = parse(&req.body)
                    .ok()
                    .and_then(|b| b.str_field("objectType").map(str::to_string))
                    .unwrap_or_default();
                let level = self.marking_for(&ot);
                (fabric::type_resource_id(&ot).0, level, "Read")
            }
            ("POST", "/api/v1/search-around") => {
                let ot = parse(&req.body)
                    .ok()
                    .and_then(|b| {
                        b.str_field("seedId")
                            .and_then(|seed| seed.split('/').next())
                            .map(str::to_string)
                    })
                    .unwrap_or_default();
                let level = self.marking_for(&ot);
                (fabric::type_resource_id(&ot).0, level, "Read")
            }
            ("POST", "/api/v1/actions") | ("POST", "/api/v1/approvals") => {
                (req.path.clone(), self.marking_for("__actions__"), "Apply")
            }
            // Deciding someone else's human gate is its own permission, so the policy
            // says who may approve, and the ledger records each decision. [NFR-AI-1]
            ("POST", path) if approval_decision(path).is_some() => {
                (path.to_string(), self.marking_for("__actions__"), "Approve")
            }
            (_, path) => (path.to_string(), self.marking_for(path), "Read"),
        }
    }

    fn marking_for(&self, key: &str) -> ClassificationLevel {
        self.markings
            .get(key)
            .copied()
            .unwrap_or(ClassificationLevel::UNCLASSIFIED)
    }

    fn authorize(
        &self,
        claims: &TokenClaims,
        id: &str,
        level: ClassificationLevel,
        action: &str,
        now: u64,
    ) -> Result<u64, Response> {
        let ctx = gov_context(claims);
        let resource = Resource {
            id: ResourceId(id.to_string()),
            marking: Marking {
                level,
                compartments: BTreeSet::new(),
            },
            // Purpose passes if the caller declared an allowed one; denial then
            // comes from role/clearance (Bell-LaPadula read-up).
            allowed_purposes: [claims.purpose.clone()].into_iter().collect(),
        };
        let mut enclosure = self.lock();
        match enclosure
            .governance_mut()
            .authorize(&ctx, &resource, action, now)
        {
            Ok(d) if d.permitted => Ok(d.audit_seq),
            Ok(d) => Err(Response::json(
                403,
                Json::obj(vec![
                    ("error", Json::s("denied by policy")),
                    ("auditSeq", Json::s(&d.audit_seq.to_string())),
                ])
                .encode(),
            )),
            Err(e) => Err(Response::json(
                500,
                error_json(&format!("audit failure: {e}")).encode(),
            )),
        }
    }

    /// The recorded audit entries so far (for verification/inspection).
    pub fn audit_len(&self) -> usize {
        self.lock().audit_len()
    }
}

/// Serve the development gateway — seeded, in memory, the published demo logins — on
/// `addr` until the process is killed. Development mode listens on loopback only: an
/// address that is not loopback, or a name that resolves to one that is not, is
/// refused with [`std::io::ErrorKind::InvalidInput`] before anything is generated or
/// bound. [RC-24] [MG-SEC-06]
pub fn serve(addr: &str) -> std::io::Result<()> {
    let addr = cli::loopback_addr(addr)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    serve_dev(addr)
}

/// [`serve`] on an address already resolved. It is checked again here, so no caller
/// can put the demo logins on another interface. [MG-SEC-06]
pub fn serve_dev(addr: SocketAddr) -> std::io::Result<()> {
    if !addr.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("development mode listens on a loopback address only, not {addr}"),
        ));
    }
    let listener = TcpListener::bind(addr)?;
    serve_listener(listener, Arc::new(SecuredGateway::dev()))
}

/// Whether a state-directory gateway may listen on an address that is not loopback.
/// [RC-24] [VLD-ANB-MIDGARD-0032]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindPolicy {
    /// Loopback only: the edge is reached through an SSH tunnel, or through a TLS
    /// terminator on the same host. The default.
    LoopbackOnly,
    /// Any interface: `--allow-non-loopback`, for a container behind PQ-mTLS or TLS 1.3
    /// termination off the host. Development mode never has it.
    AllowNonLoopback,
}

/// Check `addr` (`HOST:PORT`) against `policy` before anything binds it. Under
/// [`BindPolicy::LoopbackOnly`] the host must be a loopback IP literal (`127.0.0.0/8`,
/// `[::1]`) or the name `localhost`; any other address or host name is refused, with
/// a message that says how to serve another interface safely. [RC-24]
/// [VLD-ANB-MIDGARD-0032]
pub fn check_bind(addr: &str, policy: BindPolicy) -> Result<(), String> {
    if policy == BindPolicy::AllowNonLoopback {
        return Ok(());
    }
    let loopback = match addr.parse::<SocketAddr>() {
        Ok(sock) => sock.ip().is_loopback(),
        Err(_) => addr
            .rsplit_once(':')
            .is_some_and(|(host, port)| host == "localhost" && port.parse::<u16>().is_ok()),
    };
    if loopback {
        Ok(())
    } else {
        Err(format!(
            "refusing to bind {addr}: it is not a loopback address, and the edge speaks \
             plaintext HTTP. Production must front it with PQ-mTLS or TLS 1.3 termination; \
             behind that, pass --allow-non-loopback with --state-dir. Development mode \
             (--dev) has no override. [RC-24]"
        ))
    }
}

/// Bind `addr`, a loopback address ([`BindPolicy::LoopbackOnly`]), and serve `gateway`
/// until the process is killed — a gateway opened over a state directory by
/// [`boot::open_state_dir`]. [RC-24]
pub fn serve_gateway<S, L>(addr: &str, gateway: SecuredGateway<S, L>) -> std::io::Result<()>
where
    S: ObjectStore + Send + 'static,
    L: LedgerStore + Send + 'static,
{
    serve_shared(addr, Arc::new(gateway), BindPolicy::LoopbackOnly)
}

/// [`serve_gateway`] over a gateway another listener — the mesh door — also serves,
/// under `policy`: an address it refuses is an [`std::io::ErrorKind::InvalidInput`]
/// error and nothing is bound. [RC-24] [VLD-ANB-MIDGARD-0032]
pub fn serve_shared<S, L>(
    addr: &str,
    gw: Arc<SecuredGateway<S, L>>,
    policy: BindPolicy,
) -> std::io::Result<()>
where
    S: ObjectStore + Send + 'static,
    L: LedgerStore + Send + 'static,
{
    serve_shared_with(addr, gw, policy, EdgeLimits::default())
}

/// [`serve_shared`] within `limits`. [VLD-ANB-MIDGARD-0004]
pub fn serve_shared_with<S, L>(
    addr: &str,
    gw: Arc<SecuredGateway<S, L>>,
    policy: BindPolicy,
    limits: EdgeLimits,
) -> std::io::Result<()>
where
    S: ObjectStore + Send + 'static,
    L: LedgerStore + Send + 'static,
{
    check_bind(addr, policy)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    serve_listener_with(TcpListener::bind(addr)?, gw, limits)
}

/// Connections one source address may open in a burst before it is answered 429.
/// [VLD-ANB-MIDGARD-0004]
pub const EDGE_SOURCE_BURST: f64 = 200.0;
/// Connections a second one source address's allowance refills at.
/// [VLD-ANB-MIDGARD-0004]
pub const EDGE_SOURCE_PER_SEC: f64 = 50.0;

/// What the edge serves within: its listener's connection cap, timeouts and head
/// deadline, and how fast one source address may open connections. Under compose every
/// client reaches the edge from the proxy's one address, so there the per-source limit
/// is a global one. [VLD-ANB-MIDGARD-0004]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeLimits {
    /// The connection cap, the socket timeouts and the head deadline.
    pub listener: ListenerLimits,
    /// Connections one source address may open in a burst. With `source_per_sec`, both
    /// 0.0 turn the per-source limit off.
    pub source_burst: f64,
    /// Connections a second the allowance refills at; 0.0 with a non-zero burst makes
    /// the burst a hard quota.
    pub source_per_sec: f64,
}

impl Default for EdgeLimits {
    /// [`ListenerLimits::edge`], and [`EDGE_SOURCE_BURST`] connections in a burst,
    /// [`EDGE_SOURCE_PER_SEC`] a second, from one source address.
    fn default() -> Self {
        Self {
            listener: ListenerLimits::edge(),
            source_burst: EDGE_SOURCE_BURST,
            source_per_sec: EDGE_SOURCE_PER_SEC,
        }
    }
}

/// [`serve_listener_with`] the default [`EdgeLimits`].
pub fn serve_listener<S, L>(
    listener: TcpListener,
    gw: Arc<SecuredGateway<S, L>>,
) -> std::io::Result<()>
where
    S: ObjectStore + Send + 'static,
    L: LedgerStore + Send + 'static,
{
    serve_listener_with(listener, gw, EdgeLimits::default())
}

/// Serve `gw` on a bound listener within `limits` until the process is killed, naming
/// the address it actually listens on (a port of 0 binds an ephemeral one).
///
/// The edge serves on the bounded listener every listener in the workspace shares: at
/// most `limits.listener.max_connections` at once, each on its own named thread with
/// [`CONNECTION_STACK`], with read and write timeouts set before the first read and
/// the whole request to arrive within the head deadline. A connection over the cap is
/// answered 503 with `Retry-After: 1` on the accepting thread, and one from a source
/// address opening connections faster than its allowance is answered 429 with
/// `Retry-After: 1`; neither gets a thread. Outcomes are counted as
/// `http_connections_total{outcome}` in `/metrics`. [VLD-ANB-MIDGARD-0004]
/// [VLD-ANB-MIDGARD-0006] [NFR-SEC-1]
pub fn serve_listener_with<S, L>(
    listener: TcpListener,
    gw: Arc<SecuredGateway<S, L>>,
    limits: EdgeLimits,
) -> std::io::Result<()>
where
    S: ObjectStore + Send + 'static,
    L: LedgerStore + Send + 'static,
{
    eprintln!(
        "http-gateway (secured) listening on http://{}",
        listener.local_addr()?
    );
    eprintln!(
        "http-gateway: at most {} connections, a {}s read timeout, a {}s head deadline, {} \
         new connections a second from one address",
        limits.listener.max_connections,
        limits.listener.read_timeout.as_secs(),
        limits.listener.head_deadline.as_secs(),
        limits.source_per_sec
    );
    let metrics = gw.metrics_registry();
    let mut bounded = BoundedListener::new("http", limits.listener, metrics.clone());
    if !(limits.source_burst == 0.0 && limits.source_per_sec == 0.0) {
        let sources = RateLimiter::new(limits.source_burst, limits.source_per_sec);
        bounded = bounded.with_screen(Arc::new(move |stream, peer| {
            if sources.allow(&peer.ip().to_string()) {
                return true;
            }
            metrics.inc("http_connections_total", &[("outcome", "rate_limited")]);
            refuse_at_once(
                stream,
                &Response::json(
                    429,
                    r#"{"error":"too many connections from this address"}"#.into(),
                )
                .with_retry_after(1),
            );
            false
        }));
    }
    let head_deadline = limits.listener.head_deadline;
    bounded.serve(
        listener,
        Arc::new(move |stream, _permit| {
            if let Err(e) = handle_connection(stream, &gw, head_deadline) {
                eprintln!("http-gateway: connection error: {e}");
            }
        }),
        Arc::new(|stream| {
            refuse_at_once(
                &stream,
                &Response::json(503, r#"{"error":"the gateway is at capacity"}"#.into())
                    .with_retry_after(1),
            );
        }),
    )
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn handle_connection<S: ObjectStore, L: LedgerStore>(
    mut stream: TcpStream,
    gw: &SecuredGateway<S, L>,
    head_deadline: Duration,
) -> std::io::Result<()> {
    match read_request_within(&stream, head_deadline)? {
        Some(Incoming::Request(req)) => gw
            .handle_from(&req, stream.peer_addr().ok(), now_secs())
            .write(&mut stream),
        // Too large or unframed: refused before the body is read, and not admitted, so
        // it reaches neither authentication nor the ledger. [NFR-SEC-1]
        Some(Incoming::Refused { response, origin }) => {
            let cors = gw.allowed_origin(origin.as_deref());
            response.with_allow_origin(cors).write(&mut stream)?;
            http::linger(&stream);
            Ok(())
        }
        None => Ok(()),
    }
}

/// The 429 for a login or administration attempt from a source past its allowance.
/// [VLD-ANB-MIDGARD-0005]
fn source_refused() -> Response {
    Response::json(
        429,
        error_json("too many login attempts from this address").encode(),
    )
    .with_retry_after(2)
}

/// The 429 for a login on a name locked by repeated failures, for `seconds` more.
/// [NFR-SEC-1]
fn name_locked(seconds: u64) -> Response {
    Response::json(
        429,
        error_json(&format!(
            "too many failed logins: try again in {seconds} seconds"
        ))
        .encode(),
    )
    .with_retry_after(seconds)
}

/// The 429 for a login on a name with as many attempts already being checked as it may
/// have; not a failure. [VLD-ANB-MIDGARD-0019]
fn name_busy(seconds: u64) -> Response {
    let unit = if seconds == 1 { "second" } else { "seconds" };
    Response::json(
        429,
        error_json(&format!(
            "a login for this name is already being checked: try again in {seconds} {unit}"
        ))
        .encode(),
    )
    .with_retry_after(seconds)
}

/// A login attempt reserved by [`login_throttle::LoginThrottle::begin_attempt`]: its
/// slot is released by [`Attempt::finish_attempt`], or, if the check never finishes
/// (a panic), on drop as an [`Outcome::Error`], so no slot is held for ever.
/// [VLD-ANB-MIDGARD-0019]
struct Attempt<'a> {
    throttle: &'a Mutex<login_throttle::LoginThrottle>,
    username: &'a str,
    now: u64,
    finished: bool,
}

impl Attempt<'_> {
    /// Release the slot and record `outcome`; the lockout the name now carries, in
    /// seconds, if this failure locked it.
    fn finish_attempt(&mut self, outcome: Outcome) -> Option<u64> {
        self.finished = true;
        self.throttle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .finish_attempt(self.username, outcome, self.now)
    }
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.finish_attempt(Outcome::Error);
        }
    }
}

/// A caller-supplied username as a ledger actor. It is recorded as given, so repeated
/// attempts against one name are visible, but stripped of control characters and
/// capped so a hostile value cannot bloat a row or imitate another. [AC-M-4]
fn audit_actor(username: &str) -> String {
    let name: String = username
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    if name.trim().is_empty() {
        "(no username)".to_string()
    } else {
        name
    }
}

/// Lowercase-hex encode (the token is returned hex-encoded to the browser). [RC-37]
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    s
}

/// A `/query` body as a fabric search:
///
/// ```json
/// {"objectType":"runway",
///  "filter":{"lighted":"true"},
///  "where":[{"property":"length_ft","op":"gt","value":"5000"}],
///  "orderBy":"length_ft","descending":true,"offset":0,"limit":50}
/// ```
///
/// `filter` is exact matches, as before; `where` adds `eq`, `ne`, `lt`, `lte`, `gt`,
/// `gte` and `contains` (text; property `*` for any text property). `limit` defaults
/// to 5 and `maxResults` is still read for it. [DD-1]
fn search_of(body: &Json) -> Result<fabric::Search, EdgeError> {
    let int = |key: &str| body.get(key).and_then(Json::as_i64);
    let count = |n: i64| usize::try_from(n.max(0)).unwrap_or(0);
    let mut conditions: Vec<fabric::Condition> = body
        .get("filter")
        .map(Json::str_map)
        .unwrap_or_default()
        .into_iter()
        .map(|(property, value)| fabric::Condition {
            property,
            op: fabric::CompareOp::Eq,
            value,
        })
        .collect();
    match body.get("where") {
        None | Some(Json::Null) => {}
        Some(Json::Arr(items)) => {
            for item in items {
                let field = |key: &str| item.str_field(key).unwrap_or("").to_string();
                let op = fabric::CompareOp::parse(&field("op")).ok_or_else(|| {
                    EdgeError::bad_request(&format!(
                        "unknown operator {:?}: use eq, ne, lt, lte, gt, gte or contains",
                        field("op")
                    ))
                })?;
                conditions.push(fabric::Condition {
                    property: field("property"),
                    op,
                    value: field("value"),
                });
            }
        }
        Some(_) => return Err(EdgeError::bad_request("\"where\" must be a list")),
    }
    Ok(fabric::Search {
        object_type: body.str_field("objectType").unwrap_or("").to_string(),
        conditions,
        order_by: body
            .str_field("orderBy")
            .filter(|p| !p.is_empty())
            .map(str::to_string),
        descending: matches!(body.get("descending"), Some(Json::Bool(true))),
        offset: count(int("offset").unwrap_or(0)),
        limit: count(int("limit").or_else(|| int("maxResults")).unwrap_or(5)),
    })
}

/// Every (method, path) the edge serves, as the route template its metrics carry.
/// [VLD-ANB-MIDGARD-0003]
const ROUTES: &[(&str, &str)] = &[
    ("GET", "/"),
    ("GET", "/api/v1/health"),
    ("GET", "/api/v1/ready"),
    ("GET", "/metrics"),
    ("POST", "/api/v1/login"),
    ("POST", "/api/v1/admin/challenge"),
    ("POST", "/api/v1/admin/enrolments"),
    ("POST", "/api/v1/admin/enrolments/approve"),
    ("POST", "/api/v1/admin/enrolments/list"),
    ("GET", "/api/v1/object-types"),
    ("POST", "/api/v1/query"),
    ("POST", "/api/v1/actions"),
    ("GET", "/api/v1/whoami"),
    ("GET", "/api/v1/approvals"),
    ("POST", "/api/v1/approvals"),
    ("POST", "/api/v1/search-around"),
    ("GET", "/api/v1/audit"),
    ("GET", "/api/v1/policies"),
    ("POST", "/api/v1/policy/simulate"),
    ("GET", "/api/v1/identities"),
    ("GET", "/api/v1/keys"),
    ("GET", "/api/v1/compute"),
    ("GET", "/api/v1/observability"),
    ("GET", "/api/v1/resilience"),
    ("GET", "/api/v1/sdk-surface"),
];

/// The metrics label for a request: the template of the route it matched, with an
/// approval id collapsed to `{id}`, `OPTIONS` for a preflight and `other` for anything
/// the edge does not serve. Nothing in it is taken from the request, so the label has
/// a fixed set of values whatever a client sends. [VLD-ANB-MIDGARD-0003]
fn route_label(method: &str, path: &str) -> &'static str {
    if method == "OPTIONS" {
        return "OPTIONS";
    }
    if let Some(&(_, template)) = ROUTES.iter().find(|&&(m, p)| m == method && p == path) {
        return template;
    }
    match (method, approval_decision(path)) {
        ("POST", Some((_, true))) => "/api/v1/approvals/{id}/approve",
        ("POST", Some((_, false))) => "/api/v1/approvals/{id}/reject",
        _ => "other",
    }
}

/// `/api/v1/approvals/{id}/approve` or `/reject` → the approval id and whether it
/// approves.
fn approval_decision(path: &str) -> Option<(&str, bool)> {
    let rest = path.strip_prefix("/api/v1/approvals/")?;
    let (id, verb) = rest.split_once('/')?;
    let approve = match verb {
        "approve" => true,
        "reject" => false,
        _ => return None,
    };
    (!id.is_empty() && !id.contains('/')).then_some((id, approve))
}

/// `GET /whoami`: the session's own identity, from its verified token — who it is,
/// what it holds and when it ends — so a console shows and dry-runs as the signed-in
/// user, and knows when to ask them to sign in again. Nothing here is taken from the
/// request. [RC-37] [NFR-SEC-2]
fn whoami_json(claims: &TokenClaims) -> Json {
    let strings = |items: Vec<&str>| Json::Arr(items.into_iter().map(Json::s).collect());
    Json::obj(vec![
        ("subject", Json::s(&claims.subject)),
        (
            "roles",
            strings(claims.roles.iter().map(|r| r.0.as_str()).collect()),
        ),
        ("clearance", Json::s(enclosure::band(claims.level))),
        (
            "compartments",
            strings(claims.compartments.iter().map(|c| c.0.as_str()).collect()),
        ),
        ("purpose", Json::s(&claims.purpose.0)),
        (
            "expiresAt",
            Json::Int(i64::try_from(claims.expires_at).unwrap_or(i64::MAX)),
        ),
    ])
}

/// The caller's governance context, from their verified token claims.
fn gov_context(claims: &TokenClaims) -> GovContext {
    GovContext {
        subject: claims.subject.clone(),
        roles: claims.roles.clone(),
        clearance: Clearance {
            level: claims.level,
            compartments: claims.compartments.clone(),
        },
        purpose: claims.purpose.clone(),
    }
}

/// The dev/login policy: permit the demo operator role to Read + Apply at the edge,
/// and to run each seeded fabric action; permit the demo approver role to Read and to
/// Approve human gates. Public so a test can assemble the same governance over a
/// durable ledger. [RC-37] [NFR-AI-1]
pub fn dev_login_policy() -> PolicySet {
    let rule = |id: String, role: &str, action: &str| Rule {
        id: RuleId(id),
        role: Role(role.into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    };
    let mut rules = vec![
        rule("operator-read".into(), "operator", "Read"),
        rule("operator-apply".into(), "operator", "Apply"),
        rule("approver-read".into(), "approver", "Read"),
        rule("approver-approve".into(), "approver", "Approve"),
    ];
    // The fabric authorises an action on the action's own api name, not on the edge's
    // generic "Apply" — without one rule per action, every action is denied.
    rules.extend(
        enclosure::DEV_ACTIONS
            .iter()
            .map(|a| rule(format!("operator-{}", a.id), "operator", a.api_name)),
    );
    PolicySet::new(rules).expect("valid dev policy")
}

/// The demo operator's token attributes (dev only). [RC-37]
///
/// Cleared to the highest marking band and holding the OPS-N compartment but not SIG —
/// so the seeded SIG-compartmented sensor feed is filtered out of every read.
/// Mandatory access control, working and visible in the console.
pub fn demo_operator() -> UserRecord {
    UserRecord {
        roles: [Role("operator".into())].into_iter().collect(),
        clearance: Clearance {
            level: enclosure::level_of("confidential").expect("known marking band"),
            compartments: [Compartment("OPS-N".into())].into_iter().collect(),
        },
        purposes: [Purpose(LOGIN_PURPOSE.into())].into_iter().collect(),
    }
}

/// The demo approver's token attributes (dev only): the operator's clearance, so they
/// can see what they are asked to approve, but the approver role alone — they decide
/// human gates and cannot run actions themselves. [NFR-AI-1]
pub fn demo_approver() -> UserRecord {
    UserRecord {
        roles: [Role("approver".into())].into_iter().collect(),
        ..demo_operator()
    }
}

/// Lowercase-hex decode (the bearer token is carried hex-encoded in the header).
fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err(());
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16).ok_or(())?;
        let lo = (b[i + 1] as char).to_digit(16).ok_or(())?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Ok(out)
}

/// Map a request to a response. All paths are under `/api/v1`.
pub fn route(req: &Request, state: &State) -> Response {
    // CORS preflight.
    if req.method == "OPTIONS" {
        return Response::json(204, String::new());
    }

    let ok = |j: Json| Response::ok(j.encode());
    let m = req.method.as_str();
    match (m, req.path.as_str()) {
        ("GET", "/") | ("GET", "/api/v1/health") => ok(Json::obj(vec![
            ("status", Json::s("ok")),
            ("service", Json::s("http-gateway")),
            ("transport", Json::s("HTTP/JSON edge → enclosure")),
        ])),

        // Object fabric, governance and audit routes are NOT served here: they are
        // caller-scoped (marking-filtered and audited against an authenticated
        // identity), so they exist only behind `SecuredGateway`.
        ("GET", "/api/v1/identities") => ok(state.identities()),
        ("GET", "/api/v1/keys") => ok(state.keys()),
        ("GET", "/api/v1/compute") => ok(state.compute()),
        ("GET", "/api/v1/observability") => ok(state.observability()),
        ("GET", "/api/v1/resilience") => ok(state.resilience()),
        ("GET", "/api/v1/sdk-surface") => ok(state.contract_surface()),

        ("GET", _) | ("POST", _) => Response::json(404, error_json("not found").encode()),
        _ => Response::json(405, error_json("method not allowed").encode()),
    }
}

fn error_json(message: &str) -> Json {
    Json::obj(vec![("error", Json::s(message))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn request(method: &str, path: &str, body: &str, bearer: Option<&str>) -> Request {
        Request {
            method: method.to_string(),
            path: path.to_string(),
            query: String::new(),
            body: body.to_string(),
            authorization: bearer.map(|t| format!("Bearer {t}")),
            origin: None,
        }
    }

    #[test]
    fn a_poisoned_enclosure_lock_does_not_take_the_edge_down() {
        // RT-52 / RC-52: poison the enclosure mutex the way a panicking request handler
        // would, then prove later requests still authenticate, authorise, audit and
        // serve instead of propagating the poison as a panic.
        let gw = Arc::new(SecuredGateway::dev());
        let poisoner = Arc::clone(&gw);
        let joined = std::thread::spawn(move || {
            let _guard = poisoner.enclosure.lock();
            panic!("simulated handler panic while holding the enclosure lock");
        })
        .join();
        assert!(joined.is_err(), "the helper thread panicked");
        assert!(
            gw.enclosure.is_poisoned(),
            "the enclosure mutex is poisoned"
        );

        let login = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
        let resp = gw.handle(&request("POST", "/api/v1/login", &login, None), now_secs());
        assert_eq!(resp.status, 200, "{}", resp.body);
        let token = parse(&resp.body)
            .ok()
            .and_then(|j| j.str_field("token").map(str::to_string))
            .expect("login returns a token");

        let before = gw.audit_len();
        let query = request(
            "POST",
            "/api/v1/query",
            r#"{"objectType":"aircraft"}"#,
            Some(&token),
        );
        assert_eq!(gw.handle(&query, now_secs()).status, 200);
        assert!(
            gw.audit_len() > before,
            "the served request was still audited"
        );
    }

    #[test]
    fn route_label_names_every_served_route_and_nothing_else() {
        // [VLD-ANB-MIDGARD-0003] [AC-33.2] [AC-33.4]
        let table = [
            ("GET", "/", "/"),
            ("GET", "/api/v1/health", "/api/v1/health"),
            ("GET", "/api/v1/ready", "/api/v1/ready"),
            ("GET", "/metrics", "/metrics"),
            ("POST", "/api/v1/login", "/api/v1/login"),
            ("POST", "/api/v1/admin/challenge", "/api/v1/admin/challenge"),
            (
                "POST",
                "/api/v1/admin/enrolments",
                "/api/v1/admin/enrolments",
            ),
            (
                "POST",
                "/api/v1/admin/enrolments/approve",
                "/api/v1/admin/enrolments/approve",
            ),
            (
                "POST",
                "/api/v1/admin/enrolments/list",
                "/api/v1/admin/enrolments/list",
            ),
            ("GET", "/api/v1/object-types", "/api/v1/object-types"),
            ("POST", "/api/v1/query", "/api/v1/query"),
            ("POST", "/api/v1/actions", "/api/v1/actions"),
            ("GET", "/api/v1/whoami", "/api/v1/whoami"),
            ("GET", "/api/v1/approvals", "/api/v1/approvals"),
            ("POST", "/api/v1/approvals", "/api/v1/approvals"),
            (
                "POST",
                "/api/v1/approvals/abc-123/approve",
                "/api/v1/approvals/{id}/approve",
            ),
            (
                "POST",
                "/api/v1/approvals/xyz/reject",
                "/api/v1/approvals/{id}/reject",
            ),
            ("POST", "/api/v1/search-around", "/api/v1/search-around"),
            ("GET", "/api/v1/audit", "/api/v1/audit"),
            ("GET", "/api/v1/policies", "/api/v1/policies"),
            ("POST", "/api/v1/policy/simulate", "/api/v1/policy/simulate"),
            ("GET", "/api/v1/identities", "/api/v1/identities"),
            ("GET", "/api/v1/keys", "/api/v1/keys"),
            ("GET", "/api/v1/compute", "/api/v1/compute"),
            ("GET", "/api/v1/observability", "/api/v1/observability"),
            ("GET", "/api/v1/resilience", "/api/v1/resilience"),
            ("GET", "/api/v1/sdk-surface", "/api/v1/sdk-surface"),
            ("OPTIONS", "/api/v1/query", "OPTIONS"),
            ("OPTIONS", "/anything/at/all", "OPTIONS"),
            // Not served: a trailing slash, an empty or nested approval id, an unknown
            // verb, the wrong method, an unknown path or method.
            ("GET", "/api/v1/health/", "other"),
            ("POST", "/api/v1/approvals//approve", "other"),
            ("POST", "/api/v1/approvals/a/b/approve", "other"),
            ("POST", "/api/v1/approvals/abc/delete", "other"),
            ("GET", "/api/v1/approvals/abc/approve", "other"),
            ("GET", "/api/v1/query", "other"),
            ("POST", "/api/v1/health", "other"),
            ("GET", "/probe/1", "other"),
            ("PUT", "/api/v1/query", "other"),
        ];
        for (method, path, expected) in table {
            assert_eq!(route_label(method, path), expected, "{method} {path}");
        }
        // Every entry in the served routes is labelled as its own path.
        for &(method, path) in ROUTES {
            assert_eq!(route_label(method, path), path);
        }
    }

    /// Under the default policy only loopback hosts bind; `--allow-non-loopback`
    /// admits any. The refusal names TLS termination and the override.
    /// [RC-24] [NFR-SEC-1] [AC-13.2] [VLD-ANB-MIDGARD-0032]
    #[test]
    fn check_bind_refuses_non_loopback_hosts_unless_allowed() {
        let loopback = ["127.0.0.1:7710", "localhost:7710", "[::1]:7710"];
        let open = [
            "0.0.0.0:7710",
            "10.0.0.1:7710",
            "edge.internal:7710",
            "[::]:7710",
        ];
        for addr in loopback {
            assert_eq!(check_bind(addr, BindPolicy::LoopbackOnly), Ok(()), "{addr}");
            assert_eq!(
                check_bind(addr, BindPolicy::AllowNonLoopback),
                Ok(()),
                "{addr}"
            );
        }
        for addr in open {
            let err = check_bind(addr, BindPolicy::LoopbackOnly).expect_err(addr);
            assert!(err.contains("TLS"), "{addr}: {err}");
            assert!(err.contains("--allow-non-loopback"), "{addr}: {err}");
            assert_eq!(
                check_bind(addr, BindPolicy::AllowNonLoopback),
                Ok(()),
                "{addr}"
            );
        }
        // A look-alike prefix is not loopback, and neither is a name without a port.
        for addr in [
            "127.0.0.1.edge.internal:7710",
            "localhost.edge.internal:7710",
            "localhost",
        ] {
            assert!(
                check_bind(addr, BindPolicy::LoopbackOnly).is_err(),
                "{addr}"
            );
        }
    }
}
