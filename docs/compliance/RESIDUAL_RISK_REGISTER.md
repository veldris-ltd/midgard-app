# Residual-risk register (SAD §11/§15/§16 closure)

The remedial campaign (`docs/dev/remedial/remedial-tasks.md`, **RT-01 … RT-58**) is
fully worked. This register closes out that document's **§6 — SAD-declared residual
risks**: the design authors' own admissions of known gaps, technical debt, and
roadmap items. Each is resolved to a status so a GA reviewer sees, at a glance, what
is controlled, what is a deliberately accepted residual, and what remains open.

## Status legend

| Status | Meaning |
|--------|---------|
| **MITIGATED** | A concrete, implemented control addresses the risk. The residual (if any) is stated. |
| **ACCEPTED** | A residual risk that is inherent (performance, external process, or a fully-compromised host outside the trust boundary). A compensating control limits it; it is accepted, not eliminated. |
| **OPEN** | Not yet built, or an external activity (e.g. a certification lab) that must complete before GA. |

**Honesty note.** MITIGATED means the *control* is implemented and tested in this
repo. Controls whose full assurance is a deployment/lab activity (real HSM custody,
FIPS module certification, GPU inference) are marked with a **†** and their
deployment-tier is gated by RT-40's acceptance suite.

---

## SAD_00 — Core

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| PQ hardware acceleration not ubiquitous | Med | **ACCEPTED** | The PQ backend is pure-Rust FIPS 203/204/205 + X25519 — correct on every target without acceleration. Hardware accel is a *performance* option, not a correctness dependency. Residual: latency on low-end hardware, addressed by deployment sizing. |
| Mistral Small 3.2 weight provenance | Med | **MITIGATED †** | Model manifests are PQ-signed and provenance-recorded on custody transfer (`mimir/staging` intake + `sign_manifest`, RT-21/RT-50); load fails closed on a bad signature (AC-C-10 / `ModelSignatureInvalid`). Residual: the *real weights* are a deployment artefact — the deployment-tier suite (RT-40) verifies live inference. |
| Air-gap update workflow | Low | **MITIGATED** | Signed offline `.mgbundle` produce/import with verify-before-import over `--network none` (`deploy/air-gap-bundle`, `mg-bundle`, RT-19); the release path is Docker/native-pinned, no Nix (RT-47). |
| PQ performance overhead | Med | **ACCEPTED** | Inherent cost of post-quantum crypto; measured within budget (first-token < 800ms target, NFR-PERF-1). Compensating: deployment sizing; scheduler quotas/priority (RT-43). |
| Ontology sprawl | Med | **ACCEPTED** | Governance/operational concern, not a code defect. Compensating: typed object model + classification markings constrain unbounded growth; projection propagates markings. |
| Single model plane as shared dependency | Med | **ACCEPTED** | Architectural. Compensating: the plane is fail-closed and every product has a **defined degraded mode when Mimir is absent** (AC-M-9; AI-assist bridges fail closed to the non-AI path, RT-45). |
| Open-core boundary drift | Low | **MITIGATED** | Licence allow-list gate over the vendored OSS core (RT-06) + provenance-tag lint requiring every changed source to cite a requirement (RT-48). |
| **FIPS KAT certification before GA** | — | **OPEN** | The ACVP KATs run in-repo (`fips_backend`, `aead_kat`, `agility_fips`); **formal FIPS 140-3 module certification is an external lab activity** and is SAD_00's explicit GA bar. GA blocker. |

## SAD_01 — Forge

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| Third-party connector trust | Med | **MITIGATED** | Connectors run under a default-deny `SandboxPolicy`; the ingest funnel enforces egress for **every** connector (RT-27 + RT-55); credentials are KMS-resolved, never inline. Residual: a permitted host is trusted by operator choice. |
| Large-dataset PQ overhead | Low | **ACCEPTED** | Performance; sealing is per-dataset with a PQ-wrapped key. Compensating: deployment sizing. |
| Connector maintenance burden | Med | **ACCEPTED** | Operational. Compensating: one typed connector SDK + descriptor-based lineage keep connectors thin. |
| Pipeline drift | Med | **ACCEPTED** | Operational. Compensating: health-gating + projection markings; telemetry/SLOs (RT-33) surface drift. |

## SAD_02 — Mimir

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| Novel jailbreaks evade static guards | Med | **MITIGATED (residual ACCEPTED)** | Guardrail + per-family evals (RT-22/RT-23). Residual (accepted): a *novel* attack can by definition evade a static guard — compensated by defence-in-depth and the human gate on consequential actions. |
| **Autonomous action blast radius** | High | **MITIGATED** | The governed agent loop never puts the AI in the execution path: an action is *proposed*, human-authorised, then run through a real, audited actuator (RT-44); reversibility is checked pre-execute. |
| **Autonomy overreach** | High | **MITIGATED** | AI proposes, humans dispose: consequential actions fail closed without explicit human authorisation; the whole lineage is PQ-signed and audited (RT-44). |
| Inference capacity | Med | **MITIGATED †** | Real model backend (RT-20) + scheduler resource quotas & priority/preemption (RT-43). Residual: real GPU capacity is deployment-verified (RT-40). |
| Feedback poisoning | Med | **ACCEPTED** | Compensating: human-gated staging intake with recorded provenance; advisory assessment never approves — only the human gate does. |

