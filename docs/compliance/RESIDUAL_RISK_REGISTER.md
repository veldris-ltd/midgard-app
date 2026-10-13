# Residual-risk register (SAD §11/§15/§16 closure)

The remedial campaign (**RT-01 … RT-59**) has been worked through, and its backlog
was retired on 2026-10-13 into
[`remedial-fixes.md` Appendix 6](../dev/fixes/remedial-fixes.md#appendix-6-retired-backlog-rt-to-rf-crosswalk),
which records each task as closed or carried by an `RF-*` item. This register tracks
the retired backlog's **§6, SAD-declared residual risks**: the design authors' own
admissions of known gaps, technical debt, and roadmap items. Each has a status so a
GA reviewer sees, at a glance, what is controlled, what is partly controlled, what is
a deliberately accepted residual, and what remains open.

**Re-graded 2026-10-13.** Every row was re-graded against the per-item Status notes
of [`remedial-fixes.md`](../dev/fixes/remedial-fixes.md) (audit of 2026-10-12) and
Appendix 6. The earlier edition graded 20 rows MITIGATED; 15 of those are now
**PARTIAL**, because the control they cite exists only as a tested library that no
running service uses, runs against a stand-in, or has a known defect. Nine of the ten
High-severity rows are PARTIAL. The re-grade was done by reading; nothing was built
or run for it.

## Status legend

| Status | Meaning |
|--------|---------|
| **MITIGATED** | A concrete, implemented and tested control addresses the risk where the risk arises. The residual (if any) is stated. |
| **PARTIAL** | A control exists but does not yet address the risk in a deployment: it is a library no running service uses, it is tested only against a stand-in, or it has a known defect. The row says what exists, what is missing, and which `RF-*` item carries it. |
| **ACCEPTED** | A residual risk that is inherent (performance, external process, or a fully-compromised host outside the trust boundary). A compensating control limits it; it is accepted, not eliminated. |
| **OPEN** | Not yet built, or an external activity (e.g. a certification lab) that must complete before GA. |

**What MITIGATED means here.** MITIGATED means the *control* is implemented and tested in this
repo. Controls whose full assurance is also a deployment/lab activity (real HSM
custody, FIPS module certification, GPU inference) are marked with a **†**; that part
is gated by the deployment-tier suite (RT-40), in which RC-40.3 (HSM) and RC-40.4
(GPU) have so far only been skipped.

---

## SAD_00: Core

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| PQ hardware acceleration not ubiquitous | Med | **ACCEPTED** | The PQ backend is pure-Rust FIPS 203/204/205 + X25519, which is correct on every target without acceleration. Hardware accel is a *performance* option, not a correctness dependency. Residual: latency on low-end hardware, addressed by deployment sizing. |
| GLM 5.2 weight provenance | Med | **PARTIAL †** | *Exists:* model manifests are ML-DSA-signed and `mimir-server` re-verifies manifest and weights before serving; a bad signature or digest fails closed. *Missing:* no vendor signature is ever checked cryptographically (it is recorded as `Unverifiable`), there is no upstream fetcher, and the one real model served so far went through `mg-model`, bypassing `mimir/staging`, the registry and the human approval record, under a development signing key. Carried by RF-MS-1 (RT-21). |
| Air-gap update workflow | Low | **PARTIAL** | *Exists:* `mg-bundle pack` / `import` produce and verify a signed `.mgbundle` offline, and a refused import leaves nothing behind; the release path is Docker/native-pinned, no Nix. *Missing:* the bundle carries binaries only (chart, base image, advisory snapshot and trust anchors come from a repository checkout), and there is no install run from the bundle alone in an isolated environment, no signed receipt and no differential bundle. Carried by RF-B-4, RF-DEP-5 (RT-19). |
| PQ performance overhead | Med | **ACCEPTED** | Inherent cost of post-quantum crypto. Compensating: deployment sizing. Residual: the overhead has not been measured: the only first-token figures are from a 2B model on CPU, the 800 ms objective (NFR-PERF-1) is unvalidated on GPU, and there is no published performance envelope (RF-SCL-4). |
| Ontology sprawl | Med | **ACCEPTED** | Governance/operational concern, not a code defect. Compensating: typed object model + classification markings constrain unbounded growth; projection propagates markings. |
| Single model plane as shared dependency | Med | **ACCEPTED** | Architectural. Compensating: the plane is fail-closed and every product has a **defined degraded mode when Mimir is absent** (AC-M-9; AI-assist bridges fail closed to the non-AI path, RT-45). |
| Open-core boundary drift | Low | **MITIGATED** | Licence allow-list gate over the SBOM, run in CI and in the per-artefact release gate (`mg-scan`, RT-06). Residual: the provenance-tag lint (RT-48) governs only `services`, `mimir`, `products` and `clients` (RF-SC-6). |
| **FIPS KAT certification before GA** | n/a | **OPEN** | The ACVP KATs run in-repo (`fips_backend`, `aead_kat`, `agility_fips`); **formal FIPS 140-3 module certification is an external lab activity** and is SAD_00's explicit GA bar. GA blocker. |

## SAD_01: Forge

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| Third-party connector trust | Med | **MITIGATED** | Connectors run under a default-deny `SandboxPolicy`; the ingest funnel enforces egress for **every** connector against the exact host and port (RT-27 + RT-55); credentials are KMS-resolved, never inline. Residual: a permitted host is trusted by operator choice; no test proves a credential never reaches logs or errors, a sandbox denial raises no security event, and the PostgreSQL connector has no TLS (RF-F-2). |
| Large-dataset PQ overhead | Low | **ACCEPTED** | Performance; sealing is per-dataset with a PQ-wrapped key. Compensating: deployment sizing. |
| Connector maintenance burden | Med | **ACCEPTED** | Operational. Compensating: one typed connector SDK + descriptor-based lineage keep connectors thin. |
| Pipeline drift | Med | **ACCEPTED** | Operational. Compensating: schema drift fails the sync closed; health-gating + projection markings. Residual: drift is a hard failure, not yet a governed human decision, and there are no measured pipeline SLOs (RF-F-2, RF-OBS-1). |

## SAD_02: Mimir

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| Novel jailbreaks evade static guards | Med | **PARTIAL** | *Exists:* a deterministic pre-filter with de-obfuscation, a real sanitise rewrite, and a `Classifier` seam. *Missing:* the classifier behind the seam is a test double, the adversarial corpus is a few hand-written strings with no block-rate target, and no Eval has run against a real model. A *novel* attack will in any case evade a static guard; that part stays accepted, compensated by the human gate. Carried by RF-MI-7, RF-MI-6 (RT-22, RT-23). |
| **Autonomous action blast radius** | High | **PARTIAL** | *Exists:* in the governed loop a consequential action is proposed, human-authorised and then executed and audited, and an irreversible action forces a human gate even when pre-authorised (tested, in-process). *Missing:* no service hosts the governed loop; an unknown action type is human-gated, not refused; there is no undo; and the HTTP edge's human gate accepts any non-empty token (`services/http-gateway/src/enclosure.rs:375`). Carried by RF-MI-4, RF-UID-3. |
| **Autonomy overreach** | High | **PARTIAL** | *Exists:* consequential actions fail closed without explicit human authorisation, with PQ-signed lineage (RT-44). *Missing:* in Heimdall a non-consequential action executes from `Proposed` with no recorded human decision (`heimdall/workflow/src/engine.rs:162`), and the edge token defect above applies. Carried by RF-H-4, RF-MI-4, RF-UID-3. |
| Inference capacity | Med | **PARTIAL †** | *Exists:* a real serving backend (llama.cpp on CPU, proven with one model) and a quota/priority scheduler as a library (RT-43). *Missing:* vLLM has never run on a GPU; there is no sidecar supervision, request cancellation, VRAM accounting or admission control in the serving path. Carried by RF-MI-1, RF-MI-3, RF-MI-8 (RT-20). |
| Feedback poisoning | Med | **ACCEPTED** | Compensating: advisory assessment never approves; only the human gate does. Residual: the staging intake that records provenance and the human decision is not on the path the one real model took (RF-MS-1). |

## SAD_03: Bifrost

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| **Compromised signing key** | High | **PARTIAL †** | *Exists:* verification fails closed without a pinned anchor; a `HardwareTrustRoot` with root attestation (RT-28); `MeshCa` issue/renew/revoke with a CRL (RT-35); key material is zeroized and `mlock`ed (RT-39). *Missing:* the hardware root is tested only against a software stand-in (RC-40.3 skipped); release keys have no validity windows or revocation, and Bifrost does not quarantine on revocation; `mg-admin mesh issue`, the path deployments use, writes certificates that never expire; no key ceremony has been held. Carried by RF-SC-8, RF-IAM-6, RF-SEC-8. |
| Canary blind spots | Med | **ACCEPTED** | Compensating: health-gated rollout with automatic rollback (AC-M-7). Residual: that logic is a library nothing drives against a real fleet (RF-B-3), and canary signal coverage is an observability concern (RF-OBS-1). |
| **Faulty release fan-out** | High | **PARTIAL** | *Exists:* a deterministic rollout orchestrator (ringed, health-gated, auto-reverting, never below `min_available`), tested as a library (RT-16); the registry admits only gate-verified artefacts. *Missing:* nothing drives it from the fleet manager, and the fleet agent installs nothing and cannot roll back. Carried by RF-B-3, RF-B-2. |

## SAD_04: Heimdall

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| **Automation bias** | High | **PARTIAL** | *Exists:* consequential actions need a recorded human authorisation and are audited (RT-44); options are parsed with their citations checked against the picture. *Missing:* a non-consequential action executes from `Proposed` with no human decision, ungrounded options are flagged but still returned, and neither workflow nor decision support is used by the running feed service. Carried by RF-H-4, RF-CA-10. |
| **Feed spoofing** | High | **MITIGATED** | `heimdall-feed-server` accepts an observation only over PQ-mTLS, only when the feed id equals the certificate's identity, and only with a valid signature (RT-26). Residual: no duplicate suppression, so a resubmitted observation is counted again (RF-H-1). |
| **Over-trust in AI options** | High | **PARTIAL** | *Exists:* options are advisory and an option becomes a proposal that still needs explicit authorisation. *Missing:* an option set containing an ungrounded option is returned with a flag rather than rejected (`heimdall/decision-support/src/adapter.rs:62`), and option generation is not served by the running service. Carried by RF-H-4, RF-CA-10. |
| **Feed integrity** | High | **PARTIAL** | *Exists:* PQ-mTLS transport, signed observations, a sealed picture and tamper-evident audit lineage. *Missing:* duplicates inflate track counts; the picture merge does not converge when two nodes hold different state with equal `last_seen` (`heimdall/picture/src/delta.rs:47`); observations carry no marking; there is no feed-staleness indication. Carried by RF-H-1, RF-H-3. |
| High-rate feed load | Med | **ACCEPTED** | Compensating: per-identity rate limiting and per-connection quotas on the mesh gateway (RT-25). Residual: the feed server has no backpressure or bounded buffering (RF-H-1); sustained load is deployment sizing. |

## SAD_06: Clients

No client shell exists yet (RF-CL-1..7): the controls below live in the shared client
core as a library and have not run on any device. The statuses describe that library.

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| Rooted/jailbroken device weakens keystore | Med | **ACCEPTED** | A fully-compromised host is outside the trust boundary. Compensating: `SecurityFloor` custody floors: the core `open()` fails closed below `min_floor`; no key material crosses into shell code (KDD-1). Residual: the client store and keystore are in-memory only (RF-PER-5). |
| Extension store review | Med | **ACCEPTED** | External process. Compensating: minimal-privilege broker limits the extension's surface. |
| On-device model tamper | Low | **MITIGATED** | Verify-before-load: a model with an invalid PQ signature is refused (AC-C-10 / `ModelSignatureInvalid`). Residual: there is no on-device runtime yet to load one (RF-CL-9). |
| Binding-layer complexity | Med | **ACCEPTED** | Compensating: one shared core (KDD-1) with an FFI-shaped, opaque-blob surface (RT-58) so shells never touch merge internals. Residual: no UniFFI or wasm-bindgen bindings exist (RF-CL-1). |
| Store policy divergence | Med | **ACCEPTED** | External, per-platform store policy. |
| On-device model footprint | Med | **ACCEPTED** | Compensating: tiered inference (on-device / remote / hybrid) with a real PQ-mTLS remote bridge (RT-57). Residual: the on-device tier is implemented only by a mock (RF-CL-9); deployment sizing. |
| Keystore guarantee variance | Med | **ACCEPTED** | Compensating: the `SecurityFloor` abstraction + `min_floor` fail-closed normalise across platforms. |

## SAD_07: Admin

Only `admin-identity` and `admin-privilege` are linked into a running service. The
break-glass, config, simulation, orchestrator and AI-assist crates are libraries
exercised by tests, and the Admin console renders fixtures (RF-AD-1).

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| Approver collusion | Med | **ACCEPTED** | Dual-control/separation-of-duties (RT-31) raises the bar but cannot eliminate collusion of *distinct* approvers. Compensating: tamper-evident audit lineage. Residual: a refused self-approval is not written to the audit chain (RF-AD-4). |
| Break-glass over-use | Med | **PARTIAL** | *Exists:* a controller that issues a scoped, time-boxed capability, revokes it on expiry and audits grant, expiry and review, with a durable, retrying alerter (RT-32). *Missing:* the only alert bus is an in-memory stand-in, the crate is linked into no service, actions taken inside a window are not marked, and the console screen is a constant. Carried by RF-AD-2, RF-CA-11. |
| Copilot over-trust | Med | **MITIGATED** | The admin copilot's response types can express only proposals, refusals and cited findings, and a test shows a proposal must still pass dual control, so the copilot cannot act on its own. Residual: it is not served from a running service, and prompts and responses are not audited (RF-AD-7). |
| **Privilege concentration** | High | **PARTIAL** | *Exists:* least-privilege, distinct per-subsystem service identities with no superuser minted (AC-AD-9); dual control with no self-approval on principal enrolment through the gateway. *Missing:* no role-conflict matrix and no scope check on delegated grants; an operator holding the state directory can issue mesh identities directly; the guarded orchestrator is not in a service. Carried by RF-IAM-5, RF-IAM-6, RF-AD-1. |
| **Self-governance bypass** | High | **PARTIAL** | *Exists:* a guarded-action orchestrator that makes a mutating admin action one simulated, dual-approved, audited transaction, and denies a single-author high-risk commit (RT-31, `rc_31.rs`). *Missing:* it is a library, not the route every admin mutation takes in a deployment, and there is no test that no privileged path bypasses it. Carried by RF-AD-1, RF-AD-4, RF-SEC-8. |
| Config drift vs subsystems ("GitOps reconciliation") | Med | **PARTIAL** | *Exists:* a reconciler over the signed, hash-chained config store that diffs declared against observed state, converges and re-observes, failing closed on an unreachable subsystem (**RT-59**, `admin/config/src/reconcile.rs`). *Missing:* the `Subsystem` seam has only in-memory test implementations and nothing links the reconciler into a service. Carried by RF-AD-3. |

---

## Summary

| Status | Rows |
|--------|------|
| MITIGATED | 5 |
| PARTIAL | 15 |
| ACCEPTED | 17 |
| OPEN | 1 |
| **Total** | **38** |

## Open items

| # | Item | Kind | GA blocker? |
|---|------|------|-------------|
| 1 | **FIPS 140-3 module certification** (SAD_00) | External lab activity; in-repo KATs already pass | **Yes**: SAD_00's explicit bar |
| 2 | **Deployment-tier assurance** for the **†** controls (real HSM, GPU, multi-node cluster) | Deployment activity | **Yes**: gated by RT-40's acceptance suite + RT-49 shell audits, which fail closed until run on real infrastructure |
| 3 | **The 15 PARTIAL rows**, nine of them High severity | In-repo work, carried by the `RF-*` items each row names | **Yes** for the High-severity rows; the others before GA unless explicitly accepted with a rationale here |

The earlier edition of this register stated that only items 1 and 2 were unresolved.
That was not correct. Most of the controls it cited are real and tested, but only as
libraries that no deployed service runs yet.

## GA gate (from the retired remedial-tasks §7, restated)

> The release go/no-go decision that consumes this register is recorded in
> [`GA_READINESS.md`](GA_READINESS.md).

Do not GA until: the R0–R4 remediation phases close, the operate-&-govern controls
RT-33/34/35/38/40 close, and the two external GA blockers above (**FIPS KAT
certification** and **deployment-tier assurance on real infrastructure**)
complete. As of 2026-10-13 the first two conditions are **not** met: Appendix 6
lists RT-20, RT-21, RT-33, RT-34, RT-35, RT-38 and RT-40, among others, as carried
by open `RF-*` items. RT-40 fails a release closed until the deployment tier runs.
