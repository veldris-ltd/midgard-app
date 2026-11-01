//! The human-gate approvals journal: one sealed frame per approval state change,
//! replayed in order on boot so that a pending or approved gate, and the fact that one
//! was used, survive a restart. [NFR-AI-1] [VLD-ANB-MIDGARD-0039]
//!
//! Frames are length-prefixed in the style of the fabric's durable store: an operation
//! byte, then little-endian `u32`-prefixed strings and `u64` times. The journal is an
//! [`audit_writer::SealedJournal`] under the data sealer, domain
//! [`APPROVALS_DOMAIN`].

/// The domain the approvals journal's frames are sealed under.
pub const APPROVALS_DOMAIN: &[u8] = b"midgard-approvals-v1";

const APPROVAL_OP_REQUEST: u8 = 0;
const APPROVAL_OP_DECIDE: u8 = 1;
const APPROVAL_OP_USE: u8 = 2;
const APPROVAL_OP_PRUNE: u8 = 3;

/// One approval state change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApprovalFrame {
    /// A new pending approval.
    Request {
        id: String,
        action_id: String,
        action_title: String,
        target: String,
        requester: String,
        requested: u64,
    },
    /// A second person decided it; `expires` is meaningful only when approved.
    Decide {
        id: String,
        approved: bool,
        approver: String,
        expires: u64,
    },
    /// It was redeemed: the action ran.
    Use { id: String },
    /// It was removed from the store (expired, or settled and making room).
    Prune { id: String },
}

fn put_str(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u32).to_le_bytes());
    o.extend_from_slice(s.as_bytes());
}

impl ApprovalFrame {
    /// The frame's bytes.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        match self {
            ApprovalFrame::Request {
                id,
                action_id,
                action_title,
                target,
                requester,
                requested,
            } => {
                o.push(APPROVAL_OP_REQUEST);
                put_str(&mut o, id);
                put_str(&mut o, action_id);
                put_str(&mut o, action_title);
                put_str(&mut o, target);
                put_str(&mut o, requester);
                o.extend_from_slice(&requested.to_le_bytes());
            }
            ApprovalFrame::Decide {
                id,
                approved,
                approver,
                expires,
            } => {
                o.push(APPROVAL_OP_DECIDE);
                put_str(&mut o, id);
                o.push(u8::from(*approved));
                put_str(&mut o, approver);
                o.extend_from_slice(&expires.to_le_bytes());
            }
            ApprovalFrame::Use { id } => {
                o.push(APPROVAL_OP_USE);
                put_str(&mut o, id);
            }
            ApprovalFrame::Prune { id } => {
                o.push(APPROVAL_OP_PRUNE);
                put_str(&mut o, id);
            }
        }
        o
    }

    /// A frame from its bytes; anything truncated, trailing or unknown is refused.
    pub(crate) fn decode(b: &[u8]) -> Result<Self, String> {
        let mut r = Reader { b, at: 0 };
        let frame = match r.u8()? {
            APPROVAL_OP_REQUEST => ApprovalFrame::Request {
                id: r.text()?,
                action_id: r.text()?,
                action_title: r.text()?,
                target: r.text()?,
                requester: r.text()?,
                requested: r.u64()?,
            },
            APPROVAL_OP_DECIDE => ApprovalFrame::Decide {
                id: r.text()?,
                approved: match r.u8()? {
                    0 => false,
                    1 => true,
                    _ => return Err("approval record: bad decision".into()),
                },
                approver: r.text()?,
                expires: r.u64()?,
            },
            APPROVAL_OP_USE => ApprovalFrame::Use { id: r.text()? },
            APPROVAL_OP_PRUNE => ApprovalFrame::Prune { id: r.text()? },
            _ => return Err("approval record: unknown operation".into()),
        };
        if r.at != b.len() {
            return Err("approval record: trailing bytes".into());
        }
        Ok(frame)
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.b.len())
            .ok_or("approval record: truncated")?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, String> {
        let mut a = [0u8; 8];
        a.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(a))
    }

    fn text(&mut self) -> Result<String, String> {
        let mut n = [0u8; 4];
        n.copy_from_slice(self.take(4)?);
        let n = u32::from_le_bytes(n) as usize;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| "approval record: not UTF-8".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every frame decodes to itself, and a damaged one is refused rather than
    /// misread. [VLD-ANB-MIDGARD-0039] [AC-21.4]
    #[test]
    fn frames_round_trip_and_damage_is_refused() {
        let frames = [
            ApprovalFrame::Request {
                id: "APR-1".into(),
                action_id: "ground_aircraft".into(),
                action_title: "Ground aircraft".into(),
                target: "aircraft/AC-2214".into(),
                requester: "operator".into(),
                requested: 1_800_000_000,
            },
            ApprovalFrame::Decide {
                id: "APR-1".into(),
                approved: true,
                approver: "approver".into(),
                expires: 1_800_000_900,
            },
            ApprovalFrame::Use { id: "APR-1".into() },
            ApprovalFrame::Prune { id: "APR-1".into() },
        ];
        for frame in &frames {
            let bytes = frame.encode();
            assert_eq!(&ApprovalFrame::decode(&bytes).unwrap(), frame);
            assert!(ApprovalFrame::decode(&bytes[..bytes.len() - 1]).is_err());
            let mut longer = bytes.clone();
            longer.push(0);
            assert!(ApprovalFrame::decode(&longer).is_err());
        }
        assert!(ApprovalFrame::decode(&[9]).is_err());
        assert!(ApprovalFrame::decode(&[]).is_err());
    }
}
