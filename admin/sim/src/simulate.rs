//! The dry-run itself: evaluate every probe under the current and proposed policy
//! with the real PDP, and record which decisions flip.

use policy::{ClassificationLevel, DefaultPdp, PolicyDecisionPoint, PolicySet};

use crate::probe::AccessProbe;
use crate::report::SimulationReport;

/// The number of new grants at or above which a change is flagged high blast
/// radius even when no marked data is involved.
pub const HIGH_BLAST_GRANTS: usize = 10;

/// Simulate the effect of replacing `current` with `proposed`, over `probes`.
///
/// This is the real deny-by-default PDP evaluated twice, so the reported grants
/// and revokes are exactly what a commit of `proposed` would produce — the
/// sim-accuracy guarantee. `[FR-AD-6]` `[AC-AD-5]`
pub fn simulate(
    current: &PolicySet,
    proposed: &PolicySet,
    probes: &[AccessProbe],
) -> SimulationReport {
    let before = DefaultPdp::new(current.clone());
    let after = DefaultPdp::new(proposed.clone());

    let mut report = SimulationReport::default();
    for probe in probes {
        let req = probe.as_request();
        let was = before.decide(&req).is_permit();
        let now = after.decide(&req).is_permit();
        match (was, now) {
            (false, true) => {
                report.grants.push(probe.label.clone());
                // A grant that exposes classified data is the highest blast radius.
                if probe.resource.marking.level > ClassificationLevel::UNCLASSIFIED {
                    report.marked_grants.push(probe.label.clone());
                }
            }
            (true, false) => report.revokes.push(probe.label.clone()),
            (true, true) => report.unchanged_permit.push(probe.label.clone()),
            (false, false) => report.unchanged_deny.push(probe.label.clone()),
        }
    }
    report
}
