//! RC-27 — Given a real external source, when a pipeline with a join+aggregate runs,
//! then it ingests, executes on the real engine, and projects to the fabric — and an
//! egress-violating connector is blocked.
//!
//! End to end:
//! 1. A **real file connector** opens a CSV *from disk* (genuine I/O) → typed dataset.
//! 2. A **join + aggregate** pipeline runs on the **partitioned** engine (matched
//!    against the reference oracle).
//! 3. The curated result is **projected to the object fabric**.
//! 4. An **API connector is blocked** by the air-gapped sandbox (egress denied),
//!    while a deployment that permits the host succeeds. [FR-F-1..4] [NFR-PRV-1]

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

use fabric::store::{InMemoryStore, ObjectStore};
use fabric::value::Value;
use forge_connector_sdk::{
    ApiConnector, Connector, ConnectorError, Fetcher, FileConnector, NoCredentials, SandboxPolicy,
    SyncContext,
};
use forge_dataset_store::{Column, ColumnType, DataValue, Dataset, Schema};
use forge_engine::{
    AggFunc, AggregateSpec, ComputeBackend, ExecutionPlan, JoinType, PartitionedBackend,
    ReferenceBackend, SortKey, Transform,
};
use forge_projection::{project_objects, ObjectMapping, PropertyMapping};
use policy::{ClassificationLevel, Marking, Purpose};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn unique_path(name: &str) -> std::path::PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("forge-rc27-{n}-{name}"))
}

fn sales_schema() -> Schema {
    Schema::new(vec![
        Column::new("region", ColumnType::Text),
        Column::new("rep", ColumnType::Text),
        Column::new("amount", ColumnType::Int),
    ])
}

/// The region → manager dimension embedded in the join.
fn managers() -> Dataset {
    Dataset::new(
        Schema::new(vec![
            Column::new("region", ColumnType::Text),
            Column::new("manager", ColumnType::Text),
        ]),
        vec![
            vec![
                DataValue::Text("EMEA".into()),
                DataValue::Text("Mara".into()),
            ],
            vec![
                DataValue::Text("APAC".into()),
                DataValue::Text("Ola".into()),
            ],
            vec![
                DataValue::Text("NORAM".into()),
                DataValue::Text("Ken".into()),
            ],
        ],
    )
    .unwrap()
}

/// join(region→manager) → group by manager: total + deal count → sort by total desc.
fn pipeline() -> ExecutionPlan {
    ExecutionPlan::new(vec![
        Transform::Join {
            right: managers(),
            left_on: vec!["region".into()],
            right_on: vec!["region".into()],
            how: JoinType::Inner,
        },
        Transform::Aggregate {
            group_by: vec!["manager".into()],
            aggs: vec![
                AggregateSpec {
                    func: AggFunc::Sum("amount".into()),
                    output: "total".into(),
                },
                AggregateSpec {
                    func: AggFunc::Count,
                    output: "deals".into(),
                },
            ],
        },
        Transform::Sort(vec![SortKey::desc("total")]),
    ])
}

