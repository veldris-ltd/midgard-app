//! The in-enclosure **Model Registry** — the system of record for every loadable
//! model artefact (SAD_02b §4, FR-MS-3).
//!
//! This is distinct from the engine [`crate::registry`] (which lists the serving
//! *backends* a node might run). The Model Registry is the durable catalogue of
//! *artefacts*: each registered variant, its lifecycle state, its recorded
//! hardware fingerprint, its licence, and the audit trail of every load attempt.
//!
//! It generalises the Phase-2 verify-on-load invariant (AC-MI-7) into **AC-MS-2**:
//! on **every** load the registry verifies the Veldris ML-DSA signature, the licence
//! policy, and hardware eligibility, and it enforces the lifecycle state —
//! **failing closed on any mismatch**. The load invariant that closes the gap is
//! that a model which is *not registered*, whose *signature does not verify*,
//! whose *licence gate is unmet*, that is *revoked*, or that the *node cannot
//! serve* simply **cannot be loaded**. [FR-MS-3] [FR-MS-4]
//!
//! Every crypto check is delegated to the proven [`ModelLoader`] gate — never
//! re-derived. The registry adds lifecycle, the hardware fingerprint of record,
//! the load-event audit, and largest-eligible variant resolution on top.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::artifact::{LicencePolicy, LoadError, LoadedModel, ModelLoader, SignedArtifact};
use crate::handler::{Clock, LogicalClock};
use crate::hardware::{HwProfile, NodeProfile};

/// The lifecycle state of a registered artefact (SAD_02b §4.2). Only `Approved`
/// and `Deprecated` are loadable; `Staged` is not yet imported-and-approved, and
/// `Revoked` is a terminal kill-switch enforced at load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// Assembled/re-signed in staging, not yet imported to the enclosure.
    Staged,
    /// Verified on import; loadable.
    Approved,
    /// Superseded; still loadable but flagged.
    Deprecated,
    /// Withdrawn (licence/CVE/eval regression); not loadable. Terminal.
    Revoked,
}

impl LifecycleState {
    /// Whether an artefact in this state may ever be placed into serving.
    pub fn is_loadable(self) -> bool {
        matches!(self, LifecycleState::Approved | LifecycleState::Deprecated)
    }
}

/// A registered artefact: the signed bundle plus the registry's system-of-record
/// metadata (lifecycle, SBOM/provenance references). The hardware fingerprint is
/// the artefact manifest's [`HwProfile`], recorded per artefact. [FR-MS-5]
#[derive(Debug, Clone)]
pub struct RegisteredArtefact {
    /// The re-signed manifest + Veldris ML-DSA signature.
    pub artifact: SignedArtifact,
    /// The lifecycle state.
    pub state: LifecycleState,
    /// Reference (e.g. digest) to the CycloneDX SBOM captured at stage time.
    pub sbom_ref: Option<String>,
    /// Reference to the provenance record captured at stage time.
    pub provenance_ref: Option<String>,
}

impl RegisteredArtefact {
    /// The artefact id.
    pub fn id(&self) -> &str {
        &self.artifact.manifest.id
    }
    /// The family (Mistral / Nemotron / Gemma / Granite).
    pub fn family(&self) -> &str {
        &self.artifact.manifest.family
    }
    /// The recorded hardware fingerprint (min GPUs / VRAM / multinode). [FR-MS-5]
    pub fn hardware_fingerprint(&self) -> HwProfile {
        self.artifact.manifest.hw
    }
    /// Whether the current state permits loading.
    pub fn is_loadable_state(&self) -> bool {
        self.state.is_loadable()
    }
}

/// An audited load attempt (SAD_02b §4.1 `LoadEvent`). Every call to
/// [`ModelRegistry::load`] appends one, whether it succeeded or failed closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadEvent {
    /// Logical/wall timestamp from the injected clock.
    pub at: u64,
    /// The artefact id the load targeted.
    pub artefact_id: String,
    /// Whether the ML-DSA signature verified during this attempt.
    pub sig_verified: bool,
    /// The node the attempt was evaluated against.
    pub node: NodeProfile,
    /// The outcome (loaded model, or the fail-closed reason).
    pub outcome: Result<LoadedModel, LoadError>,
}

