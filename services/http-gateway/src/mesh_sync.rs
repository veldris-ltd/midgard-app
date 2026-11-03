//! The fabric's mesh door: certified workloads — the Forge worker — sync objects and
//! links into the enclosure over PQ-mTLS. [FR-F-4] [NFR-SEC-1] [NFR-SEC-2] [AC-M-4]
//!
//! A sync carries import-format JSON Lines (see [`crate::import_file`]). It is:
//!
//! 1. authorised and audited as `SyncRecords` in the enclosure's one ledger, for the
//!    workload's certified roles, before the records are read: a workload policy does
//!    not permit learns nothing about the ontology from its reply;
//! 2. checked against the ontology, all or nothing, and every link end must be in the
//!    sync or already in the fabric; a malformed sync from a permitted workload is
//!    audited too, as `SYNC records: MALFORMED`;
//! 3. checked against the workload's certified clearance: when its certificate carries
//!    attributes (version 3), every record's marking must be one the clearance the
//!    channel authorised it at dominates, or the whole sync is refused
//!    (`marking-above-sync-clearance`). A certificate issued before attributes binds
//!    nothing here; [VLD-ANB-MIDGARD-0063]
//! 4. checked against what it would write, all or nothing: every record's marking must
//!    be within the gateway's sync clearance
//!    ([`SecuredGateway::with_sync_clearance`]); a record for an object already in the
//!    fabric may keep or raise its marking but not lower it, and may narrow its
//!    purposes but not widen them; and no record may touch an object under a live or
//!    redeemed human gate. A refusal is audited as `SYNC refused: <reason>` against the
//!    object it names;
//! 5. recorded — how many objects it creates and updates and links it adds — before
//!    anything is written;
//! 6. applied: objects are created or replaced, and a link already present is not
//!    added again.
//!
//! Unlike an offline import, a sync replaces objects: it is how a source's changes reach
//! the fabric, so syncing the same source twice leaves the fabric as it was. Lowering a
//! marking, or changing an object a named human approved a change to, is an operator
//! action, not a sync. [VLD-ANB-MIDGARD-0001] [VLD-ANB-MIDGARD-0073]
//!
//! Responses are one outcome byte ([`outcome`]) followed by a UTF-8 message.

use std::collections::BTreeSet;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;

use fabric::{DurableObjectStore, LedgerStore, ObjectStore, PolicyContext};
use gateway::{Authorised, Gateway, RequestHandler, Router};
use policy::{
    dominates, Clearance, DefaultPdp, PolicyDecisionPoint, Purpose, Resource, ResourceId,
};
use pq_mtls::{Authorization, WorkloadIdentity};
use pqc_agility::backend::fips::provider as fips_provider;
use service_runtime::marking::open_marking;
use service_runtime::{
    apply_mesh_limits, mesh_context, serve_mesh_with, MeshLimits, ServiceIdentity,
};

use crate::enclosure::Enclosure;
use crate::import_file::{parse_import, Import};
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

