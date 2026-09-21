//! OAG-grounded response: the end-to-end path that makes AC-MI-2 hold — an
//! answer cites the object-fabric evidence it used.
//!
//! `plan (A.2) → ground (fabric, cited) → assemble (8 layers) → serve`. If the
//! task cannot be grounded, or no readable evidence exists, the response is a
//! **refusal**, never a fabricated answer (A.1 operating principle #1). When it
//! does answer, the citations are exactly the grounding facts presented to the
//! model — the evidence used. The governed loop and the model's own per-claim
//! citation selection (the A.1 output contract) arrive in Prompt 2.d; this proves
//! the evidence-binding mechanism. [FR-MI-2] [NFR-AI-1]

use mimir_prompts::{A1_DEFAULT_OUTPUT_CONTRACT, A1_MIMIR_CORE_AGENT};
use model_server::{InferenceRequest, Server, WorkloadClass};
use oag_retriever::{formulate_plan, ground, GroundingSource, PolicyContext, RetrievalPlan};

use crate::assembly::{AssemblyConfig, AssemblyError, Layers, Pipeline};

/// A structured refusal (no answer produced).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Machine-readable code (e.g. `UNGROUNDED`, `INFER_ERROR`).
    pub code: String,
    /// Human-readable detail.
    pub detail: String,
}

/// The result of a grounded response.
#[derive(Debug, Clone)]
pub struct GroundedResponse {
    /// The answer, when grounded and served.
    pub answer: Option<String>,
    /// Citations for the evidence used (`obj:<id>`), empty on refusal.
    pub citations: Vec<String>,
    /// A refusal, when the task could not be grounded/answered.
    pub refusal: Option<Refusal>,
    /// The retrieval plan formulated (A.2).
    pub plan: RetrievalPlan,
    /// Grounding facts dropped by the token budget.
    pub dropped_facts: usize,
    /// PQ-signed audit sequence numbers of the retrieval queries.
    pub retrieval_audit_seqs: Vec<u64>,
    /// The engine that served (diagnostics).
    pub model_id: String,
}

/// Response configuration.
#[derive(Debug, Clone)]
pub struct RespondConfig {
    /// The prompt token budget.
    pub budget_tokens: usize,
    /// The least-privilege tool schemas for this turn (layer 5).
    pub tools: Vec<String>,
    /// Generation budget.
    pub max_tokens: u32,
}

impl Default for RespondConfig {
    fn default() -> Self {
        Self {
            budget_tokens: 2048,
            tools: Vec::new(),
            max_tokens: 32,
        }
    }
}

/// Render the policy context (layer 2). Deterministic (sorted sets).
pub fn render_policy(ctx: &PolicyContext) -> String {
    let roles = ctx
        .roles
        .iter()
        .map(|r| r.0.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let comps = ctx
        .clearance
        .compartments
        .iter()
        .map(|c| c.0.clone())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "subject: {}\nroles: {}\nclearance: L{} [{}]\npurpose: {}",
        ctx.subject, roles, ctx.clearance.level.0, comps, ctx.purpose.0
    )
}

fn refusal(
    code: &str,
    detail: String,
    plan: RetrievalPlan,
    audit_seqs: Vec<u64>,
    model_id: String,
) -> GroundedResponse {
    GroundedResponse {
        answer: None,
        citations: Vec::new(),
        refusal: Some(Refusal {
            code: code.to_string(),
            detail,
        }),
        plan,
        dropped_facts: 0,
        retrieval_audit_seqs: audit_seqs,
        model_id,
    }
}

/// Produce an OAG-grounded response to `task`.
pub fn respond(
    task: &str,
    ctx: &PolicyContext,
    src: &mut dyn GroundingSource,
    server: &Server,
    pipeline: &Pipeline,
    cfg: &RespondConfig,
    now: u64,
) -> Result<GroundedResponse, AssemblyError> {
    let model_id = server.selection().backend_id.clone();
    let summary = src.ontology_summary();
    let plan = formulate_plan(task, &summary, &ctx.clearance);

    // A.2: not groundable in this ontology → refuse, do not guess.
    if !plan.groundable {
        let detail = plan.reason_if_not.clone().unwrap_or_default();
        return Ok(refusal("UNGROUNDED", detail, plan, Vec::new(), model_id));
    }

    let grounding = ground(&plan, src, ctx, now);

    // Groundable in principle, but nothing readable was retrieved (e.g. all
    // evidence is above clearance) → refuse rather than answer ungrounded.
    if grounding.is_empty() {
        return Ok(refusal(
            "UNGROUNDED",
            "no readable object-fabric evidence for this task".to_string(),
            plan,
            grounding.audit_seqs,
            model_id,
        ));
    }

    let layers = Layers {
        system: A1_MIMIR_CORE_AGENT,
        output_contract: A1_DEFAULT_OUTPUT_CONTRACT,
        policy: render_policy(ctx),
        grounding: &grounding,
        memory: Vec::new(),
        tools: cfg.tools.clone(),
        history: Vec::new(),
        user_message: task,
    };
    let assembled = pipeline.assemble(
        &layers,
        &AssemblyConfig {
            budget_tokens: cfg.budget_tokens,
        },
    )?;

    let req = InferenceRequest {
        purpose: ctx.purpose.0.clone(),
        workload_class: WorkloadClass::Balanced,
        quant_pref: None,
        backend_override: None,
        max_tokens: cfg.max_tokens,
        prompt: assembled.text.clone(),
    };
    match server.infer(server.prepare(&req)) {
        Ok(c) => Ok(GroundedResponse {
            answer: Some(c.text),
            // The evidence used = the grounding presented to the model.
            citations: assembled.included_citations,
            refusal: None,
            plan,
            dropped_facts: assembled.dropped_facts,
            retrieval_audit_seqs: grounding.audit_seqs,
            model_id,
        }),
        Err(e) => Ok(refusal(
            "INFER_ERROR",
            e.to_string(),
            plan,
            grounding.audit_seqs,
            model_id,
        )),
    }
}
