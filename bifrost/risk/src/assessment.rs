//! The parsed A.7 risk assessment. It is **advisory**: the deterministic gate
//! (see [`crate::gate`]) decides go/no-go using this score plus hard rules. The
//! assessment never itself deploys or blocks anything. [FR-B-6] [NFR-AI-1]

use crate::error::RiskError;
use crate::json::{self, JsonValue};

/// A risk band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskBand {
    /// Low risk.
    Low,
    /// Elevated risk.
    Elevated,
    /// High risk.
    High,
    /// Critical risk.
    Critical,
}

impl RiskBand {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "low" => Some(RiskBand::Low),
            "elevated" => Some(RiskBand::Elevated),
            "high" => Some(RiskBand::High),
            "critical" => Some(RiskBand::Critical),
            _ => None,
        }
    }
}

/// One evidence-backed risk factor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskFactor {
    /// The factor name.
    pub factor: String,
    /// The supporting evidence drawn from the release context.
    pub evidence: String,
}

/// The advisory risk assessment produced by A.7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskAssessment {
    /// 0–100 risk score.
    pub risk_score: u8,
    /// The risk band.
    pub band: RiskBand,
    /// The top evidence-backed factors.
    pub top_factors: Vec<RiskFactor>,
    /// The recommended ring strategy.
    pub recommended_ring_strategy: String,
    /// Evidence-backed critical concerns that should block (e.g. a critical CVE).
    pub blocking_concerns: Vec<String>,
}

impl RiskAssessment {
    /// Parse an A.7 JSON response.
    pub fn parse(text: &str) -> Result<Self, RiskError> {
        let root = json::parse(text).map_err(RiskError::Parse)?;

        let risk_score = root
            .get("risk_score")
            .and_then(|v| v.as_i64())
            .filter(|n| (0..=100).contains(n))
            .ok_or_else(|| RiskError::Parse("risk_score must be an integer 0-100".into()))?
            as u8;

        let band = root
            .get("band")
            .and_then(|v| v.as_str())
            .and_then(RiskBand::from_str)
            .ok_or_else(|| RiskError::Parse("band must be low|elevated|high|critical".into()))?;

        let recommended_ring_strategy = root
            .get("recommended_ring_strategy")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let top_factors = match root.get("top_factors").and_then(|v| v.as_arr()) {
            None => Vec::new(),
            Some(items) => items.iter().map(parse_factor).collect::<Result<_, _>>()?,
        };

        let blocking_concerns = string_array(root.get("blocking_concerns"));

        Ok(Self {
            risk_score,
            band,
            top_factors,
            recommended_ring_strategy,
            blocking_concerns,
        })
    }
}

fn parse_factor(v: &JsonValue) -> Result<RiskFactor, RiskError> {
    let factor = v
        .get("factor")
        .and_then(|x| x.as_str())
        .ok_or_else(|| RiskError::Parse("top_factors[].factor missing".into()))?
        .to_string();
    let evidence = v
        .get("evidence")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(RiskFactor { factor, evidence })
}

fn string_array(v: Option<&JsonValue>) -> Vec<String> {
    match v.and_then(|x| x.as_arr()) {
        None => Vec::new(),
        Some(items) => items
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect(),
    }
}
