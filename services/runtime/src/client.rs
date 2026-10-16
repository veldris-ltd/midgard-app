//! A client for a mesh service — how an agent or an operator tool reaches a service that
//! serves a [`gateway::Router`]: [`call`] for one request, a [`Session`] for many over
//! one PQ-mTLS handshake. [NFR-SEC-1]

use std::collections::BTreeSet;
use std::net::TcpStream;
use std::time::Duration;

use gateway::{decode_response, encode_request, status, RECORD_AAD};
use policy::{ClassificationLevel, Purpose};
use pq_mtls::{
    read_frame, run_initiator, write_frame, Initiator, Posture, RecordOpener, RecordSealer, Role,
    TcpChannel,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

use crate::ServiceIdentity;

/// How long a call waits for the service to answer.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// One PQ-mTLS session with a mesh service, carrying any number of routed requests in
/// order. The handshake — the expensive, post-quantum part — happens once, in
/// [`Session::open`]; each [`Session::call`] then costs one sealed request and one
/// sealed response. A service may still close the session (a rate limit or a
/// per-connection quota); the next call then fails and the caller opens a new one.
pub struct Session {
    addr: String,
    stream: TcpStream,
    seal: RecordSealer,
    open: RecordOpener,
}

impl Session {
    /// Connect to the service at `addr`, authenticated as `identity` and declaring
    /// `purpose`. A connection the service refuses — an identity it does not trust, or
    /// a channel its policy does not open — is an error.
    pub fn open(identity: &ServiceIdentity, addr: &str, purpose: &str) -> Result<Self, String> {
        let stream =
            TcpStream::connect(addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?;
        stream
            .set_read_timeout(Some(RESPONSE_TIMEOUT))
            .map_err(|e| format!("cannot set a timeout on the connection: {e}"))?;
        let provider = fips_provider();
        let mut rng = OsRng;
        let initiator = Initiator::new(
            &provider,
            identity.cert.clone(),
            &identity.signing_key,
            identity.anchor.clone(),
            Purpose(purpose.to_string()),
            Posture {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            &mut rng,
        );
        let mut channel = TcpChannel::new(stream);
        let session = run_initiator(initiator, &mut channel)
            .map_err(|e| format!("{addr} refused the PQ-mTLS channel: {e:?}"))?;
        let stream = channel.into_stream();
        let (seal, open) = session.records(Role::Initiator);
        Ok(Self {
            addr: addr.to_string(),
            stream,
            seal,
            open,
        })
    }

    /// Call `method` with `payload` and return the handler's response. An unknown
    /// method and a malformed request are errors.
    pub fn call(&mut self, method: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let addr = &self.addr;
        let sealed = self
            .seal
            .seal(RECORD_AAD, &encode_request(method, payload))
            .map_err(|e| format!("cannot seal the request: {e:?}"))?;
        write_frame(&mut self.stream, &sealed)
            .map_err(|e| format!("cannot send the request: {e:?}"))?;
        let frame = read_frame(&mut self.stream)
            .map_err(|e| format!("{addr} closed the channel: {e:?}"))?;
        let response = self
            .open
            .open(RECORD_AAD, &frame)
            .map_err(|_| format!("the response from {addr} did not authenticate"))?;

        match decode_response(&response) {
            Some((status::OK, body)) => Ok(body.to_vec()),
            Some((status::NO_ROUTE, _)) => Err(format!("{addr} does not serve {method}")),
            _ => Err(format!("{addr} refused the request as malformed")),
        }
    }
}

/// Call `method` with `payload` on the service at `addr`, authenticated as `identity`
/// and declaring `purpose`, and return the handler's response: one [`Session`], one
/// request. A connection the service refuses — an identity it does not trust, or a
/// channel its policy does not open — an unknown method and a malformed request are
/// errors.
pub fn call(
    identity: &ServiceIdentity,
    addr: &str,
    purpose: &str,
    method: &str,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    Session::open(identity, addr, purpose)?.call(method, payload)
}
