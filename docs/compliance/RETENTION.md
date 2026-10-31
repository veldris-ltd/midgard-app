# Retention schedule: durable stores and personal data

What each durable store keeps, what personal data it may hold, how long, and how that
data will be removed. [VLD-ANB-MIDGARD-0070] [NFR-PRV-1]

**Read this first.** Every durable store is an append-only sealed journal
(`audit_writer::SealedJournal`, see [BACKUP_RESTORE.md](../ops/BACKUP_RESTORE.md)).
Nothing in Midgard today deletes, rewrites, compacts or exports a record from one: a
record written stays in the journal, and in every backup of it, for the life of the
store. Replacing a record (a re-enrolment, a password change, an edited object)
appends a new record and leaves the old one in the file; `replace_password_hash`
(MG-SEC-11) is an example, and the directory keeps every earlier password hash.
Subject erasure and subject export are therefore **not implemented**, and the claims
ledger records them as specified only (`personal-data-erasure` in
[claims.toml](claims.toml)). The retention periods below are the targets the
follow-on work package implements; until then the effective retention of every store
is the life of the deployment.

## The schedule

The stores are those of [BACKUP_RESTORE.md](../ops/BACKUP_RESTORE.md), "The stores".

| Store | Personal data it may hold | Retention target | Planned removal mechanism |
|-------|---------------------------|------------------|---------------------------|
| Audit ledger (`FileLedger`, `governance.ledger` and each service's ledger) | Principal identifiers, the action and resource of every decision, timestamps | The operator's audit retention period for accountability records; never shorter than the investigation and legal-hold periods that apply | None: the ledger is immutable by design, so that it is evidence. Entries should carry identifiers, not content; erasure of a subject from the ledger is not offered, and the operator records that as a documented exemption |
| Object fabric (`objects.store`) | Whatever the ontology models: people, contacts, locations, free text | Per object type, set by the operator's records-of-processing; deleted when the purpose ends | Per-record data keys with crypto-shred tombstones: each object sealed under its own key, erasure destroys the key and appends a tombstone, and compaction drops shredded records |
| Forge dataset store | Source rows synced from external systems, with lineage | Per dataset, the source system's retention; superseded versions after the operator's version-retention period | As the object fabric: per-record (or per-version) data keys and tombstones; lineage keeps the tombstone, not the data |
| Bifrost registry / fleet | Installation identifiers and operator names on admissions | Life of the installation plus the operator's audit retention | Compaction of decommissioned installations; operator names follow the directory |
| Heimdall store | Observed entities and tracks, which can be people or their vehicles | The operational picture's retention period, set by the operator | Per-entity data keys with tombstones; time-based compaction of closed tracks |
| Admin stores (`admin/config`, `admin/identity`, `admin/privilege`, `admin/breakglass`) | Administrator identities, public passkeys, role bindings, approvals, break-glass grants | Life of the administrator's account plus the audit retention for approvals and grants | Directory-style tombstones for passkeys and roles; approvals and grants are audit records and follow the ledger |
| IAM directory (`identity.directory`) | Principal identifiers, password hashes, MFA enrolments, clearances | Life of the account; earlier password hashes not at all once replaced | Per-subject data keys with tombstones: deleting a principal destroys its key; a password change re-seals under a new key so the old hash is shredded |

Outside the journals:

| Material | Personal data | Retention |
|----------|---------------|-----------|
| Keystores and keystore passphrases | None (keys) | Life of the key; destroyed on rotation per [PKI.md](../ops/PKI.md) |
| Backups and snapshots | A copy of every store above | The snapshot policy of [BACKUP_RESTORE.md](../ops/BACKUP_RESTORE.md). Until crypto-shred exists, an erased record survives in every backup taken before the erasure; with it, destroying the data key erases the record from the backups too |
| Kubernetes seeds (`<service>-seed`) | None: keys, trust anchors, configuration and the empty governance ledger only (deploy/kind/bootstrap-secrets.sh) | Until the first start; delete the Secret after the pods have started |
| Logs and metrics | Principal identifiers in structured logs | The in-enclosure collector's retention, set by the operator |

## The operator's obligations as controller

Midgard is the processor-side tooling; the operator deploying it is the controller and
must, until erasure and export ship:

- Record in the records of processing that the stores are append-only, and that
  erasure requests are answered by restricting processing (disabling the principal,
  restricting the objects by policy) rather than by deletion.
- Set the retention periods in the schedule for the deployment and note that they are
  not yet enforced by the software.
- Minimise what is written: model personal data in the ontology only where the purpose
  needs it, and keep content out of audit records.
- Keep backups encrypted (they are, under the keystores) and their passphrases apart,
  and expire backups on the stated policy.
- On Kubernetes, keep keystores, passphrases and seeds out of etcd or encrypt etcd at
  rest ([deploy/helm/midgard/README.md](../../deploy/helm/midgard/README.md)).
- Carry out the DPIA for the deployment with these limits stated.

## What the follow-on work package delivers

Per-record data keys with crypto-shred tombstones for the directory, object and
dataset journals (three store formats), compaction, a subject erasure and a subject
export operation through `mg-admin`, and retention enforcement. It depends on the
wrapped keystore roots of keystore format version 2. The audit ledger stays immutable.
