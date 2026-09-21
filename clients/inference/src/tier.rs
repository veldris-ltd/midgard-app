//! The tier decision: on-device (Tier 1), remote (Tier 2), hybrid (Tier 3), or a
//! graceful disable. `[FR-C-6]` `[FR-C-10]`
//!
//! The choice is a **runtime** decision, not a build-time one (KDD-3): the same
//! binary tiers per request from device capability, connectivity, and policy. A
//! **deterministic heuristic** decides the clear cases; only genuinely ambiguous
//! cases consult the advisory A.C2 reasoner. Either way, a deterministic
//! [`guard`] has the final say — it clamps any tier that would violate a hard
//! invariant (data leaving the device without egress permission, an on-device tier
//! on a device with no model), so the model can never talk the client into an
//! unsafe or impossible tier. `[NFR-AI-1]`

/// The minimum RAM (MiB) an int4 on-device model needs to run at all.
pub const MIN_TIER1_RAM_MB: u32 = 4096;

/// Which inference tier serves a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Tier 1 — a small quantised model runs locally.
    OnDevice,
    /// Tier 2 — the enclosure's full Mimir models over PQ-mTLS.
    Remote,
    /// Tier 3 — an on-device draft verified/augmented by the enclosure.
    Hybrid,
    /// The feature disables cleanly (weak device + offline, or policy leaves no
    /// safe option). `[FR-C-10]`
    Disable,
}

impl Tier {
    /// Whether serving at this tier sends request data off the device.
    pub fn data_leaves_device(self) -> bool {
        matches!(self, Tier::Remote | Tier::Hybrid)
    }
}

/// Connectivity to the enclosure at request time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connectivity {
    /// A PQ-mTLS channel to the enclosure is available.
    Online,
    /// Disconnected — only local capability is available.
    Offline,
}

/// The user's / policy's privacy stance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyPreference {
    /// No special preference; correctness/performance may pick remote.
    Standard,
    /// Keep work on-device where possible.
    PreferOnDevice,
}

/// How heavy the task is relative to on-device capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskComplexity {
    /// Well within on-device capability.
    Light,
    /// Borderline — the ambiguous case that consults A.C2.
    Moderate,
    /// Beyond comfortable on-device capability.
    Heavy,
}

/// What the device can do locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceCapability {
    /// Usable RAM in MiB.
    pub ram_mb: u32,
    /// A GPU/NPU accelerator is present.
    pub has_accelerator: bool,
    /// A verified Tier-1 model is loaded and ready.
    pub on_device_model: bool,
}

impl DeviceCapability {
    /// Whether this device can serve a request on-device at all: a verified model
    /// is present and there is enough RAM to run it.
    pub fn is_capable(self) -> bool {
        self.on_device_model && self.ram_mb >= MIN_TIER1_RAM_MB
    }
}

/// Everything the tier decision needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierContext {
    /// Device capability probe.
    pub capability: DeviceCapability,
    /// Connectivity to the enclosure.
    pub connectivity: Connectivity,
    /// Whether policy permits request data to leave the device.
    pub allow_egress: bool,
    /// Privacy stance.
    pub privacy: PrivacyPreference,
    /// Task complexity.
    pub complexity: TaskComplexity,
}

impl TierContext {
    /// Shorthand: is the device capable of on-device inference right now?
    pub fn capable(&self) -> bool {
        self.capability.is_capable()
    }
    /// Shorthand: is the enclosure reachable?
    pub fn online(&self) -> bool {
        self.connectivity == Connectivity::Online
    }
}

/// The outcome of the fast-path heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heuristic {
    /// The tier is clear without consulting the model.
    Decided(Tier),
    /// The case is genuinely ambiguous — consult the A.C2 reasoner.
    Ambiguous,
}

/// The deterministic fast path. Resolves the clear cases; returns
/// [`Heuristic::Ambiguous`] only when privacy/correctness/performance genuinely
/// trade off (a capable, connected device on a borderline task). `[FR-C-6]`
pub fn heuristic(ctx: &TierContext) -> Heuristic {
    let capable = ctx.capable();

    if !ctx.online() {
        // Offline: on-device if capable, else disable gracefully. `[AC-C-5]`
        return Heuristic::Decided(if capable {
            Tier::OnDevice
        } else {
            Tier::Disable
        });
    }

    if !ctx.allow_egress {
        // Data may not leave the device: never remote/hybrid.
        return Heuristic::Decided(if capable {
            Tier::OnDevice
        } else {
            Tier::Disable
        });
    }

    match ctx.complexity {
        // Beyond on-device: delegate to the enclosure.
        TaskComplexity::Heavy => {
            Heuristic::Decided(if capable { Tier::Hybrid } else { Tier::Remote })
        }
        TaskComplexity::Light => {
            if capable && ctx.privacy == PrivacyPreference::PreferOnDevice {
                Heuristic::Decided(Tier::OnDevice)
            } else if !capable {
                Heuristic::Decided(Tier::Remote)
            } else {
                // Capable, light, egress-ok, no privacy preference: correctness vs
                // performance — let A.C2 weigh in.
                Heuristic::Ambiguous
            }
        }
        // The borderline case is exactly what the reasoner is for.
        TaskComplexity::Moderate => Heuristic::Ambiguous,
    }
}

