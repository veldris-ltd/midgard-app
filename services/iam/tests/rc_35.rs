//! RC-35 — Given the mesh, when a workload cert nears expiry, then it is renewed
//! without downtime, and revocation is honoured on the next handshake. Exercised over
//! a **real PQ-mTLS mutual handshake**: a renewed cert establishes a session, the old
//! cert keeps working until revoked, and a revoked cert is rejected by the very next
//! handshake. [RC-35]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;

use iam::{CertStatus, Idp, MeshCa, IDP_SIG_ALG};
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{
    mutual_handshake, IdentityCert, Initiator, MtlsError, Posture, Responder, ServiceId,
    TrustAnchor, WorkloadIdentity,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{Rng, SigningKey};

const NOW: u64 = 1_000;
const TTL: u64 = 3_600;

struct SeedRng(u64);
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

/// A workload keypair + identity (roles), NOT yet certified.
fn workload(id: &str, roles: &[&str], seed: u64) -> (WorkloadIdentity, SigningKey) {
    let (vk, sk) = fips_provider()
        .sig_for(IDP_SIG_ALG)
        .unwrap()
        .keypair(&mut SeedRng(seed))
        .unwrap();
    let identity = WorkloadIdentity {
        id: ServiceId(id.into()),
        roles: roles.iter().map(|r| Role(r.to_string())).collect(),
        sig_alg: IDP_SIG_ALG,
        verifying_key: vk,
    };
    (identity, sk)
}

fn channel_resource() -> Resource {
    Resource {
        id: ResourceId("channel".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

fn permit_pdp(role: &str) -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("r".into()),
            role: Role(role.into()),
            action: Action("OpenChannel".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    )
}

fn posture() -> Posture {
    Posture {
        level: ClassificationLevel::UNCLASSIFIED,
        compartments: BTreeSet::new(),
    }
}

/// Run a full mutual PQ-mTLS handshake: `client_cert` (signed by the CA) connecting
/// to a gateway that authenticates it against `responder_anchor` (carrying the CRL).
/// Returns the handshake result.
fn handshake(
    gw_cert: &IdentityCert,
    gw_sk: &SigningKey,
    client_cert: &IdentityCert,
    client_sk: &SigningKey,
    client_anchor: &TrustAnchor, // client trusts the CA (to accept the gateway)
    responder_anchor: &TrustAnchor, // gateway trusts the CA + honours the CRL
) -> Result<(), MtlsError> {
    let provider = fips_provider();
    let pdp = permit_pdp("client");
    let mut irng = SeedRng(100);
    let mut rrng = SeedRng(200);
    let mut init = Initiator::new(
        &provider,
        client_cert.clone(),
        client_sk,
        client_anchor.clone(),
        pq_mtls::ServiceId("svc-gateway".into()),
        Purpose("investigation".into()),
        posture(),
        &mut irng,
    )
    .at_time(NOW);
    let mut resp = Responder::new(
        &provider,
        gw_cert.clone(),
        gw_sk,
        responder_anchor.clone(),
        &pdp,
        channel_resource(),
        &mut rrng,
    )
    .at_time(NOW);
    mutual_handshake(&mut init, &mut resp).map(|_| ())
}

#[test]
fn renewal_is_downtime_free_and_revocation_is_honoured_on_next_handshake() {
    // The CA (IdP) and a gateway workload it certifies.
    let idp = Idp::generate("mesh-ca", &mut SeedRng(1)).unwrap();
    let mut ca = MeshCa::new(idp);
    let (gw_id, gw_sk) = workload("svc-gateway", &["server"], 2);
    let (gw_cert, _gw_serial) = ca.issue(&gw_id, NOW, NOW + TTL, posture()).unwrap();

    // A client workload gets its first cert (v1).
    let (client_v1_id, client_v1_sk) = workload("svc-client", &["client"], 3);
    let (client_v1, serial_v1) = ca.issue(&client_v1_id, NOW, NOW + TTL, posture()).unwrap();
    assert_eq!(ca.status(serial_v1, NOW), CertStatus::Valid);

    // Baseline: a mutual handshake with v1 succeeds.
    handshake(
        &gw_cert,
        &gw_sk,
        &client_v1,
        &client_v1_sk,
        &ca.trust_anchor(),
        &ca.trust_anchor(),
    )
    .expect("v1 handshake establishes a session");

    // ---- Renewal without downtime ----
    // The cert nears expiry; the CA renews it with a rekeyed identity (v2). The OLD
    // cert (v1) is untouched and still works — no session is dropped.
    let (client_v2_id, client_v2_sk) = workload("svc-client", &["client"], 4);
    let (client_v2, serial_v2) = ca
        .renew(&client_v2_id, NOW, NOW + 2 * TTL, posture())
        .unwrap();
    assert_ne!(serial_v1, serial_v2);

    // Both the renewed cert AND the still-valid old cert handshake successfully —
    // renewal introduced no downtime.
    handshake(
        &gw_cert,
        &gw_sk,
        &client_v2,
        &client_v2_sk,
        &ca.trust_anchor(),
        &ca.trust_anchor(),
    )
    .expect("renewed v2 handshake succeeds");
    handshake(
        &gw_cert,
        &gw_sk,
        &client_v1,
        &client_v1_sk,
        &ca.trust_anchor(),
        &ca.trust_anchor(),
    )
    .expect("old v1 still valid during overlap (no downtime)");

    // ---- Revocation honoured on the next handshake ----
    // Revoke v1 (e.g. the old key is retired or suspected). The CA's trust anchor now
    // carries v1's fingerprint on the CRL.
    ca.revoke(serial_v1).unwrap();
    assert_eq!(ca.status(serial_v1, NOW), CertStatus::Revoked);
    let anchor_after = ca.trust_anchor();

    // The very next handshake presenting v1 is REJECTED as revoked...
    let err = handshake(
        &gw_cert,
        &gw_sk,
        &client_v1,
        &client_v1_sk,
        &anchor_after,
        &anchor_after,
    )
    .expect_err("a revoked cert must be rejected on the next handshake");
    assert!(
        matches!(err, MtlsError::PeerCertRevoked),
        "expected PeerCertRevoked, got {err:?}"
    );

    // ...while the renewed v2 cert (a different key) is unaffected and still connects.
    handshake(
        &gw_cert,
        &gw_sk,
        &client_v2,
        &client_v2_sk,
        &anchor_after,
        &anchor_after,
    )
    .expect("revocation is targeted: the renewed cert still works");
}

#[test]
fn expiry_is_tracked_and_renewal_worklist_surfaces_expiring_certs() {
    let idp = Idp::generate("mesh-ca", &mut SeedRng(1)).unwrap();
    let mut ca = MeshCa::new(idp);
    let (id, _sk) = workload("svc-a", &["client"], 3);
    let (_cert, serial) = ca.issue(&id, NOW, NOW + 100, posture()).unwrap();

    assert_eq!(ca.status(serial, NOW), CertStatus::Valid);
    assert_eq!(ca.status(serial, NOW + 100), CertStatus::Expired);
    assert_eq!(ca.status(serial, NOW - 1), CertStatus::NotYetValid);
    assert_eq!(ca.status(999, NOW), CertStatus::Unknown);

    // The renewal worklist surfaces certs expiring by a horizon.
    assert_eq!(ca.expiring_by(NOW + 100), vec![serial]);
    assert!(ca.expiring_by(NOW + 50).is_empty());
    // Once revoked, a cert drops off the renewal worklist.
    ca.revoke(serial).unwrap();
    assert!(ca.expiring_by(NOW + 100).is_empty());
}

/// MG-SEC-03: verifiers elsewhere learn of a revocation only from the CA-signed list,
/// whose version moves forward with every revocation.
#[test]
fn the_signed_list_carries_the_revocation_to_other_verifiers() {
    let idp = Idp::generate("mesh-ca", &mut SeedRng(1)).unwrap();
    let mut ca = MeshCa::new(idp);
    let (gw_id, gw_sk) = workload("svc-gateway", &["server"], 2);
    let (gw_cert, _) = ca.issue(&gw_id, NOW, NOW + TTL, posture()).unwrap();
    let (cl_id, cl_sk) = workload("svc-client", &["client"], 3);
    let (cl_cert, serial) = ca.issue(&cl_id, NOW, NOW + TTL, posture()).unwrap();

    // A verifier that holds only the bare anchor.
    let remote = ca.idp().trust_anchor();
    let before = ca.signed_crl(NOW).unwrap();
    ca.revoke(serial).unwrap();
    let after = ca.signed_crl(NOW).unwrap();
    assert!(after.version > before.version);

    remote.install_crl(&after, ca.idp().provider()).unwrap();
    let err = handshake(&gw_cert, &gw_sk, &cl_cert, &cl_sk, &remote, &remote).unwrap_err();
    assert!(matches!(err, MtlsError::PeerCertRevoked), "{err:?}");
    // The pre-revocation list cannot be replayed to undo it.
    assert!(remote.install_crl(&before, ca.idp().provider()).is_err());
}

/// MG-SEC-02: a certificate issued for a window does not work outside it.
#[test]
fn an_expired_certificate_no_longer_handshakes() {
    let idp = Idp::generate("mesh-ca", &mut SeedRng(1)).unwrap();
    let mut ca = MeshCa::new(idp);
    let (gw_id, gw_sk) = workload("svc-gateway", &["server"], 2);
    let (gw_cert, _) = ca.issue(&gw_id, NOW - 500, NOW + TTL, posture()).unwrap();
    let (cl_id, cl_sk) = workload("svc-client", &["client"], 3);
    let (cl_cert, _) = ca.issue(&cl_id, NOW - 500, NOW, posture()).unwrap();
    let anchor = ca.trust_anchor();
    let err = handshake(&gw_cert, &gw_sk, &cl_cert, &cl_sk, &anchor, &anchor).unwrap_err();
    assert!(matches!(err, MtlsError::PeerCertExpired { .. }), "{err:?}");
}