/// Capability ordering key for a hardware profile: a larger key is a more
/// demanding — and, by design, more capable — variant. Used to offer the
/// *largest eligible* variant. [FR-MS-5]
fn capability_key(hw: &HwProfile) -> (u32, u32, u32) {
    (hw.multinode as u32, hw.min_gpus, hw.min_vram_gb)
}

/// The Model Registry: system of record + governed load path.
pub struct ModelRegistry {
    /// The crypto/licence/hardware gate (signature root of trust + licence policy).
    /// Its own revocation set is unused — the registry's lifecycle state is the
    /// single source of truth for revocation.
    loader: ModelLoader,
    /// The catalogue, keyed by artefact id (ordered for deterministic iteration).
    artefacts: BTreeMap<String, RegisteredArtefact>,
    /// The append-only load-attempt audit.
    events: Vec<LoadEvent>,
    /// Injected timestamp source.
    clock: Arc<dyn Clock>,
}

impl ModelRegistry {
    /// Build a registry trusting `trust_key` (the Veldris signing root) under
    /// `policy`, with a deterministic logical clock.
    pub fn new(trust_key: pqc_agility::VerifyingKey, policy: LicencePolicy) -> Self {
        Self::with_clock(trust_key, policy, Arc::new(LogicalClock::default()))
    }

