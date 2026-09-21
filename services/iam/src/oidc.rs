//! The OIDC-facing surface, scoped to what an **offline PQ enclosure** needs. [NFR-SEC-2]
//!
//! - **JWKS** ([`jwks_json`]): publishes the IdP's ML-DSA issuer verifying key so any
//!   relying party can verify tokens with only public material — the standard JWKS
//!   role, over the PQ key type. This is the endpoint `Idp::verifying_key` feeds.
//! - **Token endpoint**: the `/token` exchange is [`crate::auth::Authenticator`] —
//!   verify credentials + MFA, return a PQ-signed token. Federated `/token` is
//!   [`crate::federation::federated_login`].
//!
//! **SAD rescope (explicit):** the browser-redirect `/authorize` leg (consent screen,
//! auth-code + PKCE round-trip) is deliberately *out of scope* for the enclosure IdP.
//! Midgard's first-party consoles authenticate directly against the token endpoint
//! over PQ-mTLS; there is no third-party OAuth client delegation to mediate. If a
//! deployment later needs third-party OAuth clients, `/authorize` + the code store
//! are added here without changing token issuance. The JWKS + token endpoints above
//! are the real, implemented surface. [RC-29]

use crate::enc::b64url;
use crate::idp::{Idp, IDP_SIG_ALG};

/// Publish the IdP's signing key as a JWKS document. `kty`/`alg` name the PQ scheme
/// (ML-DSA-65 / FIPS 204); `x` is the base64url public key; `kid` is the issuer id.
/// Relying parties fetch this to verify tokens. No serde — hand-built JSON.
pub fn jwks_json(idp: &Idp) -> String {
    let vk = idp.verifying_key();
    format!(
        "{{\"keys\":[{{\"kty\":\"ML-DSA\",\"alg\":\"{}\",\"use\":\"sig\",\"kid\":\"{}\",\"x\":\"{}\"}}]}}",
        IDP_SIG_ALG.as_str(),
        idp.issuer_id(),
        b64url(vk.as_bytes()),
    )
}
