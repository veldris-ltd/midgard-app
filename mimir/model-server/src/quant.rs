//! Quantisation schemes the serving plane understands. A backend declares which
//! schemes it supports; a request may prefer one, honoured only if supported.
//! `[FR-MI-1]`

/// A weight quantisation scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quant {
    /// Full precision (no quantisation).
    None,
    /// 8-bit integer.
    Int8,
    /// 4-bit integer.
    Int4,
    /// Activation-aware weight quantisation.
    Awq,
    /// GGUF-packed (llama.cpp-class) weights.
    Gguf,
}

impl Quant {
    /// Wire tag.
    pub fn as_u8(self) -> u8 {
        match self {
            Quant::None => 0,
            Quant::Int8 => 1,
            Quant::Int4 => 2,
            Quant::Awq => 3,
            Quant::Gguf => 4,
        }
    }
    /// Decode a wire tag; `None` if unknown (fail closed).
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Quant::None,
            1 => Quant::Int8,
            2 => Quant::Int4,
            3 => Quant::Awq,
            4 => Quant::Gguf,
            _ => return None,
        })
    }

    /// The vLLM `--quantization` argument for this scheme, if it maps to one.
    /// `None` (full precision) and GGUF are not passed as `--quantization`.
    pub fn vllm_arg(self) -> Option<&'static str> {
        match self {
            Quant::None => None,
            Quant::Int8 => Some("int8"),
            Quant::Int4 => Some("int4"),
            Quant::Awq => Some("awq"),
            Quant::Gguf => None,
        }
    }

    /// A relative per-token latency multiplier — lower precision is cheaper to
    /// serve. Used by the reference engine's latency model.
    pub fn latency_scale(self) -> f32 {
        match self {
            Quant::None => 1.0,
            Quant::Int8 => 0.7,
            Quant::Int4 => 0.5,
            Quant::Awq => 0.55,
            Quant::Gguf => 0.6,
        }
    }
}
