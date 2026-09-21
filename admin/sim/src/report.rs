//! The result of a dry-run: which probes flip, and a plain-language impact summary
//! leading with the highest-blast-radius effect.

/// The effect of a proposed policy change over the evaluated probes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SimulationReport {
    /// Probes that flip deny → permit (newly granted).
    pub grants: Vec<String>,
    /// Probes that flip permit → deny (newly revoked).
    pub revokes: Vec<String>,
    /// Grants that expose *marked* (classified) resources — the highest blast
    /// radius, surfaced explicitly (mirrors SAD_07 A.AD2's "grants N users read on
    /// MARKED data").
    pub marked_grants: Vec<String>,
    /// Probes permitted before and after (unchanged).
    pub unchanged_permit: Vec<String>,
    /// Probes denied before and after (unchanged).
    pub unchanged_deny: Vec<String>,
}

impl SimulationReport {
    /// Whether the change has no effect on any probe.
    pub fn is_no_op(&self) -> bool {
        self.grants.is_empty() && self.revokes.is_empty()
    }

    /// Whether the change is high blast radius: it exposes marked data, or grants
    /// at or above the [`crate::simulate::HIGH_BLAST_GRANTS`] threshold.
    pub fn high_blast_radius(&self) -> bool {
        !self.marked_grants.is_empty() || self.grants.len() >= crate::simulate::HIGH_BLAST_GRANTS
    }

    /// A one-line, human-readable impact summary, leading with the highest-impact
    /// effect (marked-data exposure first).
    pub fn summary(&self) -> String {
        if self.is_no_op() {
            return "no effect: the change grants or revokes nothing".to_string();
        }
        let mut lead = String::new();
        if !self.marked_grants.is_empty() {
            lead = format!(
                "grants {} access to MARKED data; ",
                self.marked_grants.len()
            );
        }
        format!(
            "{lead}{} new grants, {} revokes (blast radius: {})",
            self.grants.len(),
            self.revokes.len(),
            if self.high_blast_radius() {
                "HIGH"
            } else {
                "normal"
            }
        )
    }
}
