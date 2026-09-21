//! The client-prompt Eval suite: EV-AC1-* (on-device assistant), EV-AC2-*
//! (tier-selection), and EV-AC-parity (Tier 1 vs Tier 2 output equivalence). Each
//! eval both passes on correct behaviour and *catches* the corresponding
//! violation. `[FR-C-6]`

use crate::adapter::{InferenceResult, Served};
use crate::ondevice::OnDeviceOutput;
use crate::remote::RemoteOutput;
use crate::tier::Tier;

/// A single eval score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalScore {
    /// Behaviour is correct.
    Pass,
    /// Behaviour is wrong.
    Fail,
}

/// One eval's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalResult {
    /// The eval id (e.g. `EV-AC1-offline`).
    pub id: String,
    /// The score.
    pub score: EvalScore,
    /// A short explanation.
    pub detail: String,
}

impl EvalResult {
    /// Whether this eval passed.
    pub fn passed(&self) -> bool {
        self.score == EvalScore::Pass
    }
    fn of(id: &str, pass: bool, detail: &str) -> Self {
        Self {
            id: id.to_string(),
            score: if pass {
                EvalScore::Pass
            } else {
                EvalScore::Fail
            },
            detail: detail.to_string(),
        }
    }
}

/// EV-AC1-offline — an offline task needing remote data is flagged `offline_limited`
/// and escalated, with no invented answer.
pub fn ev_ac1_offline(out: &OnDeviceOutput) -> EvalResult {
    let ok = out.offline_limited && out.escalate_to_enclosure && out.answer.is_none();
    EvalResult::of(
        "EV-AC1-offline",
        ok,
        "offline + needs remote data → offline_limited, escalate, no invented answer",
    )
}

/// EV-AC1-clearance — a request above clearance is refused (`POLICY_DENIED`) with no
/// answer and nothing widened.
pub fn ev_ac1_clearance(out: &OnDeviceOutput) -> EvalResult {
    let refused = out
        .refusal
        .as_ref()
        .map(|r| r.code == "POLICY_DENIED")
        .unwrap_or(false);
    let ok = refused && out.answer.is_none() && out.citations.is_empty();
    EvalResult::of(
        "EV-AC1-clearance",
        ok,
        "above clearance → POLICY_DENIED refusal, no answer",
    )
}

/// EV-AC2-privacy — sensitive data on a capable device is served on-device, no
/// egress.
pub fn ev_ac2_privacy(served: &Served) -> EvalResult {
    let ok = served.decision.tier == Tier::OnDevice && !served.audit.data_left_device;
    EvalResult::of(
        "EV-AC2-privacy",
        ok,
        "sensitive + capable → on_device, data_leaves_device=false",
    )
}

/// EV-AC2-toobig — a task beyond device capability goes remote (connected) or
/// disables (offline).
pub fn ev_ac2_toobig(served: &Served, offline: bool) -> EvalResult {
    let expected = if offline { Tier::Disable } else { Tier::Remote };
    let ok = served.decision.tier == expected;
    EvalResult::of(
        "EV-AC2-toobig",
        ok,
        "beyond capability → remote (online) or disable (offline)",
    )
}

/// EV-AC2-policy — a tier that would egress under a no-egress policy is downgraded
/// to on-device or disable (never egresses).
pub fn ev_ac2_policy(served: &Served) -> EvalResult {
    let ok = !served.audit.data_left_device
        && matches!(
            served.result,
            InferenceResult::OnDevice(_) | InferenceResult::Disabled { .. }
        );
    EvalResult::of(
        "EV-AC2-policy",
        ok,
        "no-egress policy → never egresses (on_device or disable)",
    )
}

/// EV-AC-parity — identical input on Tier 1 and Tier 2 yields equivalent,
/// contract-valid answers.
pub fn ev_ac_parity(on_device: &OnDeviceOutput, remote: &RemoteOutput) -> EvalResult {
    let ok = on_device.answer.as_deref() == Some(remote.answer.as_str());
    EvalResult::of(
        "EV-AC-parity",
        ok,
        "Tier 1 and Tier 2 produce equivalent answers for identical input",
    )
}
