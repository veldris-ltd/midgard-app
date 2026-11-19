//! The no-source mode: one built-in `local` persona at `open`, every listed
//! collection empty, every action refused with "no data source connected", nothing
//! queued, and the offline flag still toggles. [FR-C-5] [AC-C-13] [FR-C-4]

use attest::json::parse;
use attest::TrustAnchors;
use midgard_client_core::demo::enclosure::DemoEnclosure;
use midgard_client_core::local_source::{LocalSource, LOCAL_COLLECTIONS, LOCAL_PERSONA};
use midgard_client_core::marking::{clearance, MarkingLevel};
use midgard_client_core::source::{DataSource, SourceError};
use midgard_client_core::sync::Replica;

#[test]
fn there_is_one_persona_local_at_open() {
    let s = LocalSource::new();
    let personas = s.personas();
    assert_eq!(personas.len(), 1);
    let p = &personas[0];
    assert_eq!(p.id, LOCAL_PERSONA);
    assert_eq!(p.id, "local");
    assert_eq!(p.display, "Local user");
    assert_eq!(p.role, "operator");
    assert_eq!(p.clearance, MarkingLevel::Open);
    assert!(p.compartments.is_empty());
    assert!(p.roles.is_empty());

    let g = s.grant("local").unwrap();
    assert_eq!(g.persona(), p);
    assert_eq!(
        g.clearance(),
        &clearance(MarkingLevel::Open, Vec::<String>::new())
    );
}

#[test]
fn an_unknown_persona_is_refused() {
    let s = LocalSource::new();
    assert_eq!(
        s.grant("k.rao").unwrap_err(),
        SourceError::UnknownPersona("k.rao".into())
    );
    assert_eq!(
        s.grant("").unwrap_err(),
        SourceError::UnknownPersona(String::new())
    );
}

#[test]
fn a_grant_from_another_source_is_not_honoured() {
    let body = r#"{"collections":{},
     "personas":[{"id":"k.rao","display":"K. Rao","role":"analyst","clearance":"confidential","compartments":["OPS-N"],"roles":["analyst"]}],
     "format":"midgard-demo-dataset","version":1,"release":"test","inputs":{}}"#;
    let demo =
        DemoEnclosure::from_body("test", &parse(body).unwrap(), &TrustAnchors::new()).unwrap();
    let foreign = demo.grant("k.rao").unwrap();
    let mut s = LocalSource::new();
    // The persona is checked before the collection.
    assert_eq!(
        s.fetch(&foreign, "any").unwrap_err(),
        SourceError::UnknownPersona("k.rao".into())
    );
    for (name, _) in s.collections() {
        assert_eq!(
            s.fetch(&foreign, &name).unwrap_err(),
            SourceError::UnknownPersona("k.rao".into())
        );
    }
    assert_eq!(
        s.exchange(&foreign, &[]).unwrap_err(),
        SourceError::UnknownPersona("k.rao".into())
    );
}

#[test]
fn every_listed_collection_is_empty_and_an_unlisted_one_is_unknown() {
    let s = LocalSource::new();
    let g = s.grant("local").unwrap();
    let listed = s.collections();
    let table: Vec<(String, String)> = LOCAL_COLLECTIONS
        .iter()
        .map(|(n, t)| (n.to_string(), t.to_string()))
        .collect();
    assert_eq!(listed, table, "the compile-time table, as listed");
    let mut sorted = listed.clone();
    sorted.sort();
    assert_eq!(listed, sorted, "in name order");
    for (name, _) in &listed {
        assert_eq!(s.fetch(&g, name).unwrap(), vec![], "{name}");
    }
    assert_eq!(
        s.fetch(&g, "midgard.audit").unwrap_err(),
        SourceError::UnknownCollection("midgard.audit".into())
    );
}

#[test]
fn an_exchange_with_ops_is_refused_and_one_without_is_empty() {
    let mut s = LocalSource::new();
    let g = s.grant("local").unwrap();
    assert_eq!(s.exchange(&g, &[]).unwrap(), Vec::<Vec<u8>>::new());

    let mut r = Replica::new("device");
    let op = r.set("action:device-1", "verb", b"demo.ping".to_vec());
    let err = s.exchange(&g, &[op.encode()]).unwrap_err();
    assert_eq!(err, SourceError::NoSource);
    assert_eq!(err.to_string(), "no data source connected");
    // Refused before decoding: even a blob that is not an op gets the same answer.
    assert_eq!(
        s.exchange(&g, &[b"junk".to_vec()]).unwrap_err(),
        SourceError::NoSource
    );
    // Nothing was kept: an empty exchange still answers nothing.
    assert_eq!(s.exchange(&g, &[]).unwrap(), Vec::<Vec<u8>>::new());
}

#[test]
fn offline_toggles_and_refuses_like_any_source() {
    let mut s = LocalSource::new();
    let g = s.grant("local").unwrap();
    assert!(s.online(), "starts online");
    s.set_online(false);
    assert!(!s.online());
    assert_eq!(s.exchange(&g, &[]).unwrap_err(), SourceError::Offline);
    assert_eq!(s.fetch(&g, "any").unwrap_err(), SourceError::Offline);
    s.set_online(true);
    assert!(s.online());
    assert_eq!(s.exchange(&g, &[]).unwrap(), Vec::<Vec<u8>>::new());
}