    /// Build a registry with an explicit clock (for deterministic audit tests or a
    /// real wall clock in production).
    pub fn with_clock(
        trust_key: pqc_agility::VerifyingKey,
        policy: LicencePolicy,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            loader: ModelLoader::new(trust_key, policy),
            artefacts: BTreeMap::new(),
            events: Vec::new(),
            clock,
        }
    }

    /// Register a verified-on-import artefact as `Approved` (SAD §3.2: an imported,
    /// signature-verified bundle enters the registry approved). Returns the id.
    /// Re-registering an id replaces the entry. [FR-MS-3]
    pub fn register(
        &mut self,
        artifact: SignedArtifact,
        sbom_ref: Option<String>,
        provenance_ref: Option<String>,
    ) -> String {
        self.register_with_state(artifact, LifecycleState::Approved, sbom_ref, provenance_ref)
    }

    /// Register an artefact in an explicit lifecycle state (e.g. `Staged` before
    /// import approval).
    pub fn register_with_state(
        &mut self,
        artifact: SignedArtifact,
        state: LifecycleState,
        sbom_ref: Option<String>,
        provenance_ref: Option<String>,
    ) -> String {
        let id = artifact.manifest.id.clone();
        self.artefacts.insert(
            id.clone(),
            RegisteredArtefact {
                artifact,
                state,
                sbom_ref,
                provenance_ref,
            },
        );
        id
    }

    /// Transition an artefact to a new lifecycle state. Returns false if unknown.
    pub fn set_state(&mut self, id: &str, state: LifecycleState) -> bool {
        match self.artefacts.get_mut(id) {
            Some(a) => {
                a.state = state;
                true
            }
            None => false,
        }
    }

    /// Revoke an artefact — the fast kill-switch. A revoked artefact fails the load
    /// invariant regardless of signature validity. [FR-MS-4]
    pub fn revoke(&mut self, id: &str) -> bool {
        self.set_state(id, LifecycleState::Revoked)
    }

    /// Mark an artefact deprecated (still loadable, flagged).
    pub fn deprecate(&mut self, id: &str) -> bool {
        self.set_state(id, LifecycleState::Deprecated)
    }

    /// Look up a registered artefact.
    pub fn get(&self, id: &str) -> Option<&RegisteredArtefact> {
        self.artefacts.get(id)
    }

    /// Iterate the catalogue (deterministic id order).
    pub fn artefacts(&self) -> impl Iterator<Item = &RegisteredArtefact> {
        self.artefacts.values()
    }

    /// The append-only load-attempt audit.
    pub fn load_events(&self) -> &[LoadEvent] {
        &self.events
    }

    /// The governed load path — the AC-MS-2 invariant. Verifies revocation/state,
    /// then the Veldris ML-DSA signature, licence policy, and hardware eligibility;
    /// **fails closed** on any mismatch. Every attempt is audited. [FR-MS-4]
    pub fn load(&mut self, id: &str, node: &NodeProfile) -> Result<LoadedModel, LoadError> {
        let outcome = self.evaluate(id, node);
        // The signature verified iff we reached and passed the crypto gate. The
        // family/licence/hardware checks all run *after* the signature in the gate,
        // so those outcomes imply a verified signature; revocation/state/not-found
        // short-circuit before it.
        let sig_verified = matches!(
            outcome,
            Ok(_)
                | Err(LoadError::UnsupportedFamily)
                | Err(LoadError::LicenceDenied)
                | Err(LoadError::HardwareIneligible)
        );
        self.events.push(LoadEvent {
            at: self.clock.now(),
            artefact_id: id.to_string(),
            sig_verified,
            node: *node,
            outcome: outcome.clone(),
        });
        outcome
    }

    /// Whether `id` would pass the full load invariant on `node` right now,
    /// without recording an event. Used by the router to validate an explicit
    /// artefact override before committing to it. [FR-MS-8]
    pub fn is_loadable_on(&self, id: &str, node: &NodeProfile) -> bool {
        self.evaluate(id, node).is_ok()
    }

    /// The load invariant, without recording an event (used by `load` and, as a
    /// dry run, by resolution so it never offers an unloadable variant).
    fn evaluate(&self, id: &str, node: &NodeProfile) -> Result<LoadedModel, LoadError> {
        let art = self.artefacts.get(id).ok_or(LoadError::NotRegistered)?;

        // Lifecycle first: a revoked artefact fails closed regardless of signature
        // validity (AC-MS-7); a still-staged artefact is not yet loadable.
        match art.state {
            LifecycleState::Revoked => return Err(LoadError::Revoked),
            LifecycleState::Staged => return Err(LoadError::NotApproved),
            LifecycleState::Approved | LifecycleState::Deprecated => {}
        }

        // Full crypto/licence/hardware gate (AC-MS-2). A licence-policy failure is
        // refused here exactly as a revoked artefact is — fail closed, not loadable
        // (AC-MS-6); a hardware-ineligible variant is refused too (AC-MS-5).
        self.loader.verify_and_load(&art.artifact, node)
    }

    /// Resolve the **largest eligible**, loadable, policy-permitted artefact of
    /// `family` for `node`. It runs the full gate on each candidate as a dry run,
    /// so it never offers a model the node cannot actually load; among those that
    /// pass, it returns the one with the greatest hardware demand (a proxy for
    /// capability), breaking ties by lexically-smallest id for determinism. Returns
    /// `None` when the node can serve no variant of the family. [FR-MS-5] [AC-MS-5]
    pub fn resolve_largest_eligible(
        &self,
        family: &str,
        node: &NodeProfile,
    ) -> Option<&RegisteredArtefact> {
        let mut best: Option<&RegisteredArtefact> = None;
        for art in self.artefacts.values() {
            if art.family() != family || !art.is_loadable_state() {
                continue;
            }
            if self.evaluate(art.id(), node).is_err() {
                continue; // not actually loadable on this node — never offer it
            }
            best = match best {
                None => Some(art),
                Some(cur) => {
                    let k = capability_key(&art.hardware_fingerprint());
                    let c = capability_key(&cur.hardware_fingerprint());
                    if k > c || (k == c && art.id() < cur.id()) {
                        Some(art)
                    } else {
                        Some(cur)
                    }
                }
            };
        }
        best
    }

    /// Convenience: resolve the largest eligible variant of `family` and load it,
    /// recording the load event. Fails closed with [`LoadError::HardwareIneligible`]
    /// when no variant of the family is eligible on this node. [AC-MS-5]
    pub fn load_largest_eligible(
        &mut self,
        family: &str,
        node: &NodeProfile,
    ) -> Result<LoadedModel, LoadError> {
        match self.resolve_largest_eligible(family, node) {
            Some(art) => {
                let id = art.id().to_string();
                self.load(&id, node)
            }
            None => Err(LoadError::HardwareIneligible),
        }
    }
}
