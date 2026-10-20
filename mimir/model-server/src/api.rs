//! The typed inference API and its canonical wire codec.
//!
//! This is the single contract every consumer (Forge, Bifrost, Heimdall, and
//! Mimir's own agent runtime) calls. It is deliberately **model-agnostic**: the
//! caller states a workload class and (optionally) a quantisation/backend
//! preference, and the plane resolves which local engine answers — so the
//! contract is unchanged when Phase 3 adds the Mistral / Nemotron / Gemma / Granite router behind
//! it. [FR-MI-1] [DD-6]
//!
//! Encoding is a hand-rolled, canonical, length-prefixed binary form — **no
//! serde** — matching the sovereign/air-gap posture of the rest of the suite.
//! Decoding is bounds-checked and **fails closed** on any malformed input.

use crate::backend::Locality;
use crate::quant::Quant;

/// The class of work a request represents. The selector/router maps this to an
/// eligible engine (Phase 2: one local engine; Phase 3: per-family routing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadClass {
    /// Low-latency transforms (e.g. Forge NL→transform).
    FastTransform,
    /// Heavy reasoning (e.g. Heimdall course-of-action).
    DeepReasoning,
    /// Code/agentic analysis (e.g. Bifrost release risk).
    CodeAnalysis,
    /// General balanced default.
    Balanced,
    /// Embedding generation.
    Embedding,
}

impl WorkloadClass {
    /// Wire tag.
    pub fn as_u8(self) -> u8 {
        match self {
            WorkloadClass::FastTransform => 1,
            WorkloadClass::DeepReasoning => 2,
            WorkloadClass::CodeAnalysis => 3,
            WorkloadClass::Balanced => 4,
            WorkloadClass::Embedding => 5,
        }
    }
    /// Decode a wire tag; `None` if unknown (fail closed).
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => WorkloadClass::FastTransform,
            2 => WorkloadClass::DeepReasoning,
            3 => WorkloadClass::CodeAnalysis,
            4 => WorkloadClass::Balanced,
            5 => WorkloadClass::Embedding,
            _ => return None,
        })
    }
}

/// A typed inference request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceRequest {
    /// The declared purpose of access (audited, and checked by policy).
    pub purpose: String,
    /// The workload class, used to route to an eligible engine.
    pub workload_class: WorkloadClass,
    /// An explicit quantisation preference, honoured only if the selected engine
    /// supports it; otherwise the engine's default is used.
    pub quant_pref: Option<Quant>,
    /// An explicit backend id override (e.g. pin a tier). Selection fails closed
    /// if the pinned backend is not hardware-eligible — an explicit request is
    /// never silently downgraded.
    pub backend_override: Option<String>,
    /// Generation budget.
    pub max_tokens: u32,
    /// The prompt (already assembled by the caller; the 8-layer assembly pipeline
    /// arrives in Prompt 2.c).
    pub prompt: String,
    /// A short label from the calling product saying what the call is for and who asked
    /// (e.g. "heimdall · advice on alert A-1 · asked by analyst"), shown in the Mimir
    /// console's session traces. Optional and not policy-checked; encoded only when
    /// present, so a request without one is byte-for-byte as before. [FR-MI-8]
    pub context: Option<String>,
}

/// The longest [`InferenceRequest::context`] kept, in characters.
pub const MAX_CONTEXT_CHARS: usize = 160;

/// A typed inference response: either a served completion or a fail-closed
/// refusal. Both carry the sequence number of the PQ-signed audit entry that
/// recorded the call, so the caller holds cryptographic evidence of AC-MI-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceResponse {
    /// A locally-served completion.
    Ok(Completion),
    /// A fail-closed refusal (policy denied, malformed, or backend error).
    Denied {
        /// A short machine-readable reason code.
        reason: String,
        /// The audit entry recording the (denied) call.
        audit_seq: u64,
    },
}

