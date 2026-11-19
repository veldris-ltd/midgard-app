//! Console markings on the clearance lattice: a row above the session clearance is
//! withheld, never shown. [AC-C-13] [NFR-SEC-2]

use midgard_client_core::marking::{clearance, filter_rows, MarkingLevel, RowMarking, Visible};
use policy::ClassificationLevel;

/// No compartments, typed for `clearance`.
const NONE: [&str; 0] = [];

fn row(level: MarkingLevel, comps: &[&str]) -> RowMarking {
    RowMarking::new(level, comps.iter().copied())
}

#[test]
fn levels_follow_the_console_order() {
    use MarkingLevel::*;
    assert!(Open < Internal && Internal < Restricted && Restricted < Confidential);
    for l in MarkingLevel::ALL {
        assert_eq!(MarkingLevel::parse(l.as_str()), Some(l));
    }
    assert_eq!(MarkingLevel::parse("secret"), None);
    assert_eq!(MarkingLevel::parse("Confidential"), None);
    assert_eq!(MarkingLevel::parse(""), None);
}

#[test]
fn levels_map_onto_the_policy_lattice() {
    assert_eq!(
        MarkingLevel::Open.classification(),
        ClassificationLevel::UNCLASSIFIED
    );
    assert_eq!(
        MarkingLevel::Internal.classification(),
        ClassificationLevel::CONFIDENTIAL
    );
    assert_eq!(
        MarkingLevel::Restricted.classification(),
        ClassificationLevel::SECRET
    );
    assert_eq!(
        MarkingLevel::Confidential.classification(),
        ClassificationLevel::TOP_SECRET
    );
}

#[test]
fn labels_read_like_the_consoles() {
    assert_eq!(
        row(MarkingLevel::Confidential, &["SIG", "OPS-N"]).label(),
        "CONFIDENTIAL // OPS-N, SIG"
    );
    assert_eq!(row(MarkingLevel::Internal, &[]).label(), "INTERNAL");
}

#[test]
fn compartments_are_sorted_and_unique_through_the_constructor() {
    let m = row(MarkingLevel::Restricted, &["SIG", "OPS-N", "SIG"]);
    assert_eq!(m.level(), MarkingLevel::Restricted);
    assert_eq!(m.compartments(), ["OPS-N".to_string(), "SIG".to_string()]);
}

#[test]
fn a_row_above_clearance_is_withheld_without_its_id_or_content() {
    let restricted = clearance(MarkingLevel::Restricted, NONE);
    let rows = vec![
        ("a".to_string(), row(MarkingLevel::Internal, &[]), "visible"),
        (
            "b".to_string(),
            row(MarkingLevel::Confidential, &[]),
            "secret body",
        ),
    ];
    let out = filter_rows(&restricted, rows);
    assert_eq!(
        out[0],
        Visible::Shown {
            id: "a".into(),
            marking: row(MarkingLevel::Internal, &[]),
            item: "visible"
        }
    );
    assert_eq!(
        out[1],
        Visible::Withheld {
            level: MarkingLevel::Confidential
        }
    );
}

#[test]
fn a_row_exactly_at_the_clearance_level_is_shown() {
    let restricted = clearance(MarkingLevel::Restricted, NONE);
    let out = filter_rows(
        &restricted,
        vec![("r".to_string(), row(MarkingLevel::Restricted, &[]), "body")],
    );
    assert_eq!(out[0].shown(), Some(&"body"));
}

#[test]
fn an_open_clearance_withholds_an_internal_row() {
    let open = clearance(MarkingLevel::Open, NONE);
    let out = filter_rows(
        &open,
        vec![("i".to_string(), row(MarkingLevel::Internal, &[]), "body")],
    );
    assert_eq!(
        out[0],
        Visible::Withheld {
            level: MarkingLevel::Internal
        }
    );
}

#[test]
fn a_missing_compartment_withholds_even_at_the_right_level() {
    let ops_only = clearance(MarkingLevel::Confidential, ["OPS-N"]);
    let out = filter_rows(
        &ops_only,
        vec![(
            "s".to_string(),
            row(MarkingLevel::Confidential, &["SIG"]),
            1,
        )],
    );
    assert_eq!(
        out[0],
        Visible::Withheld {
            level: MarkingLevel::Confidential
        }
    );
}

#[test]
fn a_clearance_holding_the_compartment_sees_the_row() {
    let both = clearance(MarkingLevel::Confidential, ["OPS-N", "SIG"]);
    let out = filter_rows(
        &both,
        vec![(
            "s".to_string(),
            row(MarkingLevel::Confidential, &["SIG"]),
            1,
        )],
    );
    assert_eq!(out[0].shown(), Some(&1));
}

#[test]
fn map_and_shown_keep_a_withheld_row_withheld() {
    let withheld: Visible<&str> = Visible::Withheld {
        level: MarkingLevel::Restricted,
    };
    assert_eq!(withheld.shown(), None);
    assert_eq!(
        withheld.map(str::len),
        Visible::Withheld {
            level: MarkingLevel::Restricted
        }
    );

    let shown = Visible::Shown {
        id: "a".to_string(),
        marking: row(MarkingLevel::Open, &[]),
        item: "four",
    };
    assert_eq!(
        shown.map(str::len),
        Visible::Shown {
            id: "a".to_string(),
            marking: row(MarkingLevel::Open, &[]),
            item: 4
        }
    );
}
