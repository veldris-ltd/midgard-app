//! The PostgreSQL and S3 connectors against **real** servers — PostgreSQL 16 with
//! SCRAM-only host authentication, and MinIO — which the in-process fakes cannot
//! stand in for. Skipped unless `MIDGARD_LIVE_CONNECTORS=1`;
//! `deploy/acceptance/live-connectors.sh` starts and seeds the servers and runs this.
//! [FR-F-2] [NFR-SEC-1]
#![cfg(all(
    feature = "backend-fips",
    feature = "connector-db",
    feature = "connector-objstore"
))]

use forge_connector_sdk::connectors::postgres::PostgresConnector;
use forge_connector_sdk::connectors::s3::S3Connector;
use forge_connector_sdk::{
    Connector, ConnectorError, CredentialRef, CredentialResolver, SandboxPolicy, Secret,
    SyncContext,
};
use forge_dataset_store::{Column, ColumnType, DataValue, Schema};

/// The server to reach from `var`, when live checks are enabled.
fn target(var: &str) -> Option<(String, u16)> {
    if std::env::var("MIDGARD_LIVE_CONNECTORS").as_deref() != Ok("1") {
        eprintln!("{var}: skipped (set MIDGARD_LIVE_CONNECTORS=1)");
        return None;
    }
    let value = std::env::var(var).unwrap_or_else(|_| panic!("{var} is not set"));
    let (host, port) = value.rsplit_once(':').expect("host:port");
    Some((host.to_string(), port.parse().expect("a port")))
}

struct Secrets(&'static str);

impl CredentialResolver for Secrets {
    fn resolve(&self, _: &CredentialRef) -> Result<Secret, ConnectorError> {
        Ok(Secret::new(self.0.as_bytes().to_vec()))
    }
}

#[test]
fn live_postgres() {
    let Some((host, port)) = target("MIDGARD_LIVE_PG") else {
        return;
    };
    let sandbox = SandboxPolicy::air_gapped().permit_host(format!("{host}:{port}"));
    let schema = Schema::new(vec![
        Column::new("id", ColumnType::Int),
        Column::new("name", ColumnType::Text),
        Column::new("active", ColumnType::Bool),
        Column::nullable("score", ColumnType::Float),
    ]);
    let connector = |query: &str| {
        PostgresConnector::new(
            host.clone(),
            port,
            "fleet",
            "reader",
            CredentialRef::new("pg://reader"),
            query,
            schema.clone(),
        )
        .batch_rows(2)
    };

    let full = connector("SELECT id, name, active, score FROM aircraft ORDER BY id")
        .sync(&SyncContext::new(&Secrets("live-reader-pass"), &sandbox))
        .expect("a real PostgreSQL 16 sync");
    assert_eq!(full.rows().len(), 3);
    assert_eq!(
        full.rows()[0],
        [
            DataValue::Int(1),
            DataValue::Text("alpha, north".into()),
            DataValue::Bool(true),
            DataValue::Float(1.5)
        ]
    );
    assert_eq!(full.rows()[1][3], DataValue::Null);

    let incremental =
        connector("SELECT id, name, active, score FROM aircraft WHERE id > $1 ORDER BY id")
            .incremental("id", "0")
            .sync_incremental(
                &SyncContext::new(&Secrets("live-reader-pass"), &sandbox).with_checkpoint("1"),
            )
            .expect("an incremental sync");
    assert_eq!(incremental.dataset.rows().len(), 2);
    assert_eq!(incremental.checkpoint.as_deref(), Some("3"));

    let refused = connector("SELECT id, name, active, score FROM aircraft")
        .sync(&SyncContext::new(&Secrets("not the password"), &sandbox))
        .expect_err("a wrong password is refused")
        .to_string();
    assert!(refused.contains("28P01"), "{refused}");

    let written = connector(
        "INSERT INTO aircraft VALUES (9, 'x', true, 1) RETURNING id, name, active, score",
    )
    .sync(&SyncContext::new(&Secrets("live-reader-pass"), &sandbox))
    .expect_err("the read-only transaction refuses a write")
    .to_string();
    assert!(written.contains("25006"), "{written}");
    println!("LIVE postgres: PASS");
}

#[test]
fn live_s3() {
    let Some((host, port)) = target("MIDGARD_LIVE_S3") else {
        return;
    };
    let sandbox = SandboxPolicy::air_gapped().permit_host(format!("{host}:{port}"));
    let schema = Schema::new(vec![
        Column::new("tail", ColumnType::Text),
        Column::nullable("hours", ColumnType::Int),
        Column::new("note", ColumnType::Text),
    ]);
    let connector = S3Connector::new(
        host.clone(),
        port,
        "us-east-1",
        "fleet-exports",
        "aircraft/",
        "forge-reader",
        CredentialRef::new("s3://forge-reader"),
        schema,
    )
    .page_size(1);

    let first = connector
        .sync_incremental(&SyncContext::new(&Secrets("live-minio-secret"), &sandbox))
        .expect("a real MinIO sync");
    assert_eq!(first.dataset.rows().len(), 3);
    assert_eq!(
        first.dataset.rows()[0][2],
        DataValue::Text("based north, active".into())
    );
    let checkpoint = first.checkpoint.expect("a checkpoint");

    let again = connector
        .sync_incremental(
            &SyncContext::new(&Secrets("live-minio-secret"), &sandbox).with_checkpoint(&checkpoint),
        )
        .expect("an incremental sync");
    assert!(again.dataset.rows().is_empty(), "nothing changed");

    let refused = connector
        .sync(&SyncContext::new(&Secrets("not the secret"), &sandbox))
        .expect_err("a wrong secret is refused")
        .to_string();
    assert!(refused.contains("SignatureDoesNotMatch"), "{refused}");
    println!("LIVE s3: PASS");
}

#[test]
fn live_s3_through_the_egress_broker() {
    let Some((broker, port)) = target("MIDGARD_LIVE_BROKER") else {
        return;
    };
    let (_, impostor_port) = target("MIDGARD_LIVE_BROKER_IMPOSTOR").expect("the impostor route");
    let (tls_host, tls_port) = target("MIDGARD_LIVE_S3_TLS").expect("the TLS server");
    let schema = Schema::new(vec![
        Column::new("tail", ColumnType::Text),
        Column::nullable("hours", ColumnType::Int),
        Column::new("note", ColumnType::Text),
    ]);
    let connector = |host: &str, port: u16| {
        S3Connector::new(
            host,
            port,
            "us-east-1",
            "fleet-exports",
            "aircraft/",
            "forge-reader",
            CredentialRef::new("s3://forge-reader"),
            schema.clone(),
        )
    };
    let sync = |host: &str, port: u16| {
        let sandbox = SandboxPolicy::air_gapped().permit_host(format!("{host}:{port}"));
        connector(host, port).sync(&SyncContext::new(&Secrets("live-minio-secret"), &sandbox))
    };

    let through = sync(&broker, port).expect("a sync through the broker's TLS 1.3 route");
    assert_eq!(through.rows().len(), 3);

    let impostor = sync(&broker, impostor_port)
        .expect_err("a route pinned to another CA reaches nothing")
        .to_string();
    assert!(!impostor.is_empty());

    let plaintext = sync(&tls_host, tls_port)
        .expect_err("plaintext straight to a TLS server fails")
        .to_string();
    assert!(!plaintext.is_empty());
    println!("LIVE broker: PASS ({impostor} | {plaintext})");
}
