//! A session can be ended at the server. `POST /api/v1/logout` revokes the token it
//! is called with, and from then on that token is refused like a forged one, here and
//! after a restart of a gateway serving a state directory, whose revocations are
//! journaled under the issuer key. A token whose subject is no longer enrolled is
//! refused too. Revocations are purged once the token they name has expired.
//! [RC-37] [RC-29] [NFR-SEC-2] [AC-M-4] [VLD-ANB-MIDGARD-0021]
#![cfg(feature = "backend-fips")]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use attest::keystore::test_support::test_passphrase;
use attest::{Keystore, RootWrap};
use http_gateway::boot::{open_state_dir, DurableGateway};
use http_gateway::http::{Request, Response};
use http_gateway::json::parse;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, AUDIT_KEY_ID, IDP_KEY_ID};
use http_gateway::{demo_operator, SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};
use iam::{Credentials, DurableDirectory, Enrollment, PasswordHasher, Pbkdf2Hasher};
use kms::{Kms, KmsSealer};
use policy::{Action, Effect, PolicySet, Role, Rule, RuleId, Selector};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

const NOW: u64 = 1_000_000;
const TOKEN_TTL: u64 = 3_600;
const PASSWORD: &str = "correct horse battery staple";

fn call<S, L>(
    gw: &SecuredGateway<S, L>,
    method: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
    now: u64,
) -> Response
where
    S: fabric::ObjectStore,
    L: fabric::LedgerStore,
{
    gw.handle(
        &Request {
            method: method.into(),
            path: path.into(),
            query: if path == "/api/v1/audit" {
                "limit=200".into()
            } else {
                String::new()
            },
            body: body.into(),
            authorization: token.map(|t| format!("Bearer {t}")),
            origin: None,
        },
        now,
    )
}

