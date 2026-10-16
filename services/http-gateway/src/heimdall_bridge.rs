//! The browser's read path to Heimdall's common operating picture. [FR-H-1]
//! [NFR-SEC-1] [NFR-AI-1]
//!
//! Heimdall serves its picture only over PQ-mTLS, to workloads whose certificate holds a
//! role its policy lets read it. The gateway is the browser's one door, so it holds such
//! a certificate and asks for the picture on behalf of a signed-in user:
//!
//! 1. the user's request is authenticated, authorised (`Read` on
//!    `/api/v1/heimdall/picture`) and audited in the enclosure ledger, like any read;
//! 2. the gateway then calls `ReadPicture` on `heimdall-feed` as its own identity, which
//!    Heimdall authorises and audits in its own ledger.
//!
//! So both ledgers record the read: who asked, and which workload fetched it.

use service_runtime::client;
use service_runtime::json::{parse, Json};
use service_runtime::ServiceIdentity;

use crate::enclosure::EdgeError;

/// The `heimdall-feed` method that returns the picture.
const READ_PICTURE: &str = "ReadPicture";
/// Heimdall's outcome byte for a served request.
const ACCEPTED: u8 = 0;

/// Where the picture is and who the gateway is when it asks for it.
pub struct HeimdallBridge {
    identity: ServiceIdentity,
    addr: String,
    purpose: String,
}

impl HeimdallBridge {
    /// Read the picture from `heimdall-feed` at `addr` as `identity`, declaring `purpose`
    /// (the one purpose Heimdall's channel opens for).
    pub fn new(identity: ServiceIdentity, addr: &str, purpose: &str) -> Self {
        Self {
            identity,
            addr: addr.to_string(),
            purpose: purpose.to_string(),
        }
    }

    /// The picture as Heimdall serves it: `{"layers": [..], "entities": [..]}`. Heimdall
    /// being unreachable, or refusing the read, is a 502: the gateway is a client there.
    pub fn picture(&self) -> Result<Json, EdgeError> {
        let response =
            client::call(&self.identity, &self.addr, &self.purpose, READ_PICTURE, &[])
                .map_err(|e| EdgeError::new(502, &format!("Heimdall is unreachable: {e}")))?;
        let (code, body) = response
            .split_first()
            .ok_or_else(|| EdgeError::new(502, "Heimdall sent an empty response"))?;
        let body = String::from_utf8_lossy(body);
        if *code != ACCEPTED {
            return Err(EdgeError::new(
                502,
                &format!("Heimdall refused the read: {body}"),
            ));
        }
        parse(&body).map_err(|e| EdgeError::new(502, &format!("Heimdall's picture: {e}")))
    }
}
