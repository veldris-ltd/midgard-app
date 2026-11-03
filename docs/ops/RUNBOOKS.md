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
- [RB-8: Disengage Mimir inference](#rb-8-disengage-mimir-inference)
- [RB-9: Rotate or withdraw the model trust anchor](#rb-9-rotate-or-withdraw-the-model-trust-anchor)
- [RB-10: Heimdall feed observations refused as stale or replayed](#rb-10-heimdall-feed-observations-refused-as-stale-or-replayed)
- [RB-11: A service could not lock process memory](#rb-11-memory-lock)

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

## RB-8: Disengage Mimir inference
**Trigger:** a model's approval is withdrawn (licence, CVE, evaluation regression), or
inference must stop now, for an incident or on command. [VLD-ANB-MIDGARD-0057]

mimir-server re-reads two files in its state directory (`--state-dir`, `/state` in
compose and Helm) every `--reload-interval` seconds (default 60). Writing there needs
host or volume access, which is the authority this switch requires; there is no network
endpoint for it.

1. To stop all inference, create the stop file: `touch /state/STOP` (for example
   `docker compose exec mimir-server touch /state/STOP`, or
   `kubectl exec <mimir-server pod> -- touch /state/STOP`). Calls are refused
   `SERVING_STOPPED`.
2. To withdraw one model, add its manifest id (the `model` field of the `serving` log
   line) on a line of its own to `/state/revoked.list` (or the `--revocation-file`
   path); `#` starts a comment. Calls are refused `MODEL_REVOKED`. An unreadable or
   malformed list also stops serving: it fails closed.
3. Within one interval the log says `serving stopped` with the `reason` and `detail`,
   then `drained` once the completions already admitted have returned (at most
   `--infer-timeout` later; `drain timed out` if they have not).
4. To resume, delete `/state/STOP`, or remove the id from `revoked.list` (only once the
   model is approved again). Within one interval the log says `serving resumed`.
5. **Verify:** while stopped, `GET /readyz` on the health port returns 503 with
   `model-verified` failing, so an orchestrator stops routing; `mimir-probe` is refused
   `SERVING_STOPPED` or `MODEL_REVOKED`; `/metrics` counts
   `inference_refused_total{reason="serving_stopped"}` or `{reason="model_revoked"}`.
   The ledger holds each refused call as its decision entry followed by an
   `Infer:SERVING_STOPPED` or `Infer:MODEL_REVOKED` entry; verify it per RB-7. After
   resuming, `/readyz` returns 200 and a probe is served.

## RB-9: Rotate or withdraw the model trust anchor
**Trigger:** the model-signing key is rotated, or suspected of exposure.
[VLD-ANB-MIDGARD-0057]
1. Re-sign the approved model manifests with the new key (`mg-model sign`) and stage
   them beside the weights.
2. Replace the model trust anchor mimir-server reads (`--model-trust`, the pinned
   `deploy/trust/model-signing.pub`, carried read-only in the image at
   `/etc/midgard/trust/model-signing.pub`; in Helm the `model-signing.pub` key of the
   `trustConfigMap`) with the new public key from `mg-model keygen`, committed
   through a signed, reviewed change. [VLD-ANB-MIDGARD-0028]
3. A running service re-checks its manifest against the trust file every reload
   interval: a manifest the new key does not verify stops serving, refused
   `MODEL_REVOKED`, with `the manifest signature does not verify` in the
   `serving stopped` log line. Restart the service on the re-signed manifest; to
   withdraw a key with no replacement, replacing the trust file is enough to stop
   serving everywhere it is mounted.
4. **Verify:** the restarted service logs `serving` with the expected `model` and
   `model_version`, `/readyz` returns 200 and `mimir-probe` is served.

## RB-10: Heimdall feed observations refused as stale or replayed
**Trigger:** a feed source reports `denied` from `heimdall-feed`, or the feed ledger
shows `Ingest refused` rows with reason `observed-at-out-of-window`, `replay` or
`sequence-unset`. [VLD-ANB-MIDGARD-0041]
1. Read the refusal's message and reason. The feed gateway's checks are, in order:
   - **SequenceUnset** (reason `sequence-unset`): the source sent sequence 0. It was
     built against observation format version 1 or does not number its observations;
     upgrade it (version 1 observations are refused at decode as malformed).
   - **SequenceNotMonotonic** (reason `replay`, message "is not after the last
     admitted sequence"): either a replay of an observation already admitted, or a
     feed that restarted its sequence counter. A restarted feed must continue its
     sequence above the last one it sent (`heimdall-source` numbers by the clock in
     nanoseconds, so this means its clock stepped back), or be re-registered under a
     new feed identity. The service keeps each feed's last sequence in
     `feed-sequences.sealed` in its state directory, so restarting `heimdall-feed`
     does not clear it; removing that file while the service is stopped resets every
     feed, and reopens replay of anything still inside the freshness window.
   - **Stale** (reason `observed-at-out-of-window`): the feed's clock and the
     server's differ by more than `--max-observation-skew` (ahead) or the observation
     is older than `--max-observation-age`. Check both clocks; widen the age only for
     a deliberate backfill (as `mg-install`'s sample replay does), then restore it.
   - **Replayed** (reason `replay`, message "already been received"): the same
     observation id arrived again under a new sequence. The gateway remembers the last
     4,096 ids per feed in memory; the window restarts empty after a gateway restart,
     when the persisted sequence alone refuses earlier observations.
2. A run of `replay` refusals from one feed that is not explained by a restart or a
   clock change is a possible capture-and-replay attempt: treat it as a security
   event, preserve the ledger and check who holds that feed's certificate.
3. **Verify:** the source's next observation is `accepted`, and the ledger shows no
   further refusals for it.

## RB-11: A service could not lock process memory
**Trigger:** a service logs `could not lock process memory (...)` at start, naming
`RLIMIT_MEMLOCK` and `CAP_IPC_LOCK`. [VLD-ANB-MIDGARD-0036]
1. The service is running, but key material is kept out of swap only per secret, not
   for the whole process. Every mesh service calls `mlockall(MCL_CURRENT | MCL_FUTURE)`
   at start, and only when `RLIMIT_MEMLOCK` is unlimited: with a finite limit, every
   allocation past it would fail.
2. Set the memory-lock limit to unlimited for the service: `ulimit -l unlimited` in
   its start script, `LimitMEMLOCK=infinity` in a systemd unit, `ulimits: memlock:
   -1` in Compose, or the container runtime's equivalent on Kubernetes. Granting
   `CAP_IPC_LOCK` alone is not enough while the limit stays finite.
3. A locked process cannot be swapped, so its resident memory counts in full against
   the container memory limit; size `defaultResources` in
   `deploy/helm/midgard/values.yaml` for that. If a host cannot absorb it, set
   `MIDGARD_NO_MLOCKALL=1` for the service (it logs that it is running without the
   process lock) and record the decision.
4. **Verify:** restart the service; it no longer logs the line, and
   `grep -E "VmLck|VmRSS" /proc/<pid>/status` shows locked memory close to its resident size.
