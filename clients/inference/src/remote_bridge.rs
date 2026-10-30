//! The **live** Tier-2 remote-inference bridge: delegates heavier reasoning to the
//! enclosure over a real client PQ-mTLS session. [RT-57] [RC-57] [FR-C-6] [AC-C-6]
//!
//! Production counterpart to `MockRemoteInference`. Each `infer` opens an
//! authenticated hybrid PQ-mTLS session to the enclosure gateway via
//! [`midgard_client_enclosure::EnclosureClient`], sends the [`InferenceRequest`]
//! over the AES-256-GCM record layer, and decodes the [`RemoteOutput`] the
//! enclosure returns. Any transport, handshake, or decode failure fails closed as
//! [`InferenceError::EnclosureUnreachable`] — the adapter then refuses to fabricate
//! a remote answer (and separately refuses an ungrounded one). Wiring this bridge
//! is additive: with no connectivity the tiering logic is unchanged. [DD-6]

use std::net::TcpStream;

use midgard_client_enclosure::EnclosureClient;
use pq_mtls::{read_frame, write_frame, Role as RecordRole, TcpChannel};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::{CryptoProvider, OsRng, SigningKey};

use crate::error::InferenceError;
use crate::remote::{RemoteInference, RemoteOutput};
use crate::request::InferenceRequest;
use crate::wire::{decode_output, encode_request};

/// The record-layer AAD binding this delegation protocol; both the client bridge
/// and the enclosure's remote-inference responder must agree on it.
pub const REMOTE_INFERENCE_AAD: &[u8] = b"midgard-client/remote-inference/1";

/// Collapse any transport / handshake / codec error into the fail-closed
/// [`InferenceError::EnclosureUnreachable`]. A generic fn (not a closure) so it
/// applies to every error type on the delegation path.
fn unreachable<E>(_: E) -> InferenceError {
    InferenceError::EnclosureUnreachable
}

/// A [`RemoteInference`] that delegates to the enclosure over a real PQ-mTLS
/// session. Holds the connection identity + the enclosure address; each call opens
/// a fresh authenticated session (the client is stateless between requests).
pub struct PqMtlsRemoteInference {
    addr: String,
    client: EnclosureClient,
    signing_key: SigningKey,
    provider: CryptoProvider,
}

impl PqMtlsRemoteInference {
    /// Build a bridge to the enclosure gateway at `addr` (`host:port`), using
    /// `client` (the pinned trust anchor + this client's cert/purpose/posture) and
    /// `signing_key` for the handshake. Uses the production FIPS provider.
    pub fn new(addr: impl Into<String>, client: EnclosureClient, signing_key: SigningKey) -> Self {
        Self {
            addr: addr.into(),
            client,
            signing_key,
            provider: fips_provider(),
        }
    }
}

impl RemoteInference for PqMtlsRemoteInference {
    fn infer(&self, request: &InferenceRequest) -> Result<RemoteOutput, InferenceError> {
        // Any transport / handshake / codec failure fails closed as
        // `EnclosureUnreachable`; the adapter then declines to fabricate an answer.
        let stream = TcpStream::connect(&self.addr).map_err(unreachable)?;
        let mut ch = TcpChannel::new(stream);
        let mut rng = OsRng;
        // Fails closed on a non-PQ suite, an untrusted cert, or a peer-pin mismatch.
        let session = self
            .client
            .connect(&self.provider, &self.signing_key, &mut rng, &mut ch)
            .map_err(unreachable)?;

        let mut stream = ch.into_stream();
        let (mut seal, mut open) = session.records(RecordRole::Initiator);

        let sealed = seal
            .seal(REMOTE_INFERENCE_AAD, &encode_request(request))
            .map_err(unreachable)?;
        write_frame(&mut stream, &sealed).map_err(unreachable)?;

        let frame = read_frame(&mut stream).map_err(unreachable)?;
        let raw = open
            .open(REMOTE_INFERENCE_AAD, &frame)
            .map_err(unreachable)?;
        // End the stream explicitly so the gateway can tell a finished call from
        // a cut connection. [MG-SEC-12]
        let close = seal.seal_close(REMOTE_INFERENCE_AAD).map_err(unreachable)?;
        write_frame(&mut stream, &close).map_err(unreachable)?;
        decode_output(&raw).map_err(unreachable)
    }
}
