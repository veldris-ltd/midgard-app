//! The serving core: it holds the autonomously-selected engine, forms batches
//! (continuous batching), drives the KV-cache-aware engine, and attaches serving
//! evidence (locality, latency, cache-hit, batch size) to each completion.
//! [FR-MI-1] [NFR-PERF-1]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::api::{Completion, InferenceRequest};
use crate::backend::{InferError, ModelBackend, PreparedRequest};
use crate::hardware::NodeProfile;
use crate::quant::Quant;
use crate::select::{select, Selection};

/// Default maximum requests fused into one batch.
pub const DEFAULT_MAX_BATCH: usize = 16;

/// The first-token latency budget on reference hardware. [NFR-PERF-1]
pub const FIRST_TOKEN_BUDGET_MS: u32 = 800;

/// The serving core, built once per node from the engine registry.
pub struct Server {
    backend: Arc<dyn ModelBackend>,
    selection: Selection,
    max_batch: usize,
    batches_formed: AtomicU64,
}

impl Server {
    /// Autonomously select an engine for `node` and build a server around it.
    /// `override_id` pins a specific engine (fails closed if ineligible).
    pub fn bootstrap(
        node: NodeProfile,
        registry: &[Arc<dyn ModelBackend>],
        override_id: Option<&str>,
    ) -> Result<Self, InferError> {
        let (backend, selection) = select(&node, registry, override_id)?;
        Ok(Self {
            backend,
            selection,
            max_batch: DEFAULT_MAX_BATCH,
            batches_formed: AtomicU64::new(0),
        })
    }

    /// Override the batch ceiling (mainly for tests).
    pub fn with_max_batch(mut self, max_batch: usize) -> Self {
        self.max_batch = max_batch.max(1);
        self
    }

    /// The engine this server selected, and why.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Total batches formed so far (continuous-batching evidence).
    pub fn batches_formed(&self) -> u64 {
        self.batches_formed.load(Ordering::Relaxed)
    }

    /// Resolve a typed request against the selected engine (choosing the quant).
    pub fn prepare(&self, req: &InferenceRequest) -> PreparedRequest {
        let quant = self.backend.descriptor().resolve_quant(req.quant_pref);
        PreparedRequest {
            prompt: req.prompt.clone(),
            max_tokens: req.max_tokens,
            quant,
        }
    }

    /// Serve a single prepared request (a batch of one). Returns the completion
    /// with serving evidence attached; `audit_seq` is filled in by the caller
    /// (the audited gateway handler).
    pub fn infer(&self, req: PreparedRequest) -> Result<Completion, InferError> {
        Ok(self.infer_many(vec![req])?.pop().expect("one in, one out"))
    }

    /// Serve many requests, chunked into batches of at most `max_batch`. Each
    /// chunk is one engine call; wall-clock is bounded by the slowest chunk, not
    /// the sum of requests. Returns completions in input order.
    pub fn infer_many(&self, reqs: Vec<PreparedRequest>) -> Result<Vec<Completion>, InferError> {
        let d = self.backend.descriptor();
        let locality = d.locality;
        let model_id = d.id.clone();

        let mut out = Vec::with_capacity(reqs.len());
        for chunk in reqs.chunks(self.max_batch.max(1)) {
            let batch_size = chunk.len() as u32;
            let raws = self.backend.infer_batch(chunk)?;
            self.batches_formed.fetch_add(1, Ordering::Relaxed);
            for (raw, prepared) in raws.into_iter().zip(chunk.iter()) {
                out.push(Completion {
                    text: raw.text,
                    model_id: model_id.clone(),
                    locality,
                    quant: prepared.quant,
                    first_token_ms: raw.first_token_ms,
                    cache_hit: raw.cache_hit,
                    batch_size,
                    prompt_tokens: raw.prompt_tokens,
                    completion_tokens: raw.completion_tokens,
                    // Overwritten by the audited handler with the real seq.
                    audit_seq: 0,
                });
            }
        }
        Ok(out)
    }

    /// The selected engine's default quantisation (diagnostics).
    pub fn default_quant(&self) -> Quant {
        self.backend.descriptor().default_quant()
    }
}
