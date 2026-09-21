//! The router — the policy half of the Model Abstraction Layer (SAD_02b §5.2).
//! [FR-MS-8]
//!
//! Behind the **unchanged inference contract** ([`InferenceRequest`]), the router
//! resolves *which* locally-registered artefact answers a request, from three
//! inputs: the **workload class** (a family preference), **hardware eligibility**
//! (the artefact's `HwProfile` against the node), and **governance policy** (which
//! families are permitted). Selection is deterministic, **logged on every call**,
//! and overridable by an explicit request. Because this lives behind the one
//! contract, consumers need no code change when families are added, swapped, or
//! revoked (AC-MS-4, AC-MS-9), and — serving only from the verified in-enclosure
//! registry — it never reaches a hosted dependency (AC-MS-8). [DD-6] [NFR-SEC-2]

use std::collections::BTreeSet;

use crate::adapter::AdapterSet;
use crate::api::{InferenceRequest, WorkloadClass};
use crate::artifact::LoadError;
use crate::backend::{InferError, Locality, ModelBackend, PreparedRequest};
use crate::hardware::NodeProfile;
use crate::model_registry::ModelRegistry;
use crate::quant::Quant;

/// Which families a deployment permits, and the balanced default. This is the
/// governance input to routing (resolved for the request's markings/purpose/
/// tenant upstream); the router treats it as an allow-list. [NFR-SEC-2]
#[derive(Debug, Clone)]
pub struct RoutingPolicy {
    permitted_families: BTreeSet<String>,
    default_family: String,
}

impl RoutingPolicy {
    /// A policy permitting `permitted` families with `default_family` as the
    /// balanced default. The default is added to the permitted set.
    pub fn new(
        permitted: impl IntoIterator<Item = String>,
        default_family: impl Into<String>,
    ) -> Self {
        let default_family = default_family.into();
        let mut permitted_families: BTreeSet<String> = permitted.into_iter().collect();
        permitted_families.insert(default_family.clone());
        Self {
            permitted_families,
            default_family,
        }
    }

    /// Permit all three supported families with GLM as the balanced default.
    pub fn permit_all_three() -> Self {
        Self::new(["GLM", "Nemotron", "Gemma"].map(String::from), "GLM")
    }

    /// Whether `family` is permitted.
    pub fn permits(&self, family: &str) -> bool {
        self.permitted_families.contains(family)
    }

    /// The permitted families, deterministically ordered.
    pub fn permitted(&self) -> impl Iterator<Item = &String> {
        self.permitted_families.iter()
    }

    /// The ideal family for a workload class (SAD_02b §5.3 defaults). Balanced and
    /// Embedding fall to the policy default. This is a *preference*, not a binding:
    /// the router degrades to another permitted family if none is eligible.
    fn ideal_family(&self, w: WorkloadClass) -> &str {
        match w {
            WorkloadClass::FastTransform => "Gemma",
            WorkloadClass::DeepReasoning => "GLM",
            WorkloadClass::CodeAnalysis => "Nemotron",
            WorkloadClass::Balanced | WorkloadClass::Embedding => &self.default_family,
        }
    }

    /// Candidate families in priority order: the ideal first (if permitted), then
    /// the remaining permitted families in deterministic (sorted) order.
    fn candidate_order(&self, ideal: &str) -> Vec<String> {
        let mut order = Vec::with_capacity(self.permitted_families.len());
        if self.permitted_families.contains(ideal) {
            order.push(ideal.to_string());
        }
        for f in &self.permitted_families {
            if f != ideal {
                order.push(f.clone());
            }
        }
        order
    }
}

/// A recorded routing decision — one is logged for **every** selection. [FR-MS-8]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingDecision {
    /// The artefact id selected.
    pub artefact_id: String,
    /// The family it belongs to.
    pub family: String,
    /// The workload class that drove the choice.
    pub workload_class: WorkloadClass,
    /// Whether an explicit override picked it (vs policy/workload routing).
    pub overridden: bool,
    /// Human-readable, audited rationale.
    pub reason: String,
    /// The node the decision was made against.
    pub node: NodeProfile,
}

/// A routed, served completion. Same shape of evidence as a [`crate::Completion`]
/// but tagged with the routing decision; the transport handler attaches the audit
/// sequence (AC-MI-8) — routing does not change that contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedCompletion {
    /// The routing decision that produced it.
    pub decision: RoutingDecision,
    /// The post-processed (stop-token-trimmed) generated text.
    pub text: String,
    /// The routed artefact id.
    pub model_id: String,
    /// The family that answered.
    pub family: String,
    /// Where it ran — always in-enclosure. [AC-MS-8]
    pub locality: Locality,
    /// Quantisation actually used.
    pub quant: Quant,
    /// First-token latency (ms).
    pub first_token_ms: u32,
    /// Whether the prompt prefix hit the KV-cache.
    pub cache_hit: bool,
    /// Approximate prompt token count.
    pub prompt_tokens: u32,
    /// Generated token count.
    pub completion_tokens: u32,
}

/// Why routing failed — all fail closed (no hosted fallback, ever).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    /// The (explicitly requested or only-candidate) family is not permitted.
    PolicyDenied(String),
    /// No permitted family has an artefact this node can serve.
    NoEligibleArtefact,
    /// An explicit artefact override is unknown or not loadable on this node.
    OverrideNotRoutable(String),
    /// The verify-on-load invariant failed closed for the selected artefact.
    Load(LoadError),
    /// The selected family has no adapter registered.
    NoAdapter(String),
    /// The serving engine could not serve.
    Serve(InferError),
}

