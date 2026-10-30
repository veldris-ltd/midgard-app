# Backup & Restore — durable stores

Every durable store in Midgard is the **same primitive**: a KMS-sealed, append-only
journal (`audit_writer::SealedJournal`) — encrypted at rest under the KMS envelope
(AES-256-GCM under an ML-KEM-1024-wrapped key, sealed to the trust root), `fsync`ed per
record, `0600`, with the record sequence bound into the AAD so a reordered or spliced
record fails authentication. This uniformity is what makes backup and restore a single,
tested procedure for all of them.

## The stores (RT-09..14)

| Store | Crate | What it holds |
|-------|-------|---------------|
| Audit ledger | `crypto/audit-writer` (`FileLedger`) | the PQ-signed audit chain, and beside it `<ledger>.checkpoint`, the signed record of its length and last entry |
| Object fabric | `services/fabric` | fabric objects/links |
| Forge dataset store | `forge/dataset-store` | content-addressed datasets + lineage |
| Bifrost registry / fleet | `bifrost/artefact`, `bifrost/fleet` | artefacts, fleet state |
| Heimdall store | `heimdall/store` | picture + workflow state |
| Admin stores | `admin/config`, `admin/identity`, `admin/privilege`, `admin/breakglass` | config history, directory, dual-control state, break-glass grants |
| IAM directory | `services/iam` (`DurableDirectory`) | principals + credentials/MFA enrolments |

Each store persists to one (or more) journal file(s) under its data directory. The
files are opaque ciphertext at rest — a backup vault never sees plaintext.

## Backup (snapshot) procedure — uniform

Because the journals are **append-only and `fsync`ed per record**, a plain file copy is
a **consistent snapshot** (no quiesce, no lock needed — a copy captures a prefix of the
log, which is always valid and re-verifiable).

1. **Continuous (preferred):** ship each journal's newly-`fsync`ed bytes to a warm
   replica inside the enclosure (append-only tail follow). RPO ≤ 60 s.
2. **Snapshot (fallback):** on the snapshot cadence (≤ 5 min), for each store:
   - `fsync` the store's data directory,
   - copy every `*.journal` file to the backup vault, tagged with a monotonic
     snapshot id + timestamp,
   - for an audit ledger, copy the ledger **and then** its `.checkpoint` at once. A
     ledger without its checkpoint does not open, and one an append moved past (or
     short of) its copied checkpoint fails the verify step below as truncated or
     stale: re-take it. [MG-SEC-05]
   - **verify the copy**: open it read-only through the store's `open()` (replay +
     chain re-verify) on the backup host; a copy that does not re-verify is discarded
     and re-taken. **A backup is not "done" until it has been verified.**
3. Retain snapshots per policy (e.g. 7 daily + 4 weekly); older snapshots are pruned.

**Do not** back up decrypted state, and **do not** copy the trust-root secret into the
data backup — the root lives in the HSM/TPM and is recovered separately (see
[DR_PLAN.md](DR_PLAN.md) step 4). The backup is useless to an attacker without it, which
is the point.

## Restore procedure — uniform

1. Recover the **trust root** first (HSM/TPM / escrow) and confirm its attestation.
2. Copy the chosen snapshot's journal file(s) into the replacement node's data
   directory for that store.
3. **Boot the store** via its `open()` — this **replays** the journal into memory and
   **re-verifies** the signed chain. Fail-closed: a corrupt/tampered snapshot is
   rejected here; fall back to the previous snapshot.
4. Confirm the recovered head/version matches the snapshot checkpoint.

Restoring an older snapshot of an audit ledger rolls it back, and a checkpoint kept on
the same disk cannot tell a restore from an attack: record each restore as an incident.
With a checkpoint mirror (below) the gateway makes that comparison itself.

## The audit ledger's checkpoint mirror [MG-SEC-05]

Whoever can write the state directory can roll the governance ledger **and** its
`.checkpoint` back together to an earlier consistent pair, or delete both and put an
old backup in their place; on its own disk the pair still agrees. The checkpoint
mirror keeps a second copy of every checkpoint somewhere they cannot reach.

**Placing it.** Give the gateway and `mg-admin init` the same
`--checkpoint-mirror DIR`:

