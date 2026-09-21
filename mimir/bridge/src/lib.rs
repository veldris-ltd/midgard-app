//! # mimir-bridge — the production bridge from product AI-assist to Mimir
//!
//! Each product's AI-assist seam (Heimdall `OptionProvider`, Forge
//! `TransformSuggester`, Bifrost `RiskAdvisor`, Admin `CopilotAdvisor`) is a trait
//! of the shape *"send an assembled prompt to a model, get raw JSON back, or fail
//! closed."* Until now only `Mock`/`Unavailable` implementations existed. This crate
//! supplies the **live bridge**: it terminates internal **PQ-mTLS** to the Mimir
//! inference gateway, sends a typed [`InferenceRequest`], and returns the served
//! [`Completion`] — or a fail-closed [`BridgeError`]. [FR-MI-8] [RT-45] [RC-45]
//!
//! The bridge is deliberately split:
//! - [`Inference`] is the tiny transport trait the product adapters call. It carries
//!   no crypto, so an adapter can be unit-tested against an in-process fake.
//! - [`PqMtlsClient`] is the real implementation over the post-quantum channel,
//!   proven end-to-end against a live gateway in this crate's tests.
//!
//! Products keep their `Unavailable*` degraded path unchanged; wiring a
//! [`PqMtlsClient`] is purely additive — with Mimir present the seam returns live
//! advisory output, without it the product runs exactly as before. [DD-6]

// The single inference contract, re-exported so product adapters depend only on
// this crate for the request/response types.
pub use model_server::{
    Completion, InferenceRequest, InferenceResponse, Locality, Quant, WorkloadClass,
};

/// Wrap advisory `text` as a [`Completion`] carrying placeholder serving metadata.
/// Convenient for callers/tests that only care about the model's text (the real
/// [`PqMtlsClient`] returns fully-populated completions from the gateway).
pub fn text_completion(text: impl Into<String>) -> Completion {
    Completion {
        text: text.into(),
        model_id: "local-reference".into(),
        locality: Locality::InEnclosure,
        quant: Quant::None,
        first_token_ms: 0,
        cache_hit: false,
        batch_size: 1,
        prompt_tokens: 0,
        completion_tokens: 0,
        audit_seq: 0,
    }
}

/// A fail-closed bridge failure. AI-assist is advisory, so every one of these makes
/// the product fall back to its non-AI path rather than fabricate output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeError {
    /// The PQ-mTLS channel or record layer failed (connect, handshake, seal/open).
    Transport(String),
    /// The inference gateway served a fail-closed denial (policy, malformed, backend).
    Denied(String),
    /// The response bytes did not decode to a valid [`InferenceResponse`].
    Malformed(String),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BridgeError::Transport(m) => write!(f, "mimir transport error: {m}"),
            BridgeError::Denied(m) => write!(f, "mimir denied the call: {m}"),
            BridgeError::Malformed(m) => write!(f, "malformed inference response: {m}"),
        }
    }
}

impl std::error::Error for BridgeError {}

/// The transport seam the product adapters call. A single inference round-trip:
/// request in, served completion out, or a fail-closed error.
pub trait Inference {
    /// Perform one inference call.
    fn infer(&self, request: InferenceRequest) -> Result<Completion, BridgeError>;
}

#[cfg(feature = "backend-fips")]
pub use client::{MimirEndpoint, PqMtlsClient};

#[cfg(feature = "backend-fips")]
mod client {
    use std::net::TcpStream;

    use gateway::RECORD_AAD;
    use policy::Purpose;
    use pq_mtls::{
        read_frame, run_initiator, write_frame, IdentityCert, Initiator, Posture,
        Role as RecordRole, TcpChannel, TrustAnchor,
    };
    use pqc_agility::backend::fips::provider as fips_provider;
    use pqc_agility::{CryptoProvider, OsRng, SigningKey};

    use crate::{BridgeError, Completion, Inference, InferenceRequest, InferenceResponse};

    /// Everything needed to open an authenticated PQ-mTLS session to the Mimir
    /// inference gateway: its address, the mesh trust anchor, this consumer's
    /// certified workload identity + signing key, and the declared purpose/posture
    /// the gateway authorises the call against.
    pub struct MimirEndpoint {
        /// `host:port` of the inference gateway.
        pub addr: String,
        /// The mesh trust anchor (the IdP/CA public key).
        pub anchor: TrustAnchor,
        /// This consumer's certified identity (e.g. `svc-forge`).
        pub cert: IdentityCert,
        /// The signing key for the identity above.
        pub signing_key: SigningKey,
        /// The declared purpose of access (audited and policy-checked).
        pub purpose: String,
        /// The caller's clearance posture.
        pub posture: Posture,
    }

    /// The live bridge: a real PQ-mTLS client of the Mimir inference gateway. Each
    /// [`infer`](Inference::infer) opens a fresh authenticated session, exchanges one
    /// record-layer request/response, and returns the served completion. Any
    /// transport, authorisation, or decode failure is surfaced as a fail-closed
    /// [`BridgeError`] — the product then uses its non-AI path.
    pub struct PqMtlsClient {
        provider: CryptoProvider,
        endpoint: MimirEndpoint,
    }

    impl PqMtlsClient {
        /// A client for `endpoint`, using the production FIPS crypto provider.
        pub fn new(endpoint: MimirEndpoint) -> Self {
            Self {
                provider: fips_provider(),
                endpoint,
            }
        }

        /// A client with an explicit provider (for callers that already hold one).
        pub fn with_provider(provider: CryptoProvider, endpoint: MimirEndpoint) -> Self {
            Self { provider, endpoint }
        }
    }

    impl Inference for PqMtlsClient {
        fn infer(&self, request: InferenceRequest) -> Result<Completion, BridgeError> {
            let e = &self.endpoint;
            let stream = TcpStream::connect(&e.addr)
                .map_err(|err| BridgeError::Transport(format!("connect {}: {err}", e.addr)))?;
            let mut ch = TcpChannel::new(stream);
            let mut rng = OsRng;
            let init = Initiator::new(
                &self.provider,
                e.cert.clone(),
                &e.signing_key,
                e.anchor.clone(),
                Purpose(e.purpose.clone()),
                e.posture.clone(),
                &mut rng,
            );
            let session = run_initiator(init, &mut ch)
                .map_err(|err| BridgeError::Transport(format!("handshake: {err:?}")))?;
            let mut stream = ch.into_stream();
            let (mut seal, mut open) = session.records(RecordRole::Initiator);

            let sealed = seal
                .seal(RECORD_AAD, &request.encode())
                .map_err(|err| BridgeError::Transport(format!("seal: {err:?}")))?;
            write_frame(&mut stream, &sealed)
                .map_err(|err| BridgeError::Transport(format!("write: {err:?}")))?;

            let frame = read_frame(&mut stream)
                .map_err(|err| BridgeError::Transport(format!("read: {err:?}")))?;
            let raw = open
                .open(RECORD_AAD, &frame)
                .map_err(|_| BridgeError::Transport("record open/authentication failed".into()))?;

            match InferenceResponse::decode(&raw).map_err(|m| BridgeError::Malformed(m.0))? {
                InferenceResponse::Ok(completion) => Ok(completion),
                InferenceResponse::Denied { reason, .. } => Err(BridgeError::Denied(reason)),
            }
        }
    }
}
