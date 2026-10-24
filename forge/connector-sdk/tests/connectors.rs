//! Connector SDK: typed sync → PQ-sealed raw dataset (AC-F-1), sandbox egress
//! denial (air-gap default), and KMS-resolved credentials. [FR-F-2] [NFR-PQ-2]
#![cfg(all(feature = "backend-fips", feature = "connector-file"))]

use forge_connector_sdk::connectors::file::DelimitedConnector;
use forge_connector_sdk::{
    sync_to_store, ApiConnector, Connector, ConnectorError, ConnectorKind, CredentialRef,
    CredentialResolver, Fetcher, KmsCredentialResolver, NoCredentials, SandboxPolicy, SyncContext,
};
use forge_dataset_store::{Column, ColumnType, DataValue, DatasetStore, Schema};
use kms::{Kms, SoftwareTrustRoot};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::Rng;

struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

fn schema() -> Schema {
    Schema::new(vec![
        Column::new("region", ColumnType::Text),
        Column::new("units", ColumnType::Int),
        Column::nullable("note", ColumnType::Text),
    ])
}

const CSV: &str = "region,units,note\nemea,120,\namer,340,q4 spike\n";

#[test]
fn typed_file_connector_syncs_into_a_sealed_raw_dataset() {
    // AC-F-1: a connector sync creates a raw dataset encrypted with a PQ-wrapped key.
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "forge-root", &mut SeedRng(1)).unwrap();
    let mut store = DatasetStore::new(Kms::new(provider, root));

    let conn = DelimitedConnector::csv("sales.csv", schema(), CSV);
    let sandbox = SandboxPolicy::air_gapped();
    let ctx = SyncContext::new(&NoCredentials, &sandbox);

    let version = sync_to_store(&conn, &ctx, &mut store, "raw", &mut SeedRng(2)).unwrap();

    // Ingest lineage anchored at the connector source.
    assert_eq!(version.lineage.source, "connector:file:sales.csv");
    assert_eq!(version.lineage.operation, "sync");
    assert_eq!(version.row_count, 2);

    // Sealed at rest (ML-KEM-1024), no plaintext in the ciphertext.
    let stored = store.get(&version.content_id).unwrap();
    assert_eq!(stored.sealed.suite, pqc_agility::KemSuiteId::MlKem1024);
    assert!(!stored
        .sealed
        .ciphertext
        .windows(8)
        .any(|w| w == b"q4 spike"));

    // Round-trips to the parsed, typed rows.
    let ds = store.checkout(&version.content_id).unwrap();
    assert_eq!(ds.rows()[0][0], DataValue::Text("emea".into()));
    assert_eq!(ds.rows()[0][1], DataValue::Int(120));
    assert_eq!(ds.rows()[0][2], DataValue::Null);
    assert_eq!(ds.rows()[1][2], DataValue::Text("q4 spike".into()));
}

#[test]
fn descriptor_reports_file_kind_and_non_streaming() {
    let conn = DelimitedConnector::csv("sales.csv", schema(), CSV);
    let d = conn.descriptor();
    assert_eq!(d.kind, ConnectorKind::File);
    assert!(!d.streaming);
    assert_eq!(d.lineage_source(), "connector:file:sales.csv");
}

#[test]
fn air_gapped_sandbox_denies_egress_by_default() {
    let sandbox = SandboxPolicy::air_gapped();
    assert!(sandbox.check_egress("data.example.com").is_err());

    // Explicit permit opens exactly one host.
    let permitted = SandboxPolicy::air_gapped().permit_host("feeds.internal");
    assert!(permitted.check_egress("feeds.internal").is_ok());
    assert!(permitted.check_egress("evil.example.com").is_err());
}

#[test]
fn kms_credential_resolver_unseals_by_reference() {
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "cred-root", &mut SeedRng(3)).unwrap();
    let kms = Kms::new(provider, root);

    // Seal a credential bound to its reference name as AAD.
    let cref = CredentialRef::new("pg://sales-ro");
    let sealed = kms
        .seal(cref.0.as_bytes(), b"super-secret-password", &mut SeedRng(4))
        .unwrap();

    let mut resolver = KmsCredentialResolver::new(&kms);
    resolver.insert_sealed(cref.clone(), sealed);

    let secret = resolver.resolve(&cref).unwrap();
    assert_eq!(secret.expose(), b"super-secret-password");
    // Debug is redacted — secrets never leak into logs.
    assert!(format!("{secret:?}").contains("redacted"));

    // An unknown reference fails closed.
    assert!(resolver.resolve(&CredentialRef::new("unknown")).is_err());
}