/// The resource a sync — and its channel — is authorised against: open, as every mesh
/// resource is unless configured; what a sync may write is held to the workload's
/// certified clearance record by record. [VLD-ANB-MIDGARD-0063]
fn sync_resource(id: &str, purpose: &str) -> Resource {
    Resource {
        id: ResourceId(id.into()),
        marking: open_marking(),
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
    /// which the channel authorised as `authorization`, declared for `purpose`, at
    /// `now`.
    pub fn sync_records(
        &self,
        peer: &WorkloadIdentity,
        authorization: &Authorization,
        purpose: &str,
        body: &[u8],
        now: u64,
    ) -> Vec<u8> {
        let mut enclosure = self.lock();
        // Authorise before the records are read, so a workload policy does not permit
        // learns nothing about the ontology from the reply. [VLD-ANB-MIDGARD-0073]
        let context: PolicyContext =
            mesh_context(peer, authorization, &Purpose(purpose.to_string()));
        let decision = match enclosure.governance_mut().authorize(
            &context,
            &sync_resource("fabric-sync", purpose),
            ACTION_SYNC_RECORDS,
            now,
        ) {
            Ok(decision) => decision,
            Err(_) => return reply(outcome::FAILED, "the sync could not be audited"),
        };
        if !decision.permitted {
            return reply(
                outcome::DENIED,
                "policy does not permit this workload to sync records",
            );
        }
        let parsed = match (std::str::from_utf8(body), self.ontology.as_ref()) {
            (Err(_), _) => Err("the records are not UTF-8".to_string()),
            (_, None) => {
                Err("this enclosure declares no ontology, so nothing can be synced".to_string())
            }
            (Ok(text), Some(ontology)) => parse_import(text, ontology),
        };
        let import = match parsed {
            Ok(import) => import,
            Err(e) => {
                // A malformed sync is audited too: the decision row says only that it
                // was permitted.
                let recorded = enclosure.governance_mut().record_event(
                    &peer.id.0,
                    "SYNC records: MALFORMED",
                    "sync",
                    now,
                );
                if recorded.is_err() {
                    return reply(outcome::FAILED, "the sync could not be audited");
                }
                return reply(outcome::MALFORMED, &e);
            }
        };
        if import.objects.is_empty() && import.links.is_empty() {
            return reply(outcome::MALFORMED, "the sync holds no objects or links");
        }
        // A workload whose certificate carries attributes syncs nothing above the
        // clearance it was certified at. [VLD-ANB-MIDGARD-0063]
        if authorization.purposes.is_some() {
            if let Some(above) = import
                .objects
                .iter()
                .find(|o| !dominates(&context.clearance, &o.marking))
            {
                let recorded = enclosure.governance_mut().record_event(
                    &peer.id.0,
                    "SYNC refused: marking-above-sync-clearance",
                    &format!("object:{}", above.id),
                    now,
                );
                let audited = if recorded.is_ok() {
                    ""
                } else {
                    " (not audited)"
                };
                return reply(
                    outcome::DENIED,
                    &format!(
                        "marking-above-sync-clearance: {} is marked above the clearance this \
                         workload is certified at; nothing was synced{audited}",
                        above.id
                    ),
                );
            }
        }

        // What the sync would write, checked against what it would overwrite before
        // anything is written: one refused record refuses the whole batch.
        // [VLD-ANB-MIDGARD-0001]
        if let Err((id, reason)) = self.check_sync_writes(&enclosure, &import, now) {
            let recorded = enclosure.governance_mut().record_event(
                &peer.id.0,
                &format!("SYNC refused: {reason}"),
                &format!("object:{id}"),
                now,
            );
            if recorded.is_err() {
                return reply(outcome::FAILED, "the refusal could not be audited");
            }
            return reply(
                outcome::DENIED,
                &format!("sync refused: {id} ({reason}); nothing was written"),
            );
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

    /// The first object in `import` this sync may not write, and why: its marking is
    /// above the sync clearance, it would lower the marking or widen the purposes of
    /// the object it replaces, or it names an object under a live or redeemed human
    /// gate. The reasons are fixed codes and the id is one the workload sent, so a
    /// refusal tells it nothing it did not already know. [VLD-ANB-MIDGARD-0001]
    fn check_sync_writes(
        &self,
        enclosure: &Enclosure<DurableObjectStore, L>,
        import: &Import,
        now: u64,
    ) -> Result<(), (String, &'static str)> {
        let store = enclosure.fabric().store();
        let gated = enclosure.gated_targets(now);
        for object in &import.objects {
            let refuse = |reason| Err((object.id.clone(), reason));
            if !dominates(&self.sync_clearance, &object.marking) {
                return refuse("marking-above-sync-clearance");
            }
            if let Some(existing) = store.get_object(&object.id) {
                // The new marking must dominate the old: level not lower, compartments
                // a superset.
                let raised = Clearance {
                    level: object.marking.level,
                    compartments: object.marking.compartments.clone(),
                };
                if !dominates(&raised, &existing.marking) {
                    return refuse("marking-downgrade");
                }
                if !object
                    .allowed_purposes
                    .is_subset(&existing.allowed_purposes)
                {
                    return refuse("purpose-widening");
                }
            }
            if gated.contains(&object.id) {
                return refuse("human-gated-target");
            }
        }
        Ok(())
    }
}

/// Serve [`SYNC_RECORDS`] for `gateway` over PQ-mTLS on `bind`, on a background thread,
/// presenting `identity`. `channel_policy` decides who may open the channel, which opens
/// for `purpose` only. The door serves within `limits` — a connection cap and timeouts,
/// and a per-identity rate limit and request quota — and counts its connections in the
/// gateway's metrics. Returns the bound address. [VLD-ANB-MIDGARD-0004]
pub fn spawn_mesh_listener<L: LedgerStore + Send + 'static>(
    gateway: Arc<SecuredGateway<DurableObjectStore, L>>,
    identity: &ServiceIdentity,
    channel_policy: DefaultPdp,
    bind: &str,
    purpose: &str,
    limits: MeshLimits,
) -> std::io::Result<SocketAddr> {
    let metrics = gateway.metrics_registry();
    let declared = purpose.to_string();
    let handler: Arc<dyn RequestHandler> = Arc::new(Authorised(
        move |peer: &WorkloadIdentity, auth: &Authorization, body: &[u8]| {
            gateway.sync_records(peer, auth, &declared, body, crate::now_secs())
        },
    ));
    let pdp: Arc<dyn PolicyDecisionPoint + Send + Sync> = Arc::new(channel_policy);
    let mesh = apply_mesh_limits(
        Gateway::new(
            Arc::new(fips_provider()),
            identity.cert.clone(),
            identity.signing_key.clone(),
            identity.anchor.clone(),
            pdp,
            sync_resource("fabric-sync-channel", purpose),
            Arc::new(Router::new().route(SYNC_RECORDS, handler)),
        ),
        &limits,
    );
    let listener = TcpListener::bind(bind)?;
    let addr = listener.local_addr()?;
    let service = identity.service.clone();
    std::thread::spawn(move || {
        serve_mesh_with(&service, listener, mesh, metrics, limits.listener);
    });
    Ok(addr)
}
