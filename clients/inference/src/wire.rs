//! Canonical, length-prefixed wire codec for the Tier-2 remote-delegation exchange
//! ([`InferenceRequest`] out, [`RemoteOutput`] back) carried over the client's
//! PQ-mTLS record layer. Hand-rolled and **fails closed** on any malformed input —
//! no serde, matching the suite's sovereign posture. [RT-57] [FR-C-6]

use crate::remote::RemoteOutput;
use crate::request::InferenceRequest;

/// A decode failure on the wire (fails closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(pub String);

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "malformed remote-inference wire message: {}", self.0)
    }
}
impl std::error::Error for WireError {}

fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_str(o: &mut Vec<u8>, s: &str) {
    put_u32(o, s.len() as u32);
    o.extend_from_slice(s.as_bytes());
}
fn put_strs(o: &mut Vec<u8>, items: &[String]) {
    put_u32(o, items.len() as u32);
    for s in items {
        put_str(o, s);
    }
}

/// A bounds-checked cursor; every read past the end fails closed.
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| WireError("length overflow".into()))?;
        if end > self.b.len() {
            return Err(WireError("unexpected end of input".into()));
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, WireError> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn string(&mut self) -> Result<String, WireError> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| WireError("invalid utf-8".into()))
    }
    fn strings(&mut self) -> Result<Vec<String>, WireError> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            out.push(self.string()?);
        }
        Ok(out)
    }
    fn bool(&mut self) -> Result<bool, WireError> {
        match self.take(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(WireError("bad bool".into())),
        }
    }
    fn finish(self) -> Result<(), WireError> {
        if self.pos == self.b.len() {
            Ok(())
        } else {
            Err(WireError("trailing bytes".into()))
        }
    }
}

/// Encode an [`InferenceRequest`] for transit to the enclosure.
pub fn encode_request(req: &InferenceRequest) -> Vec<u8> {
    let mut o = Vec::new();
    put_str(&mut o, &req.user_message);
    put_strs(&mut o, &req.local_context_ids);
    o.push(req.needs_remote_data as u8);
    o.push(req.above_clearance as u8);
    put_str(&mut o, &req.policy_summary);
    o
}

/// Decode an [`InferenceRequest`] received at the enclosure. Fails closed.
pub fn decode_request(b: &[u8]) -> Result<InferenceRequest, WireError> {
    let mut r = Reader::new(b);
    let user_message = r.string()?;
    let local_context_ids = r.strings()?;
    let needs_remote_data = r.bool()?;
    let above_clearance = r.bool()?;
    let policy_summary = r.string()?;
    r.finish()?;
    Ok(InferenceRequest {
        user_message,
        local_context_ids,
        needs_remote_data,
        above_clearance,
        policy_summary,
    })
}

/// Encode a [`RemoteOutput`] for return to the client.
pub fn encode_output(out: &RemoteOutput) -> Vec<u8> {
    let mut o = Vec::new();
    put_str(&mut o, &out.answer);
    put_strs(&mut o, &out.citations);
    o.push(out.grounded as u8);
    o
}

/// Decode a [`RemoteOutput`] received by the client. Fails closed.
pub fn decode_output(b: &[u8]) -> Result<RemoteOutput, WireError> {
    let mut r = Reader::new(b);
    let answer = r.string()?;
    let citations = r.strings()?;
    let grounded = r.bool()?;
    r.finish()?;
    Ok(RemoteOutput {
        answer,
        citations,
        grounded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips() {
        let req = InferenceRequest {
            user_message: "who is on call?".into(),
            local_context_ids: vec!["obj:a".into(), "obj:b".into()],
            needs_remote_data: true,
            above_clearance: false,
            policy_summary: "clearance=SECRET".into(),
        };
        assert_eq!(decode_request(&encode_request(&req)).unwrap(), req);
    }

    #[test]
    fn output_round_trips() {
        let out = RemoteOutput {
            answer: "answer".into(),
            citations: vec!["obj:x".into()],
            grounded: true,
        };
        assert_eq!(decode_output(&encode_output(&out)).unwrap(), out);
    }

    #[test]
    fn truncated_input_fails_closed() {
        let bytes = encode_request(&InferenceRequest::new("q", vec!["obj:a".into()]));
        assert!(decode_request(&bytes[..bytes.len() - 1]).is_err());
        assert!(decode_output(&[0xff, 0x00]).is_err());
    }
}
