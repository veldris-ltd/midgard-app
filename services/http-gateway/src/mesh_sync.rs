//! The fabric's mesh door: certified workloads — the Forge worker — sync objects and
//! links into the enclosure over PQ-mTLS. [FR-F-4] [NFR-SEC-1] [NFR-SEC-2] [AC-M-4]
//!
//! A sync carries import-format JSON Lines (see [`crate::import_file`]). It is:
//!
//! 1. authorised and audited as `SyncRecords` in the enclosure's one ledger, for the
//!    workload's certified roles — a malformed sync is audited too;
//! 2. checked against the ontology, all or nothing, and every link end must be in the
//!    sync or already in the fabric;
//! 3. recorded — how many objects it creates and updates and links it adds — before
//!    anything is written;
//! 4. applied: objects are created or replaced, and a link already present is not
//!    added again.
//!
//! Unlike an offline import, a sync replaces objects: it is how a source's changes reach
//! the fabric, so syncing the same source twice leaves the fabric as it was.
//!
//! Responses are one outcome byte ([`outcome`]) followed by a UTF-8 message.

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;

use fabric::{DurableObjectStore, LedgerStore, ObjectStore, PolicyContext};
use gateway::{Gateway, RequestHandler, Router};
use policy::{
    ClassificationLevel, Clearance, DefaultPdp, Marking, PolicyDecisionPoint, Purpose, Resource,
    ResourceId,
};
use pq_mtls::WorkloadIdentity;
use pqc_agility::backend::fips::provider as fips_provider;
use service_runtime::{serve_mesh, ServiceIdentity};
use telemetry::MetricsRegistry;

use crate::import_file::parse_import;
use crate::SecuredGateway;

/// The method a workload syncs records with; its payload is import-format JSON Lines.
pub const SYNC_RECORDS: &str = "SyncRecords";
/// The policy action a sync is authorised as.
pub const ACTION_SYNC_RECORDS: &str = "SyncRecords";

/// The first byte of every sync response.
pub mod outcome {
    /// Applied.
    pub const ACCEPTED: u8 = 0;
    /// Refused by policy.
    pub const DENIED: u8 = 1;
    /// The records do not fit the ontology, or name a link end that does not exist.
    pub const MALFORMED: u8 = 2;
    /// It could not be audited or written.
    pub const FAILED: u8 = 3;
}

/// The resource a sync — and its channel — is authorised against.
fn sync_resource(id: &str, purpose: &str) -> Resource {
    Resource {
        id: ResourceId(id.into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose(purpose.to_string())].into_iter().collect(),
    }
}

fn reply(code: u8, message: &str) -> Vec<u8> {
    let mut out = vec![code];
    out.extend_from_slice(message.as_bytes());
    out
}

