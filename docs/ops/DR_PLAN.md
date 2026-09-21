# Disaster Recovery Plan

**Scope:** recovery of a Midgard enclosure after node loss, disk loss, site loss, or
data corruption. **Audience:** on-call SRE + the duty security officer.

Midgard is sovereign and in-enclosure: DR runs **entirely inside the customer
enclosure** with no cloud dependency and no data egress ([NFR-PRV-1]). Every durable
store is a **KMS-sealed, append-only journal** (RT-09..14) — encrypted at rest,
`fsync`ed per record, and **re-verified on boot** — which makes backup a consistent
file snapshot and restore a copy-and-replay.

## Recovery objectives

| Tier | RPO (max data loss) | RTO (time to service) | Mechanism |
|------|---------------------|-----------------------|-----------|
| **Continuous** (default) | ≤ **60 s** | ≤ **30 min** | journal shipping: each `fsync`ed record streams to a warm replica |
| **Snapshot** (fallback) | ≤ **5 min** | ≤ **30 min** | periodic full snapshot of every sealed journal to the backup vault |

These are the **stated objectives** RC-34 is measured against. The automated restore
drill (`admin/config/tests/dr_drill.rs`) asserts a real restore recovers all committed
state up to the last snapshot (RPO) and completes well inside the RTO.

## Prerequisites (must exist before an incident)

1. **Backups** taken per [BACKUP_RESTORE.md](BACKUP_RESTORE.md) — verified, not just written.
2. **Trust-root recovery material**: the KMS root is a hardware TrustRoot (RT-28). The
   HSM/TPM (or its escrow/quorum-recovery material) **must be available at the recovery
   site** — without it the sealed journals cannot be unsealed. Treat this as the #1 DR
   dependency; test its recovery on the same cadence as the data drill.
3. **IdP issuer key + trust anchor** (RT-29) available so the restored node can
   re-issue tokens and re-establish PQ-mTLS mesh trust.

## Restore runbook (simulated node loss → service)

This is the exact sequence the tested drill follows. Time it; it is the RTO clock.

1. **Declare** the incident (Sev-1/2 per [INCIDENT_RESPONSE.md](INCIDENT_RESPONSE.md)),
   page the duty security officer (DR touches key custody).
2. **Isolate** the lost node — remove it from the mesh trust anchor so a partially-dead
   node cannot serve stale state.
3. **Provision** a replacement node from the signed air-gap bundle (RT-19); verify the
   image signature before boot.
4. **Recover the trust root**: bring up the HSM/TPM (or restore escrowed root material)
   and confirm root **attestation** verifies (RT-28) before touching data.
5. **Retrieve backups**: copy each store's latest sealed-journal snapshot from the
   backup vault to the replacement node's data directory (see BACKUP_RESTORE.md for the
   store→path map).
6. **Boot the stores**: each store's `open()` **replays** its journal and **re-verifies**
   its signed chain. A tampered/corrupt backup **fails closed here** — do not override;
   fall back to the previous snapshot.
7. **Verify recovery**: confirm each store's head/version matches the backup checkpoint,
   and that `/api/v1/ready` (RT-33) reports every dependency healthy.
8. **Rejoin the mesh**: re-issue the node's workload cert from the IdP, add it back to
   the trust anchor, and let the gateway/Heimdall re-establish PQ-mTLS sessions
   (resumption tickets are invalidated by the new keys — full handshakes re-form).
9. **Resume traffic**: flip readiness to ready; watch the availability SLO recover in
   `/metrics`. **Stop the RTO clock.**
10. **Post-incident**: file the RPO actually achieved (last-snapshot vs incident time),
    the RTO actually achieved, and any deviation from this runbook. Feed gaps back into
    the next drill.

## Data-loss accounting (RPO)

Everything committed **up to the last snapshot/replicated record is recovered intact**
— the drill proves this byte-for-byte. Writes after that point and before node loss are
the acknowledged loss window; by construction it is **≤ RPO**. The drill deliberately
commits one record *after* the snapshot and asserts it is the (bounded) loss.

## Drill cadence

- **Automated** (every CI run): `admin/config/tests/dr_drill.rs` — node-loss → verified
  restore + tampered-backup rejection, on the real SealedJournal store.
- **Game-day** (quarterly): a full manual walk of the runbook above on a staging
  enclosure, including hardware trust-root recovery, timed against the RTO.

## Failure modes the drill guards

- **Silent corruption** — a tampered backup is rejected on `open()` (AEAD tag), never
  trusted. Fall back to the prior snapshot.
- **Wrong/absent trust root** — sealed journals will not open; the runbook front-loads
  trust-root recovery (step 4) precisely so this is caught before the RTO clock burns.