/// A served completion and the serving evidence attached to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Generated text.
    pub text: String,
    /// The engine that served it (e.g. "local-reference", "gpu-docker/vllm").
    pub model_id: String,
    /// Where it ran — always in-enclosure; never a hosted dependency. [AC-MI-1]
    pub locality: Locality,
    /// Quantisation actually used.
    pub quant: Quant,
    /// First-token latency in milliseconds (target < 800ms). [NFR-PERF-1]
    pub first_token_ms: u32,
    /// Whether the prompt prefix hit the KV-cache.
    pub cache_hit: bool,
    /// The batch size this request was served in (continuous batching).
    pub batch_size: u32,
    /// Approximate prompt token count.
    pub prompt_tokens: u32,
    /// Generated token count.
    pub completion_tokens: u32,
    /// The audit entry recording the call. [AC-MI-8]
    pub audit_seq: u64,
}

/// A decode failure on the wire (fails closed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Malformed(pub String);

// ---- encoding primitives (mirrors crypto/pq-mtls/src/wire.rs) --------------

fn put_u32(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(o: &mut Vec<u8>, v: u64) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn put_bytes(o: &mut Vec<u8>, b: &[u8]) {
    put_u32(o, b.len() as u32);
    o.extend_from_slice(b);
}
fn put_str(o: &mut Vec<u8>, s: &str) {
    put_bytes(o, s.as_bytes());
}
fn put_opt_quant(o: &mut Vec<u8>, q: Option<Quant>) {
    match q {
        Some(q) => {
            o.push(1);
            o.push(q.as_u8());
        }
        None => o.push(0),
    }
}
fn put_opt_str(o: &mut Vec<u8>, s: &Option<String>) {
    match s {
        Some(s) => {
            o.push(1);
            put_str(o, s);
        }
        None => o.push(0),
    }
}

/// A bounds-checked cursor; every read fails closed past the end of the buffer.
struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], Malformed> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| Malformed("length overflow".into()))?;
        if end > self.b.len() {
            return Err(Malformed("unexpected end of input".into()));
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, Malformed> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, Malformed> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, Malformed> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_le_bytes(a))
    }
    fn bytes(&mut self) -> Result<Vec<u8>, Malformed> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn string(&mut self) -> Result<String, Malformed> {
        String::from_utf8(self.bytes()?).map_err(|_| Malformed("invalid utf-8".into()))
    }
    fn opt_quant(&mut self) -> Result<Option<Quant>, Malformed> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(
                Quant::from_u8(self.u8()?).ok_or_else(|| Malformed("unknown quant".into()))?,
            )),
            _ => Err(Malformed("bad option flag".into())),
        }
    }
    fn opt_string(&mut self) -> Result<Option<String>, Malformed> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.string()?)),
            _ => Err(Malformed("bad option flag".into())),
        }
    }
    fn finish(self) -> Result<(), Malformed> {
        if self.pos == self.b.len() {
            Ok(())
        } else {
            Err(Malformed("trailing bytes".into()))
        }
    }
}

// ---- request codec ---------------------------------------------------------

impl InferenceRequest {
    /// Encode to canonical bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        put_str(&mut o, &self.purpose);
        o.push(self.workload_class.as_u8());
        put_opt_quant(&mut o, self.quant_pref);
        put_opt_str(&mut o, &self.backend_override);
        put_u32(&mut o, self.max_tokens);
        put_str(&mut o, &self.prompt);
        // Trailing and optional: absent, the encoding is unchanged.
        if let Some(c) = &self.context {
            put_str(
                &mut o,
                &c.chars().take(MAX_CONTEXT_CHARS).collect::<String>(),
            );
        }
        o
    }
    /// Decode from bytes; fails closed on any defect.
    pub fn decode(b: &[u8]) -> Result<Self, Malformed> {
        let mut r = Reader::new(b);
        let purpose = r.string()?;
        let workload_class =
            WorkloadClass::from_u8(r.u8()?).ok_or_else(|| Malformed("unknown workload".into()))?;
        let quant_pref = r.opt_quant()?;
        let backend_override = r.opt_string()?;
        let max_tokens = r.u32()?;
        let prompt = r.string()?;
        let context = if r.pos < r.b.len() {
            Some(r.string()?.chars().take(MAX_CONTEXT_CHARS).collect())
        } else {
            None
        };
        r.finish()?;
        Ok(Self {
            purpose,
            workload_class,
            quant_pref,
            backend_override,
            max_tokens,
            prompt,
            context,
        })
    }
}

