//! WebAuthn client data, parsed at the edge. A browser login handler reads the raw
//! `clientDataJSON` of an assertion with [`parse_client_data`] and hands `iam` the
//! typed [`iam::ClientData`] alongside the raw bytes (which stay what the signature
//! covers). The JSON is read by the shared, depth-limited parser and the strict
//! readers, so `iam` holds no JSON parser and nothing is found by substring: a
//! `"webauthn.get"` in some other field, or a challenge nested in an object, is not
//! taken for the real one. No route calls this yet; no browser flow is wired.
//! [VLD-ANB-MIDGARD-0084] [NFR-SEC-2]

use iam::enc::unb64url;
use iam::ClientData;
use service_runtime::json::parse;
use service_runtime::strict_json::{object, only_keys, optional_bool, text};

/// The keys `CollectedClientData` may carry (WebAuthn Level 2 section 5.8.1).
const KEYS: [&str; 5] = ["type", "challenge", "origin", "crossOrigin", "tokenBinding"];

/// Parse an assertion's raw `clientDataJSON`. It must be a UTF-8 JSON object with no
/// unknown or repeated key, `type` exactly `webauthn.get`, a base64url `challenge`
/// and a text `origin`. A ceremony run in a cross-origin frame is refused: no Midgard
/// page embeds one. `tokenBinding` is allowed and ignored. [VLD-ANB-MIDGARD-0084]
pub fn parse_client_data(raw: &[u8]) -> Result<ClientData, String> {
    const AT: &str = "clientDataJSON";
    let raw = std::str::from_utf8(raw).map_err(|_| format!("{AT} is not UTF-8"))?;
    let doc = parse(raw).map_err(|e| format!("{AT}: {e}"))?;
    only_keys(object(&doc, AT)?, AT, &KEYS)?;
    let type_ = text(&doc, AT, "type")?;
    if type_ != "webauthn.get" {
        return Err(format!("{AT}: \"type\" must be \"webauthn.get\""));
    }
    let challenge = unb64url(text(&doc, AT, "challenge")?)
        .ok_or_else(|| format!("{AT}: \"challenge\" is not base64url"))?;
    let origin = text(&doc, AT, "origin")?;
    if optional_bool(&doc, AT, "crossOrigin", false)? {
        return Err(format!("{AT}: a cross-origin ceremony is not accepted"));
    }
    Ok(ClientData {
        type_: type_.to_string(),
        challenge,
        origin: origin.to_string(),
    })
}
