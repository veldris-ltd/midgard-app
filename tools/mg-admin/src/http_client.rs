//! A minimal HTTP/1.1 client for the administration routes of a gateway on this machine.
//! Administration carries password hashes and TOTP secrets, and the gateway serves
//! plain HTTP, so it only ever talks to a loopback address. [NFR-PRV-1] [RC-24]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use http_gateway::json::{parse, Json};

use crate::Failure;

/// How long to wait for the gateway to answer.
const TIMEOUT: Duration = Duration::from_secs(60);

/// POST `body` to `path` on `gateway` (`http://<loopback>:<port>`) and return the JSON
/// reply, or the gateway's refusal with its status and message.
pub(crate) fn post_json(gateway: &str, path: &str, body: &str) -> Result<Json, Failure> {
    let authority = gateway
        .strip_prefix("http://")
        .map(|a| a.trim_end_matches('/'))
        .filter(|a| !a.contains('/'))
        .ok_or_else(|| {
            Failure::Usage(format!(
                "--gateway must be http://<loopback address>:<port>, not '{gateway}'"
            ))
        })?;
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    if !matches!(host, "127.0.0.1" | "localhost" | "[::1]") {
        return Err(Failure::Usage(format!(
            "--gateway must be a loopback address: administration sends password hashes and \
             TOTP secrets over plain HTTP, so '{host}' is refused"
        )));
    }

    let unreachable =
        |e: std::io::Error| Failure::Run(format!("cannot talk to the gateway at {gateway}: {e}"));
    let mut stream = TcpStream::connect(authority).map_err(unreachable)?;
    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(unreachable)?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(unreachable)?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).map_err(unreachable)?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(unreachable)?;

    let raw = String::from_utf8(raw)
        .map_err(|_| Failure::Run("the gateway's reply is not UTF-8".into()))?;
    let (head, reply) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| Failure::Run("the gateway's reply is malformed".into()))?;
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Failure::Run("the gateway's reply has no status".into()))?;
    let json = parse(reply)
        .map_err(|e| Failure::Run(format!("the gateway's reply is not JSON ({status}): {e}")))?;
    if (200..300).contains(&status) {
        Ok(json)
    } else {
        Err(Failure::Run(format!(
            "the gateway refused ({status}): {}",
            json.str_field("error").unwrap_or(reply)
        )))
    }
}
