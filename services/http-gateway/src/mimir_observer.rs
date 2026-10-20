//! The Mimir console's live view of the inference plane. [FR-MI-8] [FR-MI-1]
//!
//! With `--mimir-observe-addr`, signed-in users the policy lets `Read` it get
//! `GET /api/v1/mimir/models` (what is served: the signed manifest, the sidecar's
//! health and context, throughput from recent calls) and `GET /api/v1/mimir/sessions`
//! (the recent inference calls, each joined to its PQ-signed ledger entry, and the
//! newest ledger entries). The gateway reads them from `mimir-server`'s observer port
//! over PQ-mTLS as its own certified identity (role `inference-observer`), and Mimir
//! authorises and audits every read.

use service_runtime::client;
use service_runtime::json::{parse, Json};
use service_runtime::ServiceIdentity;

use crate::enclosure::EdgeError;

/// The route prefix.
pub const MIMIR_PATH: &str = "/api/v1/mimir";
/// What is served.
pub const MODELS_PATH: &str = "/api/v1/mimir/models";
/// Recent calls and the ledger.
pub const SESSIONS_PATH: &str = "/api/v1/mimir/sessions";

/// A reader of Mimir's observer port.
pub struct MimirObserver {
    identity: ServiceIdentity,
    addr: String,
    purpose: String,
}

impl MimirObserver {
    pub fn new(identity: ServiceIdentity, addr: &str, purpose: &str) -> Self {
        Self {
            identity,
            addr: addr.to_string(),
            purpose: purpose.to_string(),
        }
    }

    /// `DescribeModels`.
    pub fn models(&self) -> Result<Json, EdgeError> {
        self.call("DescribeModels", "{}")
    }

    /// `ReadSessions`, the newest `limit` calls (1 to 500).
    pub fn sessions(&self, limit: usize) -> Result<Json, EdgeError> {
        self.call(
            "ReadSessions",
            &Json::obj(vec![("limit", Json::Int(limit.clamp(1, 500) as i64))]).encode(),
        )
    }

    fn call(&self, method: &str, body: &str) -> Result<Json, EdgeError> {
        let raw = client::call(
            &self.identity,
            &self.addr,
            &self.purpose,
            method,
            body.as_bytes(),
        )
        .map_err(|e| EdgeError::new(502, &format!("Mimir is unreachable: {e}")))?;
        let json = parse(&String::from_utf8_lossy(&raw))
            .map_err(|e| EdgeError::new(502, &format!("Mimir sent malformed JSON: {e}")))?;
        match json.str_field("error") {
            Some(e) => Err(EdgeError::new(502, &format!("Mimir refused the read: {e}"))),
            None => Ok(json),
        }
    }
}

/// The `limit` query value for sessions (default 100).
pub fn sessions_limit(raw: Option<&str>) -> Result<usize, EdgeError> {
    match raw {
        None => Ok(100),
        Some(v) => v
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=500).contains(n))
            .ok_or_else(|| EdgeError::bad_request("limit must be 1 to 500")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sessions_limit_is_one_to_five_hundred() {
        assert_eq!(sessions_limit(None).unwrap(), 100);
        assert_eq!(sessions_limit(Some("20")).unwrap(), 20);
        for bad in ["0", "501", "x"] {
            assert_eq!(sessions_limit(Some(bad)).unwrap_err().status, 400);
        }
    }
}