impl<L: LedgerStore> SecuredGateway<DurableObjectStore, L> {
    /// Sync `body`, import-format JSON Lines, from the authenticated workload `peer`,
    /// declared for `purpose`, at `now`.
    pub fn sync_records(
        &self,
        peer: &WorkloadIdentity,
        purpose: &str,
        body: &[u8],
        now: u64,
    ) -> Vec<u8> {
        let mut enclosure = self
            .enclosure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let parsed = match (std::str::from_utf8(body), self.ontology.as_ref()) {
            (Err(_), _) => Err("the records are not UTF-8".to_string()),
            (_, None) => {
                Err("this enclosure declares no ontology, so nothing can be synced".to_string())
            }
            (Ok(text), Some(ontology)) => parse_import(text, ontology),
        };
        let action = if parsed.is_ok() {
            ACTION_SYNC_RECORDS
        } else {
            "SyncRecords:MALFORMED"
        };
        let context = PolicyContext {
            subject: peer.id.0.clone(),
            roles: peer.roles.clone(),
            clearance: Clearance {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            purpose: Purpose(purpose.to_string()),
        };
        let decision = match enclosure.governance_mut().authorize(
            &context,
            &sync_resource("fabric-sync", purpose),
            action,
            now,
        ) {
            Ok(decision) => decision,
            Err(_) => return reply(outcome::FAILED, "the sync could not be audited"),
        };
        let import = match parsed {
            Ok(import) => import,
            Err(e) => return reply(outcome::MALFORMED, &e),
        };
        if !decision.permitted {
            return reply(
                outcome::DENIED,
                "policy does not permit this workload to sync records",
            );
        }
        if import.objects.is_empty() && import.links.is_empty() {
            return reply(outcome::MALFORMED, "the sync holds no objects or links");
        }

        // What the sync will do, decided before anything is written.
        let (created, updated, links) = {
            let store = enclosure.fabric().store();
            let synced: BTreeSet<&str> = import.objects.iter().map(|o| o.id.as_str()).collect();
            for link in &import.links {
                for end in [&link.source_id, &link.target_id] {
                    if !synced.contains(end.as_str()) && store.get_object(end).is_none() {
                        return reply(
                            outcome::MALFORMED,
                            &format!(
                                "link {} from {} to {} names {end}, which is neither in this \
                                 sync nor in the enclosure",
                                link.link_type, link.source_id, link.target_id
                            ),
                        );
                    }
                }
            }
            let created = import
                .objects
                .iter()
                .filter(|o| store.get_object(&o.id).is_none())
                .count();
            let links: Vec<_> = import
                .links
                .iter()
                .filter(|l| {
                    !store
                        .neighbors(&l.source_id, &l.link_type)
                        .iter()
                        .any(|o| o.id == l.target_id)
                })
                .cloned()
                .collect();
            (created, import.objects.len() - created, links)
        };

        // Audit first: a sync the ledger cannot record does not happen.
        let recorded = enclosure.governance_mut().record_event(
            &peer.id.0,
            "SYNC records",
            &format!(
                "sync:{created}-created:{updated}-updated:{}-links",
                links.len()
            ),
            now,
        );
        if recorded.is_err() {
            return reply(
                outcome::FAILED,
                "the sync could not be audited; nothing was written",
            );
        }
        let added = links.len();
        let store = enclosure.fabric_mut().store_mut();
        for object in import.objects {
            if let Err(e) = store.put_object_durable(object) {
                return reply(
                    outcome::FAILED,
                    &format!("the sync stopped part-way; the ledger records it: {e}"),
                );
            }
        }
        for link in links {
            if let Err(e) = store.put_link_durable(link) {
                return reply(
                    outcome::FAILED,
                    &format!("the sync stopped part-way; the ledger records it: {e}"),
                );
            }
        }
        reply(
            outcome::ACCEPTED,
            &format!(
                "synced: {created} created, {updated} updated, {added} link(s) added, at audit \
                 sequence {}",
                decision.audit_seq
            ),
        )
    }
}

/// Serve [`SYNC_RECORDS`] for `gateway` over PQ-mTLS on `bind`, on a background thread,
/// presenting `identity`. `channel_policy` decides who may open the channel, which opens
/// for `purpose` only. Returns the bound address.
pub fn spawn_mesh_listener<L: LedgerStore + Send + 'static>(
    gateway: Arc<SecuredGateway<DurableObjectStore, L>>,
    identity: &ServiceIdentity,
    channel_policy: DefaultPdp,
    bind: &str,
    purpose: &str,
) -> std::io::Result<SocketAddr> {
    let declared = purpose.to_string();
    let handler: Arc<dyn RequestHandler> = Arc::new(move |peer: &WorkloadIdentity, body: &[u8]| {
        gateway.sync_records(peer, &declared, body, crate::now_secs())
    });
    let pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(channel_policy);
    let mesh = Gateway::new(
        Arc::new(fips_provider()),
        identity.cert.clone(),
        identity.signing_key.clone(),
        identity.anchor.clone(),
        pdp,
        sync_resource("fabric-sync-channel", purpose),
        Arc::new(Router::new().route(SYNC_RECORDS, handler)),
    );
    let listener = TcpListener::bind(bind)?;
    let addr = listener.local_addr()?;
    let service = identity.service.clone();
    std::thread::spawn(move || {
        serve_mesh(&service, listener, mesh, Arc::new(MetricsRegistry::new()));
    });
    Ok(addr)
}
