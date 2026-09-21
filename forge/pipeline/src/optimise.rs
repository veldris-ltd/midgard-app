//! Pipeline optimisation — **predicate pushdown**. Filters are moved as early as
//! possible so downstream structural work (renames, derives, casts, projections)
//! runs over fewer rows. A filter is pushed ahead of a preceding op only when doing
//! so is *result-preserving*: it never crosses a `Limit` (row-count-sensitive) or
//! another filter (stable order), and never crosses an op that produces, renames,
//! or retypes a column the filter references. The optimiser therefore changes
//! *cost*, never *output*. [FR-F-1]

use forge_engine::{Predicate, Transform};

use crate::pipeline::Pipeline;

/// Return an optimised copy of `pipeline` with filters pushed down.
pub fn optimise(pipeline: &Pipeline) -> Pipeline {
    let mut stages = pipeline.stages.clone();
    // Stable bubble: each swap strictly lowers a filter's index, so this terminates.
    let mut changed = true;
    while changed {
        changed = false;
        for i in 1..stages.len() {
            if stages[i].is_filter() {
                let cols = filter_columns(&stages[i]);
                if !blocks_pushdown(&stages[i - 1], &cols) {
                    stages.swap(i - 1, i);
                    changed = true;
                }
            }
        }
    }
    Pipeline {
        id: pipeline.id.clone(),
        source_branch: pipeline.source_branch.clone(),
        stages,
    }
}

/// The columns a filter stage references (empty for a non-filter).
fn filter_columns(t: &Transform) -> Vec<String> {
    let mut out = Vec::new();
    if let Transform::Filter(p) = t {
        collect_predicate_columns(p, &mut out);
    }
    out
}

fn collect_predicate_columns(p: &Predicate, out: &mut Vec<String>) {
    match p {
        Predicate::Compare { column, .. } => out.push(column.clone()),
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            collect_predicate_columns(a, out);
            collect_predicate_columns(b, out);
        }
        Predicate::Not(x) => collect_predicate_columns(x, out),
    }
}

/// Whether a filter referencing `filter_cols` may NOT be pushed ahead of `op`.
fn blocks_pushdown(op: &Transform, filter_cols: &[String]) -> bool {
    match op {
        // Adding a column the filter reads: the filter needs it, cannot precede it.
        Transform::Derive { name, .. } => filter_cols.iter().any(|c| c == name),
        // Renaming a column the filter reads (old or new name): ambiguous, block.
        Transform::Rename { from, to } => filter_cols.iter().any(|c| c == from || c == to),
        // Retyping a column the filter reads: the literal's type would differ, block.
        Transform::Cast { column, .. } => filter_cols.iter().any(|c| c == column),
        // A projection only drops columns; the filter's columns exist before it.
        Transform::Select(_) => false,
        // Never reorder across a row-count-sensitive op or another filter.
        Transform::Limit(_) | Transform::Filter(_) => true,
        // Relational barriers: a filter must not migrate across a sort/aggregate/
        // join/window — they change the row set, cardinality, or column set the
        // predicate would otherwise see. Conservative and always correct.
        Transform::Sort(_)
        | Transform::Aggregate { .. }
        | Transform::Join { .. }
        | Transform::Window { .. } => true,
    }
}