fn login<S, L>(gw: &SecuredGateway<S, L>, username: &str, password: &str, now: u64) -> String
where
    S: fabric::ObjectStore,
    L: fabric::LedgerStore,
{
    let body = format!(r#"{{"username":"{username}","password":"{password}"}}"#);
    let r = call(gw, "POST", "/api/v1/login", &body, None, now);
    assert_eq!(r.status, 200, "login: {}", r.body);
    parse(&r.body)
        .ok()
        .and_then(|j| j.str_field("token").map(str::to_string))
        .expect("login returns a token")
}

fn error_of(r: &Response) -> String {
    parse(&r.body)
        .ok()
        .and_then(|j| j.str_field("error").map(str::to_string))
        .unwrap_or_default()
}

/// AC-14.1: after sign-out the same token, reused verbatim at the same instant, is
/// refused with "token revoked", and the ledger holds the sign-out.
#[test]
fn a_token_is_refused_after_sign_out() {
    let gw = SecuredGateway::dev();
    let token = login(&gw, DEMO_USERNAME, DEMO_PASSWORD, NOW);
    assert_eq!(
        call(&gw, "GET", "/api/v1/whoami", "", Some(&token), NOW).status,
        200
    );

    let out = call(&gw, "POST", "/api/v1/logout", "", Some(&token), NOW);
    assert_eq!(out.status, 204, "{}", out.body);
    assert!(out.body.is_empty(), "a sign-out has no body");

    let after = call(&gw, "GET", "/api/v1/whoami", "", Some(&token), NOW);
    assert_eq!(after.status, 401, "{}", after.body);
    assert_eq!(error_of(&after), "token revoked");
    // Every other route refuses it too, and a second sign-out is just another use.
    let query = r#"{"objectType":"aircraft"}"#;
    assert_eq!(
        call(&gw, "POST", "/api/v1/query", query, Some(&token), NOW).status,
        401
    );
    assert_eq!(
        call(&gw, "POST", "/api/v1/logout", "", Some(&token), NOW).status,
        401
    );
    assert_eq!(gw.revoked_count(), 1);
    assert_eq!(
        gw.revocation_journal_len(),
        None,
        "a dev gateway holds it in memory"
    );

    // A new session is unaffected, and the ledger names the token that was ended.
    let fresh = login(&gw, DEMO_USERNAME, DEMO_PASSWORD, NOW);
    let audit = call(&gw, "GET", "/api/v1/audit", "", Some(&fresh), NOW);
    assert_eq!(audit.status, 200, "{}", audit.body);
    assert_eq!(
        audit
            .body
            .matches(&format!(
                r#""actor":"{DEMO_USERNAME}","action":"LOGOUT","resource":"token:"#
            ))
            .count(),
        1,
        "{}",
        audit.body
    );
    assert!(
        gw.metrics_text().contains("token revoked"),
        "the refusal is counted by reason"
    );
}

/// AC-14.4: a token for a principal who is no longer enrolled is refused, and the
/// reason is "subject not enrolled".
#[test]
fn a_token_for_a_removed_subject_is_refused() {
    let gw = SecuredGateway::dev();
    let token = login(&gw, DEMO_USERNAME, DEMO_PASSWORD, NOW);
    assert_eq!(
        call(&gw, "GET", "/api/v1/whoami", "", Some(&token), NOW).status,
        200
    );

    assert!(gw.remove_login(DEMO_USERNAME));
    let r = call(&gw, "GET", "/api/v1/whoami", "", Some(&token), NOW);
    assert_eq!(r.status, 401, "{}", r.body);
    assert_eq!(error_of(&r), "subject not enrolled");
    assert!(gw
        .metrics_text()
        .contains(r#"http_session_refusals_total{reason="subject not enrolled"} 1"#));
}

/// Only the holder of a valid token can sign out: no token, a forged one or a
/// malformed one is a 401 and revokes nothing.
#[test]
fn logout_without_a_valid_token_is_401() {
    let gw = SecuredGateway::dev();
    let audit_before = gw.audit_len();
    for token in [None, Some("deadbeef"), Some("not-hex")] {
        let r = call(&gw, "POST", "/api/v1/logout", "", token, NOW);
        assert_eq!(r.status, 401, "{token:?}: {}", r.body);
    }
    assert_eq!(gw.revoked_count(), 0);
    // An unauthenticated refusal writes at most the one rate-limited
    // `AUTH rejected (<class>)` row a minute per source. [VLD-ANB-MIDGARD-0054]
    assert!(
        gw.audit_len() <= audit_before + 1,
        "a refused sign-out writes no row of its own"
    );
}

// ---- a gateway serving a state directory -----------------------------------------

fn scratch(tag: &str) -> PathBuf {
    let uniq = Box::into_raw(Box::new(0u8)) as usize;
    std::env::temp_dir().join(format!(
        "session-revocation-{tag}-{}-{}",
        std::process::id(),
        uniq
    ))
}

/// The wall clock: a state directory purges expired revocations by it as it opens.
fn wall_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs()
}

/// A state directory as `mg-admin init` leaves it, with operators allowed to read and
/// one principal, `olivia`, enrolled as an operator.
fn provision(dir: &Path) -> StateDir {
    let state = StateDir::new(dir);
    std::fs::create_dir_all(state.keys()).unwrap();
    std::fs::create_dir_all(state.trust()).unwrap();
    let idp = Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut OsRng).unwrap();
    let audit = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut OsRng).unwrap();
    idp.write(
        &state.idp_keystore(),
        &RootWrap::Passphrase(&test_passphrase()),
        &mut OsRng,
    )
    .unwrap();
    audit
        .write(
            &state.audit_keystore(),
            &RootWrap::Passphrase(&test_passphrase()),
            &mut OsRng,
        )
        .unwrap();
    http_gateway::boot::init_audit_ledger(&state, &RootWrap::Passphrase(&test_passphrase()))
        .unwrap();

    let policy = PolicySet::new(vec![Rule {
        id: RuleId("operator-read".into()),
        role: Role("operator".into()),
        action: Action("Read".into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    }])
    .unwrap();
    std::fs::write(state.policy(), render_policy(&policy).unwrap()).unwrap();

    let sealer = Box::new(KmsSealer::new(Kms::new(fips_provider(), idp.trust_root())));
    let mut directory = DurableDirectory::open(state.directory(), sealer).unwrap();
    directory
        .enroll(
            "olivia",
            Enrollment {
                user: demo_operator(),
                credentials: Credentials {
                    password_hash: Pbkdf2Hasher::production().hash(PASSWORD.as_bytes()),
                    ..Credentials::default()
                },
            },
        )
        .unwrap();
    state
}

fn pin(state: &StateDir) -> audit_writer::PinnedKey {
    audit_writer::PinnedKey::new(
        Keystore::read(
            &state.audit_keystore(),
            &RootWrap::Passphrase(&test_passphrase()),
        )
        .unwrap()
        .verifying_key,
    )
}

fn open(state: &StateDir) -> (DurableGateway, http_gateway::state_dir::StateLock) {
    open_state_dir(
        state,
        vec![],
        &pin(state),
        &RootWrap::Passphrase(&test_passphrase()),
    )
    .expect("the state directory opens")
    .into_parts()
}

/// AC-14.2: a sign-out survives a restart of a durable gateway, and the revocation is
/// purged once its token has expired.
#[test]
fn revocation_survives_a_restart_and_expired_entries_are_purged() {
    let dir = scratch("restart");
    let state = provision(&dir);
    let now = wall_clock();

    let token = {
        let (gw, _lock) = open(&state);
        assert_eq!(gw.revocation_journal_len(), Some(0));
        let token = login(&gw, "olivia", PASSWORD, now);
        let out = call(&gw, "POST", "/api/v1/logout", "", Some(&token), now);
        assert_eq!(out.status, 204, "{}", out.body);
        token
    };

    let (gw, _lock) = open(&state);
    assert_eq!(
        gw.revocation_journal_len(),
        Some(1),
        "one record, journaled"
    );
    assert_eq!(
        gw.revoked_count(),
        1,
        "and replayed: its token has not expired"
    );
    let r = call(&gw, "GET", "/api/v1/whoami", "", Some(&token), now);
    assert_eq!(r.status, 401, "{}", r.body);
    assert_eq!(error_of(&r), "token revoked");

    // Once the token has expired it is refused by its own window, and its revocation
    // is no longer needed: the next revocation check, made for another session of the
    // same principal, purges it.
    let later = now + TOKEN_TTL + 1;
    let r = call(&gw, "GET", "/api/v1/whoami", "", Some(&token), later);
    assert_eq!(r.status, 401, "{}", r.body);
    assert_eq!(error_of(&r), "token rejected", "{}", r.body);
    let fresh = login(&gw, "olivia", PASSWORD, later);
    assert_eq!(
        call(&gw, "GET", "/api/v1/whoami", "", Some(&fresh), later).status,
        200
    );
    assert_eq!(gw.revoked_count(), 0, "expired revocations are purged");
    assert_eq!(gw.revocation_journal_len(), Some(1));
    drop(gw);
    let _ = std::fs::remove_dir_all(&dir);
}
