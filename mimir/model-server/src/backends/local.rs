//! The deterministic, in-enclosure reference engine.
//!
//! This stands in for a real local model (Mistral / Nemotron / Gemma / Granite) so that the
//! whole control plane — the typed API, continuous batching, the KV-cache,
//! quantisation accounting, PQ-mTLS exposure, and per-call audit — is exercised
//! and the acceptance criteria proven **offline, with no GPU and no network**.
//! The GPU-backed engines answer through the identical [`ModelBackend`] seam, so
//! nothing above this line changes when they are placed. [FR-MI-1] [AC-MI-1]
//!
//! It is deterministic on purpose: identical inputs yield identical output, which
//! keeps the Evals (Prompt 2.d onward) and bit-reproducible builds honest.

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::backend::{
    BackendDescriptor, InferError, Locality, ModelBackend, PreparedRequest, RawCompletion,
};
use crate::hardware::HwProfile;
use crate::quant::Quant;

/// Base first-token cost of the reference engine, milliseconds.
const BASE_MS: f32 = 12.0;
/// Per-uncached-prompt-token first-token cost, milliseconds.
const PER_TOKEN_MS: f32 = 0.4;
/// Cached first-token cost (prefix reuse), milliseconds.
const CACHED_MS: f32 = 6.0;
/// Prefix length (chars) used as the KV-cache key.
const PREFIX_CHARS: usize = 64;

/// The reference engine. Runs anywhere; requires no accelerator.
pub struct LocalReference {
    descriptor: BackendDescriptor,
    /// KV-cache: prompt-prefix hashes seen this process. Presence => prefix reuse.
    kv: Mutex<BTreeSet<u64>>,
}

impl Default for LocalReference {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalReference {
    /// Build the reference engine.
    pub fn new() -> Self {
        Self {
            descriptor: BackendDescriptor {
                id: "local-reference".to_string(),
                locality: Locality::InEnclosure,
                hw: HwProfile::none(),
                // Full precision and GGUF-class packing are both meaningful on a
                // CPU engine.
                quants: vec![Quant::None, Quant::Gguf, Quant::Int8],
                rank: 10,
                families: vec!["reference".to_string()],
            },
            kv: Mutex::new(BTreeSet::new()),
        }
    }

    /// FNV-1a over the prompt prefix — the KV-cache key.
    fn prefix_hash(prompt: &str) -> u64 {
        let end = prompt
            .char_indices()
            .nth(PREFIX_CHARS)
            .map(|(i, _)| i)
            .unwrap_or(prompt.len());
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in prompt[..end].as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    fn token_count(text: &str) -> u32 {
        // A stable, dependency-free token proxy: whitespace-split words, min 1.
        text.split_whitespace().count().max(1) as u32
    }

    /// Deterministically synthesise a completion body from the prompt. This is a
    /// stand-in for real generation; it never reaches the network.
    fn synthesise(prompt: &str, max_tokens: u32) -> (String, u32) {
        let words: Vec<&str> = prompt.split_whitespace().collect();
        let budget = max_tokens.clamp(1, 64) as usize;
        let mut out = String::from("[local-reference] ");
        let mut n = 0u32;
        // Echo-summarise the prompt's salient words, bounded by the budget.
        for w in words.iter().rev().take(budget) {
            if n > 0 {
                out.push(' ');
            }
            out.push_str(w);
            n += 1;
        }
        if n == 0 {
            out.push_str("acknowledged");
            n = 1;
        }
        (out, n)
    }
}

impl ModelBackend for LocalReference {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }

    fn infer_batch(&self, batch: &[PreparedRequest]) -> Result<Vec<RawCompletion>, InferError> {
        let mut kv = self.kv.lock().expect("kv cache poisoned");
        let mut out = Vec::with_capacity(batch.len());
        for req in batch {
            let prompt_tokens = Self::token_count(&req.prompt);
            let key = Self::prefix_hash(&req.prompt);
            let cache_hit = kv.contains(&key);
            kv.insert(key);

            let first_token_ms = if cache_hit {
                CACHED_MS
            } else {
                BASE_MS + PER_TOKEN_MS * prompt_tokens as f32 * req.quant.latency_scale()
            }
            .round() as u32;

            let (text, completion_tokens) = Self::synthesise(&req.prompt, req.max_tokens);
            out.push(RawCompletion {
                text,
                first_token_ms,
                cache_hit,
                prompt_tokens,
                completion_tokens,
            });
        }
        Ok(out)
    }
}
