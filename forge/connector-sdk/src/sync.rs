//! The ingest funnel: run a connector, then seal-and-commit its dataset into the
//! store as a raw, content-addressed, PQ-encrypted version with ingest lineage.
//! This is the AC-F-1 path — a synced source becomes an encrypted raw dataset.
//! [FR-F-2] [NFR-PQ-2] [AC-F-1]

use forge_dataset_store::{DatasetStore, DatasetVersion, Lineage};
use kms::TrustRoot;
use pqc_agility::Rng;

use crate::connector::{Connector, SyncContext};
use crate::error::ConnectorError;

/// Sync a connector's source and commit the result to `store` on `branch` as a
/// sealed raw dataset. Returns the committed version. The lineage records the
/// connector's source label so provenance starts at ingest. [AC-F-1]
pub fn sync_to_store<R: TrustRoot>(
    connector: &dyn Connector,
    ctx: &SyncContext,
    store: &mut DatasetStore<R>,
    branch: &str,
    rng: &mut dyn Rng,
) -> Result<DatasetVersion, ConnectorError> {
    // Enforce the sandbox for EVERY connector at the single ingest choke point:
    // each declared egress host must be permitted before the connector runs. A
    // local connector declares none, so this is a no-op that still proves the
    // sandbox was exercised; a network connector is denied here under an
    // air-gapped policy. [RT-55] [NFR-PRV-1]
    for host in connector.egress_hosts() {
        ctx.sandbox.check_egress(&host)?;
    }
    let dataset = connector.sync(ctx)?;
    let lineage = Lineage::ingest(connector.descriptor().lineage_source());
    let version = store.commit(&dataset, branch, lineage, rng)?;
    Ok(version)
}
