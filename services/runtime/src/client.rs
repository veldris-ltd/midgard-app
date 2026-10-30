//! A client for a mesh service: one PQ-mTLS session, one routed request, one response —
//! how an agent or an operator tool reaches a service that serves a
//! [`gateway::Router`]. [NFR-SEC-1]

use std::collections::BTreeSet;
use std::net::TcpStream;
use std::time::Duration;

use gateway::{close_stream, decode_response, encode_request, status, RECORD_AAD};
use policy::{ClassificationLevel, Purpose};
use pq_mtls::{
    read_frame, run_initiator, write_frame, Initiator, Posture, Role, ServiceId, TcpChannel,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

use crate::ServiceIdentity;

/// How long a call waits for the service to answer.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// Call `method` with `payload` on the service `server` at `addr`, authenticated as
/// `identity` and declaring `purpose`, and return the handler's response. Whoever
/// answers at `addr` must hold a certificate for `server`: any other mesh identity is
/// refused before the request is sent. [MG-SEC-01] A connection the service refuses —
/// an identity it does not trust, or a channel its policy does not open — an unknown
/// method and a malformed request are errors. The call ends with the record layer's
/// end-of-stream marker. [MG-SEC-12]
pub fn call(
    identity: &ServiceIdentity,
    addr: &str,
    server: &str,
    purpose: &str,
    method: &str,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let stream = TcpStream::connect(addr).map_err(|e| format!("cannot connect to {addr}: {e}"))?;
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
        ServiceId(server.to_string()),
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
    let mut stream = channel.into_stream();
    let (mut seal, mut open) = session.records(Role::Initiator);

    let sealed = seal
        .seal(RECORD_AAD, &encode_request(method, payload))
        .map_err(|e| format!("cannot seal the request: {e:?}"))?;
    write_frame(&mut stream, &sealed).map_err(|e| format!("cannot send the request: {e:?}"))?;
    let frame = read_frame(&mut stream).map_err(|e| format!("{addr} closed the channel: {e:?}"))?;
    let response = open
        .open(RECORD_AAD, &frame)
        .map_err(|_| format!("the response from {addr} did not authenticate"))?;
    close_stream(&mut stream, &mut seal)
        .map_err(|e| format!("cannot close the channel to {addr}: {e:?}"))?;

    match decode_response(&response) {
        Some((status::OK, body)) => Ok(body.to_vec()),
        Some((status::NO_ROUTE, _)) => Err(format!("{addr} does not serve {method}")),
        _ => Err(format!("{addr} refused the request as malformed")),
    }
}
