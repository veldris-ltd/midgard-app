//! The diff between two config snapshots.

use crate::state::ConfigState;

/// The difference from one snapshot to another. `[FR-AD-7]`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigDiff {
    /// Keys present only in the target (key, new value).
    pub added: Vec<(String, String)>,
    /// Keys present only in the source (key, old value).
    pub removed: Vec<(String, String)>,
    /// Keys in both with a changed value (key, old, new).
    pub changed: Vec<(String, String, String)>,
}

impl ConfigDiff {
    /// Compute the diff from `from` to `to` (deterministic; keys sorted).
    pub fn between(from: &ConfigState, to: &ConfigState) -> Self {
        let mut diff = ConfigDiff::default();
        for (k, old) in from.entries() {
            match to.get(k) {
                None => diff.removed.push((k.clone(), old.clone())),
                Some(new) if new != old => diff.changed.push((k.clone(), old.clone(), new.clone())),
                Some(_) => {}
            }
        }
        for (k, new) in to.entries() {
            if from.get(k).is_none() {
                diff.added.push((k.clone(), new.clone()));
            }
        }
        diff
    }

    /// Whether the two snapshots are identical.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}