// ---- response codec --------------------------------------------------------

const RESP_OK: u8 = 1;
const RESP_DENIED: u8 = 2;

impl InferenceResponse {
    /// Encode to canonical bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Vec::new();
        match self {
            InferenceResponse::Ok(c) => {
                o.push(RESP_OK);
                put_str(&mut o, &c.text);
                put_str(&mut o, &c.model_id);
                o.push(c.locality.as_u8());
                o.push(c.quant.as_u8());
                put_u32(&mut o, c.first_token_ms);
                o.push(c.cache_hit as u8);
                put_u32(&mut o, c.batch_size);
                put_u32(&mut o, c.prompt_tokens);
                put_u32(&mut o, c.completion_tokens);
                put_u64(&mut o, c.audit_seq);
            }
            InferenceResponse::Denied { reason, audit_seq } => {
                o.push(RESP_DENIED);
                put_str(&mut o, reason);
                put_u64(&mut o, *audit_seq);
            }
        }
        o
    }
    /// Decode from bytes; fails closed on any defect.
    pub fn decode(b: &[u8]) -> Result<Self, Malformed> {
        let mut r = Reader::new(b);
        let resp = match r.u8()? {
            RESP_OK => {
                let text = r.string()?;
                let model_id = r.string()?;
                let locality =
                    Locality::from_u8(r.u8()?).ok_or_else(|| Malformed("bad locality".into()))?;
                let quant = Quant::from_u8(r.u8()?).ok_or_else(|| Malformed("bad quant".into()))?;
                let first_token_ms = r.u32()?;
                let cache_hit = match r.u8()? {
                    0 => false,
                    1 => true,
                    _ => return Err(Malformed("bad bool".into())),
                };
                let batch_size = r.u32()?;
                let prompt_tokens = r.u32()?;
                let completion_tokens = r.u32()?;
                let audit_seq = r.u64()?;
                InferenceResponse::Ok(Completion {
                    text,
                    model_id,
                    locality,
                    quant,
                    first_token_ms,
                    cache_hit,
                    batch_size,
                    prompt_tokens,
                    completion_tokens,
                    audit_seq,
                })
            }
            RESP_DENIED => {
                let reason = r.string()?;
                let audit_seq = r.u64()?;
                InferenceResponse::Denied { reason, audit_seq }
            }
            _ => return Err(Malformed("unknown response tag".into())),
        };
        r.finish()?;
        Ok(resp)
    }
}

#[cfg(test)]
mod context_tests {
    use super::*;

    fn req(context: Option<&str>) -> InferenceRequest {
        InferenceRequest {
            purpose: "investigation".into(),
            workload_class: WorkloadClass::Balanced,
            quant_pref: None,
            backend_override: None,
            max_tokens: 8,
            prompt: "ping".into(),
            context: context.map(str::to_string),
        }
    }

    #[test]
    fn a_label_round_trips_and_its_absence_leaves_the_encoding_unchanged() {
        let labelled = req(Some("heimdall · advice on alert A-1 · asked by analyst"));
        assert_eq!(
            InferenceRequest::decode(&labelled.encode()).unwrap(),
            labelled
        );
        let plain = req(None);
        let bytes = plain.encode();
        assert_eq!(InferenceRequest::decode(&bytes).unwrap(), plain);
        // Exactly the pre-label encoding: purpose, workload, quant, override, budget, prompt.
        assert!(bytes.ends_with(b"ping"));
        let long = req(Some(&"x".repeat(500)));
        let back = InferenceRequest::decode(&long.encode()).unwrap();
        assert_eq!(back.context.unwrap().chars().count(), MAX_CONTEXT_CHARS);
    }
}
