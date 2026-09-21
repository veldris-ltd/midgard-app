//! The A.C2 tier-selection reasoner seam. When the heuristic is ambiguous the
//! adapter asks a reasoner for an advisory proposal; the deterministic
//! [`crate::tier::guard`] then clamps it. Production bridges this to Mimir with the
//! verbatim [`mimir_prompts::A_C2_TIER_SELECTION`] prompt; tests and the offline
//! fallback use deterministic implementations. `[FR-C-6]`

use crate::tier::{PrivacyPreference, Tier, TierContext};

/// An advisory tier proposal (the A.C2 output contract, typed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierProposal {
    /// The proposed tier.
    pub tier: Tier,
    /// A short rationale (<= 30 words, per the prompt).
    pub reason: String,
    /// Whether the proposal implies data leaving the device.
    pub data_leaves_device: bool,
}

/// Chooses a tier for an ambiguous request. Advisory only — the caller clamps the
/// result through [`crate::tier::guard`].
pub trait TierReasoner {
    /// Propose a tier for `ctx`.
    fn reason(&self, ctx: &TierContext) -> TierProposal;
}

/// The default deterministic reasoner: privacy first, then correctness, then
/// performance — the A.C2 policy, encoded. Used offline (no Mimir) and as the test
/// stand-in. Never proposes egress the guard would have to reverse.
#[derive(Debug, Default, Clone, Copy)]
pub struct PrivacyFirstReasoner;

impl TierReasoner for PrivacyFirstReasoner {
    fn reason(&self, ctx: &TierContext) -> TierProposal {
        let capable = ctx.capable();
        // Privacy first: if the device is capable and data shouldn't leave (or the
        // user prefers on-device), keep it local.
        if capable && (!ctx.allow_egress || ctx.privacy == PrivacyPreference::PreferOnDevice) {
            return TierProposal {
                tier: Tier::OnDevice,
                reason: "capable device, privacy-preferred: keep on-device".into(),
                data_leaves_device: false,
            };
        }
        // Correctness: if egress is permitted and a local draft plus enclosure
        // verification helps, go hybrid (capable) or remote (not).
        if ctx.online() && ctx.allow_egress {
            if capable {
                return TierProposal {
                    tier: Tier::Hybrid,
                    reason: "capable + connected: local draft, enclosure verify".into(),
                    data_leaves_device: true,
                };
            }
            return TierProposal {
                tier: Tier::Remote,
                reason: "not capable, connected: delegate to enclosure".into(),
                data_leaves_device: true,
            };
        }
        // Nothing safe: on-device if we can, else disable.
        if capable {
            TierProposal {
                tier: Tier::OnDevice,
                reason: "offline/no-egress: on-device only".into(),
                data_leaves_device: false,
            }
        } else {
            TierProposal {
                tier: Tier::Disable,
                reason: "no local capability and no egress: disable".into(),
                data_leaves_device: false,
            }
        }
    }
}

/// A fixed-answer reasoner for tests — lets a test force a specific (possibly
/// unsafe) proposal to prove the guard clamps it.
#[derive(Debug, Clone)]
pub struct FixedReasoner {
    /// The proposal always returned.
    pub proposal: TierProposal,
}

impl FixedReasoner {
    /// A reasoner that always proposes `tier`.
    pub fn new(tier: Tier, data_leaves_device: bool) -> Self {
        Self {
            proposal: TierProposal {
                tier,
                reason: "fixed test proposal".into(),
                data_leaves_device,
            },
        }
    }
}

impl TierReasoner for FixedReasoner {
    fn reason(&self, _ctx: &TierContext) -> TierProposal {
        self.proposal.clone()
    }
}