/// The final deterministic clamp. Applied to *every* tier — whether the heuristic
/// or the reasoner chose it — so no advisory decision can violate a hard
/// invariant. `[NFR-AI-1]`
///
/// - Data may only leave the device if egress is permitted.
/// - An on-device tier requires a capable device.
/// - Remote requires connectivity.
///
/// A tier that cannot be satisfied degrades to the safest reachable option,
/// bottoming out at [`Tier::Disable`] rather than doing something unsafe.
pub fn guard(tier: Tier, ctx: &TierContext) -> Tier {
    let capable = ctx.capable();
    let online = ctx.online();

    match tier {
        Tier::OnDevice => {
            if capable {
                Tier::OnDevice
            } else {
                Tier::Disable
            }
        }
        Tier::Remote => {
            if online && ctx.allow_egress {
                Tier::Remote
            } else if capable {
                // Can't (or mustn't) go remote → keep it local.
                Tier::OnDevice
            } else {
                Tier::Disable
            }
        }
        Tier::Hybrid => {
            if online && ctx.allow_egress && capable {
                Tier::Hybrid
            } else if online && ctx.allow_egress {
                // No local model to draft with, but egress is fine → remote.
                Tier::Remote
            } else if capable {
                Tier::OnDevice
            } else {
                Tier::Disable
            }
        }
        Tier::Disable => Tier::Disable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(ram: u32, model: bool) -> DeviceCapability {
        DeviceCapability {
            ram_mb: ram,
            has_accelerator: model,
            on_device_model: model,
        }
    }

    fn ctx(
        capability: DeviceCapability,
        connectivity: Connectivity,
        allow_egress: bool,
        privacy: PrivacyPreference,
        complexity: TaskComplexity,
    ) -> TierContext {
        TierContext {
            capability,
            connectivity,
            allow_egress,
            privacy,
            complexity,
        }
    }

    #[test]
    fn capable_offline_is_on_device() {
        let c = ctx(
            cap(8192, true),
            Connectivity::Offline,
            true,
            PrivacyPreference::Standard,
            TaskComplexity::Moderate,
        );
        assert_eq!(heuristic(&c), Heuristic::Decided(Tier::OnDevice));
    }

    #[test]
    fn weak_offline_disables() {
        let c = ctx(
            cap(1024, false),
            Connectivity::Offline,
            true,
            PrivacyPreference::Standard,
            TaskComplexity::Light,
        );
        assert_eq!(heuristic(&c), Heuristic::Decided(Tier::Disable));
    }

    #[test]
    fn sensitive_data_stays_on_device_when_capable() {
        let c = ctx(
            cap(8192, true),
            Connectivity::Online,
            false, // egress forbidden = sensitive
            PrivacyPreference::Standard,
            TaskComplexity::Heavy,
        );
        assert_eq!(heuristic(&c), Heuristic::Decided(Tier::OnDevice));
    }

    #[test]
    fn heavy_connected_capable_is_hybrid() {
        let c = ctx(
            cap(8192, true),
            Connectivity::Online,
            true,
            PrivacyPreference::Standard,
            TaskComplexity::Heavy,
        );
        assert_eq!(heuristic(&c), Heuristic::Decided(Tier::Hybrid));
    }

    #[test]
    fn heavy_connected_incapable_is_remote() {
        let c = ctx(
            cap(1024, false),
            Connectivity::Online,
            true,
            PrivacyPreference::Standard,
            TaskComplexity::Heavy,
        );
        assert_eq!(heuristic(&c), Heuristic::Decided(Tier::Remote));
    }

    #[test]
    fn moderate_capable_online_is_ambiguous() {
        let c = ctx(
            cap(8192, true),
            Connectivity::Online,
            true,
            PrivacyPreference::Standard,
            TaskComplexity::Moderate,
        );
        assert_eq!(heuristic(&c), Heuristic::Ambiguous);
    }

    #[test]
    fn guard_clamps_remote_without_egress_to_on_device() {
        // EV-AC2-policy: a proposed remote tier under a no-egress policy is
        // downgraded to on-device (capable) — data must not leave the device.
        let c = ctx(
            cap(8192, true),
            Connectivity::Online,
            false,
            PrivacyPreference::Standard,
            TaskComplexity::Moderate,
        );
        assert_eq!(guard(Tier::Remote, &c), Tier::OnDevice);
    }

    #[test]
    fn guard_clamps_remote_without_egress_and_incapable_to_disable() {
        let c = ctx(
            cap(1024, false),
            Connectivity::Online,
            false,
            PrivacyPreference::Standard,
            TaskComplexity::Moderate,
        );
        assert_eq!(guard(Tier::Remote, &c), Tier::Disable);
    }

    #[test]
    fn guard_clamps_on_device_without_model_to_disable() {
        let c = ctx(
            cap(1024, false),
            Connectivity::Offline,
            true,
            PrivacyPreference::Standard,
            TaskComplexity::Light,
        );
        assert_eq!(guard(Tier::OnDevice, &c), Tier::Disable);
    }

    #[test]
    fn guard_downgrades_hybrid_without_local_model_to_remote() {
        let c = ctx(
            cap(1024, false),
            Connectivity::Online,
            true,
            PrivacyPreference::Standard,
            TaskComplexity::Heavy,
        );
        assert_eq!(guard(Tier::Hybrid, &c), Tier::Remote);
    }
}
