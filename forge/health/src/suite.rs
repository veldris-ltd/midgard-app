//! A health suite: an ordered set of expectations evaluated together into a report.
//! The report is the promotion decision — if any expectation fails, the build is
//! not promotable. [FR-F-6]

use forge_dataset_store::Dataset;

use crate::expectation::{Expectation, ExpectationOutcome};

/// A collection of expectations checked against a build's output.
#[derive(Debug, Clone, Default)]
pub struct HealthSuite {
    /// The expectations to evaluate, in order.
    pub expectations: Vec<Expectation>,
}

impl HealthSuite {
    /// A suite from a list of expectations.
    pub fn new(expectations: Vec<Expectation>) -> Self {
        Self { expectations }
    }

    /// Evaluate every expectation against `dataset` and collect the report.
    pub fn check(&self, dataset: &Dataset) -> HealthReport {
        let results: Vec<ExpectationOutcome> = self
            .expectations
            .iter()
            .map(|e| e.evaluate(dataset))
            .collect();
        let passed = results.iter().all(|r| r.passed);
        HealthReport { results, passed }
    }
}

/// The outcome of checking a suite: every expectation's result and whether the
/// dataset is promotable (all passed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// Per-expectation outcomes, in suite order.
    pub results: Vec<ExpectationOutcome>,
    /// True iff every expectation passed (promotion is allowed).
    pub passed: bool,
}

impl HealthReport {
    /// The failing outcomes (empty when healthy).
    pub fn failures(&self) -> Vec<&ExpectationOutcome> {
        self.results.iter().filter(|r| !r.passed).collect()
    }

    /// A one-line human summary, e.g. `2/3 expectations passed`.
    pub fn summary(&self) -> String {
        let passed = self.results.iter().filter(|r| r.passed).count();
        format!("{}/{} expectations passed", passed, self.results.len())
    }
}