## SAD_03 — Bifrost

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| **Compromised signing key** | High | **MITIGATED †** | Hardware (HSM/TPM) KMS trust root + operational key rotation (RT-28); CA/PKI lifecycle with revocation (RT-35); break-glass with enforced grants (RT-32); key-material zeroization + mlock (RT-39). Residual: real HSM custody is a deployment activity, gated by RT-40. |
| Canary blind spots | Med | **ACCEPTED** | Compensating: health-gated rollout with automatic rollback (AC-M-7). Residual: canary signal coverage is an observability/deployment concern (RT-33). |
| **Faulty release fan-out** | High | **MITIGATED** | Deterministic rollout orchestrator: one-at-a-time, health-checked, auto-rollback, never below `min_available` (RT-16); fleet coordination. |

## SAD_04 — Heimdall

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| **Automation bias** | High | **MITIGATED** | Heimdall never auto-executes a consequential action; AI options are advisory, grounded and cited; a human authorises, and the decision is audited (RT-44). |
| **Feed spoofing** | High | **MITIGATED** | Real feed ingestion transport + cross-node picture-sync over PQ-mTLS (RT-26). |
| **Over-trust in AI options** | High | **MITIGATED** | Options are strictly advisory, grounded against the operating picture and cited; the operator turns an option into a *proposal* that still requires explicit authorisation (RT-44). |
| **Feed integrity** | High | **MITIGATED** | PQ-mTLS transport + recorded provenance + tamper-evident audit lineage. |
| High-rate feed load | Med | **ACCEPTED** | Compensating: gateway rate limiting (RT-25) + scheduler quotas/priority (RT-43). Residual: sustained load is deployment sizing. |

## SAD_06 — Clients

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| Rooted/jailbroken device weakens keystore | Med | **ACCEPTED** | A fully-compromised host is outside the trust boundary. Compensating: `SecurityFloor` custody floors — the core `open()` fails closed below `min_floor`; no key material crosses into shell code (KDD-1). |
| Extension store review | Med | **ACCEPTED** | External process. Compensating: minimal-privilege broker limits the extension's surface. |
| On-device model tamper | Low | **MITIGATED** | Verify-before-load: a model with an invalid PQ signature is refused (AC-C-10 / `ModelSignatureInvalid`, RT-50 sign paths). |
| Binding-layer complexity | Med | **ACCEPTED** | Compensating: one shared core (KDD-1) with an FFI-friendly, opaque-blob binding surface (RT-58) so shells never touch merge internals. |
| Store policy divergence | Med | **ACCEPTED** | External, per-platform store policy. |
| On-device model footprint | Med | **ACCEPTED** | Compensating: quantisation + tiered inference (on-device / remote / hybrid, RT-57). Residual: deployment sizing. |
| Keystore guarantee variance | Med | **ACCEPTED** | Compensating: the `SecurityFloor` abstraction + `min_floor` fail-closed normalise across platforms. |

## SAD_07 — Admin

| Risk | Sev | Status | Control / rationale · residual |
|------|-----|--------|--------------------------------|
| Approver collusion | Med | **ACCEPTED** | Dual-control/separation-of-duties (RT-31) raises the bar but cannot eliminate collusion of *distinct* approvers. Compensating: tamper-evident audit lineage; break-glass alerting (RT-32). |
| Break-glass over-use | Med | **MITIGATED** | Break-glass emits real alerts and enforces time-boxed grants (RT-32). |
| Copilot over-trust | Med | **MITIGATED** | The admin copilot is advisory only; a proposal executes only through the guarded dual-control path — the copilot never acts (RT-31 orchestrator; RT-45 bridge is advisory). |
| **Privilege concentration** | High | **MITIGATED** | Least-privilege, distinct per-subsystem service identities — **no superuser identity is minted** (AC-AD-9); mutating actions traverse the dual-control transaction (RT-31). |
| **Self-governance bypass** | High | **MITIGATED** | Guarded-action orchestrator: every mutating admin action is one enforced, dual-controlled transaction wired to commits (RT-31). |
| Config drift vs subsystems ("GitOps reconciliation") | Med | **MITIGATED** | GitOps-style reconciler (`admin/config/src/reconcile.rs`, **RT-59**): the signed declared config is the desired state; a `Subsystem` seam observes each managed subsystem, drift is diffed, and `reconcile` converges each drifted subsystem then **re-observes to confirm convergence**. Fails closed — an unreachable subsystem is never reported in-sync, and an apply that does not converge is a failure. |

---

## Open items (nothing else is unresolved)

| # | Item | Kind | GA blocker? |
|---|------|------|-------------|
| 1 | **FIPS 140-3 module certification** (SAD_00) | External lab activity; in-repo KATs already pass | **Yes** — SAD_00's explicit bar |
| 2 | **Deployment-tier assurance** for the **†** controls (real HSM, GPU, multi-node cluster) | Deployment activity | **Yes** — gated by RT-40's acceptance suite + RT-49 shell audits, which fail closed until run on real infrastructure |

Everything else in §6 is either **MITIGATED** by an implemented, tested control or a
deliberately **ACCEPTED** residual (performance, external process, or a compromised
host outside the trust boundary). The former SAD_07 config-drift gap
("GitOps reconciliation") is now built (**RT-59**), so the only remaining OPEN items
are the two external activities above — certification and deployment verification.

## GA gate (from remedial-tasks §7, restated)

> The release go/no-go decision that consumes this register is recorded in
> [`GA_READINESS.md`](GA_READINESS.md).

Do not GA until: the R0–R4 remediation phases close (**done** — RT-01…58), the
operate-&-govern controls RT-33/34/35/38/40 close (**done**), and the two external
GA blockers above — **FIPS KAT certification** and **deployment-tier assurance on
real infrastructure** — complete. The code-side remediation is finished; what
remains is certification and deployment verification, both external to this repo by
nature and both explicitly gated (RT-40 fails a release closed until they run).
