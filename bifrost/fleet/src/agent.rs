//! On-host fleet agents. An agent runs on each installation and periodically
//! **checks in** — reporting the version it is actually running and its health — to
//! the fleet manager over PQ-mTLS. The check-in payload is [`AgentReport`]; the
//! manager records it (last-seen stamped by its own clock) so fleet state reflects
//! live installed versions and can flag nodes that have gone silent. [FR-B-3]
//!
//! This module owns only the wire payload (encode/decode, no serde); the PQ-mTLS
//! session is established by the caller with `crypto/pq-mtls`, and the manager-side
//! bookkeeping lives on [`crate::Fleet`].

use bifrost_rollout::VersionId;

/// One agent check-in: what an installation reports about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReport {
    /// The reporting installation's id.
    pub install_id: String,
    /// The channel it follows.
    pub channel: String,
    /// The version it is actually running right now.
    pub version: VersionId,
    /// Whether the installation reports itself healthy.
    pub healthy: bool,
}

impl AgentReport {
    /// Build a report.
    pub fn new(
        install_id: impl Into<String>,
        channel: impl Into<String>,
        version: VersionId,
        healthy: bool,
    ) -> Self {
        Self {
            install_id: install_id.into(),
            channel: channel.into(),
            version,
            healthy,
        }
    }

    /// Canonical bytes for the check-in message (length-prefixed, no serde). This is
    /// what the agent seals over the PQ-mTLS record layer.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        put_str(&mut o, &self.install_id);
        put_str(&mut o, &self.channel);
        put_str(&mut o, &self.version.0);
        o.push(self.healthy as u8);
        o
    }

    /// Decode a check-in message. Fails closed on truncation or trailing bytes.
    pub fn decode(b: &[u8]) -> Result<Self, &'static str> {
        let mut r = Reader { b, p: 0 };
        let install_id = r.string()?;
        let channel = r.string()?;
        let version = VersionId(r.string()?);
        let healthy = r.u8()? != 0;
        if r.p != b.len() {
            return Err("agent report: trailing bytes");
        }
        Ok(Self {
            install_id,
            channel,
            version,
            healthy,
        })
    }
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        let e = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or("agent report: truncated")?;
        let s = &self.b[self.p..e];
        self.p = e;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn string(&mut self) -> Result<String, &'static str> {
        let b = self.take(4)?;
        let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "agent report: invalid utf8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_round_trips() {
        let r = AgentReport::new("host-a", "stable", VersionId::new("1.4.0"), true);
        assert_eq!(AgentReport::decode(&r.encode()).unwrap(), r);
        assert!(AgentReport::decode(b"\x00").is_err());
    }
}