#[test]
fn malformed_source_fails_closed_at_ingest() {
    // A non-integer in the units column is rejected at sync, never committed.
    let bad = "region,units,note\nemea,not-a-number,\n";
    let conn = DelimitedConnector::csv("sales.csv", schema(), bad);
    let sandbox = SandboxPolicy::air_gapped();
    let ctx = SyncContext::new(&NoCredentials, &sandbox);
    assert!(conn.sync(&ctx).is_err());
}

#[test]
fn a_non_finite_number_is_no_value_in_a_nullable_column_and_refused_otherwise() {
    // A sensor reporting humidity as "nan": NaN has no JSON form, so it once reached
    // the fabric as null and the whole sync was refused. In a nullable column it is
    // no value; in a non-nullable one it is refused like any bad number.
    let schema = Schema::new(vec![
        Column::new("key", ColumnType::Text),
        Column::new("temperature_c", ColumnType::Float),
        Column::nullable("humidity_pct", ColumnType::Float),
    ]);
    let sandbox = SandboxPolicy::air_gapped();
    let ctx = SyncContext::new(&NoCredentials, &sandbox);
    let ok = "key,temperature_c,humidity_pct\nA,11.8,nan\nB,9.5,-inf\nC,7.0,Infinity\nD,6.5,81.2\n";
    let ds = DelimitedConnector::csv("air.csv", schema.clone(), ok)
        .sync(&ctx)
        .unwrap();
    assert_eq!(ds.rows()[0][2], DataValue::Null);
    assert_eq!(ds.rows()[1][2], DataValue::Null);
    assert_eq!(ds.rows()[2][2], DataValue::Null);
    assert_eq!(ds.rows()[3][2], DataValue::Float(81.2));
    for bad in ["NaN", "inf", "-infinity"] {
        let body = format!("key,temperature_c,humidity_pct\nA,{bad},50\n");
        assert!(
            DelimitedConnector::csv("air.csv", schema.clone(), body)
                .sync(&ctx)
                .is_err(),
            "{bad} in a non-nullable column must be refused"
        );
    }
}

/// A fetcher that returns fixed bytes — the network transfer never actually runs
/// in these tests; the sandbox gate decides whether the connector is allowed to.
struct FixtureFetcher(&'static str);
impl Fetcher for FixtureFetcher {
    fn get(&self, _url: &str) -> Result<String, ConnectorError> {
        Ok(self.0.to_string())
    }
}

#[test]
fn rc_55_ingest_funnel_enforces_egress_for_every_connector() {
    // RT-55 / RC-55: the sandbox is enforced at the single ingest choke point
    // (`sync_to_store`) for EVERY connector via its declared `egress_hosts`, not
    // only by connectors that remember to call `check_egress` themselves.
    let provider = fips_provider();
    let root = SoftwareTrustRoot::generate(&provider, "forge-root", &mut SeedRng(3)).unwrap();
    let mut store = DatasetStore::new(Kms::new(provider, root));

    // A network connector declares its egress host.
    let api = ApiConnector::new(
        "feeds.partner.example",
        "https://feeds.partner.example/sales.csv",
        schema(),
        ',',
        true,
        Box::new(FixtureFetcher("region,units,note\nemea,5,\n")),
    );
    assert_eq!(
        api.egress_hosts(),
        vec!["feeds.partner.example".to_string()]
    );

    // Under the air-gapped default, the funnel blocks it BEFORE any fetch.
    let air_gapped = SandboxPolicy::air_gapped();
    let ctx = SyncContext::new(&NoCredentials, &air_gapped);
    let denied = sync_to_store(&api, &ctx, &mut store, "raw", &mut SeedRng(4));
    assert!(
        matches!(denied, Err(ConnectorError::SandboxDenied { .. })),
        "the ingest funnel must enforce egress, got {denied:?}"
    );

    // A local connector declares NO egress; the funnel exercises the (vacuous)
    // sandbox check and lets it through.
    let file = DelimitedConnector::csv("sales.csv", schema(), "region,units,note\nemea,7,\n");
    assert!(
        file.egress_hosts().is_empty(),
        "a local connector needs no egress"
    );
    let ok = sync_to_store(&file, &ctx, &mut store, "raw", &mut SeedRng(5));
    assert!(
        ok.is_ok(),
        "a no-egress connector passes the funnel's sandbox check"
    );

    // When the deployment permits the host, the same network connector is allowed
    // through the funnel too.
    let permitted = SandboxPolicy::air_gapped().permit_host("feeds.partner.example");
    let pctx = SyncContext::new(&NoCredentials, &permitted);
    let allowed = sync_to_store(&api, &pctx, &mut store, "raw", &mut SeedRng(6));
    assert!(
        allowed.is_ok(),
        "a permitted host ingests through the funnel: {allowed:?}"
    );
}
