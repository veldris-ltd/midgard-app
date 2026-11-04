//! Outbound TCP for network connectors, through the sandbox: a connection is opened
//! only to a `host:port` the [`SandboxPolicy`] names exactly, with bounded connect and
//! I/O timeouts, so a stalled source cannot hang a job. [NFR-PRV-1] [FR-F-2]
//!
//! The allow-list names a target, not what DNS says it is. A name is resolved once and
//! connected to only at a routable address: never loopback, private, link-local,
//! unspecified, shared (CGNAT), unique-local, broadcast or multicast, so a name that
//! resolves inside the enclosure cannot reach its services. An allowed IP literal is
//! the operator's explicit choice and is connected to as written.

use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::connector::SandboxPolicy;
use crate::error::ConnectorError;

/// How long a connection attempt may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a read or write may block.
pub const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// Connect to `host:port` if the sandbox permits exactly that target, at an address
/// a name may reach (any address, when `host` is an IP literal).
pub fn connect(
    sandbox: &SandboxPolicy,
    host: &str,
    port: u16,
) -> Result<TcpStream, ConnectorError> {
    let target = format!("{host}:{port}");
    sandbox.check_egress(&target)?;
    let literal = host.parse::<IpAddr>().is_ok();
    let resolved: Vec<_> = target
        .to_socket_addrs()
        .map_err(|e| ConnectorError::io_at(&target, format!("cannot resolve: {e}")))?
        .collect();
    if resolved.is_empty() {
        return Err(ConnectorError::io_at(&target, "resolves to no address"));
    }
    let mut last = None;
    for addr in resolved
        .into_iter()
        .filter(|addr| literal || routable(addr.ip()))
    {
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(IO_TIMEOUT))
                    .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
                    .and_then(|()| stream.set_nodelay(true))
                    .map_err(|e| ConnectorError::io_at(&target, e.to_string()))?;
                return Ok(stream);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(ConnectorError::io_at(
        &target,
        match last {
            Some(e) => format!("cannot connect: {e}"),
            None => "resolves to a non-routable address".to_string(),
        },
    ))
}

/// Whether a resolved name may be connected to at `ip`: not loopback, private
/// (10/8, 172.16/12, 192.168/16), link-local (169.254/16, fe80::/10), unspecified,
/// shared (100.64/10), unique-local (fc00::/7), broadcast or multicast. A v4-mapped
/// v6 address is judged as the v4 address it carries.
fn routable(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    };
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            let shared = a == 100 && (b & 0xc0) == 64;
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || shared)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            let link_local = (first & 0xffc0) == 0xfe80;
            let unique_local = (first & 0xfe00) == 0xfc00;
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || link_local
                || unique_local)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    // [NFR-PRV-1]
    #[test]
    fn addresses_inside_the_enclosure_are_not_routable() {
        for text in [
            "127.0.0.1",
            "127.255.0.9",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "100.127.255.255",
            "255.255.255.255",
            "224.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "febf::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.1.2.3",
            "::ffff:169.254.169.254",
        ] {
            assert!(!routable(ip(text)), "{text} is not routable");
        }
    }

    // [NFR-PRV-1]
    #[test]
    fn public_addresses_are_routable() {
        for text in [
            "8.8.8.8",
            "172.32.0.1",
            "172.15.255.255",
            "100.63.255.255",
            "100.128.0.1",
            "192.169.0.1",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
        ] {
            assert!(routable(ip(text)), "{text} is routable");
        }
    }
}