- `DIR` must be on **another mount** than the state directory: an NFS export, an
  object-store FUSE mount, a separate volume. A mirror inside the state directory is
  refused, and one on the same disk rolls back with it. Mount it with credentials
  the state directory's storage administrators and backups do not hold; where the
  target supports it, turn on versioning or object lock, so that even the gateway's
  own credentials cannot replace a newer checkpoint with an older one.
- `DIR` must already exist. A mount point that is not mounted is not a mirror: if it
  is an empty local directory, a ledger with entries refuses to boot (see below)
  rather than mirror to the wrong place.
- One mirror directory per enclosure. The governance ledger's checkpoint is
  `DIR/midgard-governance.checkpoint`, the same signed record as the local one
  (`LedgerCheckpoint::encode`), replaced atomically. `mg-admin init` refuses a
  mirror that already holds one.

**What it does.** Every append writes the entry, then the local checkpoint, then the
mirror's. The mirror only moves forward: a lower entry count, or another head at the
same count, is refused. At boot the gateway refuses to serve when:

| What it finds | Error | Means |
|---|---|---|
| the local checkpoint counts fewer entries than the mirror's | `the ledger was rolled back` | the ledger and its checkpoint were rolled back, or an older backup restored |
| the ledger does not hold the mirrored head at the mirrored position | `... is not the head its checkpoint mirror names` | the ledger was replaced by another |
| no ledger and no checkpoint | `the ledger ... is missing` | both were deleted |
| the ledger holds entries and the mirror holds no checkpoint for it | `its checkpoint mirror has no checkpoint for it` | the mirror was lost, is not mounted, or is new for this ledger |

A mirror **behind** the ledger is not an error: a crash between the local and the
mirror write leaves it one checkpoint behind, and `mg-admin` commands that write the
ledger (`user add`, `import`, `admin add`, `mesh issue`, `mesh renew`) do not write
the mirror. The gateway's next checkpoint brings it up to date.

**When the mirror cannot be written** the append fails, fail-closed: the entry is
already durable and checkpointed locally, but the request it audits is refused, and
the ledger takes **no further entry** until the mirror takes the checkpoint again (each
later append retries it first). Audited operations stop while the mirror is
unreachable; restore the mount and they resume, with nothing lost.

**Enabling it on an existing enclosure, or re-seeding a lost mirror.** Stop the
gateway and run

```bash
mg-admin audit checkpoint --state-dir DIR --audit-pin FILE --checkpoint-mirror MIRROR
```

It verifies the whole ledger and its checkpoint under the pinned audit key and writes
the checkpoint to the mirror (adopting a ledger written before checkpoints at the same
time). It refuses a mirror that is ahead of the ledger or names another head: it never
moves a mirror back. Because it accepts a mirror that holds nothing, run it only after
establishing why the mirror is empty; it is the one step that vouches for the ledger
as it now stands.

**Restore with a mirror.** Back up the mirror directory with the rest, but **never
restore it over a newer one**: it is the record of how far the ledger got.

1. Restore the ledger and its checkpoint as above, from a snapshot **at least as
   recent as the mirror's checkpoint**. The gateway boots against the mirror as it
   stands; a restored ledger that is behind it is refused with `rolled back`.
2. If only an older snapshot exists (the newer entries are lost), the loss of audit
   entries is an incident, not a restore: record it, then, with the incident's
   approval, move the mirror's checkpoint aside (keep it as evidence of how far the
   ledger got) and re-seed from the restored ledger with `mg-admin audit checkpoint
   --checkpoint-mirror`. Nothing does this automatically.
3. If the mirror itself was lost and the ledger is intact, re-seed it as above, or
   restore the mirror's own backup: an older mirror is behind the ledger, which is
   accepted and caught up by the next checkpoint.

## Tested

The restore procedure is exercised end-to-end by `admin/config/tests/dr_drill.rs`
against the real `DurableConfigStore`:

- commits state → snapshots the sealed journal → simulates total node/disk loss →
  restores from the snapshot on a fresh node → **replay + chain re-verify** →
  asserts all pre-snapshot commits recovered (RPO) within the RTO, and that a
  **tampered backup is rejected on restore**.

Since every store shares the `SealedJournal` primitive, the drill result generalises:
the same copy-and-`open()` procedure restores any of the stores above.
