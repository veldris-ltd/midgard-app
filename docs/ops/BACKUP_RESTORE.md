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
| Audit ledger | `crypto/audit-writer` (`FileLedger`) | the PQ-signed audit chain |
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

## Tested

The restore procedure is exercised end-to-end by `admin/config/tests/dr_drill.rs`
against the real `DurableConfigStore`:

- commits state → snapshots the sealed journal → simulates total node/disk loss →
  restores from the snapshot on a fresh node → **replay + chain re-verify** →
  asserts all pre-snapshot commits recovered (RPO) within the RTO, and that a
  **tampered backup is rejected on restore**.

Since every store shares the `SealedJournal` primitive, the drill result generalises:
the same copy-and-`open()` procedure restores any of the stores above.
