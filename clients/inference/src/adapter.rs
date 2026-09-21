//! The inference adapter: choose a tier, serve the request at that tier, and
//! produce an audit note for the decision. `[FR-C-6]` `[NFR-AI-1]`
//!
//! `serve` is a free function over the three seams (on-device runtime, remote
//! delegation, tier reasoner) — like `forge-ai-assist::suggest_transform` — so the
//! whole AI path is optional and testable without a model or a live enclosure.

use audit_writer::{
    Action, Actor, AuditError, AuditRecord, AuditWriter, LedgerStore, RecordId, Resource, Timestamp,
};

use crate::error::InferenceError;
use crate::ondevice::{OnDeviceOutput, OnDeviceRuntime};
use crate::prompt::assemble_on_device;
use crate::reasoner::TierReasoner;
use crate::remote::{RemoteInference, RemoteOutput};
use crate::request::InferenceRequest;
use crate::tier::{guard, heuristic, Heuristic, Tier, TierContext};

/// Where the final tier came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierSource {
    /// The deterministic heuristic decided it.
    Heuristic,
    /// The A.C2 reasoner proposed it (then the guard clamped it).
    Reasoner,
}

/// The chosen tier and how it was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierDecision {
    /// The tier that will serve the request.
    pub tier: Tier,
    /// Whether the heuristic or the reasoner chose it.
    pub source: TierSource,
}

/// Choose a tier: heuristic fast path, A.C2 reasoner on ambiguity, deterministic
/// [`guard`] always. `[FR-C-6]`
pub fn choose_tier(ctx: &TierContext, reasoner: &dyn TierReasoner) -> TierDecision {
    let (tier, source) = match heuristic(ctx) {
        Heuristic::Decided(t) => (t, TierSource::Heuristic),
        Heuristic::Ambiguous => (reasoner.reason(ctx).tier, TierSource::Reasoner),
    };
    TierDecision {
        tier: guard(tier, ctx),
        source,
    }
}

/// The served result, at whatever tier was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceResult {
    /// Served locally (Tier 1).
    OnDevice(OnDeviceOutput),
    /// Served by the enclosure (Tier 2).
    Remote(RemoteOutput),
    /// Local draft verified/augmented by the enclosure (Tier 3).
    Hybrid {
        /// The on-device draft.
        draft: OnDeviceOutput,
        /// The enclosure-verified result.
        verified: RemoteOutput,
    },
    /// The feature disabled gracefully. `[FR-C-10]`
    Disabled {
        /// Why the feature is unavailable.
        reason: String,
    },
}

/// The audit note for a served request — what to seal into the audit ledger. Every
/// tier decision and inference is auditable. `[NFR-AI-1]` `[AC-C-6]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditNote {
    /// The tier served.
    pub tier: Tier,
    /// Whether the result was grounded/cited.
    pub grounded: bool,
    /// Citations returned.
    pub citations: Vec<String>,
    /// Whether request data left the device.
    pub data_left_device: bool,
    /// Whether the turn escalated to the enclosure.
    pub escalated: bool,
}

/// A served request: the result, the tier decision, and the audit note.
#[derive(Debug, Clone)]
pub struct Served {
    /// The inference result.
    pub result: InferenceResult,
    /// How the tier was chosen.
    pub decision: TierDecision,
    /// The audit note to seal.
    pub audit: AuditNote,
}

/// Serve one request. Picks a tier, dispatches to the matching seam, and refuses an
/// ungrounded remote answer (fail closed). A disabled tier returns a clean
/// [`InferenceResult::Disabled`] rather than an error — graceful degradation.
/// `[FR-C-6]` `[FR-C-10]` `[AC-C-5]` `[AC-C-6]`
pub fn serve<R, M>(
    ctx: &TierContext,
    request: &InferenceRequest,
    runtime: Option<&R>,
    remote: Option<&M>,
    reasoner: &dyn TierReasoner,
) -> Result<Served, InferenceError>
where
    R: OnDeviceRuntime,
    M: RemoteInference,
{
    let decision = choose_tier(ctx, reasoner);

    let (result, audit) = match decision.tier {
        Tier::OnDevice => {
            let rt = runtime.ok_or(InferenceError::NoOnDeviceModel)?;
            let prompt = assemble_on_device(request, Tier::OnDevice);
            let out = rt.infer(&prompt, request)?;
            let audit = AuditNote {
                tier: Tier::OnDevice,
                grounded: out.is_grounded(),
                citations: out.citations.clone(),
                data_left_device: false,
                escalated: out.escalate_to_enclosure,
            };
            (InferenceResult::OnDevice(out), audit)
        }
        Tier::Remote => {
            let rm = remote.ok_or(InferenceError::EnclosureUnreachable)?;
            let out = rm.infer(request)?;
            require_grounded(&out)?;
            let audit = AuditNote {
                tier: Tier::Remote,
                grounded: true,
                citations: out.citations.clone(),
                data_left_device: true,
                escalated: false,
            };
            (InferenceResult::Remote(out), audit)
        }
        Tier::Hybrid => {
            let rt = runtime.ok_or(InferenceError::NoOnDeviceModel)?;
            let rm = remote.ok_or(InferenceError::EnclosureUnreachable)?;
            let prompt = assemble_on_device(request, Tier::Hybrid);
            let draft = rt.infer(&prompt, request)?;
            let verified = rm.infer(request)?;
            require_grounded(&verified)?;
            let audit = AuditNote {
                tier: Tier::Hybrid,
                grounded: true,
                citations: verified.citations.clone(),
                data_left_device: true,
                escalated: draft.escalate_to_enclosure,
            };
            (InferenceResult::Hybrid { draft, verified }, audit)
        }
        Tier::Disable => {
            let audit = AuditNote {
                tier: Tier::Disable,
                grounded: true,
                citations: vec![],
                data_left_device: false,
                escalated: false,
            };
            (
                InferenceResult::Disabled {
                    reason: "on-device inference unavailable and no safe remote option".into(),
                },
                audit,
            )
        }
    };

    Ok(Served {
        result,
        decision,
        audit,
    })
}

fn require_grounded(out: &RemoteOutput) -> Result<(), InferenceError> {
    if out.grounded && !out.citations.is_empty() {
        Ok(())
    } else {
        Err(InferenceError::Ungrounded)
    }
}

/// Seal an audit note into a PQ-signed audit ledger. The caller supplies the actor,
/// resource, and timestamp (this crate does no clock I/O). Returns the sequence
/// number of the appended entry. `[NFR-AI-1]`
pub fn seal_audit<S: LedgerStore>(
    writer: &mut AuditWriter<S>,
    note: &AuditNote,
    actor: &str,
    resource: &str,
    ts: &str,
) -> Result<u64, AuditError> {
    let action = format!(
        "infer tier={:?} grounded={} egress={} escalated={}",
        note.tier, note.grounded, note.data_left_device, note.escalated
    );
    writer.append(AuditRecord {
        id: RecordId(format!("infer:{actor}:{ts}")),
        actor: Actor(actor.to_string()),
        action: Action(action),
        resource: Resource(resource.to_string()),
        ts: Timestamp(ts.to_string()),
    })
}
