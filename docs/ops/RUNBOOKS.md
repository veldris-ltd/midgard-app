# Operational Runbooks

Concrete, in-enclosure procedures for the recurring operational tasks. Each is safe to
follow under pressure; each ends with a **verification** step. Signals come from the
local telemetry stack (RT-33).

Index:
- [RB-1: Diagnose a service that is not ready](#rb-1-service-not-ready)
- [RB-2: Availability / first-token SLO breach](#rb-2-slo-breach)
- [RB-3: Node/disk loss → restore](#rb-3-node-loss)
- [RB-4: Rotate the KMS trust root](#rb-4-rotate-trust-root)
- [RB-5: Break-glass emergency access](#rb-5-break-glass)
- [RB-6: Apply a high-risk config change](#rb-6-high-risk-config)
- [RB-7: Verify the audit chain](#rb-7-verify-audit)

Release engineering (not an in-enclosure operational task):
- [Execute the GA gate — readiness §4 A–C](RUNBOOK_ga_gate.md)

---

## RB-1: Service not ready
**Trigger:** `GET /api/v1/ready` returns 503, or an orchestrator marks a service down.
1. Read the readiness JSON — each dependency reports `healthy` + `detail`; find the
   unhealthy one(s).
2. Check that dependency: KMS trust root reachable? sealed store opened cleanly on boot
   (no fail-closed on a tampered record)? upstream PQ-mTLS peer authenticated?
3. Remediate the specific dependency (restart it, recover its store per RB-3, restore
   its trust root per RB-4).
4. **Verify:** `/api/v1/ready` returns 200 with every dependency `healthy`; availability
   SLO in `/metrics` stops breaching.

## RB-2: SLO breach
**Trigger:** `slo_breaching{slo="..."} 1` in `/metrics`.
1. Identify which SLO — `gateway_availability` (5xx ratio) or a latency SLO (e.g. Mimir
   `first_token`).
2. Availability: correlate `http_requests_total{status=~"5.."}` and logs by `trace_id`
   to find the failing route/dependency; apply RB-1 for the root dependency.
3. Latency: inspect the histogram tail (`*_latency_ms_bucket`); if first-token, check
   the model-server backend (GPU sidecar reachable, queue depth).
4. **Verify:** the attainment ratio recovers above objective and `slo_breaching` returns
   to 0 in the next scrape.

## RB-3: Node loss
Follow the tested restore runbook in [DR_PLAN.md](DR_PLAN.md#restore-runbook-simulated-node-loss--service).
The automated drill (`admin/config/tests/dr_drill.rs`) is the reference; it asserts a
verified restore within RTO/RPO and rejects a tampered backup.

## RB-4: Rotate the trust root
**Trigger:** scheduled rotation, or suspected root exposure.
1. Provision the new root on the HSM/TPM; verify its **attestation** (RT-28).
2. `RotatingKms::rotate_to(new_root)` — new data seals to the new root; **old journals
   are NOT rewritten** and still unseal with the retained old root.
3. Optionally re-seal hot stores under the new root on their next compaction.
4. Retire the old root only once all data under it is re-sealed or expired
   (`RotatingKms::retire` refuses to drop the current root).
5. **Verify:** seal/unseal round-trips on the new root; old blobs still open; audit the
   rotation.

## RB-5: Break-glass
**Trigger:** an operator needs emergency elevation an incident requires.
1. Open a grant through the break-glass controller (RT-32): justification + a distinct
   approver + a non-zero time-box. This **issues a scoped, time-boxed capability** and
   raises a **real-time security-officer alert**.
2. Use the elevation; it **auto-expires** at the time-box — do not rely on manual
   revoke.
3. After expiry, complete the **mandatory post-hoc review** (the grant stays outstanding
   until reviewed).
4. **Verify:** the grant, expiry, and review are all in the audit chain (RB-7).

## RB-6: High-risk config change
**Trigger:** a config change classified risk ≥ 4.
1. Submit through the guarded-action orchestrator (RT-31): it authenticates, **simulates**
   the change (real PDP dry-run), and parks it for approval.
2. A **distinct, qualified approver** approves → a **single-use, time-boxed commit token**
   is issued.
3. Commit with the token → the change lands in the signed config history and is audited
   as one linked transaction (request → approve → commit).
4. **Verify:** `head()` reflects the change; the linked audit entries verify (RB-7). A
   single-author attempt is refused by construction.

## RB-7: Verify the audit chain
**Trigger:** integrity check, post-mortem, or scheduled attestation.
1. Run `audit_writer::verify_chain` over the store's sealed entries with the store's
   verifying key.
2. A break means either tampering or a truncated/spliced ledger — treat as **Sev-1**
   (integrity compromise); do not continue serving from a store whose chain fails.
3. **Verify:** the chain re-verifies end-to-end; record the attestation result.