/// The Model Abstraction Layer router.
pub struct Router {
    policy: RoutingPolicy,
    adapters: AdapterSet,
    decisions: Vec<RoutingDecision>,
}

impl Router {
    /// A router with the default GLM/Nemotron/Gemma adapters under `policy`.
    pub fn new(policy: RoutingPolicy) -> Self {
        Self::with_adapters(policy, AdapterSet::default_three())
    }

    /// A router with an explicit adapter set.
    pub fn with_adapters(policy: RoutingPolicy, adapters: AdapterSet) -> Self {
        Self {
            policy,
            adapters,
            decisions: Vec::new(),
        }
    }

    /// The audited log of every routing decision made.
    pub fn decisions(&self) -> &[RoutingDecision] {
        &self.decisions
    }

    /// Select the artefact for a request, without serving or logging. Honours an
    /// explicit `backend_override` (interpreted as an artefact id) iff it is
    /// permitted and loadable; otherwise routes by workload → policy → hardware,
    /// degrading across permitted families for graceful degradation. [FR-MS-8]
    pub fn select(
        &self,
        req: &InferenceRequest,
        registry: &ModelRegistry,
        node: &NodeProfile,
    ) -> Result<RoutingDecision, RouteError> {
        // Explicit override: an artefact id pinned by the caller.
        if let Some(id) = &req.backend_override {
            let art = registry
                .get(id)
                .ok_or_else(|| RouteError::OverrideNotRoutable(id.clone()))?;
            if !self.policy.permits(art.family()) {
                return Err(RouteError::PolicyDenied(art.family().to_string()));
            }
            if !registry.is_loadable_on(id, node) {
                return Err(RouteError::OverrideNotRoutable(id.clone()));
            }
            return Ok(RoutingDecision {
                artefact_id: id.clone(),
                family: art.family().to_string(),
                workload_class: req.workload_class,
                overridden: true,
                reason: format!("explicit override to artefact '{id}' (permitted, eligible)"),
                node: *node,
            });
        }

        // Workload → ideal family, then degrade across permitted families.
        let ideal = self.policy.ideal_family(req.workload_class).to_string();
        if self.policy.permitted_families.is_empty() {
            return Err(RouteError::PolicyDenied("<none permitted>".into()));
        }

        for family in self.policy.candidate_order(&ideal) {
            if let Some(art) = registry.resolve_largest_eligible(&family, node) {
                let reason = if family == ideal {
                    format!(
                        "workload {:?} -> {} (largest eligible artefact on node)",
                        req.workload_class, family
                    )
                } else {
                    format!(
                        "workload {:?} ideal '{}' unavailable; degraded to permitted '{}'",
                        req.workload_class, ideal, family
                    )
                };
                return Ok(RoutingDecision {
                    artefact_id: art.id().to_string(),
                    family,
                    workload_class: req.workload_class,
                    overridden: false,
                    reason,
                    node: *node,
                });
            }
        }
        Err(RouteError::NoEligibleArtefact)
    }

    /// Route and serve a request end-to-end: select (and **log**) the artefact,
    /// verify-load it from the registry (fail closed), render the prompt through
    /// the family adapter, serve on the in-enclosure `engine`, and trim the
    /// family's stop tokens. Serving only from the verified local registry, this
    /// never touches a hosted dependency. [AC-MS-4] [AC-MS-8]
    pub fn serve(
        &mut self,
        req: &InferenceRequest,
        registry: &mut ModelRegistry,
        node: &NodeProfile,
        engine: &dyn ModelBackend,
    ) -> Result<RoutedCompletion, RouteError> {
        let decision = self.select(req, registry, node)?;
        // Log every selection (FR-MS-8), whether or not the subsequent load serves.
        self.decisions.push(decision.clone());

        // Verify-on-load: the artefact must pass the full AC-MS-2 gate to serve.
        let loaded = registry
            .load(&decision.artefact_id, node)
            .map_err(RouteError::Load)?;

        // Family-specific templating lives here, not in the prompt.
        let adapter = self
            .adapters
            .get(&decision.family)
            .ok_or_else(|| RouteError::NoAdapter(decision.family.clone()))?;
        let templated = adapter.render(&req.prompt);

        let quant = engine.descriptor().resolve_quant(req.quant_pref);
        let prepared = PreparedRequest {
            prompt: templated,
            max_tokens: req.max_tokens,
            quant,
        };
        let mut raw = engine
            .infer_batch(std::slice::from_ref(&prepared))
            .map_err(RouteError::Serve)?;
        let completion = raw
            .pop()
            .ok_or_else(|| RouteError::Serve(InferError::Unavailable("empty batch".into())))?;

        Ok(RoutedCompletion {
            text: adapter.postprocess(&completion.text),
            model_id: loaded.id,
            family: loaded.family,
            locality: engine.descriptor().locality,
            quant,
            first_token_ms: completion.first_token_ms,
            cache_hit: completion.cache_hit,
            prompt_tokens: completion.prompt_tokens,
            completion_tokens: completion.completion_tokens,
            decision,
        })
    }
}