#[test]
fn rc_27_file_source_join_aggregate_projects_to_fabric() {
    // ---- 1. A REAL external source: write a CSV to disk, ingest via FileConnector.
    let path = unique_path("sales.csv");
    std::fs::write(
        &path,
        "region,rep,amount\n\
         EMEA,ana,100\n\
         EMEA,bo,300\n\
         APAC,cy,50\n\
         EMEA,ana,200\n\
         APAC,cy,70\n\
         NORAM,dee,400\n",
    )
    .unwrap();

    let connector = FileConnector::csv(&path, sales_schema());
    let sandbox = SandboxPolicy::air_gapped();
    let creds = NoCredentials;
    let ctx = SyncContext::new(&creds, &sandbox);
    // Genuine disk read → typed dataset (a local file needs no egress).
    let ingested = connector
        .sync(&ctx)
        .expect("file connector reads from disk");
    let _ = std::fs::remove_file(&path);
    assert_eq!(ingested.row_count(), 6);

    // ---- 2. join + aggregate on the REAL (partitioned) engine.
    let plan = pipeline();
    let curated = PartitionedBackend::new(4)
        .execute(&plan, &ingested)
        .unwrap();
    // It must equal the single-threaded reference oracle.
    let oracle = ReferenceBackend::new().execute(&plan, &ingested).unwrap();
    assert_eq!(curated.rows(), oracle.rows());

    // Totals: Mara 600 (3 deals), Ken 400 (1), Ola 120 (2); sorted by total desc.
    assert_eq!(curated.row_count(), 3);
    assert_eq!(curated.rows()[0][0], DataValue::Text("Mara".into()));
    assert_eq!(curated.rows()[0][1], DataValue::Int(600));
    assert_eq!(curated.rows()[0][2], DataValue::Int(3));
    assert_eq!(curated.rows()[1][0], DataValue::Text("Ken".into()));
    assert_eq!(curated.rows()[1][1], DataValue::Int(400));
    assert_eq!(curated.rows()[2][0], DataValue::Text("Ola".into()));
    assert_eq!(curated.rows()[2][1], DataValue::Int(120));

    // ---- 3. Project the curated dataset to the object fabric.
    let mut marking_compartments = BTreeSet::new();
    marking_compartments.insert(policy::Compartment("SALES".into()));
    let marking = Marking {
        level: ClassificationLevel::CONFIDENTIAL,
        compartments: marking_compartments,
    };
    let mut purposes = BTreeSet::new();
    purposes.insert(Purpose("analytics".into()));
    let mapping = ObjectMapping::new(
        "ManagerSales",
        "manager",
        vec![
            PropertyMapping::new("total", "total_sales"),
            PropertyMapping::new("deals", "deal_count"),
        ],
        marking.clone(),
        purposes,
    );
    let mut store = InMemoryStore::new();
    let report = project_objects(&curated, &mapping, &mut store).unwrap();
    assert_eq!(report.count, 3);

    let mara = store.get_object("Mara").expect("projected object exists");
    assert_eq!(mara.type_id, "ManagerSales");
    assert_eq!(mara.properties.get("total_sales"), Some(&Value::Int(600)));
    assert_eq!(mara.properties.get("deal_count"), Some(&Value::Int(3)));
    // Classification propagated onto the projected object.
    assert_eq!(mara.marking, marking);
}

struct FixtureFetcher(&'static str);
impl Fetcher for FixtureFetcher {
    fn get(&self, _url: &str) -> Result<String, ConnectorError> {
        Ok(self.0.to_string())
    }
}

#[test]
fn rc_27_egress_violating_connector_is_blocked() {
    let api = ApiConnector::new(
        "feeds.partner.example",
        "https://feeds.partner.example/sales.csv",
        sales_schema(),
        ',',
        true,
        Box::new(FixtureFetcher("region,rep,amount\nEMEA,ana,100\n")),
    );

    // Air-gapped default: egress denied → the connector is blocked BEFORE any fetch.
    let air_gapped = SandboxPolicy::air_gapped();
    let creds = NoCredentials;
    let denied = api.sync(&SyncContext::new(&creds, &air_gapped));
    assert!(
        matches!(&denied, Err(ConnectorError::SandboxDenied { host }) if host == "feeds.partner.example"),
        "an egress-violating connector must be blocked, got {denied:?}"
    );

    // When a deployment explicitly permits the host, the same connector succeeds.
    let permitted = SandboxPolicy::air_gapped().permit_host("feeds.partner.example");
    let ok = api
        .sync(&SyncContext::new(&creds, &permitted))
        .expect("permitted host ingests");
    assert_eq!(ok.row_count(), 1);
    assert_eq!(ok.rows()[0][0], DataValue::Text("EMEA".into()));
}
