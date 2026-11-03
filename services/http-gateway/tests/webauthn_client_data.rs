//! WebAuthn client data is parsed at the edge with the shared JSON parser, not
//! matched by substring: the type and challenge read are the document's own, and an
//! assertion whose client data does not parse, or is not a `webauthn.get`, is refused
//! as bad client data. [AC-58.4] [VLD-ANB-MIDGARD-0084]
#![cfg(feature = "backend-fips")]

use http_gateway::webauthn_client_data::parse_client_data;
use iam::enc::b64url;
use iam::{
    AuthAssertion, ClientData, WebAuthnCredential, WebAuthnError, WebAuthnPolicy, PASSKEY_SIG_ALG,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

const ORIGIN: &str = "https://console.enclosure.local";
const CHALLENGE: &[u8] = b"server-issued-challenge-bytes";

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

/// What a browser login handler does with an assertion: parse its client data at the
/// edge (a document that does not parse is bad client data), then verify it with a
/// passkey that signed exactly `raw`.
fn verify_at_the_edge(raw: &[u8]) -> Result<u32, WebAuthnError> {
    let provider = fips_provider();
    let sig = provider.sig_for(PASSKEY_SIG_ALG).unwrap();
    let (vk, sk) = sig.keypair(&mut SeedRng(58)).unwrap();
    let cred = WebAuthnCredential {
        credential_id: b"cred-1".to_vec(),
        public_key: vk,
        rp_id: "console.enclosure.local".into(),
        sign_count: 0,
    };
    let client_data = parse_client_data(raw).map_err(|_| WebAuthnError::BadClientData)?;
    let authenticator_data = cred.authenticator_data(0x01 | 0x04, 1);
    let payload = WebAuthnCredential::signed_payload(&authenticator_data, raw);
    let assertion = AuthAssertion {
        credential_id: cred.credential_id.clone(),
        authenticator_data,
        client_data_json: raw.to_vec(),
        client_data,
        signature: sig.sign(&sk, &payload).unwrap(),
    };
    let policy = WebAuthnPolicy {
        origins: vec![ORIGIN.into()],
        require_uv: true,
    };
    cred.verify_assertion(&provider, &policy, CHALLENGE, &assertion)
}

/// [AC-58.4] [VLD-ANB-MIDGARD-0084] A `webauthn.create` whose other field contains the
/// text "webauthn.get", a challenge nested in an object, and an unknown key are each
/// refused, and verification of each fails as bad client data; a valid document
/// round-trips to the type, challenge and origin it holds, and verifies.
#[test]
fn client_data_type_and_challenge_are_read_from_parsed_json() {
    let challenge = b64url(CHALLENGE);

    // The type is "webauthn.create"; the text "webauthn.get" sits in another field.
    let create = format!(
        r#"{{"type":"webauthn.create","challenge":"{challenge}","origin":"{ORIGIN}","tokenBinding":{{"status":"\"type\":\"webauthn.get\""}}}}"#
    );
    assert!(parse_client_data(create.as_bytes())
        .unwrap_err()
        .contains("webauthn.get"));
    assert_eq!(
        verify_at_the_edge(create.as_bytes()),
        Err(WebAuthnError::BadClientData)
    );
    // The challenge is a nested object holding a challenge string.
    let nested = format!(
        r#"{{"type":"webauthn.get","challenge":{{"challenge":"{challenge}"}},"origin":"{ORIGIN}"}}"#
    );
    assert!(parse_client_data(nested.as_bytes())
        .unwrap_err()
        .contains("\"challenge\" must be a string"));
    assert_eq!(
        verify_at_the_edge(nested.as_bytes()),
        Err(WebAuthnError::BadClientData)
    );

    // An unknown key.
    let unknown = format!(
        r#"{{"type":"webauthn.get","challenge":"{challenge}","origin":"{ORIGIN}","extra":"x"}}"#
    );
    assert!(parse_client_data(unknown.as_bytes())
        .unwrap_err()
        .contains("unknown key \"extra\""));

    // A repeated key, a cross-origin ceremony and a non-JSON document are refused too.
    let repeated = format!(
        r#"{{"type":"webauthn.create","type":"webauthn.get","challenge":"{challenge}","origin":"{ORIGIN}"}}"#
    );
    assert!(parse_client_data(repeated.as_bytes()).is_err());
    let cross = format!(
        r#"{{"type":"webauthn.get","challenge":"{challenge}","origin":"{ORIGIN}","crossOrigin":true}}"#
    );
    assert!(parse_client_data(cross.as_bytes()).is_err());
    assert!(parse_client_data(b"\"type\":\"webauthn.get\"").is_err());
    assert!(parse_client_data(&[0xFF, 0xFE]).is_err());

    // A valid document, as a browser writes it, round-trips and verifies.
    let valid = WebAuthnCredential::client_data_json(CHALLENGE, ORIGIN);
    assert_eq!(
        parse_client_data(&valid),
        Ok(ClientData {
            type_: "webauthn.get".into(),
            challenge: CHALLENGE.to_vec(),
            origin: ORIGIN.into(),
        })
    );
    assert_eq!(verify_at_the_edge(&valid), Ok(1));
    let with_extras = format!(
        r#"{{"type":"webauthn.get","challenge":"{challenge}","origin":"{ORIGIN}","crossOrigin":false}}"#
    );
    assert_eq!(verify_at_the_edge(with_extras.as_bytes()), Ok(1));
}
