//! Console markings on the clearance lattice: a row above the session clearance is
//! withheld, never shown. [AC-C-13] [NFR-SEC-2]

use midgard_client_core::marking::{clearance, filter_rows, MarkingLevel, RowMarking, Visible};

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
fn a_row_above_clearance_is_withheld_without_its_id_or_content() {
    let restricted = clearance(MarkingLevel::Restricted, &[]);
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
fn a_missing_compartment_withholds_even_at_the_right_level() {
    let ops_only = clearance(MarkingLevel::Confidential, &["OPS-N".to_string()]);
    let out = filter_rows(
        &ops_only,
        vec![(
            "s".to_string(),
            row(MarkingLevel::Confidential, &["SIG"]),
            1,
        )],
    );
    assert!(matches!(out[0], Visible::Withheld { .. }));
}
