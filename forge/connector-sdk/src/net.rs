//! Outbound TCP for network connectors, through the sandbox: a connection is opened
//! only to a `host:port` the [`SandboxPolicy`] names exactly, with bounded connect and
//! I/O timeouts, so a stalled source cannot hang a job. [NFR-PRV-1] [FR-F-2]

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::connector::SandboxPolicy;
use crate::error::ConnectorError;

/// How long a connection attempt may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a read or write may block.
pub const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// Connect to `host:port` if the sandbox permits exactly that target.
pub fn connect(
    sandbox: &SandboxPolicy,
    host: &str,
    port: u16,
) -> Result<TcpStream, ConnectorError> {
    let target = format!("{host}:{port}");
    sandbox.check_egress(&target)?;
    let addrs = target
        .to_socket_addrs()
        .map_err(|e| ConnectorError::Io(format!("cannot resolve {target}: {e}")))?;
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(IO_TIMEOUT))
                    .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
                    .and_then(|()| stream.set_nodelay(true))
                    .map_err(|e| ConnectorError::Io(format!("{target}: {e}")))?;
                return Ok(stream);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(ConnectorError::Io(match last {
        Some(e) => format!("cannot connect to {target}: {e}"),
        None => format!("{target} resolves to no address"),
    }))
}
