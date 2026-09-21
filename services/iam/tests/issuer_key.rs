//! The IdP's issuer key survives a restart. An IdP rebuilt from a persisted keypair
//! verifies the tokens its previous incarnation issued, and a keypair that does not
//! belong together, or is not the IdP's algorithm, is refused when it is loaded
//! rather than when every token starts failing. [RC-29] [NFR-SEC-2]

use std::collections::BTreeSet;

use iam::{verify_token, Idp, UserRecord, IDP_SIG_ALG};
use policy::{ClassificationLevel, Clearance, Purpose, Role};
use pqc_agility::backend::fips::FipsSig;
use pqc_agility::{
    CryptoError, OsRng, SecretBytes, SigId, SignatureScheme, SigningKey, VerifyingKey,
};

const NOW: u64 = 1_800_000_000;
const PURPOSE: &str = "investigation";

fn analyst() -> UserRecord {
    UserRecord {
        roles: [Role("analyst".into())].into_iter().collect(),
        clearance: Clearance {
            level: ClassificationLevel::SECRET,
            compartments: BTreeSet::new(),
        },
        purposes: [Purpose(PURPOSE.into())].into_iter().collect(),
    }
}

#[test]
fn a_rebuilt_idp_verifies_tokens_issued_before_the_restart() {
    let (vk, sk) = FipsSig::new(IDP_SIG_ALG).keypair(&mut OsRng).unwrap();
    // What a keystore persists: the raw key bytes, reloaded on every boot.
    let vk_bytes = vk.as_bytes().to_vec();
    let sk_bytes = sk.expose().to_vec();
    let boot = || {
        Idp::from_key(
            "midgard-idp",
            VerifyingKey::new(IDP_SIG_ALG, vk_bytes.clone()),
            SigningKey::new(IDP_SIG_ALG, SecretBytes::new(sk_bytes.clone())),
        )
        .expect("the persisted issuer key loads")
    };

    let token = boot()
        .mint_token("alice", &analyst(), Purpose(PURPOSE.into()), NOW, 3_600)
        .expect("token minted before the restart");

    let restarted = boot();
    let claims = verify_token(
        restarted.provider(),
        restarted.verifying_key(),
        &token,
        NOW + 60,
    )
    .expect("a token issued before the restart still verifies");
    assert_eq!(claims.subject, "alice");
}

#[test]
fn a_keypair_that_does_not_belong_together_is_refused() {
    let scheme = FipsSig::new(IDP_SIG_ALG);
    let (vk_a, _) = scheme.keypair(&mut OsRng).unwrap();
    let (_, sk_b) = scheme.keypair(&mut OsRng).unwrap();
    assert!(matches!(
        Idp::from_key("midgard-idp", vk_a, sk_b),
        Err(CryptoError::InvalidKey)
    ));
}

#[test]
fn a_key_of_another_algorithm_is_refused() {
    let (vk, sk) = FipsSig::new(SigId::MlDsa87).keypair(&mut OsRng).unwrap();
    assert!(matches!(
        Idp::from_key("midgard-idp", vk, sk),
        Err(CryptoError::AlgorithmMismatch { .. })
    ));
}
