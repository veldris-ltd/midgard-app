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
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod admin_api;
pub mod boot;
pub mod camera_analytics;
pub mod data;
pub mod enclosure;
pub mod heimdall_bridge;
pub mod http;
pub mod import_file;
pub mod mesh_sync;
pub mod mimir_observer;
pub mod ontology_file;
pub mod state_dir;
use service_runtime::strict_json;
/// The JSON codec and the policy file, shared with the mesh services.
pub use service_runtime::{json, policy_file};

use data::State;
use enclosure::{EdgeError, Enclosure};
use fabric::{InMemoryLedger, InMemoryStore, LedgerStore, ObjectStore};
use governance::{GovContext, Governance};
use http::{read_request, Request, Response};
use iam::{
    verify_token, AuthError, Authenticator, Credentials, Directory, DurableDirectory, Enrollment,
    Idp, InMemoryDirectory, IssueError, MfaProof, PasswordHasher, Pbkdf2Hasher, TokenClaims,
    UserRecord,
};
use json::{parse, Json};
use policy::{
    Action, ClassificationLevel, Clearance, Compartment, DefaultPdp, Effect, Marking, PolicySet,
    Purpose, Resource, ResourceId, Role, Rule, RuleId, Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, OsRng, VerifyingKey};
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
pub const DEMO_USERNAME: &str = "operator";
pub const DEMO_PASSWORD: &str = "midgard-demo";

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
    /// Administration — dual-controlled enrolment — where a durable directory is served.
    admin: Option<Mutex<admin_api::AdminControl>>,
    /// The ontology records synced over the mesh are checked against, where a state
    /// directory declares one. [FR-F-4]
    ontology: Option<ontology_file::Ontology>,
    /// Password hasher for login verification (Argon2id/PBKDF2 seam, RT-29).
    hasher: Pbkdf2Hasher,
    // ---- observability (in-enclosure; nothing egresses) [RC-33] ----
    metrics: MetricsRegistry,
    health: HealthRegistry,
    readiness: Arc<Toggle>,
    availability: Mutex<AvailabilitySlo>,
    /// The read path to Heimdall's picture, where one is configured. [FR-H-1]
    heimdall: Option<heimdall_bridge::HeimdallBridge>,
    analytics: Option<camera_analytics::AnalyticsClient>,
    mimir: Option<mimir_observer::MimirObserver>,
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
        Self {
            state: State::new(),
            provider,
            issuer_vk,
            enclosure: Mutex::new(enclosure),
            allowed_origins,
            markings: BTreeMap::new(),
            idp: None,
            directory: Mutex::new(Logins::InMemory(InMemoryDirectory::new())),
            admin: None,
            ontology: None,
            hasher: Pbkdf2Hasher::production(),
            metrics: MetricsRegistry::new(),
            health,
            readiness,
            availability: Mutex::new(AvailabilitySlo::new(
                "gateway_availability",
                AVAILABILITY_OBJECTIVE,
            )),
            heimdall: None,
            analytics: None,
            mimir: None,
        }
    }

    /// Serve Heimdall's common operating picture at `GET /api/v1/heimdall/picture` and
    /// its track history at `GET /api/v1/heimdall/history`,
    /// read through `bridge` for signed-in users the policy lets `Read` it. Returns self
    /// for chaining. [FR-H-1]
    pub fn with_heimdall(mut self, bridge: heimdall_bridge::HeimdallBridge) -> Self {
        self.heimdall = Some(bridge);
        self
    }

    /// Serve camera analytics on request at `POST /api/v1/heimdall/cameras/{id}/analyse`
    /// through `client`, authorised and audited as `Apply`. [FR-P3-5]
    pub fn with_analytics(mut self, client: camera_analytics::AnalyticsClient) -> Self {
        self.analytics = Some(client);
        self
    }

    /// Serve the Mimir console's live view (`/api/v1/mimir/models`, `/sessions`) from
    /// `observer`, authorised and audited as `Read`. [FR-MI-8]
    pub fn with_mimir(mut self, observer: mimir_observer::MimirObserver) -> Self {
        self.mimir = Some(observer);
        self
    }

    /// Enable the `/api/v1/login` endpoint on this gateway: hold the token-minting IdP
    /// (its key must be the one `issuer_vk` verifies) and the password hasher used to
    /// check credentials. Returns self for chaining. [RC-37]
    pub fn with_idp(mut self, idp: Idp, hasher: Pbkdf2Hasher) -> Self {
        self.idp = Some(idp);
        self.hasher = hasher;
        self
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

    /// Serve administration — online enrolment of principals under dual control — over
    /// `admin`. Enrolment writes to the durable directory, so it is only meaningful on a
    /// gateway serving one. [FR-AD-5] [RC-29]
    pub fn with_admin(mut self, admin: admin_api::AdminControl) -> Self {
        self.admin = Some(Mutex::new(admin));
        self
    }

    /// Check records synced over the mesh against `ontology` — the one registered at
    /// boot. Without it, nothing can be synced. [FR-F-4]
    pub fn with_ontology(mut self, ontology: ontology_file::Ontology) -> Self {
        self.ontology = Some(ontology);
        self
    }

    /// `POST /api/v1/login {username, password}` → a PQ-signed bearer token. Verifies
    /// the password (constant-time) and mints a token via the IdP; a wrong credential
    /// yields a uniform 401 (no user enumeration). The returned `token` is the hex
    /// bearer the console sends on subsequent requests. [RC-37] [RC-29]
    fn handle_login(&self, req: &Request, now: u64) -> Response {
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

        let mfa = match body.str_field("totp") {
            Some(code) => MfaProof::Totp(code.to_string()),
            None => MfaProof::None,
        };

        let unauthorized = || Response::json(401, error_json("invalid credentials").encode());
        // Password, then any enrolled second factor, then the PQ-signed token bound to
        // the principal's attributes — one path, every check fail-closed. [RC-29]
        let outcome = {
            let directory = self
                .directory
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Authenticator::new(idp, &*directory, &self.hasher).authenticate(
                username,
                password.as_bytes(),
                mfa,
                Purpose(LOGIN_PURPOSE.into()),
                now,
                TOKEN_TTL,
            )
        };
        let (event, response) = match outcome {
            Ok(token) => (
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
            ),
            Err(AuthError::Issue(IssueError::Token(e))) => (
                "LOGIN failed",
                Response::json(
                    500,
                    error_json(&format!("token issuance failed: {e}")).encode(),
                ),
            ),
            // An unknown principal, a wrong password, a missing or wrong second factor,
            // or a purpose the principal does not hold: one uniform 401, so a caller
            // cannot tell which check refused them.
            Err(_) => ("LOGIN refused", unauthorized()),
        };

        // Every attempt is recorded in the same ledger as every access decision, so a
        // password spray or a stolen credential leaves lineage. A login the ledger cannot
        // record does not complete: no token leaves. [AC-M-4] [RC-29]
        match self.lock().governance_mut().record_event(
            &audit_actor(username),
            event,
            "session",
            now,
        ) {
            Ok(_) => response,
            Err(e) => Response::json(500, error_json(&format!("audit failure: {e}")).encode()),
        }
    }

    /// `POST /api/v1/admin/*` — administration, authenticated by an administrator's
    /// passkey assertion rather than a bearer token. Every request and approval, and
    /// every refusal, is recorded in the governance ledger beside the privilege
    /// service's own. Locks are taken administration → directory → enclosure.
    /// [FR-AD-5] [RC-29] [AC-M-4]
    fn handle_admin(&self, req: &Request, now: u64) -> Response {
        let Some(control) = &self.admin else {
            return Response::json(
                404,
                error_json("administration is not served by this gateway").encode(),
            );
        };
        let body = if req.path == "/api/v1/admin/challenge" {
            Json::Null
        } else {
            match parse(&req.body) {
                Ok(body) => body,
                Err(_) => return Response::json(400, error_json("malformed body").encode()),
            }
        };

        let outcome = {
            let mut admin = control
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match req.path.as_str() {
                "/api/v1/admin/challenge" => return Response::ok(admin.challenge(now).encode()),
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

    /// Flip the gateway's readiness (a dependency became un/available). Liveness is
    /// unaffected. [RC-33]
    pub fn set_ready(&self, ready: bool) {
        self.readiness.set(ready);
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
        gw.lock()
            .seed_dev(LOGIN_PURPOSE, now_secs())
            .expect("dev enclosure seeds");
        gw
    }
}

impl<S: ObjectStore, L: LedgerStore> SecuredGateway<S, L> {
    /// Handle one request: authenticate, authorise + audit, serve — with the CORS
    /// grant attached only for an allowlisted origin. Instrumented: every request is
    /// counted by path+status, its latency observed, and the availability SLO
    /// updated (a 5xx counts against availability; a correct 401/403 does not).
    /// [RC-33]
    pub fn handle(&self, req: &Request, now: u64) -> Response {
        let start = Instant::now();
        let cors = self.cors_origin(req);
        let resp = self.handle_inner(req, now).with_allow_origin(cors);
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        let status = resp.status.to_string();
        self.metrics.inc(
            "http_requests_total",
            &[("path", req.path.as_str()), ("status", &status)],
        );
        self.metrics.observe(
            "http_request_latency_ms",
            &[("path", req.path.as_str())],
            elapsed_ms,
        );
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
        req.origin
            .as_ref()
            .filter(|o| self.allowed_origins.iter().any(|a| a == *o))
            .cloned()
    }

    fn handle_inner(&self, req: &Request, now: u64) -> Response {
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
            return self.handle_admin(req, now);
        }
        if req.method == "POST" && req.path == "/api/v1/login" {
            return self.handle_login(req, now);
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
                .and_then(|body| {
                    let object_type = body.str_field("objectType").unwrap_or("").to_string();
                    let max = body.get("maxResults").and_then(|v| v.as_i64()).unwrap_or(5);
                    let filter = body.get("filter").map(Json::str_map).unwrap_or_default();
                    self.lock().query(&ctx, &object_type, max, filter, now)
                }),
            ("POST", "/api/v1/actions") => parse(&req.body)
                .map_err(|e| EdgeError::bad_request(&e))
                .and_then(|body| {
                    let action = body.str_field("actionType").unwrap_or("").to_string();
                    let params = body.get("params").map(Json::str_map).unwrap_or_default();
                    let token = body.str_field("humanGateToken").map(str::to_string);
                    self.lock()
                        .apply_action(&ctx, &action, params, token.as_deref(), now)
                }),
            ("POST", "/api/v1/search-around") => parse(&req.body)
                .map_err(|e| EdgeError::bad_request(&e))
                .and_then(|body| {
                    let seed = body.str_field("seedId").unwrap_or("").to_string();
                    let hops = body.get("hops").and_then(|v| v.as_i64()).unwrap_or(2);
                    self.lock().search_around(&ctx, &seed, hops, now)
                }),
            ("GET", "/api/v1/audit") => {
                let limit = req
                    .query_param("limit")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(50);
                Ok(self.lock().audit(&self.provider, limit))
            }
            ("GET", "/api/v1/policies") => Ok(self.lock().policies()),
            // Authorised and audited above as `Read` on this path; Heimdall audits the
            // gateway's own read in its ledger.
            ("GET", "/api/v1/heimdall/picture") => match &self.heimdall {
                Some(bridge) => {
                    heimdall_bridge::picture_layers_param(&req.query).and_then(|layers| {
                        let boxes = heimdall_bridge::Boxes::from_query(&req.query)?;
                        Ok(boxes.filter_picture(bridge.picture_layers(&layers)?))
                    })
                }
                None => Err(EdgeError::new(
                    404,
                    "this gateway is not connected to Heimdall",
                )),
            },
            // Track history, authorised and audited the same way; Heimdall audits each
            // page the gateway reads. [FR-H-1]
            ("GET", "/api/v1/heimdall/history") => match &self.heimdall {
                Some(bridge) => heimdall_bridge::HistoryParams::from_query(&req.query, now)
                    .and_then(|params| {
                        let boxes = heimdall_bridge::Boxes::from_query(&req.query)?;
                        Ok(boxes.filter_history(bridge.history(&params)?))
                    }),
                None => Err(EdgeError::new(
                    404,
                    "this gateway is not connected to Heimdall",
                )),
            },
            // Alerts, authorised and audited as `Read` on this path; Heimdall audits the
            // read too. [FR-H-4]
            ("GET", heimdall_bridge::ALERTS_PATH) => match &self.heimdall {
                Some(bridge) => heimdall_bridge::alerts_state(&req.query)
                    .and_then(|open_only| bridge.alerts(open_only)),
                None => Err(EdgeError::new(
                    404,
                    "this gateway is not connected to Heimdall",
                )),
            },
            // The Mimir console's live view, authorised and audited as `Read`. [FR-MI-8]
            ("GET", mimir_observer::MODELS_PATH) => match &self.mimir {
                Some(m) => m.models(),
                None => Err(EdgeError::new(
                    404,
                    "this gateway is not connected to Mimir",
                )),
            },
            ("GET", mimir_observer::SESSIONS_PATH) => match &self.mimir {
                Some(m) => mimir_observer::sessions_limit(req.query_param("limit").as_deref())
                    .and_then(|n| m.sessions(n)),
                None => Err(EdgeError::new(
                    404,
                    "this gateway is not connected to Mimir",
                )),
            },
            // A question over the picture (the Mimir core agent via Heimdall), a read like
            // decision support: it answers and changes nothing. [FR-MI-3] [FR-H-4]
            ("POST", heimdall_bridge::ASK_PATH) => match &self.heimdall {
                Some(bridge) => match req.query_param("question") {
                    Some(q) if q.trim().chars().count() >= 3 => {
                        heimdall_bridge::Boxes::from_query(&req.query).and_then(|boxes| {
                            let focus =
                                heimdall_bridge::ask_focus(req.query_param("focus").as_deref())?;
                            bridge.ask(&q, &claims.subject, &boxes, focus)
                        })
                    }
                    _ => Err(EdgeError::bad_request("ask a question (?question=...)")),
                },
                None => Err(EdgeError::new(
                    404,
                    "this gateway is not connected to Heimdall",
                )),
            },
            // Decision support on an alert or entity (Mimir via Heimdall), authorised and
            // audited as a read: it advises and changes nothing. [FR-H-4]
            ("POST", path) if heimdall_bridge::advise_target(path).is_some() => {
                match (&self.heimdall, heimdall_bridge::advise_target(path)) {
                    (Some(bridge), Some(target)) => target.and_then(|(kind, id)| {
                        bridge.advise(
                            kind,
                            &id,
                            &claims.subject,
                            req.query_param("question").as_deref(),
                        )
                    }),
                    _ => Err(EdgeError::new(
                        404,
                        "this gateway is not connected to Heimdall",
                    )),
                }
            }
            // Camera analytics on request, authorised and audited as `Apply` above under
            // the signed-in user, whose name the service records as `by`. [FR-P3-5]
            ("POST", path) if camera_analytics::analyse_target(path).is_some() => {
                match (&self.analytics, camera_analytics::analyse_target(path)) {
                    (Some(client), Some(id)) => {
                        id.and_then(|id| client.analyse(&id, &claims.subject))
                    }
                    _ => Err(EdgeError::new(404, "this gateway has no camera analytics")),
                }
            }
            // Watching a camera (re-analysed every few minutes for `hours`), an action
            // like an analysis; its trend is a read. [FR-P3-5]
            ("POST", path) if camera_analytics::watch_target(path).is_some() => {
                match (&self.analytics, camera_analytics::watch_target(path)) {
                    (Some(client), Some(id)) => id.and_then(|id| {
                        let hours =
                            camera_analytics::watch_hours(req.query_param("hours").as_deref())?;
                        client.watch(&id, hours, &claims.subject)
                    }),
                    _ => Err(EdgeError::new(404, "this gateway has no camera analytics")),
                }
            }
            ("GET", path) if camera_analytics::trend_target(path).is_some() => {
                match (&self.analytics, camera_analytics::trend_target(path)) {
                    (Some(client), Some(id)) => id.and_then(|id| client.trend(&id)),
                    _ => Err(EdgeError::new(404, "this gateway has no camera analytics")),
                }
            }
            // An acknowledgement, authorised and audited as `Apply` above under the
            // signed-in user, whose name Heimdall records as `by`. [FR-H-4]
            ("POST", path) if heimdall_bridge::ack_target(path).is_some() => {
                match (&self.heimdall, heimdall_bridge::ack_target(path)) {
                    (Some(bridge), Some(id)) => {
                        id.and_then(|id| bridge.acknowledge(&id, &claims.subject))
                    }
                    _ => Err(EdgeError::new(
                        404,
                        "this gateway is not connected to Heimdall",
                    )),
                }
            }
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

    fn authenticate(&self, req: &Request, now: u64) -> Result<TokenClaims, Response> {
        let token = req
            .bearer()
            .ok_or_else(|| Response::json(401, error_json("missing bearer token").encode()))?;
        let bytes = hex_decode(token)
            .map_err(|_| Response::json(401, error_json("malformed bearer token").encode()))?;
        verify_token(&self.provider, &self.issuer_vk, &bytes, now).map_err(|e| {
            Response::json(401, error_json(&format!("token rejected: {e:?}")).encode())
        })
    }

    /// The (resource id, marking level, action) a request targets.
    fn resource_action(&self, req: &Request) -> (String, ClassificationLevel, &'static str) {
        match (req.method.as_str(), req.path.as_str()) {
            ("POST", "/api/v1/query") | ("POST", "/api/v1/search-around") => {
                let ot = parse(&req.body)
                    .ok()
                    .and_then(|b| b.str_field("objectType").map(str::to_string))
                    .unwrap_or_default();
                let level = self.marking_for(&ot);
                (format!("object-type:{ot}"), level, "Read")
            }
            ("POST", "/api/v1/actions") => {
                (req.path.clone(), self.marking_for("__actions__"), "Apply")
            }
            // Asking for camera analytics runs a model and writes to Heimdall: an action.
            // [FR-P3-5]
            ("POST", path)
                if camera_analytics::analyse_target(path).is_some()
                    || camera_analytics::watch_target(path).is_some() =>
            {
                (
                    path.to_string(),
                    self.marking_for(camera_analytics::CAMERAS_PATH),
                    "Apply",
                )
            }
            ("POST", heimdall_bridge::ASK_PATH) => (
                heimdall_bridge::ASK_PATH.to_string(),
                self.marking_for(heimdall_bridge::ALERTS_PATH),
                "Read",
            ),
            ("POST", path) if heimdall_bridge::advise_target(path).is_some() => (
                path.to_string(),
                self.marking_for(heimdall_bridge::ALERTS_PATH),
                "Read",
            ),
            ("GET", path) if camera_analytics::trend_target(path).is_some() => (
                path.to_string(),
                self.marking_for(camera_analytics::CAMERAS_PATH),
                "Read",
            ),
            // Acknowledging a Heimdall alert changes it: a write, like an action.
            // [FR-H-4]
            ("POST", path) if heimdall_bridge::ack_target(path).is_some() => (
                path.to_string(),
                self.marking_for(heimdall_bridge::ALERTS_PATH),
                "Apply",
            ),
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

/// Serve the development gateway — seeded, in memory, one built-in login — on `addr`
/// until the process is killed. [RC-24]
pub fn serve(addr: &str) -> std::io::Result<()> {
    serve_gateway(addr, SecuredGateway::dev())
}

/// Bind `addr` (loopback-only in the interim, before PQ-mTLS/TLS termination fronts
/// this) and serve `gateway` until the process is killed — the development gateway,
/// or one opened over a state directory by [`boot::open_state_dir`]. [RC-24]
pub fn serve_gateway<S, L>(addr: &str, gateway: SecuredGateway<S, L>) -> std::io::Result<()>
where
    S: ObjectStore + Send + 'static,
    L: LedgerStore + Send + 'static,
{
    serve_shared(addr, Arc::new(gateway))
}

/// [`serve_gateway`] over a gateway another listener — the mesh door — also serves.
pub fn serve_shared<S, L>(addr: &str, gw: Arc<SecuredGateway<S, L>>) -> std::io::Result<()>
where
    S: ObjectStore + Send + 'static,
    L: LedgerStore + Send + 'static,
{
    if !(addr.starts_with("127.0.0.1")
        || addr.starts_with("localhost")
        || addr.starts_with("[::1]"))
    {
        eprintln!(
            "http-gateway: WARNING binding non-loopback {addr}; production MUST front this with \
             PQ-mTLS/TLS-1.3 termination that forwards to the enclosure. [RC-24]"
        );
    }
    let listener = TcpListener::bind(addr)?;
    eprintln!("http-gateway (secured) listening on http://{addr}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let gw = Arc::clone(&gw);
                thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, &gw) {
                        eprintln!("http-gateway: connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("http-gateway: accept error: {e}"),
        }
    }
    Ok(())
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
) -> std::io::Result<()> {
    match read_request(&stream)? {
        Some(req) => gw.handle(&req, now_secs()).write(&mut stream),
        None => Ok(()),
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
/// and to run each seeded fabric action. Public so a test can assemble the same
/// governance over a durable ledger. [RC-37]
pub fn dev_login_policy() -> PolicySet {
    let rule = |id: String, action: &str| Rule {
        id: RuleId(id),
        role: Role("operator".into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    };
    let mut rules = vec![
        rule("operator-read".into(), "Read"),
        rule("operator-apply".into(), "Apply"),
    ];
    // The fabric authorises an action on the action's own api name, not on the edge's
    // generic "Apply" — without one rule per action, every action is denied.
    rules.extend(
        enclosure::DEV_ACTIONS
            .iter()
            .map(|a| rule(format!("operator-{}", a.id), a.api_name)),
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

    fn login(gw: &SecuredGateway) -> String {
        let login = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
        let resp = gw.handle(&request("POST", "/api/v1/login", &login, None), now_secs());
        assert_eq!(resp.status, 200, "{}", resp.body);
        parse(&resp.body)
            .ok()
            .and_then(|j| j.str_field("token").map(str::to_string))
            .expect("login returns a token")
    }

    /// A Heimdall reader identity, as `mg-admin mesh issue` writes it, in a fresh dir.
    fn heimdall_reader() -> service_runtime::ServiceIdentity {
        use attest::Keystore;
        use pq_mtls::{encode_anchor, encode_cert, ServiceId, WorkloadIdentity};
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir = std::env::temp_dir().join(format!(
            "http-gateway-heimdall-{}-{uniq}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let idp = iam::Idp::generate("mesh-idp", &mut OsRng).unwrap();
        let service = "picture-analyst";
        let key = Keystore::generate(service, iam::IDP_SIG_ALG, &mut OsRng).unwrap();
        let audit =
            Keystore::generate(&format!("{service}-audit"), iam::IDP_SIG_ALG, &mut OsRng).unwrap();
        let cert = idp
            .certify(&WorkloadIdentity {
                id: ServiceId(service.into()),
                roles: [Role("picture-reader".into())].into_iter().collect(),
                sig_alg: iam::IDP_SIG_ALG,
                verifying_key: key.verifying_key.clone(),
            })
            .unwrap();
        key.write(&dir.join(format!("{service}.keystore"))).unwrap();
        audit
            .write(&dir.join(format!("{service}.audit.keystore")))
            .unwrap();
        std::fs::write(dir.join(format!("{service}.cert")), encode_cert(&cert)).unwrap();
        std::fs::write(dir.join("mesh.anchor"), encode_anchor(&idp.trust_anchor())).unwrap();
        let identity = service_runtime::ServiceIdentity::load(&dir, service).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        identity
    }

    /// The history route is authorised and audited like the picture: bad parameters are
    /// a 400 and an unreachable Heimdall a 502, both after the read was audited; without
    /// Heimdall it is a 404; without a token a 401. [FR-H-1] [NFR-SEC-1]
    #[test]
    fn the_history_route_maps_bad_params_and_an_unreachable_heimdall() {
        let get = |gw: &SecuredGateway, query: &str, token: Option<&str>| {
            let mut req = request("GET", "/api/v1/heimdall/history", "", token);
            req.query = query.to_string();
            gw.handle(&req, now_secs())
        };
        let unconnected = SecuredGateway::dev();
        let token = login(&unconnected);
        assert_eq!(get(&unconnected, "", None).status, 401);
        assert_eq!(get(&unconnected, "", Some(&token)).status, 404);

        // Nothing listens on port 1.
        let gw = SecuredGateway::dev().with_heimdall(heimdall_bridge::HeimdallBridge::new(
            heimdall_reader(),
            "127.0.0.1:1",
            "situational-awareness",
        ));
        let token = login(&gw);
        let before = gw.audit_len();
        let bad = get(&gw, "from=200&to=100", Some(&token));
        assert_eq!(bad.status, 400, "{}", bad.body);
        let bad = get(&gw, "step=often", Some(&token));
        assert_eq!(bad.status, 400, "{}", bad.body);
        let down = get(&gw, "from=100&to=200&layer=air", Some(&token));
        assert_eq!(down.status, 502, "{}", down.body);
        assert!(down.body.contains("unreachable"), "{}", down.body);
        assert_eq!(gw.audit_len(), before + 3, "each read was audited");
    }

    /// The alert routes: the read is authorised and audited like the picture and its
    /// `state` checked; an acknowledgement is authorised and audited as `Apply` (a
    /// reader without it is refused, audited), a bad id is a 400 and an unreachable
    /// Heimdall a 502, all after auditing; without Heimdall a 404; without a token a
    /// 401. [FR-H-4] [NFR-SEC-1]
    #[test]
    fn the_alert_routes_authorise_audit_and_map_errors() {
        let call =
            |gw: &SecuredGateway, method: &str, path: &str, query: &str, token: Option<&str>| {
                let mut req = request(method, path, "", token);
                req.query = query.to_string();
                gw.handle(&req, now_secs())
            };
        let ack = "/api/v1/heimdall/alerts/A-1/ack";
        let unconnected = SecuredGateway::dev();
        let token = login(&unconnected);
        assert_eq!(
            call(&unconnected, "GET", "/api/v1/heimdall/alerts", "", None).status,
            401
        );
        assert_eq!(call(&unconnected, "POST", ack, "", None).status, 401);
        assert_eq!(
            call(
                &unconnected,
                "GET",
                "/api/v1/heimdall/alerts",
                "",
                Some(&token)
            )
            .status,
            404
        );
        assert_eq!(
            call(&unconnected, "POST", ack, "", Some(&token)).status,
            404
        );

        let gw = SecuredGateway::dev().with_heimdall(heimdall_bridge::HeimdallBridge::new(
            heimdall_reader(),
            "127.0.0.1:1",
            "situational-awareness",
        ));
        let token = login(&gw);
        let before = gw.audit_len();
        let bad = call(
            &gw,
            "GET",
            "/api/v1/heimdall/alerts",
            "state=closed",
            Some(&token),
        );
        assert_eq!(bad.status, 400, "{}", bad.body);
        let down = call(
            &gw,
            "GET",
            "/api/v1/heimdall/alerts",
            "state=all",
            Some(&token),
        );
        assert_eq!(down.status, 502, "{}", down.body);
        assert!(down.body.contains("unreachable"), "{}", down.body);
        let bad = call(
            &gw,
            "POST",
            "/api/v1/heimdall/alerts/A%201/ack",
            "",
            Some(&token),
        );
        assert_eq!(bad.status, 400, "{}", bad.body);
        let down = call(&gw, "POST", ack, "", Some(&token));
        assert_eq!(down.status, 502, "{}", down.body);
        assert_eq!(gw.audit_len(), before + 4, "each request was audited");
        assert_eq!(
            call(&gw, "GET", ack, "", Some(&token)).status,
            404,
            "an acknowledgement is a POST"
        );

        // A user the policy lets Read but not Apply may read alerts but not
        // acknowledge them.
        let idp = iam::Idp::generate("ro-idp", &mut OsRng).unwrap();
        let read_only_policy = PolicySet::new(vec![Rule {
            id: RuleId("reader".into()),
            role: Role("operator".into()),
            action: Action("Read".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap();
        let mut read_only = SecuredGateway::new(
            fips_provider(),
            idp.verifying_key().clone(),
            Governance::new(DefaultPdp::new(read_only_policy), &mut OsRng).unwrap(),
            vec![],
        )
        .with_idp(idp, Pbkdf2Hasher::new(1024))
        .with_heimdall(heimdall_bridge::HeimdallBridge::new(
            heimdall_reader(),
            "127.0.0.1:1",
            "situational-awareness",
        ));
        read_only
            .provision_login(DEMO_USERNAME, DEMO_PASSWORD, demo_operator())
            .unwrap();
        let token = login(&read_only);
        let before = read_only.audit_len();
        let read = call(
            &read_only,
            "GET",
            "/api/v1/heimdall/alerts",
            "",
            Some(&token),
        );
        assert_eq!(
            read.status, 502,
            "permitted to read; Heimdall is down: {}",
            read.body
        );
        let refused = call(&read_only, "POST", ack, "", Some(&token));
        assert_eq!(refused.status, 403, "{}", refused.body);
        assert_eq!(read_only.audit_len(), before + 2, "the refusal was audited");
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
}
