# Midgard Suite — Remedial Fixes & Completion Plan

**Status:** Draft 1 · **Compiled:** 2026-08-25 · **Branch audited:** `dev` @ `71e2a58`
**Revision 2:** 2026-10-12 · public `main` @ `9ec8659` · adds [Workstream Y — Claim alignment & public release](#y--claim-alignment--public-release) (`RF-CA-1..13`). Every item in A–X was re-audited against that commit and now opens with a dated Status note; see [§1a](#1a-current-position--audit-of-2026-10-12).
**Scope:** the entire repository — core services, Mimir, model supply, Forge, Bifrost, Heimdall, Admin Control Panel, Client Platforms, the SDK, the design system, every console UI, the CI/release gate, deployment, documentation, and the end-to-end user flows that cross them.

---

## 0. What this document is

The [BUILD_PLAYBOOK](../../../BUILD_PLAYBOOK.md) describes how the suite was **built to specification**: 8 phases, 72 acceptance criteria, 15 runtime AI prompts, 50+ Evals. That work landed. Every one of the 72 `AC-*` ids is referenced by a test and driven by a `deploy/acceptance/verify-*.sh` script, and every `FR-*`/`NFR-*`/`DD-*` id in the SADs (bar one design-only note) is cited in source.

This document describes the **other half**: everything the playbook did not ask for, deferred to "a later phase", stubbed to keep a gate green, or left as an in-memory reference implementation of a thing that must eventually be real. It is the completion plan from *spec-complete reference implementation* to *operable product*.

It is written to be executed the same way the playbook was: each item is a self-contained, copy-pasteable **prompt** for a coding agent, followed by **acceptance criteria** in Given/When/Then form and a **definition of done**. New criteria are numbered `AC-RF-*` so they extend, rather than collide with, the 72 in Appendix B of the playbook.

### How to read an item

| Field | Meaning |
|---|---|
| **Severity** | `Critical` / `High` / `Medium` / `Low` — see §2 |
| **Area** | Which workstream it belongs to |
| **Evidence** | The file(s) and line(s) that establish the gap — verified, not inferred |
| **Depends on** | Items that must land first |
| **Prompt** | The agent instruction. Copy verbatim; adjust ids to your target |
| **Acceptance criteria** | `AC-RF-*` — the Given/When/Then contract |
| **Done when** | The gate; usually "criteria pass under `--network none`" |

### Rules that bind every item

These carry forward from the playbook and are **not restated** in each prompt. Assume them.

1. **Fail closed.** Absent, invalid, or unverifiable inputs deny. No soft passes, no silent fallbacks. A stub that returns success is a security defect, not a placeholder.
2. **Provenance tags.** Every new source file cites the `[FR-*]` / `[NFR-*]` / `[DD-*]` / `[AC-*]` ids it serves.
3. **Post-quantum by default.** New transport is PQ-mTLS; new data at rest is KEM-DEM sealed; new signatures are ML-DSA. Never re-roll crypto — consume `crypto/pqc-agility`.
4. **Air-gap first.** Anything added must build, test, and run under `docker run --network none`. Network-requiring work must be behind an explicit, off-by-default feature and must degrade to a fail-closed error, never a hosted fallback.
5. **Governed.** Every new mutation path goes through `services/governance` for the policy decision and writes PQ-signed audit lineage. Every new AI touchpoint is assembled through the eight-layer pipeline and guarded at all four interception points.
6. **Design system only.** Every pixel comes from `@midgard/design-system`. One accent per screen, mono ALL-CAPS labels, `ProductMark` not invented logos, PQ state visible in the UI, never emoji.
7. **Reproducible.** New buildable artefacts are bit-reproducible, emit a CycloneDX SBOM, and are ML-DSA-signed at release.

---

## 1. Executive summary

**What is real and good.** The post-quantum substrate is genuine: `crypto/pqc-agility` wraps ACVP-validated pure-Rust FIPS 203/204/205 with a negotiation/agility layer and a hybrid combiner; `crypto/pq-mtls` runs a real handshake over TCP; `services/kms` implements correct KEM-DEM envelope encryption with an HSM/TPM seam; `services/governance` decides and audits in one envelope with a PQ-signed append-only ledger. The object fabric, the eight-layer prompt assembly, the governed plan-act-observe loop with INV-1..8, the Evals harness, the policy simulator, the CRDT client sync, and the six console UIs are all substantially implemented — ~30 000 lines of dependency-light Rust plus ~7 200 lines of console TSX, all traceable to spec.

**What is not there.** Six structural absences, each of which blocks operation regardless of how many acceptance criteria pass:

| # | Absence | Evidence |
|---|---|---|
| 1 | **Nothing durable.** No crate anywhere performs file or database I/O. Every store — object fabric, audit ledger, model registry, dataset store, keystore, config — is an in-process `Vec`/`BTreeMap`. A restart is total data loss. | No `std::fs` / `File::` in any of `services/`, `mimir/`, `forge/`, `bifrost/`, `heimdall/`, `admin/`, `clients/`, `crypto/` |
| 2 | **Almost nothing is runnable.** 56 crates, 5 binaries: `http-gateway` and the four supply-chain tools. The other 51 are libraries with no process, no config, no lifecycle, no health endpoint. | `find -name main.rs` |
| 3 | **The release gate passes vacuously.** `mg-sign` and `mg-verify` print "STUB (Phase 0.b)" and `return ExitCode::SUCCESS`. `mg-repro workspace` — the exact invocation CI makes — returns success without building. Release-time CVE scan, licence allow-list, and provenance attestation are `echo "TODO"`. | `tools/mg-sign/src/main.rs:17`, `tools/mg-verify/src/main.rs:16`, `tools/mg-repro/src/main.rs:24`, `.github/workflows/release.yml` |
| 4 | **Five of six consoles show fabricated data.** Only the Midgard core console can point at a real edge, and only for 15 endpoints. Admin, Forge, Mimir, Bifrost, Heimdall and Clients are hard-wired to ~3 100 lines of TypeScript fixtures with no server behind them. | `products/*/ui/src/main.tsx`, `sdk/ts/src/products/*.ts`, `services/http-gateway/src/lib.rs:64-129` |
| 5 | **No inference.** The "local AI plane" serves a deterministic string-generating reference engine. The GPU backend produces a `LaunchSpec` and never spawns anything; there is no tokenizer, no weight loader, no sampler, no streaming. | `mimir/model-server/src/backends/local.rs:3-11`, `backends/gpu_docker.rs:10-15` |
| 6 | **Six client shells exist as two TOML files each.** No Swift, no Kotlin, no GTK, no Tauri, no SvelteKit, no extension, no UniFFI, no wasm-bindgen. Accessibility conformance is *declared* in `accessibility.toml` and machine-checked for completeness of the declaration — not for the behaviour. | `clients/shells/**` (12 files, all `.toml` + a README) |

**Beyond those,** the audit found: no observability of any kind (no metrics, traces, or structured logs — the Observability console renders fixtures); a 2-file proto contract for an 8-subsystem platform; no deployment artefacts (`deploy/helm/` and `deploy/air-gap-bundle/` are referenced by `deploy/README.md` and do not exist); no authentication anywhere in the web tier (no login, no session, no token refresh); effectively no frontend accessibility (7 `aria-`/`role` occurrences across 7 200 lines), no error boundaries, no i18n; zero frontend tests of any kind; no multi-tenancy; no backup/restore or DR; and several places where documentation asserts things the tree contradicts (Go and DataFusion backends that do not exist, a Python workspace whose members are Rust crates).

**Sizing.** 169 remedial items across 24 workstreams, carrying roughly 800 acceptance criteria. If sequenced as written — supply chain and runtime first, then persistence and transport, then product depth and UI wiring, then clients, operations and assurance — this is on the order of a 6–8 quarter programme for a team of 10–14. The dependency graph in §3 is the critical path; the phased plan in §4 is the suggested cut.

**Revision 2 note (2026-10-12).** Workstream Y adds 13 items and 57 acceptance criteria, for 182 items and 881 criteria across 25 workstreams. It was written against public `main` @ `9ec8659`, which has moved a long way from the tree audited above: the workspace is 70 crates, not 56; five services run as processes (`http-gateway`, `mimir-server`, `bifrost-fleet`, `heimdall-feed`, `forge-worker`); state and the audit ledger are durable and sealed; `mg-sign` and `mg-verify` are real; a real model has been served on CPU. Absences 1, 2, 3 and 5 in the table above are therefore partly closed; absences 4 and 6 stand. The summary above is kept as the record of 2026-08-25. **The current position is §1a**, and each item's Status note; where a Status note and the Evidence beneath it disagree, the Status note is current.

---

## 1a. Current position — audit of 2026-10-12

Every item was re-checked against public `main` @ `9ec8659` on 2026-10-12, by reading the tree. Each item now opens with a **Status** note giving what exists, which acceptance criteria are met, and what is still to do; the original Evidence paragraph beneath it is kept as the record of 2026-08-25 and is no longer the current position where the two differ. Nothing was built or run for this audit (Appendix 5), so *Done* means an implementation and a covering test were found for every criterion, not that the test was observed to pass.

| # | Workstream | Items | Done | Partial | Open | Criteria met |
|---|---|---|---|---|---|---|
| **A** | Supply chain & release integrity | 8 | 2 | 5 | 1 | 17 / 40 |
| **B** | Runtime & process model | 7 | 0 | 4 | 3 | 1 / 35 |
| **C** | Persistence & durability | 8 | 0 | 4 | 4 | 5 / 45 |
| **D** | Transport & API surface | 7 | 0 | 5 | 2 | 0 / 34 |
| **E** | Identity, authentication & session | 6 | 0 | 6 | 0 | 0 / 30 |
| **F** | Observability & operations telemetry | 5 | 0 | 4 | 1 | 1 / 23 |
| **G** | Mimir: real inference | 8 | 0 | 5 | 3 | 5 / 44 |
| **H** | Model supply chain | 5 | 0 | 1 | 4 | 3 / 22 |
| **I** | Forge: execution depth | 7 | 0 | 6 | 1 | 4 / 36 |
| **J** | Bifrost: delivery reality | 6 | 0 | 3 | 3 | 1 / 32 |
| **K** | Heimdall: fusion reality | 6 | 0 | 3 | 3 | 0 / 31 |
| **L** | Admin Control Panel depth | 7 | 0 | 4 | 3 | 2 / 37 |
| **M** | UI platform foundations | 10 | 0 | 2 | 8 | 0 / 56 |
| **N** | UI data wiring | 7 | 0 | 2 | 5 | 0 / 33 |
| **O** | End-to-end user flows | 12 | 0 | 9 | 3 | 0 / 61 |
| **P** | Client platforms | 9 | 0 | 2 | 7 | 1 / 44 |
| **Q** | SDKs & developer experience | 5 | 0 | 3 | 2 | 1 / 21 |
| **R** | Deployment & topology | 7 | 0 | 5 | 2 | 0 / 32 |
| **S** | Resilience, backup & disaster recovery | 5 | 0 | 5 | 0 | 2 / 23 |
| **T** | Security assurance | 8 | 0 | 6 | 2 | 1 / 36 |
| **U** | Test strategy & coverage | 8 | 0 | 2 | 6 | 1 / 33 |
| **V** | Compliance & assurance artefacts | 6 | 0 | 4 | 2 | 1 / 25 |
| **W** | Multi-tenancy, scale & performance | 5 | 0 | 1 | 4 | 0 / 21 |
| **X** | Documentation, design system & drift | 7 | 0 | 4 | 3 | 0 / 30 |
| **Y** | Claim alignment & public release | 13 | 0 | 2 | 11 | 0 / 57 |
| | **Total** | **182** | **2** | **97** | **83** | **46 / 881** |

A criterion counts as met only where both the implementation and a test exercising it were found. Criteria that need a run, hardware or an outside party are listed per item as not verifiable by reading and are not counted as met. The full per-item list is in [Appendix 1](#appendix-1--complete-item-register-182).

**Why so few criteria are met when so much has landed.** The work since August was done against the earlier backlog ([`remedial-tasks.md`](../remedial/remedial-tasks.md), ids `RT-nn` / `RC-nn`), not against this document. Almost no source file cites an `AC-RF-*` id, and `deploy/acceptance/verify-remedial.sh` (Appendix 3) does not exist. Most items are therefore *Partial*: the capability is partly or largely built, but the specific criteria written here have no named test. Closing an item now usually means writing those tests and the remaining pieces listed under *Still to do*, not starting over.

**Scope changes.** 85 items carry a *Scope change* note where the tree has moved past the item's premise. Read the note before the prompt: the prompt is kept as written, and the note says how to read it now.

### Findings that cut across items

1. **There is no CI configuration in the tree.** Every criterion of the form "when CI runs, it fails" is open, and several items cite workflows as evidence that a reader cannot find. RF-CA-4 is a prerequisite for all of them.
2. **The acceptance gate has not been re-run on any public commit**, and a fresh clone lacks the cargo configuration the offline build needs (RF-CA-3, RF-CA-5).
3. **Six consoles have no backend route.** `forge-worker`, `mimir-server`, `bifrost-fleet` and `heimdall-feed` are mesh-only services; the browser edge serves only the Midgard core surface and admin enrolment. Every criterion that names a Forge, Mimir, Bifrost, Heimdall, Admin or Clients screen is open for that reason (RF-UID-1).
4. **No TypeScript test runner exists** in any `package.json`, so no frontend criterion is met (RF-TST-2).
5. **Document paths in the prompts are wrong.** Operations documents live in `docs/ops/` and compliance documents in `docs/compliance/`, not under `docs/dev/operations/` or `docs/dev/compliance/`.

### Decisions to make before more is built

Each of these is a place where the tree took a different design from the one this document prescribes. The items affected cannot be closed, or sensibly rewritten, until the choice is recorded in an ADR (RF-DOC-4).

| Decision | The plan says | The tree does | Items affected |
|---|---|---|---|
| Persistence | A storage engine behind key-value, log and blob traits | KMS-sealed append-only journals replayed into in-memory stores | RF-PER-1, RF-PER-3, RF-PER-8, RF-SCL-2 |
| Process model | One process per service, the edge forwarding over RPC | Five binaries; iam, fabric, kms and governance composed inside the gateway | RF-RT-1, RF-API-3, RF-PER-7 |
| Inter-service contract | Proto per subsystem with generated stubs | A hand-written PQ-mTLS codec typed in Rust; protos govern the client edge only | RF-API-1, RF-API-2, RF-SDK-1..3 |
| Human second factor | WebAuthn / FIDO2 security keys | Password plus TOTP for users; ML-DSA passkeys in keystore files for administrators, which no browser can produce | RF-IAM-1, RF-UI-1, RF-UX-1, RF-UX-2, RF-AD-1 |
| Replication | Multi-replica HA with zero-downtime upgrades | A locked single-writer state directory per service; one replica each | RF-DEP-2, RF-RES-2, RF-RES-3, RF-CA-9 |
| Configuration store | A Git repository with signed commits | A signed, hash-chained store with a drift reconciler | RF-AD-3, RF-DEP-6 |
| Tenancy | To be decided before the storage keyspace is set | Undecided; the storage design has since landed | RF-SCL-1, RF-SCL-5 |
| Go and Python | SDKs and services in both | No Go or Python code; build recipes and documents still refer to them | RF-SDK-3, RF-SDK-4, RF-DOC-1 |

### Defects found while auditing

These are faults in code or documents that exist today, as distinct from missing work. Each was confirmed by opening the file.

| Defect | Where | Item |
|---|---|---|
| Picture merge does not converge: on equal `last_seen` each side adopts the other's entity | `heimdall/picture/src/delta.rs:47` | RF-H-3 |
| A non-consequential action executes from `Proposed` with no recorded human decision | `heimdall/workflow/src/engine.rs:162` | RF-H-4 |
| Ungrounded options are returned with a flag, not rejected | `heimdall/decision-support/src/adapter.rs:62` | RF-H-4, RF-CA-10 |
| The human gate accepts any non-empty token | `services/http-gateway/src/enclosure.rs:375` | RF-UID-3 |
| The observability view returns literal SLO figures ("QUERY P95 38ms") | `services/http-gateway/src/data.rs:226` | RF-UID-3, RF-SCL-4 |
| The console login form has no TOTP field, so users enrolled with `--totp` cannot sign in from it | `ui/shared/src/AuthGate.tsx` | RF-UI-1 |
| The DR plan states a 60-second RPO through journal shipping to a warm replica, which is not implemented | `docs/ops/DR_PLAN.md:16` | RF-RES-2 |
| A doc comment relies on a `just check-backends` gate that is not in the `justfile` | `crypto/pqc-agility/src/backend/mod.rs:8` | RF-SEC-3 |

Further defects reported by the item reviews and not separately re-confirmed are recorded in the status note of the item concerned — among them: mesh certificates issued by `mg-admin` never expire (RF-IAM-6), `/metrics` is served on the public port with the raw request path as a label (RF-OBS-1), the federation bridge copies roles from the upstream assertion (RF-IAM-4), and a test asserts the opposite of AC-RF-AD-4c (RF-AD-4).

---

## 2. Severity model

| Severity | Definition | Response |
|---|---|---|
| **Critical** | A security control that appears to work but does not, or an absence that makes the system unusable/unrecoverable in any real deployment. | Fix before any deployment, including internal pilots. |
| **High** | A capability the SADs treat as core that is absent or simulated; a user-facing surface backed by fabricated data; a compliance claim not evidenced. | Fix before GA. |
| **Medium** | A real gap in depth, robustness, coverage, or operability that will bite in production but has a workaround. | Fix before scale-out. |
| **Low** | Drift, polish, hygiene, or documentation accuracy. | Fix opportunistically; batch. |

**Truth-in-status rule.** Until an item is closed, no README, SAD, or release note may describe the capability it covers as present. Where this document contradicts a status line elsewhere in the repo, this document is correct — and RF-DOC-1 exists to reconcile them.

---

## 3. Workstream index and dependency order

| # | Workstream | Items | Severity ceiling | Gate |
|---|---|---|---|---|
| **A** | [Supply chain & release integrity](#a--supply-chain--release-integrity) | RF-SC-1..8 | Critical | The gate refuses an unsigned, unscanned, or non-reproducible artefact |
| **B** | [Runtime & process model](#b--runtime--process-model) | RF-RT-1..7 | Critical | Every service is a configurable, observable, gracefully-terminating process |
| **C** | [Persistence & durability](#c--persistence--durability) | RF-PER-1..8 | Critical | State survives restart; the audit chain survives restart *and* proves it |
| **D** | [Transport & API surface](#d--transport--api-surface) | RF-API-1..7 | High | Every subsystem has a proto contract and a reachable, authenticated edge |
| **E** | [Identity, authentication & session](#e--identity-authentication--session) | RF-IAM-1..6 | Critical | A human can log in, step up, and be revoked |
| **F** | [Observability & operations telemetry](#f--observability--operations-telemetry) | RF-OBS-1..5 | High | SLOs are measured, not fixtures |
| **G** | [Mimir — real inference](#g--mimir--real-inference) | RF-MI-1..8 | High | A signed GGUF answers a grounded question on a GPU node, offline |
| **H** | [Model supply chain](#h--model-supply-chain) | RF-MS-1..5 | High | A real upstream model reaches serving through staging, unmodified in trust posture |
| **I** | [Forge — execution depth](#i--forge--execution-depth) | RF-F-1..7 | High | A pipeline over 10⁸ rows executes, versions, and projects |
| **J** | [Bifrost — delivery reality](#j--bifrost--delivery-reality) | RF-B-1..6 | High | A ring rollout reaches a real fleet agent and rolls back |
| **K** | [Heimdall — fusion reality](#k--heimdall--fusion-reality) | RF-H-1..6 | High | Live feeds fuse into one picture with sub-second sync |
| **L** | [Admin Control Panel depth](#l--admin-control-panel-depth) | RF-AD-1..7 | High | Every console action reaches a real subsystem under dual control |
| **M** | [UI platform foundations](#m--ui-platform-foundations) | RF-UI-1..10 | High | Auth, errors, a11y, i18n, keyboard, responsive, offline |
| **N** | [UI data wiring](#n--ui-data-wiring) | RF-UID-1..7 | High | Zero mock gateways in any shipped build |
| **O** | [End-to-end user flows](#o--end-to-end-user-flows) | RF-UX-1..12 | High | Each journey completes against real services, audited end to end |
| **P** | [Client platforms](#p--client-platforms) | RF-CL-1..9 | High | Six shells build, ship, and pass on-device audits |
| **Q** | [SDKs & developer experience](#q--sdks--developer-experience) | RF-SDK-1..5 | Medium | Four language SDKs generated from proto, documented, versioned |
| **R** | [Deployment & topology](#r--deployment--topology) | RF-DEP-1..7 | Critical | Single-node, HA, and air-gapped installs are one command each |
| **S** | [Resilience, backup & DR](#s--resilience-backup--dr) | RF-RES-1..5 | Critical | Documented RPO/RTO, exercised restore |
| **T** | [Security assurance](#t--security-assurance) | RF-SEC-1..8 | Critical | Threat model validated by test, not assertion |
| **U** | [Test strategy & coverage](#u--test-strategy--coverage) | RF-TST-1..8 | High | Unit, property, fuzz, integration, E2E, load, chaos, a11y, visual |
| **V** | [Compliance & assurance artefacts](#v--compliance--assurance-artefacts) | RF-CMP-1..6 | High | Control mappings evidenced from CI, not prose |
| **W** | [Multi-tenancy, scale & performance](#w--multi-tenancy-scale--performance) | RF-SCL-1..5 | High | Tenant isolation proven; published performance envelope |
| **X** | [Documentation, design system & drift](#x--documentation-design-system--drift) | RF-DOC-1..7 | Medium | No statement in the repo is contradicted by the repo |
| **Y** | [Claim alignment & public release](#y--claim-alignment--public-release) | RF-CA-1..13 | Critical | Every published claim is checkable from one signed, tagged public commit |

### Critical path

```
A (supply chain)  ──┐
B (runtime)       ──┼──▶ C (persistence) ──▶ D (transport) ──▶ E (identity/session)
                    │                                              │
                    └──▶ R (deployment) ◀──────────────────────────┘
                                │
        ┌───────────────────────┼───────────────────────┐
        ▼                       ▼                       ▼
   F (observability)      G/H (Mimir+supply)      I/J/K/L (product depth)
        │                       │                       │
        └───────────────────────┴───────────┬───────────┘
                                            ▼
                             N (UI wiring) ──▶ M (UI foundations) ──▶ O (user flows)
                                            │
                                            ├──▶ P (clients) ──▶ Q (SDKs)
                                            │
                                            └──▶ S/T/U/V/W (assurance) ──▶ X (docs)
```

**Sequencing rule.** Do not start Workstream N (UI wiring) before D and E: pointing a console at an unauthenticated, contractless edge produces a second thing to rewrite. Do not start P (clients) before Q's UniFFI/wasm surface is stable. T, U and V run continuously alongside everything from B onward, not as a phase at the end.

**Workstream Y sits outside this graph.** Its truth track (RF-CA-1..7, RF-CA-13) depends on nothing above and comes first: it changes what is said, not what is built. Its capability track (RF-CA-8..12) wires libraries that A–X already produced into the services that now run, and each item names the A–X items it extends.

---
# A — Supply chain & release integrity

> **Why first.** The suite's core marketing and security claim is a post-quantum, reproducible, signed, SBOM'd supply chain. Today the tools that enforce that claim exit `0` without doing anything, and the CI job that checks reproducibility is invoked with an argument that short-circuits it. This is the only workstream where the gap is not "missing" but "misleading" — the gate looks green.

---

### RF-SC-1 · Implement `mg-sign` — ML-DSA detached signature + DSSE attestation

**Severity:** Critical · **Area:** Supply chain · **Depends on:** — · **Blocks:** RF-SC-2, RF-SC-5, RF-DEP-4, RF-B-2

> **Status at 2026-10-12 — Done, by reading the tree; not run.** `mg-sign` is a real CLI over `crypto/attest` (`sign_artefact`), with a sealed-keystore `KmsSigner` path and an env-gated `FileSigner`; the stub is gone. `crypto/attest/tests/ac_rf_sc_1.rs` has one or more tests per criterion a-f and `just test` runs `-p attest` and `-p mg-sign`. The As-built note matches the code.
> Criteria met 6 of 6 (a, b, c, d, e, f). Confidence: high.
>
> Checked: `tools/mg-sign/src/main.rs:231` · `tools/mg-sign/src/main.rs:502` · `crypto/attest/tests/ac_rf_sc_1.rs:106` · `crypto/attest/tests/ac_rf_sc_1.rs:409` · `justfile:198`

**Evidence.** [`tools/mg-sign/src/main.rs:17`](../../../tools/mg-sign/src/main.rs) prints `mg-sign STUB (Phase 0.b): would ML-DSA-sign …` and returns `ExitCode::SUCCESS`. It has no dependency on `pqc-agility` and no inline tests. It is invoked by `just sign`, by `just gate`, and by the **PQ-sign (ML-DSA)** step of [`.github/workflows/release.yml`](../../../.github/workflows/release.yml). Any artefact — including one that was never signed — passes.

**Prompt**

```
Replace the tools/mg-sign stub with the real signer.

Contract. `mg-sign <artefact-path>` must:
  1. Read the artefact and compute its SHA-256 digest via pqc_agility::sha256.
  2. Locate the sibling CycloneDX SBOM `<artefact>.sbom.cdx.json`. If it is
     absent or does not parse, FAIL CLOSED (non-zero) — never sign an artefact
     whose bill of materials is unknown.
  3. Build the signing payload as the canonical concatenation
     `digest ‖ sha256(sbom_bytes) ‖ sha256(attestation_bytes)` under a
     domain-separation label b"midgard-artefact-v1", so a signature over one
     artefact can never be replayed as a signature over another, and swapping
     the SBOM or the attestation invalidates it.
  4. Obtain an ML-DSA-87 signing key from the KMS/HSM seam. Define that seam as
     a trait `SigningBackend { fn key_id(&self)->&str; fn sign(&self, msg:&[u8])
     -> Result<Signature, SignError>; fn verifying_key(&self)->VerifyingKey; }`
     with two implementations: (a) `KmsSigner`, which talks to services/kms over
     the local trust-root interface, and (b) `FileSigner`, gated behind a
     `--key-file` flag that is REFUSED unless the env var
     MIDGARD_ALLOW_FILE_SIGNING_KEY=1 is set, for local development only. Keys
     are never read from the repo, image, or a default path. [ASVS V14.3]
  5. Emit two files:
       <artefact>.sig  — the detached ML-DSA signature, plus the algorithm id,
                         key id, and the domain label, in a minimal
                         self-describing binary envelope you also define a
                         parser for.
       <artefact>.att  — an in-toto/DSSE attestation (statement type
                         https://in-toto.io/Statement/v1) whose subject is the
                         artefact digest and whose predicate records: source
                         revision, toolchain hash, rustc version, the SBOM
                         digest, hermetic=true, and the reproducibility digests
                         from mg-repro when present.
  6. Exit non-zero on ANY of: missing artefact, missing/invalid SBOM,
     unreachable KMS, unavailable algorithm, write failure. Print the reason to
     stderr. There is no success path that did not produce a valid signature.

Constraints. No external crates: the DSSE/in-toto JSON is rendered by a small
deterministic encoder in-crate (mirror the style of tools/mg-sbom). Sorted keys,
no timestamps, no map iteration order dependence — the .att must be
bit-reproducible. The .sig is NOT byte-reproducible and must not be made so:
ML-DSA hedged signing draws fresh randomness, which resists fault attacks that
deterministic signing (fips204's `try_sign_with_seed`) does not. Nothing depends
on signature byte-stability — mg-repro establishes reproducibility by comparing
binaries, and verification checks that the signature *verifies*. Add unit tests
for the envelope round-trip, the domain separation, the SBOM-missing refusal, and
the file-signing refusal without the env var. Add the crate to `just test`.
```

> **As built.** Landed as `crypto/attest` (the format plus `sign_artefact`
> *and* `verify_artefact`) with `mg-sign` as a thin CLI over it, so RF-SC-2's
> `mg-verify` and the Bifrost/Mimir call sites converge on one implementation —
> see [AC-RF-SC-2d](#rf-sc-2--implement-mg-verify--fail-closed-signature-sbom-and-provenance-verification).
> The KMS/HSM seam is `kms::SigningRoot` (signing happens inside the custodian;
> the key is never returned), with `kms::SoftwareSigningRoot` as its software
> implementation and an ML-KEM-1024-sealed keystore for custody at rest.
> Two deliberate deviations from the prompt above, both recorded in
> `crypto/attest/src/preimage.rs`:
> **(1)** the attestation digest is bound into the preimage, so a swapped `.att`
> is caught by the signature rather than by parsing attacker-controlled JSON —
> which also means `verify_artefact` needs no JSON parser at all;
> **(2)** ML-DSA-87 is the release default while `BIFROST_SIG_ALG` and
> `VELDRIS_MODEL_SIG_ALG` stay at ML-DSA-65, since the release key is the
> highest-value key in the enclosure. RF-SC-2 verifies over the three-digest
> preimage accordingly and drops its separate attestation-consistency step.
>
> RF-SC-2 subsequently moved the domain prefix into `attest::accept`, so the
> release preimage is now `len(label) ‖ label ‖ artefact ‖ sbom ‖ attestation` —
> the same shape every other signing construct in the suite uses.
>
> A later audit against the prompt above closed three gaps:
> **(a)** the SBOM is now *parsed*, by a strict in-crate parser
> (`crypto/attest/src/json.rs`: RFC 8259, no trailing data, no duplicate keys,
> bounded nesting), rather than searched for substrings. `sbom::validate`
> requires a top-level `bomFormat` of `CycloneDX`, a `specVersion`, and a
> non-empty `components` array in which **every** entry carries a purl. Because
> `verify_artefact` shares that function, verification is stricter by the same
> amount;
> **(b)** `sign_artefact` refuses a key whose algorithm is not in
> `APPROVED_RELEASE_ALGS` and verifies its own signature against the signer's
> advertised verifying key before writing anything (`SignError::UnapprovedAlgorithm`,
> `SignError::SelfVerifyFailed`), so no successful run leaves a bundle
> `verify_artefact` would refuse. `mg-sign keygen --alg` accepts only the
> approved algorithms;
> **(c)** `mg-sign --repro` records only rows `mg-repro` marked
> `reproducible: true`, and refuses unless the report contains the digest of the
> artefact being signed. A recorded exception still signs, with a warning, but
> is not attested as reproducible; an unexcepted failure is refused.
>
> Still open from that audit: the `.att` is a bare in-toto Statement v1, not
> wrapped in a DSSE envelope (it is bound through the preimage instead);
> its top-level keys are in fixed rather than sorted order; `hermetic: true` is
> asserted, not established; and the toolchain hash falls back to hashing the
> rustc version string when `rust-toolchain.toml` is not in the working directory.

**Acceptance criteria**

- **AC-RF-SC-1a — Signing produces a verifiable artefact.** *Given* an artefact and a valid sibling SBOM, *when* `mg-sign` runs against a KMS-backed ML-DSA-87 key, *then* `<artefact>.sig` and `<artefact>.att` are written, and `mg-verify` (RF-SC-2) accepts them.
- **AC-RF-SC-1b — Missing SBOM fails closed.** *Given* an artefact with no sibling `.sbom.cdx.json`, *when* `mg-sign` runs, *then* it exits non-zero, writes no `.sig`, and names the missing SBOM in stderr.
- **AC-RF-SC-1c — Unreachable KMS fails closed.** *Given* a KMS that returns an error for every request, *when* `mg-sign` runs, *then* it exits non-zero and no signature file exists on disk afterwards.
- **AC-RF-SC-1d — No ambient key material.** *Given* `--key-file` is passed without `MIDGARD_ALLOW_FILE_SIGNING_KEY=1`, *when* `mg-sign` runs, *then* it exits non-zero. *And* a grep of the crate for filesystem key-path defaults returns nothing.
- **AC-RF-SC-1e — Domain separation holds.** *Given* artefacts A and B and their SBOMs, *when* A's `.sig` is placed alongside B, *then* verification of B fails.
- **AC-RF-SC-1f — Attestation is reproducible.** *Given* the same artefact, SBOM, and source revision, *when* `mg-sign` runs twice, *then* both `.att` files are byte-identical.

**Done when:** all AC-RF-SC-1* pass in `just test`; `just sign` on any workspace binary produces files `mg-verify` accepts; the stub text no longer exists in the tree.

---

### RF-SC-2 · Implement `mg-verify` — fail-closed signature, SBOM and provenance verification

**Severity:** Critical · **Area:** Supply chain · **Depends on:** RF-SC-1 · **Blocks:** RF-B-2, RF-MS-2, RF-DEP-4

> **Status at 2026-10-12 — Done, by reading the tree; not run.** `mg-verify` is a thin CLI over `attest::verify_artefact`, requires explicit trust anchors and prints a one-line verdict. `tools/mg-verify/tests/ac_rf_sc_2.rs` runs the real binary with a rejection table, an untrusted-key test, a verdict test and a source-inspection test for 2d; Bifrost artefact/bundle and Mimir model load all call `attest::accept::accept`. The shared function is `attest::accept`, not `mg_verify::verify` as the criterion text says (the As-built note records this).
> Criteria met 5 of 5 (a, b, c, d, e). Confidence: high.
>
> Checked: `tools/mg-verify/src/main.rs:141` · `tools/mg-verify/tests/ac_rf_sc_2.rs:159` · `tools/mg-verify/tests/ac_rf_sc_2.rs:361` · `bifrost/artefact/src/signed.rs:90` · `mimir/model-server/src/artifact.rs:310`

**Evidence.** [`tools/mg-verify/src/main.rs:16`](../../../tools/mg-verify/src/main.rs) prints `mg-verify STUB (Phase 0.b)` and returns `ExitCode::SUCCESS`. Its own doc comment states the real verifier "returns a non-zero exit on any missing/invalid signature, SBOM, or attestation — never a soft pass" — the code does the opposite. It is the pre-publish gate in `release.yml` **and** the documented pre-deploy check for `AC-B-1` and the model-load check for `AC-MS-2`.

**Prompt**

```
Replace the tools/mg-verify stub with the real verifier, and make it the ONE
implementation of "reject unless proven" that CI, Bifrost pre-deploy, and Mimir
model-load all call.

Contract. `mg-verify <artefact-path> [--trust-anchor <path-or-kms-key-id>]`:
  1. Require all three sidecars: <artefact>.sig, <artefact>.sbom.cdx.json,
     <artefact>.att. Any missing file → non-zero, named in stderr.
  2. Recompute the artefact digest; parse the .sig envelope; reject if the
     algorithm id is not an approved ML-DSA parameter set, if the key id is not
     in the trust anchor set, or if the domain label is wrong.
  3. Verify the ML-DSA signature over the RF-SC-1 preimage — the domain label
     followed by `sha256(artefact) ‖ sha256(sbom) ‖ sha256(attestation)`. Reject
     on failure.
  4. The attestation needs no parsing: its digest is bound into the preimage, so
     a swapped `.att` is caught by step 3 rather than by reading
     attacker-controlled JSON.
  5. Validate the SBOM: well-formed CycloneDX 1.5, at least one component, every
     component carries a purl. Reject otherwise.
  6. Print a one-line machine-readable verdict on stdout
     (`ok artefact=<name> digest=<hex> key=<id> components=<n>`) and exit 0 ONLY
     when every check passed.

Refactor for reuse. Move the verification logic into a library crate
`tools/mg-verify` exposes (`pub fn verify(...) -> Result<Verdict, VerifyError>`)
so bifrost/artefact and mimir/model-server call the same function rather than
re-implementing acceptance. Update those crates to depend on it. Delete any
locally-duplicated verification they currently perform, or make it delegate.

Tests. Table-driven negative tests, one per rejection reason: missing sig,
missing sbom, missing att, tampered artefact byte, tampered SBOM byte, tampered
att subject, wrong key id, unapproved algorithm, wrong domain label, malformed
CycloneDX, empty component list. Each must exit non-zero. One positive test.
```

> **As built.** The decision lives in `attest::accept` — a single
> `accept(domain, body, anchor, signature, approved_algs)` — and `mg-verify` is a
> thin CLI over `attest::verify_artefact`, which calls it. The convergence
> went further than a straight substitution, because the three call sites were
> not duplicating the *file* verifier: they verify their own in-memory manifests
> under their own preimages. All four signing constructs now route through the one
> acceptor, each under its own domain:
>
> | Construct | Domain | Call site |
> |---|---|---|
> | CI release bundle | `midgard-artefact-v1` | `attest::verify_artefact` |
> | Bifrost artefact manifest | `midgard-bifrost-artefact-v1` | `bifrost_artefact::verify` (ingest + pre-deploy) |
> | Bifrost offline-bundle manifest | `midgard-bifrost-bundle-v1` | `bifrost_bundle::import` |
> | Mimir model manifest | `midgard-mimir-model-v1` | `ModelLoader::verify_and_load` |
>
> This **added domain separation the subsystems did not have**: preimages are
> `len(label) ‖ label ‖ body`, so a Bifrost artefact signature can never be
> replayed as a bundle or model-manifest signature even though they share a key
> and a parameter set. It also gave them an approved-algorithm check they lacked.
> Signing and verification moved together, so every existing AC test
> (`AC-B-1`, `AC-B-3`, `AC-B-4`, `AC-MS-2`, `AC-MI-7`, `AC-C-7`) passes unchanged.
>
> `attest` gained a `custody` feature so verifiers do not pull `kms` in
> merely to check a signature: `bifrost-artefact`, `bifrost-bundle` and
> `model-server` depend on it with `default-features = false`.
>
> The `unapproved algorithm` row is covered at library level rather than through
> the CLI: the anchor-file parser refuses a non-approved algorithm outright, so the
> CLI cannot express that state — defence in depth ahead of the acceptor's own
> check, and itself a test.
>
> **Crate location.** The shared crate landed as `crypto/attest` (package
> `attest`), alongside `pqc-agility`, `pq-mtls`, `policy` and `audit-writer` —
> not under `tools/`. It is a cross-cutting primitive that services consume, and
> a service depending on a tools crate would have been backwards.
>
> **Two platform findings surfaced by running the gate for real:**
> **(1)** `cargo run` without `--release` overflows the main thread's stack during
> ML-KEM-1024 / ML-DSA-87 keygen, because the FIPS crates hold large fixed-size
> arrays by value that only an optimised build elides. `mg-sign` and `mg-verify`
> now run on a 64 MiB worker thread so they behave identically in either profile,
> and the gate recipes use `--release`.
> **(2)** The MSVC linker stamps a wall-clock timestamp into the PE header and the
> debug directory and derives the PDB signature GUID from it, so **no Windows
> build was ever bit-reproducible** — measured at 20 differing bytes between two
> otherwise-identical builds. `mg-repro` now passes `-Clink-arg=/Brepro` on MSVC
> targets, which brings it to zero. The broader environment sweep
> (`CARGO_BUILD_JOBS`, `RUSTC_WRAPPER`, `HOSTNAME`, `TZ`, `LC_ALL`, PATH ordering)
> remains RF-SC-3.

**Acceptance criteria**

- **AC-RF-SC-2a — Every tamper is caught.** *Given* a validly signed artefact set, *when* any single byte of the artefact, the SBOM, or the attestation is altered, *then* `mg-verify` exits non-zero for each case independently.
- **AC-RF-SC-2b — Absence is rejection.** *Given* any one of the three sidecars is deleted, *when* `mg-verify` runs, *then* it exits non-zero and names the missing file.
- **AC-RF-SC-2c — Untrusted key is rejection.** *Given* a signature made by a key id absent from the trust anchor set, *when* `mg-verify` runs, *then* it exits non-zero, even though the signature is cryptographically valid.
- **AC-RF-SC-2d — One implementation.** *Given* the Bifrost pre-deploy check (`AC-B-1`) and the Mimir model-load check (`AC-MS-2`), *when* their code paths are inspected, *then* both call `mg_verify::verify` and neither contains an independent acceptance decision.
- **AC-RF-SC-2e — Verdict is machine-readable.** *Given* a passing verification, *when* stdout is parsed, *then* it yields artefact name, digest, key id and component count, and exit code 0.

**Done when:** the negative-test table is complete and green; `bifrost-artefact` and `model-server` no longer contain independent verification logic; `just gate <artefact>` fails on a deliberately unsigned artefact.

---

### RF-SC-3 · Make the reproducibility check non-vacuous in CI

**Severity:** Critical · **Area:** Supply chain · **Depends on:** — · **Blocks:** RF-SC-5

> **Status at 2026-10-12 — Partial.** `mg-repro` no longer short-circuits `workspace`: it always builds twice, compares every artefact, fails on zero artefacts or an uncovered member, writes `repro.json` with both digests, and `mg-sign --repro` embeds it. The exception list is empty. The CI half of the item cannot be confirmed: this tree has no `.github/` directory, so the As-built claims that CI calls it and uploads `repro.json` have no workflow behind them here, and the injected-non-determinism check was a manual run with no test or script step.
> Criteria met 3 of 5 (b, d, e); open: c; not verifiable by reading: a. Confidence: medium.
>
> **Scope change:** The tool side is built, but the As-built note's CI steps (`just repro` in CI, the `repro.json` upload) have nothing behind them: the tree has no CI configuration (RF-CA-4). Criterion c was a manual run with no test.
>
> **Still to do:**
> - Add the CI workflow (or a committed script step) that runs `just repro` over the whole workspace and uploads repro.json; none exists in this tree
> - Add an automated regression fixture that injects non-determinism and asserts mg-repro exits 1 and names the package (AC 3c is only a manual claim)
> - Correct the As-built note: it states CI invokes and uploads, which is not true of the public tree
>
> Checked: `tools/mg-repro/src/main.rs:185` · `tools/mg-repro/src/main.rs:219` · `tools/mg-repro/src/main.rs:629` · `tools/mg-sign/src/main.rs:427` · `supply-chain/repro-exceptions.txt:15`

**Evidence.** [`tools/mg-repro/src/main.rs`](../../../tools/mg-repro/src/main.rs) special-cases the argument `workspace`: it prints "`'workspace' is a placeholder`" and returns `ExitCode::SUCCESS` without building anything. [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml) invokes exactly `nix develop --command just repro workspace`. The `AC-M-6` reproducibility smoke check therefore never builds and never compares. (The two-build comparison logic itself, for a concrete package, is correct.)

> **As built.** The `workspace` short-circuit is gone: `mg-repro` with no
> argument (or `workspace`) now checks the **whole workspace**, and CI calls it
> that way. Rather than 2N builds it does **two**, comparing every emitted
> artefact — binaries *and* library `.rlib`s — so a reproducibility regression in
> a library is caught before it reaches a binary. Measured on this tree:
> **57 artefacts, 57 reproducible, 0 differing.** Verified non-vacuous by injecting
> a build script that stamps a nanosecond timestamp into the binary: the check
> failed, named the artefact and exited 1 (`AC-RF-SC-3c`).
>
> Two platform causes had to be fixed first, both surfaced by running it for real:
> the MSVC linker's PE-header timestamp and PDB GUID made **every Windows build
> non-reproducible** (20 differing bytes — now zero with `-Clink-arg=/Brepro`),
> and `mg-repro` could not find its own artefact because it never resolved the
> platform executable suffix.
>
> Environment normalisation now pins `SOURCE_DATE_EPOCH`, `CARGO_INCREMENTAL`,
> `TZ`, `LC_ALL`, `LANG` and `CARGO_BUILD_JOBS`, and clears `RUSTC_WRAPPER` and
> `RUSTC_WORKSPACE_WRAPPER`. `HOSTNAME` and PATH ordering are deliberately **not**
> normalised — with paths remapped neither is known to reach the output, and
> pinning them would imply a guarantee the tool does not make.
>
> `supply-chain/repro-exceptions.txt` exists and is **empty**, as `AC-RF-SC-3e`
> requires. `repro.json` is emitted, uploaded by CI, and embedded into the
> provenance attestation by `mg-sign`.
>
> **One correctness fix beyond the item.** `repro` proved a *normalised* build
> reproducible while the gate signed a separately built binary from
> `target/release` — so the signed digest was one no third party could reproduce
> from source. `mg-repro --emit <dir>` now hands out the artefact it actually
> proved (re-checking each digest as it lands), and `just gate` SBOMs, signs and
> verifies that one. The signed digest and the reproducibility digest are now the
> same bytes.
>
> **Closed out.** Re-reading the item against the first cut found four
> shortfalls, all now fixed:
>
> * **Coverage was assumed, not enforced.** The tool reported the artefacts it
>   found; it never checked that every workspace *member* produced one, so a
>   member emitting nothing would have passed silently — the exact hole
>   `AC-RF-SC-3b` names. It now enumerates members from the root manifest and
>   each member's own `Cargo.toml` (a directory name is frequently not the
>   package name — `clients/sync` is `midgard-client-sync`), and a package with
>   no comparable artefact fails the run. Measured: **57 artefacts across all 56
>   members.**
> * **`repro.json` recorded one digest, not two.** On a failure the report has to
>   show *how* the builds diverged, not merely that they did. It now carries
>   `digest_run1` and `digest_run2`.
> * **`HOSTNAME` is pinned**, as the item asks. `PATH` ordering remains
>   deliberately un-normalised, with the reason stated in the module doc: pinning
>   it would mean choosing a toolchain layout, and a `PATH` resolving different
>   binaries is a different build rather than a formatting difference. The
>   hermetic Nix shell is what makes `PATH` reproducible for the run that matters.
> * **CI now pins `--jobs 4`** — build parallelism can change codegen-unit
>   partitioning, so a fixed value keeps the two builds comparable and the job
>   inside the time budget.
>
> The coverage-hole failure path is unit-tested rather than exercised against a
> real build: every crate in this workspace emits an artefact, so producing a
> genuine hole would mean adding a target-less member.
>
> Still open: the release matrix is hand-maintained (RF-SC-5).

**Prompt**

```
Make mg-repro cover the workspace, and make CI's invocation meaningful.

1. Replace the `workspace` short-circuit in tools/mg-repro/src/main.rs with real
   behaviour: enumerate every workspace member that produces a binary, run the
   existing two-build bit-comparison for each, and report a per-package table
   plus an aggregate verdict. Any non-reproducible package fails the whole run.
   Support `--jobs N` for parallelism and `--only <pkg>[,<pkg>]` for a subset.
2. Extend the comparison beyond the binary: also hash and compare any emitted
   .rlib/.rmeta for library-only members, so a reproducibility regression in a
   library is caught before it reaches a binary.
3. Record the results as a machine-readable `repro.json` (sorted, timestamp-free)
   listing package → {digest_run1, digest_run2, reproducible}. mg-sign (RF-SC-1)
   embeds this into the DSSE predicate when present.
4. Harden the build environment normalisation: in addition to the existing
   --remap-path-prefix and SOURCE_DATE_EPOCH, clear or pin every variable known
   to leak into output (CARGO_BUILD_JOBS, RUSTC_WRAPPER, HOSTNAME, TZ, LC_ALL,
   PATH ordering). Document each one and why in the module comment.
5. Update .github/workflows/ci.yml to call `just repro` with no argument (meaning
   the whole workspace) and to upload repro.json as a build artefact. Keep the
   job under the CI time budget by using --jobs.

If the workspace proves non-reproducible today, DO NOT weaken the check. Fix the
cause, or list the offending package in an explicit, commented allow-list with a
tracking item — the allow-list must be empty at GA.
```

**Acceptance criteria**

- **AC-RF-SC-3a — No vacuous argument.** *Given* any invocation of `mg-repro`, *when* it exits 0, *then* at least one package was built twice and compared. No argument value produces a success without a comparison.
- **AC-RF-SC-3b — Workspace coverage.** *Given* `mg-repro` with no argument, *when* it runs, *then* every workspace member appears in the report with a reproducible verdict.
- **AC-RF-SC-3c — Regression is caught.** *Given* a deliberately injected non-determinism (e.g. embedding `env!("HOSTNAME")` into a binary), *when* CI runs, *then* the reproducibility job fails and names the package.
- **AC-RF-SC-3d — Report feeds provenance.** *Given* a passing run, *when* `mg-sign` executes afterwards, *then* the DSSE predicate contains the per-package digests from `repro.json`.
- **AC-RF-SC-3e — Allow-list is empty.** *Given* the GA release tag, *when* the allow-list is inspected, *then* it contains no entries.

**Done when:** CI's reproducibility job demonstrably builds; an injected non-determinism turns it red; `repro.json` is an uploaded artefact.

---

### RF-SC-4 · Vulnerability scanning and licence allow-list over the SBOM

**Severity:** Critical · **Area:** Supply chain · **Depends on:** RF-SC-2 · **Blocks:** RF-CMP-2

> **Status at 2026-10-12 — Partial.** `tools/mg-scan` exists with an in-crate SPDX evaluator, CVSS v3.1 scorer, staleness check against `vendor/advisory-db/snapshot.toml` and expiring exception tables in `supply-chain/`; `just gate` runs scan between sbom and sign. Tests are unit-level only (licence refusal, expiry, unscored-blocks); there is no test that drives `run()` for the missing-licence, stale-database or critical-advisory-blocks-the-gate paths, no PR-time CI wiring in this tree, and no OSV data for the JS/TS workspace. The snapshot is dated 2026-08-27, so it is already past the 30-day warn threshold today.
> Criteria met 2 of 6 (b, d); open: a, c, f; not verifiable by reading: e. Confidence: high.
>
> **Scope change:** Steps and criteria written against `ci.yml` / `release.yml` cannot hold until those workflows are published (RF-CA-4); until then target the committed scripts (`tools/ci/*.sh`, `justfile`).
>
> **Still to do:**
> - Add CLI-level tests with fixture SBOM/database: critical advisory blocks, missing licence blocks, stale snapshot fails naming the date
> - Add a gate-level test or script step proving a failed scan leaves no .sig
> - Wire the PR-time `--advisory-only` scan into a CI workflow that exists in the tree
> - Vendor OSV records for the JS/TS workspace and scan the pnpm lockfile
> - Refresh the advisory snapshot (47 days old on 2026-10-12)
>
> Checked: `tools/mg-scan/src/main.rs:244` · `tools/mg-scan/src/main.rs:468` · `tools/mg-scan/src/licence.rs:250` · `justfile:364` · `vendor/advisory-db/snapshot.toml:20`

**Evidence.** [`.github/workflows/release.yml`](../../../.github/workflows/release.yml) step *Scan SBOM (CVE + licence allow-list)* is two `echo "TODO(Phase 1)"` lines. The suite's open-source posture claim (`[FR-MS-6]`, "no non-OSI deps/weights in the OSS core") is unenforced. There is no `deny.toml`, no `cargo-audit` configuration, and no advisory database vendored for offline scanning.

**Prompt**

```
Implement the release-time scan stage as a real, air-gap-capable gate.

1. Vendor an offline advisory source. Add `vendor/advisory-db/` containing a
   pinned snapshot of the RustSec advisory database (and OSV records for the
   JS/TS workspace), with a documented refresh procedure and a recorded snapshot
   date. The scan MUST work with no network; a stale-database warning is emitted
   past a configurable age, and past a hard maximum age the gate FAILS (a
   silently ancient database is worse than none).
2. Build `tools/mg-scan`: reads a CycloneDX SBOM, resolves each purl against the
   vendored advisory data, and emits a sorted, deterministic findings report.
   Fail closed on: any CRITICAL or HIGH advisory with no recorded, expiring
   exception; any component whose licence is not on the allow-list; any
   component with no licence metadata at all.
3. Define the licence allow-list in `supply-chain/licences.toml` — OSI-approved
   identifiers only, each with a one-line justification, plus an `[exceptions]`
   table where every entry carries an owner, a reason, and a mandatory expiry
   date. An expired exception fails the gate.
4. Define the vulnerability exception table in `supply-chain/advisories.toml`
   with the same owner/reason/expiry discipline, and require a link to the
   compensating control.
5. Wire it: replace the release.yml TODO with `just scan <artefact>`, add a
   `scan` recipe to the justfile, and insert `scan` into the `gate` chain
   between `sbom` and `sign` — an artefact that fails the scan must never be
   signed.
6. Also run the scan on every PR (advisory-only, non-blocking for MEDIUM and
   below; blocking for CRITICAL) so exposure is discovered at PR time, not at
   release time.
```

**Acceptance criteria**

- **AC-RF-SC-4a — Critical advisory blocks release.** *Given* a component in the SBOM with a CRITICAL advisory and no exception, *when* `just gate` runs, *then* it fails at the scan stage and the artefact is never signed.
- **AC-RF-SC-4b — Disallowed licence blocks release.** *Given* a component whose licence is absent from `licences.toml`, *when* the scan runs, *then* it fails and names the component, the licence and the purl.
- **AC-RF-SC-4c — Missing licence metadata blocks release.** *Given* a component with no licence field, *when* the scan runs, *then* it fails — unknown is not permitted.
- **AC-RF-SC-4d — Expired exception blocks release.** *Given* an exception whose expiry is in the past, *when* the scan runs, *then* it fails and reports the expiry date and owner.
- **AC-RF-SC-4e — Offline.** *Given* `docker run --network none`, *when* the scan runs against the vendored database, *then* it completes and produces a verdict.
- **AC-RF-SC-4f — Stale database is surfaced.** *Given* an advisory snapshot older than the hard maximum age, *when* the scan runs, *then* it fails with a stale-database error naming the snapshot date.

**Done when:** the scan is in the `gate` chain before `sign`; a deliberately vulnerable or wrongly-licensed dependency turns the gate red offline.

> **As built.** `tools/mg-scan` sits between `sbom` and `sign` in `just gate`, so
> a failing scan can never reach a signature. The real RustSec database is
> vendored at `vendor/advisory-db` — 1206 advisories, 3.9 MB, pinned by commit
> with its upstream commit date — refreshed by `just advisory-db-refresh`, the
> only online step in the gate.
>
> **The SBOM had to grow a licence field first.** `mg-sbom` emitted only name,
> version and purl, so there was nothing for a licence gate to check. It now
> resolves licences from the vendored crates' own manifests and from workspace
> members (following `license.workspace = true`), and emits CycloneDX
> `licenses: [{ expression }]`. All 98 components resolve. A component whose
> licence cannot be resolved emits an **empty array**, never a guess: "we looked
> and found nothing" has to be distinguishable from "nobody looked".
>
> The licence check **evaluates SPDX expressions** rather than matching strings.
> 34 of 98 components are `MIT OR Apache-2.0` or a permutation; treating the
> expression as an opaque token would reject nearly everything and push
> maintainers straight to a blanket exception, which defeats the gate. `OR` needs
> one satisfiable side, `AND` needs all, `WITH` needs a recognised exception
> modifier, and the legacy `MIT/Apache-2.0` slash form is read as `OR`.
>
> Everything is in-crate and offline: the SBOM reader, the SPDX evaluator, a
> semver comparator, a **CVSS v3.1 base-score implementation from the
> specification**, and civil-date arithmetic. No new dependencies.
>
> **A bug this found in itself.** The first cut scored CVSS with `?`, so the 55
> advisories carrying CVSS **4.0** vectors were dropped whole — the database
> loaded 1149 of 1206 and still reported "clean". An unscoreable vector now
> becomes `UNSCORED`, which *blocks*, and `mg-scan` refuses outright if fewer
> advisories load than `snapshot.toml` declares. A parser that silently skips
> what it cannot read is indistinguishable from a clean scan.
>
> Every criterion was exercised against the real database, not a fixture: a
> genuinely vulnerable `abox 0.4.0` (RUSTSEC-2020-0121) blocks, `0.4.1` — the
> patched version — passes, a valid exception downgrades it to a note, and an
> expired one blocks twice over. Stale-database, missing-licence and
> disallowed-licence paths all refuse. The whole scan runs under
> `docker --network none`.
>
> PR-time exposure runs in `ci.yml` via `--advisory-only`: reports everything,
> blocks only on CRITICAL, so an unrelated MEDIUM does not stall a change while a
> critical one still cannot be merged past.
>
> **Known limits, deliberately.** CVSS 4.0 vectors are not scored — the v4
> macrovector lookup is a much larger algorithm — so those advisories escalate to
> `UNSCORED` and block rather than being silently mis-ranked. None currently
> affects this dependency graph. The JS/TS workspace has no OSV records vendored
> yet; that lands with RF-UI-10's dependency gating.

---

### RF-SC-5 · Extend the release gate to every buildable artefact

**Severity:** High · **Area:** Supply chain · **Depends on:** RF-SC-1..4

> **Status at 2026-10-12 — Partial.** Coverage now extends to all 18 workspace binaries via `supply-chain/release-artefacts.txt`, and `tools/ci/release-artefacts.sh --check/--matrix` fails if the list and the workspace's binaries differ and emits the matrix JSON; `mg-register` admits a gated artefact to the Bifrost registry and `deploy/images/build.sh` builds verify-before-run images. The list is still hand-maintained (checked, not derived), only Rust binaries are covered (no UI bundles, npm tarball, client shells, or signed image artefacts), and there is no release workflow in this tree, so dry-run and publish-set comparison do not exist here.
> Criteria met 0 of 5; open: b, c, e; not verifiable by reading: a, d. Confidence: medium.
>
> **Scope change:** The Evidence ("four artefacts in the matrix") is superseded: `supply-chain/release-artefacts.txt` lists 18 binaries and `tools/ci/release-artefacts.sh --check` enforces it. The list is checked, not derived, so criterion a needs rewording; non-Rust artefacts remain ungated.
>
> **Still to do:**
> - Gate non-Rust artefacts (UI bundles, @midgard/sdk tarball, container images, client shells) through repro/SBOM/scan/sign/verify
> - Commit the release workflow with a dry-run mode, or drop the workflow-shaped criteria; `.github/` is absent from this tree
> - Enforce published-set equals gated-set as a failing check
> - Add a test for release-artefacts.sh --check (new binary not listed fails) and for mg-register recording digest, SBOM digest, key id and provenance
> - Decide whether a checked list satisfies 'derived, not listed' and reword AC 5a accordingly
>
> Checked: `supply-chain/release-artefacts.txt:14` · `tools/ci/release-artefacts.sh:63` · `tools/ci/release-artefacts.sh:91` · `tools/mg-register/src/main.rs:26` · `justfile:344`

**Evidence.** [`.github/workflows/release.yml`](../../../.github/workflows/release.yml) matrix lists four artefacts: `tools/mg-sign`, `tools/mg-sbom`, `tools/mg-verify`, `tools/mg-repro`. The workspace has 56 crates, one runnable service (`http-gateway`), seven UI applications, a TypeScript SDK, and (per Workstream P) six client shells — none of which pass the gate. The final step is `echo "TODO(Phase 5): hand the verified bundle to Bifrost"`.

**Prompt**

```
Generalise the release gate so "every buildable artefact" is literally true.

1. Replace the hand-maintained matrix with a generated one: add
   `just release-manifest`, which enumerates every releasable artefact —
   workspace binaries, the container images from RF-DEP-1, each product UI
   bundle, the @midgard/sdk package tarball, each client shell package from
   Workstream P — and emits a JSON matrix consumed by
   `strategy.matrix.include: ${{ fromJson(...) }}`. Adding a new artefact must
   require no workflow edit; forgetting to add one must be impossible because
   the enumeration is derived from the workspace manifests, not a list.
2. Make the gate uniform across artefact kinds. Non-Rust artefacts (UI bundles,
   npm packages, container images) get the SAME five stages: reproducible build,
   SBOM, scan, PQ-sign, verify. For UI bundles this means a deterministic Vite
   build (pinned toolchain, no timestamps in output, sorted asset names, content
   hashes only); for images, a reproducible OCI build with a fixed created-time.
3. Implement the final stage properly: assemble the release bundle
   (artefact + .sbom.cdx.json + .sig + .att + repro.json) and hand it to Bifrost
   via bifrost-artefact's registry API (RF-B-1), which becomes the single place a
   released artefact is recorded.
4. Add a `release-dry-run` workflow_dispatch mode that runs the full gate without
   publishing, so the gate is exercised on every release-affecting PR.
5. Fail the whole release if the manifest and the published set differ — no
   artefact may ship that was not gated, and no gated artefact may be silently
   dropped.
```

**Acceptance criteria**

- **AC-RF-SC-5a — Coverage is derived, not listed.** *Given* a new binary crate added to the workspace, *when* the release runs with no workflow edit, *then* that binary appears in the matrix and is gated.
- **AC-RF-SC-5b — Non-Rust artefacts are gated identically.** *Given* a product UI bundle, *when* the release runs, *then* it has a reproducible build, an SBOM, a scan verdict, an ML-DSA signature and a passing verification.
- **AC-RF-SC-5c — Nothing ships ungated.** *Given* a publish step, *when* the published set is compared to the gated set, *then* they are equal; any difference fails the release.
- **AC-RF-SC-5d — Bundles reach Bifrost.** *Given* a completed gate, *when* the release finishes, *then* each artefact is recorded in the Bifrost artefact registry with its digest, SBOM digest, signature key id and provenance.
- **AC-RF-SC-5e — Dry run.** *Given* `release-dry-run`, *when* dispatched on a branch, *then* the full gate executes and nothing is published.

**Done when:** the matrix is generated; a UI bundle and a container image both carry a valid `.sig`/`.att`; the publish/gate set comparison is enforced.

---

### RF-SC-6 · Implement the provenance-tag lint

**Severity:** Medium · **Area:** Supply chain · **Depends on:** —

> **Status at 2026-10-12 — Partial.** A diff-scoped provenance lint exists as `tools/ci/provenance-lint.sh` (not `mg-trace`), failing on changed `.rs/.ts/.tsx` files without a tag, but it governs only `services mimir products clients`, accepts `[RC-*]` and not `[KDD-*]`, and has no committed exclusion list or tag suggestion. There is no `matrix` mode, no `traceability.md`, no test of the script, and no CI workflow in the tree to run it. The plan's 'Partially delivered' note saying the lint is still an echo is stale.
> Criteria met 0 of 4; open: b, c, d; not verifiable by reading: a. Confidence: high.
>
> **Scope change:** `tools/ci/provenance-lint.sh` exists with a narrower scope. Read this item as: extend that script and add the traceability matrix.
>
> **Still to do:**
> - Extend the governed trees to forge, bifrost, heimdall, admin, crypto, ui, sdk and tools, and accept [KDD-*]
> - Implement the requirement-to-citation matrix (orphan id and invented id both fail) and generate traceability.md
> - Wire the lint into a CI workflow present in the tree and add a fixture test for the untagged-file failure
> - Update the plan note that still calls the lint an echo placeholder
>
> Checked: `tools/ci/provenance-lint.sh:31` · `tools/ci/provenance-lint.sh:39` · `tools/ci/provenance-lint.sh:81` · `justfile:129`

**Evidence.** [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml) *Provenance-tag lint* prints `TODO(Phase 1): fail if a changed source file … lacks a provenance tag. Scaffold placeholder for now.` Traceability is currently maintained by convention alone. It happens to be near-complete today (66 of 67 `FR-*` ids are cited in source) — which makes this cheap to enforce and easy to lose.

**Prompt**

```
Implement the provenance-tag lint as a real CI gate, and make traceability
bidirectional.

Build `tools/mg-trace` with two modes:

  `mg-trace lint --changed <base-ref>`
    For every changed source file under services|mimir|forge|bifrost|heimdall|
    admin|clients|crypto|products|ui|sdk|tools, require at least one provenance
    tag matching [FR-*], [NFR-*], [DD-*], [KDD-*], [AC-*] or [STD:*] in the file.
    Report each offender with a path and a suggested tag based on its directory.
    Exclude generated code via an explicit, committed exclusion list.

  `mg-trace matrix`
    Parse every id defined in docs/dev/markdown/*.md and BUILD_PLAYBOOK.md; parse
    every id cited in source and in deploy/acceptance/verify-*.sh; emit
    docs/dev/fixes/traceability.md — a generated table of
    requirement → citing files → verifying tests → verify-script line.
    Fail if any id defined in the SADs has zero citations (except ids explicitly
    marked -DESIGN, e.g. FR-MS-9-DESIGN), or if any id cited in source does not
    exist in the SADs (catches typos and invented ids).

Wire `lint` into ci.yml replacing the echo, and `matrix` into a scheduled job
that opens a PR when the generated table changes.
```

**Acceptance criteria**

- **AC-RF-SC-6a — Untagged change fails.** *Given* a PR adding a source file with no provenance tag, *when* CI runs, *then* the guards job fails and names the file.
- **AC-RF-SC-6b — Orphan requirement fails.** *Given* a requirement id present in a SAD with no citation in source and no `-DESIGN` suffix, *when* `mg-trace matrix` runs, *then* it fails and names the id.
- **AC-RF-SC-6c — Invented id fails.** *Given* a source file citing `[FR-ZZ-99]`, which appears in no SAD, *when* `mg-trace matrix` runs, *then* it fails.
- **AC-RF-SC-6d — Matrix is generated and current.** *Given* the scheduled job, *when* the traceability table drifts from the tree, *then* a PR is opened with the regenerated table.

**Done when:** the echo is gone; a deliberately untagged file and a deliberately invented id both fail CI.

> **Partially delivered.** Two of the always-on guards this item belongs beside
> now exist and gate CI: `just fmt-check` (`cargo fmt --all --check`) and
> `just lint` (clippy at `-D warnings`). The workspace was rustfmt-normalised —
> it never had been — and the ~12 clippy findings across it were fixed rather
> than allow-listed, with two documented exceptions: a `derivable_impls` on
> `LicencePolicy` kept explicit so the permissive default reads as a decision,
> and `too_many_arguments` in `http-gateway`'s seeded `data.rs`, which RF-API-3
> deletes outright.
>
> Both recipes mirror `just test`'s per-feature groupings. They cannot be a
> single `--workspace` invocation: the crypto crates' `test-backend` and
> `backend-fips` are mutually exclusive behind a `compile_error!`, so one
> workspace-wide clippy run unifies the features and fails to compile. That trap
> is documented in the justfile beside the recipe.
>
> Formatting is pinned by `rustfmt.toml` (defaults only, plus the edition) next
> to the existing `rust-toolchain.toml`, so the verdict is identical in CI and on
> a developer's machine. Without both pins a contributor on a different toolchain
> reformats files they never touched and every diff becomes noise.
>
> The provenance-tag lint itself is still the `echo` placeholder.

---

### RF-SC-7 · Vendored-dependency policy, refresh procedure and integrity check

**Severity:** Medium · **Area:** Supply chain · **Depends on:** RF-SC-4

> **Status at 2026-10-12 — Open.** Nothing asked for exists: no `supply-chain/VENDORING.md`, no `mg-vendor-check`, no `vendor-manifest.toml`, no scheduled update report. `vendor/` is populated, but the `.cargo/config.toml` the Evidence (and README/BUILD_ENV) relies on to redirect cargo to it is not in this tree; the only refresh notes are a few lines in `docs/dev/BUILD_ENV.md`.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** The first task is now RF-CA-3: `.cargo/config.toml`, which redirects cargo to `vendor/`, is not in the public tree.
>
> **Still to do:**
> - Restore or commit `.cargo/config.toml` source replacement so the vendored build is actually selected in this tree
> - Write supply-chain/VENDORING.md and supply-chain/vendor-manifest.toml
> - Build mg-vendor-check (lockfile checksum match, no local modification, no missing or undeclared crate) with tests
> - Run the check in CI and add the scheduled upstream/advisory report
>
> Checked: `docs/dev/BUILD_ENV.md:16` · `docs/dev/BUILD_ENV.md:26` · `deploy/acceptance/run-airgapped.sh:10` · `README.md:119`

**Evidence.** `vendor/` holds the offline dependency graph (including `zeroize`, `fips20x`, `x25519-dalek`, `aes-gcm`) and `.cargo/config.toml` redirects to it, which is what makes the air-gapped build work. There is no documented procedure for refreshing it, no check that the vendored sources match the upstream `Cargo.lock` hashes, and no policy governing what may be added.

**Prompt**

```
Govern the vendor/ directory.

1. Write supply-chain/VENDORING.md: what may be vendored (OSI-licensed, pure
   Rust preferred, no build scripts that execute network calls, no proc-macro
   without review), who approves an addition, how to refresh, and how to record
   the upstream provenance (source URL, git rev or crates.io checksum, date,
   reviewer).
2. Add `tools/mg-vendor-check`: verifies that every directory in vendor/
   corresponds to a package in Cargo.lock, that its checksum matches the
   lockfile, that no vendored crate has been locally modified (compare against
   the recorded checksum), and that no package in Cargo.lock is missing from
   vendor/. Fail closed on any mismatch. Run it in ci.yml.
3. Record per-crate provenance in supply-chain/vendor-manifest.toml, one entry
   per vendored package, and make mg-vendor-check enforce that every vendored
   directory has an entry.
4. Add a scheduled job that reports (does not auto-apply) upstream updates and
   any new advisories affecting vendored crates.
```

**Acceptance criteria**

- **AC-RF-SC-7a — Local modification detected.** *Given* a byte changed inside a vendored crate, *when* CI runs, *then* `mg-vendor-check` fails and names the file.
- **AC-RF-SC-7b — Undeclared vendor entry fails.** *Given* a directory in `vendor/` with no manifest entry, *when* the check runs, *then* it fails.
- **AC-RF-SC-7c — Lockfile drift fails.** *Given* a package in `Cargo.lock` absent from `vendor/`, *when* the check runs, *then* it fails — the offline build cannot be silently broken.
- **AC-RF-SC-7d — Policy is documented and applied.** *Given* a PR adding a vendored crate, *when* review occurs, *then* `VENDORING.md`'s checklist is satisfiable from the manifest entry alone.

**Done when:** `mg-vendor-check` is in CI; a tampered vendored byte turns it red.

---

### RF-SC-8 · Key management for release signing: rotation, revocation, trust distribution

**Severity:** High · **Area:** Supply chain · **Depends on:** RF-SC-1, RF-SC-2

> **Status at 2026-10-12 — Partial.** Verification fails closed without anchors and against an unpinned key, and `docs/ops/PKI.md` plus `deploy/trust/README.md` describe a ceremony and append-a-line rotation for flat `<alg> <key-id> <hex>` anchor files. There is no `supply-chain/KEYS.md`, no SLH-DSA root, no signed versioned `trust-anchors.json`, no `mg-keys`, no validity windows or revocation in `attest`, no Bifrost quarantine on revocation, and the pinned `.pub` files are not committed.
> Criteria met 1 of 5 (a); open: b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Define the root/release key hierarchy (KEYS.md) and implement a root-signed, versioned trust bundle that mg-verify consumes
> - Implement rotation with overlapping validity and a test that K1-signed artefacts still verify after K2 becomes current
> - Implement revocation in the acceptor plus Bifrost quarantine flagging, with tests
> - Carry the trust bundle through the Bifrost bundle path for offline anchor update, verified against the previous root
> - Audit ceremonies, rotations and revocations in the governance ledger
>
> Checked: `tools/mg-verify/src/main.rs:132` · `tools/mg-verify/tests/ac_rf_sc_2.rs:301` · `deploy/trust/README.md:32` · `docs/ops/PKI.md:130` · `tools/ci/release-preflight.sh:91`

**Evidence.** `release.yml` targets a `[self-hosted, kms]` runner and uses OIDC for a signing identity, but there is no defined key hierarchy, no rotation procedure, no revocation mechanism, and no way for an air-gapped site to learn the current trust anchor set. `mg-verify` (RF-SC-2) needs a trust anchor input that today has no source.

**Prompt**

```
Define and implement release key management end to end.

1. Specify the hierarchy in supply-chain/KEYS.md: an offline root (SLH-DSA,
   long-lived, air-gapped, split custody) that signs a set of online release
   keys (ML-DSA-87, short-lived, KMS/HSM-resident, one per artefact class), with
   documented ceremony, custody, and audit requirements for each.
2. Implement the trust bundle: a signed, versioned `trust-anchors.json`
   enumerating the currently valid release key ids, their algorithms, validity
   windows, and revocation status, signed by the root. mg-verify consumes it;
   the absence of a trust bundle is a verification failure, not a bypass.
3. Implement rotation: `mg-keys rotate` issues a new release key, signs it with
   the root, publishes an updated trust bundle, and overlaps validity windows so
   in-flight artefacts remain verifiable. Old artefacts signed by a rotated (not
   revoked) key MUST remain verifiable.
4. Implement revocation: a revoked key id causes mg-verify to reject every
   signature under it, including previously-accepted artefacts, and Bifrost to
   quarantine any deployed artefact signed by it (feeding RF-B-5).
5. Air-gap distribution: the trust bundle transits as a Bifrost signed bundle
   (AC-B-3 path), so a disconnected site can update its anchors offline.
6. Record every key ceremony, rotation, and revocation in the PQ-signed audit
   ledger via services/governance.
```

**Acceptance criteria**

- **AC-RF-SC-8a — No anchors, no verification.** *Given* a verification with no trust bundle available, *when* `mg-verify` runs, *then* it fails closed.
- **AC-RF-SC-8b — Rotation preserves history.** *Given* an artefact signed by key K1, *when* K1 is rotated out and K2 becomes current, *then* the old artefact still verifies and new artefacts verify under K2.
- **AC-RF-SC-8c — Revocation is retroactive.** *Given* K1 is revoked, *when* an artefact signed by K1 is verified, *then* verification fails, and *when* Bifrost inventories the fleet, *then* deployments carrying K1-signed artefacts are flagged for quarantine.
- **AC-RF-SC-8d — Offline anchor update.** *Given* an air-gapped site, *when* a signed trust bundle transits by removable media through the Bifrost bundle path, *then* the site's anchors update and the bundle's own signature is verified against the previous root before acceptance.
- **AC-RF-SC-8e — Ceremonies are audited.** *Given* any key ceremony, rotation or revocation, *when* the audit ledger is queried, *then* a PQ-signed entry exists naming the operator, the key ids, and the dual-control approver.

**Done when:** `KEYS.md` is written and reviewed; rotation and revocation are exercised in tests; an air-gapped anchor update is demonstrated.

---

# B — Runtime & process model

> **Why second.** 51 of 56 crates are libraries with no process. There is no way to *run* the enclosure — only to test it. Everything downstream (persistence, transport, observability, deployment) needs a process to live in, a configuration to read, and a lifecycle to hook.

---

### RF-RT-1 · Give every service a process: binary, configuration, lifecycle

**Severity:** Critical · **Area:** Runtime · **Depends on:** — · **Blocks:** RF-PER-*, RF-API-*, RF-OBS-*, RF-DEP-*

> **Status at 2026-10-12 — Partial.** `services/runtime` exists and five service binaries run on it (http-gateway, mimir-server, bifrost-fleet, heimdall-feed, forge-worker), with strict CLI args, a file-loaded mesh identity that refuses to load unless it verifies, and a separate `/healthz` `/readyz` `/metrics` port. There is no layered config loader or `--check-config`, no lifecycle state machine or signal-driven drain (`serve_mesh` loops forever), no panic boundary, no binaries for iam/fabric/kms/governance/scheduler/admin as separate processes, no generated config docs, and no clippy lint against unwrap/expect.
> Criteria met 1 of 7 (e); open: b, d, f, g; not verifiable by reading: a, c. Confidence: high.
>
> **Scope change:** Five server binaries run on `services/runtime`; iam, fabric, kms and governance are composed inside `http-gateway`. Decide whether those four stay embedded or become processes before working the "one binary per service" list.
>
> **Still to do:**
> - Add typed, validated, layered configuration with `--check-config` and structural secret redaction
> - Add lifecycle states with SIGTERM bounded drain and SIGINT stop to serve_mesh and the HTTP edge
> - Decide whether iam/fabric/kms/governance/scheduler/admin become separate processes or stay embedded in http-gateway, and rewrite the binary list to match
> - Add a panic boundary on request paths and fail-fast for background tasks
> - Complete the unwrap/expect/panic audit and add the denying clippy lint
>
> Checked: `services/runtime/src/lib.rs:20` · `services/runtime/src/serve.rs:14` · `services/runtime/src/args.rs:17` · `services/runtime/src/identity.rs:33` · `services/runtime/tests/identity.rs:89`

**Evidence.** `find . -name main.rs` returns five files: `services/http-gateway` and the four `tools/*`. `services/iam`, `services/fabric`, `services/kms`, `services/governance`, `services/scheduler`, `services/gateway`, every `mimir/*`, `forge/*`, `bifrost/*`, `heimdall/*` and `admin/*` crate is a library only. There is no configuration mechanism anywhere in the tree — no config file format, no environment schema, no defaults, no validation.

**Prompt**

```
Turn the service libraries into runnable services without changing their
library-first design.

1. Add a shared crate `services/runtime` providing the process harness every
   service binary uses:
     - Config: a layered loader (defaults → config file → environment →
       command-line flags), with a typed, VALIDATED schema per service and a
       `--check-config` mode that validates and exits. Unknown keys are an
       error, not a warning. Secrets are never logged, and the config dump
       redacts them structurally (by type, not by name matching).
     - Lifecycle: start → ready → serving → draining → stopped, with SIGTERM
       triggering a bounded drain (finish in-flight work, refuse new work) and
       SIGINT an immediate stop. Publish the state transitions as events.
     - Health: liveness (process is not wedged) and readiness (dependencies
       satisfied, PDP reachable, storage open, trust anchors loaded) as distinct
       checks. Readiness must be false until every dependency is genuinely
       usable — never optimistic.
     - Panic policy: a panic in a request path is caught at the boundary,
       audited, and returned as a fail-closed error; a panic in a background
       task terminates the process rather than leaving it half-alive.
     - Identity: obtain the workload identity certificate from services/iam at
       startup and fail to start without it. [NFR-SEC-1]
2. Add a binary target for each service: iam, fabric, kms, governance,
   scheduler, gateway, model-server, agent-runtime, staging, forge-engine (as
   the Forge service host), bifrost controller, heimdall fusion host, admin
   backend. Each is a thin main() that loads config, builds the library types,
   and runs the harness — all logic stays in the library so tests are unchanged.
3. Define each service's configuration surface in
   `docs/dev/operations/config/<service>.md`, generated from the typed schema so
   it cannot drift.
4. Audit the 41 `unwrap()`/`expect()`/`panic!` sites currently in non-test src/
   across services|mimir|forge|bifrost|heimdall|admin|clients|crypto. Each is
   either (a) provably infallible with a comment proving it, or (b) converted to
   a typed error. Add a clippy lint denying new ones in request paths.
```

**Acceptance criteria**

- **AC-RF-RT-1a — Every service runs.** *Given* a valid configuration, *when* each service binary is started, *then* it reaches `serving`, answers liveness and readiness, and logs its identity and version.
- **AC-RF-RT-1b — Invalid config refuses to start.** *Given* a config with an unknown key, a missing required key, or a value failing validation, *when* the service starts, *then* it exits non-zero with a message naming the key, and `--check-config` reproduces the same verdict without binding a port.
- **AC-RF-RT-1c — Readiness is honest.** *Given* an unreachable dependency (PDP, storage, IdP), *when* readiness is probed, *then* it reports not-ready and the service accepts no work.
- **AC-RF-RT-1d — Graceful drain.** *Given* in-flight requests, *when* SIGTERM is delivered, *then* new requests are refused, in-flight requests complete within the drain budget, and the process exits 0. *And* requests still in flight at the end of the budget are terminated with a fail-closed error, not silently dropped.
- **AC-RF-RT-1e — No identity, no start.** *Given* an IdP that will not issue a workload certificate, *when* a service starts, *then* it fails to start.
- **AC-RF-RT-1f — Secrets never surface.** *Given* a config containing key material or tokens, *when* the config is dumped or an error is logged, *then* no secret value appears in the output.
- **AC-RF-RT-1g — Panic hygiene.** *Given* the non-test source tree, *when* it is scanned, *then* every remaining `unwrap`/`expect`/`panic!` carries a comment proving infallibility, and the clippy lint denies new unannotated ones.

**Done when:** every service has a binary; `--check-config` works for all; drain and readiness behaviours are covered by tests; the panic audit is complete.

---

### RF-RT-2 · Concurrency and asynchrony model

**Severity:** High · **Area:** Runtime · **Depends on:** RF-RT-1

> **Status at 2026-10-12 — Partial.** Still thread-per-connection with no ADR. The mesh `gateway` has an opt-in per-identity token-bucket limiter and per-connection request quota, and the health port has a 5 s read timeout and 8 KiB head cap. The browser edge parser in `services/http-gateway/src/http.rs` is unchanged in kind: no connection limit, no deadlines, no header count/size cap, allocates `Content-Length` bytes unbounded, and does not reject Transfer-Encoding conflicts or bare LF; no fuzz target exists.
> Criteria met 0 of 5; open: a, b, c, d; not verifiable by reading: e. Confidence: high.
>
> **Still to do:**
> - Write the concurrency ADR and record the decision
> - Add bounded connection limits, per-peer caps and read/write/total deadlines in services/runtime and apply them to serve_mesh and http-gateway
> - Harden the HTTP/1.1 parser: header and body limits, CL/TE conflict, obs-fold and bare-LF rejection, path normalisation, with tests
> - Add a parser fuzz target and a nightly job
> - Load-test and publish the envelope
>
> Checked: `services/http-gateway/src/http.rs:46` · `services/http-gateway/src/http.rs:93` · `services/gateway/src/lib.rs:167` · `services/runtime/src/health_http.rs:45` · `services/runtime/src/serve.rs:30`

**Evidence.** `services/gateway` and `services/http-gateway` both use thread-per-connection `TcpListener` with a hand-rolled HTTP/1.1 parser, deliberately (the module docs state "No async runtime and no external HTTP framework"). Only 4 files in the entire non-test tree use `Mutex`/`RwLock`. That posture is defensible for a reference implementation and untenable at fleet scale: thread-per-connection caps concurrency at the thread limit and offers no backpressure, no timeouts, and no connection limits.

**Prompt**

```
Decide and implement the concurrency model, explicitly and once.

1. Write an ADR at docs/dev/adr/0001-concurrency-model.md evaluating three
   options against the sovereignty constraints (vendored deps, offline builds,
   reproducibility, auditability of the dependency graph):
     (a) keep thread-per-connection, add a bounded pool + backpressure;
     (b) adopt a vendored async runtime for I/O-bound services only;
     (c) hybrid — threads for the PQ-mTLS data plane, async for the HTTP edge.
   Record the decision, the rejected alternatives, and the consequences for the
   dependency graph and the SBOM.
2. Whichever is chosen, implement uniformly in services/runtime:
     - a bounded connection limit with an explicit reject-when-full behaviour
       (fail-closed with a retry-after, never unbounded queueing);
     - read, write, and total-request deadlines on every connection;
     - a bounded work queue per service with a documented queue-full policy;
     - per-peer connection caps to blunt a single misbehaving client;
     - shed-load metrics feeding RF-OBS-1.
3. Replace the hand-rolled HTTP/1.1 parser with a hardened one: enforce maximum
   header count and size, maximum body size, reject smuggling vectors
   (conflicting Content-Length/Transfer-Encoding, obs-fold, bare LF), and
   normalise paths before routing. Fuzz it (RF-TST-4).
4. Load-test the result and publish the measured envelope (RF-SCL-4).
```

**Acceptance criteria**

- **AC-RF-RT-2a — ADR exists and is applied.** *Given* the implemented model, *when* it is compared with the ADR, *then* they agree, and any divergence is a superseding ADR.
- **AC-RF-RT-2b — Bounded and honest under overload.** *Given* offered load beyond the configured limit, *when* it is applied, *then* excess connections are rejected promptly with a retry signal, memory stays bounded, and already-accepted requests still meet their deadlines.
- **AC-RF-RT-2c — Deadlines are enforced.** *Given* a client that opens a connection and sends nothing, *when* the read deadline elapses, *then* the connection is closed and the slot released.
- **AC-RF-RT-2d — Smuggling is rejected.** *Given* a request with both `Content-Length` and `Transfer-Encoding`, or a bare-LF header terminator, or an oversized header block, *when* it reaches the edge, *then* it is rejected with a 400 and no request is forwarded.
- **AC-RF-RT-2e — Parser survives fuzzing.** *Given* 24 hours of coverage-guided fuzzing against the HTTP parser, *when* the campaign ends, *then* no panic, hang, or unbounded allocation is found.

**Done when:** the ADR is merged; limits and deadlines are configurable and tested; the parser fuzz target is in CI's nightly job.

---

### RF-RT-3 · Structured, correlated, redacting logging

**Severity:** High · **Area:** Runtime · **Depends on:** RF-RT-1 · **Blocks:** RF-OBS-3

> **Status at 2026-10-12 — Partial.** A structured logger exists in `services/telemetry` (levels, JSON-lines stdout sink, buffer sink, optional trace/span ids, W3C traceparent propagation tested) and `serve_mesh` uses it. It lives in `telemetry`, not `runtime::log`; records are not shown to carry version or a request id, there is no `Secret<T>`/`Marked<T>` logging wrapper beyond `pqc_agility` key types with redacting Debug, no file or forwarding sink, and service mains and `http-gateway/src/lib.rs` still use `eprintln!` (33 print macros across 11 non-test files).
> Criteria met 0 of 5; open: b, d, e; not verifiable by reading: a, c. Confidence: medium.
>
> **Scope change:** The logger exists in `services/telemetry`, not `runtime::log`. Read this item as adoption across services, request ids and redaction types.
>
> **Still to do:**
> - Replace remaining eprintln!/println! diagnostics in service code with the logger and add a lint enforcing it
> - Assign and propagate a request id across HTTP, PQ-mTLS hops, audit records and agent-runtime steps, with a four-hop test
> - Add Secret<T>/Marked<T> wrappers with clearance-aware sinks, including a compile-fail test
> - Add service version to every record, plus file-rotation and opt-in forwarding sinks
> - Retarget the item at services/telemetry rather than a new runtime::log module
>
> Checked: `services/telemetry/src/log.rs:132` · `services/telemetry/tests/telemetry.rs:119` · `services/runtime/src/serve.rs:31` · `services/http-gateway/src/lib.rs:770` · `crypto/pqc-agility/src/secret.rs:111`

**Evidence.** Diagnostics across the tree are `eprintln!` (e.g. `services/http-gateway/src/lib.rs` connection and accept errors). There is no log level, no structure, no correlation id, no redaction, and no way to route logs anywhere.

**Prompt**

```
Introduce structured logging as a first-class, governed output.

1. Add `services/runtime::log`: levelled (error/warn/info/debug/trace), emitting
   deterministic, sorted-key structured records. Provide a human-readable format
   for consoles and a machine format for collectors. No external crate unless
   the RF-RT-2 ADR admits one; if not, implement a minimal encoder in-crate.
2. Correlation. Every inbound request is assigned or adopts a request id; the id
   propagates across service hops (PQ-mTLS metadata and HTTP header), into audit
   records, and into agent-runtime loop steps so an AI action can be traced from
   the operator click to the model call to the mutation.
3. Redaction is structural, not textual. Wrap sensitive values in types
   (`Secret<T>`, `Marked<T>`) whose Display/Debug print a placeholder. Logging a
   marked value above the log sink's clearance is a compile-time impossibility
   for `Secret<T>` and a runtime redaction for `Marked<T>`. Never rely on
   pattern-matching field names.
4. Replace every eprintln!/println! diagnostic in service and library code with
   the logger. Keep stdout clean for machine-readable tool output (mg-verify's
   verdict line, mg-repro's report).
5. Route logs: stderr by default; an optional file sink with size-based
   rotation; and a forwarding sink for RF-OBS-3. Log sinks obey the same
   default-deny egress policy as everything else — a remote sink is opt-in
   configuration, never a default.
```

**Acceptance criteria**

- **AC-RF-RT-3a — Structure and levels.** *Given* any service, *when* it logs, *then* every record carries timestamp, level, service, version, request id (where applicable), and sorted structured fields.
- **AC-RF-RT-3b — Correlation spans hops.** *Given* a console action that traverses edge → fabric → governance → agent-runtime, *when* logs are filtered by the request id, *then* records from every hop appear, and the audit entry for the action carries the same id.
- **AC-RF-RT-3c — Secrets cannot be logged.** *Given* a `Secret<T>`, *when* it is formatted in any log statement, *then* the output is a placeholder. *And* a test attempting to log raw key material fails to compile.
- **AC-RF-RT-3d — Marked data is redacted by clearance.** *Given* a `Marked<T>` above the sink's clearance, *when* it is logged, *then* the value is redacted and the redaction itself is recorded.
- **AC-RF-RT-3e — No stray printing.** *Given* the service and library source tree, *when* it is scanned, *then* no `eprintln!`/`println!` remains outside tool stdout contracts and tests, enforced by a clippy lint.

**Done when:** the logger is in use everywhere; correlation is demonstrated across four hops; the redaction tests pass.

---

### RF-RT-4 · Time, clocks and determinism

**Severity:** Medium · **Area:** Runtime · **Depends on:** RF-RT-1

> **Status at 2026-10-12 — Open.** No platform `Clock` seam exists: `services/runtime` exposes only `unix_seconds()` over `SystemTime::now`, and the one `Clock` trait in the tree is local to `mimir/model-server`. Most domain logic takes an injected `now: u64` (for example break-glass grants), which gives deterministic expiry tests, but there is no monotonic guard, no backward-jump refusal, no untrusted-clock state, no anomaly auditing and no air-gap time policy.
> Criteria met 0 of 5; open: a, c, d, e; not verifiable by reading: b. Confidence: medium.
>
> **Still to do:**
> - Add Clock, TestClock and MonotonicGuardClock to services/runtime and route the 13 direct OS-clock call sites through it
> - Document, per time-dependent security decision, monotonic versus wall-clock use
> - Implement untrusted-clock state that refuses token issue and time-boxed grants, with audit of anomalies
> - Define the air-gapped trusted time source and skew detection
>
> Checked: `services/runtime/src/lib.rs:39` · `mimir/model-server/src/handler.rs:26` · `mimir/mimir-server/src/main.rs:65` · `admin/breakglass/tests/durable_restart.rs:84`

**Evidence.** The reference implementation is deliberately clock-free (`forge/engine/src/reference.rs`: "No threads, no I/O, no clock"), and `audit-writer` takes an injected `Timestamp`. Real services need time — token expiry, break-glass windows, time-boxed grants, retention, rotation, rate limits — and an air-gapped enclosure has no NTP. There is currently no time source abstraction and no policy for what happens when the clock is wrong.

**Prompt**

```
Define the platform's relationship with time.

1. Add `services/runtime::clock` with a `Clock` trait (`now_utc`,
   `monotonic_elapsed`) and three implementations: SystemClock, a TestClock with
   manual advance, and a MonotonicGuardClock that detects and refuses backward
   wall-clock jumps beyond a tolerance. All time-dependent logic takes an
   injected Clock — never calls the OS directly — so every expiry, window and
   retention rule is testable deterministically.
2. Enumerate every time-dependent security decision (token expiry, session
   lifetime, break-glass window, time-boxed privilege grant, signature validity
   window, key rotation, retention/legal hold, rate-limit windows, replay
   protection) and make each one use monotonic time where the concern is a
   duration, and wall-clock time only where the concern is a calendar instant.
   Document the choice per decision.
3. Air-gapped time: define the trusted time source (hardware RTC, an operator-
   attested time injection, or a local stratum-1 source), how skew is detected,
   and what the system does when time is untrustworthy. The answer must be
   fail-closed for security decisions — an enclosure that cannot trust its clock
   refuses to issue tokens or approve time-boxed grants rather than guessing.
4. Record clock anomalies (backward jump, excessive skew, untrusted state) in
   the audit ledger and surface them in the Admin console (RF-AD-6).
```

**Acceptance criteria**

- **AC-RF-RT-4a — All time is injected.** *Given* the source tree, *when* scanned, *then* no security-relevant decision calls the OS clock directly; all take a `Clock`.
- **AC-RF-RT-4b — Expiry is deterministic under test.** *Given* a `TestClock`, *when* it is advanced past a token's expiry, *then* the token is rejected without any real waiting.
- **AC-RF-RT-4c — Backward jump is refused.** *Given* a wall-clock jump backwards beyond tolerance, *when* a time-boxed grant is evaluated, *then* the decision fails closed and an anomaly is audited.
- **AC-RF-RT-4d — Durations are monotonic.** *Given* a wall-clock adjustment during an active break-glass window, *when* the window is evaluated, *then* its duration is unaffected.
- **AC-RF-RT-4e — Untrusted clock stops issuance.** *Given* the clock is marked untrusted, *when* a token issue or a time-boxed grant is requested, *then* it is refused with a distinguishable error and audited.

**Done when:** the `Clock` seam is universal; the anomaly path is tested and visible in the Admin console.

---

### RF-RT-5 · Randomness, entropy and nonce discipline

**Severity:** High · **Area:** Runtime · **Depends on:** RF-RT-1

> **Status at 2026-10-12 — Partial.** `pqc-agility` now has two compile guards (test-backend with backend-fips, and test-backend in any release profile) and `OsRng::try_new`/`try_fill` return errors on entropy failure, with tests in `rc_51.rs`. The pq-mtls record layer uses a sequence-number nonce with `checked_add` and fails on exhaustion. Nothing calls the entropy probe at service start-up or gates readiness on it, the `just check-backends` gate named in the code comment does not exist in the justfile, there is no release-artefact symbol scan, no `nonce-policy.md`, KMS envelope nonces are random 96-bit with no per-key counter or limit, and there are no long-run nonce-uniqueness or limit tests.
> Criteria met 0 of 5; open: a, c, d, e; not verifiable by reading: b. Confidence: medium.
>
> **Still to do:**
> - Call OsRng::try_new in the runtime start-up path and gate readiness on it, with a test using a failing RNG
> - Add the `check-backends` recipe the code cites and a release-artefact scan for the deterministic RNG
> - Write the nonce policy and state the bound for random-nonce KMS sealing (or add counters and rekey triggers)
> - Add nonce non-repetition and sequence-limit tests for pq-mtls, KMS and the client store
>
> Checked: `crypto/pqc-agility/src/backend/mod.rs:22` · `crypto/pqc-agility/src/rng.rs:60` · `crypto/pqc-agility/tests/rc_51.rs:22` · `crypto/pq-mtls/src/record.rs:62` · `services/kms/src/lib.rs:239`

**Evidence.** `pqc-agility` correctly injects `&mut dyn Rng` and offers `OsRng` (getrandom) under `backend-fips` and a deterministic `SeedRng` under `test-backend`. There is no start-up entropy health check, no policy preventing a test RNG reaching a production build path beyond feature gating, and no documented nonce-uniqueness argument for the AES-GCM usage in `kms` and `pq-mtls`.

**Prompt**

```
Harden randomness end to end.

1. Start-up entropy check in services/runtime: verify the OS CSPRNG is seeded
   and healthy before the service reports ready; fail to start otherwise. On
   platforms where this is detectable (getrandom blocking behaviour, /dev/urandom
   entropy availability, TPM RNG presence) use the platform mechanism; document
   the check per platform.
2. Build-time guard: make it a compile error for any production binary to link
   the `test-backend` feature, and add a CI check that inspects release
   artefacts for the deterministic RNG symbol. The existing mutual-exclusion
   compile_error is necessary but not sufficient — a binary must be provably
   built without it.
3. Nonce discipline: write docs/dev/design/nonce-policy.md proving uniqueness for
   every AES-GCM use (KMS envelope sealing, pq-mtls record protection, client
   store sealing). For each, state the nonce construction, the key lifetime, the
   maximum messages per key, and the rekey trigger. Implement the message
   counters and rekey triggers where they do not exist, and make exceeding the
   limit a hard failure rather than a wrap.
4. Add tests asserting nonce non-repetition across a long run for each AEAD user,
   and a test that the counter limit triggers rekey rather than reuse.
```

**Acceptance criteria**

- **AC-RF-RT-5a — Unhealthy entropy blocks start.** *Given* an unseeded CSPRNG, *when* a service starts, *then* it does not reach ready and reports the entropy failure.
- **AC-RF-RT-5b — No test RNG in release.** *Given* a release artefact, *when* it is inspected in CI, *then* the deterministic RNG is absent, and *when* a build attempts to enable `test-backend` for a release binary, *then* it fails to compile.
- **AC-RF-RT-5c — Nonces never repeat.** *Given* an extended run of each AEAD consumer, *when* emitted nonces are collected, *then* no value repeats under a given key.
- **AC-RF-RT-5d — Limit triggers rekey.** *Given* a key that reaches its documented message limit, *when* the next seal is attempted, *then* a rekey occurs (or the operation fails closed) — never a nonce wrap.
- **AC-RF-RT-5e — Policy is written and matches code.** *Given* `nonce-policy.md`, *when* compared with each implementation, *then* the stated construction, limits and triggers match.

**Done when:** the entropy check gates readiness; the release-artefact scan is in CI; the nonce policy is written and enforced by tests.

---

### RF-RT-6 · Resource governance: memory, file descriptors, CPU

**Severity:** Medium · **Area:** Runtime · **Depends on:** RF-RT-2

> **Status at 2026-10-12 — Open.** No resource inventory document, no configurable bounds or eviction/spill policy, no fd or thread accounting, no memory-pressure handling and no soak test were found. Durable stores (FileLedger, SealedJournal, DurableObjectStore) still load and hold every record in memory, so persistence has not bounded resident memory; a few fixed caps exist at the HTTP edge (challenge table, search-around node cap).
> Criteria met 0 of 4; open: a, b, d; not verifiable by reading: c. Confidence: medium.
>
> **Still to do:**
> - Write the resource inventory of growable structures
> - Give each structure a configured bound and policy; make ledgers and journals page from disk rather than hold all entries in memory
> - Add thread/fd accounting and limits to the runtime (thread-per-connection is currently unbounded)
> - Define and implement high-water shed behaviour
> - Add a 24-hour soak job
>
> Checked: `crypto/audit-writer/src/file.rs:60` · `services/fabric/src/durable.rs:32` · `services/http-gateway/src/admin_api.rs:42` · `services/runtime/src/serve.rs:30`

**Evidence.** No crate imposes any bound on collection growth. `services/fabric`'s object store, `audit-writer`'s `InMemoryLedger`, `heimdall/picture`'s layers and `mimir/model-server`'s KV-cache accounting all grow with input. Combined with the absence of persistence (RF-PER-1), a long-running enclosure has an unbounded memory profile and an OOM as its only limit.

**Prompt**

```
Give every unbounded structure a bound and a policy.

1. Inventory every growable in-memory structure across the workspace (stores,
   caches, queues, ledgers, indexes, session tables, picture layers, KV caches)
   and record it in docs/dev/operations/resource-inventory.md with: what bounds
   it today (usually nothing), what should bound it, and the eviction or
   spill-to-disk policy.
2. Implement bounds: configurable maxima with an explicit policy per structure —
   reject (fail closed), evict (LRU/LFU with the eviction audited where it is
   security-relevant), or spill (to the persistent tier from RF-PER-1). A cache
   may evict; a ledger may NEVER evict — it spills or refuses.
3. Add file-descriptor and thread accounting to the runtime harness, with limits
   and a metric per RF-OBS-1.
4. Add memory pressure handling: a documented high-water behaviour (shed load,
   refuse new sessions, drop optional caches first) rather than an OOM kill.
5. Add long-running soak tests (RF-TST-6) that assert a flat memory profile over
   24 hours of representative load.
```

**Acceptance criteria**

- **AC-RF-RT-6a — Every structure is bounded.** *Given* the resource inventory, *when* each entry is checked against code, *then* every structure has a configured bound and a policy.
- **AC-RF-RT-6b — Ledgers never lose entries.** *Given* an audit ledger at its in-memory bound, *when* another record arrives, *then* it is persisted or the write fails closed — no record is evicted or dropped.
- **AC-RF-RT-6c — Soak stability.** *Given* 24 hours of representative load, *when* the soak completes, *then* resident memory is flat within a stated tolerance and file descriptors do not grow monotonically.
- **AC-RF-RT-6d — Pressure is graceful.** *Given* memory pressure, *when* the high-water mark is crossed, *then* the documented shed behaviour occurs and is logged and metered — never an unhandled OOM.

**Done when:** the inventory is complete and every entry bounded; the 24-hour soak is green in the nightly pipeline.

---

### RF-RT-7 · Feature flags, safe defaults and a documented hardening baseline

**Severity:** Medium · **Area:** Runtime · **Depends on:** RF-RT-1

> **Status at 2026-10-12 — Open.** No runtime feature-flag facility, no `hardening-baseline.md`, and no `mg-posture` exist. Some opt-outs are explicit at the CLI or env level (for example `MIDGARD_ALLOW_FILE_SIGNING_KEY`), but there is no effective-config dump to compare against a baseline and no dual-controlled expiring relaxation mechanism.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Still to do:**
> - Depends on RF-RT-1 config: add an effective-configuration dump per service
> - Write the hardening baseline document
> - Add typed runtime flags with audit, dual control and expiry for relaxations
> - Build mg-posture and wire it as a deployment gate
> - Emit a start-up warning for every relaxed control
>
> Checked: `services/runtime/src/lib.rs:20` · `services/runtime/src/args.rs:17` · `tools/mg-sign/src/main.rs:247`

**Evidence.** The workspace has meaningful feature gates (`test-backend` vs `backend-fips`, `default-features = false` across clients), but there is no runtime feature-flag mechanism, no single statement of the secure default configuration, and no way to verify a deployed instance matches the hardened baseline.

**Prompt**

```
Make the secure configuration explicit, checkable, and hard to leave.

1. Add a runtime feature-flag facility to services/runtime: typed flags with
   defaults, per-environment overrides, and an audit record for every flag
   change. Security-relevant flags (anything that relaxes a control) require
   dual-control to enable and expire automatically.
2. Write docs/dev/operations/hardening-baseline.md: the exact configuration that
   constitutes a hardened deployment, per service, with the rationale for each
   setting and the risk of deviating.
3. Implement `mg-posture`: reads a running enclosure's effective configuration
   (via the admin API, authenticated) and reports every deviation from the
   baseline, with severity. Run it as a scheduled check feeding the Admin
   console (RF-AD-6) and as a deployment gate.
4. Ensure defaults are the hardened values, so an operator must actively opt out
   of a control rather than opt in. Any relaxation is logged at start-up as a
   prominent warning naming the control and the risk.
```

**Acceptance criteria**

- **AC-RF-RT-7a — Defaults are hardened.** *Given* a service started with an empty configuration, *when* its effective settings are dumped, *then* they match the hardening baseline exactly.
- **AC-RF-RT-7b — Deviation is visible.** *Given* a deployment that relaxes a control, *when* `mg-posture` runs, *then* it reports the deviation with severity and the control's id, and the Admin console surfaces it.
- **AC-RF-RT-7c — Relaxations are gated and expiring.** *Given* an attempt to enable a security-relaxing flag, *when* it is made without dual control, *then* it is refused; *when* granted, it carries an expiry after which the flag reverts and the reversion is audited.
- **AC-RF-RT-7d — Start-up warns.** *Given* a relaxed control, *when* the service starts, *then* a warning names the control and its risk.

**Done when:** the baseline document exists; `mg-posture` runs as a gate; a deliberately relaxed deployment is detected.

---
# C — Persistence & durability

> **Why this is Critical.** There is no file or database I/O anywhere in `services/`, `mimir/`, `forge/`, `bifrost/`, `heimdall/`, `admin/`, `clients/` or `crypto/`. Every store in the platform — the object fabric, the append-only audit ledger, the model registry, the dataset store, admin configuration, the client keystore — is an in-process collection. Restarting a service destroys the audit trail. This is the single largest gap between the reference implementation and a system anyone can operate.

---

### RF-PER-1 · Choose and implement the storage substrate

**Severity:** Critical · **Area:** Persistence · **Depends on:** RF-RT-1 · **Blocks:** RF-PER-2..8, RF-RES-*, RF-SCL-*

> **Status at 2026-10-12 — Partial.** The Evidence is stale: durable, KMS-sealed, fsync-per-record persistence now exists as `audit_writer::FileLedger` and the generic `SealedJournal`, with per-crate `durable.rs` stores replaying into the existing in-memory types (fabric, governance, iam directory, admin stores, bifrost fleet/artefact, heimdall store, forge dataset-store). Restart, ciphertext-at-rest, wrong-root and root-rotation tests exist. What was asked for architecturally does not: no ADR, no KeyValueStore/AppendOnlyLog/BlobStore seam, no storage-layout document, in-memory backends are not feature-gated out of production, and there is no process-kill test.
> Criteria met 2 of 7 (c, d); open: e, f; not verifiable by reading: a, b, g. Confidence: medium.
>
> **Scope change:** The design that landed is a KMS-sealed append-only journal replayed into in-memory stores (`audit_writer::SealedJournal`, `FileLedger`, per-crate `durable.rs`), not a storage engine behind three traits. Criteria e and f no longer fit that design; record the decision in an ADR and rewrite them.
>
> **Still to do:**
> - Write the storage ADR recording the journal-replay design actually adopted, and rewrite the item around it instead of an engine behind three traits
> - Add a kill-and-restart (SIGKILL) test at process level and an all-services restart test
> - Document the on-disk layout, permissions and disk-space model
> - Decide and enforce whether production binaries may link in-memory stores
> - Address the load-everything-into-memory model (see RF-RT-6)
>
> Checked: `crypto/audit-writer/src/journal.rs:90` · `services/fabric/tests/durable.rs:151` · `services/fabric/tests/rc_42.rs:82` · `services/kms/tests/rc_28.rs:113` · `services/http-gateway/tests/state_dir_boot.rs:114`

**Evidence.** Grepping `std::fs` and `File::` across every product and service crate returns nothing. `audit_writer::InMemoryLedger` is the only ledger implementation. `services/iam` documents itself as having "No database … identities live in an in-memory store". `fabric::store::ObjectStore` is described as "horizontally-scalable" and is a `BTreeMap`.

**Prompt**

```
Introduce the storage substrate the enclosure has never had.

1. Write an ADR at docs/dev/adr/0002-storage-substrate.md. Evaluate candidates
   against the hard constraints: vendorable and offline-installable, reproducible
   builds, no phone-home, PQ-compatible at-rest encryption via services/kms
   (not the engine's own crypto), auditability of the dependency graph, and
   operability by a small team in an air-gapped site. Consider at minimum: an
   embedded engine (redb/sled-class) per service, a single embedded engine with
   logical separation, and a vendored server engine (PostgreSQL-class) for the
   HA topology. Record the decision, the rejected options, and the migration
   consequences.
2. Define ONE storage seam so no crate depends on the engine directly:
     trait KeyValueStore  — get/put/delete/scan-prefix, atomic batch
     trait AppendOnlyLog  — append, read-from-sequence, truncate-never
     trait BlobStore      — content-addressed put/get by digest
   Each with a durability contract (when a write is guaranteed on stable
   storage), a transaction/consistency contract, and an explicit statement of
   what is NOT guaranteed.
3. Encrypt at rest through services/kms, not the engine: every value written is
   KEM-DEM sealed under the trust root before it reaches storage, so a stolen
   data directory is inert. Record the root key id and suite per blob so
   rotation needs no migration (the existing kms design already supports this —
   preserve that property).
4. Implement backends: an embedded production backend per the ADR, plus the
   existing in-memory implementations retained ONLY behind a test feature. No
   production binary may link the in-memory backend (enforce as in RF-RT-5b).
5. Migrate every in-memory store to the seam, one crate at a time, preserving
   the current API so existing tests continue to pass unchanged. The test suite
   must remain runnable fully in-memory for speed.
6. Add a storage layout document: directory structure, file ownership and
   permissions, what may be safely deleted, and the disk-space model.
```

**Acceptance criteria**

- **AC-RF-PER-1a — State survives restart.** *Given* an enclosure with objects, policies, identities, audit records, model registrations and configuration, *when* every service is stopped and restarted, *then* all state is present and identical.
- **AC-RF-PER-1b — Durability contract holds.** *Given* a write that has returned success, *when* the process is killed with SIGKILL immediately afterwards, *then* the write is present after restart.
- **AC-RF-PER-1c — At-rest data is inert.** *Given* a copy of the data directory without the trust root, *when* it is inspected, *then* no plaintext object, audit payload, identity or configuration value is recoverable.
- **AC-RF-PER-1d — Rotation without migration.** *Given* data sealed under root key K1, *when* the root is rotated to K2, *then* old data remains readable and new writes use K2, with no rewrite of existing blobs.
- **AC-RF-PER-1e — No engine leakage.** *Given* the source tree, *when* scanned, *then* no crate outside the storage backend imports the engine; all use the three traits.
- **AC-RF-PER-1f — In-memory is test-only.** *Given* a production binary, *when* inspected, *then* the in-memory backend is absent.
- **AC-RF-PER-1g — Offline.** *Given* `--network none`, *when* the substrate is initialised and exercised, *then* it works.

**Done when:** the ADR is merged; every in-memory store is behind the seam; the kill-and-restart test is green in CI.

---

### RF-PER-2 · Durable, tamper-evident audit ledger with verifiable history

**Severity:** Critical · **Area:** Persistence · **Depends on:** RF-PER-1

> **Status at 2026-10-12 — Partial.** `FileLedger` is the durable ledger: KMS-sealed, length-framed, O_APPEND with fsync before acknowledging, behind the unchanged `LedgerStore`/`AuditWriter` API, and `verify_chain` names the first bad sequence; restart-and-reverify and on-disk-tamper tests exist. Missing: segments and sealed segment summaries, any `mg-audit` tool (verify or export), torn-write recovery (a truncated final frame makes `open` fail rather than truncate-and-audit), retention/archival and legal hold.
> Criteria met 1 of 7 (e); open: d, f, g; not verifiable by reading: a, b, c. Confidence: medium.
>
> **Scope change:** `FileLedger` is durable with fsync before acknowledgement. The remaining gaps are `mg-audit`, torn-write recovery (it currently fails closed instead of truncating), segmenting and legal hold.
>
> **Still to do:**
> - Build mg-audit verify and export as offline tools over the sealed ledger file
> - Implement torn-final-write recovery that truncates to the last verified entry and audits the truncation
> - Add segment roll with signed, chained segment summaries
> - Add on-disk tamper tests for delete, reorder and insert that assert the reported sequence number, and a SIGKILL durability test
> - Implement archival and legal hold under dual control
>
> Checked: `crypto/audit-writer/src/file.rs:159` · `crypto/audit-writer/src/file.rs:136` · `services/kms/tests/file_ledger_restart.rs:79` · `services/kms/tests/file_ledger_restart.rs:148` · `crypto/audit-writer/tests/chain_break.rs:33`

**Evidence.** `crypto/audit-writer` (383 lines) implements a PQ-signed chained ledger correctly — over `InMemoryLedger`. `services/governance` writes every authorisation decision into it. On restart the chain is gone, which makes `AC-M-3` (append-only, tamper-evident audit) true only within a single process lifetime, and makes `AC-AD-6`/`AC-AD-12` unfalsifiable in operation.

**Prompt**

```
Make the audit ledger durable and independently verifiable.

1. Implement `PersistentLedger` over the AppendOnlyLog seam: each entry is
   PQ-signed and chained to its predecessor's digest exactly as today, fsynced
   before the write is acknowledged, and never rewritten. Preserve the existing
   AuditWriter API so governance, admin/audit, heimdall/workflow and
   clients/inference need no change.
2. Segment and seal: roll the log into segments; at each roll, sign a segment
   summary (first/last sequence, first/last digest, entry count, segment digest)
   and chain segment summaries into a higher-level chain, so verifying the whole
   history does not require replaying every entry.
3. Build `mg-audit verify <path>`: recomputes the full chain offline, from
   genesis, against the trust anchors; detects insertion, deletion, reordering
   and modification; reports the exact sequence number of the first divergence.
   Fail closed. This is the tool an auditor runs, and it must work with no
   running service.
4. Recovery semantics: a torn final write is detected and the log is truncated
   to the last complete, verified entry — with the truncation itself audited to
   the next segment. A corrupt middle segment makes the ledger unusable and the
   service refuses to start rather than serving a broken chain.
5. Retention and legal hold: entries are never deleted; retention governs
   ARCHIVAL (sealed, exported, offline) not deletion. Implement legal hold as a
   flag that blocks archival. Both are governed operations requiring dual
   control and are themselves audited. [RF-CMP-4]
6. Export: `mg-audit export --from <seq> --to <seq>` produces a signed, portable
   evidence bundle whose verification does not require the enclosure.
```

**Acceptance criteria**

- **AC-RF-PER-2a — Chain survives restart.** *Given* N audited actions, *when* the service restarts and M more occur, *then* `mg-audit verify` validates a single unbroken chain of N+M entries.
- **AC-RF-PER-2b — Every tamper is located.** *Given* a persisted ledger, *when* an entry is modified, deleted, reordered or inserted, *then* verification fails and names the first divergent sequence number for each case.
- **AC-RF-PER-2c — Durable before acknowledged.** *Given* an audited action whose call returned, *when* the process is SIGKILLed, *then* the entry is present and verifies after restart.
- **AC-RF-PER-2d — Torn write is recovered honestly.** *Given* a truncated final record, *when* the service starts, *then* it truncates to the last verified entry and audits the truncation; it does not silently discard.
- **AC-RF-PER-2e — Corruption fails closed.** *Given* a corrupted middle segment, *when* the service starts, *then* it refuses to serve and reports the segment.
- **AC-RF-PER-2f — Offline auditor verification.** *Given* an exported evidence bundle and the trust anchors, *when* an auditor verifies it on an unrelated machine with no Midgard install beyond `mg-audit`, *then* verification succeeds.
- **AC-RF-PER-2g — Hold blocks archival.** *Given* a legal hold on a range, *when* archival is attempted, *then* it is refused and audited.

**Done when:** `mg-audit verify` and `export` exist; the tamper table is green; restart-continuity is proven in the acceptance suite.

---

### RF-PER-3 · Object fabric storage: indexes, scale and query performance

**Severity:** High · **Area:** Persistence · **Depends on:** RF-PER-1

> **Status at 2026-10-12 — Partial.** `DurableObjectStore` persists objects and links to a sealed write-ahead journal and rebuilds type, inverted-property and adjacency indexes by replay on open; a restart test asserts identical query results and the HTTP edge runs over it with per-node authorisation during search-around and fixed root/node/hop caps. Indexes are in-memory and rebuilt every start (not persisted through a key-value seam), and there is no keyspace document, query planner or EXPLAIN, published benchmark, or CI regression gate; whether a truncation marker is returned when a cap is hit was not confirmed.
> Criteria met 1 of 6 (a); open: e; not verifiable by reading: b, c, d, f. Confidence: medium.
>
> **Scope change:** Indexes are rebuilt by journal replay on every open, which conflicts with this item's persisted incremental indexes and with criterion a ("without a rebuild"). Resolve with the RF-PER-1 decision.
>
> **Still to do:**
> - Rewrite the item for the journal-replay store: decide whether persisted incremental indexes are still wanted or start-up replay is the accepted design
> - Add an explicit truncation marker to search-around results and test it
> - Add a test that an above-clearance branch is not traversed and leaks nothing
> - Build the 10^6-object benchmark and publish and gate the envelope
> - Add query planning and an EXPLAIN surface, or drop them
>
> Checked: `services/fabric/src/durable.rs:3` · `services/fabric/tests/durable.rs:151` · `services/http-gateway/src/enclosure.rs:438` · `services/http-gateway/src/enclosure.rs:541` · `services/http-gateway/tests/durable_enclosure.rs:144`

**Evidence.** `services/fabric` (990 lines) implements the type registry, instances, inverted-property indexing, typed queries, search-around and the action/function runtime — all over in-memory maps. The module doc calls the store "horizontally-scalable"; nothing about the implementation is. Search-around and per-object authorisation are O(n) scans.

**Prompt**

```
Give the object fabric a real storage and indexing layer.

1. Persist objects, links, types and actions through the KeyValueStore seam,
   with an explicit key schema documented in docs/dev/design/fabric-keyspace.md
   (object by id, object by type, property inverted index, link forward and
   reverse adjacency, action history).
2. Make indexing incremental and durable: an index update is part of the same
   atomic batch as the write it indexes, so an index can never be stale relative
   to committed data. On start-up, verify index consistency and rebuild
   incrementally if a mismatch is found, reporting progress.
3. Make search-around bounded and streaming: hop expansion streams results with
   a configurable per-hop fan-out cap and a total-result cap, applying per-object
   authorisation during traversal rather than after, so an unauthorised branch is
   never materialised. Exceeding a cap returns a truncation marker the UI
   renders (RF-UID-2), never a silent partial result.
4. Add query planning: cost-based selection between index scan and full scan,
   with an EXPLAIN surface exposed through the SDK and rendered in the Object
   Explorer so an operator can see why a query is slow.
5. Preserve dynamic security exactly: per-object authorisation at query time
   remains mandatory and remains the fabric's responsibility, not the caller's.
   Prove it still holds under the new storage path with the existing AC-M-4
   tests unchanged.
6. Benchmark and publish: 10^6 objects, 10^7 links, cold and warm, for point
   read, typed query, 3-hop search-around and action apply. Record in
   docs/dev/operations/performance.md and gate regressions in CI (RF-TST-7).
```

**Acceptance criteria**

- **AC-RF-PER-3a — Fabric survives restart with indexes intact.** *Given* a populated fabric, *when* services restart, *then* typed queries and search-around return identical results without a rebuild.
- **AC-RF-PER-3b — Index is never stale.** *Given* a crash between a write and its index update, *when* the service restarts, *then* the index either reflects the committed write or the write is absent — never a committed object missing from its index.
- **AC-RF-PER-3c — Authorisation during traversal.** *Given* a search-around whose path crosses an object above the caller's clearance, *when* the query runs, *then* that branch is not traversed and no metadata about it appears in the result or the timing.
- **AC-RF-PER-3d — Truncation is explicit.** *Given* a traversal exceeding a fan-out or result cap, *when* results return, *then* a truncation marker is present and rendered.
- **AC-RF-PER-3e — Published performance envelope.** *Given* the benchmark corpus, *when* the suite runs, *then* measured latencies are recorded and a regression beyond the stated tolerance fails CI.
- **AC-RF-PER-3f — AC-M-4 unchanged.** *Given* the existing dynamic-security tests, *when* run against the persistent fabric, *then* they pass without modification.

**Done when:** the keyspace document exists; the benchmark is in CI; restart-with-indexes is proven.

---

### RF-PER-4 · Durable registries: models, artefacts, datasets, configuration

**Severity:** High · **Area:** Persistence · **Depends on:** RF-PER-1

> **Status at 2026-10-12 — Partial.** Most listed registries are now durable with restart tests: admin config, identity, privilege (including in-flight state) and break-glass (absolute expiry recomputed from the injected clock), bifrost fleet state and artefact registry (re-verified on open), forge dataset-store and worker checkpoints, heimdall picture and in-flight workflows, and a signed on-disk model directory. Not found: versioned schema with refuse-newer and audited forward migration, a documented restart-semantics table, deterministic rollout resume-or-rollback, an interrupted-pipeline-run-marked-failed path, and persistence for staging intake records or client delivery records.
> Criteria met 1 of 6 (b); open: d, e, f; not verifiable by reading: a, c. Confidence: medium.
>
> **Scope change:** The Evidence is superseded for the admin, Bifrost, Forge and Heimdall stores. What remains is schema versioning, rollout resume, interrupted pipeline runs and the restart-semantics table.
>
> **Still to do:**
> - Add a schema version to every journal record format, refuse newer data, and audit forward migrations
> - Define and test rollout behaviour when the controller restarts mid-ring
> - Record pipeline runs durably and mark an interrupted run failed on restart
> - Write the restart-semantics table and add one test per row, including kill-after-grant
> - Persist the remaining stores (staging intake, client delivery install records) or strike them from the list
>
> Checked: `admin/breakglass/tests/durable_restart.rs:57` · `admin/privilege/tests/durable_restart.rs:122` · `bifrost/fleet/tests/durable_restart.rs:57` · `bifrost/artefact/src/durable.rs:1` · `forge/worker/src/lib.rs:678`

**Evidence.** `mimir/model-server/src/model_registry.rs` and `registry.rs`, `bifrost/artefact/src/registry.rs`, `forge/dataset-store`, `admin/config/src/store.rs` and `admin/identity/src/session.rs` are all in-memory. A restart loses every registered model, every recorded artefact, every dataset version and every admin session — including, critically, any in-flight dual-control approval or break-glass window.

**Prompt**

```
Persist every registry and every piece of operational state.

For each of: mimir model registry, mimir staging intake records, bifrost artefact
registry, bifrost rollout/fleet state, forge dataset-store versions and branches,
forge pipeline definitions and run history, heimdall picture layers and track
history, admin config store and GitOps history, admin identity sessions, admin
privilege grants and approvals, admin break-glass windows, client delivery
install records:

  1. Move the store behind the RF-PER-1 seam with a documented key schema.
  2. Define the durability point precisely: which operations must be durable
     before they are acknowledged. Anything security-relevant (a granted
     privilege, an opened break-glass window, an approved dual-control action,
     a verified model registration) is durable BEFORE it takes effect.
  3. Define restart semantics: what happens to in-flight state. A dual-control
     approval survives; a break-glass window survives WITH its original expiry
     (it does not restart); a partially-applied rollout resumes or rolls back
     deterministically; an interrupted pipeline run is marked failed, never
     silently retried.
  4. Add versioned schema migration: every persisted structure carries a schema
     version; a service refuses to start against data written by a newer schema,
     and applies forward migrations for older ones, auditing each migration.
  5. Preserve existing crate APIs so the AC-* tests are unchanged.
```

**Acceptance criteria**

- **AC-RF-PER-4a — Registries survive restart.** *Given* registered models, artefacts, datasets and configuration, *when* services restart, *then* all are present with identical verification state.
- **AC-RF-PER-4b — Break-glass windows do not reset.** *Given* an open break-glass window with 4 minutes remaining, *when* the admin service restarts, *then* the window still has approximately 4 minutes remaining and expires on the original schedule.
- **AC-RF-PER-4c — Privileges are durable before effective.** *Given* a granted time-boxed privilege, *when* the process is killed immediately after the grant returns, *then* the grant is present after restart with its original expiry.
- **AC-RF-PER-4d — Rollouts resume deterministically.** *Given* a rollout interrupted mid-ring, *when* the controller restarts, *then* it either resumes from the recorded position or rolls back — and which one is a documented, tested decision, not an accident.
- **AC-RF-PER-4e — Schema versioning is enforced.** *Given* data written by a newer schema version, *when* an older service starts, *then* it refuses to start with a clear message. *Given* older data, *when* a newer service starts, *then* it migrates forward and audits the migration.
- **AC-RF-PER-4f — Interrupted runs are honest.** *Given* a pipeline run interrupted mid-execution, *when* the service restarts, *then* the run shows as failed with the interruption recorded — never as succeeded or silently re-run.

**Done when:** every listed store is persistent; the restart-semantics table is documented and each row tested.

---

### RF-PER-5 · Client-side persistence: offline store, keystore, sync queue

**Severity:** High · **Area:** Persistence · **Depends on:** RF-PER-1

> **Status at 2026-10-12 — Open.** Unchanged from the Evidence: no file or storage I/O exists anywhere under `clients/**/src` (a grep for `std::fs`, `File::` and `OpenOptions` returns nothing), so the client store, keystore and sync queue remain in-memory. `verify-clients.sh` exists but nothing in it can prove durability across a restart.
> Criteria met 0 of 6; open: a, b, c, e, f; not verifiable by reading: d. Confidence: medium.
>
> **Still to do:**
> - Persist the client object store, pending-operation queue and CRDT metadata, sealed under the platform keystore
> - Define per-platform storage locations and floors
> - Bound the pending queue and make replay after restart deterministic, with cross-restart convergence tests
> - Implement wipe and storage-exhaustion handling
> - Extend verify-clients.sh with seed, kill, restart, converge
>
> Checked: `justfile:250`

**Evidence.** `clients/store` (297 lines), `clients/keystore` (490), `clients/sync` (311) implement sealed storage, key custody and CRDT convergence — in memory. An "offline-first" client that loses everything on process exit is not offline-first. `AC-C-4` (offline CRDT sync) is proven within a process.

**Prompt**

```
Make the client core genuinely offline-first.

1. Persist the client object store, the pending-operation queue and the CRDT
   causal metadata to local durable storage, sealed under the platform keystore
   exactly as today (the KEM-DEM envelope is unchanged — the client re-rolls no
   crypto, KDD-1).
2. Persist across the six platform floors: define the storage location and
   permissions per platform (iOS app container, Android internal storage,
   Linux XDG data dir, Windows AppData, macOS Application Support, browser
   OPFS/IndexedDB for the WASM shell) and the security floor each provides. The
   browser extension persists NOTHING sensitive (KDD-2) — verify that
   structurally, as the current type system already does for sealing.
3. Bound the pending-operation queue with an explicit full policy, and persist
   its ordering so replay after restart is deterministic and idempotent (the
   existing convergence tests already assert idempotence — extend them across a
   restart boundary).
4. Implement local wipe: a remote or local wipe destroys the sealed store and
   the key material such that recovery is infeasible, and the wipe is recorded
   for the next sync. Tie into RF-UX-9.
5. Handle storage exhaustion: a full device fails closed on writes with a clear
   user-facing state, never silent data loss.
6. Extend verify-clients.sh to prove offline durability: seed, kill, restart,
   converge — all with no network.
```

**Acceptance criteria**

- **AC-RF-PER-5a — Offline edits survive restart.** *Given* edits made with no connectivity, *when* the client process is killed and restarted still offline, *then* every edit and its causal metadata are present.
- **AC-RF-PER-5b — Convergence across restart.** *Given* two clients that diverged offline and were both restarted, *when* they reconnect, *then* they converge to the same state, and re-applying the pending list is idempotent.
- **AC-RF-PER-5c — At-rest sealing holds.** *Given* the on-device store copied off the device without the keystore, *when* inspected, *then* no plaintext object is recoverable.
- **AC-RF-PER-5d — Extension holds nothing.** *Given* the browser extension, *when* its storage is inspected after use, *then* no key material and no sealed object payload is present.
- **AC-RF-PER-5e — Wipe is effective and recorded.** *Given* a wipe, *when* it completes, *then* the sealed store and key material are unrecoverable and the wipe appears in the next sync and in the audit ledger.
- **AC-RF-PER-5f — Full device fails closed.** *Given* exhausted storage, *when* a write is attempted, *then* it fails with an explicit state surfaced to the user and no partial write is committed.

**Done when:** `verify-clients.sh` proves durability across a restart, air-gapped, for the shared core.

---

### RF-PER-6 · Data lifecycle: retention, archival, deletion and right-to-erasure

**Severity:** High · **Area:** Persistence · **Depends on:** RF-PER-1, RF-PER-2 · **Blocks:** RF-CMP-4

> **Status at 2026-10-12 — Open.** No data-lifecycle document, retention policy object, archival export, crypto-shredding or legal-hold mechanism was found in source; `docs/dev/design/` does not exist. `docs/ops/BACKUP_RESTORE.md` and `docs/compliance/DATA_RESIDENCY.md` exist but were not opened and are not implementations.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: low.
>
> **Still to do:**
> - Write the data-lifecycle design per data class
> - Implement retention as a governed, simulatable policy object
> - Implement per-subject sealed audit payloads and crypto-shredding with a post-shred chain verification test
> - Implement signed archival with restore, and legal hold that blocks archival and shredding
>
> Checked: `crypto/audit-writer/src/file.rs:149` · `crypto/audit-writer/src/journal.rs:90`

**Evidence.** No retention policy, archival mechanism, or deletion path exists anywhere. The audit ledger is append-only by design, which is correct — but there is no reconciliation between "never delete audit" and any obligation to erase personal data, and no lifecycle for objects, datasets, model artefacts, or logs.

**Prompt**

```
Design and implement the data lifecycle, including the audit/erasure tension.

1. Write docs/dev/design/data-lifecycle.md: for every data class (fabric objects,
   links, action history, audit entries, datasets and versions, model artefacts,
   logs, telemetry, session records, client stores), state the retention driver,
   default period, archival target, deletion semantics, and who may change it.
2. Implement retention as a governed policy object evaluated by
   services/governance, not as ad-hoc per-service code. Changing retention is a
   dual-control, audited, simulatable action (it must appear in admin/sim's
   dry-run exactly like an access change).
3. Implement archival: sealed, signed, verifiable export of aged data to an
   offline target, with the archive's own manifest and signature, and a
   documented restore path. Archival is not deletion.
4. Resolve erasure vs immutable audit with crypto-shredding: personal data in
   audit entries is stored as a KMS-sealed payload under a per-subject key;
   erasure destroys that key, rendering the payload unrecoverable while the
   chain, its digests and its signatures remain intact and verifiable. Document
   this as the erasure mechanism, prove the chain still verifies after a shred,
   and record the shred itself as an audit event.
5. Implement legal hold as an override that blocks archival, deletion and
   shredding, applied per subject or per range, dual-controlled and audited.
6. Surface all of it in the Admin console: current policies, next archival,
   pending erasure requests, active holds (RF-AD-5).
```

**Acceptance criteria**

- **AC-RF-PER-6a — Policy-driven, simulatable.** *Given* a proposed retention change, *when* it is simulated, *then* the report shows exactly which data classes and volumes are affected before any commit.
- **AC-RF-PER-6b — Chain survives shredding.** *Given* a crypto-shred of a subject's key, *when* the audit chain is verified, *then* it verifies end to end; *and when* the shredded payload is read, *then* it is unrecoverable.
- **AC-RF-PER-6c — Shred is itself audited.** *Given* an erasure, *when* the ledger is inspected, *then* an entry records the request, the approvers, the scope and the completion.
- **AC-RF-PER-6d — Hold beats retention.** *Given* data under legal hold whose retention period has expired, *when* archival runs, *then* the data is retained and the skip is recorded.
- **AC-RF-PER-6e — Archives are verifiable and restorable.** *Given* an archive, *when* verified on an unrelated machine, *then* its signature and manifest validate; *when* restored, *then* the data matches the original.

**Done when:** the lifecycle document is written; crypto-shredding is implemented and the post-shred chain verification test is green.

---

### RF-PER-7 · Transactionality and consistency across service boundaries

**Severity:** High · **Area:** Persistence · **Depends on:** RF-PER-1, RF-API-1

> **Status at 2026-10-12 — Open.** No consistency-model document, idempotency keys on mutating APIs, outbox, or audit-versus-state reconciliation job was found. The premise has also shifted: the core services (fabric, governance, kms, iam) still run in one `http-gateway` process rather than as separate processes with separate stores, so the cross-process problem the item anticipates only exists between the five server binaries. `admin/config/src/reconcile.rs` is GitOps config reconciliation, not this.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: low.
>
> **Scope change:** Written for one process and one store per service. The core services share one process; scope this to the five-binary topology that exists.
>
> **Still to do:**
> - Enumerate the operations that actually cross a process boundary in today's five-binary topology and write the consistency model for those
> - Add idempotency keys to mutating edge and mesh APIs with replay tests
> - Order journal and audit writes so the audit entry is the commit point, and test crash points between them
> - Add a reconciliation job that surfaces audit/state divergence
>
> Checked: `services/http-gateway/tests/durable_enclosure.rs:1` · `services/runtime/src/lib.rs:7` · `supply-chain/release-artefacts.txt:14`

**Evidence.** A single logical operation today spans several crates in one process (e.g. an action apply touches `fabric`, `governance`, `audit-writer`). Once these are separate processes with separate stores (RF-RT-1, RF-PER-1), there is no defined consistency model, no distributed transaction story, and no idempotency mechanism — which will produce duplicated mutations and audit entries that do not match reality.

**Prompt**

```
Define and implement the consistency model for multi-service operations.

1. Write docs/dev/design/consistency-model.md enumerating every operation that
   spans services (action apply, policy commit, privilege grant, model
   registration, artefact promotion, rollout step, pipeline run, break-glass
   open/close), and for each state the required consistency, the failure modes,
   and the reconciliation.
2. Prefer designs that avoid distributed transactions: make the audit write the
   commit point where possible (an action is "done" iff its audit entry is
   durable), and make downstream effects idempotent and replayable from it.
3. Implement idempotency keys on every mutating API: a caller-supplied key makes
   retries safe; the server records the key with the outcome and returns the
   original result on replay, within a documented window.
4. Implement the outbox pattern where an effect must follow a local commit:
   record the intent durably in the same transaction, then dispatch with
   at-least-once delivery plus idempotent receipt.
5. Implement reconciliation jobs that detect and report divergence (an audit
   entry with no corresponding state change, or vice versa) and surface it in
   the Admin console as an integrity alert.
6. Test with fault injection (RF-TST-8): kill each participant at each step
   boundary and assert the documented outcome every time.
```

**Acceptance criteria**

- **AC-RF-PER-7a — Retries are safe.** *Given* a mutating call retried with the same idempotency key, *when* it is replayed, *then* exactly one effect occurred and both calls return the same result.
- **AC-RF-PER-7b — Documented outcome under every crash point.** *Given* a multi-service operation, *when* any single participant is killed at any step boundary, *then* the resulting state matches the consistency model's stated outcome for that step.
- **AC-RF-PER-7c — Audit matches reality.** *Given* the reconciliation job, *when* it runs after a fault-injection campaign, *then* it reports zero unexplained divergences.
- **AC-RF-PER-7d — Divergence is surfaced.** *Given* an artificially induced divergence, *when* reconciliation runs, *then* an integrity alert appears in the Admin console naming the operation and the participants.

**Done when:** the consistency model is documented; the fault-injection matrix is green; the reconciliation job runs on a schedule.

---

### RF-PER-8 · Storage operations: capacity, compaction, integrity scrubbing

**Severity:** Medium · **Area:** Persistence · **Depends on:** RF-PER-1

> **Status at 2026-10-12 — Open.** No `mg-store`, no capacity metrics or hard-limit refusal, no compaction (journals are append-only and replayed whole), and no background scrubber exist. Integrity is checked only at open, where a bad frame fails the whole store closed. The item has no Evidence paragraph in the plan.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Still to do:**
> - Build mg-store with offline stat and verify over sealed journals and ledgers
> - Expose per-store size and growth metrics and refuse writes at a hard limit
> - Add journal compaction or snapshotting so replay time and memory stay bounded
> - Add a scheduled scrubber that audits and alerts on corruption
> - Document the disk layout and backup set
>
> Checked: `crypto/audit-writer/src/journal.rs:50` · `services/fabric/src/durable.rs:62` · `bifrost/artefact/src/durable.rs:6`

**Prompt**

```
Make the storage tier operable.

1. Capacity: expose used/available per store, growth rate, and projected
   exhaustion date as metrics (RF-OBS-1) and in the Admin console. Alert well
   before exhaustion; refuse writes with a clear fail-closed error at the hard
   limit rather than corrupting.
2. Compaction and vacuum: implement online compaction with bounded impact,
   scheduled in a configurable window, cancellable, and reporting progress.
3. Integrity scrubbing: a background scrubber that periodically reads and
   verifies stored blobs against their recorded digests, reports corruption,
   and (where a replica exists) repairs. Corruption findings are audited and
   alerted, never silently corrected.
4. Tooling: `mg-store` with `stat`, `verify`, `compact`, `export`, `import`, and
   `repair` subcommands, all authenticated and audited when run against a live
   enclosure, and usable offline against a cold data directory.
5. Document the disk layout, permissions, and the exact set of files an operator
   may back up or move.
```

**Acceptance criteria**

- **AC-RF-PER-8a — Exhaustion is predicted and then refused.** *Given* growth toward capacity, *when* the projection crosses the alert threshold, *then* an alert fires; *when* the hard limit is reached, *then* writes fail closed with a distinguishable error and no corruption.
- **AC-RF-PER-8b — Compaction is bounded and safe.** *Given* compaction during serving, *when* it runs, *then* latency stays within the stated tolerance, it can be cancelled cleanly, and no data is lost or duplicated.
- **AC-RF-PER-8c — Corruption is detected, not hidden.** *Given* a deliberately corrupted blob, *when* the scrubber runs, *then* it reports the blob, audits the finding and raises an alert.
- **AC-RF-PER-8d — Offline tooling works.** *Given* a cold data directory and no running service, *when* `mg-store verify` runs, *then* it validates integrity and reports.

**Done when:** `mg-store` exists; the scrubber runs on a schedule; capacity metrics and alerts are live.

---

# D — Transport & API surface

> **Why.** The proto contract — described as "the source of truth for all SDKs" — contains two files covering the object fabric and shared types. The browser edge exposes 15 endpoints, all Midgard-core. Five of six products have no server-side API at all. Nothing between services is defined by contract except the PQ-mTLS byte frame.

---

### RF-API-1 · Complete the proto contract for every subsystem

**Severity:** High · **Area:** API · **Depends on:** RF-RT-1 · **Blocks:** RF-API-2..7, RF-UID-*, RF-SDK-*

> **Status at 2026-10-12 — Partial.** `contracts/proto/midgard/v1/` now has three files: `common.proto`, `object_fabric.proto` and a new `gateway.proto` (13 Gateway RPCs for the Midgard-core edge). `contracts/README.md` explicitly rescopes the protos to the client SDK edge only and says inter-service traffic is a hand-rolled pq-mtls codec typed by Rust, so there are no per-subsystem packages, no typed error enum, no HUMAN_GATE_REQUIRED, no pagination model. `check-conformance.sh` only checks that each `GatewayClient` method name has an RPC of the same name; `buf.gen.yaml` uses remote buf.build plugins and there is no CI to run `buf breaking`.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Scope change:** `contracts/README.md` records a decision that the protos govern only the client edge and that inter-service traffic is a hand-written PQ-mTLS codec typed in Rust. That contradicts this item's proto-per-subsystem design; decide which stands. `buf breaking` is not wired into any CI, because there is none.
>
> **Still to do:**
> - Decide whether the README's rescope (protos govern the client edge only) stands, then either author the eleven missing subsystem packages or rewrite this item to match
> - Add the governance envelope to every response message (markings, audit sequence, redaction indicator) and a shared typed error enum including HUMAN_GATE_REQUIRED
> - Replace the name-only conformance script with a type-level conformance test between `sdk/ts/src/types.ts` and generated types
> - Vendor the buf toolchain and plugins so `just gen` runs with no network (buf.gen.yaml pulls remote plugins today)
> - Create a CI runner that executes `just proto-breaking`; none exists in the tree
>
> Checked: `contracts/proto/midgard/v1/gateway.proto:16` · `contracts/README.md:15` · `contracts/check-conformance.sh:15` · `contracts/buf.gen.yaml:6` · `justfile:19`

**Evidence.** `contracts/proto/midgard/v1/` contains exactly `common.proto` and `object_fabric.proto`. `contracts/buf.yaml` and `buf.gen.yaml` exist and `just gen` runs `buf lint` + `buf generate`, but there is nothing to generate for IAM, KMS, governance, scheduler, Mimir, model supply, Forge, Bifrost, Heimdall, admin, or clients. The `SdkScreen` in the Midgard console renders an "RPC surface" that has no proto behind it.

**Prompt**

```
Define the full contract surface, and make it the single source of truth.

1. Author one proto package per subsystem under contracts/proto/midgard/v1/:
   iam, kms, governance, scheduler, object_fabric (extend), mimir (inference,
   agents, evals, guardrails), model_supply, forge (connectors, datasets,
   pipelines, health, projection), bifrost (artefacts, bundles, channels, fleet,
   rollout, risk), heimdall (feeds, picture, entities, workflows, sync,
   lineage), admin (identity, privilege, config, sim, audit, breakglass,
   copilot), clients (sync, delivery, inference-tiering, broker).
2. Model the governance envelope in the contract, not around it: every request
   carries the policy context (subject, purpose, clearance, roles) and every
   response carries the classification markings, the audit sequence number, and
   any redaction indicator, so a client cannot render governed data without
   also having the evidence of how it was governed.
3. Model human gates explicitly: a mutating RPC either carries a valid human-
   gate token or returns a typed HUMAN_GATE_REQUIRED error carrying what the
   approval needs — never a generic denial the UI must guess at.
4. Model errors as a shared, typed enumeration with stable codes, a
   machine-readable reason, a human-readable message, a retryability flag, and a
   correlation id. No stringly-typed errors anywhere.
5. Model pagination, filtering, sorting, and partial-result truncation uniformly
   across every list RPC, and streaming (server-streaming) for audit, feeds,
   traces, build logs and agent responses.
6. Enforce compatibility: `buf breaking` against main is already wired in CI —
   extend it to every new package, and document the versioning policy (additive
   only within v1; a v2 package for anything else).
7. Generate: Rust server/client stubs, TypeScript client types, and the
   documentation surface, all from buf. The hand-written types in
   sdk/ts/src/types.ts must become generated or must be proven equivalent by a
   conformance test — they may not drift.
```

**Acceptance criteria**

- **AC-RF-API-1a — Every subsystem has a contract.** *Given* the twelve subsystem areas, *when* `contracts/proto` is inspected, *then* each has a package covering the operations its console and SDK expose.
- **AC-RF-API-1b — Governance is in the contract.** *Given* any response carrying governed data, *when* its message is inspected, *then* it carries markings, the audit sequence, and any redaction indicator.
- **AC-RF-API-1c — Human gates are typed.** *Given* a mutating RPC called without a gate token, *when* it is rejected, *then* the error is the typed `HUMAN_GATE_REQUIRED` carrying the approval requirements.
- **AC-RF-API-1d — Breaking changes are blocked.** *Given* a PR that removes or renumbers a field, *when* CI runs, *then* `buf breaking` fails.
- **AC-RF-API-1e — Generated types are the only types.** *Given* the TypeScript SDK, *when* built, *then* its domain types are generated from proto or pass a conformance test against the generated ones.
- **AC-RF-API-1f — Generation is offline.** *Given* `--network none` with the buf toolchain vendored, *when* `just gen` runs, *then* it succeeds.

**Done when:** every subsystem package lints and generates; the SDK's types derive from proto; `buf breaking` covers all packages.

---

### RF-API-2 · Implement the inter-service RPC layer over PQ-mTLS

**Severity:** High · **Area:** API · **Depends on:** RF-API-1, RF-RT-1, RF-RT-2

> **Status at 2026-10-12 — Partial.** `services/gateway` now has a method `Router`, a per-identity token-bucket `RateLimiter`, a per-connection quota and PQ-mTLS session resumption, and `services/runtime/src/client.rs` gives a one-shot `call(identity, addr, purpose, method, payload)` that four server binaries and their clients use across processes. Dispatch is still hand-written over `&[u8]` with a method string; there is no request id, deadline, idempotency key, correlation id, retry, circuit breaker, connection pool, interceptor chain or generated stub, and no `rpc-protocol.md`. Authorisation is per channel at handshake by each service's `policy.json`, not a method-level allow-matrix.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Scope change:** See RF-API-1: the inter-service layer that exists is `services/runtime` over `pq-mtls`, not generated stubs. Rewrite once the contract decision is made.
>
> **Still to do:**
> - Specify the wire envelope above pq-mtls frames (request id, deadline, policy context, idempotency key, correlation id, typed status) and write it down
> - Add deadline propagation and an idempotency-key store; the client has only a fixed 30 s read timeout
> - Add a uniform server interceptor chain (authz per method, audit, log, metrics) so handlers cannot skip it, with a test that proves it
> - Add client retries for idempotent methods, per-peer circuit breaking and pooling
> - Define and enforce a reviewed service-to-method allow-matrix and audit refused callers in the ledger (refusals are only logged and counted today)
>
> Checked: `services/gateway/src/lib.rs:116` · `services/gateway/src/lib.rs:167` · `services/runtime/src/client.rs:25` · `services/gateway/tests/gateway.rs:183` · `services/gateway/tests/session.rs:395`

**Evidence.** `crypto/pq-mtls` (1 371 lines) implements a real handshake and framed records, and `services/gateway` authenticates and authorises a peer then hands `&[u8]` to a `RequestHandler` returning `Vec<u8>`. There is no request/response framing above that, no method dispatch, no serialisation, no deadlines, no retries, no circuit breaking — every service-to-service call in the tree today is a direct Rust function call within one process.

**Prompt**

```
Build the RPC layer that turns the PQ-mTLS channel into a service mesh.

1. Define the wire protocol above pq-mtls frames: request id, method (fully
   qualified proto method name), deadline, policy context, idempotency key,
   correlation id, payload; response: status, typed error, payload, audit
   sequence. Server-streaming support for the streaming RPCs from RF-API-1.
   Specify it in docs/dev/design/rpc-protocol.md, including the exact byte
   layout and the versioning rule.
2. Generate server skeletons and typed clients from the proto definitions, so a
   service implements a trait and a caller gets a typed client — no hand-written
   dispatch.
3. Client-side reliability: per-call deadlines derived from the caller's
   remaining budget (deadline propagation, so a chain cannot exceed the
   originator's budget), bounded retries ONLY for idempotent methods, jittered
   backoff, circuit breaking per peer, and connection pooling with health
   checking. A retry must never bypass a human gate or re-execute a mutation
   without an idempotency key.
4. Server-side: concurrency limits per method, per-caller quotas, request
   validation before any work, and a uniform interceptor chain that performs
   authentication (peer identity from the certificate), authorisation (the PDP),
   audit, logging and metrics — so no handler can accidentally omit one.
5. Zero-trust: every hop authenticates mutually; a call from an unknown or
   unauthorised workload identity is refused before the payload is parsed;
   default-deny between services with an explicit, reviewed allow-matrix of
   which service may call which method.
6. Keep the loopback exception explicit: model-server → GPU sidecar remains
   loopback-only and is documented as such.
```

**Acceptance criteria**

- **AC-RF-API-2a — Typed calls across processes.** *Given* two services in separate processes, *when* one calls the other, *then* the call is typed, mutually authenticated over PQ-mTLS, and carries policy context and correlation id.
- **AC-RF-API-2b — Deadlines propagate.** *Given* a caller with a 2-second budget, *when* it invokes a chain three deep, *then* no downstream call outlives the originator's budget, and the chain returns a deadline-exceeded error rather than hanging.
- **AC-RF-API-2c — Unauthorised callers are refused early.** *Given* a workload identity absent from the allow-matrix, *when* it calls a method, *then* the call is refused before payload parsing and the refusal is audited.
- **AC-RF-API-2d — Retries never duplicate mutations.** *Given* a mutating call that times out after the server committed, *when* the client retries with the same idempotency key, *then* exactly one mutation exists.
- **AC-RF-API-2e — Interceptors cannot be skipped.** *Given* any handler, *when* it is invoked, *then* authentication, authorisation, audit, logging and metrics all ran — proven by a test that adds a handler omitting them and observes them applied anyway.
- **AC-RF-API-2f — Circuit breaking protects the caller.** *Given* a peer failing persistently, *when* the breaker opens, *then* calls fail fast, the breaker is metered, and it half-opens on the documented schedule.

**Done when:** the protocol is specified; generated clients and servers are in use; the allow-matrix is enforced and reviewed.

---

### RF-API-3 · Complete the browser edge for all six products

**Severity:** High · **Area:** API · **Depends on:** RF-API-1, RF-API-2, RF-IAM-3 · **Blocks:** RF-UID-*

> **Status at 2026-10-12 — Partial.** The Evidence is stale: `SecuredGateway` verifies a PQ-signed bearer token on every data route, authorises and audits through the real `fabric` + `governance` crates, has a login endpoint and locks CORS to an origin allowlist. It still serves only Midgard-core routes, composes the services in-process rather than forwarding over RPC, keeps fixed platform views in `data.rs`, sends no CSP/HSTS/other security headers, has no streaming, no body-size or rate limit, and the other six consoles are mock-only. `/metrics` and `/api/v1/ready` are served unauthenticated on the public port.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Scope change:** The edge is authenticated, CORS is allow-listed, and it serves the real fabric and governance crates in-process. "Forward every route over RPC; the edge holds no state" conflicts with that design. The concrete gap is that the edge has no routes for Forge, Mimir, Bifrost, Heimdall or admin.
>
> **Still to do:**
> - Add routes for the six product gateway interfaces in `sdk/ts/src/products/*.ts` (none exist; five services have no HTTP path at all)
> - Replace the fixed identities/keys/compute/observability/resilience/sdk-surface views in `data.rs` with real sources and delete the module
> - Add the browser security header set (CSP, HSTS, nosniff, Referrer-Policy, COOP/COEP/CORP) and an automated scan
> - Enforce a request body cap, content-type check and per-session rate limit in `http.rs` (Content-Length is trusted unbounded today)
> - Move `/metrics` and `/api/v1/ready` off the public listener or authenticate them, and add SSE streaming for audit and the other live surfaces
>
> Checked: `services/http-gateway/src/lib.rs:543` · `services/http-gateway/src/lib.rs:640` · `services/http-gateway/src/http.rs:164` · `services/http-gateway/src/data.rs:1` · `services/http-gateway/tests/secured_edge.rs:107`

**Evidence.** [`services/http-gateway/src/lib.rs:64-129`](../../../services/http-gateway/src/lib.rs) routes 15 paths, all Midgard-core (`object-types`, `query`, `actions`, `search-around`, `audit`, `policies`, `policy/simulate`, `identities`, `keys`, `compute`, `observability`, `resilience`, `sdk-surface`, health). The `data.rs` module seeds enclosure state directly rather than calling services. CORS is unconditionally permissive. There is no authentication on any route.

**Prompt**

```
Turn the http-gateway into the real, authenticated browser edge for all six
consoles.

1. Expand coverage: implement every RPC the six product gateway interfaces
   require (see sdk/ts/src/products/*.ts for the current shape — treat those
   interfaces as the requirements list, not the implementation). Every route
   forwards to the owning service over the RF-API-2 RPC layer; the edge holds no
   business logic and no seeded state.
2. Authenticate every route (RF-IAM-3): a valid session; step-up where the
   operation requires it; human-gate token where the operation is gated. An
   unauthenticated request receives 401 with no information about whether the
   resource exists.
3. Replace permissive CORS with a configured allow-list, and add the full
   browser security header set: CSP (no inline script; nonce or hash based),
   HSTS, X-Content-Type-Options, Referrer-Policy, Permissions-Policy,
   COOP/COEP/CORP, frame-ancestors none. Verify the consoles run under the CSP
   with no relaxations — including the inline styles the screens currently use
   (RF-UI-8).
4. Add streaming: server-sent events or long-poll for audit lineage, Heimdall
   feeds, Mimir agent responses, Forge build logs and Bifrost rollout progress,
   replacing any polling in the SDK. Backpressure and reconnect-with-resume are
   part of the contract.
5. Add edge concerns: request size limits, per-session rate limits, method
   allow-listing, strict content-type checking, and a uniform typed error body
   matching RF-API-1's error model.
6. Delete services/http-gateway/src/data.rs's seeded state entirely once every
   route forwards. The edge must not be able to serve data no service produced.
```

**Acceptance criteria**

- **AC-RF-API-3a — Every console operation has a route.** *Given* the six product gateway interfaces, *when* each method is exercised, *then* a real route exists and forwards to the owning service.
- **AC-RF-API-3b — No unauthenticated access.** *Given* a request with no session, *when* it hits any route except health, *then* it receives 401 and no data, and the response does not distinguish existing from non-existing resources.
- **AC-RF-API-3c — CSP holds with no relaxation.** *Given* the deployed CSP, *when* each console is loaded and exercised, *then* no CSP violation is reported and no `unsafe-inline` or `unsafe-eval` is present.
- **AC-RF-API-3d — Streaming replaces polling.** *Given* the audit, feeds, agent, build-log and rollout surfaces, *when* they update, *then* the UI receives pushed events; *and when* a connection drops, *then* it resumes from the last received sequence without duplication or gaps.
- **AC-RF-API-3e — No seeded state.** *Given* the edge with every downstream service stopped, *when* any data route is called, *then* it returns a dependency error — it cannot serve data from itself.
- **AC-RF-API-3f — Limits are enforced.** *Given* an oversized body or an over-rate session, *when* the request arrives, *then* it is rejected with the typed error and metered.

**Done when:** all six consoles run against the edge with no mocks; `data.rs` seeding is deleted; CSP and headers verified by an automated scan.

---

### RF-API-4 · API versioning, deprecation and compatibility policy

**Severity:** Medium · **Area:** API · **Depends on:** RF-API-1

> **Status at 2026-10-12 — Open.** No versioning policy document, no version negotiation, no deprecation telemetry and no generated changelog exist. Routes are fixed under `/api/v1` and the only compatibility control is the `buf breaking` recipe in the justfile, which nothing runs.
> Criteria met 0 of 3; open: a, b, c. Confidence: high.
>
> **Still to do:**
> - Write the API versioning and deprecation policy (the plan's target path docs/dev/design/ does not exist; pick a location)
> - Implement client/server version negotiation with a distinguishable too-old refusal
> - Count calls to deprecated methods per caller identity and expose them
> - Generate a proto-diff changelog and include it in the release bundle
>
> Checked: `justfile:19` · `services/http-gateway/src/lib.rs:896` · `contracts/buf.yaml:13`

**Prompt**

```
Write and enforce the API lifecycle policy.

1. docs/dev/design/api-versioning.md: the compatibility guarantee for proto,
   RPC, the HTTP edge and the SDKs; what is additive-only; how a field is
   deprecated (marked, warned, then removed only at a major version); the
   support window per version; and how a client discovers the server's version
   and supported features.
2. Implement version negotiation: the client announces its supported version;
   the server responds with the negotiated version or refuses. A client too old
   for a mandatory security change is refused with a distinguishable, actionable
   error, not silently degraded.
3. Implement deprecation telemetry: calls to deprecated methods or fields are
   counted, attributed to a caller identity, and surfaced in the Admin console so
   removal can be evidence-based.
4. Publish a machine-readable changelog per release, generated from the proto
   diff, and include it in the release bundle.
```

**Acceptance criteria**

- **AC-RF-API-4a — Negotiation works and refuses safely.** *Given* a client older than the minimum supported version, *when* it connects, *then* it receives a distinguishable refusal naming the required version.
- **AC-RF-API-4b — Deprecated use is measured.** *Given* a deprecated field in use, *when* the Admin console is opened, *then* the caller identity and call volume are visible.
- **AC-RF-API-4c — Changelog is generated.** *Given* a release, *when* the bundle is inspected, *then* it contains a proto-derived changelog listing every added, deprecated and removed element.

**Done when:** the policy is documented; negotiation is implemented; deprecation telemetry is live.

---

### RF-API-5 · API documentation and an offline explorer

**Severity:** Medium · **Area:** API · **Depends on:** RF-API-1

> **Status at 2026-10-12 — Open.** There is no generated API reference and no `docs/dev/api/`. `SdkScreen` now calls `getContractSurface()` on the edge instead of reading UI fixtures, but the edge answers from a fixed list in `data.rs`, not from reflection, and there is no try-it.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Generate reference docs from the protos with a build that fails on undocumented methods
> - Serve the contract surface from the real route table or proto descriptors instead of the fixed list in `data.rs`
> - Add an audited try-it path in SdkScreen that shows the resulting audit entry
> - Add compile-checked examples and ship the docs in the air-gap bundle
>
> Checked: `products/midgard/ui/src/screens/SdkScreen.tsx:48` · `services/http-gateway/src/data.rs:252` · `services/http-gateway/src/lib.rs:920` · `contracts/README.md:1`

**Evidence.** `products/midgard/ui/src/screens/SdkScreen.tsx` renders an RPC surface from fixtures. There is no generated reference for any API, and `contracts/README.md` is the only contract documentation.

**Prompt**

```
Generate the API reference and make the SdkScreen real.

1. Generate reference documentation from the proto definitions — every service,
   method, message, field, error code, and the governance envelope semantics —
   into docs/dev/api/, and publish it as part of the docs set. It must build
   offline and be included in the air-gap bundle.
2. Add worked examples per method in every SDK language (RF-SDK-*), generated
   and compile-checked so an example can never rot.
3. Make SdkScreen read the real surface from the server's reflection endpoint:
   live method list, request/response shapes, required policy context, whether a
   human gate applies, and a governed "try it" that executes under the operator's
   own session and shows the resulting audit entry. A try-it call is a real,
   audited call — never a simulation.
4. Document the errors: every code, when it occurs, whether it is retryable, and
   what an operator should do.
```

**Acceptance criteria**

- **AC-RF-API-5a — Reference is generated and complete.** *Given* the proto set, *when* docs are built, *then* every method and field appears with its documentation, and a method lacking documentation fails the build.
- **AC-RF-API-5b — Examples compile.** *Given* the examples in each SDK, *when* CI runs, *then* every example compiles and executes against a test enclosure.
- **AC-RF-API-5c — SdkScreen is live.** *Given* the running enclosure, *when* SdkScreen is opened, *then* it lists the server's actual methods; *when* try-it is used, *then* a real call executes and its audit entry is displayed.
- **AC-RF-API-5d — Offline.** *Given* the air-gap bundle, *when* the docs are opened with no network, *then* they render fully.

**Done when:** the reference builds in CI; SdkScreen reads reflection; try-it produces an audit entry.

---

### RF-API-6 · Webhooks, egress control and outbound integration policy

**Severity:** Medium · **Area:** API · **Depends on:** RF-API-2, RF-RT-7

> **Status at 2026-10-12 — Partial.** An egress broker now exists for one purpose: `forge-egress` renders an HAProxy sidecar config from a routes file so the Forge worker reaches declared S3/Kafka upstreams over TLS 1.3 with a pinned CA, and the worker refuses jobs that would bypass it; the Helm chart opens only the declared upstream ports. It is a per-pod TLS-originating proxy, not a policy-authorised, audited, marking-aware broker service, and there are no outbound or inbound webhooks, no ML-DSA payload signing, no Admin view and no source scan for outbound I/O.
> Criteria met 0 of 5; open: a, b, c, e; not verifiable by reading: d. Confidence: medium.
>
> **Still to do:**
> - Decide whether the HAProxy sidecar is the sanctioned broker or a dedicated broker service replaces it, and rewrite the item accordingly
> - Add PDP authorisation, ledger audit and a marking ceiling to every outbound route
> - Implement signed outbound webhooks and hardened inbound webhooks (none exist)
> - Add a scan that fails when any crate other than the broker and connector runtime opens outbound sockets
> - Test the broker-absent case so egress-dependent features report an explicit unavailable state
>
> Checked: `forge/worker/src/egress.rs:17` · `deploy/helm/midgard/values.yaml:60` · `deploy/helm/midgard/templates/enclosure.yaml:79` · `README.md:646`

**Evidence.** The platform is default-deny egress by design, and nothing today makes an outbound call. Real deployments will need notifications (approval requests, break-glass alerts, incident escalation, Heimdall alerts) and Forge connectors to external sources. There is currently no framework, which means the first such need will be met with an ad-hoc HTTP call that bypasses every control.

**Prompt**

```
Build the one sanctioned egress path, before anyone builds an unsanctioned one.

1. Implement an egress broker service: the ONLY component permitted to make
   outbound connections. Every outbound request is declared by policy
   (destination allow-list by host and path, method, size, rate), authorised by
   the PDP, audited with its full request/response metadata, and subject to
   data-loss rules that refuse to transmit data above a configured marking.
2. Implement outbound webhooks through it: registration is a dual-controlled
   admin action; payloads are signed (ML-DSA) so a receiver can verify origin;
   delivery is at-least-once with idempotency keys, bounded retries, dead-letter
   handling and per-endpoint circuit breaking.
3. Implement inbound webhooks symmetrically: signature verification, replay
   protection, size and rate limits, and strict schema validation before any
   processing.
4. Enforce structurally: a network-policy default-deny (RF-DEP-3) plus a CI check
   that no crate other than the broker and the Forge connector runtime performs
   outbound I/O. In an air-gapped deployment the broker is absent and every
   egress-dependent feature degrades to a clear, fail-closed state — never a
   silent no-op.
5. Surface egress in the Admin console: registered destinations, recent
   deliveries, failures, and any blocked-by-marking events.
```

**Acceptance criteria**

- **AC-RF-API-6a — One egress path.** *Given* the source tree, *when* scanned, *then* only the egress broker and the connector runtime perform outbound I/O.
- **AC-RF-API-6b — Marking blocks transmission.** *Given* a payload containing data above the destination's permitted marking, *when* delivery is attempted, *then* it is refused, audited and surfaced.
- **AC-RF-API-6c — Webhooks are verifiable.** *Given* a delivered webhook, *when* the receiver verifies its ML-DSA signature against the published key, *then* verification succeeds; *when* the body is altered, *then* it fails.
- **AC-RF-API-6d — Air-gap degrades cleanly.** *Given* no broker deployed, *when* an egress-dependent feature is used, *then* it reports an explicit unavailable state, and no operation silently succeeds without notifying.
- **AC-RF-API-6e — Inbound is hardened.** *Given* a replayed or unsigned inbound webhook, *when* it arrives, *then* it is rejected and audited.

**Done when:** the broker exists; the no-outbound-I/O CI check passes; air-gap degradation is tested.

---

### RF-API-7 · Rate limiting, quotas and abuse resistance

**Severity:** Medium · **Area:** API · **Depends on:** RF-API-2, RF-API-3

> **Status at 2026-10-12 — Partial.** The PQ-mTLS mesh gateway has a per-identity token bucket and a per-connection request quota, tested, which drop the connection when exceeded. The HTTP edge has no rate limit, no quota, no login throttle or lockout, no retry-after and no cost accounting, and limiter rejections are not metered.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Add per-account and per-source throttling and lockout to `/api/v1/login`, with an audited unlock path
> - Add per-identity and per-route limits on the HTTP edge returning a typed 429 with retry-after, and meter rejections
> - Make mesh limiter rejections return a typed error and a metric instead of silently dropping the connection
> - Add cost-weighted budgets for query size, search-around hops and inference tokens
> - Add a test that rate-limited responses for existing and unknown accounts are indistinguishable
>
> Checked: `services/gateway/src/lib.rs:167` · `services/gateway/tests/session.rs:395` · `services/http-gateway/src/lib.rs:247` · `services/http-gateway/src/http.rs:65`

**Prompt**

```
Add quota and abuse controls at every boundary.

1. Multi-dimensional limits: per identity, per tenant (RF-SCL-1), per method,
   per source, with burst and sustained rates, configurable per deployment and
   defaulting to conservative values.
2. Cost-aware limiting for expensive operations: inference tokens, search-around
   hops, pipeline rows scanned, query result bytes. A cheap request and a
   million-row scan must not count the same.
3. Fail-closed semantics with honest signalling: over-limit returns the typed
   error with a retry-after; limits are metered; sustained abuse raises a
   security event (RF-SEC-6).
4. Protect authentication specifically: per-account and per-source throttling on
   login and step-up, with account lockout policy and an operator unlock path
   that is itself audited.
5. Ensure limiting cannot be used as an oracle: a rate-limit response must not
   reveal whether an account or resource exists.
```

**Acceptance criteria**

- **AC-RF-API-7a — Limits are enforced and metered.** *Given* traffic beyond a configured limit, *when* it is applied, *then* excess is rejected with retry-after and counted per dimension.
- **AC-RF-API-7b — Cost is accounted.** *Given* an expensive query and a trivial one, *when* both are made, *then* they consume budget proportionally to cost, not per-request.
- **AC-RF-API-7c — Credential stuffing is throttled.** *Given* repeated failed logins for one account or from one source, *when* the threshold is crossed, *then* further attempts are throttled, a security event is raised, and the audit records it.
- **AC-RF-API-7d — No enumeration oracle.** *Given* rate-limited requests for an existing and a non-existent account, *when* responses are compared, *then* they are indistinguishable in body, status and timing.

**Done when:** limits are configurable, metered and tested; the auth throttle and its unlock path are exercised.

---

# E — Identity, authentication & session

> **Why Critical.** `services/iam` is a genuine offline IdP for *workload* identities and PQ-signed tokens. There is no *human* authentication anywhere: no login screen in any of the six consoles, no session, no MFA, no step-up, no logout, no revocation. `admin/identity/src/session.rs` models sessions but nothing creates one from a credential.

---

### RF-IAM-1 · Human authentication: credentials, MFA and enrolment

**Severity:** Critical · **Area:** Identity · **Depends on:** RF-PER-4 · **Blocks:** RF-IAM-2..6, RF-UI-1, RF-API-3

> **Status at 2026-10-12 — Partial.** The Evidence is stale: operators log in at `POST /api/v1/login` with a PBKDF2-hashed password plus optional TOTP against a durable KMS-sealed directory, every attempt is written to the ledger, and enrolment while the gateway runs is dual-controlled by two administrators who sign challenges with ML-DSA passkeys held in keystore files. Operators cannot use a security key, browsers cannot produce the ML-DSA passkey assertions, there is no step-up at the point of action on the edge, no recovery flow, and `mg-admin user add` enrols offline as a single actor. Break-glass exists as a library with alerting and time-boxing but is not wired to authentication.
> Criteria met 0 of 6; open: a, b, c, f; not verifiable by reading: d, e. Confidence: high.
>
> **Scope change:** Password plus TOTP login and dual-controlled enrolment exist. The WebAuthn/FIDO2 premise conflicts with the tree's ML-DSA passkeys held in keystore files, which no browser or FIDO2 authenticator can produce. Decide the human second-factor story before rewriting; the console login form has no TOTP field, so TOTP users cannot sign in from it.
>
> **Still to do:**
> - Decide the primary factor: the tree uses ML-DSA passkeys in files, which no FIDO2 authenticator or browser produces; either add standard WebAuthn/COSE verification or rewrite the item
> - Enforce step-up on the edge for high-risk actions (only the admin library models it today)
> - Design a dual-controlled recovery flow and close the single-actor offline `mg-admin user add` path for privileged roles
> - Wire break-glass into the login path and mark actions inside a window in the ledger
> - Add mechanism, source address and correlation id to authentication audit events, and move the hasher to Argon2id
>
> Checked: `services/http-gateway/src/lib.rs:247` · `services/iam/src/auth.rs:82` · `services/http-gateway/tests/login_directory.rs:102` · `services/http-gateway/tests/admin_enrolment.rs:185` · `admin/breakglass/tests/ac_ad_8.rs:20`

**Evidence.** `grep -rln "login\|signIn\|authenticat"` across `products/*/ui/src` matches two files, both of which merely *display* identity data. `services/iam` issues tokens but has no credential verification path. `clients/keystore/src/auth.rs` implements assertion verification for device biometrics — that is the only authentication primitive in the tree, and it is client-side.

**Prompt**

```
Implement human authentication for the enclosure, offline-capable and
phishing-resistant by default.

1. Primary factor: WebAuthn/FIDO2 with platform and roaming authenticators, and
   an enclosure-local relying party (no external service). Reuse the assertion
   verification already implemented in midgard_client_keystore::auth rather than
   writing a second verifier. Passwords, if supported at all for bootstrap, are
   Argon2id-hashed, never reversible, expire, and cannot be used for any
   high-risk action.
2. Second factor and step-up: define which operations require step-up
   (everything dual-controlled, break-glass, key operations, policy commits,
   privilege grants, model promotion, rollout to production rings) and enforce
   step-up at the point of action, not at login.
3. Enrolment and recovery: an operator's first credential is enrolled under dual
   control; recovery from a lost authenticator is a dual-controlled, audited,
   rate-limited process with no self-service reset path for privileged roles.
4. Break-glass authentication: a documented, sealed, dual-custody path for
   emergency access that is time-boxed, alerts on use, and cannot be exercised
   silently — integrating with admin/breakglass, which already models the window
   and the alert.
5. Offline: every mechanism works with no external IdP and no internet. External
   IdP federation (RF-IAM-4) is optional and additive, never required.
6. Audit every authentication event — success, failure, step-up, enrolment,
   recovery, break-glass — with the actor, mechanism, source and outcome.
```

**Acceptance criteria**

- **AC-RF-IAM-1a — A human can authenticate offline.** *Given* an air-gapped enclosure with no external IdP, *when* an enrolled operator authenticates with a security key, *then* a session is established.
- **AC-RF-IAM-1b — Step-up is enforced at the action.** *Given* an authenticated session without recent step-up, *when* a high-risk action is attempted, *then* it is refused pending step-up, and after step-up it proceeds.
- **AC-RF-IAM-1c — No self-service privileged recovery.** *Given* a privileged operator who lost their authenticator, *when* they attempt recovery, *then* it requires dual control and is audited; no path exists that a single actor can complete.
- **AC-RF-IAM-1d — Break-glass is loud and bounded.** *Given* break-glass authentication, *when* used, *then* an alert fires immediately, the window is time-boxed, and every action within it is marked as break-glass in the audit.
- **AC-RF-IAM-1e — Credentials are never recoverable.** *Given* the credential store, *when* inspected, *then* no verifier can be reversed to a credential, and no secret is logged.
- **AC-RF-IAM-1f — Every event is audited.** *Given* any authentication event, *when* the ledger is queried, *then* it records actor, mechanism, source, outcome and correlation id.

**Done when:** an operator can log in to a console on an air-gapped enclosure with a security key; the step-up matrix is implemented and tested.

---

### RF-IAM-2 · Session management, token lifecycle and revocation

**Severity:** Critical · **Area:** Identity · **Depends on:** RF-IAM-1, RF-PER-4

> **Status at 2026-10-12 — Partial.** A login mints a stateless ML-DSA-signed bearer token with a fixed one-hour lifetime; expiry is checked on every request. There is no server-side session record, no refresh, no revocation, no idle timeout, no logout, no session listing and no step-up state, and the console stores the token in `sessionStorage`.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Still to do:**
> - Add durable server-side session records referenced by the token, with idle and absolute timeouts
> - Add revocation of sessions, credentials and identities checked on every request within a configured staleness bound
> - Add rotating refresh tokens with reuse detection over an HttpOnly cookie, and remove the token from `sessionStorage`
> - Add logout, session listing and per-session termination
> - Add a step-up state with its own expiry
>
> Checked: `services/http-gateway/src/lib.rs:58` · `services/http-gateway/src/lib.rs:648` · `services/iam/src/token.rs:215` · `ui/shared/src/AuthGate.tsx:61`

**Prompt**

```
Implement the session lifecycle.

1. Sessions are server-side records (durable per RF-PER-4) referenced by a
   short-lived, PQ-signed token. Include: absolute lifetime, idle timeout,
   binding to the authenticating device/channel, the clearance and purpose in
   force, and the step-up state with its own expiry.
2. Refresh: rotate on every use with reuse detection — a replayed refresh token
   invalidates the whole session family and raises a security event.
3. Revocation must be immediate and global: an administrator revoking a session,
   a credential, or an identity takes effect on the next request everywhere, not
   at token expiry. Implement a revocation check that cannot be cached past a
   bounded, documented staleness, and make that bound a configured security
   parameter.
4. Browser-side storage: tokens in memory plus an HttpOnly, Secure, SameSite
   cookie for the refresh path; never localStorage. CSRF defence via
   double-submit or origin checking; verify against the CSP from RF-API-3.
5. Concurrent sessions: list, attribute (device, source, start time, last use),
   and terminate individually or all-but-current, from the operator's own
   profile and from the Admin console.
6. Logout: terminates server-side, clears client state, and is audited.
```

**Acceptance criteria**

- **AC-RF-IAM-2a — Revocation is immediate.** *Given* an active session, *when* an administrator revokes it, *then* the next request from that session fails, within the documented staleness bound.
- **AC-RF-IAM-2b — Refresh reuse is detected.** *Given* a refresh token used twice, *when* the second use occurs, *then* the entire session family is invalidated and a security event is raised.
- **AC-RF-IAM-2c — Idle and absolute timeouts hold.** *Given* an idle session past the idle timeout, or any session past the absolute lifetime, *when* a request is made, *then* it is rejected and re-authentication is required.
- **AC-RF-IAM-2d — Tokens are not in web storage.** *Given* a console session, *when* localStorage and sessionStorage are inspected, *then* no token is present.
- **AC-RF-IAM-2e — Sessions are visible and terminable.** *Given* multiple sessions, *when* the operator views their profile, *then* all are listed with attribution and can be terminated individually.
- **AC-RF-IAM-2f — Step-up expires independently.** *Given* a step-up performed 30 minutes ago with a 15-minute step-up window, *when* a high-risk action is attempted, *then* step-up is required again while the base session remains valid.

**Done when:** revocation, reuse detection, timeouts and session listing are implemented and tested end to end through a console.

---

### RF-IAM-3 · Authorise the browser edge and bind sessions to policy context

**Severity:** Critical · **Area:** Identity · **Depends on:** RF-IAM-2, RF-API-3

> **Status at 2026-10-12 — Partial.** The server-trust half of the Evidence is stale: the edge builds the policy context only from verified token claims and no handler reads a context from the body, and the SDK no longer sends one on the wire. The SDK still exposes a settable `policy` option whose default is a hard-coded demo operator, the purpose is fixed to `operations` at login with no selection flow, the console chrome does not show subject, clearance or purpose, and there is no forged-context regression test or security event.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** The edge now derives policy context from token claims only. What remains is SDK cleanup, purpose selection, displaying the context in the console chrome, and a regression test.
>
> **Still to do:**
> - Add the regression test: a body asserting higher clearance changes nothing, and record the attempt as a security event
> - Remove `policy` from `HttpGatewayOptions` and make `policy()` return what the server reports (add a whoami route)
> - Let a client request a purpose at login, validate it against entitlements and audit the selection
> - Show subject, clearance and purpose in the shared console chrome from the server's view
>
> Checked: `services/http-gateway/src/lib.rs:824` · `services/http-gateway/src/lib.rs:60` · `sdk/ts/src/http/HttpGateway.ts:41` · `sdk/ts/src/http/HttpGateway.ts:62`

**Evidence.** `sdk/ts/src/http/HttpGateway.ts` accepts an optional `token` and a `policy` context that "defaults to the demo operator" — the browser supplies its own policy context, which the server trusts. That is an authorisation bypass by construction.

**Prompt**

```
Make the server the sole source of policy context.

1. Remove client-supplied policy context from the wire. The subject, roles,
   clearance and purpose in force are derived server-side from the session; a
   client may REQUEST a purpose from those it is entitled to, and the server
   validates and records the selection. A client-asserted clearance is ignored,
   and an attempt to assert one is a security event.
2. Purpose selection becomes an explicit, audited UI act (RF-UX-2): the operator
   chooses a lawful purpose for the working session, it is bound to the session,
   it is displayed persistently in the console chrome, and changing it is
   audited.
3. Every edge route resolves the session, derives the context, and forwards it in
   the RPC envelope (RF-API-1). No handler accepts a context from the body.
4. Update the SDK: remove the `policy` option from HttpGatewayOptions; expose a
   read-only `policy()` that reflects what the server says is in force.
5. Add a regression test asserting that a forged context in a request body has no
   effect on the decision.
```

**Acceptance criteria**

- **AC-RF-IAM-3a — Forged context is inert.** *Given* a request whose body asserts a higher clearance, *when* it is processed, *then* the decision uses the session's clearance, the request is not elevated, and the attempt is recorded as a security event.
- **AC-RF-IAM-3b — Purpose is server-validated.** *Given* an operator requesting a purpose they are not entitled to, *when* they select it, *then* it is refused and audited.
- **AC-RF-IAM-3c — Context is visible.** *Given* an authenticated console, *when* any screen is shown, *then* the subject, clearance and purpose in force are displayed in the chrome and match the server's view.
- **AC-RF-IAM-3d — SDK cannot supply context.** *Given* the SDK's public surface, *when* inspected, *then* there is no way to set a policy context; `policy()` is read-only.

**Done when:** the SDK option is removed; the forged-context regression test is green; purpose selection is a real, audited UI flow.

---

### RF-IAM-4 · Optional federation: OIDC, SAML and SCIM provisioning

**Severity:** Medium · **Area:** Identity · **Depends on:** RF-IAM-1, RF-API-6

> **Status at 2026-10-12 — Partial.** `services/iam/src/federation.rs` has a `FederatedIdp` trait and a reference bridge that trusts an upstream signing ML-DSA assertions, with one positive test; nothing wires it into the gateway. There is no OIDC or SAML relying-party code, no SCIM, and `oidc.rs` explicitly rules the authorize/PKCE leg out of scope. The reference assertion carries roles and clearance from the upstream into the local token, which is the opposite of criterion a.
> Criteria met 0 of 5; open: a, b, c, d; not verifiable by reading: e. Confidence: high.
>
> **Scope change:** `iam/oidc.rs` rules the authorize/PKCE leg out of scope, and the existing federation bridge copies roles and clearance from the upstream assertion into the local token — the opposite of criterion a.
>
> **Still to do:**
> - Change federation so an upstream assertion establishes identity only and roles and clearance are always looked up locally
> - Implement OIDC (state, nonce, PKCE, issuer and redirect allow-lists) and SAML with a negative-test suite, or drop them from scope as `oidc.rs` already does
> - Implement SCIM deactivation and tie it to session revocation (which RF-IAM-2 must provide first)
> - Add dual-controlled, simulatable attribute-to-role mapping
> - Add a test that a fresh install has federation disabled
>
> Checked: `services/iam/src/federation.rs:23` · `services/iam/src/federation.rs:70` · `services/iam/src/oidc.rs:9` · `services/iam/tests/rc_29.rs:200`

**Prompt**

```
Add optional enterprise federation without weakening the offline posture.

1. OIDC and SAML 2.0 as OPTIONAL, off-by-default identity sources. When enabled,
   the enclosure remains the authorisation authority: an external assertion
   establishes identity only; roles, clearance and purpose entitlements are
   always assigned locally. An external IdP can never grant clearance.
2. Harden the implementations against the classic failures: signature and
   audience validation, replay protection, XML canonicalisation and XSW defences
   for SAML, nonce/state validation and PKCE for OIDC, strict issuer and
   redirect-URI allow-listing, and clock-skew bounds tied to RF-RT-4.
3. SCIM 2.0 inbound provisioning for lifecycle: create, update, deactivate. A
   deactivation upstream must revoke sessions immediately (RF-IAM-2). Mapping
   from upstream attributes to local roles is an explicit, reviewed,
   dual-controlled configuration — never inferred.
4. Federation is configured through the Admin console as a dual-controlled,
   simulatable change (an operator can see exactly which local roles the mapping
   would grant, using admin/sim, before committing).
5. Air-gapped deployments run with federation disabled and must lose no
   capability beyond federation itself.
```

**Acceptance criteria**

- **AC-RF-IAM-4a — External assertions never carry authority.** *Given* an OIDC or SAML assertion claiming an administrative role or a clearance, *when* it is consumed, *then* local entitlements alone determine access, and the claim is recorded but not honoured.
- **AC-RF-IAM-4b — Classic attacks fail.** *Given* a replayed assertion, an XSW-mutated SAML response, a mismatched audience, a missing PKCE verifier, or a redirect to a non-allow-listed URI, *when* each is attempted, *then* each is rejected and audited.
- **AC-RF-IAM-4c — Deactivation revokes immediately.** *Given* a SCIM deactivation, *when* it is processed, *then* all sessions for that identity are revoked within the documented bound.
- **AC-RF-IAM-4d — Mapping is simulatable.** *Given* a proposed attribute-to-role mapping, *when* simulated, *then* the report lists exactly which identities gain which local roles before commit.
- **AC-RF-IAM-4e — Disabled by default, air-gap intact.** *Given* a fresh install, *when* configuration is inspected, *then* federation is disabled; *and given* an air-gapped enclosure, *then* every capability except federation is available.

**Done when:** both protocols pass a negative-test suite; SCIM deactivation revokes sessions; mapping simulation works.

---

### RF-IAM-5 · Role model, least privilege and separation of duties at scale

**Severity:** High · **Area:** Identity · **Depends on:** RF-IAM-1

> **Status at 2026-10-12 — Partial.** Unchanged in substance from the Evidence: `admin/identity` holds a five-role admin catalogue defined in Rust code with scoped, expiring bindings, and `admin/privilege` enforces dual control and no self-approval, all tested. There is no platform-wide catalogue file, no role-conflict matrix, no delegated-administration scope check on grants, no recertification campaign and no drift or dormancy report.
> Criteria met 0 of 5; open: a, c, d, e; not verifiable by reading: b. Confidence: medium.
>
> **Still to do:**
> - Move the role catalogue to a reviewed data file covering every subsystem, with a test that fails when enforced capabilities diverge from it
> - Add a separation-of-duties conflict matrix and refuse and audit conflicting grants
> - Add tests proving a scoped administrator cannot grant beyond scope or to themselves
> - Implement recertification campaigns with auto-revocation at the deadline
> - Implement continuous drift and dormancy detection
>
> Checked: `admin/identity/src/role.rs:72` · `admin/identity/tests/ac_ad_1_2.rs:79` · `admin/privilege/tests/ac_ad_3_4_13.rs:202` · `services/http-gateway/src/boot.rs:14`

**Evidence.** `admin/identity` (601 lines) implements roles, capabilities and the no-super-admin separation; `admin/privilege` (612 lines) implements risk-tiered dual control and time-boxed grants; `crypto/policy` implements the PDP. What is missing is the operational layer: role definitions for the whole platform (not just admin), delegated administration, entitlement review, and drift detection.

**Prompt**

```
Extend the role model from the admin panel to the whole platform.

1. Define the complete role catalogue across every subsystem — who may register a
   model, promote an artefact, run a pipeline, edit a picture layer, approve a
   workflow, read audit — as data, in a reviewed catalogue file, with each role's
   capabilities, risk tier, and the separation-of-duties conflicts it has with
   other roles. Enforce the conflicts: a single identity may never simultaneously
   hold two conflicting roles, and an attempt is refused and audited.
2. Implement delegated administration: a scoped administrator can manage
   identities and grants within their scope only, and cannot escalate beyond it
   or grant themselves. Prove the non-escalation property with tests.
3. Implement entitlement review (access recertification): a scheduled campaign
   presenting each owner with their grantees, requiring explicit keep/revoke, with
   auto-revocation of unreviewed entitlements after a deadline. Surface it in the
   Admin console (RF-AD-4).
4. Implement drift and dormancy detection: unused entitlements, expired-but-still-
   present grants, orphaned identities, role assignments that violate the
   conflict matrix. Report continuously, not only at review time.
5. Every grant, revocation, review decision and conflict refusal is audited, and
   every one is simulatable through admin/sim before commit.
```

**Acceptance criteria**

- **AC-RF-IAM-5a — Conflicts are refused.** *Given* an identity holding role A, *when* conflicting role B is granted, *then* the grant is refused, named, and audited.
- **AC-RF-IAM-5b — No self-escalation.** *Given* a scoped administrator, *when* they attempt to grant themselves or anyone a capability beyond their scope, *then* it is refused and audited.
- **AC-RF-IAM-5c — Unreviewed access is revoked.** *Given* a recertification campaign whose deadline passes with entitlements unreviewed, *when* the deadline is reached, *then* those entitlements are revoked and the revocation is audited and notified.
- **AC-RF-IAM-5d — Drift is visible.** *Given* a dormant entitlement or an expired grant still present, *when* the Admin console is opened, *then* it appears in a drift report with age and owner.
- **AC-RF-IAM-5e — Catalogue is the source of truth.** *Given* the role catalogue, *when* compared with enforced capabilities, *then* they agree, verified by a test that fails on divergence.

**Done when:** the catalogue exists and is enforced; recertification runs; drift detection is live in the console.

---

### RF-IAM-6 · Service and machine identity lifecycle

**Severity:** High · **Area:** Identity · **Depends on:** RF-API-2

> **Status at 2026-10-12 — Partial.** `iam::MeshCa` now implements issue, renew and revoke with a CRL carried in the trust anchor, and a test shows a revoked certificate fails its next handshake. The path deployments actually use, `mg-admin mesh issue`, writes a certificate that never expires, with no renewal, no CRL distribution and no attestation; an operator with the state directory issues identities directly. Per-service `policy.json` files are hand-written, not derived from declared dependencies.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** `MeshCa` issue, renew and revoke with a CRL exist and are tested. The gap is that `mg-admin mesh issue` bypasses it and issues certificates that never expire.
>
> **Still to do:**
> - Issue short-lived certificates from `mg-admin mesh issue` (via `MeshCa`) and add automatic renewal in `services/runtime`
> - Make a service drain and stop before its certificate expires when renewal fails
> - Distribute the CRL to running services and to air-gapped sites through the bundle path, with a stated staleness bound
> - Add attested or one-time-token enrolment that fails closed when policy requires attestation
> - Generate each workload's method allow-list from declared dependencies
>
> Checked: `services/iam/src/mesh_ca.rs:106` · `services/iam/tests/rc_35.rs:128` · `tools/mg-admin/src/mesh.rs:115` · `README.md:572`

**Evidence.** `admin/service-identity` (145 lines) and `services/iam` issue and verify workload identities for PQ-mTLS. There is no issuance ceremony for a new node, no certificate rotation, no revocation distribution, and no attestation of what a workload is before it is certified.

**Prompt**

```
Complete the workload identity lifecycle.

1. Enrolment: define how a new node or workload obtains its first identity —
   an attested join (TPM/measured boot where available, an operator-approved
   one-time token otherwise), never a shared secret. Fail closed where
   attestation is required by policy and unavailable.
2. Rotation: short-lived workload certificates with automatic renewal well before
   expiry, and a documented behaviour when renewal fails (drain and stop, never
   continue on an expired identity).
3. Revocation: a revoked workload identity is refused at every peer within a
   bounded, documented staleness; distribute revocation to air-gapped sites via
   the Bifrost bundle path.
4. Least privilege for workloads: each workload identity carries the exact method
   allow-list it needs (RF-API-2's allow-matrix), generated from the service's
   declared dependencies rather than hand-maintained.
5. Audit every issuance, renewal, revocation and refused enrolment.
```

**Acceptance criteria**

- **AC-RF-IAM-6a — Unattested workloads are refused.** *Given* policy requiring attestation, *when* a workload without valid attestation attempts to enrol, *then* it is refused and audited.
- **AC-RF-IAM-6b — Expiry drains, never continues.** *Given* a workload whose renewal fails repeatedly, *when* its certificate approaches expiry, *then* it drains and stops before expiry rather than serving with an invalid identity.
- **AC-RF-IAM-6c — Revocation propagates.** *Given* a revoked workload identity, *when* it calls any peer, *then* it is refused within the documented staleness bound, including at an air-gapped site after bundle transit.
- **AC-RF-IAM-6d — Allow-lists are derived.** *Given* a service's declared dependencies, *when* its identity is issued, *then* its method allow-list matches exactly, and a call outside it is refused.

**Done when:** enrolment, rotation and revocation are implemented and tested, including the air-gapped revocation path.

---

# F — Observability & operations telemetry

> **Why.** There is no metric, no trace and no structured log in the platform. `products/midgard/ui/src/screens/ObservabilityScreen.tsx` renders `SloMetric` fixtures from `sdk/ts/src/mock/fixtures.ts`. An enclosure in production would be operated blind.

---

### RF-OBS-1 · Metrics: instrument every service and every governed decision

**Severity:** High · **Area:** Observability · **Depends on:** RF-RT-1, RF-RT-3

> **Status at 2026-10-12 — Partial.** The workstream premise is stale: `services/telemetry` provides a Prometheus-text metrics registry, and the HTTP edge counts and times every request in one wrapper while the four mesh services export a connection counter on a separate health port. The edge labels metrics with the raw request path (unbounded, caller-controlled), there is no label allow-list, no per-method mesh metrics, no governed-path metrics (policy decisions, audit writes, guardrails, gates), `/metrics` is unauthenticated, there is no SLO document, and the Observability screen still reads fixed data.
> Criteria met 0 of 6; open: a, b, c, e, f; not verifiable by reading: d. Confidence: high.
>
> **Scope change:** Metrics, logs and a tracing module exist in `services/telemetry` (tracing is unused); the item's `services/runtime::metrics` path is wrong. Two defects to fix here: the edge uses the raw request path as a metric label, and `/metrics` is served on the public port.
>
> **Still to do:**
> - Stop using the raw request path as a label; map to the matched route and add a per-metric label allow-list with a test
> - Add per-method rate, error and latency metrics to the mesh `Router` so handlers are instrumented without their own code
> - Instrument policy decisions, audit writes and verification failures, human gates, break-glass, guardrails and inference
> - Authenticate the metrics endpoint and take it off the edge's public port
> - Write the SLO definitions and feed the Observability route from the registry instead of `data.rs`
>
> Checked: `services/http-gateway/src/lib.rs:517` · `services/http-gateway/src/lib.rs:563` · `services/runtime/src/serve.rs:20` · `services/telemetry/tests/telemetry.rs:79` · `services/http-gateway/src/data.rs:215`

**Prompt**

```
Instrument the platform.

1. Add `services/runtime::metrics`: counters, gauges, histograms with bounded
   cardinality (an explicit label allow-list per metric — no unbounded label
   values such as object ids or user input, ever). Expose in an open,
   scrape-friendly text format on a separate, authenticated, non-public port.
2. Instrument uniformly via the RF-API-2 interceptor chain so no handler can be
   uninstrumented: request rate, error rate by typed code, latency histogram,
   in-flight, queue depth, shed-load, deadline exceeded, retry, circuit state.
3. Instrument the governed paths specifically — these are the metrics that make
   this platform's claims measurable:
     - policy decisions by outcome (permit/deny) and by rule
     - audit writes, chain length, verification failures
     - guardrail interceptions by point (pre-model, pre-act, post-model,
       post-run) and by category
     - human gates raised, approved, denied, expired
     - break-glass windows opened, actions within, expiries
     - Evals run, passed, failed, by prompt and by model family
     - inference: tokens in/out, time-to-first-token, batch occupancy, KV-cache
       hit rate, tier selection distribution, fail-closed events
     - supply chain: verifications, rejections by reason
     - storage: capacity, growth, compaction, scrub findings
   Ensure no metric label can leak governed content.
4. Define SLIs and SLOs per service in docs/dev/operations/slos.md — availability,
   latency, correctness — with error budgets, and compute them from real metrics.
5. Feed the Observability console (RF-UID-3) from these metrics, replacing the
   fixtures entirely.
```

**Acceptance criteria**

- **AC-RF-OBS-1a — Every endpoint is instrumented.** *Given* any RPC or route, *when* it is exercised, *then* rate, error and latency metrics exist for it without per-handler code.
- **AC-RF-OBS-1b — Cardinality is bounded.** *Given* a request carrying arbitrary user input, *when* metrics are emitted, *then* no label takes its value from that input, enforced by a test that attempts it and fails.
- **AC-RF-OBS-1c — Governance is measurable.** *Given* the listed governed paths, *when* each is exercised, *then* a corresponding metric moves.
- **AC-RF-OBS-1d — No content leakage.** *Given* the full metric output, *when* inspected after handling classified data, *then* no governed content or identifier appears.
- **AC-RF-OBS-1e — SLOs are computed.** *Given* the SLO definitions, *when* the platform runs, *then* current attainment and remaining error budget are computed from real metrics and visible in the console.
- **AC-RF-OBS-1f — Endpoint is protected.** *Given* the metrics port, *when* accessed without authentication, *then* it is refused; it is never exposed on the public ingress.

**Done when:** the interceptor instruments every path; the Observability screen reads live metrics; the SLO document exists and is computed.

---

### RF-OBS-2 · Distributed tracing across services, agents and the UI

**Severity:** High · **Area:** Observability · **Depends on:** RF-RT-3, RF-API-2

> **Status at 2026-10-12 — Partial.** `services/telemetry/src/trace.rs` implements span contexts, W3C `traceparent` encode/parse and a buffered exporter, with a unit test. No service, client or SDK creates or propagates a span: the module has no users outside its own tests, so nothing is traced.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Read and emit `traceparent` on the HTTP edge and carry it in the mesh request envelope (depends on RF-API-2)
> - Create spans for edge, PDP decision, storage and audit write, and put the trace id on audit entries and log lines
> - Span the Mimir agent loop stages with invariant-check attributes
> - Add always-on sampling for errors, denials and gates
> - Provide an offline trace viewer and a redaction guard for span attributes
>
> Checked: `services/telemetry/src/trace.rs:1` · `services/telemetry/tests/telemetry.rs:136` · `services/runtime/src/client.rs:25` · `services/http-gateway/src/http.rs:64`

**Prompt**

```
Implement end-to-end tracing, including the AI path.

1. Adopt an open trace context propagated over PQ-mTLS RPC metadata, the HTTP
   edge, and into the client SDKs. The trace id equals the correlation id from
   RF-RT-3 so logs, traces, metrics exemplars and audit entries all join.
2. Span the whole path: browser interaction → edge → service → PDP decision →
   storage → audit write, and for AI: agent loop step → prompt assembly layer →
   guardrail pass → model call → tool gate → mutation. Each agent loop step is a
   span whose attributes record the invariant checks (INV-1..8) that ran.
3. Sample adaptively: always trace errors, denials, guardrail blocks, human
   gates, break-glass actions and anything above a latency threshold; sample the
   rest at a configurable rate.
4. Redact by construction: span attributes may never carry governed content or
   secrets; use the `Secret<T>`/`Marked<T>` types from RF-RT-3 so it cannot
   happen accidentally.
5. Provide a local, offline trace viewer in the air-gap bundle — tracing must
   not require an external service.
6. Surface traces in the Mimir Session Traces screen (RF-UID-4) so an operator
   sees the real assembled prompt layers, the real guardrail verdicts, and the
   real gate decisions for a session.
```

**Acceptance criteria**

- **AC-RF-OBS-2a — One id joins everything.** *Given* a console action, *when* its correlation id is used, *then* logs, spans, metric exemplars and the audit entry for that action are all retrievable.
- **AC-RF-OBS-2b — The AI path is fully spanned.** *Given* an agent run that assembles a prompt, is guardrailed, calls a model, proposes a mutation and is gated, *when* its trace is viewed, *then* every one of those stages is a span, and the invariant checks are recorded as attributes.
- **AC-RF-OBS-2c — Errors are always traced.** *Given* a failing, denied or guardrail-blocked request under low sampling, *when* traces are queried, *then* it is present.
- **AC-RF-OBS-2d — No content in spans.** *Given* a trace of a request handling classified data, *when* span attributes are inspected, *then* no governed content or secret appears.
- **AC-RF-OBS-2e — Offline viewing.** *Given* an air-gapped site, *when* an operator investigates, *then* traces are viewable locally with no external dependency.

**Done when:** the AI path is traced end to end; Session Traces renders real spans; the offline viewer ships in the bundle.

---

### RF-OBS-3 · Log and telemetry pipeline, retention and governed access

**Severity:** Medium · **Area:** Observability · **Depends on:** RF-RT-3, RF-PER-6

> **Status at 2026-10-12 — Open.** Services write structured JSON log lines to stdout through `telemetry::Logger` and nothing else: there is no in-enclosure collector, no buffering or disk spill, no governed or audited telemetry query path, no retention policy and no forwarding.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Build or adopt an in-enclosure collector with buffering, backpressure and disk spill
> - Add an authorised, marking-aware, audited query path for logs
> - Define telemetry retention and erasure as policy
> - Add default-off, marking-aware forwarding through the egress path
>
> Checked: `services/telemetry/src/log.rs:1` · `services/runtime/src/serve.rs:33` · `services/telemetry/src/lib.rs:3`

**Prompt**

```
Make telemetry a governed data class, not an escape hatch.

1. Collection: an in-enclosure collector aggregating logs, metrics and traces
   with buffering, backpressure and disk spill, so a collector outage never
   blocks a service or loses data silently.
2. Governance: telemetry is subject to the same marking and clearance rules as
   any other data. Reading logs requires authorisation; reading logs that contain
   markings requires the corresponding clearance; every telemetry query is
   audited. There is no "just look at the logs" bypass.
3. Retention: per RF-PER-6 — telemetry has its own retention period, archival
   path and erasure semantics, configured as policy and simulatable.
4. Egress: forwarding telemetry outside the enclosure goes through the egress
   broker (RF-API-6), is default-off, and is refused for content above a
   configured marking.
5. Air-gap: everything works with no external collector; the offline bundle
   includes the local viewer and query tooling.
```

**Acceptance criteria**

- **AC-RF-OBS-3a — Collector outage is survivable.** *Given* the collector is stopped, *when* services continue to run, *then* they do not block, telemetry is buffered and spilled, and it is delivered on recovery with no silent loss.
- **AC-RF-OBS-3b — Telemetry access is governed.** *Given* an operator without the required clearance, *when* they query logs containing marked content, *then* the marked content is withheld and the query is audited.
- **AC-RF-OBS-3c — Every query is audited.** *Given* any telemetry query, *when* the ledger is inspected, *then* an entry records the actor, the query and the scope returned.
- **AC-RF-OBS-3d — Egress respects markings.** *Given* forwarding enabled, *when* a record above the permitted marking would be sent, *then* it is withheld and the block is recorded.

**Done when:** the collector is deployed with spill; telemetry access is authorised and audited; forwarding is default-off and marking-aware.

---

### RF-OBS-4 · Alerting, on-call and incident response integration

**Severity:** High · **Area:** Observability · **Depends on:** RF-OBS-1, RF-API-6

> **Status at 2026-10-12 — Partial.** What exists is documentation and two signals: `docs/ops/INCIDENT_RESPONSE.md` (severities, on-call, escalation), `docs/ops/RUNBOOKS.md` (RB-1..RB-7), an SLO-breach gauge in `/metrics`, and a break-glass alert bus in `admin/breakglass` with tests. There is no alert catalogue, no alert rules or evaluator, no routing, acknowledgement or suppression, no local alert surface and no check tying alerts to runbooks.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Scope change:** The doc paths this item targets (`docs/dev/operations/`) do not exist; operations documents live in `docs/ops/`.
>
> **Still to do:**
> - Write the alert catalogue with condition, severity, response and runbook per alert, and a script that fails when one lacks a runbook
> - Implement an in-enclosure alert evaluator over the metrics and ledger for the listed security conditions, with a test that induces each
> - Add a local alert surface (console banner or local notifier) that needs no egress
> - Add time-boxed, dual-controlled, audited suppression
> - Write the missing runbooks for security alerts (existing RB-1..7 cover operations only)
>
> Checked: `docs/ops/RUNBOOKS.md:14` · `docs/ops/INCIDENT_RESPONSE.md:15` · `admin/breakglass/tests/rc_32.rs:79` · `services/telemetry/tests/telemetry.rs:79`

**Prompt**

```
Turn signals into a response.

1. Define the alert catalogue in docs/dev/operations/alerts.md: for each alert —
   the condition, the SLO or control it protects, the severity, the expected
   response, the runbook link, and the auto-resolution criteria. Alerts that
   nobody would act on are not created.
2. Security alerts are first-class and must page: guardrail block spike,
   authentication anomaly, forged policy context, signature verification
   failure, audit chain verification failure, break-glass opened, revoked key
   still in use, integrity/reconciliation divergence, storage corruption.
3. Routing through the egress broker with severity-based escalation, on-call
   schedules, acknowledgement and suppression — all configured as
   dual-controlled admin actions, and all audited. Alert suppression is
   time-boxed and expires.
4. Air-gap: local alerting (console banner, local notification, physical signal
   hook) with no dependency on an external pager.
5. Write runbooks for every alert in docs/dev/operations/runbooks/ (RF-DOC-5),
   and add a CI check that every alert has one.
```

**Acceptance criteria**

- **AC-RF-OBS-4a — Every alert is actionable.** *Given* the alert catalogue, *when* checked, *then* each alert has a severity, an expected response and an existing runbook, enforced in CI.
- **AC-RF-OBS-4b — Security conditions page.** *Given* each listed security condition, *when* induced in a test environment, *then* the corresponding alert fires at the stated severity.
- **AC-RF-OBS-4c — Air-gapped alerting works.** *Given* no egress, *when* an alert fires, *then* it is surfaced locally in the Admin console and by the local notification path.
- **AC-RF-OBS-4d — Suppression is bounded and audited.** *Given* a suppressed alert, *when* the suppression expires, *then* it resumes; *and* the suppression and its expiry are audited.

**Done when:** the catalogue and runbooks exist with CI enforcement; each security alert is proven to fire.

---

### RF-OBS-5 · Health, dependency and readiness surfaces per service

**Severity:** Medium · **Area:** Observability · **Depends on:** RF-RT-1

> **Status at 2026-10-12 — Partial.** The four mesh services serve `/healthz`, `/readyz` and `/metrics` on a separate port through `services/runtime`, and the edge serves `/api/v1/health` and `/api/v1/ready`; readiness names each failing dependency and a test proves liveness stays 200 while readiness is 503. There is no `/startupz`, readiness checks are in-process toggles (the trait forbids I/O) apart from Mimir's sidecar health, there is no last-checked time or latency, no version or build provenance on the surface, and the Resilience view is fixed data.
> Criteria met 1 of 4 (a); open: b, c, d. Confidence: high.
>
> **Scope change:** Criterion c needs an Admin console view, and the Admin console has no backend (RF-CA-11, RF-AD-1).
>
> **Still to do:**
> - Add `/startupz` and wire a startupProbe in the chart
> - Make dependency checks real round trips with a timeout and a bounded cached result, reporting last-checked time and latency
> - Give the HTTP edge real readiness checks (it registers one always-true toggle)
> - Expose version, git revision, build digest and signing key id on the health surface
> - Replace the fixed Resilience view with an aggregate of live probes
>
> Checked: `services/runtime/src/health_http.rs:59` · `services/runtime/tests/health_port.rs:29` · `services/telemetry/src/health.rs:20` · `mimir/mimir-server/src/main.rs:86` · `services/http-gateway/src/lib.rs:154`

**Prompt**

```
Make health machine-checkable and honest.

1. Standardise three endpoints per service: /healthz (liveness — the process is
   not wedged), /readyz (readiness — every dependency is usable), /startupz
   (initialisation complete). Never conflate them.
2. Readiness enumerates dependencies with per-dependency status, last-checked
   time and latency: PDP, storage, IdP, trust anchors, KMS, and any peer service
   or model backend the service requires.
3. Dependency checks are real: an actual round trip against the dependency, with
   its own timeout and a cached result bounded by a short, documented TTL. Never
   assume-healthy.
4. Aggregate into a platform health view in the Admin console: every service,
   version, health, dependency graph and current SLO attainment — replacing the
   Resilience screen's fixtures (RF-UID-3).
5. Version and build provenance are exposed on the health surface: version, git
   revision, build digest, signing key id — so an operator can confirm exactly
   what is running and that it was signed (ties to RF-SC-8's revocation sweep).
```

**Acceptance criteria**

- **AC-RF-OBS-5a — The three endpoints are distinct.** *Given* a service whose dependency is down, *when* probed, *then* liveness is healthy and readiness is not-ready with the failing dependency named.
- **AC-RF-OBS-5b — Checks are real.** *Given* a dependency that accepts connections but does not respond, *when* readiness is evaluated, *then* the check times out and reports not-ready.
- **AC-RF-OBS-5c — Platform view is complete.** *Given* the Admin console health view, *when* opened, *then* every deployed service appears with version, health, dependencies and SLO attainment, sourced from live probes.
- **AC-RF-OBS-5d — Provenance is verifiable at runtime.** *Given* a running service, *when* its health surface is read, *then* it reports the build digest and signing key id, and these match the released artefact's attestation.

**Done when:** all services expose the three endpoints; the Admin health view is live; runtime provenance matches attestations.

---
# G — Mimir: real inference

> **Why.** Mimir's control plane is genuinely good: the eight-layer assembly with per-layer budgets and a reject-don't-trim rule, the governed loop enforcing INV-1..8, the four guardrail interception points, the Evals harness with per-family execution, the model router and hardware-aware selection. What it does not have is a model. [`mimir/model-server/src/backends/local.rs:3-11`](../../../mimir/model-server/src/backends/local.rs) is a deterministic string generator "standing in for a real local model"; [`backends/gpu_docker.rs:10-15`](../../../mimir/model-server/src/backends/gpu_docker.rs) commits "the descriptor, the hardware gating, and the exact container launch specification … Actually spawning the sidecar and streaming tokens from it is a GPU-node integration".

---

### RF-MI-1 · Launch and supervise the GPU serving sidecar

**Severity:** High · **Area:** Mimir · **Depends on:** RF-RT-1, RF-DEP-1 · **Blocks:** RF-MI-2..6

> **Status at 2026-10-12 — Partial.** A real sidecar client exists: `SidecarBackend` streams OpenAI-compatible completions from llama.cpp or vLLM over a loopback-only address, fails closed on any error, and `mimir-server --engine llama-cpp|vllm` gates readiness on the sidecar's /health. IBM Granite on llama.cpp (CPU) is wired through compose and a smoke script; vLLM on a GPU has never been run. `GpuDockerBackend::launch` only spawns `docker run` after a digest-pin check: there is no supervision, restart, shutdown, cancellation, VRAM accounting or image-signature verification, and the caller receives one whole completion, not a token stream.
> Criteria met 2 of 6 (b, c); open: d, e, f; not verifiable by reading: a. Confidence: high.
>
> **Scope change:** The sidecar client, loopback-only refusal and fail-closed paths exist, and Granite runs on llama.cpp on CPU. What remains is supervision, cancellation, image-signature verification and an actual GPU run.
>
> **Still to do:**
> - Add sidecar supervision to GpuDockerBackend: readiness wait, restart with backoff, clean shutdown (launch() today only spawns the child)
> - Implement request cancellation that frees the engine slot, with a bounded-time test
> - Verify the sidecar image's signed bundle before launch and audit refusals (today only an '@sha256:' pin check)
> - Run vLLM on a GPU node and pass RC-40.4 (first token under 800 ms); stream tokens to the caller instead of returning one completion
> - Add VRAM accounting/eviction and surface the recorded Selection reason in the Model Plane screen (UI is MimirMock only)
>
> Checked: `mimir/model-server/src/backends/openai_compat.rs:80` · `mimir/model-server/src/backends/openai_compat.rs:938` · `mimir/model-server/src/backends/gpu_docker.rs:90` · `mimir/mimir-server/tests/serve.rs:370` · `deploy/acceptance/deployment/run-deployment-tier.sh:212`

**Prompt**

```
Make GpuDockerBackend actually serve.

1. Implement sidecar lifecycle: take the existing LaunchSpec (keep it exactly as
   the inspectable, testable contract it is) and add spawn, readiness probing,
   supervision, restart-with-backoff, and clean shutdown. The sidecar is
   co-located and reached over LOOPBACK only — preserve that invariant and
   assert it in a test that fails if a non-loopback address is ever used.
2. Implement the client to the serving engine (vLLM/TensorRT-LLM class):
   completion and chat endpoints, token streaming, stop sequences, logprobs
   where available, and cancellation that actually frees the slot.
3. Fail closed, always: if the sidecar is absent, unhealthy, or the model is not
   loaded, serving returns an error. There is NO fallback to a hosted model and
   no silent degradation to the reference engine. Assert this with a test that
   removes the sidecar mid-flight.
4. Preserve the selection logic: the autonomous selector already places a
   backend by HwProfile — extend it so a node with insufficient VRAM degrades to
   a smaller quantisation tier or to the CPU reference engine, and RECORDS which
   it chose and why, surfaced in the Model Plane screen.
5. Resource governance: VRAM accounting, model eviction policy when several
   models compete, and refusal (not thrashing) when a requested model cannot fit.
6. Air-gap: the sidecar image is delivered as a signed Bifrost bundle and
   verified before launch (RF-SC-2). It must never pull an image at runtime.
```

**Acceptance criteria**

- **AC-RF-MI-1a — A model answers on a GPU node.** *Given* a GPU node with a verified model bundle, *when* an inference request is made, *then* the sidecar serves it and tokens stream back.
- **AC-RF-MI-1b — Loopback only.** *Given* the running system, *when* the sidecar connection is inspected, *then* it is loopback; *and* a test configuring a non-loopback address fails closed.
- **AC-RF-MI-1c — No fallback on failure.** *Given* the sidecar is killed mid-request, *when* the request completes, *then* it returns a fail-closed error; no hosted model is contacted and no reference-engine output is substituted.
- **AC-RF-MI-1d — Selection is explained.** *Given* a node with insufficient VRAM for the preferred tier, *when* a model is placed, *then* the chosen tier and the reason are recorded and visible in the Model Plane screen.
- **AC-RF-MI-1e — Unverified image never launches.** *Given* a sidecar image whose signature does not verify, *when* launch is attempted, *then* it is refused and audited.
- **AC-RF-MI-1f — Cancellation frees resources.** *Given* a cancelled long generation, *when* it is cancelled, *then* the slot and its KV-cache are released within the stated bound.

**Done when:** a real model answers a grounded question on a GPU node with no network; every fail-closed path is tested.

---

### RF-MI-2 · Model loading, tokenizers and per-family adapters

**Severity:** High · **Area:** Mimir · **Depends on:** RF-MI-1, RF-MS-1

> **Status at 2026-10-12 — Partial.** Weights are now really read: a model directory loads only if its ML-DSA-signed manifest verifies and the weights file streams to the signed size, SHA-256 and a GGUF/safetensors magic sniff, and `mimir-server` refuses to start otherwise. Chat templates exist for GLM, Nemotron, Gemma and Granite and are applied by the sidecar client. There is still no tokenizer (assembly budgets use `WordCounter`), no header/tensor-inventory parse, no byte-exact golden template tests, no sampling parameters and no Eval run against a real model.
> Criteria met 1 of 6 (a); open: b, c, d, e, f. Confidence: high.
>
> **Scope change:** Signed-manifest and weight verification now gate serving. Narrow this to tokenizer handling, byte-exact golden templates, sampling and header parsing, and add Granite as a fourth family.
>
> **Still to do:**
> - Implement a real per-family tokenizer and exact token counter and replace WordCounter in the assembly pipeline
> - Add byte-exact golden template tests per family (current tests only assert substrings) and verify templates against the upstream chat templates
> - Add a test that scans mimir/prompts for family-specific tokens (the crate has no tests)
> - Add sampling parameters (temperature, top-p, top-k, repetition penalty, seed) to the request contract and the sidecar call, with a seeded-determinism test
> - Parse GGUF/safetensors headers (tensor inventory, quantisation detection), audit refused loads, and run the Eval suite against real models per family
>
> Checked: `mimir/model-server/src/artifact.rs:346` · `mimir/model-server/src/artifact.rs:437` · `mimir/model-server/tests/model_dir.rs:105` · `mimir/model-server/src/adapter.rs:55` · `mimir/agent-runtime/src/assembly.rs:46`

**Evidence.** `mimir/model-server/src/adapter.rs`, `quant.rs`, `artifact.rs` and `model_registry.rs` model families, quantisations and artefacts as metadata. There is no tokenizer, no weight file reader, and no chat-template application — the "Model Abstraction Layer" that makes prompts portable across GLM/Nemotron/Gemma exists as a type-level seam only.

**Prompt**

```
Implement the Model Abstraction Layer for real.

1. Artefact loading: read and validate real weight formats (GGUF and safetensors
   at minimum) — header parse, tensor inventory, quantisation detection, and a
   digest check against the registry entry BEFORE any load. A mismatch fails
   closed (this is the AC-MS-2 path; call mg_verify::verify, do not re-implement).
2. Tokenizers: load the family's real tokenizer from the artefact; implement
   encode/decode, special-token handling, and an exact token counter. Replace
   agent-runtime's WordCounter with the real counter for budget enforcement —
   the per-layer token budgets are only meaningful against real tokens.
3. Chat templating per family: system-message placement, role markers, stop
   tokens, and BOS/EOS handling for GLM, Nemotron and Gemma. Prompts stay
   family-agnostic (that is the FR-MS-7/8 contract) — all family specifics live
   in the adapter, and a test asserts no prompt text in mimir/prompts contains a
   family-specific token.
4. Golden tests per family: a fixed prompt renders to a byte-exact expected
   template; a fixed string tokenizes to an expected id sequence. These catch
   adapter drift, which is otherwise silent and corrupts every downstream Eval.
5. Sampling: temperature, top-p, top-k, repetition penalty, seed — with seeded
   determinism so Evals are reproducible.
6. Run the full Eval suite per family against the real models and record the
   per-family certification, as FR-MS-7 requires.
```

**Acceptance criteria**

- **AC-RF-MI-2a — Real artefacts load, bad ones do not.** *Given* a valid signed GGUF, *when* loaded, *then* it serves; *given* a digest mismatch or corrupt header, *then* the load fails closed and is audited.
- **AC-RF-MI-2b — Token budgets use real tokens.** *Given* an assembled prompt, *when* budgets are enforced, *then* the count comes from the family tokenizer, and a prompt exceeding a layer budget is rejected (not trimmed), as today.
- **AC-RF-MI-2c — Templating is family-exact.** *Given* the golden template tests for each family, *when* run, *then* rendered prompts are byte-identical to the expected output.
- **AC-RF-MI-2d — Prompts stay portable.** *Given* `mimir/prompts`, *when* scanned, *then* no family-specific token or template syntax appears.
- **AC-RF-MI-2e — Evals certify per family.** *Given* each supported family, *when* the Eval suite runs against the real model, *then* results are recorded per family and a failure blocks that family's certification.
- **AC-RF-MI-2f — Sampling is reproducible.** *Given* a fixed seed and parameters, *when* the same prompt is run twice, *then* the outputs are identical.

**Done when:** real weights load and tokenize; golden template tests exist per family; per-family Eval certification is recorded.

---

### RF-MI-3 · Serving performance: batching, KV-cache, scheduling and admission

**Severity:** Medium · **Area:** Mimir · **Depends on:** RF-MI-1, RF-MI-2

> **Status at 2026-10-12 — Partial.** The sidecar client measures first-token latency at the boundary and reads real token usage and llama.cpp's prompt-cache hit from the stream, so a real engine reports measured values. Nothing else has moved: the reference engine still carries unlabelled synthetic constants (`BASE_MS`, `PER_TOKEN_MS`), there is no admission control, priority, deadline or fairness logic, no benchmark, no docs/dev/operations/performance.md, and no CI configuration in the tree. The Model Plane screen is backed by `MimirMock`.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Add a bounded admission queue with priority classes, per-tenant fair sharing and deadline-aware refusal in front of Server::infer
> - Mark reference-engine latencies as synthetic in the Completion contract so they cannot be reported as measurements
> - Benchmark TTFT and tokens/s per family, quantisation and hardware profile; publish docs/dev/operations/performance.md and gate regressions
> - Measure prefix-cache gain against a no-cache baseline on a real engine
> - Feed measured occupancy, cache-hit and latency to a live Model Plane screen (currently mock data)
>
> Checked: `mimir/model-server/src/backends/openai_compat.rs:294` · `mimir/model-server/src/backends/openai_compat.rs:309` · `mimir/model-server/src/backends/local.rs:23` · `mimir/model-server/tests/serving.rs:51` · `sdk/ts/src/products/mimir.ts:439`

**Evidence.** `mimir/model-server/src/serve.rs`, `handler.rs` and `router.rs` model continuous batching and KV-cache accounting; `backends/local.rs` computes synthetic latencies (`BASE_MS = 12.0`, `PER_TOKEN_MS = 0.4`). No real scheduling exists.

**Prompt**

```
Make the serving plane real and measured.

1. Wire the existing batching and KV-cache accounting to the real engine's
   actual state rather than modelled values — real occupancy, real cache hit
   rate, real queue depth. Where the engine exposes them, read them; where it
   does not, measure them at the boundary.
2. Admission control and fairness: bounded queue with a documented full policy,
   priority classes (interactive operator request vs background Eval vs batch
   job), per-tenant fair sharing (RF-SCL-1), and deadline-aware scheduling that
   rejects rather than silently queues past a caller's deadline.
3. Prefix caching for the shared system-prompt prefix — the eight-layer assembly
   means layers 1–2 are often identical across requests, which is exactly the
   case prefix caching serves. Measure the gain.
4. Publish the real performance envelope: time-to-first-token and tokens/second
   per family, per quantisation, per hardware profile, at several concurrency
   levels. Record in docs/dev/operations/performance.md and gate regressions.
5. Replace the synthetic latency constants in the reference engine with values
   clearly labelled as synthetic, and make it impossible for a synthetic value to
   be reported as a measurement in the Model Plane screen.
```

**Acceptance criteria**

- **AC-RF-MI-3a — Metrics are measured, not modelled.** *Given* the Model Plane screen, *when* it shows occupancy, cache hit rate and latency, *then* those values come from the real engine, and any synthetic value is labelled as such in the UI.
- **AC-RF-MI-3b — Deadlines are honoured or refused.** *Given* a request whose deadline cannot be met at current queue depth, *when* it is submitted, *then* it is refused immediately rather than queued to failure.
- **AC-RF-MI-3c — Fairness holds.** *Given* one tenant saturating the queue, *when* another submits, *then* the second receives its fair share within the documented bound.
- **AC-RF-MI-3d — Prefix caching helps measurably.** *Given* requests sharing the assembled prefix, *when* served, *then* time-to-first-token improves against the no-cache baseline by a recorded margin.
- **AC-RF-MI-3e — Envelope is published and gated.** *Given* the benchmark, *when* a change regresses latency beyond tolerance, *then* CI fails.

**Done when:** the Model Plane shows measured values; the envelope is published; the regression gate is live.

---

### RF-MI-4 · Real tool execution in the governed loop

**Severity:** High · **Area:** Mimir · **Depends on:** RF-API-2, RF-MI-1

> **Status at 2026-10-12 — Partial.** The governed loop's `ActionExecutor` is implemented for the real `fabric::Fabric` and tests show a human-approved action changing fabric state with gate and execution audited, and an irreversible action forcing a human gate even when pre-authorised. That is in-process only: `agent-runtime` is a library that no service runs, there are no Forge or Bifrost executors, no per-turn tool injection (an unknown action type is treated as irreversible and human-gated, not refused), no undo/compensation, only a proposed-action-count budget with no test, and no sandbox.
> Criteria met 2 of 6 (a, c); open: b, d, e, f. Confidence: medium.
>
> **Scope change:** `ActionExecutor` is implemented for the real `fabric::Fabric` and tested. The gap is that no service hosts the governed loop, and there are no Forge or Bifrost executors.
>
> **Still to do:**
> - Derive the per-turn tool set from policy and refuse (not human-gate) any action type the turn did not inject, with a test
> - Add executors for Forge runs and Bifrost promotions over the mesh, carrying the agent's scoped identity, and host the loop in a running service
> - Declare a compensating action per reversible action and implement audited undo
> - Add wall-clock, token and cost budgets and a test that step-budget exhaustion leaves nothing half-applied (HaltReason::StepBudgetExhausted is untested)
> - Sandbox code-executing tools (no network, file allow-list, limits, timeout) and raise a security event on escape; make the human decision record name the irreversibility
>
> Checked: `mimir/agent-runtime/src/govloop.rs:60` · `mimir/agent-runtime/src/govloop.rs:626` · `mimir/agent-runtime/src/govloop.rs:645` · `mimir/agent-runtime/tests/ac_mi_3.rs:215` · `mimir/agent-runtime/tests/ac_mi_3.rs:260`

**Evidence.** `mimir/agent-runtime/src/govloop.rs` (the governed loop) is thorough — gating, pre-authorisation, human decisions, healing, audit sequences, step budgets. `ActionExecutor` is a trait; the concrete executors are test doubles. The agent can propose and be gated, but there is no production executor invoking real subsystem operations.

**Prompt**

```
Connect the governed loop to real capabilities.

1. Implement a production ActionCatalog and ActionExecutor backed by the RF-API-2
   RPC clients, so a gated, approved action performs a real fabric action, Forge
   run, Bifrost promotion or Heimdall workflow step. Every executor call carries
   the agent's scoped identity, not the operator's ambient authority.
2. Least privilege per turn (INV-2): the tool schemas injected for a turn are
   derived from policy for THIS subject, purpose and task — never a static
   catalogue. A tool absent from the turn's injection must be unreachable even if
   the model names it; add a test that the model naming an uninjected tool
   produces a refusal, not an execution.
3. Reversibility (INV-5): every executor declares whether its effect is
   reversible and, if so, supplies the compensating action. Irreversible actions
   require explicit human sign-off naming the irreversibility. Implement undo for
   reversible actions and expose it in the UI.
4. Budgets (INV-8): wall-clock, token, step and cost budgets per run, with
   exhaustion halting to a human — and the partial work left in a clean,
   documented state, never half-applied.
5. Sandboxing: any tool that executes code (Forge transforms, projections) runs
   with no network, a filesystem allow-list, memory and CPU limits, and a hard
   timeout. Escape attempts are security events.
6. Audit every step with the correlation id so the Session Traces screen can
   reconstruct the run exactly (RF-UID-4).
```

**Acceptance criteria**

- **AC-RF-MI-4a — Approved actions really happen.** *Given* a gated, human-approved action, *when* the loop executes it, *then* the target subsystem state changed and the audit records both the gate and the effect.
- **AC-RF-MI-4b — Uninjected tools are unreachable.** *Given* a model output naming a tool not injected this turn, *when* the loop processes it, *then* it refuses, records the refusal, and executes nothing.
- **AC-RF-MI-4c — Irreversible needs sign-off.** *Given* an irreversible action, *when* it is proposed, *then* execution requires an explicit human decision that names the irreversibility; a pre-authorised scope alone is insufficient.
- **AC-RF-MI-4d — Undo works.** *Given* an executed reversible action, *when* undo is invoked, *then* the compensating action runs and both are audited.
- **AC-RF-MI-4e — Budget exhaustion is clean.** *Given* a run that exhausts its step or token budget mid-plan, *when* it halts, *then* no action is half-applied and the state is the documented one.
- **AC-RF-MI-4f — Sandbox holds.** *Given* a transform attempting network access or an out-of-allow-list file read, *when* it runs, *then* it is refused, the run fails, and a security event is raised.

**Done when:** the production executor is wired for at least fabric, Forge and Bifrost; every criterion above has a test.

---

### RF-MI-5 · Memory, sessions and multi-turn context

**Severity:** Medium · **Area:** Mimir · **Depends on:** RF-PER-1, RF-MI-2

> **Status at 2026-10-12 — Open.** No session store or memory store exists. Assembly layers 4 (memory) and 6 (history) are still plain `Vec<String>` inputs the caller supplies, and the A.5 distiller's `LearningState` is an in-memory struct with no persistence, clearance marking, expiry or tenant scoping.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Build a durable, sealed session store keyed by (tenant, subject, agent, session) with expiry, listing and audited deletion, feeding layer 6
> - Build a durable, sealed memory store with provenance, clearance marking and expiry, filtered by the reader's clearance before assembly
> - Make the A.5 distiller the only write path into memory and test that a direct write fails
> - Add cross-subject and cross-tenant isolation tests
> - Expose memory provenance and revocation through a live Feedback screen (currently mock data)
>
> Checked: `mimir/agent-runtime/src/assembly.rs:64` · `mimir/agent-runtime/src/assembly.rs:253` · `mimir/agent-runtime/src/learn.rs:215` · `mimir/agent-runtime/tests/self_learning.rs:16`

**Evidence.** Assembly layer 4 is "Agent memory — scoped prior facts" from a "Memory store (PQ)" and layer 6 is "Task history" from a "Session store". Neither store exists; both are inputs the caller supplies.

**Prompt**

```
Implement the memory and session stores the assembly pipeline assumes.

1. Session store: durable, PQ-sealed multi-turn history scoped to
   (subject, agent, session), with the assembly's ordering and budget rules
   applied on read. Sessions expire, are listable, and are deletable by their
   owner and by an administrator, with deletion audited.
2. Memory store: durable, PQ-sealed scoped facts with explicit provenance (which
   run wrote it, from which grounded source), a clearance marking, and an
   expiry. Memory is retrieved under the reader's clearance — a fact learned at
   a higher clearance is invisible to a lower-clearance reader, and the retrieval
   filter is applied before assembly, not after.
3. Poisoning resistance: memory writes come only from the A.5 feedback distiller
   path, which already rejects poisoned and regressive feedback; nothing else may
   write memory. Every write records provenance and is reviewable and revocable
   in the Feedback screen.
4. Isolation: memory and sessions are strictly scoped by subject and tenant; add
   a test that cross-subject and cross-tenant retrieval returns nothing.
5. Budgets: memory retrieval is bounded by the layer budget and by relevance; an
   over-budget retrieval rejects rather than trims (consistent with the existing
   assembly rule).
```

**Acceptance criteria**

- **AC-RF-MI-5a — Multi-turn context persists.** *Given* a session with prior turns, *when* the service restarts and the session continues, *then* prior turns are present and assembled at layer 6.
- **AC-RF-MI-5b — Clearance filters memory.** *Given* a fact recorded at a high marking, *when* a lower-clearance subject runs an agent, *then* the fact is not retrieved and does not influence output.
- **AC-RF-MI-5c — Only the distiller writes memory.** *Given* the source tree, *when* memory write paths are inspected, *then* only the A.5 distiller path can write; a test attempting a direct write fails.
- **AC-RF-MI-5d — No cross-subject leakage.** *Given* two subjects and two tenants, *when* each runs agents, *then* neither retrieves the other's memory or session content.
- **AC-RF-MI-5e — Memory is reviewable and revocable.** *Given* stored memory, *when* the Feedback screen is opened, *then* each fact shows its provenance and can be revoked, with the revocation audited.

**Done when:** both stores are durable and clearance-aware; the isolation and provenance tests are green.

---

### RF-MI-6 · Evals as a continuous, gating governance system

**Severity:** High · **Area:** Mimir · **Depends on:** RF-MI-2, RF-OBS-1

> **Status at 2026-10-12 — Open.** Evals are still unit tests over deterministic stand-ins. A `FamilyEvalReport` now records each (eval, family) cell separately and is tested with three distinct mock family models, but no Eval has run against a real model, there is no Eval service, result store, baseline, regression detection or promotion gate in `ModelRegistry`, the repository holds no CI configuration, and the Evals screen is mock-backed.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Still to do:**
> - Build an Eval runner that drives the suites through mimir-server against a real model and persists results with model, family, quantisation, prompt and adapter versions
> - Make ModelRegistry refuse promotion to a loadable state unless critical Evals passed for every routed family
> - Add a CI job that runs the affected Evals per family on any change under mimir/prompts
> - Record baselines and block promotion on regression beyond a documented tolerance, with an alert
> - Grow the adversarial and benign corpora, set a false-positive budget per guardrail (only A.3 has one, over five inputs), and make the Evals screen live
>
> Checked: `mimir/agent-runtime/src/evals.rs:511` · `mimir/agent-runtime/tests/eval_matrix.rs:82` · `mimir/agent-runtime/src/evals.rs:287` · `mimir/model-server/src/model_registry.rs:32` · `deploy/acceptance/verify-mimir.sh:116`

**Evidence.** `mimir/agent-runtime/src/evals.rs` implements the A.1–A.5 starter suites with `per_family` execution and `MODEL_FAMILIES`; `admin/ai-assist/src/evals.rs` and `clients/inference/src/evals.rs` add theirs. They run as unit tests against deterministic doubles. There is no continuous execution against real models, no result store, no trend, and no gate on promotion.

**Prompt**

```
Turn Evals from a test suite into the governance mechanism the SADs describe.

1. Eval service: run the full library (all 15 prompts' suites, ~50 Evals) against
   real models on a schedule and on every model, prompt or adapter change.
   Persist results with the model id, family, quantisation, prompt version,
   adapter version and hardware profile.
2. Gate promotion: a model may not be promoted to serving, and a prompt change
   may not ship, unless every critical Eval passes for every family it will be
   routed to. Enforce it in the model registry and in CI, not by convention.
   INV-4 already says a failing critical Eval halts the loop at runtime — this
   extends the same rule to release time.
3. Regression detection: compare against the recorded baseline; a statistically
   significant regression blocks promotion and alerts. Handle nondeterminism
   honestly — fixed seeds where possible, and a documented tolerance with
   repeated sampling where not.
4. Expand coverage beyond the starter suites: adversarial prompt-injection
   corpora for A.3 (including obfuscation and multi-turn), grounding-fidelity and
   citation-accuracy for A.1/A.2/A.8, refusal-correctness and false-positive rate
   for every guardrail, mutation-as-proposal for A.1, and tier-parity for the
   client prompts. Record a false-positive budget per guardrail and treat
   exceeding it as a failure — an over-blocking guardrail is also a defect.
5. Surface it: the Evals screen shows real current results, per family, with
   history, trend, and which promotions are blocked and why (RF-UID-4).
```

**Acceptance criteria**

- **AC-RF-MI-6a — Evals run against real models.** *Given* a registered model, *when* the Eval service runs, *then* results are recorded per family with full provenance.
- **AC-RF-MI-6b — Failing Evals block promotion.** *Given* a model failing a critical Eval for a family it would be routed to, *when* promotion is attempted, *then* it is refused and the reason is recorded.
- **AC-RF-MI-6c — Prompt changes are gated.** *Given* a PR changing a library prompt, *when* CI runs, *then* the affected Evals run per family and a failure blocks merge.
- **AC-RF-MI-6d — Regressions are detected.** *Given* a change degrading grounding fidelity beyond tolerance, *when* Evals run, *then* the regression is detected, promotion is blocked, and an alert fires.
- **AC-RF-MI-6e — Over-blocking is a failure.** *Given* a guardrail whose false-positive rate exceeds its budget, *when* Evals run, *then* it fails.
- **AC-RF-MI-6f — Results are visible.** *Given* the Evals screen, *when* opened, *then* it shows real results, per family, with history and blocked promotions.

**Done when:** the Eval service runs on schedule and in CI; promotion gating is enforced in the registry; the Evals screen is live.

---

### RF-MI-7 · Guardrail depth and adversarial resistance

**Severity:** High · **Area:** Mimir · **Depends on:** RF-MI-6

> **Status at 2026-10-12 — Partial.** The input guardrail has a deterministic pre-filter with de-obfuscation plus a `ModelBackedGuardrail` that combines it with a `Classifier` seam, and tests cover a handful of obfuscated and paraphrased injections and a real sanitise rewrite. The classifier behind the seam is still a test double (no model-plane classifier), the corpora are a few hand-written strings with no block-rate target, there is no indirect-injection test over grounded content, no pre-act intent check, and citation validity is an Eval (`ev_a1_ground`) whose loop-level halt has no test.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: medium.
>
> **Still to do:**
> - Implement a Classifier that calls the local model plane and define the agreement rules (deterministic block wins, disagreement sanitises)
> - Create adversarial (obfuscated, translated, multi-turn) and benign corpora with a stated block-rate target and false-positive budget, run in the acceptance script
> - Add an indirect-injection test where grounded fabric content carries instructions and assert the layer boundary holds
> - Add a loop-level test that an output citing an id outside the injected grounding is rejected, and a pre-act check that escalates over-broad actions
> - Give the Guardrails screen a real probe path (currently MimirMock)
>
> Checked: `mimir/agent-runtime/src/defend.rs:317` · `mimir/agent-runtime/tests/guardrail_paraphrase.rs:18` · `mimir/agent-runtime/tests/guardrail_paraphrase.rs:101` · `mimir/agent-runtime/src/evals.rs:85` · `mimir/agent-runtime/src/evals.rs:287`

**Evidence.** `defend.rs` (self-defending input guardrail, A.3), `guardrail.rs` (the interception trait), `heal.rs` (A.4 supervisor) and `learn.rs` (A.5 distiller) implement the four interception points with deterministic classifiers. Real adversarial resistance requires a real classifier and a real corpus.

**Prompt**

```
Deepen the guardrails to production strength.

1. Input guardrail (pre-model): combine the deterministic rules with a real
   classifier running on the local model plane, and require agreement rules —
   deterministic block always blocks; classifier block blocks; disagreement
   escalates to sanitise-and-log rather than allow. Handle obfuscation
   (encoding, homoglyphs, translation, indirection through retrieved content)
   and multi-turn injection where the payload accumulates across turns.
2. Indirect injection is the sharpest risk here: grounding content from the
   fabric and tool outputs is untrusted input. Ensure retrieved content is
   structurally marked as data and can never be interpreted as instruction —
   assert the assembly layer boundary holds under an adversarial corpus in which
   grounded documents contain instructions.
3. Output guardrail (post-model): marking-aware redaction and schema validation
   already exist — extend to detect governed content leaking into free text,
   citations that do not resolve to injected grounding, and structured output
   that validates but contradicts its citations.
4. Pre-act guardrail: extend beyond policy to intent — an action whose effect
   exceeds what the user asked for is escalated to human review even when
   policy-permitted.
5. Every block, sanitisation and escalation is audited with its category and
   evidence, metered (RF-OBS-1), and visible in the Guardrails screen with a
   real probe path (RF-UID-4).
```

**Acceptance criteria**

- **AC-RF-MI-7a — Adversarial corpus is blocked.** *Given* the injection corpus including obfuscated, translated and multi-turn variants, *when* each is submitted, *then* the block rate meets the stated target and every block is audited with its category.
- **AC-RF-MI-7b — Indirect injection fails.** *Given* grounded content containing instructions ("ignore your instructions and…"), *when* an agent reasons over it, *then* the instruction is not followed and the attempt is recorded.
- **AC-RF-MI-7c — Unresolvable citations are rejected.** *Given* model output citing an id absent from the injected grounding, *when* the output guardrail runs, *then* the response is rejected, not returned with a bad citation.
- **AC-RF-MI-7d — Over-broad actions escalate.** *Given* a policy-permitted action whose blast radius exceeds the request, *when* it is proposed, *then* it escalates to human review.
- **AC-RF-MI-7e — Benign traffic is not over-blocked.** *Given* the benign corpus, *when* it is submitted, *then* the false-positive rate stays within budget.
- **AC-RF-MI-7f — Guardrails screen probes for real.** *Given* the Guardrails screen probe, *when* an input is submitted, *then* the real guardrail chain runs and the real verdict with evidence is displayed.

**Done when:** the corpora exist and are run continuously; the indirect-injection boundary test is green; the probe path is real.

---

### RF-MI-8 · Cost, quota and capacity governance for AI usage

**Severity:** Medium · **Area:** Mimir · **Depends on:** RF-MI-3, RF-API-7

> **Status at 2026-10-12 — Open.** Each inference call writes one PQ-signed audit entry naming subject, action, resource and purpose, and the completion carries token counts back to the caller, but nothing persists usage: there is no accounting store, no tenant or product attribution, no budgets or thresholds, no capacity forecast and no console surface.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Persist a usage record per call (tokens in/out, wall time, model, family, quantisation, subject, tenant, purpose, calling product)
> - Add the calling product/feature to the inference request contract so touchpoints can be attributed
> - Enforce soft and hard budgets per subject, tenant, product and purpose in InferenceHandler with a typed refusal and no model downgrade
> - Expose consumption, remaining budget and GPU headroom in the Admin console and console chrome
>
> Checked: `mimir/model-server/src/handler.rs:108` · `mimir/model-server/src/handler.rs:158` · `mimir/model-server/src/backends/openai_compat.rs:326` · `mimir/model-server/tests/inference_pqmtls.rs:191`

**Prompt**

```
Account for and govern AI consumption.

1. Account every inference: tokens in/out, wall time, GPU seconds, model, family,
   quantisation, subject, tenant, purpose, and the calling product. Persist it.
2. Budget and enforce: per subject, per tenant, per product and per purpose, with
   soft warning and hard fail-closed thresholds. Exceeding a hard budget refuses
   with a typed, actionable error — never a silent downgrade to a weaker model.
3. Capacity planning: forecast GPU demand from usage trends and surface it in the
   Admin console with the headroom before the next hardware decision.
4. Attribute the AI touchpoints individually so an operator can see which feature
   (Forge NL-to-transform, Bifrost risk analysis, Heimdall options, Admin
   copilot, client assistants) consumes what.
5. Expose the operator's own consumption and remaining budget in the console
   chrome, so a limit is never a surprise.
```

**Acceptance criteria**

- **AC-RF-MI-8a — Every call is accounted.** *Given* any inference from any touchpoint, *when* accounting is queried, *then* the call appears with full attribution.
- **AC-RF-MI-8b — Hard budgets fail closed.** *Given* an exhausted hard budget, *when* an inference is requested, *then* it is refused with an actionable error and no weaker model is silently substituted.
- **AC-RF-MI-8c — Warnings precede refusal.** *Given* consumption crossing the soft threshold, *when* it does, *then* the operator sees a warning in the console before any refusal occurs.
- **AC-RF-MI-8d — Attribution by feature.** *Given* the Admin console, *when* AI usage is viewed, *then* consumption is broken down by product feature and purpose.

**Done when:** accounting is persistent; budgets are enforced; the console shows consumption and headroom.

---

# H — Model supply chain

> **Status.** `mimir/staging` (1 158 lines) implements intake, provenance and licence/risk assessment (A.9), and `model-server`'s registry models artefacts, families and quantisations. The gap is that nothing has ever transited it: there is no real upstream ingest, no weight handling, and the re-sign step operates on metadata.

---

### RF-MS-1 · End-to-end intake of a real upstream model

**Severity:** High · **Area:** Model supply · **Depends on:** RF-SC-1, RF-SC-2, RF-PER-4

> **Status at 2026-10-12 — Partial.** One real model (IBM Granite 3.3 2B GGUF) has made a shortened journey: `mg-model sign` checks the upstream-published SHA-256, signs a manifest with an ML-DSA key, `mimir-server` re-verifies manifest and weights before serving on CPU, and `mg-model pack`/`unpack` carry a verified model across the air gap as a signed bundle. That path bypasses `mimir/staging` (still driven by an in-memory `ReferenceFetcher`), runs no malware scan, no A.9 assessment, no human approval record, no ModelRegistry entry and no Eval certification, uses a development signing key, and has not run on a GPU.
> Criteria met 3 of 6 (c, e, f); open: a, b, d. Confidence: medium.
>
> **Scope change:** A real model has been signed, verified, served and bundled — but through `mg-model`, bypassing `mimir/staging`, the registry and Evals. This item is now about joining those two paths.
>
> **Still to do:**
> - Join the two halves: make staging intake read a real distribution from disk (weights, tokenizer, config, licence, model card) and emit the manifest mg-model/mimir-server load
> - Add the enclave scan to intake: refuse pickle and other executable formats at intake with an audit entry (today refusal happens at sign/load, unaudited), plus tensor-inventory sanity and malware scan
> - Carry the unverifiable-upstream risk, SBOM and human approval onto the registry entry and surface them at promotion
> - Have mimir-server load through ModelRegistry with Eval-certification status, and run the journey on a GPU node
> - Write the intake-to-serving runbook and replace the development model-signing key flow
>
> Checked: `deploy/compose/stage-model.sh:61` · `tools/mg-model/tests/bundle.rs:87` · `mimir/model-server/tests/model_dir.rs:171` · `mimir/staging/tests/ac_ms_1.rs:185` · `mimir/staging/tests/ev_a9.rs:93`

**Prompt**

```
Take a real upstream model all the way through staging to serving.

1. Ingest: accept a real model distribution (weights, tokenizer, config, licence,
   model card) from removable media or a staging drop. Verify the upstream
   digest where published, record it, and record its absence explicitly where it
   is not — an unverifiable upstream is a recorded risk, never an assumption.
2. Scan in the enclave: format validation, deserialisation safety (reject pickle
   and any format permitting code execution — safetensors/GGUF only), size and
   tensor-inventory sanity, and a malware scan of the whole distribution.
3. Assess: run the A.9 licence/risk assessor over the real licence and model
   card. It captures restrictions and marks unknowns — it MUST NOT issue a
   verdict (that is the EV-A9-noverdict contract). A human makes the decision on
   its evidence.
4. Re-sign: on approval, re-sign the artefact with the enclosure's own ML-DSA key
   via mg-sign, producing the SBOM (listing the model's own components and
   provenance), the signature and the attestation. From this point the enclosure
   trusts its own signature, not the upstream's.
5. Register and promote: record in the model registry with family, quantisation,
   hardware requirements, licence restrictions, and the Eval certification
   status. Promotion to serving requires passing Evals per family (RF-MI-6).
6. Air-gap transit: the approved bundle moves to disconnected sites via the
   Bifrost signed-bundle path (the AC-MS-3 route), verified offline before load.
```

**Acceptance criteria**

- **AC-RF-MS-1a — A real model completes the journey.** *Given* a real upstream distribution, *when* it passes intake, scan, assessment, approval, re-sign, registration and Eval certification, *then* it serves inference on a GPU node with no network at any stage after the initial media ingest.
- **AC-RF-MS-1b — Unsafe formats are refused.** *Given* a distribution containing a pickle-format weight file, *when* intake runs, *then* it is refused and audited.
- **AC-RF-MS-1c — The assessor never decides.** *Given* any A.9 assessment, *when* its output is inspected, *then* it contains restrictions and unknowns but no approve/reject verdict, and approval requires a recorded human decision.
- **AC-RF-MS-1d — Unverifiable upstream is recorded.** *Given* a distribution with no published digest, *when* it is ingested, *then* the absence is recorded as a risk on the registry entry and surfaced at promotion.
- **AC-RF-MS-1e — Enclosure signature governs load.** *Given* a registered model, *when* it is loaded, *then* verification is against the enclosure's own signature via `mg_verify::verify`, and a tampered artefact fails closed.
- **AC-RF-MS-1f — Air-gapped sites can receive it.** *Given* an approved bundle on removable media, *when* it is verified and loaded at a disconnected site, *then* it serves; *and* a tampered bundle is refused there.

**Done when:** the full journey is demonstrated with a real model and documented as a runbook.

---

### RF-MS-2 · Model licence, restriction and export-control enforcement at runtime

**Severity:** High · **Area:** Model supply · **Depends on:** RF-MS-1, RF-CMP-5

> **Status at 2026-10-12 — Open.** The only licence data is three manifest fields (name, OSI-approved, redistribution-ok) and the only enforcement is the pre-existing load-time `require_redistribution_ok` switch, which defaults to off; a comment in `artifact.rs` defers the rest to this item. There are no machine-readable purpose or jurisdiction restrictions, no routing-time check, `mg-model pack` does not refuse a non-redistributable model, and the Model Plane screen is mock-backed.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Extend the signed manifest with machine-readable restrictions (permitted/prohibited purposes, attribution, redistribution, jurisdiction)
> - Refuse at serving time a request whose purpose the served model prohibits, naming the restriction
> - Make mg-model pack refuse and record a model whose licence forbids redistribution
> - Return required attribution with completions and show restrictions on a live Model Plane screen
> - Tie export-restricted models into the cross-jurisdiction transit controls (RF-CMP-5)
>
> Checked: `mimir/model-server/src/artifact.rs:35` · `mimir/model-server/src/artifact.rs:177` · `mimir/model-server/tests/model_registry.rs:215` · `tools/mg-model/src/main.rs:271`

**Prompt**

```
Make recorded restrictions actually constrain use.

1. Model restrictions as machine-readable policy attached to the registry entry:
   permitted purposes, prohibited uses, attribution requirements, redistribution
   terms, jurisdiction limits, and any output-use conditions.
2. Enforce at routing time: a request whose purpose is prohibited for the
   selected model is refused (or routed to a permitted model, explicitly and
   visibly), never silently served. The refusal names the restriction.
3. Enforce at distribution time: a model whose licence forbids redistribution
   cannot be packaged into a transit bundle; the attempt is refused and audited.
4. Attribution: where a licence requires attribution, surface it in the UI
   wherever that model's output is shown, and include it in exports.
5. Export control: where a model or its outputs are subject to export
   restrictions, integrate with the RF-CMP-5 controls so cross-jurisdiction
   transit is blocked.
6. Surface every restriction on the Model Plane screen so an operator sees the
   terms governing the model they are using.
```

**Acceptance criteria**

- **AC-RF-MS-2a — Prohibited purpose is refused.** *Given* a model prohibiting a purpose and a request under it, *when* routed, *then* the request is refused naming the restriction, or explicitly re-routed with the reason shown.
- **AC-RF-MS-2b — Non-redistributable models cannot transit.** *Given* such a model, *when* bundle packaging is attempted, *then* it is refused and audited.
- **AC-RF-MS-2c — Attribution is displayed and exported.** *Given* output from a model requiring attribution, *when* it is shown or exported, *then* the attribution accompanies it.
- **AC-RF-MS-2d — Restrictions are visible.** *Given* the Model Plane screen, *when* a model is selected, *then* its full restriction set is displayed.

**Done when:** restrictions are machine-readable, enforced at routing and distribution, and visible in the UI.

---

### RF-MS-3 · Model versioning, rollback and deprecation

**Severity:** Medium · **Area:** Model supply · **Depends on:** RF-MS-1, RF-MI-6

> **Status at 2026-10-12 — Open.** A signed manifest binds id, version and weights digest, and `mimir-server` serves exactly the one model directory it verified and logs its SHA-256, but that is the whole lifecycle. `ModelRegistry` has a `Deprecated` state that is only a flag; there is no canary, automatic rollback, usage-evidenced removal, or recording of model/prompt/adapter version and sampling parameters on agent actions.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Define an immutable model version identifier (family, base version, quantisation, adapter version) and require serving/routing config to name it
> - Record model, prompt and adapter versions and sampling parameters on every audited agent action and add a replay path
> - Add canary promotion with automatic rollback on Eval/quality regression, reusing Bifrost's ring mechanics
> - Block removal of a deprecated version until recorded usage is zero for a documented window
> - Show versions, canary state and rollback history on a live Model Plane screen
>
> Checked: `mimir/model-server/src/artifact.rs:111` · `mimir/model-server/src/model_registry.rs:32` · `mimir/mimir-server/src/main.rs:186` · `mimir/model-server/src/router.rs:94`

**Prompt**

```
Give models the same lifecycle discipline as code artefacts.

1. Version every registered model (family, base version, quantisation, adapter
   version) with immutable identifiers, and make routing target a resolvable
   version — never a floating "latest".
2. Canary and rollback: promote a new model version to a fraction of traffic,
   compare Eval and production quality signals against the incumbent, and roll
   back automatically on regression. Reuse Bifrost's ring mechanics rather than
   inventing a second rollout system.
3. Deprecation: mark a version deprecated, warn its callers, and remove only
   after a documented window with usage evidence that it is unused.
4. Reproducibility of AI decisions: every recorded agent action names the exact
   model version, prompt version, adapter version and sampling parameters used,
   so a past decision can be reconstructed and re-run. This is what makes the
   audit lineage meaningful for AI actions.
5. Surface versions, canary state and rollback history in the Model Plane screen.
```

**Acceptance criteria**

- **AC-RF-MS-3a — No floating targets.** *Given* any routing configuration, *when* inspected, *then* it resolves to an immutable version identifier.
- **AC-RF-MS-3b — Canary regression rolls back.** *Given* a canary whose quality signals regress beyond tolerance, *when* detected, *then* traffic returns to the incumbent automatically and the event is audited and alerted.
- **AC-RF-MS-3c — Past decisions are reconstructible.** *Given* an audited AI action from the past, *when* its recorded model, prompt, adapter and parameters are re-applied, *then* the decision can be reproduced (or the reason it cannot is recorded, e.g. a deleted model).
- **AC-RF-MS-3d — Deprecation is evidence-based.** *Given* a deprecated version, *when* removal is attempted with recorded usage, *then* it is blocked until usage is zero for the documented window.

**Done when:** versioned routing, canary rollback and decision reconstruction are implemented and demonstrated.

---

### RF-MS-4 · Fine-tuning, adapters and derived-model governance

**Severity:** Medium · **Area:** Model supply · **Depends on:** RF-MS-1, RF-MS-2

> **Status at 2026-10-12 — Open.** Nothing for derived models exists: no tuning-run record, no dataset-marking propagation to a model, no lineage fields for base model, dataset digests or hyperparameters, no memorisation check, and no adapter (LoRA) artefact type. The A.5 distiller structurally excludes weight changes, which is the opposite path.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Scope change:** No training or fine-tuning capability exists anywhere in the tree. Defer this item until one is proposed.
>
> **Still to do:**
> - Decide whether in-enclosure fine-tuning is in scope at all before building this (no training capability exists in the tree)
> - Add a derived-model manifest: base model id, dataset content ids from forge/dataset-store, hyperparameters, code version, approver
> - Propagate the highest tuning-dataset marking onto the derived model and enforce it at serving
> - Refuse registration without a recorded approval and add a memorisation/leakage check that blocks promotion
> - Treat LoRA-class adapters as signed, registered, version-pinned artefacts
>
> Checked: `mimir/model-server/src/artifact.rs:111` · `mimir/model-server/src/model_registry.rs:54` · `mimir/agent-runtime/src/learn.rs:14`

**Prompt**

```
Govern models the enclosure produces, not only those it receives.

1. Define the derived-model lifecycle: what data may be used for tuning (with
   marking and purpose constraints enforced at dataset selection), who approves,
   how the training run is recorded, and how the result is signed and registered.
2. Data governance for training is the sharp edge: a derived model can encode the
   data it saw. Enforce that a tuning dataset's markings propagate to the derived
   model's marking, so a model tuned on classified data inherits that
   classification and its routing restrictions.
3. Provenance: the derived model's registry entry records base model, dataset
   versions (content-addressed, from forge/dataset-store), hyperparameters,
   training code version, and the approving authority — a complete, verifiable
   lineage.
4. Evals: derived models face the same per-family certification, plus regression
   against the base model, plus a memorisation/leakage check against the training
   data.
5. Adapters (LoRA-class) follow the same path as full models: signed, registered,
   Eval-gated, and version-pinned in routing.
```

**Acceptance criteria**

- **AC-RF-MS-4a — Markings propagate.** *Given* tuning on a dataset with a marking, *when* the derived model is registered, *then* it carries at least that marking, and routing restricts it accordingly.
- **AC-RF-MS-4b — Lineage is complete and verifiable.** *Given* a derived model, *when* its provenance is inspected, *then* base model, dataset content digests, hyperparameters, code version and approver are all present and verifiable.
- **AC-RF-MS-4c — Leakage is tested.** *Given* a derived model, *when* the memorisation check runs against its training data, *then* results are recorded and a failure blocks promotion.
- **AC-RF-MS-4d — Unapproved tuning is impossible.** *Given* a tuning run without recorded approval, *when* registration is attempted, *then* it is refused.

**Done when:** the derived-model path is implemented with marking propagation and leakage testing.

---

### RF-MS-5 · Multi-family routing policy and portability certification

**Severity:** Medium · **Area:** Model supply · **Depends on:** RF-MI-2, RF-MI-6

> **Status at 2026-10-12 — Open.** The `Router` is unchanged from the plan's evidence and is not in the serving path: `mimir-server` serves one backend and never constructs a Router or ModelRegistry. Family preference per workload class is still hard-coded in `ideal_family`, degradation picks the next permitted family alphabetically with no certification check, there is no prompt-by-family certification matrix, and FR-MS-9 is cited nowhere in source.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** `mimir-server` never constructs `Router` or `ModelRegistry`. Routing policy and certification are moot until RF-CA-8 lands.
>
> **Still to do:**
> - Decide whether mimir-server will host several models; if so wire Router and ModelRegistry into it, otherwise re-scope this item to the single-model case
> - Move routing rules (purpose, product, clearance, hardware to family) into a policy file and remove the hard-coded ideal_family mapping
> - Generate a prompt-by-family certification matrix from real Eval results and refuse uncertified routes and substitutions
> - Cite FR-MS-9 in the certification code and add its verifying test; fail the build on a stale matrix
> - Audit every cross-family substitution and show the matrix on live Model Plane and Evals screens
>
> Checked: `mimir/model-server/src/router.rs:67` · `mimir/model-server/src/router.rs:78` · `mimir/mimir-server/src/main.rs:268` · `mimir/model-server/tests/router.rs:228`

**Evidence.** `mimir/model-server/src/router.rs` and `select.rs` implement routing and hardware-aware selection. `FR-MS-9` (the model-portability consequence for the prompt library) is the one requirement id in the SADs with no citation in source — it is tagged `-DESIGN`, but the portability property it describes is exactly what needs a test.

**Prompt**

```
Prove portability, and make routing policy explicit.

1. Routing policy as data: which model serves which purpose, product, clearance
   and hardware profile, with explicit precedence and a documented default. No
   routing decision may be implicit in code.
2. Portability certification: for each library prompt, run its Eval suite against
   every family it may be routed to and record a certification matrix
   (prompt × family × result). A prompt may only be routed to a family it is
   certified against; an uncertified route is refused, not attempted.
3. Cite FR-MS-9 in the certification implementation, closing the one traceability
   gap, and add its verification to the acceptance suite.
4. Degradation policy: when the preferred family is unavailable, the documented
   behaviour is an explicit, visible, audited substitution to another CERTIFIED
   family — or a refusal. Never an uncertified substitution and never a silent
   one.
5. Surface the certification matrix in the Model Plane and Evals screens.
```

**Acceptance criteria**

- **AC-RF-MS-5a — Uncertified routes are refused.** *Given* a prompt not certified for family F, *when* routing would select F, *then* the request is refused or routed to a certified family, visibly and with an audit record.
- **AC-RF-MS-5b — Certification matrix is real and current.** *Given* the matrix, *when* compared with Eval results, *then* they agree; a stale matrix fails CI.
- **AC-RF-MS-5c — Routing policy is data.** *Given* the source tree, *when* routing decisions are traced, *then* each derives from the policy file, not from hard-coded logic.
- **AC-RF-MS-5d — FR-MS-9 is traced.** *Given* the traceability matrix (RF-SC-6), *when* generated, *then* FR-MS-9 has a citing implementation and a verifying test.

**Done when:** the certification matrix gates routing; FR-MS-9 is closed in traceability.

---

# I — Forge: execution depth

> **Status.** `forge/` is 4 000 lines across seven crates: a reference execution engine explicitly "no threads, no I/O, no clock", a content-addressed dataset store, a pipeline compiler with a codegen seam for "a future backend (Polars/DuckDB/Spark)", connectors, health expectations, projection to the fabric, and the A.6 NL-to-transform assistant. It is a complete design with a deliberately minimal executor. `products/README.md` states the Forge backend is "Rust (DataFusion)" — DataFusion is not a dependency of any crate.

---

### RF-F-1 · A real execution backend behind the codegen seam

**Severity:** High · **Area:** Forge · **Depends on:** RF-PER-1, RF-RT-2

> **Status at 2026-10-12 — Partial.** A second backend exists behind the seam: `PartitionedBackend` runs partitions on worker threads with a hash shuffle and a spill-to-disk round-trip, and tests assert its output equals the reference engine for one filter-join-aggregate-sort pipeline across worker counts. It is bespoke and in-process: there is no ADR (no docs/dev/adr directory), `products/README.md` still claims DataFusion, the spill writes a partition and reads it straight back so memory is not bounded below dataset size, there is no incremental or partition-aware execution, no progress reporting and no benchmark. The running forge-worker applies no transforms at all.
> Criteria met 2 of 5 (a, c); open: b, d, e. Confidence: high.
>
> **Scope change:** A `PartitionedBackend` with an equivalence test already sits behind the seam, so the ADR is retrospective. `products/README.md` still claims DataFusion, and the spill is a write-then-read round trip, not out-of-core execution.
>
> **Still to do:**
> - Write ADR 0003 choosing the engine (the bespoke PartitionedBackend is the de facto answer) and correct the DataFusion claim in products/README.md
> - Make execution truly out-of-core: streamed input, bounded memory, real spill and progress reporting, tested on data larger than RAM
> - Widen the equivalence suite to every Transform and edge case (nulls, empty inputs, left joins, windows) rather than one pipeline
> - Add partitioned/incremental execution with late-arriving data and a test that it equals a full recompute
> - Benchmark at 10^6 to 10^9 rows, publish the envelope and gate regressions; wire pipelines into forge-worker so transforms actually run
>
> Checked: `forge/engine/src/partitioned.rs:56` · `forge/engine/src/partitioned.rs:254` · `forge/engine/tests/partitioned.rs:93` · `forge/engine/tests/execution.rs:73` · `products/README.md:9`

**Prompt**

```
Land a real execution engine behind the existing seam without disturbing it.

1. Decide in an ADR (docs/dev/adr/0003-forge-execution-engine.md): DataFusion,
   Polars, or a bespoke vectorised executor, judged against vendorability,
   offline builds, reproducibility, memory behaviour on constrained nodes, and
   the SBOM/licence impact. products/README.md currently claims DataFusion —
   either make that true or correct the document (RF-DOC-1).
2. Implement the backend behind forge/pipeline's codegen seam so the reference
   engine remains the semantic oracle: every operation the real backend performs
   must produce results identical to the reference engine on the test corpus.
   That equivalence test is the core deliverable, not the backend itself.
3. Support out-of-core execution: spill to disk, bounded memory, and progress
   reporting for datasets that exceed RAM. A pipeline must not OOM a node.
4. Support partitioned and incremental execution: process new partitions only,
   with correct handling of late-arriving data and a documented consistency
   model for readers during a run.
5. Preserve determinism where it is claimed: a pipeline declared deterministic
   must produce bit-identical output for identical input, which is what makes
   the content-addressed versioning meaningful. Non-deterministic operations are
   declared and their outputs marked.
6. Publish the performance envelope: rows/second by operation, memory profile,
   spill behaviour, at 10^6, 10^8 and 10^9 rows.
```

**Acceptance criteria**

- **AC-RF-F-1a — Semantic equivalence.** *Given* the test corpus, *when* the same pipeline runs on the reference engine and the real backend, *then* results are identical for every case.
- **AC-RF-F-1b — Out-of-core works.** *Given* a dataset several times available RAM, *when* a pipeline runs, *then* it completes within the memory bound by spilling, and reports progress throughout.
- **AC-RF-F-1c — Determinism holds.** *Given* a pipeline declared deterministic, *when* run twice on identical input, *then* outputs are bit-identical and hash to the same content address.
- **AC-RF-F-1d — Incremental is correct.** *Given* new and late-arriving partitions, *when* an incremental run executes, *then* results match a full recomputation.
- **AC-RF-F-1e — Envelope published and gated.** *Given* the benchmark, *when* throughput regresses beyond tolerance, *then* CI fails.

**Done when:** the ADR is merged; the equivalence suite is green; out-of-core and incremental execution are demonstrated at 10^8 rows.

---

### RF-F-2 · Real connectors and ingestion

**Severity:** High · **Area:** Forge · **Depends on:** RF-F-1, RF-API-6

> **Status at 2026-10-12 — Partial.** Concrete connectors now exist and run in a service: delimited files, PostgreSQL (hand-rolled v3 client, SCRAM, read-only, watermark), S3-compatible stores (SigV4, incremental by last-modified) and Kafka (bounded resumable reads), each checked against the sandbox's exact host:port and resuming from a checkpoint; `forge-worker` syncs them into a sealed versioned store with ingest lineage and on into the fabric. PostgreSQL and S3 have live-server tests behind a script, Kafka only a fake broker. Missing: Parquet/JSON/NDJSON, a real HTTP source, schema inference, drift as a governed decision (drift fails the sync), per-record quarantine (one bad record fails the whole sync), filesystem/memory sandboxing, declared delivery semantics and SDK docs/template.
> Criteria met 1 of 6 (f); open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** File, PostgreSQL, S3 and Kafka connectors run in `forge-worker`. Narrow this to further formats, HTTP, quarantine, governed schema drift and a real Kafka broker. Media ingest is RF-CA-12.
>
> **Still to do:**
> - Add Parquet, JSON and NDJSON parsing and a real HTTP/REST connector routed through the egress broker; test Kafka against a real broker
> - Route failing records to a quarantine dataset with reasons and report partial success instead of failing the whole sync
> - Turn schema drift into a paused, human-decided event (accept/reject/quarantine) rather than a hard failure, and add schema discovery
> - Add a test that no credential appears in logs, errors, metrics or datasets (only Secret's Debug redaction is tested), and raise a security event on sandbox denial; add file allow-list and resource limits
> - Declare delivery semantics per connector, give PostgreSQL TLS, and publish SDK docs with a connector template
>
> Checked: `forge/connector-sdk/tests/live.rs:40` · `forge/connector-sdk/tests/kafka.rs:456` · `forge/connector-sdk/tests/postgres.rs:434` · `forge/connector-sdk/src/connector.rs:64` · `forge/worker/tests/sync.rs:173`

**Evidence.** `forge/connector-sdk` (528 lines) defines the connector contract and credential handling via KMS. No concrete connector exists. `ConnectorsScreen` lists fixtures.

**Prompt**

```
Ship a real connector set and the runtime that hosts them.

1. Implement a first-party connector set covering the realistic sovereign
   deployment: local and network filesystems (CSV, Parquet, JSON, NDJSON),
   object storage (S3-compatible), relational databases via a vendored driver,
   message streams, and a generic HTTP/REST source — the last two routed through
   the egress broker (RF-API-6) so no connector makes uncontrolled outbound
   calls.
2. Connector runtime: sandboxed execution (no ambient network, filesystem
   allow-list, memory and time limits), credential injection from KMS (never
   plaintext, never logged), backpressure, checkpointing and exactly-once or
   idempotent-at-least-once semantics — declared per connector, not assumed.
3. Schema discovery and drift: infer schemas, detect upstream drift, and surface
   it as a governed decision (accept, reject, quarantine) rather than silently
   adapting. Drift that would change a downstream projection's semantics
   requires human approval.
4. Incremental capture: watermarks and change capture where the source supports
   them, with a documented behaviour for sources that do not.
5. Error handling: per-record error routing to a quarantine dataset with the
   reason, so one bad record never fails a whole run silently and never
   disappears.
6. Connector SDK docs and a template so third parties can write one that inherits
   every control by construction.
```

**Acceptance criteria**

- **AC-RF-F-2a — Real sources ingest.** *Given* each first-party connector and a real source, *when* ingestion runs, *then* data lands in a versioned dataset with recorded lineage.
- **AC-RF-F-2b — Credentials never leak.** *Given* a connector using credentials, *when* logs, traces, metrics, error messages and the dataset are inspected, *then* no credential material appears.
- **AC-RF-F-2c — Sandbox holds.** *Given* a connector attempting network access outside the broker or a file outside its allow-list, *when* it runs, *then* it is refused and a security event is raised.
- **AC-RF-F-2d — Drift is a governed decision.** *Given* an upstream schema change affecting a projection, *when* it is detected, *then* the run pauses for a human decision and does not silently adapt.
- **AC-RF-F-2e — Bad records are quarantined, not lost.** *Given* records failing validation, *when* the run completes, *then* they are in the quarantine dataset with reasons and counts, and the run reports partial success explicitly.
- **AC-RF-F-2f — Resumption is correct.** *Given* an interrupted ingestion, *when* it resumes from its checkpoint, *then* no record is duplicated or dropped relative to the declared semantics.

**Done when:** the first-party set is implemented and tested against real sources; the SDK template exists.

---

### RF-F-3 · Pipeline orchestration, scheduling and dependency management

**Severity:** High · **Area:** Forge · **Depends on:** RF-F-1, RF-PER-4

> **Status at 2026-10-12 — Partial.** There is now a running `forge-worker` that executes jobs on a fixed interval in file order, retries a failed run after at most ten seconds, keeps durable per-job checkpoints recorded only after the gateway accepts the records, and audits each run. It is an interval loop, not an orchestrator: no DAG or dependency resolution, cycle detection, backfill, concurrency limits, transient-versus-data failure distinction, cancellation, SLA/freshness, column-level lineage or versioned pipeline definitions. `forge/pipeline`'s `Scheduler` still only sorts pipelines into batch, stream and manual lists.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Scope change:** `forge-worker` is an interval job runner with durable checkpoints and audit. Read this item as growing it into a DAG orchestrator.
>
> **Still to do:**
> - Add job dependencies with cycle detection at load and run a job only after its upstreams succeed (today order is file order)
> - Classify failures so data-quality failures are never retried and transient ones retry within a bounded backoff policy
> - Add a kill-mid-run test proving a re-run equals a clean run and partial output is never visible
> - Add bounded, pausable backfill and per-pipeline/global concurrency limits, timeouts and cancellation
> - Record column-level lineage as a queryable graph, declare freshness SLAs with alerts, and version pipeline definitions with approval
>
> Checked: `forge/worker/src/main.rs:51` · `forge/worker/src/main.rs:197` · `forge/pipeline/src/scheduler.rs:55` · `forge/pipeline/tests/build.rs:207` · `forge/worker/tests/sync.rs:173`

**Evidence.** `forge/pipeline` compiles pipelines; `SchedulesScreen` renders fixtures; `services/scheduler` is a replica rolling-upgrade state machine, not a job scheduler. There is no orchestrator.

**Prompt**

```
Build the pipeline orchestrator.

1. DAG scheduling with real dependency resolution: trigger on schedule, on
   upstream completion, or on data arrival; detect cycles at definition time;
   support backfill over a date range with bounded concurrency.
2. Execution management: concurrency limits per pipeline and globally, priority,
   retry with backoff for transient failures only (a data-quality failure must
   not be retried into success), timeouts, and cancellation that actually stops
   work and cleans up.
3. State and idempotency: a run's state is durable (RF-PER-4); an interrupted run
   is marked failed and is safely re-runnable; partial outputs are never visible
   as complete.
4. SLAs and freshness: declare expected completion and data freshness per
   pipeline; alert on breach (RF-OBS-4); surface freshness on every dataset so a
   consumer can see how stale their data is.
5. Lineage: record dataset-level and column-level lineage across runs, and expose
   it as a queryable graph — this is what makes impact analysis possible before a
   change.
6. Governance: pipeline definitions are versioned, changes are reviewable and
   audited, and a pipeline that would alter a projection's semantics requires
   approval.
```

**Acceptance criteria**

- **AC-RF-F-3a — Dependencies are honoured.** *Given* a DAG, *when* it runs, *then* each task starts only after its upstreams succeed, and a cycle is refused at definition time.
- **AC-RF-F-3b — Retries do not mask data errors.** *Given* a data-quality failure, *when* the run fails, *then* it is not retried; *given* a transient infrastructure failure, *then* it is retried within the bounded policy.
- **AC-RF-F-3c — Interrupted runs are safe to re-run.** *Given* a run killed mid-execution, *when* it is re-run, *then* the result equals a clean run and no partial output was ever visible as complete.
- **AC-RF-F-3d — Freshness is visible and alerted.** *Given* a pipeline breaching its freshness SLA, *when* the breach occurs, *then* an alert fires and every consuming dataset shows its staleness.
- **AC-RF-F-3e — Lineage answers impact.** *Given* a proposed schema change, *when* impact analysis runs, *then* it lists every downstream dataset, projection, object type and dashboard affected, at column granularity.
- **AC-RF-F-3f — Backfill is bounded.** *Given* a backfill over a long range, *when* it runs, *then* concurrency is bounded, progress is reported, and it can be paused and resumed.

**Done when:** the orchestrator runs real DAGs with backfill; lineage-based impact analysis works; freshness SLAs alert.

---

### RF-F-4 · Data quality: expectations, profiling and quarantine

**Severity:** Medium · **Area:** Forge · **Depends on:** RF-F-3

> **Status at 2026-10-12 — Open.** Unchanged from the plan's evidence: `forge/health` has five expectations (not-null, unique, row-count min/max, int range) and a gate that blocks a build's commit to the promotion branch when one fails. There is no profiling, drift detection, per-expectation enforcement policy, quarantine or scorecard, the forge-worker does not run the health gate before projecting, and `DataHealthScreen` is backed by `ForgeMock`.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Run a health suite in forge-worker before projection and refuse to project a dataset that fails a blocking expectation, recording the reason
> - Add per-expectation policy (warn, quarantine, fail) and a first-class quarantine dataset that can be inspected and re-processed
> - Compute and store column statistics on every run and flag drift against the historical profile
> - Expand the library: schema conformance, referential integrity, distribution, freshness, volume anomaly, custom expressions
> - Publish a per-dataset quality scorecard and surface it on a live Data Health screen and in consumers
>
> Checked: `forge/health/src/expectation.rs:10` · `forge/health/tests/ac_f_3.rs:71` · `forge/worker/src/lib.rs:1` · `sdk/ts/src/products/forge.ts:539`

**Evidence.** `forge/health` (300 lines) implements expectations; `DataHealthScreen` renders fixtures.

**Prompt**

```
Make data quality operational.

1. Expand the expectation library: schema conformance, nullability, uniqueness,
   referential integrity across datasets, range and distribution checks,
   freshness, volume anomaly, and custom expressions.
2. Profile automatically: on every run, compute and store column statistics, and
   detect drift against the historical profile, so an anomaly is caught without
   anyone having written an expectation for it.
3. Enforcement policy per expectation: warn, quarantine the failing records, or
   fail the run. A dataset failing a blocking expectation must not be projected
   to the fabric — bad data must not reach the operational picture.
4. Quarantine is first-class: failing records are retained with their reason,
   inspectable in the UI, and re-processable after correction.
5. Publish a data-quality scorecard per dataset with trend, and surface it on the
   dataset and in every consumer that depends on it.
```

**Acceptance criteria**

- **AC-RF-F-4a — Blocking failures stop projection.** *Given* a dataset failing a blocking expectation, *when* projection is attempted, *then* it is refused and the reason is recorded.
- **AC-RF-F-4b — Drift is detected without an explicit rule.** *Given* a column whose distribution shifts materially, *when* profiling runs, *then* drift is flagged even with no expectation defined for it.
- **AC-RF-F-4c — Quarantine is inspectable and re-processable.** *Given* quarantined records, *when* they are viewed and corrected, *then* they can be re-processed and the outcome is recorded.
- **AC-RF-F-4d — Consumers see quality.** *Given* a dataset with a degraded scorecard, *when* a consumer views anything derived from it, *then* the quality state is visible.

**Done when:** profiling and drift run automatically; blocking expectations gate projection; the scorecard is live.

---

### RF-F-5 · Dataset versioning, branching and time travel at scale

**Severity:** Medium · **Area:** Forge · **Depends on:** RF-PER-1, RF-F-1

> **Status at 2026-10-12 — Partial.** The plan's premise that the store is in memory is stale: `DurableDatasetStore` persists every sealed, content-addressed version blob and atomic branch refs under a directory, reloads them on open, and fails closed on a tampered blob; forge-worker uses it. Deduplication is only whole-snapshot (identical content shares one blob), every blob is loaded into memory at open, it writes to the local filesystem rather than a BlobStore seam, and there is no merge, diff, branch delete, garbage collection, legal hold, timestamp time travel or clearance check on reads.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** `DurableDatasetStore` persists versions and refs and is restart-tested. What remains is chunk-level deduplication, merge, garbage collection, hold and time-travel authorisation.
>
> **Still to do:**
> - Chunk datasets by partition so an unchanged partition is stored once across versions, and stop loading every blob at open
> - Add merge with conflict detection, schema/statistical diff and branch delete
> - Add garbage collection of unreferenced versions that skips and records versions under legal hold, plus a retention policy
> - Add read-as-of version/timestamp authorised at the reader's current clearance, with a downgrade test
> - Report deduplication ratio and footprint per dataset; move persistence behind the BlobStore seam
>
> Checked: `forge/dataset-store/src/durable.rs:43` · `forge/dataset-store/src/durable.rs:99` · `forge/dataset-store/tests/durable_restart.rs:101` · `forge/dataset-store/tests/durable_restart.rs:170` · `forge/dataset-store/src/store.rs:55`

**Evidence.** `forge/dataset-store` (785 lines) implements content-addressed versioning and branches, in memory.

**Prompt**

```
Make versioning durable and practical at data scale.

1. Persist versions, branches and commits over the BlobStore seam with
   content-addressed chunking, so an unchanged partition is stored once across
   versions.
2. Implement branch operations properly: create, merge with conflict detection,
   diff (schema and statistical, not row-by-row for large data), and delete with
   garbage collection of unreferenced content.
3. Time travel: read any dataset as of a version or a timestamp, with the
   performance characteristics documented, and enforce that time-travel reads are
   authorised at the CURRENT clearance — not the clearance in force when the
   version was written.
4. Retention for versions: how many are kept, for how long, and how GC interacts
   with legal hold (RF-PER-6). A version under hold is never collected.
5. Storage efficiency: measure and report deduplication ratio and total footprint
   per dataset, and surface it so an operator understands cost.
```

**Acceptance criteria**

- **AC-RF-F-5a — Versions survive restart and deduplicate.** *Given* many versions differing in one partition, *when* stored, *then* unchanged content is stored once and all versions are readable after restart.
- **AC-RF-F-5b — Time travel is authorised now.** *Given* a version written when a subject had higher clearance, *when* they read it after downgrade, *then* current clearance governs and the read is filtered accordingly.
- **AC-RF-F-5c — Merge conflicts are detected.** *Given* divergent branches with incompatible changes, *when* merged, *then* conflicts are reported rather than silently resolved.
- **AC-RF-F-5d — GC respects hold.** *Given* an unreferenced version under legal hold, *when* GC runs, *then* it is retained and the skip is recorded.

**Done when:** versioning is durable with deduplication; time-travel authorisation is proven; GC and hold interact correctly.

---

### RF-F-6 · Projection to the fabric: incremental, consistent, reversible

**Severity:** High · **Area:** Forge · **Depends on:** RF-PER-3, RF-F-3

> **Status at 2026-10-12 — Partial.** Projection now reaches a real fabric: forge-worker projects rows to object and link instances carrying the job's marking and purposes and sends them over PQ-mTLS to the gateway, which authorises, audits and upserts them; a re-run leaves the fabric unchanged and a changed source row is updated. The marking is one per mapping (no column markings), the gateway does not check the worker may write at the declared marking, a sync never deletes, and there is no atomic visibility, simulation, rollback, cross-dataset conflict detection or scale measurement.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Scope change:** `forge-worker` projects into the real fabric through the gateway. The concrete gaps: no deletes, markings only at mapping level, and no gateway check on the marking a job declares.
>
> **Still to do:**
> - Propagate deletes and removed links, then assert a sequence of incremental syncs equals a full re-projection
> - Add column-level markings carried onto properties, refuse mapping a marked column to an unmarked property, and have the gateway check the worker may write at the declared marking
> - Make a projection atomic to fabric readers (stage then swap) and define the visibility semantics
> - Add dry-run simulation with create/update/delete counts and samples, and audited rollback to the prior fabric state
> - Define precedence and contradiction detection when two datasets project to one type; measure a 10^8-row envelope
>
> Checked: `forge/projection/tests/ac_f_4.rs:53` · `forge/worker/tests/sync.rs:173` · `forge/worker/src/lib.rs:11` · `forge/projection/src/mapping.rs:1` · `README.md:564`

**Evidence.** `forge/projection` (306 lines) maps dataset columns to object types.

**Prompt**

```
Make projection production-grade — this is the seam where data becomes the
operational picture, and where a mistake becomes operationally visible.

1. Incremental projection: project only changed rows, with correct handling of
   updates, deletes and reordering; a full re-projection must produce the same
   fabric state as the incremental path (assert this).
2. Consistency: a projection is atomic from a reader's perspective — the fabric
   never shows a half-projected dataset. Define and implement the visibility
   semantics.
3. Marking propagation: a dataset's and a column's markings propagate to the
   projected objects and properties, so classified data cannot be laundered into
   an unmarked object. Test the negative case explicitly.
4. Reversibility: a projection can be rolled back, restoring the prior fabric
   state, with the rollback audited. Mapping changes are simulated first — show
   exactly which objects would change, be created, or be removed, before commit.
5. Conflict handling: when two datasets project to the same object type, define
   precedence and detect contradictions rather than last-writer-wins.
6. Scale: project 10^8 rows within a published time envelope without degrading
   fabric query latency beyond a stated bound.
```

**Acceptance criteria**

- **AC-RF-F-6a — Incremental equals full.** *Given* a sequence of incremental projections, *when* compared to a full re-projection, *then* the fabric state is identical.
- **AC-RF-F-6b — No half-projected state.** *Given* a projection in progress, *when* the fabric is queried, *then* it shows either the prior or the complete new state, never a mixture.
- **AC-RF-F-6c — Markings cannot be laundered.** *Given* a marked column, *when* projected, *then* the resulting property carries the marking; *and* an attempt to map it to an unmarked property is refused.
- **AC-RF-F-6d — Simulation precedes commit.** *Given* a mapping change, *when* simulated, *then* the exact create/update/delete counts and samples are shown before any commit.
- **AC-RF-F-6e — Rollback restores.** *Given* a committed projection, *when* rolled back, *then* the prior fabric state is restored and both operations are audited.
- **AC-RF-F-6f — Scale envelope holds.** *Given* a 10^8-row projection, *when* it runs, *then* it completes within the published envelope and fabric query latency stays within the stated bound.

**Done when:** incremental/full equivalence, marking propagation and rollback are all proven at scale.

---

### RF-F-7 · Transform IDE and NL-to-transform, made real

**Severity:** Medium · **Area:** Forge · **Depends on:** RF-MI-4, RF-UID-2

> **Status at 2026-10-12 — Partial.** `forge/ai-assist` now has a live `MimirSuggester` that sends the A.6 prompt to Mimir over PQ-mTLS, alongside the unchanged proposal-only adapter whose tests show a suggestion is only ever a preview and an invented column is flagged and blocked. Nothing runs it: no service hosts the assistant, the gateway does not route to mimir-server, and the Transform IDE and Pipeline Builder screens are still served by `ForgeMock`, with no node editing, real preview execution, versioned save or approval flow.
> Criteria met 1 of 5 (d); open: a, b, c, e. Confidence: high.
>
> **Scope change:** A live `MimirSuggester` bridge exists but nothing hosts it, and the screens run on `ForgeMock`.
>
> **Still to do:**
> - Expose Forge pipeline, preview and assist operations through the gateway and add a non-mock ForgeGateway implementation
> - Execute transform preview on a bounded real sample through the engine and return real rows and errors
> - Make the Pipeline Builder an editor (add, remove, connect, configure nodes; live validation; versioned save)
> - Host the A.6 assistant against mimir-server with the real dataset schema, and audit the operator's commit with the prompt and proposal
> - Add definition history, diff and an approval gate before a production pipeline change takes effect
>
> Checked: `forge/ai-assist/src/mimir_bridge.rs:16` · `forge/ai-assist/tests/ac_f_5.rs:54` · `forge/ai-assist/tests/ac_f_5.rs:86` · `sdk/ts/src/products/forge.ts:634` · `products/forge/ui/src/screens/TransformIdeScreen.tsx:46`

**Evidence.** `forge/ai-assist` (859 lines) implements the A.6 NL-to-transform prompt with its proposal-only contract and Evals. `TransformIdeScreen` (168 lines) and `PipelineBuilderScreen` (220 lines) render DAGs and editors from fixtures — the "builder" cannot add, remove or connect nodes.

**Prompt**

```
Make the authoring experience real.

1. Transform IDE: a real editor over the transform language with syntax
   highlighting, real-time validation against the actual dataset schema,
   autocomplete from real columns, inline errors, and a genuine preview that
   executes on a bounded sample through the real engine and shows real rows.
2. Pipeline builder: genuinely interactive — add, remove, connect and configure
   nodes; validate the DAG live; show schema propagation through the graph; and
   save as a versioned, reviewable definition. Today it renders a fixed DAG; it
   must become an editor.
3. NL-to-transform: wire A.6 to the real model plane and the real schema. Keep
   the proposal-only contract absolutely — the assistant proposes a transform,
   the operator reviews a diff and a preview, and only the operator commits.
   Never auto-apply, and surface the EV-A6 guarantees (real columns only, no
   hallucinated fields) as visible evidence in the UI.
4. Testing affordances: sample-based unit tests for transforms, saved as part of
   the pipeline definition and run in CI for the pipeline.
5. Collaboration: definitions are versioned with history, diff, comments and an
   approval flow before a production pipeline changes.
```

**Acceptance criteria**

- **AC-RF-F-7a — The builder builds.** *Given* the pipeline builder, *when* an operator adds, connects, configures and removes nodes, *then* the DAG updates, validates live, and saves as a new version.
- **AC-RF-F-7b — Preview is real.** *Given* a transform, *when* preview runs, *then* it executes on real sampled data through the real engine and displays actual rows and actual errors.
- **AC-RF-F-7c — The assistant only proposes.** *Given* an NL request, *when* A.6 responds, *then* a diff and preview are shown and nothing is applied until the operator commits; the commit is audited with the prompt and the proposal.
- **AC-RF-F-7d — No hallucinated columns.** *Given* an NL request naming a non-existent column, *when* A.6 responds, *then* it refuses or asks, and never emits a transform referencing a column that is not in the schema.
- **AC-RF-F-7e — Changes are reviewed.** *Given* a change to a production pipeline, *when* it is saved, *then* it requires approval before taking effect, with a visible diff.

**Done when:** the builder is interactive; preview executes real data; A.6 runs against the real model plane with its proposal-only contract visibly enforced.

---
# J — Bifrost: delivery reality

> **Status.** `bifrost/` is ~1 750 lines: an artefact registry with signature records, a bundle format for air-gapped transit, a rollout state machine, a fleet model, and the A.7 release-risk advisor with its gate. What it lacks is the other end: nothing installs anything, no agent reports, and the artefacts it registers were signed by a stub (RF-SC-1).

---

### RF-B-1 · Artefact registry backed by real content and real verification

**Severity:** High · **Area:** Bifrost · **Depends on:** RF-SC-1, RF-SC-2, RF-PER-4

> **Status at 2026-10-12 — Partial.** `bifrost/artefact` keeps each artefact's payload, SBOM and ML-DSA signature together in one file per (id, version), verifies on ingest, re-verifies every file on open and again in `verify_for_deploy`, and `tools/mg-register` admits gate-verified binaries into it. It trusts a single fixed key with no revocation input, stores no attestation, has no content-addressed blob store, no component/advisory/deployment index, no promotion states and no garbage collection. Nothing calls `verify_for_deploy` outside its own test.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** Payload, SBOM and signature are stored together and re-verified on ingest, on open and at the deploy gate. The real gaps are revocation, attestation, indexes, promotion states and garbage collection.
>
> **Still to do:**
> - Add key revocation to the registry trust model so retrieval of an artefact signed by a revoked key fails, with a test (AC b)
> - Store the release attestation alongside payload, SBOM and signature, index by digest, and add a durable-registry test that all four are returned consistent (AC a)
> - Build a component-version index over stored SBOMs joined to fleet deployments, and a query for advisory impact (AC c)
> - Add promotion states with dual-controlled, audited transitions (AC d)
> - Add garbage collection that retains anything deployed or under legal hold (AC e)
>
> Checked: `bifrost/artefact/src/durable.rs:129` · `bifrost/artefact/src/registry.rs:49` · `bifrost/artefact/src/signed.rs:77` · `bifrost/artefact/tests/registry.rs:105` · `tools/mg-register/src/main.rs:177`

**Prompt**

```
Make the artefact registry the system of record for what exists and what may run.

1. Store artefact content, not only metadata: content-addressed blobs in the
   BlobStore, with the SBOM, signature and attestation stored alongside and
   indexed by digest.
2. Verify on ingest and on every retrieval via mg_verify::verify — never trust a
   prior verification result without a recorded, time-bounded attestation of it.
   A revoked signing key (RF-SC-8) invalidates previously accepted artefacts.
3. Index for the questions operators actually ask: which artefacts contain a
   given component version, which are affected by a given advisory, which are
   deployed where, which are signed by a given key.
4. Promotion model: an artefact moves through defined states (registered,
   scanned, risk-assessed, approved, promoted, deprecated, quarantined) with
   explicit, audited, dual-controlled transitions for the ones that matter.
5. Garbage collection: unreferenced artefacts age out, but never one that is
   deployed anywhere or under legal hold.
```

**Acceptance criteria**

- **AC-RF-B-1a — Content and evidence are stored together.** *Given* a registered artefact, *when* retrieved, *then* content, SBOM, signature and attestation are all available and mutually consistent.
- **AC-RF-B-1b — Verification is not cached indefinitely.** *Given* an artefact verified an hour ago whose signing key is now revoked, *when* it is retrieved for deployment, *then* verification fails.
- **AC-RF-B-1c — Component search works.** *Given* an advisory naming a component version, *when* the registry is queried, *then* it lists every affected artefact and every deployment carrying it.
- **AC-RF-B-1d — Promotion is governed.** *Given* a promotion to a production ring, *when* attempted, *then* it requires dual control and is audited.
- **AC-RF-B-1e — GC protects deployed artefacts.** *Given* an old artefact still deployed at one site, *when* GC runs, *then* it is retained.

**Done when:** the registry stores content and evidence, re-verifies on retrieval, and answers component-impact queries.

---

### RF-B-2 · Fleet agent: installation, reporting and rollback on the target

**Severity:** High · **Area:** Bifrost · **Depends on:** RF-B-1, RF-IAM-6

> **Status at 2026-10-12 — Partial.** `bifrost-agent` is a one-shot CLI that checks an installation in over PQ-mTLS with a version string and a health flag, or reads fleet status; `bifrost-fleet-server` authorises, audits and journals check-ins, refuses a check-in for another installation, and reports version drift and stale nodes. The agent installs nothing, verifies nothing, receives no assignments, reports no digests and has no rollback; the `DeployAgent` trait in `bifrost/rollout` has no implementation anywhere.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Scope change:** A `bifrost-agent` CLI and `bifrost-fleet-server` do authenticated, audited check-ins with drift and staleness. Read this item as extending the agent to receive assignments, verify, install and roll back.
>
> **Still to do:**
> - Implement a long-running agent that receives signed, replay-protected assignments and implements `DeployAgent` (AC a, f)
> - Have the agent verify the artefact locally against its own trust anchor before install and report refusals (AC b)
> - Implement staged-then-switched install with a retained prior version, local rollback without the controller, and defined power-loss behaviour (AC c, d)
> - Extend `AgentReport` with installed artefact digests and add on-node digest drift detection with a configurable policy (AC e)
> - Add acceptance of assignments from a verified transit bundle for disconnected nodes
>
> Checked: `bifrost/fleet-server/src/bin/bifrost-agent.rs:55` · `bifrost/fleet-server/src/lib.rs:104` · `bifrost/fleet/src/agent.rs:15` · `bifrost/rollout/src/orchestrator.rs:37` · `bifrost/fleet-server/tests/serve.rs:182`

**Evidence.** `bifrost/fleet` is 134 lines modelling installations; `bifrost/rollout` (203 lines) is a state machine. Nothing installs. `clients/delivery` (160 lines) models client install records similarly.

**Prompt**

```
Build the agent that makes a rollout real.

1. Implement the fleet agent that runs on every managed node: authenticates with
   its workload identity over PQ-mTLS, reports inventory (installed artefacts,
   versions, digests, health, hardware profile), receives assignments, verifies
   artefacts locally before install (mg_verify — the target verifies for itself
   and never trusts the controller's word), installs, health-checks, and reports
   the outcome.
2. Local rollback: the agent retains the prior version and can roll back without
   contacting the controller — an unreachable controller must never leave a node
   stuck on a bad version.
3. Atomicity: install is atomic from the service's perspective (staged, then
   switched), with a documented behaviour if the node loses power mid-install.
4. Drift detection: the agent detects and reports local modification of installed
   artefacts (digest mismatch), and the policy for what it does about it
   (report-only, or restore) is configuration, not hard-coded.
5. Disconnected operation: an air-gapped node's agent accepts assignments from a
   verified transit bundle on removable media and reports back the same way.
6. Agent security: least privilege, no shell execution from controller
   instructions, signed assignments, and replay protection.
```

**Acceptance criteria**

- **AC-RF-B-2a — A real rollout installs.** *Given* an approved artefact and a fleet, *when* a rollout runs, *then* nodes install it, report success, and the reported digest matches the registry.
- **AC-RF-B-2b — Targets verify for themselves.** *Given* a tampered artefact presented to a node, *when* the agent processes it, *then* it refuses to install and reports the refusal, regardless of what the controller claims.
- **AC-RF-B-2c — Rollback works offline.** *Given* a failed health check with the controller unreachable, *when* the agent evaluates, *then* it rolls back locally and reports when connectivity returns.
- **AC-RF-B-2d — Power loss is survivable.** *Given* power loss mid-install, *when* the node restarts, *then* it is running either the old or the new version — never a broken hybrid — and reports which.
- **AC-RF-B-2e — Drift is reported.** *Given* a locally modified installed artefact, *when* the agent checks, *then* the digest mismatch is reported and alerted.
- **AC-RF-B-2f — Assignments are signed and non-replayable.** *Given* a replayed or unsigned assignment, *when* the agent receives it, *then* it refuses and records a security event.

**Done when:** a rollout reaches real nodes, installs, health-checks and rolls back — including in a disconnected site.

---

### RF-B-3 · Ring orchestration, progressive delivery and automatic abort

**Severity:** High · **Area:** Bifrost · **Depends on:** RF-B-2, RF-OBS-1

> **Status at 2026-10-12 — Open.** `bifrost/rollout` is still a library: a plan of named rings sized by install count (canary then broad), a boolean `HealthGate` per ring, and automatic revert of promoted rings when a gate fails. Nothing drives it from the fleet manager, and there is no ring membership by node attributes, soak time, signal-driven abort, concurrency or failure limit, kill switch, freeze window or dependency ordering.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Still to do:**
> - Wire the fleet manager to drive `Orchestrator::roll_out_live` against real agents (depends on RF-B-2)
> - Define rings as configuration by node attribute with soak times and promotion criteria, refusing promotion with the unmet criterion named (AC a)
> - Feed error-rate, latency and SLO-burn signals into the gate and record the abort evidence (AC b)
> - Add concurrency and failure-threshold limits and a dual-controlled global kill switch (AC c, d)
> - Add freeze periods with an audited emergency override, and dependency-ordered sequencing (AC e, f)
>
> Checked: `bifrost/rollout/src/plan.rs:56` · `bifrost/rollout/src/orchestrator.rs:152` · `bifrost/rollout/tests/rollout.rs:50` · `bifrost/fleet/src/lib.rs:227`

**Prompt**

```
Make progressive delivery operational.

1. Rings as configuration: define ring membership (canary → early → broad →
   production) by node attributes, not a hard-coded list, with soak times and
   promotion criteria per ring.
2. Automatic promotion and abort driven by real signals: health checks, error
   rate, latency, and SLO burn from RF-OBS-1. A ring does not promote until its
   soak passes; a regression aborts and rolls back automatically, and the
   decision evidence is recorded.
3. Blast-radius controls: maximum concurrent nodes, maximum concurrent failures
   before halt, and a global kill switch that stops every in-flight rollout,
   requiring dual control to engage and to release.
4. Dependency-aware ordering: services with a required upgrade order are
   sequenced, and an incompatible version pairing is refused before it is
   attempted.
5. Scheduling: change windows, freeze periods (with a dual-controlled emergency
   override that is loudly audited), and maintenance-window awareness.
6. Reuse this mechanism for model version canaries (RF-MS-3) rather than building
   a second rollout system.
```

**Acceptance criteria**

- **AC-RF-B-3a — Soak gates promotion.** *Given* a ring whose soak has not elapsed or whose criteria are unmet, *when* promotion is attempted, *then* it is refused with the unmet criterion named.
- **AC-RF-B-3b — Regression aborts automatically.** *Given* an error-rate or SLO-burn regression in a canary, *when* it is detected, *then* the rollout aborts, affected nodes roll back, and the evidence is recorded and alerted.
- **AC-RF-B-3c — Blast radius is bounded.** *Given* a rollout with a concurrency limit, *when* it runs, *then* no more than that many nodes are ever in flight, and the failure threshold halts it.
- **AC-RF-B-3d — Kill switch works.** *Given* several in-flight rollouts, *when* the kill switch is engaged under dual control, *then* all halt, and release requires dual control.
- **AC-RF-B-3e — Freeze is respected.** *Given* an active freeze, *when* a rollout is attempted, *then* it is refused unless an emergency override is exercised, and the override is audited with its justification.
- **AC-RF-B-3f — Ordering is enforced.** *Given* services with a required order, *when* a rollout would violate it, *then* it is refused before any node is touched.

**Done when:** rings, soak, auto-abort, kill switch and freeze are implemented and exercised against a real fleet.

---

### RF-B-4 · Air-gapped bundle workflow, end to end

**Severity:** High · **Area:** Bifrost · **Depends on:** RF-B-1, RF-SC-8

> **Status at 2026-10-12 — Partial.** `mg-bundle pack` and `mg-bundle import` (not `create`/`apply`) produce and verify a signed bundle offline, streaming in 16 MiB chunks with per-chunk digests, staging the output so a refused import leaves nothing; `deploy/air-gap-bundle/` now exists with a README, `produce.sh` and `install.sh` (verify, build images, helm deploy). A bundle is a single file: there is no differential bundle, no splitting across media, no signed receipt, no report-back or staleness in the fleet view, and no outbound evidence path.
> Criteria met 1 of 6 (b); open: a, c, d, e, f. Confidence: high.
>
> **Scope change:** `deploy/air-gap-bundle/` exists and the verbs are `mg-bundle pack` / `import`. Step 6 is done; what remains is receipts, differential bundles, media splitting and the outbound path.
>
> **Still to do:**
> - Emit a signed receipt on import and ingest it at the origin to update the fleet view with an explicit age (AC a, d)
> - Add differential bundles against a stated site baseline with a test that the result equals a full bundle (AC c)
> - Split a bundle across size-limited media with per-part integrity and resumable reassembly (AC f)
> - Add the outbound path for audit, telemetry and evidence with marking-based withholding (AC e)
> - Carry trust anchors and configuration in the bundle, and write the physical-media runbook after a demonstrated round trip
>
> Checked: `tools/mg-bundle/src/main.rs:181` · `bifrost/bundle/src/stream.rs:151` · `bifrost/bundle/tests/streaming.rs:131` · `tools/mg-bundle/tests/cli.rs:45` · `deploy/air-gap-bundle/install.sh:75`

**Evidence.** `bifrost/bundle` (228 lines) and `tests/ac_b_3.rs` implement and prove the bundle format and offline verification. `deploy/README.md` promises a `deploy/air-gap-bundle/` directory that does not exist. The operational workflow — produce, transport, ingest, apply, report back — is undefined.

**Prompt**

```
Turn the bundle format into an operational workflow.

1. Produce: `mg-bundle create` assembles a signed bundle from a set of promoted
   artefacts plus their SBOMs, signatures, attestations, the current trust
   anchors, model artefacts, configuration changes and the bundle manifest. It
   supports differential bundles against a stated site baseline so a site with
   most content already present receives only the delta.
2. Transport: define and document the media format, chunking for size-limited
   media, integrity verification per chunk, and a resumable transfer for
   multi-media transfers.
3. Ingest: `mg-bundle apply` at the receiving site verifies the whole chain
   offline against the site's anchors, refuses on ANY failure, applies
   atomically, and produces a signed receipt.
4. Report back: the receipt returns by the same physical path and updates the
   controller's fleet view, so a disconnected site's state is knowable at the
   origin — with an explicit staleness indicator so nobody mistakes an old
   receipt for current truth.
5. Bidirectional: the same mechanism carries audit exports, telemetry and
   incident evidence outward from the disconnected site, subject to marking-based
   egress rules (RF-API-6).
6. Write the runbook and populate deploy/air-gap-bundle/ — or correct
   deploy/README.md (RF-DOC-1).
```

**Acceptance criteria**

- **AC-RF-B-4a — Full round trip.** *Given* a promoted artefact set, *when* a bundle is created, physically transported, verified, applied and receipted, *then* the receiving site runs the new versions and the origin sees the receipt.
- **AC-RF-B-4b — Any tamper is refused.** *Given* a bundle with any altered byte, *when* applied, *then* it is refused offline with the failure identified, and nothing is applied.
- **AC-RF-B-4c — Differential bundles work.** *Given* a site at a known baseline, *when* a differential bundle is produced, *then* it contains only the delta and applies to exactly the same result as a full bundle.
- **AC-RF-B-4d — Staleness is explicit.** *Given* a site whose last receipt is old, *when* the fleet view is shown, *then* the staleness is displayed and the state is not presented as current.
- **AC-RF-B-4e — Outbound respects markings.** *Given* an outbound evidence bundle containing content above the permitted marking, *when* it is produced, *then* that content is withheld and the withholding is recorded.
- **AC-RF-B-4f — Chunked media works.** *Given* a bundle larger than the media, *when* split and transported, *then* it reassembles, verifies and applies.

**Done when:** the round trip is demonstrated with physical media and documented as a runbook.

---

### RF-B-5 · Release-risk intelligence wired to real evidence

**Severity:** Medium · **Area:** Bifrost · **Depends on:** RF-SC-4, RF-MI-4

> **Status at 2026-10-12 — Open.** `bifrost/risk` is unchanged in substance: `ReleaseContext` is five caller-supplied fields (a summary string, a tests flag, two counts and a ring string), the gate is a fixed set of booleans plus one score threshold, and `EV-A7-evidence` exists only as an eval function called from tests. A live `MimirAdvisor` bridge exists, but nothing constructs a context from a real SBOM diff, scan results or history, and nothing calls the crate outside its tests.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Build `ReleaseContext` from the real SBOM diff, scan findings, change set and test results, and record those inputs with the assessment (AC a)
> - Reject an assessment with any uncited factor inside `assess`, not only in an eval run by tests (AC b)
> - Make the gate a configurable policy mapping risk level to required approvals, and record overrides with who, why and what was shown (AC c)
> - Record predicted risk against observed outcome and report calibration on the Release Console (AC d)
>
> Checked: `bifrost/risk/src/context.rs:7` · `bifrost/risk/src/engine.rs:15` · `bifrost/risk/src/gate.rs:13` · `bifrost/risk/src/evals.rs:73` · `bifrost/risk/src/mimir_bridge.rs:44`

**Evidence.** `bifrost/risk` (780 lines) implements the A.7 release-risk prompt, the advisor and a gate. Its evidence today is supplied by the caller.

**Prompt**

```
Ground the release-risk analysis in real evidence.

1. Feed A.7 real inputs: the actual SBOM diff between the incumbent and the
   candidate, the actual scan findings from RF-SC-4, the actual change set
   (commits, touched subsystems), the actual test and Eval results, and the
   actual recent incident and rollback history for the affected services.
2. Enforce the EV-A7 contract: every risk claim must cite specific evidence; an
   uncited claim is rejected by the output guardrail rather than shown. The
   advisory-only nature is preserved — A.7 informs the gate, humans decide.
3. Make the gate policy explicit and configurable: which risk levels require
   which approvals, which are blocked outright, and how an override is recorded
   (who, why, what they were told).
4. Track calibration: record predicted risk against observed outcome (did it roll
   back? did it incident?) and report the advisor's accuracy over time. An
   uncalibrated risk score is worse than none because it manufactures confidence.
5. Surface it on the Release Console: the risk assessment, its evidence, the
   gate decision, and the calibration history.
```

**Acceptance criteria**

- **AC-RF-B-5a — Evidence is real.** *Given* a candidate release, *when* A.7 runs, *then* its inputs are the real SBOM diff, scan findings, change set and test results, verifiable from the record.
- **AC-RF-B-5b — Uncited claims are rejected.** *Given* an A.7 output containing a claim with no evidence citation, *when* the output guardrail runs, *then* the response is rejected, not displayed.
- **AC-RF-B-5c — The gate is policy, not prose.** *Given* a risk level, *when* the gate evaluates, *then* the required approvals follow the configured policy, and an override records who, why and what they were shown.
- **AC-RF-B-5d — Calibration is reported.** *Given* accumulated releases, *when* the Release Console is opened, *then* predicted-versus-observed risk accuracy is shown.

**Done when:** A.7 runs on real evidence with citation enforcement; the gate policy is configurable; calibration is tracked.

---

### RF-B-6 · Configuration and secret delivery

**Severity:** High · **Area:** Bifrost · **Depends on:** RF-B-2, RF-PER-4

> **Status at 2026-10-12 — Open.** Bifrost has no notion of a configuration artefact: bundles carry only service, tool, client, model-bundle and attestation components, and the fleet records versions only. `admin/config` has a signed, versioned config store and a drift reconciler, but as an unwired library unrelated to ring delivery; no `--check-config` dry run exists in the tree and there is no automated secrets-in-artefacts check.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: medium.
>
> **Still to do:**
> - Define configuration as a signed, versioned artefact type delivered through the same rings and rollback as code (AC a; depends on RF-B-2 and RF-B-3)
> - Add on-node schema validation and a dry run before apply, keeping the current config on failure (AC b)
> - Add an automated check that no secret value appears in any config artefact, bundle, log or UI, with references resolved from KMS on the node (AC c)
> - Declare hot-reload versus restart per setting and drive restarts through the rollout mechanism (AC d)
> - Report effective-versus-intended config drift per node with the differing keys (AC e)
>
> Checked: `tools/mg-bundle/src/main.rs:42` · `bifrost/fleet/src/agent.rs:15` · `admin/config/src/reconcile.rs:46` · `bifrost/rollout/src/orchestrator.rs:37`

**Prompt**

```
Deliver configuration and secrets with the same rigour as code.

1. Configuration as a versioned, signed, rollable artefact travelling the same
   rings, with the same verification and the same rollback. A configuration
   change is a release, not an out-of-band edit.
2. Validate before apply: schema validation and a dry-run on the target
   (RF-RT-1's --check-config) before the change takes effect; a node that cannot
   validate does not apply.
3. Secrets are never in configuration artefacts. Deliver references; the node
   resolves them from KMS under its workload identity. A secret value must never
   appear in a bundle, a manifest, a log or a UI.
4. Reload semantics: declare per setting whether it is hot-reloadable or requires
   a restart, and drive restarts through the rollout mechanism so a config change
   cannot cause an unplanned outage.
5. Drift: detect nodes whose effective configuration differs from the intended
   version and report it (feeds RF-RT-7's mg-posture).
```

**Acceptance criteria**

- **AC-RF-B-6a — Config rolls and rolls back.** *Given* a configuration change, *when* it is delivered through rings, *then* it applies progressively and rolls back on failure exactly as code does.
- **AC-RF-B-6b — Invalid config never applies.** *Given* a configuration failing validation on a node, *when* it is delivered, *then* the node keeps its current configuration and reports the failure.
- **AC-RF-B-6c — No secrets in artefacts.** *Given* any configuration artefact, bundle, log or UI surface, *when* inspected, *then* no secret value is present — only references.
- **AC-RF-B-6d — Restarts are orchestrated.** *Given* a change requiring restart, *when* applied, *then* restarts flow through the rollout mechanism with availability preserved.
- **AC-RF-B-6e — Drift is detected.** *Given* a node whose effective config differs from intent, *when* posture is checked, *then* the drift is reported with the differing keys.

**Done when:** configuration is a first-class rollable artefact; the secrets-never-in-artefacts check is automated.

---

# K — Heimdall: fusion reality

> **Status.** `heimdall/` is ~3 000 lines: the common operating picture with layers, feed gateway, entity fusion, classification, picture-sync, decision workflows and the A.8 option generator with its six Evals. The `OperatingPictureScreen` (314 lines) is the largest console screen in the tree. What is absent is any real feed, any real geospatial or temporal machinery, and any real-time transport.

---

### RF-H-1 · Real feed ingestion and normalisation

**Severity:** High · **Area:** Heimdall · **Depends on:** RF-API-6, RF-RT-2

> **Status at 2026-10-12 — Partial.** `heimdall-feed-server` runs as a mesh service: a source submits a signed observation over PQ-mTLS, the feed id must equal the certificate's identity, the signature is verified, the observation is fused and the picture is sealed at rest; `heimdall-source` is the CLI client. There is one generic observation format and no per-class adapters, no watermarks, no duplicate suppression (a resubmitted observation increments the track count), no marking on observations or entities (every reader is treated as UNCLASSIFIED against one resource), and no feed-health, staleness or degraded indication.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** `heimdall-feed-server` is a running PQ-mTLS service with signed, identity-bound ingest and a sealed picture, so feed authentication is largely there. What remains is markings, deduplication, feed health and adapters.
>
> **Still to do:**
> - Add a marking to feeds and observations, propagate it to entities, and filter `ReadPicture` by the reader's clearance without leaking existence (AC c)
> - Track per-feed last-arrival, raise an alert past a staleness threshold and mark affected entities degraded in the picture (AC d)
> - Suppress duplicates by observation id and add a test that out-of-order ingestion equals in-order ingestion (AC b, e)
> - Implement adapters for each feed class that normalise to the observation model and retain lineage to the source message (AC a)
> - Add backpressure, bounded buffering and per-feed quality metrics
>
> Checked: `heimdall/feed-server/src/lib.rs:166` · `heimdall/feed-server/src/lib.rs:308` · `heimdall/fusion/src/engine.rs:55` · `heimdall/picture/src/observation.rs:29` · `heimdall/feed-server/tests/serve.rs:187`

**Evidence.** `heimdall/feed-gateway` is 234 lines; `FeedsScreen` (106 lines) renders fixtures.

**Prompt**

```
Ingest real operational feeds.

1. Implement adapters for the realistic feed classes: message streams, sensor
   telemetry, structured event feeds, geospatial position reports, and file-drop
   batches — each with schema normalisation to the internal track/entity model,
   and each declaring its delivery semantics.
2. Ingest robustness: backpressure, bounded buffering, out-of-order and
   late-arriving data with watermarks, duplicate suppression, and per-feed
   quality metrics (rate, gaps, latency, malformed count).
3. Feed authentication and integrity: every feed source is authenticated, and
   where the source supports it, per-message integrity is verified. An
   unauthenticated feed is a configuration choice that is loudly recorded, not a
   default.
4. Marking at the boundary: a feed declares the markings of the data it carries,
   and every derived track, entity and picture element inherits them. Test that
   a lower-clearance operator cannot see a picture element derived from a
   higher-marked feed.
5. Feed health as an operational surface: gap detection, staleness, and an alert
   when a feed the picture depends on stops — a silently dead feed is the most
   dangerous failure in a fusion system, because the picture looks fine.
```

**Acceptance criteria**

- **AC-RF-H-1a — Real feeds ingest and normalise.** *Given* each supported feed class, *when* connected to a real source, *then* messages normalise into tracks and entities with lineage to the source message.
- **AC-RF-H-1b — Out-of-order data is handled.** *Given* late and out-of-order reports, *when* ingested, *then* the resulting picture matches in-order ingestion of the same data.
- **AC-RF-H-1c — Markings propagate to the picture.** *Given* a high-marked feed, *when* a lower-clearance operator views the picture, *then* the derived elements are not visible and their absence does not leak their existence.
- **AC-RF-H-1d — Dead feeds are loud.** *Given* a feed that stops, *when* the staleness threshold passes, *then* an alert fires and the picture marks the affected area as degraded rather than showing stale data as current.
- **AC-RF-H-1e — Duplicates are suppressed.** *Given* duplicated messages, *when* ingested, *then* tracks are not duplicated.

**Done when:** each feed class ingests from a real source; the dead-feed alert and degraded-picture indication are demonstrated.

---

### RF-H-2 · Entity resolution, tracking and correlation at scale

**Severity:** High · **Area:** Heimdall · **Depends on:** RF-H-1, RF-PER-3

> **Status at 2026-10-12 — Open.** Fusion still correlates by exact `entity_key` equality and keeps only first-seen, last-seen, a count and the latest attributes; there is no probabilistic matching, confidence, merge or split, state estimation or coasting. `heimdall/classify` assigns descriptive labels by rule, not security classification, so no aggregation rule exists, and the running feed server attaches no classifier at all. An entity records only the set of feed names that contributed, not which reports support which attribute.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** `heimdall/classify` assigns descriptive labels by rule; it does not do security classification, and the running feed server attaches no classifier. The aggregation-classification criterion needs markings from RF-H-1 first.
>
> **Still to do:**
> - Implement recorded, reversible merge and split of entities with audit entries (AC a)
> - Add a tracking filter with exposed uncertainty and coasting, and define the observed-versus-inferred visual language in the design system (AC b)
> - Record per-attribute provenance to source reports and expose it for drill-down (AC c)
> - Implement the aggregation classification rule and enforce the aggregate marking (AC d; depends on RF-H-1 markings)
> - Publish and gate a 10^5-track latency envelope with a benchmark (AC e)
>
> Checked: `heimdall/fusion/src/engine.rs:57` · `heimdall/picture/src/entity.rs:89` · `heimdall/classify/src/rules.rs:1` · `heimdall/feed-server/src/lib.rs:71`

**Evidence.** `heimdall/fusion` (312 lines) and `heimdall/classify` (237 lines) implement correlation and classification.

**Prompt**

```
Make fusion algorithmically real and operationally honest.

1. Entity resolution: probabilistic matching across feeds with configurable
   thresholds, explicit confidence scores, and a human-adjudication path for
   ambiguous matches. Every merge and split is recorded and reversible — an
   incorrect merge in an operating picture is an operational hazard, so undo is
   not optional.
2. Tracking: state estimation over time (a Kalman-class filter or equivalent
   appropriate to the domain), track initiation, maintenance, coasting through
   gaps, and termination — with the uncertainty exposed, not hidden.
3. Confidence is visible everywhere: the picture must never render an inferred
   position or identity with the same visual weight as an observed one. Design
   this into the design system (RF-DOC-6) so it is consistent.
4. Provenance: every entity attribute traces to the source reports supporting it,
   and the UI can drill from a picture element to its evidence.
5. Scale: 10^5 concurrent tracks with sub-second update propagation; publish and
   gate the envelope.
6. Classification: derive and propagate classification from constituent sources
   using the aggregation rule — a fusion of unclassified sources may itself be
   classified, and the system must apply the rule rather than assume the maximum
   of the inputs.
```

**Acceptance criteria**

- **AC-RF-H-2a — Merges are reversible and audited.** *Given* an incorrect merge, *when* it is split, *then* the prior entities are restored and both operations are audited.
- **AC-RF-H-2b — Uncertainty is visible.** *Given* an inferred or coasted track, *when* rendered, *then* its uncertainty is visually distinct from an observed position, and the distinction is defined in the design system.
- **AC-RF-H-2c — Evidence is reachable.** *Given* any entity attribute, *when* an operator drills in, *then* the supporting source reports are listed with timestamps and feeds.
- **AC-RF-H-2d — Aggregation classification is applied.** *Given* a fusion whose aggregate classification exceeds its inputs', *when* it is produced, *then* the aggregate marking is applied and enforced.
- **AC-RF-H-2e — Scale envelope holds.** *Given* 10^5 concurrent tracks, *when* updates flow, *then* propagation latency stays within the published bound.

**Done when:** resolution, tracking and aggregation-classification are implemented; the uncertainty visual language is in the design system; scale is proven.

---

### RF-H-3 · Real-time picture synchronisation across sites and clients

**Severity:** High · **Area:** Heimdall · **Depends on:** RF-API-3, RF-PER-5

> **Status at 2026-10-12 — Partial.** `heimdall/picture-sync` exports a full-picture snapshot as a signed delta, verifies the peer's signature before a last-write-wins merge, and can carry it over a PQ-mTLS session; tests show two nodes converging after one exchange. It is not used by the feed server or any binary, there is no push to clients, no incremental delta, resume, clearance or area filtering, degraded scoping or clock model, and the merge is not convergent when two nodes hold different states with equal `last_seen` (each side takes the other's).
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** Add a merge fix as the first step. The last-write-wins merge at `heimdall/picture/src/delta.rs:47` takes the incoming entity whenever it is at least as recent, so two nodes holding different state with equal `last_seen` each adopt the other's and do not converge.
>
> **Still to do:**
> - Define a deterministic tie-break so equal-timestamp conflicts converge, and add a reordering-and-partition convergence test (AC a)
> - Wire replication into the running feed server with push of incremental deltas and sequence-based resume or bounded resync (AC d)
> - Filter deltas server-side by the recipient's clearance and verify on the wire (AC c; depends on RF-H-1 markings)
> - Mark the picture degraded with the exact stale sources when a peer link is lost (AC b)
> - Add area-of-interest filtering and rate adaptation for constrained links (AC e)
>
> Checked: `heimdall/picture/src/delta.rs:47` · `heimdall/picture-sync/src/replicator.rs:35` · `heimdall/picture-sync/tests/ac_h_5.rs:76` · `heimdall/picture-sync/tests/rc_26.rs:182` · `heimdall/feed-server/src/lib.rs:153`

**Evidence.** `heimdall/picture-sync` (218 lines) implements sync; `PictureSyncScreen` (127 lines) renders fixtures.

**Prompt**

```
Make one picture, everywhere, provably.

1. Real-time transport: push picture deltas to every connected client and peer
   site with bounded latency, backpressure and reconnect-with-resume. Full-state
   resynchronisation is available and bounded, so a client can always recover.
2. Convergence: define and prove the consistency model. Two sites receiving the
   same events in different orders must converge to the same picture, and a test
   must demonstrate it under partition and reordering.
3. Degraded and disconnected operation: a site that loses its peer link continues
   with local feeds, marks the picture as degraded with the exact scope of what
   is stale, and reconciles deterministically on reconnection — with conflicts
   surfaced to a human where they are operationally significant, not
   auto-resolved.
4. Bandwidth adaptation: delta compression, per-client filtering by area of
   interest and clearance, and rate adaptation for constrained links. A client
   never receives elements above its clearance, and the filtering happens at the
   server.
5. Clock discipline: picture times use a monotonic, skew-aware model (RF-RT-4);
   a site with untrusted time marks its contributions accordingly.
```

**Acceptance criteria**

- **AC-RF-H-3a — Convergence under reordering and partition.** *Given* two sites receiving the same events in different orders, and a partition that heals, *when* they reconcile, *then* their pictures are identical.
- **AC-RF-H-3b — Degradation is explicit and scoped.** *Given* a lost peer link, *when* the picture is viewed, *then* it shows degraded status naming exactly which sources are stale, and continues serving local feeds.
- **AC-RF-H-3c — Clearance filtering is server-side.** *Given* a client below the clearance of some elements, *when* it syncs, *then* those elements are never transmitted, verified by inspecting the wire.
- **AC-RF-H-3d — Resume is lossless.** *Given* a client that disconnects and reconnects, *when* it resumes, *then* it receives exactly the missed deltas or a bounded full resync, with no gaps or duplicates.
- **AC-RF-H-3e — Constrained links degrade gracefully.** *Given* a low-bandwidth link, *when* sync runs, *then* it adapts rate and filters by area of interest while preserving correctness.

**Done when:** convergence under partition is proven by test; degraded scoping and server-side clearance filtering are demonstrated.

---

### RF-H-4 · Decision workflows and course-of-action support

**Severity:** High · **Area:** Heimdall · **Depends on:** RF-MI-4, RF-H-2

> **Status at 2026-10-12 — Partial.** `heimdall/workflow` is a single-step propose, authorise or deny, execute engine with a signed audit chain and a real actuator seam, with durable in-flight state in `heimdall/store`; `heimdall/decision-support` parses A.8 options, computes grounding of citations against the picture and has a live Mimir bridge. Neither is used by the feed server or any service. Ungrounded options are flagged but still returned, a non-consequential action can execute from `Proposed` with no human decision, and there are no multi-step definitions, deadlines, escalation, versioning, latency budget or full decision-lineage record.
> Criteria met 0 of 6; open: a, b, d, e, f; not verifiable by reading: c. Confidence: high.
>
> **Scope change:** Two code changes are needed before the tests this item asks for can pass. `heimdall/workflow/src/engine.rs:162` lets a non-consequential action execute from `Proposed` with no recorded human decision (criterion a), and `request_options` (`heimdall/decision-support/src/adapter.rs:62`) returns ungrounded options flagged, not rejected (criterion b). Serving options from the running service is RF-CA-10.
>
> **Still to do:**
> - Reject, rather than flag, any option set containing an uncited or ungrounded option inside `request_options` (AC b)
> - Close the path that lets a non-consequential action execute from `Proposed`, or scope it out explicitly, and add a test asserting no route from option generation to execution without a recorded human decision (AC a)
> - Record picture state, options shown, evidence, and model and prompt versions with each decision so it can be reconstructed (AC d)
> - Add workflow definitions with roles, deadlines, escalation on timeout and versioning (AC e)
> - Enforce an option-generation latency budget that reports when it cannot be met, and expose the whole flow from a running service (AC f)
>
> Checked: `heimdall/workflow/src/engine.rs:162` · `heimdall/decision-support/src/adapter.rs:88` · `heimdall/workflow/tests/ac_h_4.rs:96` · `heimdall/decision-support/tests/ac_h_3.rs:134` · `heimdall/decision-support/src/mimir_bridge.rs:27`

**Evidence.** `heimdall/workflow` (358 lines) and `heimdall/decision-support` (871 lines) implement workflows and the A.8 option generator with six Evals (`EV-A8-cited/-grounded/-ranked/-neutral/-insuff/-advisory`).

**Prompt**

```
Make decision support operational and provably advisory.

1. Workflow engine: definable multi-step workflows with roles, approvals,
   deadlines, escalation on timeout, delegation, parallel and conditional
   branches, and full audit. Workflows are versioned; a running instance
   completes on the version it started with.
2. A.8 on real grounding: options are generated from the real picture and real
   fabric evidence, with every option citing the evidence supporting it. Enforce
   EV-A8 in production, not only in tests: an uncited or ungrounded option is
   rejected by the output guardrail; ranking must be explained; the output must
   be neutral in presentation; insufficiency must produce an explicit
   "insufficient evidence" rather than a speculative option.
3. Advisory is structural: the system cannot execute a course of action. Options
   flow into a workflow where a human decides; the decision, the decider, the
   options shown and the evidence they were shown are all recorded. Add a test
   asserting no code path exists from option generation to execution.
4. Decision lineage: reconstruct any past decision completely — the picture state
   at the time, the options presented, the evidence, the model and prompt
   versions, who decided and when. This is what the Decision Lineage screen must
   show.
5. Time-critical operation: options generate within an operationally meaningful
   latency budget or explicitly report that they cannot, rather than arriving too
   late to matter.
```

**Acceptance criteria**

- **AC-RF-H-4a — No execution path.** *Given* the source tree, *when* traced from option generation, *then* no path reaches an executing action without an intervening recorded human decision.
- **AC-RF-H-4b — Uncited options are rejected.** *Given* generated options where one lacks evidence citations, *when* the guardrail runs, *then* the response is rejected rather than displayed.
- **AC-RF-H-4c — Insufficiency is stated.** *Given* insufficient grounding, *when* options are requested, *then* the system states insufficiency and offers no speculative option.
- **AC-RF-H-4d — Decisions are reconstructible.** *Given* a past decision, *when* its lineage is opened, *then* the picture state, options, evidence, model and prompt versions, decider and time are all shown.
- **AC-RF-H-4e — Escalation works.** *Given* a workflow step past its deadline, *when* the deadline passes, *then* it escalates per definition and the escalation is audited.
- **AC-RF-H-4f — Latency budget is honoured or reported.** *Given* the option-generation budget, *when* it cannot be met, *then* the system reports it rather than returning late.

**Done when:** the no-execution-path test is green; A.8 runs on real grounding with EV-A8 enforced in production; lineage reconstruction works.

---

### RF-H-5 · Geospatial and temporal capability

**Severity:** Medium · **Area:** Heimdall · **Depends on:** RF-H-2

> **Status at 2026-10-12 — Open.** There is still no geospatial or temporal machinery: an observation is a kind, a correlation key, free-form attributes and one `observed_at` timestamp, with no position type, reference frame, spatial index or event-versus-observation time. The picture store keeps only current state, and the Heimdall console runs on `HeimdallMock` with no map renderer or tile source.
> Criteria met 0 of 5; open: a, b, d, e; not verifiable by reading: c. Confidence: high.
>
> **Still to do:**
> - Add a position type that always carries its coordinate reference system and accuracy (AC a)
> - Add a spatial index with bounding-box, radius, polygon and nearest-neighbour queries and a published bound at 10^5 entities (AC b)
> - Retain picture history and separate event time from observation time to support as-of queries and replay (AC d)
> - Render the map in the console from in-enclosure vector tiles with a test that no request leaves the enclosure (AC c)
> - Make the picture keyboard-operable and not colour-dependent, and audit it (AC e)
>
> Checked: `heimdall/picture/src/observation.rs:29` · `heimdall/picture/src/entity.rs:14` · `heimdall/feed-server/src/lib.rs:348` · `products/heimdall/ui/src/main.tsx:17`

**Evidence.** No geospatial machinery exists anywhere: no coordinate systems, no projections, no spatial indexing, no map rendering. `OperatingPictureScreen` draws an abstract layout.

**Prompt**

```
Add the geospatial and temporal capability a common operating picture requires.

1. Coordinate handling: geodetic and projected coordinate systems, datum
   transformations, and explicit precision and accuracy metadata. Never store or
   display a position without its reference frame.
2. Spatial indexing and query: bounding-box, radius, polygon containment and
   nearest-neighbour over 10^5+ entities with sub-second response.
3. Map rendering in the console: vector tiles served from within the enclosure
   (never an external tile provider — that would be both an egress violation and
   an intelligence leak about what is being viewed), with offline basemaps
   delivered by the Bifrost bundle path, layer control, and the design system's
   visual language for markings and confidence.
4. Temporal: time-slider replay of the picture, temporal queries ("what did we
   believe at 14:32"), and a clear distinction between event time and
   observation time throughout.
5. Accessibility: the picture must be operable without a mouse and
   comprehensible without colour alone — a map is where accessibility is most
   often abandoned, and where the operational cost of abandoning it is highest
   (RF-UI-3).
```

**Acceptance criteria**

- **AC-RF-H-5a — No unreferenced positions.** *Given* any stored or displayed position, *when* inspected, *then* it carries its coordinate reference system and accuracy.
- **AC-RF-H-5b — Spatial queries scale.** *Given* 10^5 entities, *when* bounding-box, radius, polygon and nearest-neighbour queries run, *then* each returns within the published bound.
- **AC-RF-H-5c — No external tile requests.** *Given* the console rendering a map, *when* network activity is inspected, *then* no request leaves the enclosure.
- **AC-RF-H-5d — Time travel in the picture.** *Given* a past instant, *when* selected on the time slider, *then* the picture shows what was believed at that instant, distinguishing event from observation time.
- **AC-RF-H-5e — Map is accessible.** *Given* keyboard-only operation and a screen reader, *when* the picture is used, *then* every layer, entity and control is reachable and announced, and no information is conveyed by colour alone.

**Done when:** in-enclosure map rendering works offline; spatial and temporal queries meet their envelopes; the accessibility audit passes.

---

### RF-H-6 · Alerting, watch conditions and operator attention management

**Severity:** Medium · **Area:** Heimdall · **Depends on:** RF-H-2, RF-OBS-4

> **Status at 2026-10-12 — Open.** Nothing in `heimdall/` implements watch conditions, alerts, acknowledgement, suppression, handover or operator presence; the classify rules engine evaluates conditions over an entity only to produce a label. The feed server exposes just `Ingest` and `ReadPicture`.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Implement operator-defined watch conditions evaluated on each picture change with a stated latency (AC a)
> - Add alert deduplication, grouping and acknowledgement, and report acknowledgement rate and time-to-acknowledge per alert type (AC b)
> - Add time-boxed, audited suppression with automatic expiry and a visible indicator (AC c)
> - Produce and retain an auditable shift-handover record (AC d)
> - Push acknowledgements and decisions to other operators on the same area without a refresh (AC e)
>
> Checked: `heimdall/classify/src/rules.rs:13` · `heimdall/feed-server/src/lib.rs:153` · `heimdall/fusion/src/engine.rs:92`

**Prompt**

```
Manage operator attention as a first-class concern.

1. Watch conditions: operator-defined rules over the picture (entity enters
   area, track crosses threshold, feed goes stale, classification changes),
   evaluated continuously with bounded latency.
2. Alert presentation: prioritised, deduplicated, grouped, and acknowledgeable,
   with an explicit design that resists alert fatigue — the failure mode of every
   fusion system. Measure acknowledgement rates and time-to-acknowledge and treat
   a low rate as a defect in the alert, not the operator.
3. Handover: a shift handover surface summarising what changed, what is
   outstanding, what was acknowledged and by whom — recorded and auditable.
4. Suppression is time-boxed and audited, with automatic expiry and a visible
   indicator that something is suppressed.
5. Multi-operator awareness: who is viewing what, who has acknowledged what, and
   who has taken which decision — so two operators do not silently duplicate or
   contradict each other.
```

**Acceptance criteria**

- **AC-RF-H-6a — Watch conditions fire promptly.** *Given* a defined condition, *when* the picture satisfies it, *then* the alert fires within the stated latency.
- **AC-RF-H-6b — Fatigue is measured.** *Given* accumulated alerts, *when* the alert-quality report is opened, *then* acknowledgement rate and time-to-acknowledge are shown per alert type.
- **AC-RF-H-6c — Suppression expires visibly.** *Given* a suppressed alert, *when* suppressed, *then* an indicator is visible and the suppression expires automatically; both are audited.
- **AC-RF-H-6d — Handover is complete and recorded.** *Given* a shift change, *when* handover is produced, *then* it lists changes, outstanding items and acknowledgements, and is retained as an auditable record.
- **AC-RF-H-6e — Concurrent operators see each other.** *Given* two operators on the same area, *when* one acknowledges or decides, *then* the other sees it without refreshing.

**Done when:** watch conditions, fatigue measurement, handover and multi-operator awareness are implemented and exercised.

---

# L — Admin Control Panel depth

> **Status.** The admin backend is the most complete product: 3 466 lines across eight crates covering role-separated identity with no super-admin, risk-tiered dual control, time-boxed grants, policy simulation using the real PDP, GitOps configuration, break-glass with alerting, a PQ-signed audit ledger, service identity, and the three admin AI prompts with their Evals. All 13 `AC-AD-*` pass. The gap is that the console is wired to `AdminMock` and the backend has no process, so none of it is reachable by a human.

---

### RF-AD-1 · Wire the Admin console to the real admin backend

**Severity:** High · **Area:** Admin · **Depends on:** RF-API-3, RF-IAM-3 · **Blocks:** RF-AD-2..7

> **Status at 2026-10-12 — Partial.** The admin console still constructs `AdminMock` and every screen reads fixtures from `sdk/ts/src/products/admin.ts`. No admin crate has a binary; the only reachable admin surface is the http-gateway's `/api/v1/admin/` enrolment API (challenge, request, approve, list), which enforces passkey proof and dual control and is driven by the `mg-admin` CLI. Only `admin-identity` and `admin-privilege` are linked into a service; break-glass, config, sim, audit, orchestrator and ai-assist are libraries exercised by tests alone.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** The http-gateway serves `/api/v1/admin/` enrolment under passkey and dual control, driven by `mg-admin`. Step 1 is to decide between extending that API and a separate admin service (RF-CA-11 assumes the latter for alerts). The console is still on `AdminMock`.
>
> **Still to do:**
> - Expose the full AdminGateway surface from a running service (extend the gateway's admin API or stand up an admin service) through the guarded orchestrator
> - Replace `AdminMock` with an HTTP-backed client and make all eight screens read and write real state (AC a)
> - Make dual-controlled actions submittable and approvable from the console, not only from `mg-admin` (AC c)
> - Add the no-privileged-bypass test comparing the admin service's capabilities with the subsystem APIs (AC b)
> - Delete the admin fixtures and verify the build bundles none, and that every console action lands in the ledger (AC d, e)
>
> Checked: `products/admin/ui/src/main.tsx:17` · `sdk/ts/src/products/admin.ts:397` · `services/http-gateway/src/lib.rs:355` · `services/http-gateway/src/admin_api.rs:274` · `tools/mg-admin/tests/enrol.rs:96`

**Evidence.** [`products/admin/ui/src/main.tsx:17`](../../../products/admin/ui/src/main.tsx) — `const gateway: AdminGateway = new AdminMock();`. [`sdk/ts/src/products/admin.ts`](../../../sdk/ts/src/products/admin.ts) is 623 lines of interface plus fixtures. The admin crates have no binary.

**Prompt**

```
Make the Admin console operate the real enclosure.

1. Stand up the admin backend as a service (RF-RT-1) exposing the AdminGateway
   surface over the RF-API-2 RPC layer and the RF-API-3 edge.
2. Replace AdminMock with the real client. Every one of the eight screens
   (Overview, IAM, Approvals, Registry, Config, Ledger, BreakGlass, Copilot)
   must read and write real state.
3. Enforce the admin panel's own constraint: it COMPOSES subsystem APIs and is
   bound by the same policy and audit — no privileged bypass. Add a test proving
   the admin service has no capability a suitably-roled operator could not
   exercise through the subsystem's own API.
4. Every mutating action from the console flows through the existing dual-control
   and simulation machinery — which already exists and is tested; it must now be
   reachable from the UI, not only from Rust tests.
5. Delete the admin fixtures once every screen is live.
```

**Acceptance criteria**

- **AC-RF-AD-1a — Every screen is live.** *Given* the running enclosure, *when* each admin screen is opened, *then* it displays real state and its actions affect real subsystems.
- **AC-RF-AD-1b — No privileged bypass.** *Given* the admin service, *when* its capabilities are compared with the subsystem APIs, *then* it holds none a suitably-roled operator lacks, proven by test.
- **AC-RF-AD-1c — Dual control is reachable from the UI.** *Given* a high-risk action initiated in the console, *when* it is submitted, *then* it enters the real dual-control flow and cannot complete without a second, distinct approver.
- **AC-RF-AD-1d — Everything is audited.** *Given* any console action — committed, denied or simulated — *when* the ledger is queried, *then* an entry exists (this is AC-AD-6, now exercised through the UI).
- **AC-RF-AD-1e — Fixtures are gone.** *Given* the admin console build, *when* inspected, *then* no fixture data is bundled.

**Done when:** all eight screens operate the real backend; the no-bypass test is green; fixtures are deleted.

---

### RF-AD-2 · Break-glass: end-to-end operational flow

**Severity:** High · **Area:** Admin · **Depends on:** RF-AD-1, RF-IAM-1, RF-OBS-4

> **Status at 2026-10-12 — Partial.** `admin/breakglass` now has a `DurableAlerter` that retains and retries undelivered alerts, a controller that issues a scoped, time-boxed capability, revokes it on expiry and audits grant, expiry and review, and a sealed store that preserves the original expiry across restart. The only alert bus is the in-memory `SecurityOfficerBus` stand-in, there is no escalation or multiple channels, no early closure, actions taken inside a window are not marked, the crate is linked into no service, and the console screen is fixtures.
> Criteria met 1 of 6 (c); open: a, b, d, e, f. Confidence: high.
>
> **Scope change:** A `DurableAlerter` with retry, a scoped-capability controller with audited expiry, and restart-safe expiry exist; step 4 is done. Step 1 reduces to implementing a real `AlertBus` — the only one is a test stand-in. See RF-CA-11.
>
> **Still to do:**
> - Implement a real `AlertBus` to the alerting path with every configured channel and escalation on delivery failure, and persist the pending queue (AC a)
> - Run the controller inside a service and have subsystem authorisation consult it, auditing out-of-scope attempts as denials (AC b)
> - Mark every action taken under a window with the window id in the ledger (AC e)
> - Build the console flow on real state: request, second approver, scope, countdown, early closure and a review that cannot be dismissed (AC d)
> - Schedule the break-glass drill including alert delivery and record its first run (AC f)
>
> Checked: `admin/breakglass/src/alert.rs:146` · `admin/breakglass/src/controller.rs:42` · `admin/breakglass/tests/rc_32.rs:79` · `admin/breakglass/tests/durable_restart.rs:57` · `admin/breakglass/src/service.rs:109`

**Evidence.** `admin/breakglass` (352 lines) implements windows and an in-memory alerter (`alert.rs:33` — "A deterministic in-memory alerter (tests / dev)"). `BreakGlassScreen` is 186 lines of fixtures.

**Prompt**

```
Make break-glass a complete, exercised operational capability.

1. Replace the in-memory alerter with real alerting through RF-OBS-4: opening a
   window pages the security on-call immediately, by every configured channel,
   and in an air-gapped site by the local path. An alert that fails to deliver
   escalates; it must be impossible to open a window quietly.
2. Complete the flow in the UI: request with justification, second-approver
   authorisation, explicit scope selection (never blanket), the live countdown,
   the running list of actions taken within the window, voluntary early closure,
   automatic expiry, and the mandatory post-incident review that the window
   cannot be closed administratively without.
3. Constrain the window: actions within it are still policy-evaluated and still
   audited, marked as break-glass, and the scope limits what may be done. A
   break-glass window is elevated access, not unlimited access.
4. Survive restart with the original expiry (AC-RF-PER-4b).
5. Test the whole flow as an exercise, including the alert delivery, and make
   that exercise a scheduled operational drill with a recorded outcome.
```

**Acceptance criteria**

- **AC-RF-AD-2a — Opening always alerts.** *Given* a break-glass request, *when* the window opens, *then* alerts deliver on every configured channel, and a delivery failure escalates. No path opens a window without an alert attempt.
- **AC-RF-AD-2b — Scope constrains.** *Given* a window scoped to one subsystem, *when* an action outside the scope is attempted, *then* it is denied and audited.
- **AC-RF-AD-2c — Expiry is automatic and honest.** *Given* an open window, *when* its time elapses, *then* elevated access ends immediately and the expiry is audited; a restart does not extend it.
- **AC-RF-AD-2d — Review is mandatory.** *Given* a closed window, *when* the review is outstanding, *then* it is prominently outstanding in the console and cannot be dismissed without completion.
- **AC-RF-AD-2e — Every action is marked.** *Given* actions within a window, *when* the ledger is queried, *then* each is marked as break-glass with the window id.
- **AC-RF-AD-2f — The drill runs.** *Given* the scheduled drill, *when* it executes, *then* the full flow including alert delivery is exercised and the outcome recorded.

**Done when:** real alerting is wired; the complete UI flow works; the drill is scheduled and its first run recorded.

---

### RF-AD-3 · GitOps configuration: real repository integration and drift reconciliation

**Severity:** Medium · **Area:** Admin · **Depends on:** RF-AD-1, RF-B-6

> **Status at 2026-10-12 — Partial.** `admin/config` is a signed, hash-chained, append-only config store of its own (not a Git repository) with diff and rollback, plus a `Reconciler` that detects per-subsystem drift with the differing keys and converges it through a `Subsystem` trait that has only in-memory test implementations. `admin/orchestrator` refuses a commit that was not simulated, but no test exercises that refusal. There is no Git integration, no drift classes or policy, no air-gapped mirror, and none of it is linked into a service.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** The tree has its own signed, hash-chained configuration store and a drift `Reconciler`, not a Git repository with commit signatures. Decide between Git and the existing store before working criterion a.
>
> **Still to do:**
> - Decide whether desired state comes from a signed Git repository or the existing signed store, then make reconciliation refuse and alert on an unsigned or invalid commit (AC a)
> - Implement real `Subsystem` adapters and classify drift so safe classes auto-reconcile and security-relevant ones are propose-only (AC b)
> - Add a test that a commit without a simulation is refused, and show the simulated effect to the approver (AC c)
> - Route rollback through the guarded orchestrator flow with a recorded reason (AC d)
> - Carry configuration by bundle to a local mirror so a disconnected site reconciles offline (AC e; depends on RF-B-6)
>
> Checked: `admin/config/src/reconcile.rs:206` · `admin/config/src/store.rs:106` · `admin/orchestrator/src/lib.rs:134` · `admin/config/tests/ac_ad_7.rs:104` · `admin/config/src/reconcile.rs:347`

**Evidence.** `admin/config` (372 lines) implements a config store with signing. `ConfigScreen` (166 lines) is fixtures. There is no repository integration.

**Prompt**

```
Complete the GitOps loop.

1. Repository integration: read desired configuration from a signed Git
   repository (commit signature verification mandatory — an unsigned commit is
   never applied), with a local mirror for air-gapped sites updated by bundle
   transit.
2. Reconciliation: continuously compare desired against actual, report drift,
   and reconcile per policy (auto-reconcile for declared-safe classes,
   propose-only for anything security-relevant).
3. Change flow: proposed change → automatic validation → policy simulation
   (using admin/sim, which already gives exact fidelity because it is the real
   PDP) → approval → signed commit → progressive rollout (RF-B-6) → verification.
   A configuration change must be as reviewable as a code change.
4. Rollback: revert to any prior configuration version with the same rigour, and
   record why.
5. Secrets: never in the repository. References only, resolved from KMS at the
   node (RF-B-6).
```

**Acceptance criteria**

- **AC-RF-AD-3a — Unsigned commits never apply.** *Given* an unsigned or invalidly-signed commit, *when* reconciliation runs, *then* it is refused and alerted.
- **AC-RF-AD-3b — Drift is detected and classified.** *Given* configuration drift, *when* detected, *then* it is reported with the differing keys and handled per the class's policy.
- **AC-RF-AD-3c — Simulation precedes approval.** *Given* a proposed change, *when* it reaches approval, *then* the approver is shown the exact simulated effect, and approval without a simulation result is impossible.
- **AC-RF-AD-3d — Rollback works.** *Given* any prior version, *when* rolled back to, *then* it applies through the same flow and the reason is recorded.
- **AC-RF-AD-3e — Air-gapped sites reconcile.** *Given* a disconnected site, *when* a configuration bundle transits, *then* its local mirror updates and reconciliation proceeds offline.

**Done when:** signed-commit enforcement, drift reconciliation and simulation-before-approval are live, including offline.

---

### RF-AD-4 · Approvals, delegation and the operator's work queue

**Severity:** High · **Area:** Admin · **Depends on:** RF-AD-1, RF-IAM-5

> **Status at 2026-10-12 — Open.** The backend still enforces dual control and refuses self-approval, now also reachable for principal enrolment through the gateway and `mg-admin`, but a refused self-approval is deliberately not written to the audit chain. There is no unified work queue, no approval context with simulated effect, no delegation, and no request expiry, escalation or notification; `ApprovalsScreen` renders fixtures.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: medium.
>
> **Scope change:** Criterion c ("refused and audited") contradicts tested behaviour: `admin/privilege/tests/ac_ad_3_4_13.rs:222` asserts that a refused self-approval seals no audit entry. One of the two must change.
>
> **Still to do:**
> - Audit refused self-approval attempts; today the test asserts no entry is sealed (AC c)
> - Build one prioritised queue of approvals, break-glass reviews, drift items and expiring grants on real state (AC a; depends on RF-AD-1)
> - Bind approval to a displayed simulation result so approving without it is impossible (AC b)
> - Add time-boxed, scoped, revocable delegation with both delegate and delegator recorded (AC d)
> - Add request deadlines with escalation and an explicit expired state that notifies the requester (AC e)
>
> Checked: `admin/privilege/tests/ac_ad_3_4_13.rs:222` · `tools/mg-admin/tests/enrol.rs:189` · `services/http-gateway/src/admin_api.rs:338` · `products/admin/ui/src/main.tsx:17`

**Evidence.** `admin/privilege` implements risk tiers, dual control and time-boxed tokens. `ApprovalsScreen` (155 lines) renders fixtures.

**Prompt**

```
Build the approval experience the dual-control machinery deserves.

1. A unified work queue: everything awaiting this operator — approvals, break-
   glass reviews, recertification campaigns, drift items, security alerts,
   expiring grants — in one prioritised, filterable place. An operator should
   never have to visit eight screens to find out what needs them.
2. Approval context: the approver sees WHAT is being requested, WHO requested it,
   WHY (mandatory justification), the simulated effect, the risk tier and its
   basis, the requester's recent history, and what happens if they decline.
   Approving without seeing the simulated effect must be impossible.
3. Separation of duties enforced at approval time: the requester can never
   approve; conflicting-role holders cannot approve each other's requests; and
   an attempt is refused and audited.
4. Delegation with an audit trail: time-boxed, scoped, revocable, and the
   delegate's actions attributed to both delegate and delegator.
5. Expiry and escalation: requests expire; approaching expiry escalates;
   expired requests are recorded as expired, never silently dropped.
6. Notifications through RF-API-6 with an air-gapped local path.
```

**Acceptance criteria**

- **AC-RF-AD-4a — One queue.** *Given* an operator with pending items of every type, *when* they open the console, *then* all appear in one prioritised queue.
- **AC-RF-AD-4b — Blind approval is impossible.** *Given* an approval request, *when* the approver acts, *then* they have been shown the simulated effect; no path permits approval without it.
- **AC-RF-AD-4c — Self-approval is refused.** *Given* a requester attempting to approve their own request, *when* they try, *then* it is refused and audited.
- **AC-RF-AD-4d — Delegation is attributed.** *Given* a delegated approval, *when* the ledger is queried, *then* both delegate and delegator are recorded.
- **AC-RF-AD-4e — Expiry is explicit.** *Given* an unactioned request past its deadline, *when* it expires, *then* it is recorded as expired and the requester is notified.

**Done when:** the unified queue is live; blind approval is structurally impossible; delegation and expiry are exercised.

---

### RF-AD-5 · Governance surfaces: policy authoring, markings, purposes and retention

**Severity:** High · **Area:** Admin · **Depends on:** RF-AD-1, RF-PER-6

> **Status at 2026-10-12 — Open.** No authoring surface exists: `admin/sim` still simulates a proposed policy against the real PDP and flags `HIGH_BLAST_GRANTS`, and `sim_explainer` produces a summary, but there is no rule conflict detection, no marking-lattice, purpose or retention administration, and the console screens are fixtures. The orchestrator does not depend on `admin/ai-assist`, so the explainer cannot influence authorisation, but no test asserts that.
> Criteria met 0 of 5; open: a, b, c, e; not verifiable by reading: d. Confidence: medium.
>
> **Still to do:**
> - Implement policy rule validation with detection of unreachable and contradictory rules before commit (AC a)
> - Surface the high-blast flag in the console with a required explicit acknowledgement (AC b; depends on RF-AD-1)
> - Add marking-lattice administration with well-formedness validation and simulation (AC c)
> - Add purpose, retention and legal-hold administration under simulation and dual control (AC e)
> - Add a test that the A.AD2 explainer has no input to the authorisation decision and label its output as explanation (AC d)
>
> Checked: `admin/sim/src/simulate.rs:11` · `admin/sim/src/report.rs:29` · `admin/ai-assist/src/sim_explainer.rs:95` · `admin/orchestrator/src/lib.rs:31`

**Evidence.** `crypto/policy` implements the PDP; `admin/sim` simulates changes against it. `PoliciesScreen` (230 lines, Midgard console) and `RegistryScreen` (208 lines, Admin) render fixtures. There is no authoring experience.

**Prompt**

```
Give governance a first-class authoring and administration surface.

1. Policy authoring: create, edit, version, review and simulate policy rules with
   validation, conflict detection (rules that can never fire, rules that
   contradict), and a plain-language explanation of each rule's effect generated
   by A.AD2 — whose job is explanation, never authorisation.
2. Marking and clearance administration: define the marking lattice, the
   clearance levels, the dominance relations and the aggregation rules, with
   validation that the lattice is well-formed and simulation of any change.
3. Purpose administration: define lawful purposes, which roles may assert them,
   and which data classes each permits — feeding the session purpose binding
   from RF-IAM-3.
4. Retention and legal hold administration (RF-PER-6): define, simulate and
   commit retention policies; place and release holds; see what is held and why.
5. Everything here is dual-controlled, simulated before commit, and audited. A
   policy change is the highest-blast-radius action in the platform and must feel
   like it — the existing HIGH_BLAST_GRANTS threshold in admin/sim should surface
   prominently in the UI.
```

**Acceptance criteria**

- **AC-RF-AD-5a — Authoring with conflict detection.** *Given* a rule that can never fire or contradicts another, *when* it is authored, *then* the conflict is reported before commit.
- **AC-RF-AD-5b — Blast radius is prominent.** *Given* a change exceeding the high-blast threshold, *when* it is reviewed, *then* the console displays the scale prominently and requires explicit acknowledgement.
- **AC-RF-AD-5c — Lattice changes are validated and simulated.** *Given* a marking-lattice change, *when* proposed, *then* well-formedness is validated and the access effect is simulated before commit.
- **AC-RF-AD-5d — A.AD2 explains, never decides.** *Given* the explanation shown to an approver, *when* the code path is inspected, *then* the explainer has no influence on the authorisation decision, and its output is labelled as explanation.
- **AC-RF-AD-5e — Retention is administrable.** *Given* retention policies and holds, *when* administered through the console, *then* changes are simulated, dual-controlled and audited.

**Done when:** authoring, lattice, purpose and retention surfaces are live with simulation and dual control.

---

### RF-AD-6 · Operational overview: platform health, posture and integrity

**Severity:** High · **Area:** Admin · **Depends on:** RF-OBS-1, RF-OBS-5, RF-RT-7

> **Status at 2026-10-12 — Open.** `OverviewScreen` still renders `AdminMock` fixtures and no backend composes health, SLO, posture, audit-chain, capacity, expiry or break-glass signals into one ranked view. Individual sources exist in places (for example fleet status and break-glass pending reviews) but nothing aggregates them or serves them to the console.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: medium.
>
> **Still to do:**
> - Build a backend endpoint that composes the live signals named in the prompt (depends on RF-AD-1 and the observability items)
> - Rank items by urgency with a recommended action first (AC b)
> - Add drill-through to the resolving screen with a runbook link per item (AC c)
> - Compute and show time horizons for certificate and key expiry and storage exhaustion (AC d)
> - Show the age of every bundle-derived element for air-gapped sites (AC e; depends on RF-B-4 receipts)
>
> Checked: `products/admin/ui/src/main.tsx:17` · `sdk/ts/src/products/admin.ts:263` · `bifrost/fleet-server/src/lib.rs:167` · `admin/breakglass/src/service.rs:109`

**Evidence.** `OverviewScreen` (134 lines) renders fixtures.

**Prompt**

```
Make the Overview screen the enclosure's true operational picture.

1. Compose live signals: service health and versions (RF-OBS-5), SLO attainment
   and error budgets (RF-OBS-1), security posture deviations (RF-RT-7's
   mg-posture), audit chain verification status (RF-PER-2), storage capacity and
   scrub findings (RF-PER-8), certificate and key expiry horizons (RF-SC-8,
   RF-IAM-6), supply-chain verification failures, open break-glass windows,
   pending approvals, active incidents and reconciliation divergences.
2. Rank by what needs attention, not by category. The top of this screen is the
   most valuable real estate in the product; it must answer "is anything wrong,
   and what should I do first".
3. Every item drills through to the surface that resolves it, and carries its
   runbook link.
4. Time horizons: show what will break — expiring certificates, filling disks,
   exhausting budgets — with enough lead time to act.
5. Air-gapped sites show their own state plus the staleness of anything received
   by bundle transit, so nothing old is mistaken for current.
```

**Acceptance criteria**

- **AC-RF-AD-6a — Every signal is live.** *Given* the Overview screen, *when* opened, *then* every element derives from a live source and no fixture remains.
- **AC-RF-AD-6b — Attention-ranked.** *Given* multiple conditions of differing severity, *when* the screen renders, *then* the most urgent is first with its recommended action.
- **AC-RF-AD-6c — Drill-through works.** *Given* any item, *when* selected, *then* it navigates to the resolving surface with context preserved and its runbook linked.
- **AC-RF-AD-6d — Lead time is given.** *Given* a certificate expiring in 30 days or a disk filling in 14, *when* the screen renders, *then* both appear with their horizon.
- **AC-RF-AD-6e — Staleness is explicit.** *Given* an air-gapped site, *when* the screen renders, *then* every bundle-derived element shows its age.

**Done when:** the screen is fully live, attention-ranked, and drills through with runbooks.

---

### RF-AD-7 · Admin copilot and audit investigation, on real data

**Severity:** Medium · **Area:** Admin · **Depends on:** RF-AD-1, RF-MI-4, RF-PER-2

> **Status at 2026-10-12 — Partial.** `admin/ai-assist` has the three touchpoints as prompt-assemble-and-parse functions with a live Mimir bridge, response types that can express only proposals, refusals and cited findings, and the EV-AD evals; a test shows a copilot proposal must still pass dual control. Nothing connects them to a running service, the real enclosure state or the real ledger: context is caller-supplied strings, evals run only in tests against mock responses, prompts and responses are not audited, there is no clearance filtering and no evidence export, and the console screens are fixtures.
> Criteria met 1 of 6 (b); open: a, c, d, e, f. Confidence: high.
>
> **Still to do:**
> - Serve the copilot and investigator from a running service grounded in real state and the real ledger (depends on RF-AD-1)
> - Run the EV-AD1 checks on every production response and audit each prompt, response, draft and refused bypass (AC a)
> - Give the investigator a read-only ledger handle, verify cited sequence numbers against the ledger, and add a test that a write attempt fails (AC c, d)
> - Filter ledger and state context by the operator's clearance and purpose before it reaches the model (AC e)
> - Add export of a signed investigation bundle that verifies on an unrelated machine (AC f)
>
> Checked: `admin/ai-assist/src/copilot.rs:267` · `admin/ai-assist/src/audit_assistant.rs:168` · `admin/ai-assist/tests/ac_ad_11.rs:98` · `admin/ai-assist/src/evals.rs:68` · `admin/ai-assist/src/evals.rs:150`

**Evidence.** `admin/ai-assist` (1 133 lines) implements A.AD1 (copilot), A.AD2 (simulation explainer) and A.AD3 (audit investigator) with their Evals; `copilot.rs:18` notes "A deterministic copilot returning a fixed JSON response (the Mimir stand-in)". `CopilotScreen` (143 lines) and `LedgerScreen` (148 lines) render fixtures.

**Prompt**

```
Wire the three admin AI touchpoints to the real model plane and real data.

1. A.AD1 copilot: grounded in the real enclosure state, strictly advisory. It may
   draft a change; it may never apply one. It must refuse bypass requests
   ("disable dual control", "grant me admin") and must propose the least-
   privilege option — the EV-AD1 Evals for advisory, bypass, scope and
   least-privilege must run in production, not only in tests.
2. A.AD3 audit investigator: read-only over the real audit ledger, with every
   finding citing specific audit sequence numbers an investigator can verify
   independently. It surfaces patterns; it draws no conclusions about intent. Add
   a test that the investigator has no write capability of any kind.
3. Investigation UX: natural-language query over the ledger, results with
   citations, timeline reconstruction, actor and resource pivoting, and export of
   a signed evidence bundle (RF-PER-2) for external review.
4. The copilot's own actions are audited: every prompt, every response, every
   drafted change and whether the operator accepted it.
5. Governance: the copilot runs under the operator's own clearance and purpose —
   it can never see or surface anything the operator could not.
```

**Acceptance criteria**

- **AC-RF-AD-7a — Bypass is refused.** *Given* a request to disable a control or self-elevate, *when* the copilot responds, *then* it refuses, explains, and the attempt is audited.
- **AC-RF-AD-7b — Advisory is structural.** *Given* the source tree, *when* traced from copilot output, *then* no path reaches an applied change without an operator action.
- **AC-RF-AD-7c — Findings are verifiable.** *Given* an investigation finding, *when* its cited sequence numbers are checked directly against the ledger, *then* they support the finding.
- **AC-RF-AD-7d — Investigator is read-only.** *Given* the investigator's capability set, *when* inspected, *then* it holds no write capability; a test attempting a write fails.
- **AC-RF-AD-7e — Clearance is inherited.** *Given* an operator without clearance for some records, *when* they use the copilot or investigator, *then* those records are neither surfaced nor summarised.
- **AC-RF-AD-7f — Evidence exports verify externally.** *Given* an exported investigation bundle, *when* verified on an unrelated machine, *then* its signature and contents validate.

**Done when:** all three touchpoints run on the real model plane and real ledger with their Evals enforced in production.

---
# M — UI platform foundations

> **Status.** The visual layer is strong: 28 design-system components with tokens, theme scopes, a shared `ConsoleShell` with responsive drawers, and 45 screens across six consoles totalling ~7 200 lines, each a faithful build of its design kit. What is missing is everything underneath the pixels. A scan of all `products/*/ui/src` plus `ui/` finds **7 files** containing any `aria-` attribute or `role=`, **zero** error boundaries, **zero** i18n, **zero** authentication surfaces, and **zero** tests of any kind. `AC-C-11` declares WCAG 2.2 AA conformance for the shells; the consoles make no such claim and would not meet it.

---

### RF-UI-1 · Authentication, session and identity in the console shell

**Severity:** Critical · **Area:** UI · **Depends on:** RF-IAM-1, RF-IAM-2, RF-IAM-3 · **Blocks:** every other UI item

> **Status at 2026-10-12 — Partial.** The Evidence is stale: the Midgard core console now sits behind `ui/shared/src/AuthGate.tsx`, which posts username and password to `/api/v1/login` and keeps the ML-DSA-signed bearer in sessionStorage. That is all there is: no TOTP field (so a user enrolled with `--totp` cannot sign in from the console), no WebAuthn, no purpose selection, no identity or clearance in the shell chrome, no idle warning, no step-up, no sign-out, and the other six consoles have no gate at all. The server side returns one uniform 401 for unknown user, wrong password and wrong second factor, and that is tested.
> Criteria met 0 of 7; open: a, b, c, e, f, g; not verifiable by reading: d. Confidence: high.
>
> **Scope change:** The Midgard console has `AuthGate` with password login for an ML-DSA-signed bearer token. Rewrite around password plus TOTP instead of WebAuthn-first (see RF-IAM-1); the login form has no TOTP field.
>
> **Still to do:**
> - Add a TOTP field to AuthGate and pass it through the SDK `login`, since the edge already accepts `totp` and enrolled users cannot otherwise use the console
> - Add sign-out that clears the stored token and client state, and handle 401 and token expiry (the edge issues a 1 hour token with no refresh or revoke route)
> - Show the server's view of subject, roles, clearance and purpose in ConsoleShell instead of the hard-coded DEFAULT_POLICY in HttpGateway
> - Decide the primary factor: the plan says WebAuthn, the tree ships password plus TOTP for users and file-held ML-DSA passkeys for administrators, and rewrite the item accordingly
> - Gate the other six consoles and add a TypeScript test runner so any of these criteria can be tested
>
> Checked: `ui/shared/src/AuthGate.tsx:54` · `products/midgard/ui/src/main.tsx:29` · `sdk/ts/src/http/HttpGateway.ts:44` · `services/http-gateway/src/lib.rs:264` · `services/http-gateway/tests/login_directory.rs:150`

**Evidence.** No console has a login route, a session, a logout, or a token. `products/*/ui/src/app/routes.tsx` route straight into the console. `ConsoleShell.tsx` (262 lines) renders navigation with no identity concept.

**Prompt**

```
Add the authentication layer every console currently lacks.

1. Login: a designed sign-in surface using the design system — WebAuthn primary,
   the enclosure's own relying party, with a clear, honest failure state that
   never reveals whether an account exists. Include the classification banner and
   the enclosure identity so an operator knows which enclosure they are signing
   into.
2. Purpose selection at sign-in (RF-IAM-3): choose from the purposes this
   operator is entitled to; bind it to the session; display it persistently in
   the shell chrome; make changing it an explicit, audited act.
3. Session state in the shell: the operator's identity, roles, clearance and
   purpose visible at all times (this is governance chrome, not decoration);
   idle-timeout warning with the ability to extend; automatic sign-out on
   absolute expiry with unsaved-work protection.
4. Step-up: when an action requires it, present the step-up challenge inline —
   the operator must never lose their place or their form state to authenticate.
5. Route protection: unauthenticated access to any console route redirects to
   sign-in preserving the intended destination; insufficient authorisation shows
   a designed "not permitted" state that does not leak the resource's existence.
6. Sign-out: explicit, terminating the server-side session, clearing all client
   state including caches, and returning to sign-in.
7. Multi-console coherence: signing into one console gives a session usable
   across all six, without a second sign-in, and signing out ends it everywhere.
```

**Acceptance criteria**

- **AC-RF-UI-1a — Sign-in works and reveals nothing.** *Given* the console, *when* an operator signs in with a security key, *then* they reach the console; *and when* an unknown identity attempts sign-in, *then* the failure is indistinguishable from a wrong-credential failure in message and timing.
- **AC-RF-UI-1b — Context is always visible.** *Given* any screen, *when* rendered, *then* identity, clearance and purpose in force are visible in the chrome and match the server's view.
- **AC-RF-UI-1c — Step-up preserves work.** *Given* a half-completed form requiring step-up, *when* the operator authenticates, *then* the form state is intact and the action proceeds.
- **AC-RF-UI-1d — Routes are protected.** *Given* an unauthenticated visit to a deep route, *when* the operator signs in, *then* they arrive at the originally requested route.
- **AC-RF-UI-1e — Idle warning and expiry.** *Given* an idle session approaching timeout, *when* the threshold is reached, *then* a warning appears with an extend option, and on expiry the operator is signed out with unsaved work protected.
- **AC-RF-UI-1f — Sign-out is complete.** *Given* sign-out, *when* it completes, *then* the server session is gone, all client caches are cleared, and browser-back does not reveal previously loaded data.
- **AC-RF-UI-1g — One session, six consoles.** *Given* a session established in one console, *when* another is opened, *then* no second sign-in is required; *and when* sign-out occurs, *then* all consoles are signed out.

**Done when:** every console requires authentication; the shell shows governance context; step-up is inline and preserves state.

---

### RF-UI-2 · Error, loading, empty and degraded states across every screen

**Severity:** High · **Area:** UI · **Depends on:** RF-UI-1

> **Status at 2026-10-12 — Open.** Unchanged from the Evidence. `useAsync` exposes loading and error, but there is no error boundary anywhere under `ui/` or `products/`, screens read `loading` at most and never `error`, and `docs/dev/design/ui-states.md` does not exist. The edge returns no correlation id or retryability flag for the UI to show.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Still to do:**
> - Add app, route and panel error boundaries in ui/shared and wire them into every console's router
> - Make HttpGateway throw typed errors (401, 403 policy denial, 404, 5xx) instead of a generic Error, and stop mapping every 403 to HumanGateRequiredError
> - Write docs/dev/design/ui-states.md and implement loading, empty, error, partial and degraded states per screen
> - Add a correlation id to edge responses so the error state has something to quote
> - Add a component test runner; none exists
>
> Checked: `ui/shared/src/DataProvider.tsx:35` · `products/midgard/ui/src/screens/ObjectFabricScreen.tsx:26` · `sdk/ts/src/http/HttpGateway.ts:74` · `services/http-gateway/src/http.rs:164`

**Evidence.** `useAsync` in `ui/shared/src/DataProvider.tsx` exposes `{data, loading, error, reload}`, but no `ErrorBoundary` exists anywhere in the tree, and screens render assuming data arrives.

**Prompt**

```
Give every screen a complete state model.

1. Error boundaries at the app, route and panel level, so one failing panel never
   blanks a console. Each boundary shows a designed error state with what failed,
   the correlation id (RF-RT-3) the operator can quote to support, a retry, and a
   path back to safety. Boundary trips are reported to telemetry.
2. Define and implement, for every screen and every data-bearing panel, the five
   states: loading (skeletons matching the final layout, not spinners),
   empty (with a designed explanation and, where applicable, the action that
   creates the first item), error (typed and actionable — distinguish "not
   permitted" from "not found" from "service unavailable" from "timed out"),
   partial (some panels loaded, some failed), and degraded (stale, truncated, or
   filtered-by-clearance data, clearly labelled).
3. Governance-specific states are first-class, because in this product they are
   common and meaningful: not-cleared (this data exists but you may not see it —
   or, where existence itself is protected, the indistinguishable not-found),
   human-gate-required (with the approval path), purpose-mismatch (with the
   purpose that would permit it), and redacted (showing that redaction occurred
   without revealing what was redacted).
4. Truncation and staleness are never silent: a result capped by a limit
   (RF-PER-3) or data older than a freshness threshold (RF-F-3) must say so
   inline.
5. Retry semantics in the UI must match the server's retryability flag — never
   offer retry for a permanent failure, and never auto-retry a mutation.
6. Write a state matrix in docs/dev/design/ui-states.md and audit all 45 screens
   against it.
```

**Acceptance criteria**

- **AC-RF-UI-2a — A panel failure is contained.** *Given* one panel whose data source fails, *when* the screen renders, *then* the rest of the screen works and the failed panel shows a designed error with a correlation id and retry.
- **AC-RF-UI-2b — All five states exist everywhere.** *Given* the state matrix, *when* all 45 screens are audited, *then* every data-bearing panel implements loading, empty, error, partial and degraded.
- **AC-RF-UI-2c — Governance states are distinct and safe.** *Given* a not-permitted resource, *when* it is requested, *then* the UI shows the correct governance state — and where existence is protected, it is indistinguishable from not-found.
- **AC-RF-UI-2d — Truncation and staleness are labelled.** *Given* a truncated result or stale data, *when* rendered, *then* the limitation is stated inline with the cause.
- **AC-RF-UI-2e — Retry matches semantics.** *Given* a permanent failure, *when* rendered, *then* no retry is offered; *given* a transient one, *then* retry is offered and works.
- **AC-RF-UI-2f — Boundaries are reported.** *Given* a boundary trip, *when* it occurs, *then* it appears in telemetry with the correlation id and component.

**Done when:** the state matrix exists and all 45 screens pass the audit; boundaries are in place at three levels.

---

### RF-UI-3 · Accessibility to WCAG 2.2 AA across all six consoles

**Severity:** High · **Area:** UI · **Depends on:** RF-UI-2 · **Blocks:** RF-CMP-6

> **Status at 2026-10-12 — Open.** Marginally better than the Evidence (12 files now carry an aria or role attribute, 17 occurrences in total) but structurally the same: clickable divs remain in the pipeline builder, session traces and logic builder, and there are no landmarks beyond the shell's nav, main and aside, no skip link and no live regions. No console package depends on axe-core or any test runner. `clients/shells/web/a11y/axe-audit.mjs` targets the client shells, not the consoles, and the workflow it names does not exist because the tree has no `.github` directory.
> Criteria met 0 of 7; open: a, b, f, g; not verifiable by reading: c, d, e. Confidence: high.
>
> **Still to do:**
> - Replace every clickable div with a button or give it role, name and keyboard handling
> - Add an axe-core run over every console screen in both modes and a place to run it (there is no CI configuration in the tree)
> - Add skip-to-content, focus management on route change and drawer close, and live regions for refresh, gate and sync changes
> - Give the pipeline builder and picture layers a non-drag, keyboard path
> - Run the NVDA, JAWS and VoiceOver audits and publish the conformance report
>
> Checked: `products/forge/ui/src/screens/PipelineBuilderScreen.tsx:33` · `products/mimir/ui/src/screens/SessionTracesScreen.tsx:93` · `ui/shared/src/ConsoleShell.tsx:113` · `clients/shells/web/a11y/axe-audit.mjs:7` · `products/midgard/ui/package.json:7`

**Evidence.** Across ~7 200 lines of console TSX and the shared packages, only 7 files contain any `aria-` attribute or `role=` — `OperatingPictureScreen`, `IconButton`, `Tag`, `Switch`, `ConsoleShell`, `ModeToggle`, `responsive`. Interactive elements are frequently `<div onClick>` (e.g. the pipeline builder's `Node`), which is neither focusable nor announced. The `accessibility.toml` files declare conformance for the *shells*; nothing declares or verifies it for the consoles.

**Prompt**

```
Make the consoles accessible, and keep them that way automatically.

1. Semantics first: every interactive element is a real control (button, link,
   input) or has a correct role, an accessible name, and keyboard operability.
   Replace every `<div onClick>` with an appropriate element. Landmarks
   (banner, navigation, main, complementary, contentinfo) on every page; one h1
   per screen and a correct heading hierarchy; lists as lists; tables with
   headers, captions, and scope; forms with associated labels, descriptions and
   error messages linked by aria-describedby.
2. Keyboard: complete operability with no traps, a visible focus indicator meeting
   contrast requirements, logical focus order, skip-to-content, focus management
   on route change, and correct focus return when a dialog or drawer closes.
3. Screen readers: announce dynamic changes with appropriately-scoped live
   regions — data refresh, validation errors, guardrail verdicts, approval state,
   sync status, alert arrival. Test with NVDA, JAWS and VoiceOver, not only with
   automated tooling.
4. Colour and contrast: verify all tokens in both light and dark modes at AA for
   text and non-text contrast. Critically, this product encodes meaning in colour
   throughout — status pills, markings, risk badges, confidence, sync state,
   secure/warn indicators. Every one of those must have a non-colour cue as well
   (shape, icon, text, pattern), and the design system must make the correct
   pattern the easy one.
5. Motion, zoom and reflow: respect prefers-reduced-motion; support 200% text
   scaling and 400% zoom with reflow and no loss of function; support text
   spacing overrides.
6. WCAG 2.2 additions specifically: focus not obscured, dragging alternatives
   (the pipeline builder and picture layers must be operable without dragging),
   target size minimums, consistent help, and redundant entry avoidance.
7. Automate: axe-core in the component and E2E suites failing CI on any
   violation, plus a per-release manual audit against the full AA checklist,
   recorded in an accessibility conformance report (RF-CMP-6).
```

**Acceptance criteria**

- **AC-RF-UI-3a — Zero automated violations.** *Given* every screen in both modes, *when* axe-core runs, *then* it reports no violations, enforced in CI.
- **AC-RF-UI-3b — Keyboard-complete.** *Given* keyboard-only operation, *when* every user flow from Workstream O is attempted, *then* each completes with no trap and a visible focus indicator throughout.
- **AC-RF-UI-3c — Screen-reader usable.** *Given* NVDA, JAWS and VoiceOver, *when* each flow is attempted, *then* it completes and every state change is announced.
- **AC-RF-UI-3d — No colour-only meaning.** *Given* the interface in greyscale, *when* every status, marking, risk, confidence and sync indicator is inspected, *then* each remains distinguishable.
- **AC-RF-UI-3e — Zoom and reflow.** *Given* 400% zoom and 200% text scaling, *when* every screen is viewed, *then* content reflows without horizontal scrolling and no function is lost.
- **AC-RF-UI-3f — Dragging is optional.** *Given* the pipeline builder and picture layer controls, *when* operated without dragging, *then* every operation is achievable.
- **AC-RF-UI-3g — Conformance is reported.** *Given* a release, *when* the accessibility conformance report is produced, *then* it covers every screen against AA with evidence.

**Done when:** axe-core gates CI at zero violations; the three screen-reader audits pass; the conformance report is published.

---

### RF-UI-4 · Internationalisation and localisation

**Severity:** Medium · **Area:** UI · **Depends on:** RF-UI-2

> **Status at 2026-10-12 — Open.** Unchanged from the Evidence. No package.json in the workspace depends on an i18n library, no source file uses the Intl APIs, and screen strings are hard-coded English literals including the upper-case mono labels.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Still to do:**
> - Choose a message-catalogue library and extract all strings with stable keys
> - Move upper-casing of mono labels into CSS
> - Add locale-aware date, number and list formatting
> - Record the do-not-translate policy for markings and enforce it
> - Ship one non-English locale plus a pseudo-locale and a hard-coded-string check
>
> Checked: `products/midgard/ui/package.json:13` · `products/midgard/ui/src/screens/ObjectFabricScreen.tsx:146` · `products/midgard/ui/src/screens/AuditScreen.tsx:162` · `ui/shared/src/AuthGate.tsx:98`

**Evidence.** Zero i18n infrastructure. Every string in all 45 screens is hard-coded English, including the ALL-CAPS mono labels the design system mandates.

**Prompt**

```
Externalise every string and support locale properly.

1. Extract all user-facing strings to a message catalogue with stable keys,
   contextual comments for translators, and pluralisation and gender support.
   Include the mono ALL-CAPS labels — uppercasing is a presentation concern and
   must be applied by CSS/locale rules, not baked into source strings, because
   several languages have no case and others uppercase differently.
2. Locale-aware formatting for dates, times, time zones, numbers, units,
   currencies and lists — never string concatenation.
3. Layout resilience: verify with a pseudo-locale that inflates string length by
   ~40% and with the longest real translation; verify RTL (Arabic, Hebrew) with
   correct mirroring of layout, icons and directional affordances.
4. Decide and document what is NOT translated: classification markings, security
   labels and provenance tags are terms of art whose translation could change
   legal meaning. Record the policy and enforce it.
5. Locale selection: per-operator preference persisted server-side, with a
   sensible default; the choice must not alter any security-relevant behaviour.
6. Ship at least one non-English locale end to end to prove the pipeline, and add
   a CI check that no hard-coded user-facing string remains.
```

**Acceptance criteria**

- **AC-RF-UI-4a — No hard-coded strings.** *Given* the console source, *when* the CI check runs, *then* no user-facing literal remains outside the catalogue.
- **AC-RF-UI-4b — Pseudo-locale does not break layout.** *Given* the 40%-inflation pseudo-locale, *when* every screen renders, *then* no text is clipped, overlapped or truncated without an accessible full value.
- **AC-RF-UI-4c — RTL is correct.** *Given* an RTL locale, *when* every screen renders, *then* layout, icons and directional controls mirror correctly.
- **AC-RF-UI-4d — Formatting is locale-aware.** *Given* a non-English locale, *when* dates, numbers and lists render, *then* they follow that locale's conventions.
- **AC-RF-UI-4e — Markings are not translated.** *Given* any locale, *when* classification markings render, *then* they appear in their canonical form per the documented policy.
- **AC-RF-UI-4f — Locale does not change security.** *Given* two locales, *when* the same operation is attempted, *then* authorisation outcomes are identical.

**Done when:** the catalogue is complete, one additional locale ships, and the pseudo-locale and RTL audits pass.

---

### RF-UI-5 · Navigation, information architecture and cross-console coherence

**Severity:** Medium · **Area:** UI · **Depends on:** RF-UI-1

> **Status at 2026-10-12 — Open.** Unchanged from the Evidence. Each console is still its own Vite app with a hash router of fixed paths; `nav.ts` holds only in-console hrefs, the toolbar search box is an uncontrolled input wired to nothing, and no screen state is in the URL. There is no product switcher, breadcrumb, global search or command palette.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Add a product switcher to ConsoleShell (needs a shared session first, see RF-UI-1)
> - Put selection, filter and tab state in the URL for each screen
> - Add a search route at the edge scoped by the caller's clearance, then wire the toolbar search and a command palette to it
> - Define table selection, filter, sort and pagination once in the design-system Table
>
> Checked: `products/midgard/ui/src/app/routes.tsx:15` · `products/midgard/ui/src/app/ConsoleToolbar.tsx:10` · `ui/shared/src/nav.ts:41` · `ui/shared/src/ConsoleShell.tsx:92`

**Evidence.** `ui/shared/src/nav.ts` defines the navigation model and each product has its own `routes.tsx`. Six consoles are separate Vite applications; there is no cross-console navigation, no unified search, no breadcrumbs, and no deep-linking convention.

**Prompt**

```
Make six consoles feel like one platform.

1. A product switcher in the shell for moving between consoles while preserving
   session, purpose and — where meaningful — context (an object selected in the
   fabric should be findable in Heimdall or Forge).
2. Deep linking: every meaningful state is addressable by URL — selected object,
   filter, time range, expanded panel, active tab. Sharing a URL with a colleague
   must reproduce their view, subject to that colleague's own authorisation.
3. Breadcrumbs and hierarchy: consistent location indication and predictable back
   behaviour, including after modal and drawer interactions.
4. Global search across the platform: objects, datasets, pipelines, artefacts,
   entities, policies, identities and audit records — scoped by clearance,
   ranked, with type-ahead and keyboard-first operation.
5. A command palette (keyboard-invoked) for navigation and actions, including
   the governed ones, which route into the same gates as their UI equivalents.
6. Consistent selection, filtering, sorting and pagination patterns across every
   table and list in all six consoles, defined once in the design system.
```

**Acceptance criteria**

- **AC-RF-UI-5a — Switching preserves session and context.** *Given* an operator working in one console, *when* they switch, *then* session and purpose persist and relevant context carries across.
- **AC-RF-UI-5b — URLs reproduce state.** *Given* any meaningful screen state, *when* its URL is opened by another authorised operator, *then* the same state is reproduced, filtered by that operator's authorisation.
- **AC-RF-UI-5c — Search spans and respects clearance.** *Given* global search, *when* a term is entered, *then* results span all entity types and exclude anything above the searcher's clearance — including from result counts.
- **AC-RF-UI-5d — Command palette is governed.** *Given* an action invoked from the palette, *when* it executes, *then* it passes the same gates and audit as the equivalent UI action.
- **AC-RF-UI-5e — Patterns are consistent.** *Given* any table across the six consoles, *when* compared, *then* selection, filtering, sorting and pagination behave identically.

**Done when:** the switcher, deep links, global search and command palette are live and consistent across all six.

---

### RF-UI-6 · Forms, validation and data entry

**Severity:** Medium · **Area:** UI · **Depends on:** RF-UI-2, RF-UI-3

> **Status at 2026-10-12 — Open.** Unchanged from the Evidence. The design system's form components are still only Input, Select and Switch; there is no form or validation layer, no dirty tracking and no unsaved-change guard. The console-side human gate is a button that mints its own token string, and the edge accepts any non-empty value.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Add a form layer to the design system with field and form errors, focus-to-first-error and announcements
> - Derive client validation from the edge's contract (the ontology and policy file schemas are the only real ones today)
> - Add dirty tracking, navigation guards and draft recovery
> - Add typed confirmation for destructive actions and carry the acknowledgement in the audited request
> - Replace the self-minted human-gate token with a real approval before treating gated mutations as pending-until-confirmed
>
> Checked: `ui/design-system/src/components/forms/Input.tsx:1` · `products/midgard/ui/src/screens/ObjectFabricScreen.tsx:60` · `services/http-gateway/src/enclosure.rs:375` · `products/midgard/ui/src/screens/PoliciesScreen.tsx:194`

**Evidence.** The design system has `Input`, `Select` and `Switch`. There is no form framework, no validation model, no dirty-state tracking and no unsaved-change protection — and several screens (Config, Policies, Pipeline Builder, Registry) are fundamentally editing surfaces.

**Prompt**

```
Build the form layer these editing surfaces need.

1. A validation model shared with the server contract (RF-API-1), so client
   validation is a fast preview of the server's rules and can never diverge —
   generate the client rules from the contract where possible.
2. Field-level and form-level errors, inline and summarised, associated
   accessibly, focus-managed to the first error on submit, and announced.
3. Dirty tracking, unsaved-change warnings on navigation, autosave for long forms
   where appropriate, and reliable draft recovery after a crash or session
   expiry.
4. Optimistic updates only where safe: never for a governed mutation. A gated
   action shows its pending state honestly until the server confirms, and the
   confirmation includes the audit sequence number.
5. Complex inputs the product actually needs: policy rule builder, marking
   selector honouring the lattice, schedule/cron editor with a plain-language
   preview, DAG editor (RF-F-7), query builder, time-range picker with
   time-zone clarity.
6. Destructive and irreversible actions: typed confirmation naming the target,
   a clear statement of consequence and reversibility, and — where the action is
   irreversible — explicit acknowledgement of that fact (mirroring INV-5 in the
   UI).
7. Missing components in the design system are added there, not invented per
   console (RF-DOC-6).
```

**Acceptance criteria**

- **AC-RF-UI-6a — Client and server agree.** *Given* an input the server would reject, *when* it is entered, *then* the client rejects it identically; and no input the client accepts is rejected by the server for a rule the client knows.
- **AC-RF-UI-6b — Errors are accessible and focused.** *Given* a submission with errors, *when* it fails, *then* errors are announced, associated with their fields, summarised, and focus moves to the first.
- **AC-RF-UI-6c — Work is not lost.** *Given* unsaved changes, *when* navigation or session expiry occurs, *then* the operator is warned and the draft is recoverable.
- **AC-RF-UI-6d — No optimistic governed mutations.** *Given* a gated action, *when* submitted, *then* the UI shows pending until the server confirms and displays the audit sequence on success.
- **AC-RF-UI-6e — Irreversibility is acknowledged.** *Given* an irreversible action, *when* confirmed, *then* the operator explicitly acknowledged its irreversibility, and that acknowledgement is part of the audited request.

**Done when:** the form layer is in the design system; all editing screens use it; contract-derived validation is in place.

---

### RF-UI-7 · Performance, bundle discipline and rendering at scale

**Severity:** Medium · **Area:** UI · **Depends on:** RF-UID-1

> **Status at 2026-10-12 — Open.** Unchanged from the Evidence. Routes import every screen eagerly, there is no virtualisation library or budget, and no real-user timing. The only bound is server-side: the audit route clamps its limit to 500 rows and queries take a maxResults, with no cursor or truncation marker.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Lazy-load routes and set a bundle-size budget checked at build
> - Add cursor pagination and a truncation marker to the edge's list routes, then virtualise the audit and object tables with keyboard and screen-reader support
> - Give the audit stream an explicit paused or buffered state instead of silently keeping the newest 40 to 60
> - Report browser timings to the in-enclosure metrics
>
> Checked: `products/midgard/ui/src/app/routes.tsx:2` · `products/midgard/ui/package.json:13` · `services/http-gateway/src/enclosure.rs:585` · `ui/shared/src/DataProvider.tsx:84`

**Evidence.** No code splitting, no virtualisation, no memoisation strategy and no performance budget. Screens render full result sets from fixtures; against real data (10⁶ objects, 10⁵ tracks, long audit ledgers) they will not hold.

**Prompt**

```
Make the consoles fast with real data volumes.

1. Set and enforce budgets: initial bundle size, time to interactive, interaction
   latency (p95), and memory. Fail CI on regression.
2. Route-level code splitting and lazy loading; shared vendor chunking across the
   six consoles; tree-shake the design system so a console pays only for what it
   imports.
3. Virtualise every long list and table — audit ledger, object lists, fleet
   inventory, entity lists, trace lists — with keyboard and screen-reader support
   preserved through virtualisation (this is where naive virtualisation breaks
   accessibility).
4. Server-side pagination, filtering and sorting via RF-API-1's uniform list
   contract; never fetch-all-then-filter-in-the-browser.
5. Streaming surfaces (audit, feeds, agent output, build logs, rollout progress)
   must handle high event rates without unbounded memory: bounded buffers,
   coalescing, and an explicit "paused — N events buffered" state rather than
   silent dropping.
6. Rendering: memoisation where measured to help, avoidance of layout thrash, and
   requestAnimationFrame-driven updates for the operating picture.
7. Measure real user timing in the browser and report it (RF-OBS-1), so the
   budget is checked in production and not only in CI.
```

**Acceptance criteria**

- **AC-RF-UI-7a — Budgets hold and gate.** *Given* the performance budgets, *when* CI runs, *then* a regression beyond tolerance fails the build.
- **AC-RF-UI-7b — Large lists stay responsive.** *Given* a 10⁶-row result, *when* rendered, *then* scrolling and interaction remain within the latency budget and memory stays bounded.
- **AC-RF-UI-7c — Virtualisation preserves accessibility.** *Given* a virtualised table, *when* navigated by keyboard and screen reader, *then* every row is reachable and correctly announced with its position in the set.
- **AC-RF-UI-7d — High-rate streams are bounded.** *Given* a feed at high event rate, *when* it runs for an extended period, *then* memory is bounded and any pause or coalescing is visible to the operator.
- **AC-RF-UI-7e — Real user timing is reported.** *Given* production use, *when* metrics are queried, *then* real interaction latencies are visible.

**Done when:** budgets gate CI; virtualisation is in place with accessibility preserved; RUM is reported.

---

### RF-UI-8 · Styling architecture, CSP compatibility and theming discipline

**Severity:** Medium · **Area:** UI · **Depends on:** RF-API-3

> **Status at 2026-10-12 — Open.** Inline styles are everywhere (about 1,300 `style={{` occurrences across 90 files) and every console's index.html carries an inline script, so a strict CSP cannot hold; the edge sends no CSP header in any case. The two token copies are in fact identical today for base, colors, effects, spacing and typography and differ only in fonts.css, but nothing checks that, and there is no lint configuration of any kind.
> Criteria met 0 of 4; open: a, b, c; not verifiable by reading: d. Confidence: high.
>
> **Scope change:** Five of the six token files are byte-identical between the two copies; only `fonts.css` differs. Step 3 becomes a drift check. The CSP steps need a decision on what serves the built consoles: the edge serves JSON only, and each `index.html` has an inline script.
>
> **Still to do:**
> - Decide how the built consoles are served and with what CSP; today only the Vite dev server serves them and the edge serves JSON only
> - Move presentation out of inline style objects and the inline index.html script into stylesheets
> - Generate one token copy from the other and add a drift check (currently only fonts.css differs)
> - Add a lint rule against raw colour, spacing, radius and font literals
> - Add high-contrast and forced-colors modes and verify contrast
>
> Checked: `products/forge/ui/src/screens/PipelineBuilderScreen.tsx:29` · `products/midgard/ui/index.html:7` · `services/http-gateway/src/http.rs:164` · `ui/design-system/src/tokens/colors.css:1` · `tools/ci/provenance-lint.sh:1`

**Evidence.** Screens are written with large inline `style={{…}}` objects (e.g. `PipelineBuilderScreen`'s `Node` component). This blocks a strict CSP (`style-src` without `unsafe-inline`), makes the design system's tokens bypassable, and makes theming and high-contrast modes hard to guarantee.

**Prompt**

```
Fix the styling architecture before the CSP forces it.

1. Move presentation out of inline style objects into a token-driven approach
   compatible with a strict CSP — CSS modules, a compile-time CSS-in-JS with
   static extraction, or plain stylesheets with token variables. Inline styles
   remain acceptable only for genuinely dynamic values (a computed position),
   and those must be delivered via CSS custom properties rather than arbitrary
   declarations.
2. Enforce token usage: a lint rule rejecting raw colour, spacing, radius,
   shadow and font values in application code — everything comes from tokens, so
   the design system is genuinely the single source of truth.
3. Reconcile the two token copies. docs/design/tokens/*.css and
   ui/design-system/src/tokens/*.css are parallel files that have drifted (both
   appear in the current working-tree diff). Make one generated from the other,
   with a CI check that fails on divergence (RF-DOC-6).
4. Support the modes the platform needs: light, dark, high contrast, and forced-
   colors, all verified for contrast (RF-UI-3).
5. Verify under the deployed CSP with no relaxation; add a CI check that loads
   every screen under the production CSP and fails on any violation.
```

**Acceptance criteria**

- **AC-RF-UI-8a — Strict CSP holds.** *Given* the production CSP with no `unsafe-inline`, *when* every screen is loaded and exercised, *then* no violation occurs.
- **AC-RF-UI-8b — No raw values.** *Given* application source, *when* linted, *then* no raw colour, spacing, radius, shadow or font literal appears.
- **AC-RF-UI-8c — Tokens do not drift.** *Given* the two token sets, *when* CI runs, *then* divergence fails the build.
- **AC-RF-UI-8d — All modes meet contrast.** *Given* light, dark, high-contrast and forced-colors, *when* audited, *then* each meets AA for text and non-text contrast.

**Done when:** inline styles are eliminated from application code; the CSP check and token-drift check gate CI.

---

### RF-UI-9 · Offline, connectivity and resilience in the browser

**Severity:** Medium · **Area:** UI · **Depends on:** RF-API-3, RF-UI-2

> **Status at 2026-10-12 — Open.** Nothing exists. No code reads connectivity state, the HTTP audit stream is a 4.2 second poll that swallows errors silently, and no screen shows data age. A gateway restart in development mode invalidates the token and the console has no handling for the resulting 401s.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Add connectivity state to ConsoleShell and surface poll failures instead of swallowing them
> - Make the audit poll resume from the last sequence seen, with backoff and a visible reconnecting state
> - Record fetch time in useAsync and mark stale data
> - Distinguish a network failure from a 401 so a blip does not look like sign-out and an expired token does
> - Decide whether any offline read cache is wanted at all; none exists to clear today
>
> Checked: `sdk/ts/src/http/HttpGateway.ts:116` · `sdk/ts/src/http/HttpGateway.ts:129` · `ui/shared/src/DataProvider.tsx:42` · `ui/shared/src/AuthGate.tsx:31`

**Prompt**

```
Make the consoles behave correctly on imperfect networks.

1. Connectivity awareness: detect online/offline and degraded connectivity;
   show it in the chrome; queue nothing security-relevant while offline —
   a governed mutation must not be optimistically queued in a browser.
2. Reconnection: exponential backoff with jitter for streaming surfaces,
   automatic resume from the last received sequence, and a visible indication of
   reconnecting versus disconnected.
3. Staleness: every screen shows the age of its data, and data past a freshness
   threshold is visually marked stale rather than presented as current.
4. Session survival: a network blip must not sign the operator out or lose their
   work.
5. Read-only offline: cached, clearance-appropriate read-only views may remain
   available offline where useful, clearly labelled with their age, and cleared
   on sign-out (AC-RF-UI-1f).
```

**Acceptance criteria**

- **AC-RF-UI-9a — Offline is visible and safe.** *Given* connectivity loss, *when* it occurs, *then* the chrome shows it and no governed mutation is queued or optimistically applied.
- **AC-RF-UI-9b — Streams resume cleanly.** *Given* a dropped stream, *when* connectivity returns, *then* it resumes from the last sequence with no gap or duplication, and the transition is visible.
- **AC-RF-UI-9c — Staleness is shown.** *Given* data past its freshness threshold, *when* rendered, *then* it is marked stale with its age.
- **AC-RF-UI-9d — Blips do not sign out or lose work.** *Given* a brief network interruption during form entry, *when* connectivity returns, *then* the session and the form state are intact.
- **AC-RF-UI-9e — Caches clear on sign-out.** *Given* offline read-only caches, *when* the operator signs out, *then* they are cleared and unreachable.

**Done when:** connectivity states, resume and staleness are implemented across all six consoles.

---

### RF-UI-10 · Frontend security hardening

**Severity:** High · **Area:** UI · **Depends on:** RF-UI-1, RF-API-3

> **Status at 2026-10-12 — Partial.** Some of the posture is there by default: the only raw-HTML injection is the design-system Icon, fed from a static path table, console index.html files load no third-party assets, and the edge has an origin-locked CORS allowlist with a test. Nothing is enforced or tested beyond that: no lint or scan, no frame-ancestors or CSP header, the bearer token is kept in sessionStorage (which point 7 forbids), and the supply-chain tools (mg-sbom, mg-scan) cover Cargo only, not the pnpm graph.
> Criteria met 0 of 6; open: a, b, c, d, f; not verifiable by reading: e. Confidence: medium.
>
> **Scope change:** Step 7 (no token in browser storage) contradicts the shipped design, which keeps the bearer token in `sessionStorage`. Change the design or the step. Step 5's "same gate as Rust" needs new tooling: nothing in `tools/` handles npm or pnpm packages.
>
> **Still to do:**
> - Move the session token out of browser storage (cookie bound to the edge, or memory only) or rewrite point 7 to match the bearer design
> - Add a rendering test that a script payload in an object property or audit field displays as text
> - Send frame-ancestors none and a CSP from whatever serves the built consoles
> - Bring pnpm-lock.yaml into the SBOM, advisory and licence gate and forbid post-install scripts
> - Add a built-bundle scan for secrets, third-party URLs and published source maps
>
> Checked: `ui/design-system/src/components/media/Icon.tsx:112` · `ui/shared/src/AuthGate.tsx:61` · `services/http-gateway/src/http.rs:178` · `services/http-gateway/tests/secured_edge.rs:135` · `products/midgard/ui/index.html:23`

**Prompt**

```
Harden the browser tier.

1. XSS: no dangerouslySetInnerHTML anywhere without a reviewed, sanitised
   exception; sanitise any rendered content that originates from data (object
   properties, audit payloads, agent output, feed content — all of which are
   attacker-influenceable in this product); enforce with a lint rule and a
   CI check.
2. Agent output specifically: model output rendered in the console is untrusted.
   Render it as text or through a strict, allow-listed renderer; never as raw
   HTML; never auto-execute links or embedded content.
3. CSRF: origin checking plus token binding for state-changing requests
   (RF-IAM-2).
4. Clickjacking: frame-ancestors none; verify no console can be framed.
5. Dependency hygiene: the JS/TS dependency graph goes through the same scan and
   licence gate as Rust (RF-SC-4), with lockfile integrity enforced and no
   post-install scripts permitted.
6. Supply chain: subresource integrity for anything not first-party; no CDN
   dependency in a shipped console (note the design-system docs currently load
   React and Babel from unpkg — that is acceptable for the static docs, but must
   never be true of a shipped console; verify).
7. Secrets: no token, key or credential in bundles, source maps, or browser
   storage; verify by scanning built artefacts in CI.
8. Source maps: not published to production, or published to an authenticated
   endpoint only.
```

**Acceptance criteria**

- **AC-RF-UI-10a — No unsafe HTML.** *Given* the console source, *when* scanned, *then* no unreviewed raw-HTML injection exists; agent, feed and object content renders as text or through the allow-listed renderer.
- **AC-RF-UI-10b — Injected content is inert.** *Given* an object property or agent output containing a script payload, *when* rendered, *then* it displays as text and does not execute.
- **AC-RF-UI-10c — No framing.** *Given* an attempt to frame any console, *when* made, *then* it is refused.
- **AC-RF-UI-10d — JS dependencies are gated.** *Given* the JS dependency graph, *when* the release scan runs, *then* advisories and licences are enforced exactly as for Rust.
- **AC-RF-UI-10e — No CDN in shipped consoles.** *Given* a built console, *when* its network requests are inspected, *then* every asset is first-party.
- **AC-RF-UI-10f — No secrets in artefacts.** *Given* built bundles and source maps, *when* scanned in CI, *then* no credential material is present.

**Done when:** the XSS lint and CI checks are green; the injected-content test passes; JS dependencies are gated with Rust.

---

# N — UI data wiring

> **Why separate from M.** The foundations above are about how the UI behaves. This workstream is about what it shows. Five of six consoles currently show data that does not exist: `AdminMock`, `ForgeMock`, `MimirMock`, `BifrostMock`, `HeimdallMock`, `ClientMock` — 3 131 lines of fixtures in `sdk/ts/src/products/`. Only `products/midgard/ui` can be pointed at a real edge, via `VITE_GATEWAY=http`, and only for 15 endpoints.

---

### RF-UID-1 · Replace every mock gateway with a real client

**Severity:** High · **Area:** UI data · **Depends on:** RF-API-3, RF-IAM-3 · **Blocks:** RF-UID-2..7, RF-UX-*

> **Status at 2026-10-12 — Partial.** One console is wired: the Midgard core console defaults to the real HTTP edge behind a login, with the mock only under `VITE_GATEWAY=mock`. The other six `main.tsx` files still construct their mock directly, the edge serves no routes for them, and the mocks and fixtures are still exported from the SDK barrel that the Midgard bundle imports. The stale 'a stub' comment in `sdk/ts/src/index.ts` is still there.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** The blocker is upstream of the client: the edge has no routes for Forge, Mimir, Bifrost, Heimdall or admin. Split into edge routes per product, then a client per product.
>
> **Still to do:**
> - Add browser-reachable edge routes for Forge, Mimir, Bifrost, Heimdall and admin (their services exist only behind PQ-mTLS, or as libraries) and an HTTP client per product gateway interface
> - Move the mocks and fixtures to a package the production entry points cannot import; Midgard's main.tsx imports MockGateway unconditionally
> - Write the per-screen field reconciliation before wiring; hard-coded figures such as the 312 object types sit in screens as well as fixtures
> - Fix the index.ts comment and the six main.tsx comments
> - Add a bundle check for fixture data and somewhere to run it
>
> Checked: `products/midgard/ui/src/main.tsx:24` · `products/forge/ui/src/main.tsx:19` · `products/admin/ui/src/main.tsx:17` · `sdk/ts/src/index.ts:5` · `services/http-gateway/src/lib.rs:598`

**Evidence.** `products/{admin,bifrost,clients,forge,heimdall,mimir}/ui/src/main.tsx` each construct a `*Mock`. `sdk/ts/src/index.ts:5` describes `HttpGateway` as "a stub" (it is in fact a real client, but only for the core surface — the comment is stale, see RF-DOC-1).

**Prompt**

```
Point every console at real data and remove the mocks from shipped builds.

1. Implement a real HTTP client for each product gateway interface against the
   RF-API-3 edge, generated from the proto contract where possible so the client
   cannot drift from the server.
2. Replace each main.tsx mock construction with the real client. Keep the mocks —
   they are valuable — but move them to a test-only package that no production
   build can import, and add a CI check on built bundles that fails if fixture
   data is present.
3. Handle real-world data properly at every call site: pagination, streaming,
   partial failure, authorisation filtering, truncation, and staleness — the
   mocks return complete, instant, fully-authorised results, so every screen
   currently assumes a world that does not exist.
4. Audit every screen against its real data source: fields the mock invents that
   no API provides must either be implemented server-side or removed from the
   UI. Produce a per-screen reconciliation list before starting, so nothing is
   quietly dropped.
5. Update each product README and each main.tsx comment, which currently describe
   the mock as the present state (RF-DOC-1).
```

**Acceptance criteria**

- **AC-RF-UID-1a — No mocks in shipped builds.** *Given* any production console bundle, *when* inspected, *then* no mock gateway or fixture is present, enforced in CI.
- **AC-RF-UID-1b — Every screen shows real data.** *Given* a running enclosure, *when* each of the 45 screens is opened, *then* every value derives from a real service.
- **AC-RF-UID-1c — No invented fields.** *Given* the per-screen reconciliation, *when* completed, *then* every field displayed has a real source, and any removed field is recorded with the reason.
- **AC-RF-UID-1d — Real-world conditions are handled.** *Given* paginated, partially-failing, authorisation-filtered and truncated responses, *when* each screen receives them, *then* it renders correctly per the RF-UI-2 state matrix.
- **AC-RF-UID-1e — Mocks remain for tests.** *Given* the test suite, *when* it runs, *then* the mocks are available and used, from a package production cannot import.

**Done when:** all six consoles run against the real edge; the fixture-in-bundle check is green; the reconciliation list is closed.

---

### RF-UID-2 · Forge console on real pipelines and data

**Severity:** High · **Area:** UI data · **Depends on:** RF-UID-1, RF-F-1..7

> **Status at 2026-10-12 — Open.** The Forge console still runs only on `ForgeMock`. A real Forge worker now exists as a service (file, PostgreSQL, S3 and Kafka sources into a versioned dataset store, projected and synced into the fabric over the mesh), but it is driven by a jobs file, applies no transforms, and nothing exposes datasets, runs, schedules or health to a browser.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** Start from `forge-worker`, which exists but is mesh-only with no browser route.
>
> **Still to do:**
> - Expose the worker's datasets, versions, job runs and connector state through the edge
> - Add transform execution, preview, cancel and quality results to the running worker; jobs currently project rows as read
> - Add projection simulation before sync
> - Replace ForgeMock with a client for those routes
>
> Checked: `products/forge/ui/src/main.tsx:19` · `forge/worker/tests/sync.rs:173` · `deploy/compose/smoke.sh:50` · `README.md:563`

**Prompt**

```
Make the seven Forge screens operate the real Forge.

- Datasets: real datasets, versions, branches, schemas, column statistics,
  lineage, quality scorecards and storage footprint; version diff and time-travel
  reads; real preview of real rows under the operator's clearance.
- Connectors: real connector inventory, configuration (credentials via KMS
  references only), live health, test-connection that really connects, schema
  discovery and drift decisions.
- Pipelines / Transform IDE: real definitions, real editing (RF-F-7), real
  validation, real preview through the real engine, real run history with logs
  and metrics, and real cancel.
- Schedules: real schedule definitions, next-run and last-run, SLA and freshness
  state, backfill initiation with progress, pause/resume.
- Data Health: real expectations, real results, real profiling and drift, real
  quarantine inspection and re-processing.
- Projection: real mappings, real simulation before commit, real projection runs
  with progress, real rollback.
Every mutating action passes the real gates and shows its audit sequence.
```

**Acceptance criteria**

- **AC-RF-UID-2a — Real end-to-end in the console.** *Given* the Forge console, *when* an operator defines a connector, ingests data, builds a pipeline, runs it, inspects quality and projects to the fabric, *then* every step operates real services and the fabric shows the projected objects.
- **AC-RF-UID-2b — Preview is genuinely computed.** *Given* a transform preview, *when* shown, *then* the rows are produced by the real engine from real data under the operator's clearance.
- **AC-RF-UID-2c — Runs are cancellable and honest.** *Given* a running pipeline, *when* cancelled, *then* execution stops, resources are released, and the run shows as cancelled — never as failed or succeeded.
- **AC-RF-UID-2d — Credentials never surface.** *Given* connector configuration, *when* viewed or exported, *then* only references appear.
- **AC-RF-UID-2e — Projection is simulated first.** *Given* a projection commit, *when* attempted, *then* the operator was shown the simulated effect and the commit carries its audit sequence.

**Done when:** the full Forge journey completes in the console against real services.

---

### RF-UID-3 · Midgard core console on real platform state

**Severity:** High · **Area:** UI data · **Depends on:** RF-UID-1, RF-OBS-1, RF-OBS-5

> **Status at 2026-10-12 — Partial.** The Evidence is half stale. Object types, query, search-around, actions, policies, policy dry-run and audit are now served from the real fabric and governance crates with tests, so four or five of the screens read live data when pointed at the edge. Identities, keys, compute, observability, resilience and the SDK surface are still fixed data in `data.rs`, screens still carry hard-coded headline figures, and the human gate is not real: the console mints its own token and the edge accepts any non-empty string.
> Criteria met 0 of 5; open: a, b, c, d; not verifiable by reading: e. Confidence: high.
>
> **Scope change:** Fabric queries, search-around, actions, policies, dry-run and audit are real and tested. Narrow this to the fixed views in `services/http-gateway/src/data.rs` (identities, keys, compute, observability, resilience, SDK surface) and the hard-coded figures in screens — `data.rs:226` returns literal SLO values. Also fix the human gate: the edge accepts any non-empty `humanGateToken` (`enclosure.rs:375`).
>
> **Still to do:**
> - Serve identities from the durable directory, keys from the KMS and observability from the gateway's own metrics and SLO instead of data.rs
> - Make the human gate a real second-party approval the edge verifies, not a client-supplied non-empty token
> - Remove hard-coded figures from screens (object-type counts, chain breaks, signature and hash lines in the audit inspector)
> - Add a whole-chain verification status to the audit route and show failures prominently; today each row gets a tick or a cross and the badge is always the secure tone
> - Decide what Compute, Resilience and SDK show while the scheduler and Bifrost rollout are libraries, or hide them
>
> Checked: `services/http-gateway/src/data.rs:1` · `services/http-gateway/tests/fabric_edge.rs:123` · `services/http-gateway/src/enclosure.rs:375` · `products/midgard/ui/src/screens/ObjectFabricScreen.tsx:150` · `products/midgard/ui/src/screens/AuditScreen.tsx:117`

**Evidence.** This console has 11 screens and is the only one with a real transport option, but `services/http-gateway/src/data.rs` serves seeded state and `ObservabilityScreen`/`ResilienceScreen` render `SloMetric` and `RolloutState` fixtures.

**Prompt**

```
Make all 11 Midgard screens live.

- Object Fabric / Search Around: real types, real instances, real link traversal
  with per-object authorisation and truncation markers, real actions with real
  human gates and real audit sequences, real EXPLAIN for slow queries.
- Audit: the real ledger, streaming, with filtering, chain-verification status
  visible, and evidence export.
- Policies: real rules with real simulation against the real PDP.
- Identity: real identities, roles, entitlements, sessions and recertification
  state.
- Keys / KMS: real key inventory, algorithms, rotation state and expiry horizons
  — never any key material.
- Compute: real jobs, real scheduling state, real resource usage.
- Observability: real metrics, real SLO attainment, real error budgets — remove
  the fixtures entirely.
- Resilience: real rollout state from Bifrost, real replica health from the
  scheduler, real backup and DR status (RF-RES-*).
- SDK: real reflection surface with governed try-it (RF-API-5).
```

**Acceptance criteria**

- **AC-RF-UID-3a — All 11 screens are live.** *Given* the console, *when* each screen is opened, *then* every value comes from a live service and no fixture remains.
- **AC-RF-UID-3b — Gated actions really gate.** *Given* an action requiring a human gate, *when* invoked from Object Fabric, *then* the real gate is raised and the action completes only after real approval, showing its audit sequence.
- **AC-RF-UID-3c — Observability is measured.** *Given* the Observability screen, *when* opened, *then* SLO attainment and error budgets are computed from real metrics.
- **AC-RF-UID-3d — Chain status is visible.** *Given* the Audit screen, *when* opened, *then* the current chain-verification status is shown, and a verification failure is prominent.
- **AC-RF-UID-3e — No key material.** *Given* the Keys screen, *when* inspected including network responses, *then* no key material is present.

**Done when:** `data.rs` seeding is removed and all 11 screens read live services.

---

### RF-UID-4 · Mimir console on the real model plane

**Severity:** High · **Area:** UI data · **Depends on:** RF-UID-1, RF-MI-1..7

> **Status at 2026-10-12 — Open.** The Mimir console still runs only on `MimirMock`. `mimir-server` is a real PQ-mTLS inference service that can serve Granite on llama.cpp, with audited calls, but only `mimir-probe` calls it: the gateway does not route AI requests, so no browser can reach it. Agents, guardrails, evals and traces exist as libraries only.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** Start from `mimir-server`, which exists but is mesh-only with no browser route.
>
> **Still to do:**
> - Route inference from the edge to mimir-server under the caller's identity
> - Put the agent runtime, retriever and guardrail chain on that running path; today only raw completion is served
> - Expose model, eval and trace state through the edge, with reference-engine figures labelled
> - Replace MimirMock with a client for those routes
>
> Checked: `products/mimir/ui/src/main.tsx:18` · `mimir/mimir-server/tests/serve.rs:177` · `README.md:570` · `deploy/compose/smoke.sh:19`

**Prompt**

```
Make the seven Mimir screens operate the real AI plane.

- Agent Studio: real agents against real models with real streaming responses,
  real citations resolving to real fabric evidence, real tool proposals entering
  real gates, and real refusals shown with their reason code.
- Model Plane: real registered models, real serving state, real hardware
  placement and the reason for it, real measured latency and throughput (with any
  synthetic value labelled), real quantisation and VRAM accounting, real
  licence restrictions.
- Guardrails: real interception configuration, real recent blocks with category
  and evidence, real false-positive rate against budget, and a probe that runs
  the real guardrail chain.
- Evals: real per-family results with history and trend, and which promotions are
  currently blocked and why.
- Session Traces: real traces from RF-OBS-2 showing the actual assembled prompt
  layers, actual guardrail verdicts, actual gate decisions and actual model and
  prompt versions.
- Feedback: real distilled feedback with provenance, review and revocation.
- Logic Builder: real agent definitions with real tool scoping derived from
  policy, versioned and reviewable.
```

**Acceptance criteria**

- **AC-RF-UID-4a — Real inference, streamed.** *Given* Agent Studio, *when* a question is asked, *then* a real model answers with streamed tokens and citations that resolve to real evidence.
- **AC-RF-UID-4b — Traces are real.** *Given* a session trace, *when* opened, *then* it shows the actual eight assembled layers, the actual guardrail verdicts and the actual gate decisions for that run.
- **AC-RF-UID-4c — Guardrail probe is real.** *Given* the probe, *when* an injection payload is submitted, *then* the real chain runs and the real verdict with evidence is shown.
- **AC-RF-UID-4d — Evals are real and gate visibly.** *Given* the Evals screen, *when* opened, *then* real per-family results are shown along with any blocked promotion and its reason.
- **AC-RF-UID-4e — Synthetic values are labelled.** *Given* any figure originating from the reference engine rather than measurement, *when* displayed, *then* it is explicitly labelled as such.

**Done when:** every Mimir screen reflects the real plane, including traces and Evals.

---

### RF-UID-5 · Bifrost console on the real fleet

**Severity:** High · **Area:** UI data · **Depends on:** RF-UID-1, RF-B-1..6

> **Status at 2026-10-12 — Open.** The Bifrost console still runs only on `BifrostMock`. A fleet manager service exists and is real (agents check in over PQ-mTLS, operators read versions, liveness, drift and convergence as JSON), but it only records: nothing drives the rollout orchestrator, and it has no browser route. Registry, risk, rollout and bundles are libraries and CLI tools.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** Start from `bifrost-fleet`, which exists but is mesh-only with no browser route.
>
> **Still to do:**
> - Expose the fleet status the manager already serves through the edge and wire the Fleet screen to it
> - Drive the rollout orchestrator from the fleet manager so promotion and rollback exist as running operations
> - Serve the artefact registry, risk assessment and bundle receipts from a service
> - Put promotion and rollback under the dual control the admin enrolment path already implements
>
> Checked: `products/bifrost/ui/src/main.tsx:18` · `deploy/compose/smoke.sh:31` · `bifrost/rollout/tests/rollout.rs:50` · `README.md:568`

**Prompt**

```
Make the six Bifrost screens operate the real delivery system.

- Artefacts: real registry with real SBOM, signature and attestation detail;
  real verification status; real component search and advisory impact.
- Release Console: real candidates, real A.7 risk assessment with real evidence,
  real gate decisions, real promotion under dual control.
- Channels / Rings: real ring definitions and membership, real soak state, real
  promotion criteria and their current values.
- Fleet: real node inventory from real agents, real installed versions and
  digests, real health, real drift.
- Rollback: real rollback plans, real execution, real per-node outcome.
- Bundles: real air-gap bundle creation, real manifests, real receipts with
  staleness.
Every promotion, rollback and kill-switch action passes real dual control.
```

**Acceptance criteria**

- **AC-RF-UID-5a — Fleet reflects reality.** *Given* the Fleet screen, *when* opened, *then* every node, version and digest matches what the agents actually report.
- **AC-RF-UID-5b — Promotion is really gated.** *Given* a promotion to a production ring, *when* attempted, *then* real dual control is required and the rollout really begins.
- **AC-RF-UID-5c — Risk shows real evidence.** *Given* the Release Console, *when* a candidate's risk is shown, *then* the evidence is the real SBOM diff and scan findings, each citable.
- **AC-RF-UID-5d — Rollback really rolls back.** *Given* a rollback executed from the console, *when* it completes, *then* nodes report the prior version and the outcome is shown per node.
- **AC-RF-UID-5e — Bundle receipts and staleness.** *Given* an air-gapped site, *when* its state is shown, *then* the last receipt and its age are displayed.

**Done when:** the console drives a real rollout and rollback against a real fleet.

---

### RF-UID-6 · Heimdall console on real feeds and the real picture

**Severity:** High · **Area:** UI data · **Depends on:** RF-UID-1, RF-H-1..6

> **Status at 2026-10-12 — Open.** The Heimdall console still runs only on `HeimdallMock`. A feed gateway service is real (signed observations are policy-checked, fused into tracked entities, sealed and readable as JSON by a certified analyst), but the picture stays inside Heimdall, entities are unclassified, there is no per-reader clearance filtering of the picture, and no browser route.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** Start from `heimdall-feed`, which exists but is mesh-only with no browser route.
>
> **Still to do:**
> - Expose the fused picture and feed health through the edge, filtered by the caller's clearance
> - Attach the classifier and staleness detection on the running path
> - Put workflow, decision lineage and picture sync behind a service; they are libraries today
> - Replace HeimdallMock with a client and a real in-enclosure map
>
> Checked: `products/heimdall/ui/src/main.tsx:17` · `deploy/compose/smoke.sh:42` · `README.md:566` · `heimdall/decision-support/tests/ac_h_3.rs:7`

**Prompt**

```
Make the six Heimdall screens operate the real fusion system.

- Operating Picture: real tracks and entities on a real in-enclosure map
  (RF-H-5), real-time updates, real confidence and uncertainty rendering, real
  clearance filtering, real layer control, real time-slider replay.
- Feeds: real feed inventory, real health, gap and staleness detection, real
  quality metrics, real markings.
- Entities: real resolved entities with real provenance drill-down, real merge
  and split with audit.
- Workflows: real workflow definitions and instances, real approvals, real
  escalation.
- Decision Lineage: real reconstruction of past decisions with the picture state,
  options, evidence and model/prompt versions.
- Picture Sync: real peer sites, real sync state, real degradation scope, real
  convergence status.
```

**Acceptance criteria**

- **AC-RF-UID-6a — The picture is live.** *Given* real feeds, *when* the Operating Picture is open, *then* it updates in real time within the stated latency and reflects real tracks.
- **AC-RF-UID-6b — Clearance filters visibly and safely.** *Given* two operators of differing clearance, *when* both view the same area, *then* each sees only what they may, and the lower-clearance view does not reveal that anything is hidden where existence itself is protected.
- **AC-RF-UID-6c — Uncertainty is rendered.** *Given* coasted or inferred tracks, *when* displayed, *then* their uncertainty is visually distinct.
- **AC-RF-UID-6d — Lineage reconstructs.** *Given* a past decision, *when* opened, *then* the picture as it was, the options, the evidence and the decider are all shown.
- **AC-RF-UID-6e — Degradation is scoped.** *Given* a lost peer link or dead feed, *when* the picture is viewed, *then* the degraded scope is explicit and stale data is not shown as current.

**Done when:** the picture runs live on real feeds with real clearance filtering and real degradation indication.

---

### RF-UID-7 · Clients console on the real client core

**Severity:** Medium · **Area:** UI data · **Depends on:** RF-UID-1, RF-CL-1

> **Status at 2026-10-12 — Open.** Unchanged. The clients app is still one `ClientApp` in device frames on `ClientMock`; its README describes it as 'the client platforms app' without saying it is a demonstration, and nothing in the UI labels it as one. No decision between web shell and demo harness is recorded. It is not in the release artefact list, but that list only covers Rust binaries, so its absence is not a control.
> Criteria met 0 of 3; open: a, b; not verifiable by reading: c. Confidence: high.
>
> **Still to do:**
> - Record the decision: web shell or demonstration harness
> - If a demonstration, label it in the UI itself and state it in the README
> - If a product, bind the real client core over wasm; no wasm build or shell code exists
> - Make exclusion from shipped artefacts explicit once consoles are packaged at all
>
> Checked: `products/clients/ui/src/main.tsx:18` · `products/clients/ui/README.md:3` · `products/clients/ui/src/App.tsx:15` · `supply-chain/release-artefacts.txt:1`

**Evidence.** `products/clients/ui` (1 074 lines) renders one `ClientApp` inside five device frames against `ClientMock`. It is a demonstration of the shared-core concept, not a client.

**Prompt**

```
Decide what this console is, then make it honest.

1. Clarify its purpose in products/clients/ui/README.md: it is either (a) the
   web shell itself — in which case it moves to clients/shells/web and binds the
   real client core via wasm (RF-CL-4) — or (b) a design and demonstration
   harness, in which case it must say so unmistakably in the UI and never be
   presented as a shipping client.
2. If (a): bind the real core over wasm-bindgen; real sync, real key custody,
   real tier selection, real offline behaviour; delete the device frames or move
   them to the design-system documentation.
3. If (b): label it clearly, keep the mock, and exclude it from release artefacts
   so it can never be mistaken for a product.
4. Either way, remove the ambiguity — a demo that looks like a product is a
   liability in a governed platform.
```

**Acceptance criteria**

- **AC-RF-UID-7a — Purpose is unambiguous.** *Given* the console, *when* opened, *then* its nature is unmistakable from the UI itself, not only from a README.
- **AC-RF-UID-7b — If a product, it is real.** *Given* the shipping web shell, *when* used, *then* it binds the real core, syncs real data, and enforces real key custody.
- **AC-RF-UID-7c — If a demo, it cannot ship.** *Given* the release manifest, *when* inspected, *then* the demo is absent from shipped artefacts.

**Done when:** the decision is recorded and implemented; ambiguity is removed.

---
# O — End-to-end user flows

> **Why this workstream exists.** Workstreams A–N deliver capabilities. This one delivers *journeys*: the complete paths a real person takes, crossing services, consoles and governance gates, from the first moment to the audited outcome. Every screen in the repo can be individually correct while no journey completes — which is the current state. Each item below is a flow, and its acceptance criteria are written as the journey, not as a feature.
>
> **Common requirements for every flow.** Each must: complete with keyboard only and with a screen reader (RF-UI-3); render correctly at 400% zoom and on a narrow viewport; show honest loading, empty, error, partial and degraded states throughout (RF-UI-2); be resumable after a network interruption without data loss; write PQ-signed audit lineage at every governed step, correlatable by a single id (RF-RT-3); and be documented as a runbook with screenshots in `docs/dev/operations/flows/`.

---

### RF-UX-1 · First-run: enclosure bootstrap and initial administrator

**Severity:** Critical · **Area:** User flows · **Depends on:** RF-DEP-1, RF-IAM-1, RF-AD-1

> **Status at 2026-10-12 — Partial.** The Evidence ('there is no bootstrap path') is stale: `mg-admin init` creates a state directory with sealed issuer and audit keys, trust anchors, a starter policy and an empty ontology, `mg-admin user add` and `admin add` enrol the first people, and the gateway re-verifies the ledger and refuses to start on tampering. It is a single-person, command-line path, though: no ceremony record, no dual custody, no single-use bootstrap capability, and anyone with the state directory can run `user add` or `admin add` again while the gateway is stopped. It needs no network by construction.
> Criteria met 0 of 6; open: a, b, c, e; not verifiable by reading: d, f. Confidence: high.
>
> **Scope change:** `mg-admin init`, `user add` and `admin add` are the bootstrap path. Read this item as hardening that CLI path (two-person control, a ceremony record in the ledger, closing the offline path), with the designed UI split out.
>
> **Still to do:**
> - Require two people for init and the first administrators, and write a signed ceremony record as the ledger's first entries
> - Close the offline path after bootstrap: today init, user add and admin add work for whoever holds the directory and its keys
> - Add a posture check at the end (services healthy, chain verifies, anchors loaded) as one command
> - Rewrite the item for the CLI and state-directory model, or scope the designed bootstrap UI as a separate later step
> - Run the flow once under no network and record it
>
> Checked: `tools/mg-admin/src/main.rs:203` · `tools/mg-admin/tests/cli.rs:83` · `services/http-gateway/tests/state_dir_boot.rs:188` · `admin/identity/tests/ac_ad_1_2.rs:36` · `README.md:420`

**Evidence.** There is no bootstrap path. An installed enclosure has no identities, no trust root, no policy, and no way to create the first one — and this is precisely the flow where a shortcut becomes a permanent backdoor.

**Prompt**

```
Design and implement the enclosure bootstrap flow, with no permanent privileged
back door.

1. Trust root ceremony: generate or import the root key material with documented
   custody, split control where policy requires, and produce a signed record of
   the ceremony including who participated.
2. First administrator: enrolled during bootstrap under dual custody (two people
   present), with a WebAuthn credential each, in a role that is scoped and
   auditable — never a super-admin, consistent with the platform's no-super-admin
   design. The bootstrap capability is single-use and is destroyed on completion.
3. Baseline policy: install the default marking lattice, clearance levels,
   purposes and the hardened baseline configuration (RF-RT-7), all recorded.
4. Verification: the flow ends by verifying the enclosure's own posture —
   services healthy, chain genesis written and verifiable, trust anchors loaded,
   posture matching baseline — and displaying it.
5. Everything about the ceremony is written to the audit ledger as its genesis
   entries, so the chain begins with the enclosure's own creation.
6. Design the UI for this: it is the first thing anyone sees, it is performed
   under pressure, and it must be impossible to complete insecurely.
```

**Acceptance criteria**

- **AC-RF-UX-1a — A fresh enclosure becomes operable.** *Given* a freshly installed enclosure, *when* two operators complete bootstrap, *then* both can sign in, the baseline policy is in force, and the posture check passes.
- **AC-RF-UX-1b — No single-person bootstrap.** *Given* one operator, *when* bootstrap is attempted alone, *then* it cannot be completed.
- **AC-RF-UX-1c — The bootstrap path closes.** *Given* a completed bootstrap, *when* the bootstrap capability is attempted again, *then* it is refused and the attempt is a security event.
- **AC-RF-UX-1d — No super-admin exists.** *Given* the first administrator, *when* their capabilities are inspected, *then* they hold a scoped role, and no identity in the system holds unrestricted authority.
- **AC-RF-UX-1e — Genesis is auditable.** *Given* the completed bootstrap, *when* the ledger is verified from genesis, *then* the ceremony entries are present and the chain verifies.
- **AC-RF-UX-1f — Air-gapped bootstrap works.** *Given* no network at any point, *when* bootstrap runs, *then* it completes.

**Done when:** a fresh install reaches an operable, verified state through the flow, twice — once connected, once air-gapped.

---

### RF-UX-2 · Operator onboarding: enrolment to first productive action

**Severity:** High · **Area:** User flows · **Depends on:** RF-UX-1, RF-IAM-1, RF-IAM-5

> **Status at 2026-10-12 — Partial.** Request, dual-control approval and first login exist on a real path, command-line only: one IAM administrator runs `mg-admin enrol request`, a different one runs `enrol approve`, both sign single-use challenges with ML-DSA passkeys, the pending request survives a restart, and the new user can then log in at the edge or the Midgard console. There is no invitation (the requesting administrator chooses the user's password), no simulated-effect view for the approver, no expiry on user grants, no orientation, and no single correlation id.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** Rewrite around the CLI dual-control enrolment that exists (`mg-admin enrol request` / `approve`). The WebAuthn invitation step does not fit: the requesting administrator sets the user's password, and a browser cannot produce the administrators' passkey assertions.
>
> **Still to do:**
> - Let the enrolled person set their own credential; today the requesting administrator knows the password
> - Show the approver what the grant will allow before approval
> - Add time-boxed user grants with visible expiry and a renewal path
> - Tie request, approval, first login and first action together in the ledger under one id
> - Add first-sign-in orientation in the console and write the runbook
>
> Checked: `tools/mg-admin/tests/enrol.rs:96` · `services/http-gateway/tests/admin_enrolment.rs:185` · `services/http-gateway/src/admin_api.rs:499` · `tools/mg-admin/tests/cli.rs:232` · `README.md:421`

**Prompt**

```
Implement the flow from "this person should have access" to "this person did
something useful", entirely inside the platform.

1. Request: a manager or delegated administrator requests access for a person,
   specifying roles, clearance, purposes and duration, with a justification.
2. Approve: dual control where the risk tier requires it, with the approver shown
   the simulated effect of the grant (exactly what this person will be able to
   see and do) before approving.
3. Enrol: the person receives an enrolment invitation through a channel that does
   not itself grant access, registers a WebAuthn credential, and — where clearance
   requires it — completes any attestation the deployment mandates.
4. Orient: first sign-in presents what they can access, what their clearance and
   purposes mean in practice, where the governance chrome is, and what is audited.
   This is a security control as much as a courtesy: an operator who does not
   understand the marking model will mishandle data.
5. First action: the operator completes a real, governed action end to end and
   sees its audit entry — so the audit trail is understood from the outset as
   normal, not as surveillance.
6. Time-boxing: where the grant is time-boxed, expiry is visible from day one and
   renewal is a defined flow, not a scramble.
```

**Acceptance criteria**

- **AC-RF-UX-2a — The journey completes.** *Given* a new person, *when* the flow runs from request to first governed action, *then* it completes with every step audited under one correlation id.
- **AC-RF-UX-2b — Approval is informed.** *Given* the approver, *when* they approve, *then* they were shown the simulated access effect; approval without it is impossible.
- **AC-RF-UX-2c — Enrolment does not grant access.** *Given* an intercepted enrolment invitation, *when* used by anyone other than the intended person, *then* it does not yield access (bound to a second factor or an in-person step per the deployment's policy).
- **AC-RF-UX-2d — Expiry is visible early.** *Given* a time-boxed grant, *when* the operator signs in, *then* the expiry is visible, and renewal is reachable before it lapses.
- **AC-RF-UX-2e — Comprehension is supported.** *Given* first sign-in, *when* orientation is shown, *then* it explains clearance, purpose, markings and audit in the deployment's own terms.

**Done when:** the flow completes end to end for a new operator and is documented as a runbook.

---

### RF-UX-3 · Analyst journey: question to grounded, cited, audited answer

**Severity:** High · **Area:** User flows · **Depends on:** RF-UID-3, RF-UID-4, RF-MI-4

> **Status at 2026-10-12 — Open.** No step of this journey runs. The Mimir console is mock-only, the gateway does not call `mimir-server`, and the inference service answers raw completions to a probe with no fabric grounding, citations, guardrails or sessions on the running path. Grounding, the agent runtime and guardrails exist as tested libraries only.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> **Still to do:**
> - Route a question from the edge to mimir-server under the caller's clearance and purpose
> - Run retrieval over the real fabric and return citations that resolve to object ids the edge serves
> - Put guardrails and typed refusals on that path
> - Turn proposed mutations into the edge's gated actions, once the human gate is a real approval
> - Add a signed export of question, answer, citations and audit references
>
> Checked: `products/mimir/ui/src/main.tsx:18` · `README.md:570` · `deploy/compose/smoke.sh:19` · `services/http-gateway/src/lib.rs:598`

**Prompt**

```
Implement the flagship journey: an analyst asks a question and gets an answer
they can defend.

1. Ask in natural language in the Mimir console, or start from an object in the
   Midgard console and ask about it in context.
2. The system grounds the question in the object fabric under the analyst's own
   clearance and purpose, assembles the prompt through the eight layers, passes
   the input guardrail, and answers with citations.
3. Every citation resolves: clicking one navigates to the actual object, dataset
   row or audit record that supports the claim, and the analyst can verify it
   themselves. An answer whose citations do not resolve is never shown.
4. The analyst can follow up in a multi-turn session with context preserved
   (RF-MI-5), can search around from any cited object, and can pin findings into
   a working set.
5. Where the analyst asks for something requiring a mutation, the agent proposes;
   the gate is raised; the analyst approves or declines; the mutation executes or
   does not; and the audit shows the whole chain.
6. Export: the analyst produces a shareable, signed artefact containing the
   question, the answer, the citations, the model and prompt versions, and the
   audit references — so the answer is reproducible and defensible later.
7. Refusals are informative: when the model refuses (ungrounded, out of scope,
   policy-denied, guardrail-blocked), the analyst is told which and what they
   could do instead.
```

**Acceptance criteria**

- **AC-RF-UX-3a — Grounded and cited.** *Given* a question with supporting evidence in the fabric, *when* asked, *then* the answer cites specific evidence and every citation resolves to a record the analyst can open.
- **AC-RF-UX-3b — Clearance governs the answer.** *Given* evidence above the analyst's clearance, *when* the question is asked, *then* that evidence neither appears in the answer nor influences it, and the answer states that grounding was incomplete where that is true.
- **AC-RF-UX-3c — Mutations are proposed, never taken.** *Given* a question implying a change, *when* the agent responds, *then* it proposes and the change occurs only after an explicit human approval, with both audited.
- **AC-RF-UX-3d — Refusals explain.** *Given* each refusal class, *when* triggered, *then* the analyst sees which class and a constructive next step.
- **AC-RF-UX-3e — Exports are verifiable.** *Given* an exported finding, *when* verified independently, *then* its signature validates and its audit references resolve.
- **AC-RF-UX-3f — Multi-turn context holds.** *Given* a follow-up question, *when* asked, *then* prior turns inform it and the assembled context is visible in the session trace.

**Done when:** the journey completes against a real model and real fabric, with resolving citations and a verifiable export.

---

### RF-UX-4 · Data engineer journey: source to governed operational objects

**Severity:** High · **Area:** User flows · **Depends on:** RF-UID-2, RF-F-2, RF-F-6

> **Status at 2026-10-12 — Partial.** A thin real path exists without any UI: declare sources in a jobs file, the Forge worker reads them (CSV in compose; PostgreSQL, S3 and Kafka connectors exist), commits versioned datasets, projects rows onto the ontology with the job's marking and purposes, and syncs objects and links into the fabric, where an enrolled operator sees them in the Midgard console. Missing are connection test and schema discovery, profiling and expectations, transforms, review, schedules beyond a fixed interval, projection simulation, quarantine, and alerts.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Add transforms and quality expectations with quarantine to the worker's run; rows are projected as read
> - Add projection simulation with counts and samples before sync
> - Carry markings per column and have the gateway check the worker may write at the marking its jobs file declares
> - Expose runs, logs and lineage for diagnosis somewhere other than container logs
> - Wire the Forge console to it (RF-UID-2)
>
> Checked: `forge/worker/tests/sync.rs:173` · `deploy/compose/smoke.sh:50` · `README.md:457` · `README.md:565` · `products/forge/ui/src/main.tsx:19`

**Prompt**

```
Implement the journey from a raw source to objects an analyst can act on.

1. Connect a source: choose a connector, configure it, supply credentials through
   KMS references, test the connection for real, and discover the schema.
2. Ingest: run the first load, watch real progress, see real row counts and real
   quarantined records with their reasons.
3. Understand: profile the data, see column statistics and inferred types, and
   define expectations — including ones suggested from the profile.
4. Transform: build the pipeline (or ask A.6 in natural language and review the
   proposal), preview on real sampled data, iterate, and commit a versioned
   definition through review.
5. Schedule: define the schedule, its freshness SLA, and its dependencies.
6. Project: map columns to object types, see the simulated effect (how many
   objects created, updated, removed, and samples of each), confirm marking
   propagation, and commit under approval.
7. Verify: see the objects in the Midgard console, confirm their markings, and
   confirm an analyst at the appropriate clearance can find and act on them.
8. Operate: watch the schedule run, see quality results, receive a freshness
   breach alert, and diagnose it from the run log and lineage.
```

**Acceptance criteria**

- **AC-RF-UX-4a — The journey completes.** *Given* a real source, *when* the engineer completes all eight steps, *then* real objects appear in the fabric with correct markings and an analyst can query them.
- **AC-RF-UX-4b — Simulation precedes projection.** *Given* the projection commit, *when* made, *then* the engineer saw exact counts and samples first.
- **AC-RF-UX-4c — Markings propagate correctly.** *Given* a marked source column, *when* projected, *then* the object property carries the marking and a lower-clearance analyst cannot see it.
- **AC-RF-UX-4d — Failure is diagnosable.** *Given* a failed or late run, *when* the engineer investigates, *then* the run log, the failing expectation, the quarantined records and the lineage together identify the cause.
- **AC-RF-UX-4e — Nothing is lost silently.** *Given* records failing validation, *when* the run completes, *then* they are in quarantine with reasons and the run reports partial success explicitly.

**Done when:** the full journey completes against a real source and produces governed objects.

---

### RF-UX-5 · Release journey: commit to production, with rollback

**Severity:** High · **Area:** User flows · **Depends on:** RF-SC-5, RF-UID-5, RF-B-3

> **Status at 2026-10-12 — Partial.** The build half exists as commands: `just gate` reproduces, SBOMs, scans, signs and verifies a binary, and the image entrypoint re-verifies before running. The delivery half does not run: the fleet manager records check-ins and reports drift but pushes nothing, the rollout orchestrator with automatic rollback is a tested library that nothing drives, the Bifrost console is mock-only, no release has been cut, and the workflows the README names are not in this tree (there is no `.github` directory).
> Criteria met 0 of 5; open: a, b, c, e; not verifiable by reading: d. Confidence: medium.
>
> **Scope change:** Assumes a CI and release pipeline; none is in the tree (RF-CA-4).
>
> **Still to do:**
> - Drive the rollout orchestrator from the fleet manager so agents are told to install and roll back
> - Add promotion under dual control as a running operation
> - Join registry and fleet data so 'what runs where, signed by whom, containing what' can be answered
> - Run the air-gap bundle path end to end against a disconnected install and return a receipt
> - Restore or add CI and release workflows; none are in the repository
>
> Checked: `justfile:344` · `deploy/compose/smoke.sh:31` · `bifrost/rollout/tests/rollout.rs:50` · `bifrost/bundle/tests/ac_b_3.rs:65` · `README.md:568`

**Prompt**

```
Implement the journey from a merged change to a verified production deployment —
and back out of it.

1. Build: the change is built reproducibly, SBOM'd, scanned, signed and verified
   by the gate, and the engineer can see each stage's result and evidence.
2. Register: the artefact appears in the Bifrost registry with its full evidence
   chain.
3. Assess: A.7 produces a risk assessment on real evidence; the release manager
   sees the risk, the evidence and the gate decision.
4. Promote: promotion to the canary ring under dual control; the release manager
   watches real nodes install, real health checks pass, and real soak elapse.
5. Progress: automatic promotion through rings on criteria, with the manager able
   to pause, resume or abort at any point.
6. Detect and roll back: a regression appears in the metrics; the rollout aborts
   automatically or the manager aborts it; nodes roll back; the manager confirms
   the fleet is back on the prior version.
7. Air-gapped variant: the same journey where the target is a disconnected site,
   via bundle creation, physical transport, offline verification, application and
   receipt.
8. Post-release: the manager can answer, from the console, exactly what is running
   where, signed by which key, containing which components.
```

**Acceptance criteria**

- **AC-RF-UX-5a — The journey completes.** *Given* a change, *when* the release journey runs, *then* it reaches production through rings with every gate enforced and every step audited.
- **AC-RF-UX-5b — Rollback restores.** *Given* a regression, *when* rollback occurs, *then* every affected node reports the prior version and the console confirms it.
- **AC-RF-UX-5c — Unsigned never ships.** *Given* an artefact that fails verification at any stage, *when* promotion is attempted, *then* it is refused and cannot be overridden without a dual-controlled, loudly-audited exception.
- **AC-RF-UX-5d — The air-gapped variant completes.** *Given* a disconnected site, *when* the bundle journey runs, *then* the site is updated and the receipt returns.
- **AC-RF-UX-5e — Inventory answers.** *Given* the question "what is running where, signed by whom, containing what", *when* asked in the console, *then* it is answered from real fleet and registry data.

**Done when:** both the connected and air-gapped release journeys complete, including a real rollback.

---

### RF-UX-6 · Governance journey: policy change from proposal to enforcement

**Severity:** High · **Area:** User flows · **Depends on:** RF-AD-5, RF-AD-3, RF-AD-4

> **Status at 2026-10-12 — Partial.** On the running path a policy change is: edit `policy.json` by hand and restart the gateway, which refuses an invalid file. The edge's policy dry-run is a real PDP decision for one probe against the current policy. Proposal, simulation of a change, second-approver review, versioned commit and revert exist only as tested admin libraries (`admin-sim`, `admin-config`) with no service, and the admin console is mock-only.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Serve policy change as an administration route under the same dual control as enrolment: propose, simulate, approve, commit
> - Run the simulation against the gateway's real PDP and verify the committed effect equals it
> - Version and sign policy.json changes and reload without a restart
> - Add revert through the same route
> - Add the plain-language explanation only after the above; nothing calls Mimir yet
>
> Checked: `services/http-gateway/tests/fabric_edge.rs:347` · `admin/sim/tests/ac_ad_5.rs:99` · `admin/config/tests/ac_ad_7.rs:45` · `products/admin/ui/src/main.tsx:17` · `README.md:419`

**Prompt**

```
Implement the highest-blast-radius journey in the platform.

1. Propose: an administrator drafts a policy, marking-lattice or retention change
   in the console, optionally with A.AD1's assistance (advisory only).
2. Validate: syntax, conflicts and unreachable rules are surfaced immediately.
3. Simulate: the exact effect is computed against the real PDP over real probes —
   who gains access, who loses it, which data classes are affected, and whether
   the blast radius crosses the high threshold.
4. Explain: A.AD2 renders the simulated diff in plain language, leading with
   impact and flagging anything irreversible. It explains; it never authorises.
5. Review and approve: a second, distinct approver sees the draft, the simulation
   and the explanation, and approves or declines with a recorded reason.
6. Commit: the change is signed, versioned, committed to the configuration
   repository, and rolled out progressively.
7. Verify: the administrator confirms enforcement — a probe that was denied is now
   permitted (or vice versa) exactly as simulated.
8. Roll back: the change can be reverted through the same flow with the same
   rigour.
```

**Acceptance criteria**

- **AC-RF-UX-6a — Simulation matches enforcement.** *Given* a committed change, *when* the actual effect is measured, *then* it equals the simulated effect for every probe (this is AC-AD-5, now exercised through the whole journey).
- **AC-RF-UX-6b — No unapproved commit.** *Given* a proposed change, *when* commit is attempted without a distinct approver, *then* it is refused.
- **AC-RF-UX-6c — Blast radius is confronted.** *Given* a high-blast change, *when* it reaches approval, *then* the scale is prominent and requires explicit acknowledgement.
- **AC-RF-UX-6d — Explanation has no authority.** *Given* the code path, *when* traced, *then* A.AD2's output does not influence the decision, and the UI labels it as explanation.
- **AC-RF-UX-6e — Revert is symmetric.** *Given* a committed change, *when* reverted, *then* it goes through the same validation, simulation, approval and rollout.

**Done when:** the journey completes end to end with simulation-equals-enforcement verified.

---

### RF-UX-7 · Incident journey: alert to resolution to evidence

**Severity:** High · **Area:** User flows · **Depends on:** RF-OBS-4, RF-AD-2, RF-AD-7

> **Status at 2026-10-12 — Open.** No incident journey runs. The gateway exposes health, readiness and Prometheus metrics with an SLO breach visible in a scrape, and `docs/ops` has incident-response and runbook documents, but there is no alerting, no correlation id, no investigator on a running path, and break-glass exists only as a tested library behind a mock admin console.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: medium.
>
> **Still to do:**
> - Add alert rules over the existing metrics with runbook links
> - Add a correlation id to requests, logs and ledger entries
> - Serve break-glass as an administration route with alerting and mandatory review
> - Derive an incident timeline from the ledger and export it as a signed bundle
> - Run one drill end to end and record it
>
> Checked: `services/http-gateway/tests/rc_33.rs:30` · `admin/breakglass/tests/ac_ad_8.rs:20` · `products/admin/ui/src/main.tsx:17` · `services/http-gateway/src/lib.rs:563`

**Prompt**

```
Implement the operational incident journey, including the break-glass branch.

1. Detect: an alert fires with severity, affected scope and a runbook link.
2. Triage: the responder opens the Overview, sees the incident in context
   alongside health, posture and recent changes, and can correlate by time.
3. Investigate: traces, logs and audit are reachable from the alert by
   correlation id; the audit investigator (A.AD3) can be asked questions over the
   ledger and answers with citations.
4. Act: the responder takes remedial action through normal governed paths where
   possible. Where normal paths are insufficient, the break-glass branch opens a
   scoped, time-boxed, alerting window (RF-AD-2), and every action within it is
   marked.
5. Communicate: incident status is visible to others; concurrent responders see
   each other's actions; a timeline accumulates automatically from the audit
   trail rather than from manual note-taking.
6. Resolve and review: the incident closes with a resolution; the break-glass
   review is mandatory; the timeline, evidence and decisions export as a signed
   bundle for the post-incident review.
7. Learn: actions from the review become tracked items, and any alert that proved
   unactionable is flagged for tuning (RF-OBS-4).
```

**Acceptance criteria**

- **AC-RF-UX-7a — Alert to evidence in one path.** *Given* an alert, *when* the responder follows it, *then* the runbook, the correlated traces, logs and audit entries are all reachable without manual correlation.
- **AC-RF-UX-7b — Break-glass is scoped, loud and reviewed.** *Given* the break-glass branch, *when* used, *then* alerts deliver, scope constrains, every action is marked, and the review is mandatory.
- **AC-RF-UX-7c — Timeline is automatic.** *Given* a resolved incident, *when* the timeline is produced, *then* it is derived from the audit trail and requires no manual reconstruction.
- **AC-RF-UX-7d — Evidence exports verify.** *Given* the incident bundle, *when* verified independently, *then* its signature and contents validate.
- **AC-RF-UX-7e — Concurrent responders coordinate.** *Given* two responders, *when* one acts, *then* the other sees it without refreshing.

**Done when:** a simulated incident is run as a drill, end to end, producing a verifiable evidence bundle.

---

### RF-UX-8 · Operator journey: monitor, decide, act on the operating picture

**Severity:** High · **Area:** User flows · **Depends on:** RF-UID-6, RF-H-4

> **Status at 2026-10-12 — Partial.** Only the first sliver runs: a certified source submits signed observations, they are fused into tracked entities, and a certified analyst reads the picture as JSON from the command line. The Heimdall console is mock-only, there are no watch conditions or alerts, entity adjudication, handover or workflow on a running path, and decision support is a library tested against a deterministic mock provider.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Expose the picture and feed health to the console through the edge (RF-UID-6)
> - Add watch conditions and alerts on the feed service
> - Put the workflow engine and decision support behind a service, with decision support calling mimir-server
> - Add reversible entity adjudication and shift handover
>
> Checked: `deploy/compose/smoke.sh:42` · `heimdall/decision-support/tests/ac_h_3.rs:7` · `heimdall/workflow/tests/ac_h_4.rs:96` · `products/heimdall/ui/src/main.tsx:17` · `README.md:566`

**Prompt**

```
Implement the Heimdall operator journey.

1. Situational awareness: the operator opens the picture, orients by area of
   interest, sees feed health and any degradation, and configures watch
   conditions.
2. Detect: a watch condition fires; the alert is prioritised, deduplicated and
   acknowledgeable; the operator navigates directly to the triggering entity.
3. Understand: the operator drills from the picture element to its constituent
   source reports, sees confidence and its basis, and can adjudicate an ambiguous
   entity resolution.
4. Decide: the operator requests courses of action; A.8 generates cited, ranked,
   neutrally-presented options grounded in the real picture, or states
   insufficiency; the operator selects, with the options and evidence recorded.
5. Act: the decision enters a workflow with approvals and deadlines; the operator
   tracks it to completion; escalation happens automatically if it stalls.
6. Hand over: at shift end, the operator produces a handover covering what
   changed, what is outstanding and what was decided.
7. Review: any decision can be reconstructed later exactly as it was made.
```

**Acceptance criteria**

- **AC-RF-UX-8a — The journey completes.** *Given* real feeds, *when* the operator moves from awareness through detection, understanding, decision, action and handover, *then* every step works and is audited.
- **AC-RF-UX-8b — Options are cited and neutral.** *Given* generated options, *when* presented, *then* each cites evidence, ranking is explained, presentation is neutral, and insufficiency is stated rather than filled with speculation.
- **AC-RF-UX-8c — Adjudication is reversible.** *Given* an entity-resolution adjudication, *when* it proves wrong, *then* it can be reversed and both actions are audited.
- **AC-RF-UX-8d — Stalled workflows escalate.** *Given* a workflow step past its deadline, *when* it passes, *then* escalation occurs automatically and is visible.
- **AC-RF-UX-8e — Handover is complete.** *Given* shift end, *when* handover is produced, *then* it covers changes, outstanding items and decisions, and is retained.

**Done when:** the journey completes against real feeds with A.8 on real grounding.

---

### RF-UX-9 · Client journey: device enrolment, offline work, sync and revocation

**Severity:** High · **Area:** User flows · **Depends on:** RF-CL-1..6, RF-PER-5

> **Status at 2026-10-12 — Open.** No client exists to take the journey. `clients/shells` still holds manifests, accessibility declarations and audit tooling only, with no shell code, and the clients web app is a mock demonstration. Sync convergence, tier selection, sealed storage and verified install are tested at library level.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** Blocked on Workstream P: the client shells are still manifests only.
>
> **Still to do:**
> - Build at least one real shell on the shared core (the web shell over wasm is the nearest)
> - Add device enrolment and revocation to the edge; no device identity exists there
> - Give the client a sync endpoint on the enclosure side
> - Surface operationally significant conflicts for a human instead of auto-merging
> - Treat this item as blocked on workstream P and say so in the plan
>
> Checked: `clients/shells/web/manifest.toml:1` · `products/clients/ui/src/main.tsx:18` · `clients/sync/tests/convergence.rs:36` · `clients/inference/tests/tiering.rs:188` · `clients/delivery/tests/install.rs:50`

**Prompt**

```
Implement the end-to-end client lifecycle as a user journey.

1. Install: the operator installs a signed client from its platform channel; the
   client verifies its own integrity before first run.
2. Enrol: the device is enrolled to the enclosure with the operator's identity
   and a device credential in the platform's key custody, showing the operator
   which custody floor their device provides (Secure Enclave, StrongBox, TPM,
   Secret Service, brokered) and what that means.
3. Work online: the operator sees the same governed data as on the console,
   filtered by clearance, with markings visible.
4. Work offline: connectivity drops; the operator continues; edits queue; the UI
   is explicit about what is local and unsynced.
5. Sync: connectivity returns; changes converge; conflicts — where they are
   operationally significant — are surfaced for human resolution rather than
   silently merged.
6. Use AI on device: the operator asks a question; the tier selector chooses
   on-device, hybrid or remote per privacy and policy, shows which tier and why,
   and refuses rather than egressing data that may not leave.
7. Lose the device: the operator or an administrator revokes it; the next contact
   wipes it; if it never contacts again, its sealed data remains inert; the
   revocation and the wipe are audited.
```

**Acceptance criteria**

- **AC-RF-UX-9a — The journey completes on every supported platform.** *Given* each shell, *when* the journey runs, *then* it completes, with the platform's key-custody floor correctly reported.
- **AC-RF-UX-9b — Offline work is durable and honest.** *Given* offline edits and a device restart, *when* connectivity returns, *then* everything syncs and the UI was explicit throughout about what was unsynced.
- **AC-RF-UX-9c — Tier selection is privacy-first and visible.** *Given* a question involving data that may not egress, *when* asked, *then* it is served on-device or refused, never egressed, and the tier and reason are shown.
- **AC-RF-UX-9d — Revocation wipes or renders inert.** *Given* a revoked device, *when* it next contacts, *then* it wipes; *and if* it never contacts, *then* its data remains cryptographically inert.
- **AC-RF-UX-9e — Conflicts reach a human where they matter.** *Given* an operationally significant conflict, *when* sync occurs, *then* it is surfaced for resolution rather than auto-merged.

**Done when:** the journey completes on all six shells and the revocation path is proven both ways.

---

### RF-UX-10 · Auditor journey: independent verification without operational access

**Severity:** High · **Area:** User flows · **Depends on:** RF-PER-2, RF-AD-7, RF-CMP-1

> **Status at 2026-10-12 — Partial.** The pieces are partly there but not joined. An `Auditor` administrator role exists, is proven read-only at library level and can be bound with `mg-admin admin add`, but the edge gives it no surface: the ledger is read through `GET /api/v1/audit` with an ordinary user's bearer token, each row re-verified. There is no `mg-audit` tool anywhere in the tree, no evidence export, no sampling, and ledger rows are not filtered by the reader's clearance.
> Criteria met 0 of 5; open: b, c, d; not verifiable by reading: a, e. Confidence: medium.
>
> **Scope change:** Assumes an `mg-audit` verifier. No such tool exists; building it is the first step (see RF-PER-2).
>
> **Still to do:**
> - Build the offline verifier the item assumes (`mg-audit` does not exist) and a signed ledger export for it to check against trust/audit.pub
> - Give the Auditor role a read-only route to ledger, policy and configuration history, with a test that every mutation is refused at the edge
> - Filter ledger rows by the reader's clearance and record the withholding
> - Add control sampling over approvals and enrolments
> - Confirm with a test that audit reads and exports are themselves recorded
>
> Checked: `admin/identity/tests/ac_ad_1_2.rs:58` · `services/http-gateway/src/enclosure.rs:577` · `tools/mg-admin/src/main.rs:87` · `services/http-gateway/src/lib.rs:624`

**Prompt**

```
Implement the journey for someone whose job is to check, not to operate.

1. Scoped access: an auditor role with read-only access to audit, policy history,
   configuration history and evidence — and no operational capability at all.
   Prove the separation with a test.
2. Investigate: search and filter the ledger; reconstruct a timeline for an actor,
   a resource or an incident; use A.AD3 with citations they can verify
   independently.
3. Verify independently: export a signed evidence bundle and verify it on a
   machine with no Midgard installation beyond mg-audit and the trust anchors —
   the whole point is that the auditor need not trust the system they are
   auditing.
4. Sample and test controls: pull a sample of dual-control approvals, break-glass
   windows, policy changes or AI decisions and confirm each followed its control.
5. Report: produce a findings artefact with citations, and record it — the audit
   of the audit.
6. Constrain: the auditor's own activity is audited, and the auditor cannot see
   content above their clearance, even in evidence.
```

**Acceptance criteria**

- **AC-RF-UX-10a — Read-only is structural.** *Given* the auditor role, *when* any mutation is attempted through any surface, *then* it is refused; no capability exists.
- **AC-RF-UX-10b — Independent verification succeeds.** *Given* an exported bundle, *when* verified on an unrelated machine with only `mg-audit` and the anchors, *then* it validates without contacting the enclosure.
- **AC-RF-UX-10c — Control sampling works.** *Given* a control and a period, *when* the auditor samples, *then* they receive a defensible sample with the evidence for each item.
- **AC-RF-UX-10d — Clearance still applies.** *Given* evidence above the auditor's clearance, *when* they investigate, *then* it is withheld and the withholding itself is visible and audited.
- **AC-RF-UX-10e — The auditor is audited.** *Given* auditor activity, *when* the ledger is queried, *then* every query and export is recorded.

**Done when:** the auditor role exists with proven read-only separation and offline independent verification.

---

### RF-UX-11 · Air-gapped site journey: install, operate, update, report

**Severity:** High · **Area:** User flows · **Depends on:** RF-DEP-5, RF-B-4

> **Status at 2026-10-12 — Partial.** Individual offline pieces exist: the acceptance suite runs under `--network none`, `mg-admin` bootstrap and import never touch the network, bundles are verified offline with tamper refused in tests, and there are produce and install scripts for an air-gap bundle and for model bundles. No site has been taken through install, operate, update and outbound report as one journey, no release has been cut to install from, and several of the journeys it must run offline (analyst, AI, admin) do not run at all yet.
> Criteria met 0 of 5; open: b, c, d, e; not verifiable by reading: a. Confidence: medium.
>
> **Still to do:**
> - Cut a signed bundle and install it on a host with no network, then bootstrap with mg-admin, and record the run
> - Show a bundle's contents and intended changes before applying, and produce a receipt
> - Add a marking-aware outbound export that lists what was withheld
> - Ship the docs/ops runbooks inside the bundle and exercise one recovery from them
> - Re-scope criterion b to the journeys that actually run
>
> Checked: `deploy/acceptance/run-airgapped.sh:1` · `deploy/air-gap-bundle/install.sh:1` · `bifrost/bundle/tests/ac_b_3.rs:77` · `tools/mg-admin/src/main.rs:4` · `README.md:36`

**Prompt**

```
Implement the complete life of a disconnected site as one documented journey.

1. Install: the site is installed from physical media alone, with every artefact
   verified offline before it runs, and bootstrap (RF-UX-1) completed locally.
2. Operate: every capability except federation and egress works — analyst
   journeys, data engineering, fusion, AI inference, admin, audit.
3. Update: a signed bundle arrives physically; it is verified offline; the
   operator sees exactly what it contains and what it will change; it applies
   through the normal ring mechanics; a receipt is produced.
4. Report outward: audit exports, telemetry and incident evidence leave by the
   same physical path, subject to marking-based egress rules, with the operator
   seeing exactly what is and is not included and why.
5. Recover: a failure is diagnosed and recovered locally, with no assumption of
   remote assistance, using the runbooks shipped in the bundle.
6. Prove: the whole journey is exercised under `--network none` and recorded.
```

**Acceptance criteria**

- **AC-RF-UX-11a — Install from media alone.** *Given* only physical media, *when* installation and bootstrap run with no network, *then* the site is operable.
- **AC-RF-UX-11b — Full capability offline.** *Given* the air-gapped site, *when* the analyst, data, fusion, AI and admin journeys are attempted, *then* each completes.
- **AC-RF-UX-11c — Updates are transparent.** *Given* an arriving bundle, *when* it is inspected, *then* the operator sees its contents and intended changes before applying, and any tamper is refused.
- **AC-RF-UX-11d — Outbound respects markings.** *Given* an outbound export, *when* produced, *then* the operator sees what was withheld and why.
- **AC-RF-UX-11e — Local recovery works.** *Given* an induced failure, *when* the operator follows the shipped runbook, *then* the site recovers without external assistance.

**Done when:** the whole journey runs under `--network none` and is documented as the definitive air-gap runbook.

---

### RF-UX-12 · Developer journey: build on the platform

**Severity:** Medium · **Area:** User flows · **Depends on:** RF-SDK-1..5, RF-API-5

> **Status at 2026-10-12 — Partial.** A local enclosure does come up in one command: `just compose-up` builds and starts five services with a demonstration ontology and sample data, with a smoke script, after a one-time online toolchain image build; `just gateway-http` gives a seeded development edge. Beyond that the developer journey is thin: the only SDK is TypeScript with no tests or examples, its HTTP client reports every 403 as a human-gate error, the SDK screen's reflection data is fixed, there is no try-it, and there are no extension templates.
> Criteria met 0 of 4; open: b, c, d; not verifiable by reading: a. Confidence: medium.
>
> **Still to do:**
> - Distinguish policy denial from human-gate-required in the edge's 403 and in the SDK's typed errors, with a compiled example
> - Add an extension template for at least a Forge connector, with a test that it cannot skip policy and audit
> - Enrol a development user in compose so the seeded enclosure is usable without stopping the gateway
> - Decide which SDK languages exist; only sdk/ts does
> - Run compose-up with the network off after the image build and record it
>
> Checked: `justfile:51` · `deploy/compose/smoke.sh:13` · `sdk/ts/src/http/HttpGateway.ts:84` · `services/http-gateway/src/lib.rs:920` · `forge/connector-sdk/tests/connectors.rs:1`

**Prompt**

```
Implement the journey for someone extending the platform rather than operating it.

1. Get started: a local development enclosure stands up in one command with
   representative seeded data and clear instructions, offline.
2. Explore: browse the API reference, use the governed try-it (RF-API-5), and
   understand the governance envelope from the documentation before writing code.
3. Build: obtain an SDK for their language, authenticate as a workload or a user,
   make governed calls, handle typed errors and human gates correctly, and see
   their own audit entries.
4. Extend: write a Forge connector, an object action function, a Heimdall feed
   adapter, or an agent tool — each from a template that inherits every control
   by construction, so a third-party extension cannot bypass governance.
5. Test: run against the local enclosure with the mocks available and the real
   services available, and use the same acceptance tooling the platform uses.
6. Ship: package, sign and register their extension through the same supply-chain
   gate as first-party code.
```

**Acceptance criteria**

- **AC-RF-UX-12a — One command, offline.** *Given* a clean machine with the prerequisites, *when* the developer runs the documented command with no network, *then* a working enclosure with seeded data is available.
- **AC-RF-UX-12b — Extensions inherit controls.** *Given* an extension built from a template, *when* it runs, *then* policy, audit, marking and sandboxing apply automatically; a test attempting to bypass them fails.
- **AC-RF-UX-12c — Errors and gates are handleable.** *Given* the SDK, *when* a typed error or human-gate response is received, *then* the documented handling produces correct behaviour, proven by a compiled example.
- **AC-RF-UX-12d — Extensions ship through the same gate.** *Given* a third-party extension, *when* released, *then* it passes the same reproducibility, SBOM, scan, sign and verify stages.

**Done when:** the local enclosure is one command; extension templates exist for all four extension points; examples compile and run.

---

# P — Client platforms

> **Status.** `clients/` contains 3 119 lines of genuine shared core across eight crates — key custody with per-platform security floors, sealed local storage, CRDT sync, PQ-mTLS enclosure connection, tiered inference with the A.C1–A.C3 prompts, the extension broker, and delivery/install records. `clients/shells/` contains **twelve TOML files and a README**. There is no Swift, Kotlin, GTK, Tauri, SvelteKit or extension code, no UniFFI binding, and no wasm build. `verify-shells-accessibility.sh` checks that each shell *declares* a complete affordance set — a conformance contract, not conformance.

---

### RF-CL-1 · Language bindings: UniFFI and wasm-bindgen for the shared core

**Severity:** High · **Area:** Clients · **Depends on:** RF-PER-5 · **Blocks:** RF-CL-2..6

> **Status at 2026-10-12 — Partial.** No UniFFI or wasm-bindgen code exists: no .udl, no `#[uniffi::export]`, no `#[wasm_bindgen]`, and neither crate is vendored. The only step taken is an FFI-shaped opaque-blob surface on `ClientCore` (`drain_pending_encoded` / `merge_remote_encoded`, RT-58) with one Rust test; `clients/shells/SHELL_BUILD.md` states the real bindings are still to do.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** An FFI-shaped opaque-blob surface exists (`clients/core/src/lib.rs`) and `SHELL_BUILD.md` lists the remaining steps. Scope this to vendoring uniffi and wasm-bindgen, annotating, and widening the surface beyond sync blobs.
>
> **Still to do:**
> - Vendor uniffi and wasm-bindgen and define one FFI definition covering session, sync state, object read/write, key custody/unlock, inference and events (today only edit/read/sync blobs are FFI-shaped)
> - Generate Swift, Kotlin and wasm bindings from that definition and check the generated surface in
> - Map CoreError classes to per-language errors preserving class and retryability, with a test per class
> - Define the async/threading model for sync and inference and test that no UI thread blocks
> - Add boundary leak / lifetime tests and a smoke app per language
>
> Checked: `clients/core/src/lib.rs:123` · `clients/core/src/lib.rs:146` · `clients/core/tests/one_core.rs:152` · `clients/shells/SHELL_BUILD.md:1` · `clients/shells/apple/manifest.toml:11`

**Prompt**

```
Expose the shared Rust core to every shell language.

1. Define the FFI surface deliberately: the smallest API that lets a shell be
   presentation-only (KDD-1) — session, sync state, object read/write, pending
   operations, key custody state and unlock, inference request with tier, policy
   context, and events. Nothing security-relevant crosses as raw data the shell
   could misuse.
2. Implement UniFFI bindings for Swift and Kotlin, and wasm-bindgen for web and
   the extension, from one definition so the surfaces cannot diverge.
3. Async and threading: define how long-running operations (sync, inference)
   surface to each platform's concurrency model without blocking a UI thread.
4. Errors: map the core's typed errors into idiomatic errors per language, losing
   no information, and never collapsing a fail-closed refusal into a generic
   failure the shell might retry blindly.
5. Memory and lifetime safety across the boundary, with tests for leaks and
   use-after-free.
6. Build integration: each binding builds reproducibly and is gated (RF-SC-5),
   and the generated surface is checked into review so a change to it is visible.
```

**Acceptance criteria**

- **AC-RF-CL-1a — One core, five bindings.** *Given* the FFI definition, *when* bindings are generated, *then* Swift, Kotlin, wasm and native surfaces all derive from it and expose equivalent capability.
- **AC-RF-CL-1b — No security logic crosses.** *Given* the FFI surface, *when* inspected, *then* no cryptographic operation, key material, or sync-merge decision is exposed for a shell to perform.
- **AC-RF-CL-1c — Errors survive the boundary.** *Given* each core error class, *when* it crosses, *then* the platform error preserves its class and retryability.
- **AC-RF-CL-1d — No UI blocking.** *Given* a long sync or inference, *when* it runs, *then* the UI thread is never blocked on any platform.
- **AC-RF-CL-1e — No leaks.** *Given* an extended run exercising the boundary, *when* memory is measured, *then* it is stable.

**Done when:** all bindings generate from one definition and are exercised by a smoke app per language.

---

### RF-CL-2 · Apple shell (iOS, macOS)

**Severity:** High · **Area:** Clients · **Depends on:** RF-CL-1

> **Status at 2026-10-12 — Open.** `clients/shells/apple/` holds only `manifest.toml`, `accessibility.toml` and an empty `audits/` directory; there is no Swift, Xcode project or XCUITest code. Biometric assertion verification exists only in the Rust core (`clients/keystore/src/auth.rs`) as the original plan described.
> Criteria met 0 of 5; open: a, c, d, e; not verifiable by reading: b. Confidence: high.
>
> **Still to do:**
> - Land the SwiftUI app project linking the UniFFI Swift binding (blocked on RF-CL-1)
> - Implement Secure Enclave custody and LocalAuthentication unlock with the assertion verified by the core
> - Implement app-switcher snapshot protection, background sync, notifications and deep links
> - Write the XCUITest accessibility suite and record an on-device audit under `clients/shells/apple/audits/`
> - Produce signed iOS and macOS builds through the release gate
>
> Checked: `clients/shells/apple/manifest.toml:1` · `clients/shells/README.md:1` · `clients/keystore/tests/auth.rs:24`

**Prompt**

```
Build the SwiftUI shell — presentation only, per KDD-1.

1. Navigation and screens matching the client journey (RF-UX-9), rendered with
   the Midgard design language adapted to Apple HIG, using the core for
   everything security- or data-related.
2. Key custody: Secure Enclave for the device key, biometric unlock via
   LocalAuthentication with the assertion verified by the core
   (midgard_client_keystore::auth) — never by the shell.
3. Platform integration: notifications, background sync within the platform's
   limits, share sheet, deep links, universal links, Handoff where sensible,
   and correct behaviour on lock, background and termination (no governed data
   visible in the app switcher snapshot).
4. Accessibility: VoiceOver, Dynamic Type, contrast, reduce motion, full
   keyboard access on macOS — verified with XCUITest, satisfying the
   accessibility.toml declaration rather than merely declaring it.
5. Distribution: signed builds for App Store and enterprise/MDM, reproducible
   where the platform permits, gated by RF-SC-5.
6. Offline: full offline operation with sealed local storage and honest
   unsynced-state indication.
```

**Acceptance criteria**

- **AC-RF-CL-2a — The journey completes on iOS and macOS.** *Given* each platform, *when* RF-UX-9 is performed, *then* it completes.
- **AC-RF-CL-2b — Custody is Secure Enclave.** *Given* the enrolled device, *when* custody is inspected, *then* the key is non-exportable in the Secure Enclave and the reported floor matches.
- **AC-RF-CL-2c — Biometrics are verified by the core.** *Given* a biometric unlock, *when* traced, *then* the assertion is verified in the core and no secret crosses into shell code.
- **AC-RF-CL-2d — No data in the switcher.** *Given* backgrounding, *when* the app switcher is shown, *then* no governed content is visible.
- **AC-RF-CL-2e — Accessibility is verified.** *Given* the XCUITest accessibility suite, *when* it runs, *then* every declared affordance is demonstrated.

**Done when:** signed iOS and macOS builds complete the journey and pass the on-device accessibility suite.

---

### RF-CL-3 · Android shell

**Severity:** High · **Area:** Clients · **Depends on:** RF-CL-1

> **Status at 2026-10-12 — Open.** `clients/shells/android/` holds only two TOML declarations and an empty `audits/` directory; there is no Kotlin, Gradle or Espresso code anywhere in the tree.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Land the Jetpack Compose app project linking the UniFFI Kotlin binding (blocked on RF-CL-1)
> - Implement StrongBox/TEE custody with honest floor reporting and below-floor enrolment refusal
> - Apply FLAG_SECURE to governed content and handle process death / configuration change
> - Write the Espresso accessibility suite and run the Accessibility Scanner across the device matrix
> - Produce signed AAB and APK through the release gate
>
> Checked: `clients/shells/android/manifest.toml:1` · `clients/shells/README.md:1` · `clients/shells/SHELL_BUILD.md:1`

**Prompt**

```
Build the Jetpack Compose shell — presentation only.

1. Screens and navigation per RF-UX-9, in the Midgard design language adapted to
   Material guidelines.
2. Key custody: StrongBox where available with a documented, reported fallback to
   the TEE-backed keystore; BiometricPrompt with the assertion verified by the
   core. Report the actual floor achieved, never the best case.
3. Platform integration: notifications with correct channel and privacy settings,
   WorkManager background sync, share targets, deep links, and correct handling
   of process death, configuration change and FLAG_SECURE for governed content.
4. Accessibility: TalkBack, font scaling, contrast, touch targets, switch access —
   verified with Espresso and the Accessibility Scanner.
5. Distribution: signed AAB for Play and a signed APK for sideload/MDM, gated by
   RF-SC-5.
6. Device diversity: verify across API levels, screen sizes and OEM keystore
   behaviours; a device failing the minimum custody floor is refused enrolment
   with a clear explanation, not silently downgraded.
```

**Acceptance criteria**

- **AC-RF-CL-3a — The journey completes.** *Given* Android, *when* RF-UX-9 is performed, *then* it completes across the supported API range.
- **AC-RF-CL-3b — Custody floor is honest.** *Given* a device without StrongBox, *when* enrolled, *then* the reported floor is the actual one and policy decides whether it is acceptable.
- **AC-RF-CL-3c — Below-floor devices are refused.** *Given* a device below the minimum floor, *when* enrolment is attempted, *then* it is refused with an explanation.
- **AC-RF-CL-3d — Governed content is protected.** *Given* governed content on screen, *when* a screenshot or recents snapshot is attempted, *then* it is blocked.
- **AC-RF-CL-3e — Accessibility is verified.** *Given* the Espresso accessibility suite and the Accessibility Scanner, *when* run, *then* every declared affordance is demonstrated with no violations.

**Done when:** signed Play and sideload builds complete the journey and pass the accessibility suite across the device matrix.

---

### RF-CL-4 · Web shell (SvelteKit + WASM)

**Severity:** High · **Area:** Clients · **Depends on:** RF-CL-1, RF-UI-3

> **Status at 2026-10-12 — Open.** `clients/shells/web/` has two TOML files, an empty `audits/` directory and a Playwright + axe-core audit script (`a11y/axe-audit.mjs`) with nothing to audit: there is still no `package.json`, no SvelteKit source and no wasm build, although `pnpm-workspace.yaml` still lists the directory.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Add `clients/shells/web/package.json` and the SvelteKit app binding the core via wasm-bindgen (blocked on RF-CL-1)
> - Implement WebAuthn custody, sealed OPFS/IndexedDB storage and complete clearing on sign-out
> - Add the service worker with offline operation and an explicit update flow
> - Run the axe audit and strict-CSP checks against the built shell
> - Decide whether `products/clients/ui` is this shell or a demo
>
> Checked: `pnpm-workspace.yaml:8` · `clients/shells/web/a11y/axe-audit.mjs:1` · `clients/shells/web/manifest.toml:1` · `clients/shells/SHELL_BUILD.md:1`

**Evidence.** `pnpm-workspace.yaml` lists `clients/shells/web` as a workspace package; the directory contains two TOML files and no `package.json`, so `pnpm -r` silently skips it.

**Prompt**

```
Build the enclosure-hosted web client.

1. SvelteKit application binding the core via wasm-bindgen, with the same
   presentation-only discipline as the native shells.
2. Browser key custody: WebAuthn-backed, using the strongest available mechanism,
   with the actual floor reported. Persist sealed data in OPFS/IndexedDB
   (RF-PER-5) and clear it completely on sign-out.
3. Service worker for offline: application shell caching, offline operation, and
   an explicit update flow — never a silently-stale application version, which in
   a governed client is a security issue.
4. Meet every RF-UI-3 accessibility requirement and run under the strict CSP from
   RF-API-3, with wasm permitted explicitly and nothing else relaxed.
5. Resolve the overlap with products/clients/ui (RF-UID-7): this is the shipping
   web client; that is either this, or a demo.
6. Add the missing package.json so the workspace actually builds it, and gate its
   bundle through RF-SC-5.
```

**Acceptance criteria**

- **AC-RF-CL-4a — The journey completes in a browser.** *Given* the web shell, *when* RF-UX-9 is performed, *then* it completes including offline operation.
- **AC-RF-CL-4b — Sealed and cleared.** *Given* local storage after use, *when* inspected, *then* data is sealed; *and* after sign-out, *then* it is gone.
- **AC-RF-CL-4c — Updates are not silent.** *Given* a new application version, *when* it is available, *then* the operator is informed and the update is explicit.
- **AC-RF-CL-4d — CSP and a11y hold.** *Given* the strict CSP and the accessibility suite, *when* run, *then* no violation occurs.
- **AC-RF-CL-4e — The workspace builds it.** *Given* `pnpm -r build`, *when* run, *then* the web shell builds.

**Done when:** the web shell ships as a gated artefact and completes the journey offline.

---

### RF-CL-5 · Desktop shell (Tauri: Windows, macOS, Linux)

**Severity:** Medium · **Area:** Clients · **Depends on:** RF-CL-1

> **Status at 2026-10-12 — Open.** `clients/shells/desktop-tauri/` holds only TOML declarations and an empty `audits/` directory, plus a helper script `clients/shells/tools/tauri-webview-axe.sh`; there is no Tauri project, no IPC allow-list and no installer packaging.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Land the Tauri 2 project linking the core natively with an allow-listed IPC surface and a test that only those commands are reachable
> - Implement per-platform custody (TPM/Hello, Keychain/Secure Enclave, Secret Service) with floor reporting
> - Deliver signed, verified, reversible updates through Bifrost rather than a third-party updater
> - Verify the journey with Narrator, VoiceOver and Orca and record audits
> - Build signed MSI, DMG and Linux packages through the release gate
>
> Checked: `clients/shells/desktop-tauri/manifest.toml:1` · `clients/shells/README.md:1` · `clients/shells/SHELL_BUILD.md:1`

**Prompt**

```
Build the Tauri desktop shell.

1. Tauri 2.x application linking the core natively, with an IPC surface that is
   minimal and allow-listed — the webview must not be able to invoke arbitrary
   core functions.
2. Key custody per platform: TPM/Windows Hello, macOS Keychain/Secure Enclave,
   Linux Secret Service — with the actual floor reported and a documented minimum.
3. Desktop integration: system tray, native notifications, file associations,
   auto-start policy, multi-window, and platform-appropriate menus.
4. Update: signed, verified updates delivered through Bifrost (RF-B-2) rather
   than a third-party updater, with rollback.
5. Accessibility: platform screen readers (Narrator, VoiceOver, Orca), keyboard
   navigation and high contrast, verified with a Tauri test harness.
6. Distribution: MSI, DMG and Linux packages, all signed and gated.
```

**Acceptance criteria**

- **AC-RF-CL-5a — The journey completes on three platforms.** *Given* Windows, macOS and Linux, *when* RF-UX-9 is performed, *then* it completes on each.
- **AC-RF-CL-5b — IPC is minimal.** *Given* the IPC surface, *when* inspected, *then* only the allow-listed commands are reachable from the webview.
- **AC-RF-CL-5c — Updates are verified and reversible.** *Given* an update, *when* delivered, *then* it is signature-verified before install and can be rolled back.
- **AC-RF-CL-5d — Custody floors are correct.** *Given* each platform, *when* custody is reported, *then* it matches the actual mechanism in use.
- **AC-RF-CL-5e — Accessibility is verified.** *Given* each platform's screen reader, *when* the journey is attempted, *then* it completes.

**Done when:** signed installers for all three platforms complete the journey and pass accessibility verification.

---

### RF-CL-6 · Linux GTK shell (mobile and desktop)

**Severity:** Medium · **Area:** Clients · **Depends on:** RF-CL-1

> **Status at 2026-10-12 — Open.** `clients/shells/linux-gtk/` holds only TOML declarations and an empty `audits/` directory, plus a helper `clients/shells/tools/atspi-walk.sh`; there is no GTK4/libadwaita code and no Flatpak manifest.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Land the GTK4/libadwaita application linking `midgard-client-core` with adaptive phone/desktop layout
> - Implement TPM / Secret Service custody with floor reporting and PAM/fprintd unlock verified by the core
> - Write the Flatpak manifest with a justified minimal permission set
> - Verify the journey under Orca / AT-SPI and record an audit
>
> Checked: `clients/shells/linux-gtk/manifest.toml:1` · `clients/shells/README.md:1` · `clients/shells/SHELL_BUILD.md:1`

**Prompt**

```
Build the GTK4 shell for Linux mobile and desktop.

1. GTK4/libadwaita application linking the core natively, with adaptive layout
   for phone and desktop form factors.
2. Key custody: TPM where present, Secret Service otherwise, with the floor
   reported; PAM/fprintd for biometric unlock with the assertion verified by the
   core.
3. Integration: desktop notifications, portals for file access, session lock
   handling, and correct behaviour under Wayland and X11.
4. Accessibility: Orca, AT-SPI correctness, keyboard navigation and high
   contrast, verified.
5. Distribution: Flatpak and distribution packages, signed and gated, with a
   documented sandbox permission set that is as narrow as the application allows.
```

**Acceptance criteria**

- **AC-RF-CL-6a — The journey completes on both form factors.** *Given* Linux mobile and desktop, *when* RF-UX-9 is performed, *then* it completes and the layout adapts.
- **AC-RF-CL-6b — Custody floor is correct.** *Given* TPM presence or absence, *when* reported, *then* it matches reality.
- **AC-RF-CL-6c — Orca works.** *Given* Orca, *when* the journey is attempted, *then* it completes with correct announcements.
- **AC-RF-CL-6d — Sandbox is narrow.** *Given* the Flatpak manifest, *when* reviewed, *then* permissions are the minimum the application requires and each is justified.

**Done when:** signed Flatpak and packages complete the journey with verified accessibility.

---

### RF-CL-7 · Browser extension (broker mode)

**Severity:** Medium · **Area:** Clients · **Depends on:** RF-CL-1, RF-CL-4

> **Status at 2026-10-12 — Open.** Unchanged from the plan's evidence: the Rust `clients/broker` crate and its tests (no key material, no-scrape and no-secret evals) exist, but `clients/shells/extension/` is two TOML files with no `package.json`, no Manifest V3 manifest, no content script and no wasm build.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Build the Manifest V3 extension (Chrome and Firefox) over a wasm build of the broker, with `package.json` so the workspace builds it
> - Add a test that the extension wasm build cannot construct a sealing root (KDD-2 through the wasm boundary)
> - Enforce the EV-AC3 no-scrape / no-secret guarantees in the shipped content script, tested against a page with credentials
> - Implement origin-verified typed messaging and a hostile-page rejection test
> - Document per-permission justification and audit brokered operations in the enclosure with the extension identity
>
> Checked: `clients/broker/tests/broker.rs:65` · `clients/broker/tests/broker.rs:80` · `clients/shells/extension/manifest.toml:1` · `pnpm-workspace.yaml:9`

**Evidence.** `clients/broker` (348 lines) implements the broker and the A.C3 in-page assistant prompt; `clients/keystore` models `BrokeredKeystore` such that the type system forbids it backing a local seal (KDD-2). `products/clients/ui/src/ExtensionPopup.tsx` is 76 lines of demonstration. `pnpm-workspace.yaml` lists `clients/shells/extension`, which has no `package.json`.

**Prompt**

```
Build the extension, preserving KDD-2 structurally.

1. Manifest V3 extension for Chrome and Firefox, holding NO key material and
   NO sealed data. Every privileged operation is brokered to the web shell or
   the enclosure. The existing type-level prohibition must be preserved through
   the wasm boundary — a test must show the extension build cannot construct a
   sealing root.
2. In-page assistant per A.C3: it must not scrape the page, must not read
   credentials or form fields, and must operate only on content the user
   explicitly selects and submits. Enforce the EV-AC3 guarantees (no scrape, no
   secret) in the shipped extension, not only in tests.
3. Permissions: the narrowest possible set, with activeTab rather than broad host
   permissions, justified individually in the store listing and in the repo.
4. Isolation: content script and page context strictly separated; no injected
   content can reach the broker; message passing is origin-verified and typed.
5. Distribution: Chrome Web Store and AMO packages, signed and gated, with the
   review-facing justification documented.
```

**Acceptance criteria**

- **AC-RF-CL-7a — No key material, structurally.** *Given* the extension build, *when* inspected and when a sealing operation is attempted, *then* no key material exists and the operation is impossible by construction.
- **AC-RF-CL-7b — No scraping, no secrets.** *Given* a page containing credentials and unselected content, *when* the assistant is used, *then* only explicitly selected content is submitted, and no credential or form value is read.
- **AC-RF-CL-7c — Permissions are minimal.** *Given* the manifest, *when* reviewed, *then* it requests the narrowest set and each permission has a documented justification.
- **AC-RF-CL-7d — Page cannot reach the broker.** *Given* a hostile page attempting to message the broker, *when* it tries, *then* the message is rejected on origin.
- **AC-RF-CL-7e — Brokered operations are audited.** *Given* a brokered privileged operation, *when* performed, *then* it is audited in the enclosure with the extension's identity.

**Done when:** both store packages ship, gated, with the KDD-2 structural test and the EV-AC3 guarantees enforced in the shipped build.

---

### RF-CL-8 · Client delivery, update and integrity across all shells

**Severity:** High · **Area:** Clients · **Depends on:** RF-B-2, RF-CL-2..7

> **Status at 2026-10-12 — Open.** Unchanged from the plan's evidence: `clients/delivery` (160 lines) verifies a signed client artefact or offline bundle before returning an install record and has library tests for tamper, wrong key and offline bundle import, but nothing delivers, launches, self-verifies at runtime or enforces a minimum version. No minimum-client-version check was found anywhere in the Rust tree, and the only client fleet view is mock data in `sdk/ts/src/products/clients.ts`.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: medium.
>
> **Still to do:**
> - Implement minimum-supported-version enforcement in the enclosure handshake with an actionable refusal and a test
> - Implement launch-time self-verification and tamper refusal in each shell (blocked on RF-CL-2..7)
> - Extend the offline bundle path to an end-to-end air-gapped install plus local enrolment test
> - Build a real client-fleet view (version, custody floor, last seen, revoke, require update) backed by a service, not SDK fixtures
> - Add per-store data-handling declarations and a release-time check that they match
>
> Checked: `clients/delivery/src/lib.rs:60` · `clients/delivery/src/lib.rs:99` · `clients/delivery/tests/install.rs:60` · `clients/delivery/tests/install.rs:113`

**Evidence.** `clients/delivery` (160 lines) models install records and verification; nothing delivers.

**Prompt**

```
Deliver and maintain clients as a first-class fleet.

1. Distribution per channel: App Store, Play, Chrome Web Store, AMO, Flatpak,
   MSI/DMG/deb/rpm, and direct enterprise/MDM — each with a signed artefact that
   the client verifies for itself at install and at every launch.
2. Update: enforce a minimum supported version (an out-of-date client with a
   known vulnerability must be refused by the enclosure, not merely warned), with
   a clear user path to update and a documented grace period.
3. Integrity at runtime: detect tampering and refuse to run; report the detection
   where the platform permits.
4. Air-gapped delivery: clients transit by Bifrost bundle to disconnected sites
   and install from local media with offline verification.
5. Fleet visibility: which clients are enrolled, on which versions, with which
   custody floors, last seen when — visible in the Admin console and actionable
   (revoke, require update).
6. Store compliance: privacy declarations, data-safety forms and review
   requirements per store, kept accurate — a store listing that misstates data
   handling is a compliance failure, not a marketing detail.
```

**Acceptance criteria**

- **AC-RF-CL-8a — Clients verify themselves.** *Given* a tampered client binary, *when* launched, *then* it refuses to run and reports where possible.
- **AC-RF-CL-8b — Old clients are refused.** *Given* a client below the minimum supported version, *when* it connects, *then* the enclosure refuses it with an actionable update instruction.
- **AC-RF-CL-8c — Air-gapped install works.** *Given* a disconnected site, *when* a client bundle is transported and installed, *then* it verifies offline and enrols locally.
- **AC-RF-CL-8d — The client fleet is visible.** *Given* the Admin console, *when* the client fleet is viewed, *then* every enrolled device shows version, custody floor and last-seen, and can be revoked or required to update.
- **AC-RF-CL-8e — Store declarations are accurate.** *Given* each store listing, *when* compared with actual data handling, *then* they agree, checked at each release.

**Done when:** every shell ships through its channel with self-verification, minimum-version enforcement and fleet visibility.

---

### RF-CL-9 · On-device inference and tier selection, made real

**Severity:** Medium · **Area:** Clients · **Depends on:** RF-MI-2, RF-CL-1

> **Status at 2026-10-12 — Partial.** The tier logic, model signature check and a real PQ-mTLS remote bridge (RT-57, `remote_bridge.rs`) exist and are tested, but there is still no on-device runtime: `OnDeviceRuntime` is only implemented by `MockOnDeviceRuntime`, and parity is asserted between two mocks that share one answer function. Tier context is supplied by the caller, not probed from a device.
> Criteria met 1 of 5 (e); open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Bind `OnDeviceRuntime` to a real quantised model runtime on at least one mobile and one desktop platform
> - Prove the no-egress refusal at the network layer (today it is asserted only on the tier decision) including the case where local capability is insufficient
> - Add an operator force-on-device control and surface tier plus reason to the shell
> - Re-run EV-AC-parity against a real on-device model and the real enclosure instead of the two mocks
> - Feed tier selection from real device capability, model availability and network probes; add resource and thermal limits
>
> Checked: `clients/inference/src/ondevice.rs:70` · `clients/inference/src/model.rs:39` · `clients/inference/tests/model_and_audit.rs:39` · `clients/inference/tests/tiering.rs:188` · `clients/inference/tests/rc_57.rs:120`

**Evidence.** `clients/inference` (1 205 lines) implements the A.C1 assistant, the A.C2 tier reasoner, the adapter, the model seam and the Evals including `EV-AC-parity`. There is no on-device runtime.

**Prompt**

```
Make on-device inference real, and make tier selection trustworthy.

1. On-device runtime per platform: a quantised small model executing locally
   (Core ML / NNAPI / GGUF-class CPU inference as appropriate), with the model
   delivered as a signed artefact through client delivery and verified before
   load.
2. Tier selection (A.C2) with real inputs: real device capability, real model
   availability, real network state, real data sensitivity, and real egress
   policy. Privacy-first: if the data may not leave the device, the answer is
   on-device or a refusal — never a remote call. Test the refusal path
   explicitly.
3. Transparency: the operator always sees which tier served their request and
   why, and can constrain it (force on-device) with the consequence explained.
4. Parity: enforce EV-AC-parity — governance behaviour (clearance filtering,
   citation requirements, refusal classes) must be identical whichever tier
   served the request. A weaker on-device model may give a less complete answer;
   it must never give a less governed one.
5. Resource discipline: bounded memory and battery impact, thermal awareness,
   and a documented behaviour when the device cannot sustain inference.
```

**Acceptance criteria**

- **AC-RF-CL-9a — On-device inference works offline.** *Given* an enrolled device with no connectivity, *when* a question is asked, *then* it is answered on-device with citations to local grounding.
- **AC-RF-CL-9b — Sensitive data never egresses.** *Given* data that may not leave the device, *when* a question requires more capability than is local, *then* the system refuses and explains — no remote call is made, verified at the network layer.
- **AC-RF-CL-9c — Tier is visible and constrainable.** *Given* any answer, *when* shown, *then* the serving tier and reason are visible; *and when* the operator forces on-device, *then* it is honoured.
- **AC-RF-CL-9d — Governance parity holds.** *Given* the same governed question on each tier, *when* answered, *then* clearance filtering, citation requirements and refusal classes are identical.
- **AC-RF-CL-9e — Unverified models never load.** *Given* an unsigned or tampered on-device model, *when* load is attempted, *then* it is refused.

**Done when:** on-device inference runs on at least one mobile and one desktop platform with parity and the egress-refusal path proven.

---
# Q — SDKs & developer experience

> **Status.** `sdk/ts` is 4 013 lines: a domain model, a transport interface, one real HTTP client for the core surface, and six product mocks. `sdk/README.md` describes the SDK set; `sdk/rust`, `sdk/python` and `sdk/go` do not exist.

---

### RF-SDK-1 · Generate the TypeScript SDK from the contract and complete it

**Severity:** Medium · **Area:** SDK · **Depends on:** RF-API-1, RF-UID-1

> **Status at 2026-10-12 — Open.** `sdk/ts` is still hand-written (no `src/gen`, `types.ts` is 212 hand-written lines) with a `private` 0.0.0 package, no test script and no examples. The only addition since the plan is `http/login.ts`; `HttpGateway` still takes a client-supplied `PolicyContext`, maps every 403 to `HumanGateRequiredError`, polls for audit instead of streaming, and the stale 'stub' / 'only implementation is MockGateway' comments are still there.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Generate types and clients from `contracts/proto` or add a conformance test that fails on drift from `types.ts`
> - Add token refresh and typed auth / retryable errors; stop treating every 403 as a human gate and carry the approval requirements in a typed outcome
> - Remove the client-supplied policy context from `HttpGatewayOptions`
> - Replace polling `auditStream` with a resumable subscription; add pagination, cancellation and deadlines
> - Fix the stale comments in `index.ts` and `client.ts`, version and package the SDK (ESM/CJS), add tests and compiled examples
>
> Checked: `sdk/ts/src/index.ts:4` · `sdk/ts/src/client.ts:26` · `sdk/ts/src/http/HttpGateway.ts:41` · `sdk/ts/src/http/HttpGateway.ts:84` · `sdk/ts/package.json:5`

**Prompt**

```
Make the TypeScript SDK generated, complete and trustworthy.

1. Generate the domain types and clients from the proto contract; retire the
   hand-written types in sdk/ts/src/types.ts or prove equivalence with a
   conformance test that fails on drift.
2. Complete the transport: authentication and session handling (RF-IAM-2),
   automatic token refresh, typed errors with retryability, human-gate handling
   as a first-class typed outcome, streaming subscriptions with resume,
   pagination helpers, cancellation, and deadline propagation.
3. Remove the client-supplied policy context (RF-IAM-3).
4. Package properly: versioned, changelog'd, ESM and CJS, tree-shakeable, typed
   exports, and a documented compatibility matrix against server versions.
5. Test: unit tests, contract tests against a real server, and a compiled example
   per method (RF-API-5).
6. Correct the stale documentation in sdk/ts/src/index.ts and client.ts, which
   currently describe HttpGateway as a stub and MockGateway as the only
   implementation (RF-DOC-1).
```

**Acceptance criteria**

- **AC-RF-SDK-1a — Types are generated or proven equivalent.** *Given* a proto change, *when* the SDK is built, *then* types update or the conformance test fails.
- **AC-RF-SDK-1b — Auth is handled.** *Given* an expiring token, *when* an SDK call is made, *then* it refreshes transparently, and a revoked session surfaces a typed authentication error.
- **AC-RF-SDK-1c — Gates are first-class.** *Given* a gated call without a token, *when* made, *then* the SDK surfaces a typed human-gate outcome carrying the approval requirements — not a generic error.
- **AC-RF-SDK-1d — Streaming resumes.** *Given* a dropped subscription, *when* it reconnects, *then* it resumes without gaps or duplicates.
- **AC-RF-SDK-1e — Examples compile and run.** *Given* the examples, *when* CI runs, *then* each compiles and executes against a test enclosure.

**Done when:** the SDK is generated, complete, versioned and documented, with no stale comments.

---

### RF-SDK-2 · Rust SDK

**Severity:** Medium · **Area:** SDK · **Depends on:** RF-API-2

> **Status at 2026-10-12 — Open.** There is no `sdk/rust` directory and no generated Rust client; `sdk/` contains only `ts/` and a README. `contracts/buf.gen.yaml` names a prost/tonic output under `services/gen/rust` that does not exist in the tree.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Create the Rust SDK crate with clients generated from the contract over the PQ-mTLS RPC layer
> - Require a workload identity before any request is sent and test the fail-closed path
> - Add compiled examples and `#![deny(missing_docs)]`
> - Document MSRV and the server compatibility matrix and gate the crate
>
> Checked: `sdk/README.md:1` · `contracts/buf.gen.yaml:13`

**Prompt**

```
Publish a first-class Rust SDK — the natural client for services, tools and
extensions inside the enclosure.

Generate typed clients from the contract over the PQ-mTLS RPC layer, with
workload-identity authentication, deadline propagation, typed errors, streaming,
and idempotency-key support. Provide both a low-level generated client and an
ergonomic wrapper. Ship compiled examples, doc comments on every public item, and
a documented MSRV and compatibility matrix. Gate the crate through RF-SC-5.
```

**Acceptance criteria**

- **AC-RF-SDK-2a — Typed and complete.** *Given* the contract, *when* the SDK is generated, *then* every service and method is available with typed requests, responses and errors.
- **AC-RF-SDK-2b — Identity is required.** *Given* a client without a workload identity, *when* it calls, *then* it fails closed before any request is sent.
- **AC-RF-SDK-2c — Examples compile.** *Given* the examples, *when* CI runs, *then* all compile and run.
- **AC-RF-SDK-2d — Documented surface.** *Given* the crate, *when* docs are built, *then* every public item is documented and the build fails on a missing doc.

**Done when:** the Rust SDK ships gated, documented and exercised by examples.

---

### RF-SDK-3 · Python SDK

**Severity:** Medium · **Area:** SDK · **Depends on:** RF-API-3

> **Status at 2026-10-12 — Partial.** The root `pyproject.toml` the plan cites has been removed, so the dishonest workspace declaration is gone, but no Python SDK exists. Stale Python references remain: `just build-py` still runs `uv sync --frozen`, and `sdk/README.md` and `contracts/buf.gen.yaml` still describe an `sdk/py` that is not there.
> Criteria met 1 of 5 (e); open: a, b, c, d. Confidence: high.
>
> **Scope change:** The root `pyproject.toml` is gone, so criterion e is satisfied. The stale promises are now `just build-py`, `sdk/README.md` and `contracts/buf.gen.yaml`, which still describe an `sdk/py` that does not exist.
>
> **Still to do:**
> - Build the typed Python client over the HTTP edge with runtime validation and typed exceptions
> - Add dataframe conversion that surfaces truncation and a notebook governance-context display
> - Package a vendored wheel set and test offline install
> - Remove or make real the `build-py` recipe and the `sdk/py` claims in `sdk/README.md` and `buf.gen.yaml`
>
> Checked: `justfile:34` · `sdk/README.md:8` · `contracts/buf.gen.yaml:9`

**Evidence.** `pyproject.toml` declares a uv workspace whose members are `mimir/*` — which are Rust crates. `just build-py` runs `uv sync --frozen` against it. Either the Python workspace becomes real or the declaration is corrected (RF-DOC-1).

**Prompt**

```
Ship the Python SDK — the language analysts and data engineers will actually
reach for.

Generate a typed client (with type stubs and runtime validation) over the HTTP
edge, with session authentication, typed exceptions, streaming iterators,
pagination, and human-gate handling. Provide notebook-friendly ergonomics:
dataframe conversion for query results (respecting clearance filtering and
truncation markers — a truncated result must not silently become a complete-
looking dataframe), and a clear governance-context display so an analyst always
knows what clearance and purpose their session carries. Package for offline
installation from a vendored wheel set. Fix or remove the pyproject workspace
declaration.
```

**Acceptance criteria**

- **AC-RF-SDK-3a — Typed and validated.** *Given* the SDK, *when* used with a type checker, *then* it type-checks; *and when* a response violates the contract, *then* it raises rather than silently coercing.
- **AC-RF-SDK-3b — Truncation is not hidden.** *Given* a truncated query result, *when* converted to a dataframe, *then* the truncation is surfaced and cannot be mistaken for completeness.
- **AC-RF-SDK-3c — Governance context is visible.** *Given* a session, *when* displayed in a notebook, *then* subject, clearance and purpose are shown.
- **AC-RF-SDK-3d — Offline install works.** *Given* the vendored wheels and no network, *when* installed, *then* it works.
- **AC-RF-SDK-3e — The workspace declaration is honest.** *Given* `pyproject.toml`, *when* inspected, *then* its members are real Python packages or the declaration is removed.

**Done when:** the Python SDK installs offline, type-checks, and surfaces governance context and truncation honestly.

---

### RF-SDK-4 · Go SDK, or an honest removal

**Severity:** Low · **Area:** SDK · **Depends on:** RF-API-3

> **Status at 2026-10-12 — Partial.** `go.work` has been deleted and there are no `.go` files, so the tree has effectively taken the removal option, but the removal is incomplete and unrecorded. Criterion b does not apply because no Go SDK was shipped. Go references remain in the justfile (including a comment saying go.work exists), `products/README.md`, `flake.nix` and `contracts/buf.gen.yaml`, and no ADR records the decision.
> Criteria met 0 of 3; open: a, c; not verifiable by reading: b. Confidence: high.
>
> **Scope change:** `go.work` is deleted and there are no `.go` files. Finish the removal: the `build-go` recipe and its comment in the `justfile`, `products/README.md`, `flake.nix`, the Go plugins in `contracts/buf.gen.yaml`, and the ADR.
>
> **Still to do:**
> - Remove `build-go` from the justfile `build` recipe and the `go test` line and its stale comment from `test`
> - Correct `products/README.md` rows that say Bifrost and Admin backends are Go
> - Remove the Go plugins from `contracts/buf.gen.yaml`, the Go toolchain from `flake.nix` and the Go block from `.gitignore`
> - Fix README line describing `just build` as building Go/TS/Python members
> - Record the removal decision in an ADR
>
> Checked: `justfile:28` · `justfile:202` · `products/README.md:10` · `flake.nix:47` · `contracts/buf.gen.yaml:17`

**Evidence.** `go.work` declares a Go workspace with no modules and a comment promising "compute-scheduler, Bifrost controller, admin backend"; `products/README.md` states the Bifrost and admin backends are Go. All of them are Rust. `just build-go` prints "no go modules yet".

**Prompt**

```
Resolve the Go question definitively.

Either (a) ship a Go SDK generated from the contract, with the same feature set
as the others, and keep go.work; or (b) remove go.work, remove the Go build
target from the justfile and CI, and correct products/README.md and every other
document asserting Go backends. Do not leave a workspace declaring intentions
the tree contradicts. Record the decision in an ADR.
```

**Acceptance criteria**

- **AC-RF-SDK-4a — The tree and the documents agree.** *Given* the repository, *when* Go is mentioned anywhere, *then* the claim is true.
- **AC-RF-SDK-4b — If shipped, it is complete.** *Given* a Go SDK, *when* compared with the TypeScript SDK's feature set, *then* it is equivalent and its examples compile.
- **AC-RF-SDK-4c — If removed, nothing dangles.** *Given* removal, *when* the build runs, *then* no Go target, workspace file or documentation reference remains.

**Done when:** the decision is recorded and the tree is consistent with it.

---

### RF-SDK-5 · Developer environment, samples and extension templates

**Severity:** Medium · **Area:** SDK · **Depends on:** RF-DEP-1, RF-UX-12

> **Status at 2026-10-12 — Partial.** A one-command local enclosure exists (`just compose-up`, torn down by `compose-down`; `deploy/compose/smoke.sh` exercises five real services on throwaway volumes) and `CONTRIBUTING.md` covers provenance tags and the offline build. There are no sample applications, no extension templates for any of the four extension points and no CI definition in the public tree.
> Criteria met 0 of 4; open: b, c, d; not verifiable by reading: a. Confidence: medium.
>
> **Still to do:**
> - Write one sample application per SDK exercising auth, purpose, clearance filtering, a human gate, a refusal and audit read-back
> - Create the four extension templates (Forge connector, object action function, Heimdall feed adapter, agent tool) with bypass-attempt tests
> - Demonstrate `compose-up` on a clean machine with no network and document reset
> - Extend `CONTRIBUTING.md` with the extension invariants and how to run the acceptance suite locally
> - Publish a CI definition that compiles and runs every sample and template
>
> Checked: `justfile:51` · `deploy/compose/compose.yaml:35` · `deploy/compose/smoke.sh:13` · `CONTRIBUTING.md:18`

**Prompt**

```
Make it easy to build on Midgard, offline.

1. One-command local enclosure with representative seeded data, running every
   service, with documented reset and teardown — and it must work with no
   network.
2. Sample applications, one per SDK, demonstrating the governance model
   end to end: authenticate, select a purpose, query under clearance, handle a
   human gate, handle a refusal, read your own audit entries.
3. Extension templates for the four extension points — Forge connector, object
   action function, Heimdall feed adapter, agent tool — each inheriting policy,
   audit, marking and sandboxing by construction, each with tests and each
   shipping through the supply-chain gate.
4. A contributor guide covering the invariants an extension must not violate, the
   provenance-tag requirement, and how to run the acceptance suite locally.
5. Keep every sample compiled and run in CI so none can rot.
```

**Acceptance criteria**

- **AC-RF-SDK-5a — One command, offline.** *Given* a clean machine, *when* the documented command runs with no network, *then* a full enclosure with seeded data is available.
- **AC-RF-SDK-5b — Samples demonstrate governance.** *Given* each sample, *when* run, *then* it exercises authentication, purpose, clearance filtering, a human gate, a refusal and audit read-back.
- **AC-RF-SDK-5c — Templates inherit controls.** *Given* each template, *when* an attempt is made to bypass policy, audit or sandboxing, *then* it fails.
- **AC-RF-SDK-5d — Nothing rots.** *Given* CI, *when* it runs, *then* every sample and template compiles and passes its tests.

**Done when:** the local enclosure, samples and four templates exist and are CI-verified.

---

# R — Deployment & topology

> **Status.** `deploy/` contains an acceptance-toolchain Dockerfile, an air-gapped acceptance runner, eight verification scripts, and a README promising `helm/` and `air-gap-bundle/` directories that do not exist. There is no container image for any service, no orchestration manifest, no installer, and no documented topology.

---

### RF-DEP-1 · Container images for every service

**Severity:** Critical · **Area:** Deployment · **Depends on:** RF-RT-1 · **Blocks:** RF-DEP-2..7

> **Status at 2026-10-12 — Partial.** The workstream status is stale: `deploy/images/service.Dockerfile` plus `build.sh` build one verify-before-run image per service from gate-verified binaries with no network, running as uid 10001, and `deploy/compose` runs all five services with a smoke script. The base is the full `rust:1.82` image pinned by digest (it has a shell and package manager), there is no multi-arch build, no image-level SBOM or digest reproducibility check, the release has never run (no pinned trust keys, no CI), and compose builds a different unsigned image from source rather than using the release images.
> Criteria met 0 of 5; open: a, b, c, d; not verifiable by reading: e. Confidence: high.
>
> **Scope change:** Images, compose, a Helm chart and a kind cluster path exist. `deploy/README.md` still says parts of this are not present.
>
> **Still to do:**
> - Switch the runtime base to distroless or scratch with a static binary (the Dockerfile itself notes this is pending)
> - Run the release gate once so signed images exist; it needs the key ceremony and a runner
> - Add arm64 and an image-digest reproducibility check
> - Produce an SBOM for the whole image including OS components
> - Make compose consume the release images instead of `deploy/compose/Dockerfile`
>
> Checked: `deploy/images/service.Dockerfile:21` · `deploy/images/service.Dockerfile:43` · `deploy/images/build.sh:75` · `deploy/compose/Dockerfile:1` · `deploy/trust/README.md:26`

**Prompt**

```
Build reproducible, minimal, signed images for every service.

1. One image per service binary: distroless or scratch base, non-root user, no
   shell, no package manager, read-only root filesystem, minimal capabilities,
   and a documented, minimal writable volume set.
2. Reproducible: fixed created-time, deterministic layer ordering, no build
   timestamps, pinned base digests. `mg-repro` must confirm image digests are
   identical across builds (RF-SC-3).
3. Multi-architecture: amd64 and arm64, both reproducible.
4. Gated: each image passes reproducible build, SBOM (including OS-level
   components, not only Rust crates), scan, ML-DSA signature and verification
   (RF-SC-5). The image SBOM must describe the whole image, not just the app.
5. Runtime provenance: each image records its build digest and signing key id and
   exposes them on its health surface (RF-OBS-5).
6. Local composition: a compose-style single-node topology for development and
   small deployments, using the same images as production.
```

**Acceptance criteria**

- **AC-RF-DEP-1a — Every service has a gated image.** *Given* the service set, *when* the release runs, *then* each has a reproducible, SBOM'd, scanned, signed and verified image for both architectures.
- **AC-RF-DEP-1b — Images are minimal and non-root.** *Given* any image, *when* inspected, *then* it has no shell or package manager, runs as non-root, and mounts its root filesystem read-only.
- **AC-RF-DEP-1c — Digests are stable.** *Given* two independent builds of the same revision, *when* compared, *then* the image digests are identical.
- **AC-RF-DEP-1d — SBOM covers the image.** *Given* an image SBOM, *when* inspected, *then* it lists OS-level components as well as application dependencies.
- **AC-RF-DEP-1e — Single-node topology runs.** *Given* the compose topology, *when* started, *then* the full enclosure is operable locally.

**Done when:** every service ships a gated, reproducible, minimal image and the single-node topology runs from them.

---

### RF-DEP-2 · Kubernetes deployment: charts, policies and the HA topology

**Severity:** Critical · **Area:** Deployment · **Depends on:** RF-DEP-1, RF-DEP-3

> **Status at 2026-10-12 — Partial.** A Helm chart exists at `deploy/helm/midgard` and has been run on a three-node kind cluster: one single-replica StatefulSet per service with non-root, read-only root, dropped capabilities, seccomp, resource limits, readiness and liveness probes, PVCs and secrets provisioned outside the chart. There is one release values file plus a kind override, no HA topology (services hold a single-writer locked state directory), no PodDisruptionBudget, anti-affinity, startup probe or priority class, no policy tests, no chart signing and no CI.
> Criteria met 0 of 5; open: a, b, c; not verifiable by reading: d, e. Confidence: high.
>
> **Scope change:** High availability, anti-affinity and zero-downtime cannot be chart tasks yet: every service holds a locked single-writer state directory and the chart pins one replica by design. The replication decision in RF-CA-9 comes first.
>
> **Still to do:**
> - Rewrite the HA and zero-downtime requirements: every service is single-writer, so replicas need a replication design (RF-RES-3) before the chart can offer them
> - Add values files for the single-node and air-gapped topologies and install each on a cluster
> - Add policy tests over the rendered manifests and a secret scan of chart and values
> - Add PodDisruptionBudgets and startup probes
> - Sign and gate the chart, and create a runner for lint, render check and dry-run install
>
> Checked: `deploy/helm/midgard/templates/workload.yaml:47` · `deploy/helm/midgard/templates/workload.yaml:9` · `deploy/helm/midgard/values.yaml:36` · `deploy/k8s/render.sh:6` · `deploy/acceptance/deployment/README.md:23`

**Prompt**

```
Create the deployment artefacts deploy/README.md already promises.

1. Helm charts (or equivalent) for the full suite with values for the three
   documented topologies: single-node, HA cluster, and air-gapped. Charts are
   versioned, signed and gated like any other artefact.
2. Per-workload hardening: security contexts (non-root, read-only root,
   dropped capabilities, seccomp), resource requests and limits derived from the
   measured envelopes, pod disruption budgets, anti-affinity for HA, topology
   spread, priority classes, and liveness/readiness/startup probes wired to
   RF-OBS-5.
3. Zero-downtime upgrade: rolling strategies consistent with services/scheduler's
   min-available guarantee (AC-M-7), with the same behaviour proven in a real
   cluster, not only in the state machine's tests.
4. Storage: persistent volume claims sized from RF-PER-8's model, with storage
   class guidance and a documented backup interaction (RF-RES-1).
5. Secrets: no secret in a chart or a values file; integrate with the KMS-backed
   mechanism (RF-B-6).
6. Air-gapped variant: no image pulls from outside, no chart repository access,
   installable from a local bundle only.
7. Validate: lint, policy-test (a policy engine asserting the hardening rules),
   and a dry-run install in CI.
```

**Acceptance criteria**

- **AC-RF-DEP-2a — Three topologies install.** *Given* each topology's values, *when* installed on a cluster, *then* the enclosure becomes healthy and operable.
- **AC-RF-DEP-2b — Hardening is enforced by policy.** *Given* the policy tests, *when* run against the rendered manifests, *then* every workload satisfies the hardening rules; a violation fails CI.
- **AC-RF-DEP-2c — Zero-downtime upgrade in a real cluster.** *Given* a running deployment under load, *when* upgraded, *then* availability never drops below the configured minimum and no request fails.
- **AC-RF-DEP-2d — No secrets in charts.** *Given* the charts and values, *when* scanned, *then* no secret is present.
- **AC-RF-DEP-2e — Air-gapped install works.** *Given* a cluster with no external registry access, *when* installed from a local bundle, *then* it succeeds.

**Done when:** charts exist, are gated, install all three topologies, and pass the hardening policy tests.

---

### RF-DEP-3 · Network policy, service mesh interaction and default-deny

**Severity:** Critical · **Area:** Deployment · **Depends on:** RF-DEP-2, RF-API-2

> **Status at 2026-10-12 — Partial.** The chart ships a namespace-wide default-deny NetworkPolicy for ingress and egress (DNS only), per-port ingress policies naming the allowed client pods, per-service egress policies, and an egress-broker exception limited to declared upstream ports. The kind smoke test checks one blocked path (an unlabelled pod cannot reach mimir-server). The policies come from hand-written `from` and `talksTo` lists in `values.yaml`, separate from each service's `policy.json`; no all-pairs or egress test exists and no mesh interaction is documented.
> Criteria met 0 of 4; open: a, b, c; not verifiable by reading: d. Confidence: medium.
>
> **Scope change:** There is no RF-API-2 allow-matrix to generate policy from; the chart's `from` / `talksTo` lists are the source. The egress broker is an HAProxy sidecar per Forge pod, not one broker service, and compose has none.
>
> **Still to do:**
> - Extend the cluster test to every service pair not allowed and to outbound attempts from every pod
> - Derive the chart's network lists and the services' policy files from one allow-matrix source
> - Restrict cross-namespace reachability to the edge and document ingress
> - Document PQ-mTLS behaviour under a service mesh and test that no intermediary terminates it
>
> Checked: `deploy/helm/midgard/templates/enclosure.yaml:21` · `deploy/helm/midgard/values.yaml:60` · `deploy/kind/smoke.sh:11` · `deploy/helm/midgard/templates/enclosure.yaml:79`

**Prompt**

```
Enforce the zero-trust posture at the network layer, not only in the application.

1. Default-deny network policies: no pod may reach another except as explicitly
   allowed by the RF-API-2 allow-matrix, generated from it so the two cannot
   diverge. Egress is denied by default; the egress broker (RF-API-6) is the sole
   exception and its destination allow-list is explicit.
2. Ingress: only the gateway and the HTTP edge are reachable from outside, with
   everything else unreachable even from within the cluster's other namespaces.
3. PQ-mTLS interaction: the application performs its own mutual authentication;
   document precisely how that interacts with any service mesh present, and
   ensure a mesh cannot terminate or downgrade the PQ handshake. Where a mesh is
   present, it must be transparent to the PQ-mTLS layer or absent from those
   paths.
4. DNS and service discovery hardening, and protection against pod-to-pod
   spoofing.
5. Verify: automated tests in a real cluster asserting that every disallowed path
   is actually blocked, and that egress from any pod other than the broker fails.
```

**Acceptance criteria**

- **AC-RF-DEP-3a — Disallowed paths are blocked.** *Given* the deployed policies, *when* every service pair not in the allow-matrix is tested, *then* each connection fails.
- **AC-RF-DEP-3b — Egress is denied except through the broker.** *Given* any pod other than the broker, *when* it attempts an outbound connection, *then* it fails.
- **AC-RF-DEP-3c — Policies derive from the matrix.** *Given* a change to the allow-matrix, *when* manifests are rendered, *then* the network policies change correspondingly, with no hand-maintained duplicate.
- **AC-RF-DEP-3d — PQ handshake is not downgraded.** *Given* a mesh in the path, *when* a service-to-service call is made, *then* the PQ-mTLS handshake completes end to end and no intermediary terminates it.

**Done when:** generated default-deny policies are deployed and the blocked-path test suite is green in a real cluster.

---

### RF-DEP-4 · Installer and upgrade tooling

**Severity:** High · **Area:** Deployment · **Depends on:** RF-DEP-2, RF-SC-2

> **Status at 2026-10-12 — Partial.** There is no `mg-install`, `mg-upgrade` or `mg-uninstall`. `deploy/air-gap-bundle/install.sh` is the only installer: it verifies the bundle offline with `mg-bundle import`, builds the images, runs `mg-verify` inside each, and runs `helm upgrade --install`, stopping at the first failure. It has no preflight, no posture report, no backup, migration, rollback or uninstall, and it is not covered by an automated test.
> Criteria met 0 of 5; open: a, c, e; not verifiable by reading: b, d. Confidence: medium.
>
> **Scope change:** The installer that exists is `deploy/air-gap-bundle/install.sh` with `mg-bundle`, not `mg-install`. Rewrite around extending it.
>
> **Still to do:**
> - Add preflight checks and a post-install posture report to the installer
> - Build the upgrade path: compatibility check, verified backup first, staged rollout, tested rollback
> - Build uninstall with an explicit retain-or-destroy choice and key-destruction evidence
> - Add an automated test that a tampered artefact aborts install before any change and that a second run is a no-op
>
> Checked: `deploy/air-gap-bundle/install.sh:77` · `deploy/air-gap-bundle/install.sh:13` · `tools/mg-bundle/tests/cli.rs:45` · `deploy/images/build.sh:75`

**Prompt**

```
Make installing and upgrading an enclosure a supported, verifiable procedure.

1. `mg-install`: preflight checks (platform, resources, storage, entropy, clock,
   connectivity expectations), signature verification of every artefact it is
   about to install, guided configuration, install, and post-install
   verification that reports the enclosure's posture.
2. `mg-upgrade`: version compatibility checking, a pre-upgrade backup
   (RF-RES-1), schema migration with progress and the ability to abort safely,
   staged upgrade using the ring mechanics, post-upgrade verification, and a
   documented, tested rollback.
3. `mg-uninstall`: complete, verifiable removal with an explicit decision about
   data — destroy or retain — and cryptographic assurance for the destroy path.
4. Idempotency: running install or upgrade twice is safe.
5. All three work offline, and all three refuse to proceed on any verification
   failure.
```

**Acceptance criteria**

- **AC-RF-DEP-4a — Install from media is verified.** *Given* signed artefacts and no network, *when* `mg-install` runs, *then* it verifies everything before installing and reports posture afterwards.
- **AC-RF-DEP-4b — Unverified artefacts stop the install.** *Given* any artefact failing verification, *when* install runs, *then* it aborts before changing anything.
- **AC-RF-DEP-4c — Upgrade backs up and can roll back.** *Given* an upgrade, *when* it runs, *then* a verified backup precedes it and rollback restores the prior version and data.
- **AC-RF-DEP-4d — Idempotent.** *Given* a second run of install or upgrade, *when* executed, *then* the outcome is unchanged and no error occurs.
- **AC-RF-DEP-4e — Destroy is cryptographic.** *Given* uninstall-with-destroy, *when* it completes, *then* data is unrecoverable by key destruction, evidenced.

**Done when:** install, upgrade and uninstall are implemented, offline-capable, and exercised including rollback.

---

### RF-DEP-5 · Air-gapped installation bundle

**Severity:** High · **Area:** Deployment · **Depends on:** RF-DEP-4, RF-B-4

> **Status at 2026-10-12 — Partial.** `deploy/air-gap-bundle/` exists (the deploy README still says it does not): `mg-bundle` packs gate-verified binaries with their signatures and SBOMs into one ML-DSA-signed, chunked `.mgbundle`, imports it offline verifying every artefact, and models travel as their own signed bundle. The bundle carries binaries only: images are rebuilt on site from a base image the site must already have, and the chart, documentation, runbooks, advisory database and trust anchors come from a repository checkout. There is no completeness test, no isolated install run and no differential bundle.
> Criteria met 0 of 5; open: a, b, d, e; not verifiable by reading: c. Confidence: medium.
>
> **Scope change:** `deploy/air-gap-bundle/` exists (README, `produce.sh`, `install.sh`).
>
> **Still to do:**
> - Add the chart, base image, docs and runbooks, advisory snapshot and trust anchors to the bundle so a site needs no checkout
> - Write a bundle specification and a completeness check that fails the build naming what is missing
> - Run an install from the bundle alone in a network-isolated environment followed by the acceptance suite
> - Implement differential bundles against a stated baseline
> - Fix the stale 'Not yet present' line in `deploy/README.md`
>
> Checked: `deploy/air-gap-bundle/README.md:21` · `deploy/air-gap-bundle/install.sh:77` · `tools/mg-bundle/tests/cli.rs:45` · `deploy/README.md:46` · `supply-chain/release-artefacts.txt:15`

**Prompt**

```
Produce the complete offline installation bundle deploy/README.md promises.

1. `deploy/air-gap-bundle/`: a specification and build for a bundle containing
   every image, chart, binary, model artefact, vendored dependency, advisory
   database snapshot, documentation set, runbook and trust anchor needed to
   install and operate an enclosure with no network, ever.
2. Completeness is verified, not asserted: an automated test installs from the
   bundle in a network-isolated environment and runs the full acceptance suite.
   Anything missing fails the bundle build.
3. Media handling: size reporting, chunking for constrained media, per-chunk
   integrity, and a documented transport procedure.
4. Verification before use: every component is signature-verified at the
   receiving site before installation.
5. Update bundles: differential against a stated installed baseline (RF-B-4).
6. Documentation inside the bundle: the operator at a disconnected site has no
   internet, so every runbook, reference and troubleshooting guide must be in the
   bundle and usable offline.
```

**Acceptance criteria**

- **AC-RF-DEP-5a — Completeness is proven.** *Given* a network-isolated environment, *when* an enclosure is installed from the bundle alone, *then* the full acceptance suite passes.
- **AC-RF-DEP-5b — Missing components fail the build.** *Given* a deliberately incomplete bundle, *when* the bundle test runs, *then* it fails and names what is missing.
- **AC-RF-DEP-5c — Every component verifies.** *Given* the bundle at the receiving site, *when* installation begins, *then* every component's signature is verified and any failure aborts.
- **AC-RF-DEP-5d — Documentation is inside.** *Given* the bundle, *when* opened offline, *then* the complete documentation and runbook set is present and usable.
- **AC-RF-DEP-5e — Differential updates apply.** *Given* an installed baseline, *when* a differential bundle is applied, *then* the result equals a full install of the target version.

**Done when:** the bundle exists, its completeness test passes in isolation, and it carries its own documentation.

---

### RF-DEP-6 · Environment strategy and configuration promotion

**Severity:** Medium · **Area:** Deployment · **Depends on:** RF-DEP-2, RF-AD-3

> **Status at 2026-10-12 — Open.** No environment strategy, promotion flow, digest-equality check or ephemeral environment tooling exists. The only environments in the tree are local compose and kind, both built from source, and a deployment-tier script that expects an unspecified staging cluster.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Document the environments with purpose, data policy and access model
> - Add a promotion check that compares artefact digests (`images.lock`) across environments and refuses a rebuild
> - Define per-environment approval requirements for configuration promotion
> - Add ephemeral review environments with synthetic data and automatic teardown
>
> Checked: `deploy/README.md:19` · `deploy/acceptance/deployment/README.md:8` · `deploy/images/build.sh:19`

**Prompt**

```
Define how a change reaches production safely.

1. Environments: development, test, staging and production, with a documented
   purpose, data policy (production data never in lower environments without
   documented, audited de-identification) and access model for each.
2. Promotion: the same artefact digest moves between environments; only
   configuration differs. Rebuilding for an environment is prohibited and the
   prohibition is enforced by comparing digests.
3. Configuration promotion follows the RF-AD-3 GitOps flow with per-environment
   approval requirements that increase toward production.
4. Ephemeral environments for review, created and destroyed automatically, with
   synthetic data only and no access to production secrets.
5. Parity: document known differences between staging and production and keep the
   list short and reviewed — unknown differences are the cause of most
   deployment surprises.
```

**Acceptance criteria**

- **AC-RF-DEP-6a — Same digest promotes.** *Given* a promoted artefact, *when* its digest is compared across environments, *then* it is identical; a rebuild is detected and refused.
- **AC-RF-DEP-6b — Production data stays put.** *Given* a lower environment, *when* its data is inspected, *then* it contains no production data absent a documented, audited de-identification.
- **AC-RF-DEP-6c — Approvals increase toward production.** *Given* the promotion flow, *when* a change moves toward production, *then* the approval requirements strengthen as configured.
- **AC-RF-DEP-6d — Ephemeral environments are safe and clean up.** *Given* a review environment, *when* it is created and later destroyed, *then* it used only synthetic data and left nothing behind.

**Done when:** the environment strategy is documented and enforced, including the same-digest check.

---

### RF-DEP-7 · Capacity, sizing and deployment planning guidance

**Severity:** Medium · **Area:** Deployment · **Depends on:** RF-SCL-4, RF-OBS-1

> **Status at 2026-10-12 — Open.** No sizing guidance, calculator, bottleneck list or reference architecture exists. The chart carries untuned default resource requests and a namespace quota, and the README records one informal Granite-on-CPU timing.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Benchmark each service and publish sizing per topology from the results
> - Write the scaling and bottleneck document (single-writer state directories and whole-picture re-seal are known limits)
> - Build the sizing calculator
> - Define and validate reference architectures
>
> Checked: `deploy/helm/midgard/values.yaml:22` · `README.md:546` · `README.md:566`

**Prompt**

```
Tell operators what hardware they need.

1. Publish sizing guidance from measured envelopes: CPU, memory, storage, GPU and
   network per service, at several workload scales, for each topology.
2. Provide a sizing calculator taking workload parameters (objects, links, users,
   feeds, pipeline volume, inference rate) and producing a recommended topology.
3. Document scaling characteristics honestly: what scales horizontally, what does
   not, and where the known bottlenecks are. An honest bottleneck list is more
   valuable than an optimistic one.
4. Provide reference architectures for the realistic deployment shapes: small
   single-site, multi-site with sync, air-gapped tactical, and large central.
5. Keep it current: regenerate the guidance from benchmark results each release
   so it cannot drift from reality.
```

**Acceptance criteria**

- **AC-RF-DEP-7a — Guidance is measured.** *Given* the sizing guidance, *when* compared with benchmark results, *then* it derives from them and is regenerated each release.
- **AC-RF-DEP-7b — The calculator is accurate.** *Given* a workload deployed per the calculator's recommendation, *when* it runs at that workload, *then* it meets its SLOs.
- **AC-RF-DEP-7c — Bottlenecks are documented.** *Given* the scaling documentation, *when* read, *then* it names what does not scale horizontally and where the limits are.
- **AC-RF-DEP-7d — Reference architectures are validated.** *Given* each reference architecture, *when* deployed, *then* it passes the acceptance suite.

**Done when:** sizing guidance is generated from benchmarks and each reference architecture is validated.

---

# S — Resilience, backup & disaster recovery

> **Why Critical.** With no persistence there is nothing to back up; with persistence (Workstream C) and no backup, a single failure is a total, unrecoverable loss of the audit ledger — which is the platform's primary evidentiary artefact.

---

### RF-RES-1 · Backup: complete, verified, restorable

**Severity:** Critical · **Area:** Resilience · **Depends on:** RF-PER-1

> **Status at 2026-10-12 — Partial.** `docs/ops/BACKUP_RESTORE.md` documents a manual file-copy backup of the KMS-sealed journals and a copy-and-replay restore, and `admin/config/tests/dr_drill.rs` exercises snapshot, loss, restore and tampered-backup rejection for the config store only. There is no `mg-backup` tool, no schedule, no automatic verification or alert, no cross-store consistency point, no partial restore and no full-enclosure restore test.
> Criteria met 1 of 5 (c); open: a, b, d, e. Confidence: medium.
>
> **Scope change:** Durable sealed journals, `docs/ops/BACKUP_RESTORE.md` and a config-store restore drill exist.
>
> **Still to do:**
> - Build `mg-backup create|verify|restore|list` over a gateway state directory and the mesh services' state directories
> - Test a full restore into a clean environment with the audit chain verified from genesis
> - Define a cross-store consistency point (the gateway holds a lock that currently blocks offline tools while it runs)
> - Add scheduled backups with automatic test-restore and an escalating unverified alert
> - Add single-store and time-range restore
>
> Checked: `docs/ops/BACKUP_RESTORE.md:25` · `docs/ops/BACKUP_RESTORE.md:57` · `admin/config/tests/dr_drill.rs:71` · `services/kms/tests/envelope.rs:59`

**Prompt**

```
Implement backup as a verified capability, not a scheduled hope.

1. Scope: every durable store — object fabric, audit ledger, registries,
   configuration, identities, datasets, model artefacts, and the key material
   needed to read them (handled with the custody its sensitivity demands).
2. Consistency: backups are point-in-time consistent across stores, or the
   inconsistency window is documented and bounded. A backup that cannot be
   reasoned about is not a backup.
3. Encryption and custody: backups are sealed such that possession of the backup
   alone yields nothing; the restore key path is documented, tested and
   dual-controlled.
4. Verification: every backup is automatically verified by a test restore into an
   isolated environment, with the result recorded. An unverified backup is
   reported as unverified, and the alert escalates the longer that persists.
5. Retention and offsite: retention per policy (RF-PER-6), with an offsite or
   off-media copy where the deployment permits, and an air-gapped variant using
   physical media.
6. Tooling: `mg-backup create|verify|restore|list` with clear progress, dry-run,
   and partial-restore (a single store, a time range) support.
```

**Acceptance criteria**

- **AC-RF-RES-1a — Restore reproduces the enclosure.** *Given* a backup, *when* restored into a clean environment, *then* the enclosure is functionally identical and the audit chain verifies from genesis.
- **AC-RF-RES-1b — Every backup is verified automatically.** *Given* a completed backup, *when* verification runs, *then* a test restore is performed and the result recorded; an unverified backup raises an escalating alert.
- **AC-RF-RES-1c — Backups are inert alone.** *Given* backup media without the restore key path, *when* inspected, *then* nothing is readable.
- **AC-RF-RES-1d — Consistency is bounded.** *Given* a backup taken under load, *when* restored, *then* cross-store consistency holds within the documented window and no store references data another lacks.
- **AC-RF-RES-1e — Partial restore works.** *Given* a request to restore one store or one time range, *when* executed, *then* it succeeds without disturbing the rest.

**Done when:** automated verified backups run on a schedule and a full restore has been demonstrated.

---

### RF-RES-2 · Disaster recovery: documented, exercised, measured

**Severity:** Critical · **Area:** Resilience · **Depends on:** RF-RES-1, RF-DEP-4

> **Status at 2026-10-12 — Partial.** `docs/ops/DR_PLAN.md` states one pair of RPO/RTO tiers (60 s or 5 min, 30 min), a ten-step node-loss restore runbook and a drill cadence, and says plainly that sealed journals cannot be opened without the trust root. The journal-shipping replica it relies on for the 60 s tier is not implemented, targets are not per data class or topology, only the node-loss scenario has a runbook, and the only exercised drill is the automated config-store test, not a site loss.
> Criteria met 1 of 5 (d); open: a, b, c; not verifiable by reading: e. Confidence: medium.
>
> **Scope change:** `docs/ops/DR_PLAN.md` claims a 60-second RPO through journal shipping to a warm replica. That mechanism is not implemented; correct the document or build it.
>
> **Still to do:**
> - Either implement journal shipping to a warm replica or remove the 60 s RPO tier from the plan
> - Set and justify RPO and RTO per data class and per topology
> - Write runbooks for storage corruption, key-material loss, destructive action, ransomware and a compromised administrator
> - Run and record a full site-loss exercise with measured RPO and RTO and chain verification from genesis
> - Add the communication plan including the air-gapped physical path
>
> Checked: `docs/ops/DR_PLAN.md:12` · `docs/ops/DR_PLAN.md:28` · `docs/ops/DR_PLAN.md:69` · `admin/config/tests/dr_drill.rs:71`

**Prompt**

```
Define and prove disaster recovery.

1. Define RPO and RTO per data class and per deployment topology, and justify
   each against what the deployment can tolerate — not against what is
   convenient to achieve.
2. Write DR runbooks for the real scenarios: total site loss, storage
   corruption, key-material loss (and what is unrecoverable if the root is lost —
   state this plainly), accidental destructive action, ransomware, and a
   compromised administrator.
3. Implement what the runbooks require: cross-site replication where the topology
   supports it, standby promotion, and the exact procedures with commands.
4. Exercise on a schedule: a real DR test where a site is lost and recovered,
   with measured RPO and RTO compared against target and the gap recorded. An
   unexercised DR plan is an assumption.
5. Communication plan: who is told, when, and how, including for an air-gapped
   site where the notification path is physical.
```

**Acceptance criteria**

- **AC-RF-RES-2a — Targets are defined and justified.** *Given* the DR documentation, *when* reviewed, *then* every data class has an RPO and RTO with justification.
- **AC-RF-RES-2b — The exercise meets targets.** *Given* a scheduled DR exercise, *when* a site is lost and recovered, *then* measured RPO and RTO are recorded and compared with target, and any gap has a remediation item.
- **AC-RF-RES-2c — The chain survives DR.** *Given* a recovered enclosure, *when* the audit chain is verified from genesis, *then* it verifies, and any gap caused by the disaster is explicitly recorded rather than silently absent.
- **AC-RF-RES-2d — Unrecoverable cases are stated.** *Given* the runbooks, *when* read, *then* they state plainly what is unrecoverable (e.g. root key loss) and what the mitigations are.
- **AC-RF-RES-2e — Runbooks are executable.** *Given* the runbooks, *when* followed by someone who did not write them, *then* recovery succeeds.

**Done when:** targets are defined, runbooks are written, and a full DR exercise has been completed and recorded.

---

### RF-RES-3 · High availability and graceful degradation

**Severity:** High · **Area:** Resilience · **Depends on:** RF-DEP-2, RF-RT-2

> **Status at 2026-10-12 — Partial.** Fail-closed behaviour is coded on the edge (an audit write failure returns 500 and no token or data leaves) and the fabric has a tested degraded mode without Mimir. Every service runs as a single replica over a locked single-writer state directory, so there is no redundancy, leader election or quorum; there is no dependency-and-degradation matrix, no chaos test, no test of an unavailable ledger, and the Resilience view is fixed data.
> Criteria met 0 of 5; open: a, c, d, e; not verifiable by reading: b. Confidence: medium.
>
> **Scope change:** Blocked on the same replication decision as RF-DEP-2 and RF-CA-9.
>
> **Still to do:**
> - Write the dependency-and-degradation matrix for the five services as they are deployed
> - Add tests that inject an unavailable ledger and an unavailable PDP and assert refusal
> - Decide the redundancy model for single-writer stores (replication and failover) before promising HA
> - Add kill-each-component tests against compose or kind that assert the matrix
> - Surface live degradation in the console in place of the fixed Resilience view
>
> Checked: `services/http-gateway/src/lib.rs:719` · `services/fabric/tests/degraded.rs:189` · `deploy/helm/midgard/values.yaml:36` · `services/http-gateway/src/data.rs:233`

**Prompt**

```
Define what stays working when parts fail.

1. Availability model: for each service, the failure modes, the redundancy, the
   failover mechanism and time, and what degrades if it is unavailable. Publish
   as a dependency-and-degradation matrix.
2. Degradation must be graceful and explicit: if Mimir is down, the rest of the
   platform works and AI features report unavailable; if the PDP is down,
   everything fails CLOSED (never open); if audit is unavailable, mutations are
   refused (an unaudited mutation is not permitted, and this must be a deliberate,
   documented choice).
3. Implement the redundancy the model requires: leader election where needed,
   quorum semantics, split-brain prevention, and idempotent recovery.
4. Prove it with chaos testing (RF-TST-8): kill each component and assert the
   documented degradation, not a guessed one.
5. Surface current degradation in the UI so operators know what they cannot rely
   on right now.
```

**Acceptance criteria**

- **AC-RF-RES-3a — Documented degradation is actual.** *Given* the matrix, *when* each component is killed, *then* the observed behaviour matches the documented degradation exactly.
- **AC-RF-RES-3b — PDP failure fails closed.** *Given* an unavailable PDP, *when* any authorisation is required, *then* it is denied; no path permits an open failure.
- **AC-RF-RES-3c — Unaudited mutations are refused.** *Given* an unavailable audit ledger, *when* a mutation is attempted, *then* it is refused.
- **AC-RF-RES-3d — No split brain.** *Given* a network partition, *when* it occurs and heals, *then* no divergent authoritative state exists and recovery is deterministic.
- **AC-RF-RES-3e — Degradation is visible.** *Given* a degraded platform, *when* an operator uses it, *then* the affected capabilities are visibly marked unavailable.

**Done when:** the matrix exists, chaos tests confirm every row, and degradation is visible in the UI.

---

### RF-RES-4 · Data integrity assurance and corruption recovery

**Severity:** High · **Area:** Resilience · **Depends on:** RF-PER-2, RF-PER-8

> **Status at 2026-10-12 — Partial.** Integrity is checked at start only: every sealed store replays and re-verifies its signed chain on open, and the gateway refuses to boot on a tampered ledger, both tested; a manual procedure (RB-7) covers chain verification. Nothing runs on a schedule while a service is up: no scrubbing, no index or cross-store referential check, no derived-data rebuild, no corruption runbook per store class and no ledger record of a detection.
> Criteria met 0 of 4; open: a, b, d; not verifiable by reading: c. Confidence: medium.
>
> **Still to do:**
> - Add scheduled full and incremental chain verification and store scrubbing in running services, with results recorded and alerted
> - Add cross-store referential checks (fabric objects against ledger entries)
> - Add rebuild of derived data from authoritative sources, recorded in the ledger
> - Write corruption runbooks per store class
> - Add induced-corruption tests per store class with a stated detection interval
>
> Checked: `services/http-gateway/tests/state_dir_boot.rs:188` · `services/kms/tests/file_ledger_restart.rs:148` · `docs/ops/RUNBOOKS.md:81` · `README.md:368`

**Prompt**

```
Detect corruption early and recover from it deliberately.

1. Continuous verification: scheduled audit-chain verification (full and
   incremental), storage scrubbing (RF-PER-8), index consistency checks, and
   cross-store referential checks. Results are recorded and alerted.
2. Corruption response runbooks per class: audit chain, object store, dataset,
   model artefact, configuration — each stating how to identify scope, what to
   restore, and how to record the event.
3. Repair where safe: rebuild derived data (indexes, projections, caches) from
   authoritative sources automatically; never "repair" authoritative data
   silently — that is a data-integrity incident requiring a human.
4. Report honestly: corruption and its remediation are audited and reported, not
   quietly fixed. In an evidentiary system, a silent repair is indistinguishable
   from tampering.
```

**Acceptance criteria**

- **AC-RF-RES-4a — Corruption is detected promptly.** *Given* induced corruption in each store class, *when* the scheduled checks run, *then* each is detected within its stated interval and alerted.
- **AC-RF-RES-4b — Derived data rebuilds.** *Given* a corrupted index or projection, *when* repair runs, *then* it rebuilds from the authoritative source and the rebuild is recorded.
- **AC-RF-RES-4c — Authoritative corruption escalates.** *Given* corruption in authoritative data, *when* detected, *then* it is escalated to a human and never silently repaired.
- **AC-RF-RES-4d — Everything is recorded.** *Given* any detection or remediation, *when* the ledger is queried, *then* it is present with scope and action.

**Done when:** continuous verification runs, runbooks exist per class, and the induced-corruption tests pass.

---

### RF-RES-5 · Business continuity for the air-gapped and degraded case

**Severity:** Medium · **Area:** Resilience · **Depends on:** RF-RES-2, RF-UX-11

> **Status at 2026-10-12 — Partial.** Only library-level pieces exist: `clients/sync` and `heimdall/picture-sync` have convergence tests for reconnecting replicas, and the platform runs with no network by design. There is no site-level isolation or rejoin test, no handling of expired credentials or superseded configuration at rejoin (mesh certificates do not expire at all), no on-site recovery kit in the bundle and no recorded manual-fallback drill.
> Criteria met 0 of 4; open: b, c, d; not verifiable by reading: a. Confidence: low.
>
> **Still to do:**
> - Define which components sync between sites at all; today no deployed service replicates to a peer site
> - Add a rejoin test covering divergent state, expired credentials, revoked keys and superseded configuration (needs RF-IAM-6 expiry first)
> - Ship runbooks, docs and spare artefacts in the bundle so a site can recover alone
> - Document manual fallbacks and record a drill
>
> Checked: `clients/sync/tests/convergence.rs:80` · `clients/sync/tests/convergence.rs:36` · `tools/mg-admin/src/mesh.rs:115` · `deploy/air-gap-bundle/README.md:40`

**Prompt**

```
Plan for continuity where there is no remote help.

1. A site that loses its connection to every peer must continue operating on
   local data indefinitely, with a clear statement of what is degraded and what
   is stale.
2. Local recovery: everything needed to diagnose and recover — runbooks,
   documentation, tooling, spare artefacts — is present on site, because nobody
   is going to send it.
3. Reconciliation after a long disconnection: define and test what happens when a
   site rejoins after weeks, including divergent state, expired credentials,
   revoked keys and superseded configuration. Expired credentials at a rejoining
   site are a predictable operational trap; handle it deliberately.
4. Manual and physical fallbacks: where an automated path is impossible, document
   the manual one, and make sure it is exercised.
```

**Acceptance criteria**

- **AC-RF-RES-5a — Indefinite local operation.** *Given* total isolation, *when* the site operates for an extended period, *then* it continues functioning with degradation clearly stated.
- **AC-RF-RES-5b — Long-disconnection rejoin works.** *Given* a site rejoining after an extended disconnection with expired credentials and superseded configuration, *when* it reconnects, *then* reconciliation completes deterministically and every conflict is either resolved by rule or surfaced.
- **AC-RF-RES-5c — Local recovery is self-sufficient.** *Given* an isolated site with a failure, *when* the on-site operator follows on-site materials, *then* recovery succeeds with no external contact.
- **AC-RF-RES-5d — Manual fallbacks are exercised.** *Given* the documented manual procedures, *when* exercised in a drill, *then* they work and the drill is recorded.

**Done when:** extended-isolation and long-disconnection rejoin are both tested and documented.

---
# T — Security assurance

> **Status.** The design is security-led and the primitives are real. What is absent is *assurance*: the threat models exist as diagrams (`docs/dev/diagrams/*_threat_dfd.*`) with no validating tests, there is no fuzzing, no penetration test, no static or dynamic analysis in CI beyond a secret scan, and no vulnerability disclosure process.

---

### RF-SEC-1 · Validate the threat models with tests

**Severity:** Critical · **Area:** Security · **Depends on:** RF-TST-3

> **Status at 2026-10-12 — Partial.** A written STRIDE threat model now exists at `docs/security/THREAT_MODEL.md` naming controls and RT ids per category, but threats have no stable ids, no per-threat test mapping, and no test or source file cites a threat id. There is no traceability check and no PR template or CI in the public tree to enforce review.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** A written STRIDE model exists at `docs/security/THREAT_MODEL.md`. The gap is stable threat ids, an attack test per threat, and traceability.
>
> **Still to do:**
> - Assign every threat a stable id and restructure the model per element and data flow with control and proving test
> - Write the missing attack tests against a running multi-process environment (the compose enclosure can host them)
> - Cite threat ids in mitigating code and fail a traceability check on any unmapped threat
> - Reconcile the model with the built system (compose/Helm services, mesh flows, sidecars) and the seven DFDs
> - Add a threat-model review check for PRs that add a service, flow or trust boundary
>
> Checked: `docs/security/THREAT_MODEL.md:34` · `docs/security/THREAT_MODEL.md:97` · `docs/security/THREAT_MODEL.md:106`

**Evidence.** Seven STRIDE data-flow diagrams exist (`midgard_threat_dfd`, `mimir_`, `forge_`, `bifrost_`, `heimdall_`, `admin_`, `client_`). No test references a threat id.

**Prompt**

```
Turn the threat models from diagrams into a tested contract.

1. Write out each threat model as structured text alongside its diagram: for
   every element and data flow, the STRIDE threats, the mitigating control, and
   the test that proves the control works. Assign every threat a stable id.
2. Write the missing tests. Each threat's test attempts the attack and asserts it
   fails, in an integration environment rather than a unit test wherever the
   threat crosses a boundary.
3. Cite threat ids in the mitigating code, and add them to the traceability
   matrix (RF-SC-6) so an unmitigated or untested threat is a CI failure.
4. Review the models against the implementation as built — the diagrams predate
   the implementation and will have drifted. Any element or flow present in code
   and absent from the model is a gap in the model.
5. Re-review on every architectural change, enforced by requiring a threat-model
   review checkbox on PRs that add a service, a data flow or a trust boundary.
```

**Acceptance criteria**

- **AC-RF-SEC-1a — Every threat has a control and a test.** *Given* the threat models, *when* the traceability matrix is generated, *then* every threat id maps to a control in code and a test, and any gap fails CI.
- **AC-RF-SEC-1b — Attacks actually fail.** *Given* each threat's test, *when* it attempts the attack against a running environment, *then* the attack fails and the control's evidence (audit entry, denial, alert) is observed.
- **AC-RF-SEC-1c — Models match the implementation.** *Given* the implemented architecture, *when* compared with the models, *then* every service, data flow and trust boundary is represented.
- **AC-RF-SEC-1d — Drift is prevented.** *Given* a PR adding a trust boundary, *when* it is opened without a threat-model update, *then* the check fails.

**Done when:** every threat id has a mitigating control and a passing attack test, and the models match the built system.

---

### RF-SEC-2 · Fuzzing and property-based testing of every parser and boundary

**Severity:** High · **Area:** Security · **Depends on:** RF-TST-4

> **Status at 2026-10-12 — Open.** No fuzz target, corpus, `cargo fuzz` directory, proptest or quickcheck usage exists outside `vendor/`. The nearest things are example-based tests such as the exhaustive permutation test in `clients/sync/tests/convergence.rs` and a single empty-policy deny test in `crypto/policy/tests/pdp.rs`.
> Criteria met 0 of 5; open: a, c, d, e; not verifiable by reading: b. Confidence: high.
>
> **Still to do:**
> - Create coverage-guided fuzz targets with checked-in corpora for the HTTP/1.1 parser, JSON codec, PQ-mTLS handshake/record decoder, RPC frames, signature envelope/DSSE, bundle manifest, SBOM and model header parsers
> - Add property suites for CRDT laws, deny-by-default over generated requests, audit-chain mutation detection, suite negotiation and seal/unseal
> - Add differential tests for the two Forge engines and in-memory versus durable stores
> - Run 24-hour campaigns per target and check in minimised reproducers as regression tests
>
> Checked: `clients/sync/tests/convergence.rs:36` · `crypto/policy/tests/pdp.rs:65` · `justfile:151`

**Evidence.** The tree contains several hand-rolled parsers on trust boundaries: the HTTP/1.1 parser and JSON codec in `services/http-gateway`, the PQ-mTLS frame decoder in `crypto/pq-mtls`, the `Cargo.lock` parser in `mg-sbom`, and (once implemented) the signature envelope, DSSE, bundle, model-artefact and proto decoders. None is fuzzed.

**Prompt**

```
Fuzz everything that parses untrusted input.

1. Build coverage-guided fuzz targets for: the HTTP/1.1 parser, the JSON codec,
   the PQ-mTLS handshake and record decoder, the RPC frame decoder, the signature
   envelope and DSSE parser, the bundle manifest parser, the SBOM parser, the
   model artefact (GGUF/safetensors) header parser, the feed adapters, the
   connector input paths, and every proto decode boundary.
2. Run continuously in a nightly job with a persistent corpus; any crash, hang,
   OOM or assertion failure fails the job and files an issue with the reproducer.
   Minimise and check in every reproducer as a regression test.
3. Add property-based tests for the algorithmic cores where invariants are
   stated: CRDT convergence (idempotence, commutativity, associativity), the
   policy engine (deny-by-default holds for every generated request), the audit
   chain (any mutation is detected), the crypto agility layer (negotiation always
   selects a mutually-supported suite or fails), envelope encryption
   (seal/unseal round-trips, wrong root fails), and the fusion tracker.
4. Add differential testing where two implementations exist: the Forge reference
   engine versus the real backend (RF-F-1a), and the in-memory versus persistent
   stores.
5. Measure and report fuzz coverage; a parser below the coverage threshold is a
   gap.
```

**Acceptance criteria**

- **AC-RF-SEC-2a — Every parser is fuzzed.** *Given* the list of parsers, *when* the fuzz suite is inspected, *then* each has a target with a persistent corpus.
- **AC-RF-SEC-2b — Campaigns are clean.** *Given* a 24-hour campaign per target, *when* it completes, *then* no crash, hang, OOM or assertion failure occurs.
- **AC-RF-SEC-2c — Reproducers become tests.** *Given* any historical fuzz finding, *when* the regression suite runs, *then* its minimised reproducer is included.
- **AC-RF-SEC-2d — Properties hold.** *Given* the property suites, *when* run with a large case count, *then* every stated invariant holds; deny-by-default in particular holds for every generated request.
- **AC-RF-SEC-2e — Differential agreement.** *Given* the two Forge engines and the two store implementations, *when* differentially tested, *then* they agree on every generated case.

**Done when:** all targets exist, nightly campaigns are clean, and the property and differential suites are green.

---

### RF-SEC-3 · Static and dynamic analysis in CI

**Severity:** High · **Area:** Security · **Depends on:** RF-SC-4

> **Status at 2026-10-12 — Partial.** `just lint` runs clippy at `-D warnings` across the crates, `mg-scan` does an offline advisory and licence scan against a vendored RustSec snapshot, and `.gitleaks.toml` configures secret scanning. Only 9 of roughly 70 crates carry `#![forbid(unsafe_code)]`, there is no workspace lint table, no SAST, no DAST, no findings dashboard, and no CI workflow is published (the gitleaks config refers to a `.github/workflows/ci.yml` that is not in the tree).
> Criteria met 0 of 5; open: a, b, c, e; not verifiable by reading: d. Confidence: medium.
>
> **Scope change:** The Evidence says CI runs `gitleaks`; there is no CI configuration in the tree (RF-CA-4).
>
> **Still to do:**
> - Add `#![forbid(unsafe_code)]` to every crate that can take it and document the exception for `pqc-agility` (mlock in `secret.rs`); add a check that enforces it
> - Publish the CI definition with a merge-blocking policy for new high-severity findings
> - Add DAST (authenticated and unauthenticated) against the compose enclosure's HTTP edge
> - Run a full-history secret scan and add a pre-commit hook
> - Add TypeScript security lint and a single findings register with severity, owner and age
>
> Checked: `justfile:129` · `clients/core/src/lib.rs:23` · `crypto/pqc-agility/src/secret.rs:36` · `.gitleaks.toml:3` · `vendor/advisory-db/snapshot.toml:17`

**Evidence.** CI runs `gitleaks` and a placeholder provenance lint. There is no SAST, no dependency policy enforcement beyond what RF-SC-4 adds, no unsafe-code audit (though `clients/keystore` uses `#![forbid(unsafe_code)]` — a good pattern not applied workspace-wide), and no DAST.

**Prompt**

```
Add analysis depth to the pipeline.

1. Rust: clippy at deny level with a curated security-relevant lint set;
   `#![forbid(unsafe_code)]` in every crate that can have it, and for the few
   that cannot, a documented justification plus a review requirement on changes
   to unsafe blocks. Add cargo-geiger-style reporting of unsafe usage across the
   dependency graph.
2. TypeScript: strict compiler settings everywhere, a security-focused lint set,
   and a check that `any` does not appear on a trust boundary.
3. SAST across both languages with results triaged, and a policy that a new
   high-severity finding blocks merge while existing ones are tracked with owners
   and deadlines.
4. DAST against a running enclosure in CI: authenticated and unauthenticated
   scans of the HTTP edge, checking headers, CSP, injection, authorisation
   bypass, and information disclosure.
5. Secret scanning extended to history, not only the diff, plus a pre-commit
   hook.
6. Container and image scanning (RF-DEP-1) integrated with the same policy.
7. Report all findings to one dashboard with severity, owner and age, so security
   debt is visible rather than distributed across tool outputs.
```

**Acceptance criteria**

- **AC-RF-SEC-3a — Unsafe is bounded and justified.** *Given* the workspace, *when* inspected, *then* every crate either forbids unsafe or documents why it cannot, and unsafe blocks require review.
- **AC-RF-SEC-3b — New high-severity findings block merge.** *Given* a PR introducing one, *when* CI runs, *then* it fails.
- **AC-RF-SEC-3c — DAST runs authenticated.** *Given* the running enclosure, *when* DAST runs, *then* it exercises authenticated paths and reports no unaddressed high-severity finding.
- **AC-RF-SEC-3d — History is clean.** *Given* the full git history, *when* scanned, *then* no secret is present; any historical finding is remediated and the credential rotated.
- **AC-RF-SEC-3e — Debt is visible.** *Given* the dashboard, *when* opened, *then* every open finding has severity, owner and age.

**Done when:** the full analysis set runs in CI with merge-blocking policy and a single findings dashboard.

---

### RF-SEC-4 · Cryptographic assurance and validation evidence

**Severity:** High · **Area:** Security · **Depends on:** RF-RT-5

> **Status at 2026-10-12 — Partial.** The compliance matrix and residual-risk register state that in-repo ACVP KATs are not a FIPS 140-3 module certificate, and `docs/ops/PKI.md` documents key ceremony, renewal, revocation and rotation cadence. There is no single cryptographic inventory, no side-channel document, no timing test, no independent review report, and algorithm retirement is exercised only at primitive level (`agility_fips.rs`, `agility_hotswap.rs`) with no documented end-to-end procedure. `ct_eq` is used in about ten files but no audit or test shows it is used at every secret comparison.
> Criteria met 1 of 5 (b); open: a, c, e; not verifiable by reading: d. Confidence: medium.
>
> **Still to do:**
> - Write the cryptographic inventory (algorithms, parameter sets, key types, lifetimes, protocols, validation status) and check it against the code
> - Audit every comparison of secret material for `ct_eq` and add a test or lint that detects a variable-time comparison
> - Document side-channel considerations per primitive and the full key lifecycle including archival and destruction
> - Document and test an end-to-end algorithm retirement on sealed data and sessions, not only on a bare CryptoProvider
> - Commission the independent cryptographic review and track its findings
>
> Checked: `docs/compliance/COMPLIANCE_MATRIX.md:5` · `docs/compliance/RESIDUAL_RISK_REGISTER.md:35` · `docs/ops/PKI.md:123` · `crypto/pqc-agility/src/hash.rs:276` · `crypto/pqc-agility/tests/agility_fips.rs:70`

**Evidence.** `pqc-agility` uses ACVP-validated FIPS 203/204/205 implementations and has KAT tests (`fips_backend`, `aead_kat`, `agility_fips`), which is a strong base. What is missing is the assurance wrapper: no documented cryptographic inventory, no side-channel consideration, no formal review, and no statement of what "FIPS-validated" does and does not mean for this deployment.

**Prompt**

```
Document and evidence the cryptographic posture.

1. Cryptographic inventory: every algorithm, parameter set, key type, key
   lifetime, protocol and their purposes, in one document, with the standard each
   conforms to and the validation status of the implementation used.
2. Be precise about validation claims: using an ACVP-validated implementation is
   not the same as holding a FIPS 140-3 module certificate. State exactly what is
   claimed and what is not, so no deployment is misled.
3. Side channels: document the timing, cache and power considerations for each
   primitive as used here; ensure constant-time comparison everywhere it matters
   (the `ct_eq` helper exists — verify it is used at every comparison of secret
   material); and test for timing variation where feasible.
4. Key management: full lifecycle per key type — generation, storage, use,
   rotation, archival, destruction — with the custody requirement for each.
5. Independent review: commission a cryptographic review of the protocol
   constructions (the hybrid combiner, the PQ-mTLS handshake, the KEM-DEM
   envelope, the audit chain construction, the signature domain separation) and
   remediate findings. Protocol composition is where correct primitives most
   often go wrong.
6. Agility in practice: document and test the procedure for retiring an algorithm
   and adopting a successor without data migration — the crate is designed for
   it, so prove it end to end.
```

**Acceptance criteria**

- **AC-RF-SEC-4a — Inventory is complete and accurate.** *Given* the inventory, *when* compared with the code, *then* every algorithm and key type in use appears with its correct parameters.
- **AC-RF-SEC-4b — Claims are precise.** *Given* the validation statement, *when* read, *then* it distinguishes implementation validation from module certification and states exactly what is claimed.
- **AC-RF-SEC-4c — Constant-time where it matters.** *Given* every comparison of secret material, *when* audited, *then* each uses the constant-time helper; a test detects a variable-time comparison.
- **AC-RF-SEC-4d — Independent review completed.** *Given* the review report, *when* checked, *then* every finding is remediated or has an accepted, documented risk with an owner.
- **AC-RF-SEC-4e — Agility is exercised.** *Given* a retired algorithm, *when* the documented procedure is followed, *then* existing data remains readable and new operations use the successor, with no migration.

**Done when:** the inventory and precise claims are published, the review is complete, and an algorithm retirement has been exercised.

---

### RF-SEC-5 · Penetration testing and red teaming

**Severity:** High · **Area:** Security · **Depends on:** RF-SEC-1, RF-DEP-2

> **Status at 2026-10-12 — Open.** No penetration-test scope, report, findings register or finding-derived regression suite exists in the repository; the only matches for the term are in design SADs and a Mimir staging assessor. This item is an external activity that cannot be established from the tree.
> Criteria met 0 of 4; open: c; not verifiable by reading: a, b, d. Confidence: medium.
>
> **Still to do:**
> - Write and commit the penetration-test and AI red-team scope
> - Commission the external test against a deployed enclosure
> - Track findings with owners and deadlines and retest
> - Add a regression test per finding in a dedicated suite
> - Add the pen test to the GA readiness blockers list, which currently omits it
>
> Checked: `docs/compliance/GA_READINESS.md:74` · `docs/security/THREAT_MODEL.md:97`

**Prompt**

```
Have the system attacked by people who want it to fail.

1. Scope an external penetration test covering: the browser edge and consoles,
   authentication and session, authorisation and clearance enforcement, the RPC
   layer and workload identity, the supply chain, the AI surfaces (prompt
   injection, indirect injection, tool abuse, exfiltration through model output),
   the client shells, and the physical/air-gap transit path.
2. Add an AI-specific red team: attempts to exfiltrate governed data through
   model output, to escalate via tool use, to poison memory or feedback, to
   bypass guardrails through obfuscation and multi-turn accumulation, and to
   cause harmful action through gated paths.
3. Remediate with tracked findings, owners and deadlines by severity; retest to
   confirm.
4. Convert every finding into a permanent regression test so it cannot recur.
5. Repeat per major release and after any significant architectural change.
```

**Acceptance criteria**

- **AC-RF-SEC-5a — Full-scope test completed.** *Given* the defined scope, *when* the test is performed, *then* every area is covered and reported.
- **AC-RF-SEC-5b — Findings are remediated and retested.** *Given* each finding, *when* remediation completes, *then* a retest confirms it and the finding closes.
- **AC-RF-SEC-5c — Findings become tests.** *Given* the regression suite, *when* run, *then* it contains a test for every historical finding.
- **AC-RF-SEC-5d — AI red team covered.** *Given* the AI-specific scope, *when* tested, *then* exfiltration, escalation, poisoning and guardrail-bypass attempts are all attempted and their outcomes reported.

**Done when:** a full-scope test and an AI red team have been completed, remediated, retested and converted to regression tests.

---

### RF-SEC-6 · Security monitoring, detection and response

**Severity:** High · **Area:** Security · **Depends on:** RF-OBS-1, RF-OBS-4

> **Status at 2026-10-12 — Partial.** Security alerting exists only for break-glass (`admin/breakglass/src/alert.rs`, tested in `ac_ad_8.rs`), alongside SLO breach telemetry and an incident-response document with severity levels. There is no detection catalogue, no behavioural baseline, no insider-threat detection, no per-detection playbook and no one-step containment action in a console.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Still to do:**
> - Define and implement the detection set (auth anomalies, context forgery, clearance probing, access volume, signature and chain failures, revoked-key use, egress attempts) with a simulation test per detection
> - Add insider-threat detections (enumeration, repeated denials, off-incident break-glass) with simulations
> - Expose revoke / quarantine / isolate as single dual-controlled audited console actions
> - Store security telemetry under the audit chain's append-only guarantee and test tamper detection
> - Write a playbook per detection
>
> Checked: `admin/breakglass/src/alert.rs:7` · `admin/breakglass/tests/ac_ad_8.rs:20` · `docs/ops/INCIDENT_RESPONSE.md:7` · `services/http-gateway/tests/rc_33.rs:68`

**Prompt**

```
Detect attacks, not only failures.

1. Define detections for the attacks that matter here: authentication anomalies,
   privilege escalation attempts, policy-context forgery, clearance-boundary
   probing, unusual data access volume or pattern, guardrail-block spikes, tool
   abuse in agent runs, signature verification failures, audit chain anomalies,
   revoked-key use, workload identity anomalies, and egress attempts outside the
   broker.
2. Baseline and anomaly detection where static rules are insufficient — per
   subject and per workload behavioural baselines with alerting on deviation.
3. Insider threat is a first-class concern in a platform whose whole purpose is
   governing access: detect and alert on patterns such as broad enumeration
   before departure, unusual after-hours access, repeated denied attempts, and
   break-glass use outside an incident.
4. Response: playbooks for each detection, with the containment action (revoke
   session, revoke identity, quarantine artefact, isolate node) available as a
   one-step, dual-controlled, audited action from the console.
5. Retain security telemetry for the compliance-required period, protected from
   modification by the same append-only guarantees as the audit ledger.
```

**Acceptance criteria**

- **AC-RF-SEC-6a — Each detection fires.** *Given* each defined attack pattern, *when* simulated, *then* the corresponding detection fires with the correct severity.
- **AC-RF-SEC-6b — Containment is one step.** *Given* a detection, *when* the responder acts, *then* revoke/quarantine/isolate is available as a single dual-controlled action from the console and is audited.
- **AC-RF-SEC-6c — Insider patterns are detected.** *Given* the insider-threat scenarios, *when* simulated, *then* each is detected and alerted.
- **AC-RF-SEC-6d — Security telemetry is tamper-evident.** *Given* security telemetry, *when* modification is attempted, *then* it is detectable by the same mechanism as the audit chain.

**Done when:** every detection is implemented and proven by simulation, with one-step containment available.

---

### RF-SEC-7 · Vulnerability management and disclosure

**Severity:** Medium · **Area:** Security · **Depends on:** RF-SC-4

> **Status at 2026-10-12 — Partial.** `SECURITY.md` now exists with a reporting address, acknowledgement and assessment times, a 90-day coordinated-disclosure default, safe-harbour terms and scope. Internal handling (SLAs by severity, backport decision), machine-readable advisories, the air-gapped notification path and an emergency re-vendoring procedure are not documented; `just advisory-db-refresh` only refreshes upstream RustSec data.
> Criteria met 0 of 5; open: b, c, d, e; not verifiable by reading: a. Confidence: medium.
>
> **Scope change:** `SECURITY.md` exists with a contact, acknowledgement and disclosure timelines, safe harbour and scope. What remains is internal SLAs, machine-readable advisories, the air-gapped notification path and emergency re-vendoring. The vendored advisory snapshot is past `mg-scan`'s 30-day warning threshold.
>
> **Still to do:**
> - Confirm the reporting mailbox, the security.txt key and private reporting are live and monitored
> - Document triage, CVSS plus deployment-context severity, remediation SLAs and backport policy, and measure against them
> - Publish Midgard advisories in a machine-readable format that `mg-scan` consumes
> - Define the out-of-band advisory path for air-gapped sites (for example carried in the `.mgbundle`)
> - Document and exercise the emergency re-vendoring process for a vulnerable vendored crate
>
> Checked: `SECURITY.md:11` · `SECURITY.md:23` · `SECURITY.md:66` · `vendor/advisory-db/snapshot.toml:1`

**Evidence.** No `SECURITY.md`, no disclosure policy, no advisory process. `LICENSE` is present but untracked in git (it appears as `??` in status), which itself needs resolving (RF-DOC-2).

**Prompt**

```
Establish the vulnerability lifecycle for an open-source, sovereignty-focused
platform.

1. SECURITY.md: how to report, the contact and its key, the expected response
   times, the disclosure timeline, safe-harbour terms for good-faith research,
   and scope.
2. Internal handling: triage, severity assessment (CVSS plus a deployment-context
   adjustment, since an air-gapped site's exposure differs), remediation SLAs by
   severity, and a documented decision path for whether a fix is backported.
3. Advisory publication: issue advisories with affected versions, CVSS,
   mitigations and fixed versions, in a machine-readable format so the scan
   pipeline (RF-SC-4) of downstream deployments consumes them automatically.
4. Air-gapped notification: a site with no connectivity must still learn about
   advisories — define the physical or out-of-band path, since this is exactly
   the deployment least able to learn on its own.
5. Coordinated disclosure with dependency upstreams, and a process for handling a
   vulnerability in a vendored dependency including emergency re-vendoring.
```

**Acceptance criteria**

- **AC-RF-SEC-7a — Reporting works.** *Given* a good-faith researcher, *when* they follow SECURITY.md, *then* they reach a monitored channel and receive acknowledgement within the stated time.
- **AC-RF-SEC-7b — SLAs are met and measured.** *Given* accumulated reports, *when* metrics are reviewed, *then* time-to-triage and time-to-fix are measured against the SLA.
- **AC-RF-SEC-7c — Advisories are machine-readable.** *Given* a published advisory, *when* the scan pipeline runs at a downstream deployment, *then* it is consumed automatically.
- **AC-RF-SEC-7d — Air-gapped sites are reachable.** *Given* an advisory and a disconnected site, *when* the defined path is followed, *then* the site learns of it.
- **AC-RF-SEC-7e — Vendored dependencies are handled.** *Given* a vulnerability in a vendored crate, *when* the emergency process runs, *then* a re-vendored, gated release is produced within the SLA.

**Done when:** SECURITY.md is published, the process is documented and exercised once, and advisories are machine-readable.

---

### RF-SEC-8 · Physical and supply-chain security for air-gapped operations

**Severity:** Medium · **Area:** Security · **Depends on:** RF-B-4, RF-DEP-5

> **Status at 2026-10-12 — Partial.** `docs/ops/PKI.md` documents the HSM key ceremony, two-officer witness sign-off and rotation, the KMS hardware trust-root seam has a root-attestation test (`rc_28.rs`, software stand-in), and the guarded-action orchestrator denies a single-author high-risk commit (`rc_31.rs`). Media handling is one sentence in the air-gap bundle README, node-enrolment attestation was not found, and site and hardware assumptions are not written down as a verifiable list.
> Criteria met 0 of 4; open: a, b, d; not verifiable by reading: c. Confidence: medium.
>
> **Still to do:**
> - Write the removable-media procedure (sourcing, sanitisation, chain of custody, tamper-evident packaging, receipt verification, destruction) and exercise it once
> - Verify measured-boot / TPM attestation at node enrolment where hardware supports it
> - Prove no single-person path exists for dual-custody operations across every route including break-glass, not only the orchestrator
> - Document hardware-trust and site-security assumptions as an explicit checklist a deployer can verify
> - Document break-glass credential sealing and the physical controls for each ceremony
>
> Checked: `docs/ops/PKI.md:106` · `services/kms/tests/rc_28.rs:81` · `admin/orchestrator/tests/rc_31.rs:150` · `deploy/air-gap-bundle/README.md:44` · `services/kms/src/hardware.rs:47`

**Prompt**

```
Address the parts of the threat model that are not software.

1. Media handling: procedures for the removable media that carry bundles —
   sourcing, sanitisation, chain of custody, tamper-evident packaging,
   verification on receipt, and destruction. Media is the air-gapped site's
   entire attack surface for inbound content.
2. Hardware trust: what is assumed about the hardware, how the platform is
   verified (measured boot, TPM attestation where available), and what changes if
   the assumption fails.
3. Personnel: the dual-custody roles the platform depends on, and what the
   platform does when only one person is available (the answer must be "some
   things are impossible", not "there is an override").
4. Ceremony documentation: key generation, root custody, break-glass credential
   sealing, and the physical controls each requires.
5. Site security assumptions: what the platform assumes about the environment it
   is in, stated explicitly so a deployment can verify it holds.
```

**Acceptance criteria**

- **AC-RF-SEC-8a — Media procedures exist and are exercised.** *Given* a bundle transfer, *when* it is performed following the procedure, *then* chain of custody is recorded and verification on receipt is evidenced.
- **AC-RF-SEC-8b — Attestation is used where available.** *Given* hardware supporting measured boot and TPM attestation, *when* a node enrols, *then* attestation is verified (RF-IAM-6a).
- **AC-RF-SEC-8c — No single-person override.** *Given* a dual-custody operation with one person available, *when* attempted, *then* it cannot be completed by any path.
- **AC-RF-SEC-8d — Assumptions are explicit.** *Given* the documentation, *when* read by a deploying organisation, *then* every environmental assumption is stated and verifiable.

**Done when:** the physical procedures are documented, exercised once, and the no-override property is proven.

---

# U — Test strategy & coverage

> **Status.** The Rust test suite is substantial and genuine: `just test` runs 45 explicit invocations across both crypto backends, and 72 acceptance criteria are exercised by dedicated tests and eight verification scripts. The gaps are: **zero** frontend tests of any kind (no vitest, no playwright, no storybook, no test script in any `package.json`), no integration testing across process boundaries (because there are no processes), no load, chaos, fuzz, accessibility or visual testing, and no coverage measurement.

---

### RF-TST-1 · Test strategy, taxonomy and coverage targets

**Severity:** Medium · **Area:** Testing · **Depends on:** —

> **Status at 2026-10-12 — Open.** There is no `docs/dev/testing/` directory or strategy document, no coverage tooling (no llvm-cov, tarpaulin or grcov reference outside vendor) and no flake-quarantine process. The only description of test levels is the three-tier table in `deploy/acceptance/deployment/README.md`.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Write `docs/dev/testing/strategy.md` covering levels, placement and naming, coverage targets, time budgets and gates
> - Add Rust and TypeScript coverage measurement reported per component and gated on decrease
> - Define the flake quarantine process with owner and deadline
> - State and enforce the regression-test-with-every-fix rule
>
> Checked: `deploy/acceptance/deployment/README.md:3` · `justfile:151` · `CONTRIBUTING.md:26`

**Prompt**

```
Write the test strategy the suite has grown past needing implicitly.

docs/dev/testing/strategy.md defining: the test levels (unit, property, fuzz,
contract, integration, end-to-end, acceptance, load, chaos, accessibility,
visual, security), what belongs at each, coverage targets per level and per
component (with higher targets for security-relevant code), the naming and
placement conventions, how flakiness is handled (quarantine with an owner and a
deadline, never a silent retry), how long each level may take, and which gate
each blocks. Include the rule that a bug fix lands with a regression test that
fails before the fix.

Then add coverage measurement to CI for Rust and TypeScript, report it per
component, and gate on a decrease rather than an absolute number — so coverage
can improve incrementally without an arbitrary cliff.
```

**Acceptance criteria**

- **AC-RF-TST-1a — Strategy exists and is followed.** *Given* the strategy, *when* the suite is audited against it, *then* every level exists and tests are placed per the conventions.
- **AC-RF-TST-1b — Coverage is measured and does not regress.** *Given* a PR reducing coverage in a component, *when* CI runs, *then* it fails.
- **AC-RF-TST-1c — Flakes are quarantined, not retried.** *Given* a flaky test, *when* it is identified, *then* it is quarantined with an owner and a deadline, and no silent retry masks it.
- **AC-RF-TST-1d — Fixes carry regression tests.** *Given* a bug-fix PR, *when* reviewed, *then* it includes a test that fails without the fix.

**Done when:** the strategy is published, coverage is measured and gated, and the flake process is in place.

---

### RF-TST-2 · Frontend testing from zero

**Severity:** High · **Area:** Testing · **Depends on:** RF-UI-2, RF-UID-1

> **Status at 2026-10-12 — Open.** No `*.test.*`, `*.spec.*`, vitest, playwright config or stories file exists, and none of the ten workspace `package.json` files defines a `test` script, so the root `pnpm -r test` still runs nothing. The only frontend test artefact is `clients/shells/web/a11y/axe-audit.mjs`, which targets a web shell that does not exist.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Add a test runner and a `test` script to every package under `ui/`, `products/*/ui` and `sdk/ts`
> - Write component tests with axe assertions for the design system and screen tests for the consoles
> - Add SDK contract tests against the real `http-gateway`
> - Add Playwright end-to-end flows against the compose enclosure in three browsers and two viewports
> - Add visual regression with a reviewed baseline
>
> Checked: `package.json:13` · `sdk/ts/package.json:11` · `clients/shells/web/a11y/axe-audit.mjs:1` · `pnpm-workspace.yaml:4`

**Evidence.** No `*.test.*`, `*.spec.*`, `vitest*`, `playwright*` or `*.stories.*` file exists anywhere. No `package.json` in the workspace defines a `test` script, so the root `pnpm -r test` runs nothing.

**Prompt**

```
Build the frontend test suite from nothing.

1. Component tests for all 28 design-system components: every variant, state,
   theme scope and mode; keyboard interaction; accessible name and role; and an
   axe-core assertion per component.
2. Screen tests for all 45 console screens: rendering under each of the five
   data states (RF-UI-2), interaction behaviour, and governance chrome
   correctness (markings, clearance, purpose).
3. Contract tests: the SDK client against a real server, asserting the client and
   server agree on every method, error and streaming behaviour.
4. End-to-end tests with Playwright covering every user flow in Workstream O,
   run against a real enclosure, in Chromium, Firefox and WebKit, at desktop and
   mobile viewports.
5. Accessibility tests: axe-core in component and E2E suites at zero-violation,
   plus keyboard-only traversal of every flow.
6. Visual regression for the design system and key screens, in light, dark and
   high-contrast modes, with a reviewed baseline.
7. Add a `test` script to every package so `pnpm -r test` actually runs
   something, and wire all of it into CI.
```

**Acceptance criteria**

- **AC-RF-TST-2a — `pnpm -r test` runs the suite.** *Given* the workspace, *when* the root test script runs, *then* every package's tests execute.
- **AC-RF-TST-2b — Every component and screen is covered.** *Given* the 28 components and 45 screens, *when* coverage is reported, *then* each has tests meeting the strategy's target.
- **AC-RF-TST-2c — Every flow passes E2E.** *Given* the Workstream O flows, *when* E2E runs against a real enclosure, *then* each completes in all three browsers at both viewports.
- **AC-RF-TST-2d — Zero accessibility violations.** *Given* the axe-core assertions, *when* CI runs, *then* no violation is reported.
- **AC-RF-TST-2e — Visual changes are reviewed.** *Given* a visual change, *when* CI runs, *then* it is flagged against the baseline and requires explicit approval.

**Done when:** component, screen, contract, E2E, accessibility and visual suites all exist and gate CI.

---

### RF-TST-3 · Integration testing across process boundaries

**Severity:** High · **Area:** Testing · **Depends on:** RF-RT-1, RF-API-2

> **Status at 2026-10-12 — Partial.** The plan's premise that there are no processes is no longer true: Rust tests spawn the real service binaries over real PQ-mTLS and durable state (`mimir-server`, `bifrost-fleet-server`, `heimdall-feed-server`, `forge-worker`), and `deploy/compose/smoke.sh`, `deploy/kind/smoke.sh` and the RC-40 deployment tier exercise the multi-process system. The acceptance suites (`verify-*.sh`) still run in-process library tests, failure coverage is limited to refusal and hard-kill restart, and there is no upgrade test.
> Criteria met 1 of 4 (a); open: b, c, d. Confidence: medium.
>
> **Scope change:** Rust tests already spawn real service binaries over PQ-mTLS with durable state, and compose, kind and deployment-tier smokes exist; criterion a is met. Rewrite around re-running the acceptance criteria against that system, a failure matrix and upgrade testing.
>
> **Still to do:**
> - Re-run the acceptance criteria against the multi-process enclosure and record any that fail
> - Add integration tests for the cross-service operations not yet covered (action apply, policy commit, privilege grant, artefact promotion, agent run with tools)
> - Build the failure matrix: dependency unavailable, timeout, partial failure, retry and idempotency per crash point
> - Add a previous-release-to-current upgrade and migration test
> - Update the item's AC count (it assumes 72) to the current acceptance matrix
>
> Checked: `mimir/mimir-server/tests/serve.rs:177` · `bifrost/fleet-server/tests/serve.rs:182` · `deploy/compose/smoke.sh:13` · `deploy/kind/smoke.sh:1` · `deploy/acceptance/run-airgapped.sh:35`

**Prompt**

```
Test the system as a system, not as a set of libraries.

1. Provide a test harness that starts the real services as real processes with
   real PQ-mTLS between them, real storage, and real configuration — fast enough
   to run in CI and deterministic enough to be trusted.
2. Write integration tests for every cross-service operation: action apply,
   policy commit, privilege grant, model registration and load, artefact
   promotion, rollout step, pipeline run and projection, feed ingest to picture,
   agent run with tool execution, and every governed path's audit write.
3. Test the failure paths, not only the happy ones: dependency unavailable,
   timeout, partial failure, retry, idempotency, and each crash point from the
   consistency model (RF-PER-7).
4. Test upgrade and migration: run the previous version, upgrade to the current,
   assert data and behaviour are correct — every release.
5. Re-run the 72 existing acceptance criteria against the integrated, multi-
   process system, not only against in-process libraries. This is the single
   highest-value test change in this workstream: it converts the acceptance
   suite from a specification check into a system check.
```

**Acceptance criteria**

- **AC-RF-TST-3a — Real multi-process harness.** *Given* the harness, *when* started, *then* real services run in separate processes with real PQ-mTLS and real storage.
- **AC-RF-TST-3b — All 72 ACs pass integrated.** *Given* the acceptance suite, *when* run against the multi-process system, *then* all 72 pass — and any that do not are recorded as findings against this plan.
- **AC-RF-TST-3c — Failure paths are covered.** *Given* the failure matrix, *when* each scenario is injected, *then* the documented behaviour occurs.
- **AC-RF-TST-3d — Upgrade is tested every release.** *Given* the previous release, *when* upgraded to the current, *then* data and behaviour are correct.

**Done when:** the harness exists, the 72 ACs pass against it, and the failure and upgrade matrices are green.

---

### RF-TST-4 · Fuzzing infrastructure

**Severity:** High · **Area:** Testing · **Depends on:** RF-SEC-2

> **Status at 2026-10-12 — Open.** No fuzzing infrastructure of any kind exists: no fuzz directories, targets, corpora, nightly job, triage or replay suite, and no documentation on adding a target.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Add target scaffolding and minimised checked-in corpora
> - Add a scheduled job with per-target time budget and coverage reporting
> - Add crash triage producing a minimised deterministic reproducer
> - Add a regression suite replaying reproducers on every build and a check that flags a parser with no target
> - Document how to add a target
>
> Checked: `justfile:151` · `CONTRIBUTING.md:26`

**Prompt**

```
Stand up the fuzzing infrastructure RF-SEC-2 requires: target scaffolding,
persistent corpora checked into the repo (minimised), a nightly CI job with a
per-target time budget, crash triage that files an issue with a minimised
reproducer, coverage reporting per target with a threshold, and a regression
suite that replays every historical finding on every build. Document how to add
a target so a new parser is fuzzed by default.
```

**Acceptance criteria**

- **AC-RF-TST-4a — Nightly job runs and reports.** *Given* the nightly job, *when* it completes, *then* every target ran for its budget and coverage is reported.
- **AC-RF-TST-4b — Crashes are actionable.** *Given* a crash, *when* triage runs, *then* an issue is filed with a minimised reproducer that fails deterministically.
- **AC-RF-TST-4c — Regressions replay.** *Given* every build, *when* the regression suite runs, *then* all historical reproducers pass.
- **AC-RF-TST-4d — New parsers are fuzzed by default.** *Given* a new parser added without a target, *when* CI runs, *then* the omission is detected.

**Done when:** nightly fuzzing runs with triage, coverage thresholds and a replaying regression suite.

---

### RF-TST-5 · Performance and load testing

**Severity:** High · **Area:** Testing · **Depends on:** RF-SCL-4, RF-TST-3

> **Status at 2026-10-12 — Open.** No workload models, data generators, load, stress, soak or spike tests, benchmarks or published performance envelope exist. The only performance assertion found is the 800 ms first-token check in the RC-40.4 deployment probe, which needs a GPU cluster.
> Criteria met 0 of 4; open: a, d; not verifiable by reading: b, c. Confidence: high.
>
> **Still to do:**
> - Build per-product and mixed workload models and realistic data generators
> - Run load, stress and 24-hour soak against a deployed enclosure and record results per release
> - Publish the performance envelope and record the breaking point and failure mode
> - Gate performance regressions at a stated tolerance
>
> Checked: `deploy/acceptance/deployment/run-deployment-tier.sh:212` · `deploy/acceptance/deployment/README.md:25`

**Prompt**

```
Measure the system under realistic load.

1. Build representative workload models per product: analyst query mix, ingestion
   volume and pipeline throughput, fusion event rate and track count, inference
   request mix and concurrency, admin and audit activity, and client sync
   patterns — including the mixed workload where they run simultaneously, which
   is where contention actually appears.
2. Generate realistic data at scale: 10⁶–10⁸ objects, 10⁷–10⁹ links, 10⁵ tracks,
   large datasets and long audit ledgers, with realistic distributions rather
   than uniform synthetic data.
3. Run load, stress (find the breaking point), soak (24h+ for leaks and
   degradation) and spike tests, and record results per release.
4. Publish the performance envelope (RF-DEP-7) and gate regressions in CI at a
   stated tolerance.
5. Profile to find bottlenecks and record them honestly in the scaling
   documentation.
```

**Acceptance criteria**

- **AC-RF-TST-5a — Realistic workloads run.** *Given* the workload models including the mixed case, *when* executed against a deployed enclosure at scale, *then* results are recorded per release.
- **AC-RF-TST-5b — Breaking point is known.** *Given* the stress test, *when* run, *then* the breaking point is identified and the failure mode is graceful (shed load, not corruption).
- **AC-RF-TST-5c — Soak is clean.** *Given* a 24-hour soak, *when* it completes, *then* memory, descriptors and latency are stable within tolerance.
- **AC-RF-TST-5d — Regressions gate.** *Given* a change regressing performance beyond tolerance, *when* CI runs, *then* it fails.

**Done when:** the workload suite runs per release, the envelope is published, and regressions gate CI.

---

### RF-TST-6 · Long-running stability and resource testing

**Severity:** Medium · **Area:** Testing · **Depends on:** RF-RT-6

> **Status at 2026-10-12 — Open.** No multi-day stability run, resource-profile assertions or clock-crossing tests exist. Restart-recovery tests (`durable_restart.rs` in several crates) prove ledger recovery after a restart but not behaviour over time.
> Criteria met 0 of 4; open: d; not verifiable by reading: a, b, c. Confidence: medium.
>
> **Still to do:**
> - Build a long-run harness on the compose or kind enclosure with representative activity and memory, descriptor and latency sampling
> - Trigger rotation, compaction, backup, certificate renewal, key rotation and token expiry during the run and assert SLOs
> - Verify the audit chain from genesis at the end of the run
> - Add clock-crossing tests (DST, leap second, month and year boundaries) for expiry, windows and retention
> - Execute and record a 7-day run
>
> Checked: `services/governance/tests/durable_restart.rs:1` · `services/iam/tests/idp.rs:102`

**Prompt**

```
Prove the system survives time, not only load.

Run multi-day continuous operation with representative activity, asserting: flat
memory and descriptor profiles; no unbounded structure growth (RF-RT-6); stable
latency; correct behaviour across log rotation, compaction, scrubbing, backup,
certificate renewal, key rotation and token expiry; and a verifiable audit chain
at the end. Include a clock-crossing test (daylight saving, leap second handling,
month and year boundaries) because time bugs surface only in long runs.
```

**Acceptance criteria**

- **AC-RF-TST-6a — Multi-day run is stable.** *Given* a 7-day run, *when* it completes, *then* memory, descriptors and latency are flat within tolerance and no restart occurred.
- **AC-RF-TST-6b — Background operations do not disturb.** *Given* rotation, compaction, scrubbing, backup, renewal and rotation events during the run, *when* each occurs, *then* service continues within its SLO.
- **AC-RF-TST-6c — Chain verifies at the end.** *Given* the run's audit ledger, *when* verified from genesis, *then* it verifies.
- **AC-RF-TST-6d — Time boundaries are safe.** *Given* the clock-crossing scenarios, *when* they occur, *then* no expiry, window or retention behaves incorrectly.

**Done when:** a 7-day run completes clean with a verifiable chain.

---

### RF-TST-7 · Regression, benchmark and golden-output suites

**Severity:** Medium · **Area:** Testing · **Depends on:** RF-TST-1

> **Status at 2026-10-12 — Open.** No golden-output files or review-on-change mechanism, no benchmarks (no `[[bench]]`, criterion or trend reporting), no dedicated governance regression suite and no cross-version compatibility tests were found. Governance properties are covered piecemeal by per-crate acceptance tests run through `verify-*.sh`, with no published CI to run them per PR.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Still to do:**
> - Add golden outputs with explicit accept-on-review for assembled prompts, chat templates, SBOM, DSSE attestation, audit record and wire encoding
> - Collect the governance tests (deny-by-default, clearance, markings, gates, dual control, fail-closed) into one named suite run on every PR
> - Add benchmarks with tolerances and trend reporting
> - Add previous-SDK versus current-server and current-SDK versus previous-server tests
>
> Checked: `justfile:151` · `crypto/policy/tests/pdp.rs:65` · `admin/orchestrator/tests/rc_31.rs:150`

**Prompt**

```
Protect the behaviours that must not change.

1. Golden outputs with review-on-change for: assembled prompts per family
   (RF-MI-2), chat templates, tokenizer id sequences, SBOM output, DSSE
   attestation structure, audit record structure, and the wire protocol encoding.
   A change to any of these is meaningful and must be seen, not slipped through.
2. Benchmarks in CI with per-benchmark tolerances and trend reporting, so a slow
   drift is caught rather than only a sudden regression.
3. A dedicated governance regression suite: deny-by-default, clearance filtering,
   marking propagation, human gates, dual control, audit completeness and
   fail-closed behaviours — run on every PR, because these are the properties
   whose silent regression would be most damaging.
4. Backward-compatibility tests: the current server against the previous SDK, and
   the current SDK against the previous server, within the support window.
```

**Acceptance criteria**

- **AC-RF-TST-7a — Golden changes are reviewed.** *Given* a change to any golden output, *when* CI runs, *then* it fails until the new golden is explicitly reviewed and accepted.
- **AC-RF-TST-7b — Governance regressions are caught on every PR.** *Given* a change weakening any listed governance property, *when* CI runs, *then* it fails.
- **AC-RF-TST-7c — Compatibility holds.** *Given* the support window, *when* cross-version tests run, *then* each supported pairing works.
- **AC-RF-TST-7d — Drift is visible.** *Given* benchmark trends, *when* reviewed, *then* gradual degradation is detectable before it crosses the tolerance.

**Done when:** golden, benchmark, governance-regression and compatibility suites all run in CI.

---

### RF-TST-8 · Chaos and fault-injection testing

**Severity:** High · **Area:** Testing · **Depends on:** RF-RES-3, RF-TST-3

> **Status at 2026-10-12 — Partial.** Process-level tests hard-kill services and assert they restart on the same sealed ledger, and individual fail-closed tests exist for expired tokens and tickets, but there is no chaos suite: no network partition, disk, clock, memory or CPU fault injection, no recorded hypotheses and no continuous run.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Scope change:** Hard-kill-and-recover tests exist in the process-level suites. Read this item as a fault-injection harness on the compose and kind enclosures.
>
> **Still to do:**
> - Build a fault-injection harness on the compose or kind enclosure covering kill at step boundaries, partition, loss, latency, disk full/slow/error, clock skew, memory and CPU pressure
> - Write each experiment with a hypothesis tied to the degradation matrix and record outcomes
> - Assert audit-chain verification and store integrity after every fault
> - Add security faults: unavailable audit ledger, key revoked mid-operation, compromised workload identity, expired credentials
> - Run it continuously in a non-production environment and convert divergences into permanent tests
>
> Checked: `mimir/mimir-server/tests/serve.rs:177` · `heimdall/feed-server/tests/serve.rs:187` · `services/iam/tests/idp.rs:102` · `services/gateway/tests/session.rs:366` · `deploy/compose/smoke.sh:24`

**Prompt**

```
Verify the failure behaviour the resilience model claims.

1. Fault injection: process kill (SIGKILL at every step boundary from the
   consistency model), network partition, packet loss and latency, disk full,
   disk slow, disk error, clock skew and jump, memory pressure, CPU starvation,
   dependency unavailability and dependency slowness.
2. Chaos experiments with an explicit hypothesis each time ("if the PDP is lost,
   all authorisation denies and the platform reports degraded"), executed against
   a real deployment, with the result recorded and any divergence from the
   documented behaviour treated as a defect in the system or the documentation.
3. Run continuously in a non-production environment, not as a one-off exercise.
4. Include security-relevant faults: an unavailable audit ledger, a revoked key
   mid-operation, a compromised workload identity, and expired credentials.
5. Record every experiment and outcome, and convert each discovered divergence
   into a permanent test.
```

**Acceptance criteria**

- **AC-RF-TST-8a — Every fault class is exercised.** *Given* the fault list, *when* the chaos suite runs, *then* each is injected and the behaviour recorded.
- **AC-RF-TST-8b — Behaviour matches documentation.** *Given* each experiment's hypothesis, *when* it runs, *then* the observed behaviour matches the degradation matrix; a divergence is a tracked defect.
- **AC-RF-TST-8c — Data integrity survives.** *Given* every fault, *when* the system recovers, *then* the audit chain verifies and no store is corrupted.
- **AC-RF-TST-8d — Security faults fail closed.** *Given* an unavailable ledger, a revoked key or expired credentials, *when* operations are attempted, *then* each fails closed.

**Done when:** the chaos suite runs continuously and every hypothesis is confirmed or the divergence is fixed.

---
# V — Compliance & assurance artefacts

> **Status.** `BUILD_PLAYBOOK.md` Appendix D lists the governance standards the suite targets — arc42/TOGAF, NIST 800-53, OWASP ASVS, ISO 27001 — and individual `[STD:*]` tags appear in source. There is no control mapping, no evidence collection, and no compliance artefact anywhere in the repository. For a platform whose value proposition is sovereignty and governance, this is a substantive gap.

---

### RF-CMP-1 · Control framework mapping with automated evidence

**Severity:** High · **Area:** Compliance · **Depends on:** RF-OBS-1, RF-PER-2

> **Status at 2026-10-12 — Partial.** The plan's status line (no control mapping anywhere) is stale: `docs/compliance/COMPLIANCE_MATRIX.md` is a 66-line self-attestation mapping about eight NIST 800-53 control groups, three OWASP ASVS chapters and a one-paragraph ISO 27001 summary to crates, and `docs/compliance/RESIDUAL_RISK_REGISTER.md` plus `GA_READINESS.md` record SAD residual risks and a NO-GO release decision. There is no SOC 2 mapping, no per-control test reference or shared-responsibility column, no baseline coverage (moderate/high, ASVS L2/L3, full Annex A), no generated evidence report, and no build check tying controls to tests. The files live in `docs/compliance/`, not the `docs/dev/compliance/` path the prompt names.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** `docs/compliance/COMPLIANCE_MATRIX.md`, `RESIDUAL_RISK_REGISTER.md` and `GA_READINESS.md` exist. Read this item as: extend the matrix, add SOC 2, generate evidence, and add a control-gap register. Settle the path (`docs/compliance/`, not `docs/dev/compliance/`).
>
> **Still to do:**
> - Expand the matrix to complete per-control mappings for NIST 800-53 moderate/high, ISO 27001 Annex A, ASVS L2/L3 and add SOC 2 TSC, each with code location, proving test and shared-responsibility statement
> - Build an evidence-report generator fed by test results, audit-ledger queries and configuration state, and regenerate it per release
> - Create a control gap register with owner, plan and date per unimplemented or partial control (the residual-risk register tracks SAD risks, not controls, and has no owners or dates)
> - Add a build check that fails and names the control when a control's evidence-generating test is removed
> - Decide the canonical location (docs/compliance vs docs/dev/compliance) and update the item text
>
> Checked: `docs/compliance/COMPLIANCE_MATRIX.md:21` · `docs/compliance/COMPLIANCE_MATRIX.md:42` · `docs/compliance/RESIDUAL_RISK_REGISTER.md:99` · `docs/compliance/GA_READINESS.md:10`

**Prompt**

```
Map the platform's controls to the frameworks it claims, and collect the evidence
automatically.

1. Produce control mappings in docs/dev/compliance/: NIST SP 800-53 (moderate and
   high baselines), ISO/IEC 27001 Annex A, OWASP ASVS L2/L3, and SOC 2 Trust
   Services Criteria. For each control state: how the platform implements it,
   where in the code, which test proves it, and what the deploying organisation
   must do (because many controls are shared responsibility — say so explicitly
   rather than implying the platform covers them).
2. Automate evidence: generate a control-evidence report from CI results, test
   outcomes, audit ledger queries and configuration state, so an assessment is a
   report generation rather than a scramble. Regenerate per release.
3. Gap register: controls not implemented, partially implemented, or dependent on
   the deployment, each with an owner, a plan and a date. An honest gap register
   is worth more than an optimistic mapping — assessors find the gaps anyway.
4. Traceability: extend RF-SC-6's matrix so a control maps to requirements,
   implementation, tests and evidence in one generated view.
5. Keep it current: a control whose evidence-generating test disappears must fail
   the build.
```

**Acceptance criteria**

- **AC-RF-CMP-1a — Mappings exist for all four frameworks.** *Given* the compliance documentation, *when* reviewed, *then* each framework has a complete mapping with implementation, code location, test and shared-responsibility statement per control.
- **AC-RF-CMP-1b — Evidence is generated, not assembled by hand.** *Given* a release, *when* the evidence report is produced, *then* it derives from CI results, tests, audit queries and configuration state automatically.
- **AC-RF-CMP-1c — Gaps are registered honestly.** *Given* the gap register, *when* reviewed, *then* every unimplemented or partial control appears with an owner and a date.
- **AC-RF-CMP-1d — Evidence cannot silently disappear.** *Given* a removed evidence-generating test, *when* CI runs, *then* it fails and names the affected control.

**Done when:** all four mappings exist with generated evidence and an honest gap register.

---

### RF-CMP-2 · Software supply-chain attestation (SLSA) and SBOM distribution

**Severity:** High · **Area:** Compliance · **Depends on:** RF-SC-1..5

> **Status at 2026-10-12 — Partial.** The provenance machinery exists: `crypto/attest` emits in-toto Statement v1 attestations, `mg-verify` fails closed against explicit trust anchors with a test suite, and the rc.1 notes give a one-line verify command per artefact. There is no SLSA level document, no VEX output (`supply-chain/advisories.toml` is an empty internal exception list, not VEX), no standalone consumer verification guide, and no third-party assessment. Nothing has been published: `deploy/trust/` holds only a README (no `.pub` anchors), there are no tags, and the release workflow is not in the public tree.
> Criteria met 0 of 4; open: a, c; not verifiable by reading: b, d. Confidence: high.
>
> **Still to do:**
> - Write the SLSA document: target level and a met/not-met statement per requirement (hermetic, provenance, isolated, parameterless, reproducible), including the software-key and unrun-workflow shortfalls
> - Emit CycloneDX VEX statements alongside each SBOM for advisories judged not applicable
> - Write a consumer verification guide covering artefact, SBOM and provenance using only published material
> - Run the key ceremony, commit the trust anchors and publish a release so third-party verification can actually be attempted
> - Commission the third-party pipeline assessment and record findings
>
> Checked: `crypto/attest/src/attestation.rs:17` · `tools/mg-verify/src/main.rs:13` · `deploy/trust/README.md:28` · `supply-chain/advisories.toml:35` · `docs/releases/v0.1.0-rc.1.md:78`

**Prompt**

```
Make the supply-chain claims externally assessable.

1. Target and evidence a specific SLSA build level: document what the pipeline
   provides against each requirement (hermetic, provenance, isolated,
   parameterless, reproducible) and where it falls short, honestly. Reproducible
   builds and vendored dependencies put a high level within reach — claim
   precisely what is achieved.
2. Publish provenance: DSSE/in-toto attestations (RF-SC-1) accompany every
   artefact and are verifiable by a third party using only public information and
   the trust anchors.
3. Distribute SBOMs: publish them alongside releases in CycloneDX, with VEX
   statements for advisories that do not apply, so downstream consumers are not
   forced to triage irrelevant findings.
4. Provide a consumer verification guide: exactly how a downstream organisation
   verifies an artefact, its SBOM and its provenance without trusting the
   publisher's word.
5. Third-party assessment: have the pipeline assessed against the claimed level
   and remediate findings.
```

**Acceptance criteria**

- **AC-RF-CMP-2a — Level is claimed precisely.** *Given* the SLSA documentation, *when* compared with the pipeline, *then* every requirement is addressed with an accurate met/not-met statement.
- **AC-RF-CMP-2b — Third-party verification works.** *Given* only the published artefacts, attestations and trust anchors, *when* an independent party follows the verification guide, *then* verification succeeds.
- **AC-RF-CMP-2c — VEX accompanies SBOMs.** *Given* an advisory that does not apply, *when* the release is published, *then* a VEX statement explains why.
- **AC-RF-CMP-2d — Assessment completed.** *Given* the third-party assessment, *when* it concludes, *then* findings are remediated or documented as accepted.

**Done when:** the level is claimed accurately, third-party verification is demonstrated, and VEX is published.

---

### RF-CMP-3 · Privacy: data protection by design and by default

**Severity:** High · **Area:** Compliance · **Depends on:** RF-PER-6

> **Status at 2026-10-12 — Partial.** Purpose limitation is a real runtime control: the PDP has a purpose gate that denies before discretionary rules, and the HTTP edge binds one purpose to the session token, both with tests. `docs/compliance/DATA_RESIDENCY.md` describes residency as a structural no-egress property and assigns controller duties to the operator. No personal-data inventory, DPIA, data-subject-rights workflow, residency constraint in policy, or retention concept exists in code (no occurrence of retention, erasure, crypto-shredding or residency in any Rust source outside vendor).
> Criteria met 1 of 5 (c); open: a, b, d, e. Confidence: medium.
>
> **Still to do:**
> - Add a personal-data marker to the type registry and generate the data inventory and flow map from it
> - Implement and audit the six data-subject-rights workflows (erasure depends on RF-PER-6 crypto-shredding, which does not exist)
> - Add a residency constraint to policy and block violating transfers (bundle export, picture-sync, DR replication), with a test
> - Introduce a retention setting with a conservative non-indefinite default for every data class
> - Write the DPIA for cross-source fusion, AI-assisted decisions and behavioural audit
>
> Checked: `crypto/policy/src/pdp.rs:68` · `crypto/policy/tests/pdp.rs:153` · `services/http-gateway/tests/fabric_edge.rs:347` · `services/http-gateway/src/enclosure.rs:658` · `docs/compliance/DATA_RESIDENCY.md:35`

**Prompt**

```
Address privacy as a design property, not a policy document.

1. Data inventory and flow mapping: every category of personal data the platform
   may process, where it enters, where it is stored, who may access it, where it
   flows, and how long it is kept. Generated from the type registry and the
   marking model where possible so it stays current.
2. Data-protection impact assessment covering the high-risk processing this
   platform enables: fusion of personal data across sources, AI-assisted
   decisions affecting individuals, and audit of individual behaviour.
3. Data-subject rights: access, rectification, erasure (via crypto-shredding,
   RF-PER-6), restriction, portability and objection — each with a defined,
   implemented, audited workflow and a defensible response time.
4. Minimisation and purpose limitation enforced technically: the purpose bound to
   a session (RF-IAM-3) constrains which data classes are reachable, so purpose
   limitation is a runtime control rather than a promise.
5. Cross-border and residency: where data may be stored and processed, enforced
   by policy and by deployment topology, with transfers blocked rather than
   logged after the fact.
6. Retention defaults that are conservative, and a default that is never
   "indefinite".
```

**Acceptance criteria**

- **AC-RF-CMP-3a — Inventory is current and generated.** *Given* a new object type carrying personal data, *when* the inventory is regenerated, *then* it appears without manual editing.
- **AC-RF-CMP-3b — Rights workflows work end to end.** *Given* each data-subject right, *when* exercised, *then* the workflow completes within the defined time and is audited.
- **AC-RF-CMP-3c — Purpose limitation is enforced at runtime.** *Given* a session bound to one purpose, *when* data outside that purpose is requested, *then* it is denied.
- **AC-RF-CMP-3d — Residency is enforced.** *Given* a residency constraint, *when* a transfer would violate it, *then* the transfer is blocked, not merely recorded.
- **AC-RF-CMP-3e — No indefinite default.** *Given* any new data class, *when* created without an explicit retention setting, *then* a conservative default applies.

**Done when:** the inventory generates, rights workflows are implemented, and purpose and residency are runtime-enforced.

---

### RF-CMP-4 · Records management, retention and legal hold as a compliance function

**Severity:** Medium · **Area:** Compliance · **Depends on:** RF-PER-6, RF-PER-2

> **Status at 2026-10-12 — Open.** Nothing exists: there is no records schedule, legal-hold mechanism, disposal certificate, eDiscovery export or written reconciliation of the append-only ledger with erasure. No Rust source outside vendor mentions retention, legal hold, disposal or erasure; the only related statement is a passing reference to processor-side deletion paths in `docs/compliance/DATA_RESIDENCY.md`.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Write the records schedule (driver, period, disposal method, authority per record type) keyed to the RF-CMP-3 inventory
> - Implement legal hold with dual control and audit that overrides every retention and erasure path, with tests
> - Implement signed, independently verifiable disposal certificates
> - Implement eDiscovery search/preserve/export with a chain-of-custody record
> - Document how the append-only ledger coexists with erasure via crypto-shredding in docs/compliance
>
> Checked: `docs/compliance/DATA_RESIDENCY.md:56` · `docs/compliance/COMPLIANCE_MATRIX.md:27`

**Prompt**

```
Complete the records-management story around the append-only ledger.

1. Records schedule: classify every record type by its retention driver
   (regulatory, contractual, operational, evidentiary), with its period, disposal
   method and the authority for each.
2. Legal hold: apply, track, release and report holds by subject, matter or
   range, with dual control, full audit, and an override of every retention and
   erasure path (RF-PER-6d).
3. Defensible disposal: when data is disposed, produce a signed disposal
   certificate recording what, when, under what authority and by what method —
   so disposal is evidenced rather than merely done.
4. eDiscovery: search, preserve and export by subject or matter, producing a
   verifiable evidence package (RF-PER-2g) with a chain-of-custody record.
5. Reconcile with immutability: state clearly, in the compliance documentation,
   how an append-only ledger coexists with erasure obligations, and why
   crypto-shredding satisfies both. Assessors will ask; the answer should be
   written down before they do.
```

**Acceptance criteria**

- **AC-RF-CMP-4a — Schedule covers everything.** *Given* the records schedule, *when* compared with the data inventory, *then* every record type has a retention driver, period, disposal method and authority.
- **AC-RF-CMP-4b — Holds override everything.** *Given* an active hold, *when* retention, archival or erasure would act, *then* each is blocked and the block is recorded.
- **AC-RF-CMP-4c — Disposal is certified.** *Given* a disposal, *when* it completes, *then* a signed certificate exists and is independently verifiable.
- **AC-RF-CMP-4d — eDiscovery produces defensible packages.** *Given* a matter, *when* an export is produced, *then* it carries a chain-of-custody record and verifies independently.

**Done when:** the schedule, hold mechanism, disposal certificates and eDiscovery export all exist and are exercised.

---

### RF-CMP-5 · Export control, sanctions and jurisdictional constraints

**Severity:** Medium · **Area:** Compliance · **Depends on:** RF-MS-2, RF-API-6

> **Status at 2026-10-12 — Open.** No export-control classification, distribution record, sanctions process or jurisdictional restriction on models or data exists; a search of the tree outside vendor for export control, ECCN, Wassenaar or sanctions finds nothing except this plan. The repository is already public under Apache-2.0 with pure-Rust FIPS 203/204/205 and AES implementations, so the classification question is live rather than hypothetical.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Still to do:**
> - Document the export classification, its basis and distribution obligations (including any notification duty for publicly available cryptographic source) in docs/compliance
> - Add jurisdiction restrictions to model and dataset manifests and refuse and audit restricted transit at bundle pack/import and egress
> - Define what distribution records are kept, if any are required for the chosen distribution model
> - Publish the deploying organisation's obligations and the responsibility boundary
>
> Checked: `NOTICE:1` · `docs/compliance/COMPLIANCE_MATRIX.md:9` · `docs/compliance/DATA_RESIDENCY.md:51`

**Prompt**

```
Address the constraints a sovereign, cryptography-heavy platform inevitably meets.

1. Export classification: classify the software (strong cryptography, and
   potentially dual-use given the operational-fusion capability) under the
   relevant regimes, and document the classification, the basis, and the
   obligations it creates for distribution.
2. Distribution controls: where required, restrict downloads, publish the
   notices, and record what was distributed to whom.
3. Model and data export: models and derived data may carry their own
   restrictions (RF-MS-2); enforce them at transit and at egress, including for
   air-gapped bundle transport across jurisdictions.
4. Sanctions screening for the deployment and distribution processes, where the
   distribution model requires it.
5. Document what a deploying organisation must do — much of this is their
   obligation, and the documentation must be clear about the boundary rather than
   implying the platform discharges it.
```

**Acceptance criteria**

- **AC-RF-CMP-5a — Classification is documented.** *Given* the export documentation, *when* reviewed, *then* the classification, its basis and the resulting obligations are stated.
- **AC-RF-CMP-5b — Restricted transit is blocked.** *Given* a model or dataset with a jurisdictional restriction, *when* transit across the boundary is attempted, *then* it is refused and audited.
- **AC-RF-CMP-5c — Distribution is recorded.** *Given* controlled distribution, *when* it occurs, *then* the record satisfies the regime's requirements.
- **AC-RF-CMP-5d — The responsibility boundary is clear.** *Given* the documentation, *when* read by a deploying organisation, *then* their obligations are unambiguous.

**Done when:** classification is documented, jurisdictional enforcement works at transit, and the responsibility boundary is published.

---

### RF-CMP-6 · Accessibility conformance reporting

**Severity:** Medium · **Area:** Compliance · **Depends on:** RF-UI-3, RF-CL-2..7

> **Status at 2026-10-12 — Partial.** The Evidence is partly stale: besides the declaration check there is now `deploy/acceptance/verify-shell-audits.sh`, a fail-closed per-release gate that requires a recorded on-device audit for each shell with every declared affordance measured as pass, plus `clients/shells/AUDIT_SCHEMA.md`, an axe script for the web shell and AT-SPI/Tauri helper scripts. No audit records exist (the `audits/` directories are empty), the workflow that would write them (`.github/workflows/shell-audit.yml`) is not in the public tree, there is no VPAT/ACR, and the seven consoles under `products/*/ui` have no accessibility tests or tooling at all.
> Criteria met 0 of 4; open: a, b, c; not verifiable by reading: d. Confidence: medium.
>
> **Scope change:** `deploy/acceptance/verify-shell-audits.sh` is a fail-closed gate on recorded on-device audits. The gaps are that no audit records exist and the workflow that writes them is not in the tree. There are seven console packages, not six.
>
> **Still to do:**
> - Run the on-device audits and commit records so the audit gate can pass for a release
> - Make each accessibility.toml affordance name the test that demonstrates it and fail when a declaration has no test (the current gate matches recorded results, not tests)
> - Add axe-core and keyboard/screen-reader tests to the consoles (there are seven console packages, not six) and include them in the report
> - Publish a WCAG 2.2 AA ACR with per-criterion statements and named evidence
> - Publish an accessibility feedback and remediation path with response commitments
>
> Checked: `deploy/acceptance/verify-shell-audits.sh:3` · `deploy/acceptance/verify-shells-accessibility.sh:47` · `clients/shells/web/accessibility.toml:11` · `docs/compliance/GA_READINESS.md:83`

**Evidence.** `clients/shells/*/accessibility.toml` declares conformance for the six shells; `verify-shells-accessibility.sh` checks the declaration's completeness, not the behaviour. The six consoles have no accessibility claim, and would not currently support one.

**Prompt**

```
Produce real accessibility conformance evidence.

1. Publish a VPAT/ACR (Accessibility Conformance Report) covering the consoles
   and the six shells against WCAG 2.2 AA (and the relevant platform guidelines
   for native shells), with per-criterion conformance and honest "partially
   supports" entries where that is the truth.
2. Base it on evidence: automated results (axe-core), manual audit results,
   assistive-technology testing results, and the on-device platform suites
   (XCUITest, Espresso, Playwright, Tauri, Orca).
3. Replace declaration-checking with behaviour-checking: keep the
   accessibility.toml contracts, but make each declared affordance point at the
   test that demonstrates it, and fail the build when a declared affordance has
   no demonstrating test.
4. Publish a feedback and remediation path for accessibility issues, with
   response commitments.
5. Re-verify per release; a regression is a release blocker, not a backlog item.
```

**Acceptance criteria**

- **AC-RF-CMP-6a — ACR is published and evidenced.** *Given* the report, *when* reviewed, *then* every criterion has a conformance statement backed by named evidence.
- **AC-RF-CMP-6b — Declarations are demonstrated.** *Given* each `accessibility.toml` affordance, *when* the verification runs, *then* it points at a passing test; an undemonstrated declaration fails the build.
- **AC-RF-CMP-6c — Consoles are covered.** *Given* the six consoles, *when* the report is produced, *then* they are included, not only the shells.
- **AC-RF-CMP-6d — Regressions block release.** *Given* an accessibility regression, *when* detected, *then* the release is blocked.

**Done when:** an evidenced ACR covers consoles and shells, and declaration-checking is replaced by behaviour-checking.

---

# W — Multi-tenancy, scale & performance

> **Why.** Nothing in the platform models a tenant. Every store, index, cache, queue and metric is single-scope. Retrofitting tenancy after a system is in production is one of the most expensive changes there is, so the decision belongs early even if the implementation does not.

---

### RF-SCL-1 · Decide and implement the tenancy model

**Severity:** High · **Area:** Scale · **Depends on:** RF-PER-1, RF-IAM-5 · **Blocks:** RF-SCL-2..5

> **Status at 2026-10-12 — Open.** No tenancy ADR exists (there is no ADR directory at all) and no document states the tenancy model. The premise that nothing models a tenant is slightly wrong: `services/scheduler` carries a `TenantId` label on every workload (documented as a label over a shared quota pool), and the Mimir system prompt tells the model it serves one tenant, so the tree currently half-implies both answers.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** `services/scheduler/src/admission.rs` carries a `TenantId` label while the Mimir prompt says "one tenant". Name that as the ambiguity the ADR resolves.
>
> **Still to do:**
> - Write docs/dev/adr/0004-tenancy-model.md and decide the model
> - If single-tenant per enclosure: say so in README, SADs and deploy docs, and rename or remove scheduler `TenantId` so nothing implies multi-tenancy
> - If multi-tenant: implement isolation at every listed layer with a cross-tenant leakage suite and fault-injection variant
> - Define enclosure/tenant deletion with cryptographic assurance and a certificate
>
> Checked: `services/scheduler/src/admission.rs:35` · `services/scheduler/tests/rc_43.rs:14` · `mimir/prompts/src/lib.rs:27` · `deploy/helm/midgard/values.yaml:36`

**Prompt**

```
Decide tenancy before the storage layer sets in concrete.

1. Write an ADR at docs/dev/adr/0004-tenancy-model.md evaluating: single-tenant
   per enclosure (the simplest and arguably the most aligned with a sovereignty
   product), logical multi-tenancy with enforced isolation, and physical
   multi-tenancy. Consider the sovereignty positioning, the security review
   burden, the operational cost, and what customers will actually ask for.
2. If single-tenant is chosen, say so explicitly everywhere and ensure nothing in
   the API or data model half-implies tenancy — an ambiguous model is worse than
   either choice.
3. If multi-tenant, implement isolation at every layer: storage keyspace,
   indexes, caches, queues, metrics labels, logs, traces, audit ledgers, model
   memory and sessions, inference queues and rate limits. Add a
   cross-tenant-access test suite that attempts leakage at every one of those
   layers and asserts failure.
4. Per-tenant configuration, keys, quotas and retention, with a documented
   noisy-neighbour policy.
5. Tenant lifecycle: provisioning, suspension, migration between deployments, and
   deletion with cryptographic assurance and a verifiable certificate.
```

**Acceptance criteria**

- **AC-RF-SCL-1a — The model is decided and unambiguous.** *Given* the ADR and the implementation, *when* compared, *then* they agree and no API or data-model element implies a different model.
- **AC-RF-SCL-1b — No cross-tenant leakage.** *Given* the cross-tenant test suite, *when* it attempts leakage at every listed layer, *then* every attempt fails.
- **AC-RF-SCL-1c — Isolation survives failure.** *Given* faults injected during multi-tenant operation, *when* the system recovers, *then* no tenant's data appears in another's scope.
- **AC-RF-SCL-1d — Deletion is verifiable.** *Given* tenant deletion, *when* it completes, *then* the data is cryptographically unrecoverable and a certificate is issued.

**Done when:** the ADR is merged and either single-tenancy is explicit everywhere or the isolation suite is green.

---

### RF-SCL-2 · Horizontal scalability of the data plane

**Severity:** High · **Area:** Scale · **Depends on:** RF-PER-3, RF-SCL-1

> **Status at 2026-10-12 — Partial.** Only the claim-correction half has moved: the rc.1 notes and the Helm values now state one replica per service with a single-writer state directory, and `services/fabric/src/store.rs` describes `ObjectStore` as a seam for a future distributed backend with an in-memory reference (a `DurableObjectStore` also exists). The crate header still calls it a horizontally-scalable ObjectStore, and no partitioning, scatter-gather, rebalancing, consistency statement or scale test exists.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Reword `services/fabric/src/lib.rs` and store.rs so they say single-node today with a seam for a distributed backend, and add a scaling-limits section to the deploy docs
> - Decide whether distributed fabric is in scope before GA; if so design partitioning and cross-partition traversal
> - Implement scatter-gather with per-object authorisation at the partition and explicit partial-failure results
> - Implement rebalancing and state the consistency model
> - Run the 10^8 object / 10^9 link multi-node test with a node failure
>
> Checked: `services/fabric/src/lib.rs:11` · `services/fabric/src/store.rs:2` · `docs/releases/v0.1.0-rc.1.md:95` · `deploy/helm/midgard/values.yaml:36`

**Evidence.** `services/fabric` describes a "horizontally-scalable ObjectStore" that is a single-process map.

**Prompt**

```
Make the claim true, or correct the claim.

1. Define the partitioning strategy for the object fabric (by object type, by
   hash, by tenant, or hybrid) with its consequences for cross-partition queries
   and search-around traversal — traversal across partitions is the hard case and
   must be designed, not discovered.
2. Implement distributed query: scatter-gather with per-object authorisation
   applied at the partition (never centrally after collection, which would move
   unauthorised data across the wire), bounded fan-out, and partial-failure
   semantics that are explicit in the result.
3. Rebalancing: add and remove nodes with online data movement, bounded impact
   and no correctness loss.
4. Consistency: state and implement the model for cross-partition reads and
   writes, and be honest about what is not linearisable.
5. Prove it: 10⁸ objects and 10⁹ links across a multi-node deployment, meeting
   the published latency envelope, with a node failure during the test.
6. Where a component genuinely does not scale horizontally, document it in the
   scaling guidance rather than implying otherwise (RF-DEP-7).
```

**Acceptance criteria**

- **AC-RF-SCL-2a — Distributed scale is demonstrated.** *Given* a multi-node deployment, *when* loaded with 10⁸ objects and 10⁹ links, *then* queries and traversals meet the published envelope.
- **AC-RF-SCL-2b — Authorisation happens at the partition.** *Given* a distributed query, *when* traced, *then* unauthorised objects never leave their partition.
- **AC-RF-SCL-2c — Rebalancing is safe.** *Given* a node added or removed under load, *when* rebalancing completes, *then* no data is lost or duplicated and latency stayed within tolerance.
- **AC-RF-SCL-2d — Node failure is survivable.** *Given* a node failure mid-query, *when* it occurs, *then* the query either completes or reports explicit partial failure — never silently incomplete results.
- **AC-RF-SCL-2e — Claims match reality.** *Given* the documentation, *when* compared with measured behaviour, *then* every scalability claim is supported.

**Done when:** multi-node scale is demonstrated at the stated volumes with node failure, or the claims are corrected.

---

### RF-SCL-3 · Caching strategy with governance correctness

**Severity:** Medium · **Area:** Scale · **Depends on:** RF-PER-3, RF-SCL-1

> **Status at 2026-10-12 — Open.** There is no authorisation-result, query-result or policy-decision cache in the services, so nothing to key, invalidate or bound; no caching strategy document exists. The only server-side cache is the model server's KV/prefix cache, which the reference backend keys by a hash of the prompt prefix alone (no subject, clearance or purpose) and which changes first-token latency, so it is the one place the item's cross-principal and timing concerns already apply.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Scope change:** No authorisation or query cache exists. The only server-side cache is the model prefix cache, keyed by prompt prefix alone. Scope this to a caching strategy plus an assessment of that cache.
>
> **Still to do:**
> - Write the caching strategy: what may be cached at which layer, key composition, staleness bound as a reviewed security parameter
> - Assess the model KV/prefix cache for cross-principal reuse and timing leakage and either scope it by authorisation context or document why it is safe
> - When any cache is introduced, add the cross-principal leakage test, revocation-invalidation test and not-found versus not-permitted timing test
> - Bound and instrument every cache (size, eviction, hit-rate metrics)
>
> Checked: `mimir/model-server/src/backends/local.rs:34` · `mimir/model-server/src/backends/local.rs:116` · `mimir/model-server/tests/serving.rs:72` · `services/http-gateway/src/http.rs:168`

**Prompt**

```
Introduce caching without breaking the security model — the classic way a
governed platform leaks.

1. Identify what may be cached and at what layer: type registry, policy
   decisions, identity and session state, query results, projections, model
   outputs, and static assets.
2. Cache keys MUST include the full authorisation context — subject, roles,
   clearance, purpose, tenant — so a cached result can never be served to a
   principal who would not have been authorised to produce it. Add a test that
   attempts exactly this and fails.
3. Invalidation: a policy change, a role change, a clearance change, a marking
   change or a revocation invalidates every affected cache entry within a bounded
   and documented staleness — and that bound is a security parameter, reviewed as
   such.
4. Negative caching must not become an oracle: a cached denial must not reveal
   existence, and cache timing must not distinguish not-found from not-permitted.
5. Instrument hit rates, sizes and evictions (RF-OBS-1), and bound every cache
   (RF-RT-6).
```

**Acceptance criteria**

- **AC-RF-SCL-3a — Cached results never cross principals.** *Given* a result cached for one subject, *when* another requests the same resource, *then* they receive a result computed under their own authorisation; the leakage test fails to leak.
- **AC-RF-SCL-3b — Revocation invalidates promptly.** *Given* a clearance downgrade or a revocation, *when* the affected data is requested, *then* the new decision applies within the documented bound.
- **AC-RF-SCL-3c — No timing oracle.** *Given* a not-found and a not-permitted request, *when* timings are compared across cached and uncached paths, *then* they are indistinguishable.
- **AC-RF-SCL-3d — Caches are bounded and observed.** *Given* every cache, *when* inspected, *then* it has a bound, an eviction policy and metrics.

**Done when:** all caches are context-keyed, bounded and instrumented, and the leakage and oracle tests pass.

---

### RF-SCL-4 · Performance engineering and the published envelope

**Severity:** High · **Area:** Scale · **Depends on:** RF-TST-5

> **Status at 2026-10-12 — Open.** No benchmark, criterion or load-test code exists anywhere in the workspace, and there is no performance requirements or envelope document and no stated non-goals. The only measurements are two first-token figures from manual smoke runs in the rc.1 notes against an 800 ms target that is itself declared unproven on GPU; the gateway's SLO view returns hard-coded figures such as a 38 ms query p95.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** The observability view returns hard-coded SLO figures (`services/http-gateway/src/data.rs:226`); no measured performance figure exists anywhere.
>
> **Still to do:**
> - Write per-operation-class performance requirements and explicit non-goals
> - Build the benchmark suite for the critical paths (query, search-around, action apply, policy decision, audit write/verify, inference, ingest, sync)
> - Run it in CI with trend reporting and a regression gate (no CI definition is in the public tree)
> - Replace or clearly label the hard-coded SLO figures served by `services/http-gateway/src/data.rs`
> - Publish the measured envelope
>
> Checked: `docs/releases/v0.1.0-rc.1.md:50` · `docs/releases/v0.1.0-rc.1.md:90` · `services/http-gateway/src/data.rs:226` · `docs/compliance/RESIDUAL_RISK_REGISTER.md:31`

**Prompt**

```
Establish performance as an engineered property.

1. Define performance requirements per operation class from the user's
   perspective (interactive < 100 ms p95, query < 1 s p95, and so on), not from
   what the implementation happens to achieve.
2. Build a continuous benchmark suite covering every critical path: object query,
   search-around, action apply, policy decision, audit write and verify,
   inference time-to-first-token and throughput, pipeline throughput, projection,
   feed ingest, picture update, sync convergence, client startup and sync.
3. Profile and optimise the hot paths, recording each optimisation's measured
   effect so the work is evidence-driven.
4. Publish the envelope (RF-DEP-7) and gate regressions.
5. Set explicit non-goals: what the platform is not optimised for, so deployments
   are not surprised.
```

**Acceptance criteria**

- **AC-RF-SCL-4a — Requirements are user-derived and met.** *Given* the requirements, *when* the benchmark suite runs at the reference scale, *then* every operation class meets its target or the gap is a tracked item.
- **AC-RF-SCL-4b — Benchmarks run continuously.** *Given* CI, *when* it runs, *then* the critical-path benchmarks execute and report trends.
- **AC-RF-SCL-4c — Regressions gate.** *Given* a regression beyond tolerance, *when* CI runs, *then* it fails.
- **AC-RF-SCL-4d — Non-goals are published.** *Given* the performance documentation, *when* read, *then* it states what the platform is not optimised for.

**Done when:** requirements are defined and measured, benchmarks run continuously, and the envelope with non-goals is published.

---

### RF-SCL-5 · Cost model and resource efficiency

**Severity:** Low · **Area:** Scale · **Depends on:** RF-SCL-4, RF-MI-8

> **Status at 2026-10-12 — Open.** No cost model, per-product resource attribution, efficiency measurement or energy reporting exists in code, console or documentation; the only adjacent mechanism is the scheduler's CPU/memory/GPU quota admission. The criterion's per-tenant breakdown also presupposes an RF-SCL-1 decision that has not been made.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: medium.
>
> **Scope change:** Criterion b needs a per-tenant cost breakdown, which presupposes an RF-SCL-1 outcome that is undecided.
>
> **Still to do:**
> - Build a cost model per topology and workload and validate it against measured consumption
> - Instrument resource consumption per product and feature (and per tenant only if RF-SCL-1 chooses multi-tenancy)
> - Surface cost and efficiency in the Admin console
> - Report an energy/efficiency figure for the inference plane
> - Record before/after measurements for each efficiency change
>
> Checked: `services/scheduler/src/admission.rs:107` · `services/http-gateway/src/data.rs:224`

**Prompt**

```
Make the cost of running Midgard predictable.

Build a cost model per topology and workload covering compute, GPU, storage and
network; instrument actual resource consumption per tenant, product and feature
so cost is attributable; identify and address the largest inefficiencies with
measured before/after; and surface cost and efficiency in the Admin console
alongside capacity. Include the sustainability dimension — an enclosure running
GPU inference continuously has a real energy footprint, and operators will be
asked about it.
```

**Acceptance criteria**

- **AC-RF-SCL-5a — Cost is predictable.** *Given* a workload and topology, *when* the model is applied, *then* projected resource consumption is within a stated tolerance of the measured actual.
- **AC-RF-SCL-5b — Cost is attributable.** *Given* the Admin console, *when* cost is viewed, *then* it is broken down by tenant, product and feature.
- **AC-RF-SCL-5c — Optimisations are evidenced.** *Given* each efficiency change, *when* reviewed, *then* it carries before/after measurements.
- **AC-RF-SCL-5d — Energy is reported.** *Given* the operational reporting, *when* produced, *then* it includes an energy/efficiency figure for the inference plane.

**Done when:** the model predicts within tolerance and cost attribution is visible in the console.

---

# X — Documentation, design system & drift

> **Why this matters here more than usual.** This repository's primary artefact is a *specification-driven implementation*. Its credibility rests on documents and code agreeing. Several places where they do not agree are listed below — each is small, and collectively they undermine the thing that makes the project distinctive.

---

### RF-DOC-1 · Reconcile every status claim with the tree

**Severity:** Medium · **Area:** Documentation · **Depends on:** — (do this first; it is cheap and it is load-bearing)

> **Status at 2026-10-12 — Partial.** Of the twelve rows, four are fixed: the crate count (70), `mg-sign`/`mg-verify` now return real exit codes with a fail-closed test suite, `LICENSE` is tracked and matches the workspace `license = Apache-2.0`, and `go.work` and `pyproject.toml` have been deleted. Three still hold unchanged: `products/README.md` still lists Forge as Rust (DataFusion) and Bifrost/Admin as Go and says scaffold only; the SDK comments still call `HttpGateway` a stub and `MockGateway` the only implementation although `HttpGateway.ts` is 182 lines; `clients/shells/web` and `extension` still have no `package.json` while listed in `pnpm-workspace.yaml`. Three have inverted or moved: `deploy/helm` and `deploy/air-gap-bundle` now exist but `deploy/README.md` still says the bundle directory is not yet present and several docs cite `.github/workflows/release.yml`, `ci.yml` and `.github/CODEOWNERS`, none of which is in the public tree; `ui/README.md` says the Midgard console is wired to the mock gateway when it now defaults to HTTP with `VITE_GATEWAY=mock` as the opt-in, while the other consoles are mock-only and README line 794 says consoles bind to the gateway edge; the justfile still has `build-go` and `build-py` recipes and `sdk/README.md` advertises a `py/` SDK that does not exist. The 72/72 claim sits under the headline Implementation: complete and verified, qualified by a caveat list but not by any status vocabulary, and `services/README.md` and `clients/README.md` still say scaffold only. Brand: both token copies now say Veldris, but Kryotech remains in SAD_00, SAD_01, SAD_06 and one diagram source, and Akuma Engineering Ltd. remains in `docs/dev/README.md`, the SADs, `BUILD_PLAYBOOK.md` and `docs/design/readme.md`; no Rust source carries a licence header and no mechanical drift check exists.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Scope change:** The Evidence table below is superseded; the status note above lists which rows are fixed and which hold. New drift to add: documents cite `.github/workflows/release.yml`, `ci.yml` and `CODEOWNERS`, none of which is in the tree; the `justfile` keeps `build-go` and `build-py`; `sdk/README.md` advertises a Python SDK that does not exist; `deploy/README.md` says the air-gap bundle is not present when it is; `ui/README.md` says the Midgard console uses the mock gateway; no `.rs` file carries a licence header. Headline numbers are RF-CA-2.
>
> **Still to do:**
> - Rewrite `products/README.md`: all backends are Rust, no DataFusion, seven console packages exist, drop scaffold only; fix the same DataFusion and scaffold claims in `services/README.md` and `clients/README.md`
> - Update the SDK comments in `sdk/ts/src/index.ts` and `client.ts` to describe `HttpGateway` as a real client, and fix `sdk/README.md` (no `py/`, no generated `src/gen`)
> - Fix `ui/README.md` and README line 794 to state per console which data source it uses (Midgard: HTTP by default, others: mock)
> - Fix `deploy/README.md` (air-gap-bundle exists; trust anchors absent) and every reference to `.github/` files that the public tree does not contain
> - Remove `clients/shells/web` and `clients/shells/extension` from `pnpm-workspace.yaml` or give them a package.json; delete the `build-go` and `build-py` justfile recipes
> - Remove the remaining Kryotech strings (SAD_00/01/06, `docs/dev/diagrams/midgard_c4_containers.mmd`, and the derived docx/png) and decide what to do about Akuma Engineering Ltd. branding
> - Define the spec/reference/operable/production vocabulary and label every component README and the README status block with it, restating the 72/72 claim as reference-implementation, air-gapped
> - Decide the licence-header policy (no .rs file has one) and record it so criterion d can be closed; note d is otherwise satisfied
> - Add a mechanical drift check script under tools/ci (referenced directories exist, workspace members resolve, no Go claim without .go files) and wire it wherever CI is published
> - Rewrite the Evidence table: rows 1, 3, 4, 5, 9 and 11 are superseded and rows 3 and 7 now drift in the opposite direction
>
> Checked: `products/README.md:9` · `products/README.md:18` · `deploy/README.md:52` · `sdk/ts/src/index.ts:4` · `sdk/ts/src/client.ts:25` · `ui/README.md:16` · `products/midgard/ui/src/main.tsx:24` · `pnpm-workspace.yaml:8` · `tools/mg-verify/src/main.rs:23` · `README.md:22` · `docs/dev/markdown/SAD_00_Midgard_Core.md:78` · `justfile:23`

> **Revision 2.** The first row is superseded: the workspace has 70 members, and the README and GETTING_STARTED counts were corrected on `dev` on 2026-10-12. Every headline figure is now governed by [RF-CA-2](#rf-ca-2--one-numbers-sheet-generated-from-the-tag), whose CI check implements step 6 below for numbers. The other rows have not been rechecked.

**Evidence.** Verified drift, each independently checked:

| Claim | Location | Reality |
|---|---|---|
| "a 56-crate Rust workspace" | [`README.md:9`](../../../README.md), [`GETTING_STARTED.md:5`](../../../GETTING_STARTED.md) | `Cargo.toml` has **55** members |
| Forge backend "Rust (DataFusion)"; Bifrost and Admin backends "Go" | [`products/README.md`](../../../products/README.md) | No DataFusion dependency; **zero** `.go` files in the tree |
| `deploy/helm/` and `deploy/air-gap-bundle/` described as directories | [`deploy/README.md`](../../../deploy/README.md) | Neither exists |
| Python workspace members are `mimir/*` | [`pyproject.toml`](../../../pyproject.toml) | Those are Rust crates; `uv sync` has no real members |
| Go workspace for "compute-scheduler, Bifrost controller, admin backend" | [`go.work`](../../../go.work) | No modules; those components are Rust |
| `HttpGateway` is "a stub" and `MockGateway` the "only implementation" | [`sdk/ts/src/index.ts:5`](../../../sdk/ts/src/index.ts), [`sdk/ts/src/client.ts:25`](../../../sdk/ts/src/client.ts) | `HttpGateway` is a real 180-line client for the core surface |
| Consoles "bind to the gateway edge" | [`ui/README.md`](../../../ui/README.md) | Five of six bind to mocks; one is opt-in via `VITE_GATEWAY=http` |
| `clients/shells/web` and `clients/shells/extension` are pnpm workspace packages | [`pnpm-workspace.yaml`](../../../pnpm-workspace.yaml) | Neither has a `package.json`; both are silently skipped |
| `mg-sign`/`mg-verify` doc comments describe fail-closed behaviour | `tools/mg-sign/src/main.rs`, `tools/mg-verify/src/main.rs` | Both return `ExitCode::SUCCESS` unconditionally |
| "All 72 acceptance criteria pass" | [`README.md:11`](../../../README.md) | True for the in-process reference implementation; not yet demonstrated for a deployed, multi-process system (RF-TST-3b) |
| `LICENSE` | repository root | Present in the working tree but **untracked** in git |
| Brand: "Kryotech house red" | `docs/design/tokens/colors.css` and `ui/design-system/src/tokens/colors.css` | A Kryotech → Veldris rebrand is mid-flight in the working tree; both token copies were edited separately |

**Prompt**

```
Reconcile every claim in the repository with the repository.

1. Fix each row of the table above: correct the number, correct or implement the
   technology claim, create or stop referencing the directories, make the
   workspace declarations honest, update the stale SDK comments, and describe the
   consoles' actual data source.
2. Add a status vocabulary and use it consistently everywhere: `spec` (designed,
   not built), `reference` (built as an in-process reference implementation),
   `operable` (runs as a deployed service), `production` (operable plus the
   assurance in Workstreams S/T/U/V). Label every component in every README with
   one of these. Most of the tree is currently `reference` and is described in
   language that reads as `production`.
3. Be precise about the acceptance criteria claim: state that all 72 pass against
   the reference implementation air-gapped, and state what remains to demonstrate
   them against a deployed system.
4. Track the LICENSE file, and confirm the licence headers and the Cargo
   `license` field agree with it.
5. Complete the Kryotech → Veldris rebrand consistently, and make one token file
   generate the other (RF-DOC-6) so the same edit is never needed twice.
6. Add a CI check for the mechanical claims — crate count, referenced directories
   existing, workspace members resolving, no `.go` files while Go is claimed —
   so this class of drift cannot silently return.
```

**Acceptance criteria**

- **AC-RF-DOC-1a — Every row is resolved.** *Given* the drift table, *when* each row is rechecked, *then* the claim and the tree agree.
- **AC-RF-DOC-1b — Status vocabulary is applied.** *Given* any component README, *when* read, *then* it carries one of the four status labels and the label is accurate.
- **AC-RF-DOC-1c — Mechanical drift is caught.** *Given* a change that breaks a mechanical claim (crate count, referenced directory, workspace member), *when* CI runs, *then* it fails.
- **AC-RF-DOC-1d — LICENSE is tracked and consistent.** *Given* the repository, *when* inspected, *then* `LICENSE` is tracked and agrees with the Cargo `license` field and the source headers.
- **AC-RF-DOC-1e — Rebrand is complete.** *Given* the tree, *when* searched, *then* no stale brand reference remains.

**Done when:** every row is fixed, the status vocabulary is applied throughout, and the mechanical CI check is green.

---

### RF-DOC-2 · Operations documentation and runbooks

**Severity:** High · **Area:** Documentation · **Depends on:** RF-OBS-4, RF-RES-2

> **Status at 2026-10-12 — Partial.** The Evidence (no operations documentation of any kind) is stale: `docs/ops/` now holds seven runbooks (RB-1..7), a DR plan, backup/restore, incident response, a PKI lifecycle document and a GA-gate runbook, each with verification steps. Missing are install, upgrade, rollback, capacity, node-replacement and air-gap-transit procedures, an alert catalogue with one runbook per alert, symptom-organised troubleshooting, any generated configuration/metric/error reference, evidence of execution by a non-author, and inclusion of the docs in the air-gap bundle. The set lives in `docs/ops/`, not `docs/dev/operations/`, while `supply-chain/advisories.toml` points at the latter path.
> Criteria met 0 of 5; open: a, c, d, e; not verifiable by reading: b. Confidence: high.
>
> **Scope change:** `docs/ops/` holds runbooks plus DR, backup, incident-response, PKI and GA-gate documents. Read this item as gap-filling, and use `docs/ops/`, not `docs/dev/operations/`.
>
> **Still to do:**
> - Write the missing procedures: install, upgrade, rollback, capacity expansion, node replacement, air-gap transit
> - Define the alert catalogue and add a check that every alert and listed procedure has a runbook
> - Generate the configuration, metric, alert, error-code and CLI reference from source
> - Add symptom-first troubleshooting guides
> - Ship the operations set in the air-gap bundle and have a non-author execute each runbook
>
> Checked: `docs/ops/README.md:6` · `docs/ops/RUNBOOKS.md:7` · `docs/ops/PKI.md:47` · `deploy/air-gap-bundle/README.md:16` · `supply-chain/advisories.toml:32`

**Evidence.** `docs/dev/` contains the SADs, diagrams and `BUILD_ENV.md`. There is no operations documentation of any kind: no runbooks, no troubleshooting, no procedures.

**Prompt**

```
Write the documentation an operator needs, in docs/dev/operations/.

1. Runbooks: one per alert (RF-OBS-4a enforces this), plus procedures for
   install, upgrade, rollback, backup, restore, DR, key rotation, certificate
   renewal, break-glass, incident response, capacity expansion, node replacement,
   air-gap transit, and tenant lifecycle. Each runbook: when to use it,
   prerequisites, exact commands, expected output, verification, rollback, and
   escalation.
2. Troubleshooting guides organised by symptom (what the operator sees) rather
   than by component (what the engineer knows), because the operator does not
   know which component is at fault.
3. Architecture-for-operators: the deployment view, the data flows, the
   dependencies, and the failure modes — distinct from the SADs, which are
   design documents.
4. Reference: every configuration option (generated from the schema, RF-RT-1),
   every metric, every alert, every error code, every CLI command.
5. Test the runbooks by having someone who did not write them execute them, and
   fix what they stumble on. An untested runbook is a hypothesis.
6. Ship it all in the air-gap bundle (RF-DEP-5d).
```

**Acceptance criteria**

- **AC-RF-DOC-2a — Every alert and procedure has a runbook.** *Given* the alert catalogue and the procedure list, *when* checked, *then* each has a runbook, enforced in CI.
- **AC-RF-DOC-2b — Runbooks are executable by others.** *Given* an operator who did not write it, *when* they follow a runbook, *then* they complete the procedure successfully.
- **AC-RF-DOC-2c — Reference is generated.** *Given* the configuration, metrics, alerts and error codes, *when* the reference is built, *then* it is generated from source and cannot drift.
- **AC-RF-DOC-2d — Symptom-first troubleshooting exists.** *Given* a symptom, *when* the operator searches, *then* they find a guide organised by what they observed.
- **AC-RF-DOC-2e — Offline availability.** *Given* the air-gap bundle, *when* opened, *then* the full operations set is present.

**Done when:** every alert and procedure has a tested runbook and the reference is generated.

---

### RF-DOC-3 · User documentation and in-product help

**Severity:** Medium · **Area:** Documentation · **Depends on:** RF-UX-*

> **Status at 2026-10-12 — Open.** No user documentation exists: there are no role guides, task guides, glossary or help content anywhere outside this plan, and the console READMEs under `products/*/ui` are developer run instructions. `docs/demo/` has a screenshot capture script for the launch deck, which is not wired to an E2E suite or to documentation. In-product refusal explanations were not examined screen by screen.
> Criteria met 0 of 4; open: a, b, d; not verifiable by reading: c. Confidence: medium.
>
> **Still to do:**
> - Write a role guide per persona explaining clearance, markings, purpose, human gates, break-glass and audit in operational terms
> - Write a glossary of the governance terms of art
> - Add contextual help and refusal explanations in the consoles and audit every refusal path
> - Generate documentation screenshots from an E2E suite
> - Make the documentation available offline in the bundle and accessible
>
> Checked: `products/forge/ui/README.md:1` · `ui/README.md:8` · `docs/dev/README.md:6`

**Evidence.** There is no user documentation. `README.md` and `GETTING_STARTED.md` address builders; `docs/design/` addresses designers; the SADs address architects. Nobody has written for the analyst, the data engineer, the release manager, the operator or the auditor.

**Prompt**

```
Write for the people who will use this.

1. Role-based guides, one per persona in Workstream O: what this product does for
   you, how to do the things you need, and what the governance model means in
   your daily work. The governance concepts — clearance, markings, purpose,
   human gates, break-glass, audit — must be explained in operational terms,
   because misunderstanding them causes real mishandling.
2. Task guides mapping to the Workstream O flows, with screenshots kept current
   automatically (generated from the E2E suite so they cannot rot).
3. In-product help: contextual help on every screen, plain-language explanations
   of every governance state, and inline explanation of why an action was refused
   with what to do instead.
4. Onboarding and orientation content used in RF-UX-2.
5. A glossary covering every term of art, since this domain has many and they are
   not interchangeable.
6. Accessible, localisable (RF-UI-4), searchable, and available offline.
```

**Acceptance criteria**

- **AC-RF-DOC-3a — Every persona is served.** *Given* each persona, *when* they look for guidance, *then* a role guide covers their work.
- **AC-RF-DOC-3b — Screenshots stay current.** *Given* a UI change, *when* documentation is built, *then* screenshots regenerate from the E2E suite.
- **AC-RF-DOC-3c — Refusals are explained in-product.** *Given* any refusal, *when* it is shown, *then* the operator sees why and what they could do.
- **AC-RF-DOC-3d — Offline and accessible.** *Given* an air-gapped site and assistive technology, *when* documentation is used, *then* it is fully available and accessible.

**Done when:** role guides exist for all six personas, screenshots auto-generate, and in-product help covers every governance state.

---

### RF-DOC-4 · Architecture documentation currency and decision records

**Severity:** Medium · **Area:** Documentation · **Depends on:** —

> **Status at 2026-10-12 — Open.** There is no ADR directory, template or index. All SADs are still Version 1.0 dated 2026-07-02 with no changelog, and two more (SAD_08 Gungnir, SAD_08b Forseti) have been added without `docs/dev/README.md` being updated: it lists nine documents and 63 diagrams against eleven SADs and 76 diagram sources, and still says the set is not evidence of an implemented system. Every SAD published in this public repository is marked CONFIDENTIAL — RECIPIENT EYES ONLY, and the rc.1 notes list disclosure of the design documents as an undecided owner action.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Scope change:** `docs/dev/README.md` lists nine SADs and 63 diagrams; the tree has eleven SADs (with `SAD_08`, `SAD_08b`) and 76 diagram sources. Every published SAD is marked confidential (RF-CA-7).
>
> **Still to do:**
> - Create docs/dev/adr/ with a template and index and write the retrospective ADRs (lean-std transport, pure-Rust FIPS backend, in-memory reference posture, no-external-dependency stance)
> - Resolve the CONFIDENTIAL marking on publicly released SADs and the Kryotech/Akuma branding inside them
> - Reconcile each SAD with the implementation (for example Go and DataFusion backends, GLM model choice, single-replica services) or record the divergence in an ADR
> - Version the SADs with a changelog and update `docs/dev/README.md` to the current document and diagram set
> - Add a diagram-currency check for new services and data flows
>
> Checked: `docs/dev/markdown/SAD_00_Midgard_Core.md:7` · `docs/dev/README.md:6` · `docs/dev/README.md:34` · `docs/dev/markdown/SAD_08_Gungnir.md:6` · `docs/releases/v0.1.0-rc.1.md:109`

**Evidence.** The eight SADs are the design source of truth and are dated v1.0, 2026-07-02 — before the implementation. Every `.md` is currently modified in the working tree (the rebrand). There are no ADRs; this document alone calls for at least four.

**Prompt**

```
Keep the architecture documents true as the system changes.

1. Establish an ADR practice: docs/dev/adr/ with a template and an index. Record
   every significant decision, including retrospectively for the ones already
   made (the no-async lean-std transport, the pure-Rust FIPS backend over
   liboqs, the in-memory reference posture, the no-external-dependency stance).
   These were good decisions with real reasoning that currently lives only in
   module comments and would be lost.
2. Reconcile the SADs with the implementation: identify where the built system
   diverges from the design and either update the SAD or record an ADR
   explaining the divergence. A design document that no longer describes the
   system is worse than none, because people trust it.
3. Regenerate the diagrams from the implemented architecture and add a check that
   a new service or data flow requires a diagram update.
4. Version the SADs alongside releases, with a changelog.
5. Add a documentation review step to any PR that changes architecture.
```

**Acceptance criteria**

- **AC-RF-DOC-4a — ADRs exist for significant decisions.** *Given* the ADR index, *when* reviewed, *then* every significant decision — past and present — is recorded with context, options, decision and consequences.
- **AC-RF-DOC-4b — SADs match the implementation.** *Given* each SAD, *when* compared with the built system, *then* they agree or the divergence is recorded in an ADR.
- **AC-RF-DOC-4c — Diagrams stay current.** *Given* a new service or data flow, *when* the PR is opened without a diagram update, *then* the check fails.
- **AC-RF-DOC-4d — Versioned with a changelog.** *Given* a release, *when* the SADs are inspected, *then* their version and changelog reflect the changes.

**Done when:** the ADR practice is established with retrospective entries, and SADs are reconciled with the implementation.

---

### RF-DOC-5 · Contribution, governance and project documentation

**Severity:** Low · **Area:** Documentation · **Depends on:** —

> **Status at 2026-10-12 — Partial.** `CONTRIBUTING.md` (ground rules, provenance tags, offline build and test, style, PRs, release and version policy), `SECURITY.md` and a Keep-a-Changelog `CHANGELOG.md` with Unreleased and 0.1.0-rc.1 sections exist at the root. `CODE_OF_CONDUCT.md`, `GOVERNANCE.md`, `MAINTAINERS.md`, issue and PR templates and a standalone release-process document (support window, deprecation policy, branching) do not, and `CONTRIBUTING.md` links to `.github/CODEOWNERS` and `.github/workflows/release.yml`, which are absent from the public tree.
> Criteria met 0 of 3; open: a, b; not verifiable by reading: c. Confidence: high.
>
> **Still to do:**
> - Add CODE_OF_CONDUCT.md, GOVERNANCE.md and MAINTAINERS.md
> - Publish issue and PR templates with provenance-tag, test, threat-model and documentation checkboxes (requires a public .github directory)
> - Publish CODEOWNERS or remove the link from CONTRIBUTING.md
> - Extend the release section into a release-process document covering branching, support window and deprecation policy
>
> Checked: `CONTRIBUTING.md:60` · `CONTRIBUTING.md:69` · `CHANGELOG.md:10` · `SECURITY.md:72`

**Prompt**

```
Complete the project's own documentation.

Add: CONTRIBUTING.md (development setup, coding standards, the provenance-tag
requirement, test requirements, review process, commit conventions);
CODE_OF_CONDUCT.md; SECURITY.md (RF-SEC-7); GOVERNANCE.md (decision-making,
maintainership, release authority); MAINTAINERS.md; a CHANGELOG following a
recognised convention; issue and PR templates including the threat-model and
documentation checkboxes this plan requires; and a release-process document
covering versioning, branching, the support window and deprecation policy.
```

**Acceptance criteria**

- **AC-RF-DOC-5a — Standard files exist.** *Given* the repository root, *when* inspected, *then* every listed document is present and current.
- **AC-RF-DOC-5b — Templates enforce the checks.** *Given* a PR, *when* opened, *then* the template requires the provenance-tag, test, threat-model and documentation confirmations.
- **AC-RF-DOC-5c — Changelog is maintained.** *Given* a release, *when* the changelog is read, *then* it accurately describes the changes.

**Done when:** all listed documents exist and the PR template enforces the required checks.

---

### RF-DOC-6 · Design system: single source, coverage and governance patterns

**Severity:** Medium · **Area:** Design system · **Depends on:** RF-UI-3, RF-UI-6, RF-UI-8

> **Status at 2026-10-12 — Open.** Unchanged from the Evidence: the design system still exists twice, 28 JSX components under `docs/design/components` and 28 TSX ports under `ui/design-system/src/components`, with token CSS copied by hand per `PORTING.md` (five of six token files are byte-identical today; `fonts.css` has already diverged in its header comment). There is no generator, divergence check, test, component workshop or contribution process, and `ui/design-system/package.json` has only build and typecheck scripts. Of the governance patterns only ClassificationBanner, MarkingChip, HumanGate, BreakGlass, GuardedAction and ProvenanceTag exist.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> **Still to do:**
> - Pick one token source and generate the other, with a check that fails on divergence (reconcile fonts.css first)
> - Add a test runner to `ui/design-system` with behaviour and accessibility tests per component
> - Build the missing governance components (clearance indicator, purpose selector, dual-control card, audit-reference chip, confidence, redaction and staleness indicators)
> - Audit the consoles for locally defined components and move them into the design system; build the listed primitives (modal, drawer, toast, combobox, virtualised table and so on)
> - Add an offline component workshop and a contribution process
>
> Checked: `ui/design-system/PORTING.md:3` · `ui/design-system/PORTING.md:9` · `ui/design-system/package.json:11` · `docs/design/tokens/fonts.css:7` · `ui/README.md:4`

**Evidence.** The design system exists twice: `docs/design/` (the stated authority — tokens, 28 components as JSX with `.prompt.md` and `.d.ts` files, guidelines, UI kits, a marketing site) and `ui/design-system/` (the TypeScript port consumed by the apps, with `PORTING.md` describing the mapping). The token files are duplicated and both copies were edited independently in the current working tree. Neither has tests or a component workshop.

**Prompt**

```
Make the design system one thing, complete and governed.

1. Single source: generate the app package from the authority (or vice versa),
   with a CI check failing on divergence. Tokens especially must exist once —
   the current duplication has already produced parallel edits.
2. Close the component gaps this plan creates demand for: date/time picker,
   combobox with async search, multi-select, tree, drawer, modal with correct
   focus management, toast/notification, tooltip and popover, breadcrumb,
   pagination, virtualised table, code editor, DAG canvas, map container, chart
   primitives, file upload, command palette, skeleton loaders, empty-state and
   error-state components. Build them in the design system so consoles stop
   inventing them locally.
3. Governance patterns as first-class components, since they are what makes this
   design system distinctive: classification banner and marking chip (exist),
   plus clearance indicator, purpose selector, human-gate dialog, dual-control
   approval card, break-glass banner with countdown, audit-reference chip,
   provenance/citation chip, confidence and uncertainty indicator, redaction
   indicator, and staleness indicator. Each with correct semantics, accessible
   naming and a non-colour cue.
4. Component workshop (Storybook or equivalent) with every variant and state,
   accessibility annotations, and usage guidance — usable offline and shipped in
   the bundle.
5. Test every component (RF-TST-2) and visual-regression every variant.
6. Document contribution: how a new component is proposed, reviewed and added,
   so the system grows deliberately rather than by accretion in consoles.
```

**Acceptance criteria**

- **AC-RF-DOC-6a — One source of truth.** *Given* the two copies, *when* CI runs, *then* one is generated from the other and divergence fails the build.
- **AC-RF-DOC-6b — Gaps are closed.** *Given* the console screens, *when* audited, *then* no console defines a component that belongs in the design system.
- **AC-RF-DOC-6c — Governance patterns exist and are accessible.** *Given* each governance component, *when* tested, *then* it has correct semantics, an accessible name, and a non-colour cue.
- **AC-RF-DOC-6d — Workshop is complete and offline.** *Given* the workshop, *when* opened offline, *then* every component appears with every variant and state.
- **AC-RF-DOC-6e — Everything is tested.** *Given* the 28+ components, *when* CI runs, *then* each has behaviour, accessibility and visual tests.

**Done when:** the duplication is resolved by generation, the gaps are closed, and the workshop ships offline.

---

### RF-DOC-7 · Public-facing site accuracy and brand consistency

**Severity:** Low · **Area:** Documentation · **Depends on:** RF-DOC-1

> **Status at 2026-10-12 — Partial.** The rebrand half is done for the site and tokens: the 13 pages under `docs/design/site/` and both `colors.css` copies use Veldris, with no Kryotech string left in them. Nothing else has moved: the pricing page presents three tiers as offerings with no indicative marking, including an Enterprise HA control plane and cross-site DR replication while the rc.1 notes state one replica per service; the privacy page is a self-described preview dated 2026-07-08 with no sign of counsel review; no status vocabulary exists to audit claims against; and there is no site build, link or accessibility check.
> Criteria met 0 of 4; open: a, b, d; not verifiable by reading: c. Confidence: medium.
>
> **Scope change:** The rebrand is complete in the site and tokens. What remains is claim accuracy — the pricing page sells an HA control plane while every service runs one replica — plus the site check.
>
> **Still to do:**
> - Audit every site claim against the RF-DOC-1 status vocabulary once it exists; start with pricing tiers, the bit-for-bit rebuild and one-signature-per-release claims, and HA/DR
> - Mark pricing as indicative or tie it to a deliverable offering
> - Finish the brand clean-up outside the site (Kryotech in SADs and a diagram, Akuma Engineering Ltd. in docs) so criterion b can close
> - Have counsel review privacy, terms and legal and align the privacy notice with RF-CMP-3
> - Add a site check for build, internal links, accessibility and capability claims below operable
>
> Checked: `docs/design/site/pricing.html:28` · `docs/design/site/privacy.html:58` · `docs/design/site/github.html:42` · `docs/design/tokens/colors.css:28` · `docs/releases/v0.1.0-rc.1.md:95`

**Evidence.** `docs/design/site/` contains a complete marketing site (index, architecture, four product pages, pricing, about, contact, legal, privacy, terms, github), all currently modified by the in-flight rebrand. `README.md` notes that no logos exist and `ProductMark` sets the name in plain type.

**Prompt**

```
Ensure the public-facing material is accurate and consistent.

1. Audit every claim on the site against the status vocabulary from RF-DOC-1. A
   marketing site describing capabilities at `reference` status as though they
   were `production` is the most consequential form of the drift this workstream
   addresses.
2. Complete the rebrand consistently across the site, the design tokens, the
   documentation and the source.
3. Legal pages: have privacy, terms and legal reviewed by counsel; ensure the
   privacy statement matches what the software actually does (RF-CMP-3).
4. Pricing: ensure it reflects a real, deliverable offering, or mark it clearly
   as indicative.
5. Accessibility: the site meets the same WCAG 2.2 AA bar as the product.
6. Add a CI check that the site builds, has no broken internal links, and
   contains no reference to a capability marked below `operable`.
```

**Acceptance criteria**

- **AC-RF-DOC-7a — Claims match status.** *Given* every site claim, *when* checked against the status vocabulary, *then* none overstates.
- **AC-RF-DOC-7b — Brand is consistent.** *Given* the site, tokens, documentation and source, *when* searched, *then* the brand is used consistently and no stale reference remains.
- **AC-RF-DOC-7c — Legal pages are reviewed and accurate.** *Given* the privacy statement, *when* compared with actual data handling, *then* they agree, and counsel has reviewed.
- **AC-RF-DOC-7d — Site is accessible and builds clean.** *Given* CI, *when* it runs, *then* the site builds, has no broken links, and passes the accessibility bar.

**Done when:** claims match status, the rebrand is complete, and the CI check is green.

---
# Y — Claim alignment & public release

> **Added in Revision 2 (2026-10-12).** An external assessment compared the statements made about Midgard in this repository's own documents and in the launch material with what a reader gets by cloning public `main`. It found two classes of problem. First, capabilities described as built that are absent from the public tree, or present only as tested libraries that no running service uses. Second, headline numbers that disagree with each other and with the tree. Both were re-verified against public `main` @ `9ec8659` before this workstream was written; the evidence in each item is that verification, not the assessment's wording.
>
> **Scope.** This workstream covers what this repository says about itself. Statements made elsewhere are managed separately and are out of scope; the claims ledger (RF-CA-1) and the numbers sheet (RF-CA-2) are what they should be drawn from.
>
> **Why this precedes everything else.** Readers clone the repository and check. A claim the tree contradicts costs more than the capability it describes would have earned, and the truth-in-status rule in §2 already forbids it. Nothing in this workstream requires new capability to close: every capability item has an honest wording that is true today.

**Two tracks.**

- **Truth track — RF-CA-1..7 and RF-CA-13.** No new capability. Every statement becomes checkable from one signed, tagged commit. Target: the tagged public release, due **Thursday 2026-10-22 18:00**.
- **Capability track — RF-CA-8..12.** Each takes a capability from `reference` (or absent) to `operable`, using the status vocabulary of RF-DOC-1. Each item states the wording that is true until it lands.

**Status at the start of this workstream**, each row verified on public `main` @ `9ec8659`:

| Claim | Status | What exists | Item |
|---|---|---|---|
| Three model families hot-swapping | `reference` | Router, three family adapters and a hot-swap test on the deterministic engine. `mimir-server` serves one backend and does not use the router. One real model (Granite, CPU) has run. | RF-CA-8 |
| Zero-downtime upgrades | `reference` | Rolling and blue-green upgrade logic tested in-process. The chart deploys one replica per service; nothing drives the rollout orchestrator. | RF-CA-9 |
| Heimdall option generation | `reference` | Ranked, cited option generation with a Mimir bridge and `AC-H-3`. The running feed server does not depend on it. | RF-CA-10 |
| Admin console alerts | `reference` | Durable break-glass alerter as a library. The admin console renders a hard-coded list and runs on mock data. | RF-CA-11 |
| Multimodal ingest (georeferencing, captions, transcripts) | absent | No code and no requirement id. Forge reads CSV files, PostgreSQL, S3 and Kafka. | RF-CA-12 |
| Likan | external | A separate, proprietary tool for data-structure ontologies. Not in this repository, by design; the tree does not yet say so. | RF-CA-13 |
| OAG retriever | `reference` | `mimir/oag-retriever` (`plan.rs:67`, `ground.rs:82`), tests in `tests/grounding.rs`. Nothing routes requests to it. | cite as-is |
| Agent runtime | `reference` | `mimir/agent-runtime`, tests `ac_mi_2.rs`, `ac_mi_3.rs`. Nothing routes requests to it. | cite as-is |
| Ontology loading | `operable` | `services/http-gateway/src/ontology_file.rs:114`, `enclosure.rs:727`, registered at every gateway start. | cite as-is |
| Checked import | `operable` | `tools/mg-admin/src/main.rs:376`, `services/http-gateway/src/import_file.rs`, tests in `tools/mg-admin/tests/import.rs`. | cite as-is |

**Recommended cut for the tag.** RF-CA-1..7 and RF-CA-13 land in full. Of the capability track, RF-CA-10 and the read path of RF-CA-11 are the smallest (each wires an existing, tested library into an existing service) and are the only candidates for the tag. RF-CA-8, RF-CA-9 and RF-CA-12 will not land by 2026-10-22; their claims take the fallback wording in each item and name a delivery sprint in the claims ledger (RF-CA-1). A capability item that is attempted for the tag and is not green by the freeze takes its fallback wording — it does not ship half-wired.

---

### RF-CA-1 · Claims ledger — one row per published claim, with status and evidence

**Severity:** Critical · **Area:** Claim alignment · **Depends on:** — · **Blocks:** RF-CA-6

> **Status at 2026-10-12 — Open.** No claims ledger exists; `docs/compliance/` has no `CLAIMS.md` or `claims.toml`, and there is no `tools/ci/claims.sh`.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.

**Evidence.** There is no single list of what has been claimed about the platform. The same capability is described differently in the README, [`GA_READINESS.md`](../../compliance/GA_READINESS.md), the launch deck ([`docs/launch/pipeline/build-deck.js`](../../launch/pipeline/build-deck.js)), and nothing ties a claim to the code or test that supports it. The status table at the head of this workstream had to be assembled by hand.

**Prompt**

```
Create the claims ledger and make it the only source for capability statements.

1. Add docs/compliance/CLAIMS.md, generated from a data file
   docs/compliance/claims.toml. One entry per capability the repository states:
   id, the claim as worded, status (spec | reference | operable | production —
   the RF-DOC-1 vocabulary), evidence (source paths with line anchors, and the
   named tests that exercise it), the acceptance criterion ids it maps to, and —
   for anything below `operable` — the delivery milestone and the fallback
   wording to use until then.
2. Seed it with every row of the Workstream Y status table, then sweep the
   README, GETTING_STARTED, GA_READINESS, CHANGELOG, the release notes, the
   launch deck source and the public site for further capability statements and
   add each.
3. Wording rule, enforced in review: a claim may use "built", "delivers",
   "supports" or the present tense only at `operable` or above. A `reference`
   capability reads "implemented and tested as a library (<test>); not yet in a
   running service". A `spec` capability reads "specified in <SAD>; not
   implemented".
4. Add tools/ci/claims.sh --check: every evidence path exists, every line anchor
   is inside the file, every named test exists as a #[test] function, and
   CLAIMS.md is exactly what claims.toml renders. A row at `operable` must name
   a test that drives a running service (a compose or kind smoke step), not only
   a library test.
5. Reword every document in the repository so that each capability statement
   matches its ledger row.
```

**Acceptance criteria**

- **AC-RF-CA-1a — Every claim has a row.** *Given* the swept documents, *when* each capability statement is looked up, *then* it has a ledger row with a status and evidence.
- **AC-RF-CA-1b — Evidence resolves.** *Given* `tools/ci/claims.sh --check`, *when* an evidence path, line anchor or test name does not exist, *then* the check fails and names the row.
- **AC-RF-CA-1c — `operable` needs a service test.** *Given* a row marked `operable` whose only evidence is a library test, *when* the check runs, *then* it fails.
- **AC-RF-CA-1d — Wording follows status.** *Given* any swept document, *when* a capability below `operable` is described, *then* it uses the fallback wording from its row.

**Done when:** the ledger covers every published claim, the check is green in CI, and no swept document states a capability above its ledger status.

---

### RF-CA-2 · One numbers sheet, generated from the tag

**Severity:** Critical · **Area:** Claim alignment · **Depends on:** RF-CA-5 · **Blocks:** RF-CA-6

> **Status at 2026-10-12 — Partial.** The crate count in `README.md` and both `GETTING_STARTED.md` copies was corrected to 70 on `dev` on 2026-10-12. There is no numbers sheet and no check; the test and criteria figures still cite a commit that is not in public history.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> **Still to do:**
> - Write `tools/ci/numbers.sh` and generate `docs/compliance/NUMBERS.md`
> - Replace the remaining figures in `README.md`, `GA_READINESS.md`, this document's section 1 and the launch deck source
>
> Checked: `README.md:9` · `README.md:28` · `GETTING_STARTED.md:5` · `docs/compliance/GA_READINESS.md:44`

**Evidence.** Several sets of figures are in circulation. Measured on public `main` @ `9ec8659`, excluding `vendor/`: **70** workspace crates, **97,041** raw Rust lines (about 75,100 excluding blank and comment lines) in 564 files, **771** `#[test]` functions, none ignored.

| Figure | Tree | Stated elsewhere |
|---|---|---|
| Crates | 70 | 62 (`README.md:9`) · 56 (`GETTING_STARTED.md:5`, both copies) · 55 and 56 (this document, §1 and RF-DOC-1) · 44 (launch deck) |
| Rust lines | 97,041 raw | ~30,000 (this document, §1) |
| Tests | 771 functions | 248 passed (`README.md:29`) · 664 passed (`GA_READINESS.md:44`, launch deck) |
| Acceptance criteria | not re-run | 72 / 72 (`README.md:28`) |

A count of test functions and a count of tests passed in a run are different measures, and the documents do not say which they quote. The README and GETTING_STARTED crate counts were corrected to 70 on `dev` on 2026-10-12; the rest stands.

**Prompt**

```
Generate every headline number from the tree, once, and use it everywhere.

1. Add tools/ci/numbers.sh. It prints, and with --write renders to
   docs/compliance/NUMBERS.md, for the commit it runs on:
     - workspace crates: members of the root Cargo.toml
     - Rust source: files, raw lines, and lines excluding blank and //-comment
       lines, over tracked *.rs outside vendor/
     - test functions: #[test] attributes outside vendor/, and how many are
       #[ignore]
     - tests executed and passed, and acceptance criteria passed: read from the
       recorded gate run for that commit (RF-CA-5), never typed by hand
     - server binaries and shipped binaries: from
       supply-chain/release-artefacts.txt
     - console packages, SAD documents and diagrams
   State the counting rule beside each figure, so a reader can reproduce it with
   one command.
2. NUMBERS.md names the commit and tag it was generated from. A figure with no
   recorded gate run prints "not run at this commit", never a stale value.
3. Add numbers.sh --check to CI: it scans README.md, both GETTING_STARTED.md
   copies, GA_READINESS.md, CHANGELOG.md, docs/releases/*, the launch deck
   source and this document for crate, line, test and criteria counts, and fails
   on any figure that differs from the sheet. Figures that are deliberately
   historical must be written with their commit ("at <commit>: …") to pass.
4. Replace every figure in those documents with the sheet's, and regenerate the
   launch deck from its pipeline.
5. Quote test functions and tests passed as two separately labelled figures
   wherever either appears. The sheet is the single source for any statement
   of these figures, inside or outside the repository.
```

**Acceptance criteria**

- **AC-RF-CA-2a — The sheet reproduces.** *Given* a clean checkout of the tag, *when* `tools/ci/numbers.sh` runs, *then* its output equals `docs/compliance/NUMBERS.md` byte for byte.
- **AC-RF-CA-2b — Drift fails CI.** *Given* a document edited to quote a different crate, line, test or criteria count, *when* `numbers.sh --check` runs, *then* it fails and names the file and line.
- **AC-RF-CA-2c — No unlabelled run results.** *Given* any figure for tests passed or criteria passed, *when* read, *then* it names the commit of the gate run it came from, and that commit is in public history.
- **AC-RF-CA-2d — One figure everywhere.** *Given* the swept documents at the tag, *when* searched for each headline figure, *then* every occurrence equals the sheet.

**Done when:** the sheet is generated from the tag, `--check` is green, and every swept document quotes it unchanged.

---

### RF-CA-3 · Restore the hermetic build configuration to the public tree

**Severity:** Critical · **Area:** Claim alignment · **Depends on:** — · **Blocks:** RF-CA-5

> **Status at 2026-10-12 — Open.** `.cargo/config.toml` is absent from the tree. The offline build was not attempted.
> Criteria met 0 of 3; open: a, b, c. Confidence: high.
>
> Checked: `README.md:119` · `deploy/acceptance/run-airgapped.sh:10`

**Evidence.** `.cargo/config.toml` is not in public `main`. [`README.md:119`](../../../README.md) says dependencies are vendored through "`vendor/` + `.cargo/config.toml`", and [`deploy/acceptance/run-airgapped.sh:10`](../../../deploy/acceptance/run-airgapped.sh) relies on it ("see .cargo/config.toml"). No other file in the tree replaces the crates-io source. `.github/` is missing too (RF-CA-4), while dot-files at the root (`.gitignore`, `.gitattributes`, `.gitleaks.toml`) are present — consistent with the public export having dropped dot-directories. A fresh clone therefore may not build offline as documented; this was established by reading the tree, not by running the build.

**Prompt**

```
Make a fresh public clone build and test offline, exactly as the README says.

1. Add .cargo/config.toml replacing the crates-io source with the vendored
   directory, so cargo never contacts a registry.
2. Find why the public export dropped .cargo/ and .github/, fix the export so
   dot-directories are carried, and add a check to the export that diffs the
   exported file list against the source tree and fails on any unexplained
   omission.
3. Prove it from a fresh clone, in the pinned image, under --network none:
   `cargo build --workspace --release --offline` and `just test` both succeed
   with an empty cargo registry cache.
4. Add a CI job that performs exactly that from a fresh clone with no cache, so
   a missing vendored crate or missing configuration fails the build rather
   than being masked by a warm cache.
5. Verify vendor/ against Cargo.lock: every locked package is present at the
   locked version and checksum, and vendor/ contains nothing the lock does not
   name (RF-SC-7).
```

**Acceptance criteria**

- **AC-RF-CA-3a — A fresh clone builds offline.** *Given* a fresh clone of public `main` and an empty cargo cache, *when* the workspace is built under `--network none`, *then* it succeeds.
- **AC-RF-CA-3b — A missing crate fails.** *Given* one crate removed from `vendor/`, *when* the offline build runs, *then* it fails naming that crate; it does not fetch.
- **AC-RF-CA-3c — The export is complete.** *Given* the public export, *when* its file list is compared with the source tree, *then* every omission is listed in an explicit exclusions file with a reason.

**Done when:** the fresh-clone offline job is green on public `main` and the export check is in place.

---

### RF-CA-4 · Publish the CI and release workflows the documents cite

**Severity:** High · **Area:** Claim alignment · **Depends on:** RF-CA-3 · **Blocks:** RF-CA-6

> **Status at 2026-10-12 — Open.** There is no `.github/` directory in the tree.
> Criteria met 0 of 3; open: a, b, c. Confidence: high.
>
> Checked: `README.md:36` · `README.md:644` · `README.md:759`

**Evidence.** Public `main` has no `.github/` directory. [`README.md:36`](../../../README.md) and `:759` cite `.github/workflows/release.yml`; `:644` says "CI fails if it is not exactly what the chart renders"; `:823` says what CI does and does not cover. This document cites the same workflow as evidence in RF-SC-1 and RF-SC-5. A reader cannot see, run or verify any of it.

**Prompt**

```
Publish the workflows, or stop citing them.

1. Restore .github/workflows/ to the public tree: the CI workflow (build, both
   crypto-backend test invocations, the fresh-clone offline job from RF-CA-3,
   the rendered-chart check, numbers.sh --check, claims.sh --check) and the
   release workflow.
2. Remove anything that cannot be public — runner labels that disclose
   infrastructure, internal hostnames — by parameterising it, not by deleting
   the step.
3. Pin every third-party action by full commit digest and grant each job the
   minimum token permissions.
4. Make the workflows required status checks on `main`, alongside the existing
   branch protection.
5. Check every sentence in the README, GA_READINESS and this document that says
   what CI does against the published workflow; correct any that the workflow
   does not do.
```

**Acceptance criteria**

- **AC-RF-CA-4a — Cited workflows exist.** *Given* every workflow path cited in the repository's documents, *when* resolved on public `main`, *then* the file exists.
- **AC-RF-CA-4b — CI claims are true.** *Given* each documented statement of what CI checks, *when* compared with the published workflow, *then* a job performs that check.
- **AC-RF-CA-4c — Checks gate `main`.** *Given* a pull request that fails the build, the offline job, the numbers check or the claims check, *when* a merge to `main` is attempted, *then* it is blocked.

**Done when:** the workflows are public, green on `main`, and required.

---

### RF-CA-5 · Re-run the acceptance gate at the release commit and record it

**Severity:** Critical · **Area:** Claim alignment · **Depends on:** RF-CA-3 · **Blocks:** RF-CA-2, RF-CA-6

> **Status at 2026-10-12 — Open.** No gate run is recorded for any commit in public history; `docs/compliance/gate-runs/` does not exist.
> Criteria met 0 of 4; open: a, b, c, d. Confidence: high.
>
> Checked: `README.md:24`

**Evidence.** [`README.md:24`](../../../README.md) attributes "72 / 72 pass" and "248 passed · 0 failed · 0 ignored" to commit `60091a9`. That commit is not in the public history, which has two commits. `GA_READINESS.md:44` reports 664 passed for `just test` at an unnamed commit. The tree now holds 771 test functions. No gate run is recorded for any commit a reader can check out, and neither audit behind this document ran the suite (Appendix 5).

**Prompt**

```
Produce one gate run a reader can reproduce, and derive every status line from it.

1. On the release candidate commit, run `bash deploy/acceptance/run-airgapped.sh`
   in the pinned image under --network none, and `just test` likewise. Capture
   the complete logs.
2. Add tools/ci/gate-record.sh: it parses the logs into
   docs/compliance/gate-runs/<commit>.md — commit, image digest, toolchain
   version, per-criterion PASS/FAIL for all 72 ids, tests executed / passed /
   failed / ignored per crate invocation and in total, both crypto backends, wall
   time, and the SHA-256 of each raw log. Keep the raw logs as release assets.
3. Fail the record, not just the run, if any criterion is missing from the log:
   72 ids in, 72 results out, or the record is refused.
4. Replace the README status table and the GA_READINESS sweep table with content
   generated from the record, naming its commit. Remove the reference to
   60091a9.
5. If any criterion fails at the release commit, the README says so, with the id.
   A failing criterion is fixed or reported; it is never omitted.
```

**Acceptance criteria**

- **AC-RF-CA-5a — The cited commit exists.** *Given* any gate result quoted in the repository, *when* its commit is looked up in public history, *then* it resolves.
- **AC-RF-CA-5b — The record matches the log.** *Given* the raw logs and the record, *when* compared, *then* every count and every per-criterion result agrees, and the log digests match.
- **AC-RF-CA-5c — All 72 are accounted for.** *Given* a log from which one criterion id is absent, *when* `gate-record.sh` runs, *then* it refuses to write a record.
- **AC-RF-CA-5d — The run reproduces.** *Given* the tag and the recorded image digest, *when* a third party runs the gate, *then* they obtain the same per-criterion results.

**Done when:** a gate record exists for the tag's commit and the README and GA_READINESS tables are generated from it.

---

### RF-CA-6 · Signed, tagged public release

**Severity:** Critical · **Area:** Claim alignment · **Depends on:** RF-CA-1..5, RF-SC-8 · **Blocks:** —

> **Status at 2026-10-12 — Open.** No tag exists and the commits on public `main` are unsigned. The release signing key is not provisioned.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> Checked: `Cargo.toml:81` · `README.md:22`

**Evidence.** Public `main` has no tags and neither of its two commits is signed. The workspace version is `0.1.0-rc.1` ([`Cargo.toml:81`](../../../Cargo.toml)) and [`README.md:22`](../../../README.md) reads "Release: not yet cut", because tagging "needs a key ceremony and a provisioned signing runner". The repository is configured to sign commits and tags, but the release signing key is not provisioned. Until a tag exists, no claim can be pinned to a commit.

**Prompt**

```
Cut v0.1.0-rc.1 as a signed tag that everything else points at.

1. Hold the key ceremony in docs/ops/PKI.md: generate the release signing key in
   its custodian, record the ceremony, and publish the verifying material in the
   repository (an allowed-signers file for git, the ML-DSA trust anchor for
   artefacts) together with its fingerprint through a second channel.
2. Require signed commits on `main` and `dev` in branch protection. From this
   point every commit on the release path verifies.
3. Freeze the release commit. On that exact commit: the gate record (RF-CA-5),
   the numbers sheet (RF-CA-2) and the claims ledger (RF-CA-1) are generated and
   committed, and CI (RF-CA-4) is green.
4. Create the annotated, signed tag v0.1.0-rc.1 on that commit and push it. Run
   the release workflow on the provisioned signing runner: gate every binary in
   supply-chain/release-artefacts.txt, build the verify-before-run images, pack
   the signed air-gap bundle, and publish a pre-release with the SBOMs,
   signatures, attestations, gate logs and NUMBERS.md attached.
5. State in the release notes what the tag is and is not: a release candidate,
   with GA_READINESS still NO-GO and each blocker named. Do not soften that to
   make the tag look like more than it is.
6. Protect the tag against deletion and rewriting.
```

**Acceptance criteria**

- **AC-RF-CA-6a — The tag verifies.** *Given* a fresh clone and the published allowed-signers file, *when* `git tag -v v0.1.0-rc.1` runs, *then* the signature is good and names the release key.
- **AC-RF-CA-6b — One commit, three records.** *Given* the tag, *when* the gate record, the numbers sheet and the claims ledger are read, *then* each names the tag's commit.
- **AC-RF-CA-6c — Artefacts verify offline.** *Given* any released binary with its signature, SBOM and attestation, *when* `mg-verify` runs under `--network none` with the published trust anchor, *then* it accepts; with one byte changed, it refuses.
- **AC-RF-CA-6d — Unsigned commits cannot land.** *Given* an unsigned commit, *when* it is pushed toward `main`, *then* it is rejected.
- **AC-RF-CA-6e — The tag is immutable.** *Given* the published tag, *when* a delete or force-update is attempted, *then* it is refused.

**Done when:** `v0.1.0-rc.1` is public, signed and verifiable, and every claim, number and gate result in circulation points at it.

---

### RF-CA-7 · History, classification and publication statements

**Severity:** High · **Area:** Claim alignment · **Depends on:** — · **Blocks:** RF-CA-6

> **Status at 2026-10-12 — Partial.** `dev` was rebuilt on `main` on 2026-10-12 and GitHub now compares the two. This document carries per-item status. The history sentence in `GETTING_STARTED.md`, the confidentiality marking, the duplicated documents and `remedial-tasks.md` are unchanged.
> Criteria met 0 of 4; open: a, b, c; not verifiable by reading: d. Confidence: high.
>
> **Still to do:**
> - Reword the history sentence in both `GETTING_STARTED.md` copies
> - Resolve the classification marking on the published SADs
> - Keep one copy of `GETTING_STARTED.md` and `BUILD_PLAYBOOK.md`
> - Bring `remedial-tasks.md` to current status or retire it
> - Add a branch rule so `dev` cannot diverge from `main` again
>
> Checked: `GETTING_STARTED.md:5` · `README.md:842` · `docs/dev/remedial/remedial-tasks.md:1`

**Evidence.**

| Statement | Location | Reality |
|---|---|---|
| "its git history maps one commit per BUILD_PLAYBOOK.md prompt" | [`GETTING_STARTED.md:5`](../../../GETTING_STARTED.md) and `readme/GETTING_STARTED.md:5` | Public history is two commits. `README.md:11` on `main` was already reworded to say the public repository begins from a squashed import. |
| "Classification of source SADs: CONFIDENTIAL — RECIPIENT EYES ONLY" | [`README.md:842`](../../../README.md) | Those SADs are published in `docs/dev/markdown/` in a public repository. |
| `GETTING_STARTED.md` and `BUILD_PLAYBOOK.md` | root and `readme/` | Each exists twice, byte-identical, and has to be edited twice. |
| This document and `docs/dev/remedial/remedial-tasks.md` | `docs/dev/` | Two gap analyses are public and stay public (decided 2026-10-12, with the repository). Both were written against an earlier tree and contain statements — "Almost nothing is runnable" — that are no longer true and will be read as current unless each item carries its present status. |
| `dev` and `main` | GitHub | Shared no ancestor until 2026-10-12, when `dev` was rebuilt on `main`; GitHub could not compare or merge them. |

**Prompt**

```
Make every statement about the repository itself true.

1. Reword the GETTING_STARTED history sentence to match the README: the working
   history maps one commit per playbook prompt; the public repository starts
   from a squashed import of that state.
2. Resolve the classification marking with the SADs' author. Either the SADs
   are cleared for publication — replace the marking with the correct public
   one in the README footer and in each SAD header — or they are not, in which
   case remove them from the public tree and from its history, and state what
   was withheld. Do not leave a confidentiality marking on published material.
3. Keep one copy of GETTING_STARTED.md and BUILD_PLAYBOOK.md. Delete the other,
   or generate it, and repoint every link.
4. The repository and both gap analyses stay public. Each analysis therefore
   opens with its audit date and commit and a plain statement of which tree it
   describes, and each item carries its current status and what remains
   (Appendix 4 step 8). This document was brought to that standard on
   2026-10-12; do the same for docs/dev/remedial/remedial-tasks.md, or retire
   it with a pointer here if every RT-nn task is covered by an RF-* item.
5. Branch hygiene: `dev` always descends from `main`; unrelated-history pushes
   are refused by a branch rule; the default branch and the branch evaluators
   are pointed at are the same.
```

**Acceptance criteria**

- **AC-RF-CA-7a — History claims are true.** *Given* each statement about git history in the repository, *when* compared with `git log` on public `main`, *then* it holds.
- **AC-RF-CA-7b — No confidentiality marking on public material.** *Given* the public tree, *when* searched for classification markings, *then* none marks a published file as restricted.
- **AC-RF-CA-7c — No duplicated documents.** *Given* the tree, *when* file contents are hashed, *then* no two tracked documents are identical unless one is declared generated and CI regenerates it.
- **AC-RF-CA-7d — Branches merge.** *Given* `dev` and `main` on GitHub, *when* compared, *then* they share an ancestor and a pull request can be opened.

**Done when:** every row above is resolved and both gap analyses carry current status.

---

### RF-CA-8 · Multi-family routing and hot-swap in the running inference plane

**Severity:** High · **Area:** Claim alignment · Mimir · **Depends on:** RF-MI-2, RF-MS-1 · **Extends:** RF-MS-3, RF-MS-5

> **Status at 2026-10-12 — Open.** As the Evidence below describes: the router and hot-swap are a tested library; `mimir-server` serves one backend.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> Checked: `mimir/mimir-server/src/main.rs:151` · `mimir/model-server/tests/router.rs:228`

**Evidence.** The router and three family adapters exist: `GlmAdapter`, `NemotronAdapter`, `GemmaAdapter` ([`mimir/model-server/src/adapter.rs:136`](../../../mimir/model-server/src/adapter.rs)), `Router::serve` (`router.rs:248`) and `ModelRegistry::revoke` (`model_registry.rs:190`). The hot-swap test, `ac_ms_9_cross_family_swap_is_hot_swappable_without_contract_change` ([`tests/router.rs:228`](../../../mimir/model-server/tests/router.rs)), revokes one family in an in-memory registry and re-serves through `LocalReference`, the deterministic engine — no model is loaded or unloaded. The running service, [`mimir/mimir-server/src/main.rs:151`](../../../mimir/mimir-server/src/main.rs), constructs a single `SidecarBackend` and uses neither `Router` nor `ModelRegistry`. `README.md:569`: "One real model, on CPU"; the vLLM GPU path "has not been run". The one real model run is Granite, which is a fourth family and not one of the three (`artifact.rs:281` lists four; `adapter.rs:136` registers three).

**Until this lands, the claim reads:** "Routing across model families and hot-swap between them are implemented and tested as a library against a deterministic engine (`AC-MS-9`). The running inference service serves one model; one real model, IBM Granite 3.3 2B on CPU, has been served end to end."

**Prompt**

```
Put the router and registry into mimir-server and swap real models at run time.

1. mimir-server owns a ModelRegistry and a Router. At start it loads every
   staged model directory under its models volume, verifying each through the
   existing load gate (signed manifest, digest, size, format, family, hardware,
   licence). A directory that fails verification is not registered; the service
   still starts if at least one model verifies and refuses otherwise.
2. One engine per loaded model: each has its own sidecar on its own loopback
   port, started and supervised by the service, with its own readiness. The
   Router selects by workload class, hardware and routing policy; the request
   and response types on the wire do not change.
3. Add a governed control surface over PQ-mTLS: ListModels, SetModelState
   (approve | deprecate | revoke) and ReloadModels. State changes need the
   Model-Steward role and dual control, are authorised by policy.json and are
   written to the service's ledger before they take effect.
4. Hot-swap semantics. Revoking a model: requests already being served by it
   complete; no new request is routed to it; its engine is stopped once idle or
   after a bounded drain. Approving a newly staged model: it is verified,
   its engine started, and it receives traffic only once ready. At no point does
   a request fail because a swap is in progress, and a swap never falls back to
   an unverified or unpermitted family — if no permitted family is eligible the
   request is refused (RouteError), as the library already does.
5. Every completion records the family and artefact id that served it in the
   response metadata and the audit entry, so a swap is visible per call.
6. Reconcile the family lists: one authoritative set used by the adapter set,
   the licence policy and the routing policy.
7. Extend mg-model with `list` and `set-state`, and add `just compose-swap-smoke`:
   stage two real models from two different families whose licences the licence
   policy admits, start the service, run a continuous stream of certified calls,
   revoke the serving family, and assert that every call succeeded, that calls
   after the swap were answered by the other family, and that re-approving
   restores it.
8. Claim discipline: the number of families described as "demonstrated" is the
   number that have served a real model in a recorded smoke run. A third family
   on GPU is demonstrated only once RC-40.4 passes on a GPU node.
```

**Acceptance criteria**

- **AC-RF-CA-8a — The service routes.** *Given* `mimir-server` with two verified models from different families, *when* certified calls of different workload classes arrive, *then* each is served by the family the routing policy selects, and the audit entry names it.
- **AC-RF-CA-8b — A swap drops no request.** *Given* a continuous stream of calls, *when* the serving family is revoked, *then* every call in the stream succeeds, and calls begun after the revocation are served by another permitted family.
- **AC-RF-CA-8c — Swaps are governed.** *Given* a `SetModelState` request without the Model-Steward role, or with the same administrator as requester and approver, *when* it is submitted, *then* it is refused and the refusal is audited.
- **AC-RF-CA-8d — No unverified fallback.** *Given* a revocation that leaves no permitted, verified family, *when* a call arrives, *then* it is refused; it is not answered by the deterministic engine or an unpermitted model.
- **AC-RF-CA-8e — The contract is unchanged.** *Given* a client built before this item, *when* it calls the swapped service, *then* it works without recompilation.

**Done when:** `just compose-swap-smoke` is green with two real families and is a step in the recorded gate run; the claims ledger row is `operable` and states how many families have been demonstrated.

---

### RF-CA-9 · Zero-downtime upgrade of a deployed service

**Severity:** High · **Area:** Claim alignment · Deployment · **Depends on:** RF-DEP-2, RF-PER-7 · **Extends:** RF-B-3, RF-RES-3

> **Status at 2026-10-12 — Open.** As the Evidence below describes: upgrade logic is a tested library; every deployed service runs one replica.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> Checked: `deploy/helm/midgard/templates/workload.yaml:47` · `services/scheduler/src/lib.rs:175`

**Evidence.** `Scheduler::rolling_upgrade` and `drive_rolling_upgrade` ([`services/scheduler/src/lib.rs:175`](../../../services/scheduler/src/lib.rs), `:242`) are tested by `tests/rollout.rs` (`AC-M-7`) and `tests/rollout_real.rs:209`, whose "real" fleet is replicas inside the test process. `bifrost/rollout/src/orchestrator.rs` implements blue-green ring promotion against a trait. Neither crate has a binary. The chart sets `replicas: 1` ([`deploy/helm/midgard/templates/workload.yaml:47`](../../../deploy/helm/midgard/templates/workload.yaml)) for every service, and `README.md:594` explains why: "Each service holds a single-writer, locked state directory, so a second replica would be refused". `README.md:568`: the fleet manager "reports drift but does not push upgrades: nothing drives the rollout orchestrator from it yet". Upgrading any deployed service today means stopping its only replica.

**Until this lands, the claim reads:** "Rolling and blue-green upgrade logic with health-gated rollback is implemented and tested as a library (`AC-M-7`, `AC-B-2`). Deployed services run one replica each and restart on their recovered state (`RC-40.1`); upgrading one interrupts it."

**Prompt**

```
Upgrade a deployed service under load without failing a request.

1. Write the replica-model ADR first. For each service state what is
   single-writer and why, and choose per service: (a) stateless replicas over
   shared state, (b) per-replica ledgers with a replica id that the verifier
   merges into one verifiable history, or (c) leader with warm standby and
   fenced failover. An unaudited or doubly-audited request is not an acceptable
   outcome of any choice.
2. Implement it for mimir-server first — the request path is closest to
   stateless — then http-gateway. Record services that stay single-replica, and
   exclude them by name from the zero-downtime claim.
3. Chart: replicas >= 2 for covered services, a PodDisruptionBudget, a rolling
   strategy with maxUnavailable 0, readiness that goes false before shutdown
   and a preStop drain, so in-flight requests finish. PQ-mTLS clients retry on
   another replica when a connection is refused during the roll.
4. Health-gated rollback: an upgrade to an image that never becomes ready, or
   that fails its post-roll probe, is rolled back automatically, and the old
   version served throughout.
5. Add `just kind-upgrade-smoke`: with a continuous stream of certified calls
   running, upgrade the service from version N to N+1, then attempt an upgrade
   to a deliberately broken image. Assert zero failed requests in both cases,
   that the broken upgrade rolled back, and that the service ledger verifies
   end to end afterwards with no gap and no duplicate sequence.
6. Drive it from Bifrost: bifrost-fleet invokes the rollout orchestrator to move
   a channel's desired version ring by ring, using the mechanism above for each
   installation (RF-B-3).
7. Add the smoke as a deployment-tier criterion alongside RC-40.1..4.
```

**Acceptance criteria**

- **AC-RF-CA-9a — No request fails during an upgrade.** *Given* a covered service under continuous certified load on kind, *when* it is upgraded from N to N+1, *then* every request succeeds.
- **AC-RF-CA-9b — A bad upgrade rolls back.** *Given* an upgrade to an image that never becomes ready, *when* it is applied, *then* it is rolled back automatically and no request fails.
- **AC-RF-CA-9c — The audit history stays whole.** *Given* the ledger after an upgrade under load, *when* it is verified, *then* every served request has exactly one entry and the chain verifies.
- **AC-RF-CA-9d — Bifrost drives it.** *Given* a channel whose desired version changes, *when* the fleet manager acts, *then* installations upgrade ring by ring and the fleet converges without a manual step.
- **AC-RF-CA-9e — The claim names its scope.** *Given* the claims ledger row, *when* read, *then* it lists the services covered and the services that remain single-replica.

**Done when:** `just kind-upgrade-smoke` is green for at least `mimir-server` and is recorded in a deployment-tier run; the ledger row is `operable` for the named services only.

---

### RF-CA-10 · Heimdall option generation in the running service

**Severity:** High · **Area:** Claim alignment · Heimdall · **Depends on:** RF-MI-1, RF-H-1 · **Extends:** RF-H-4

> **Status at 2026-10-12 — Open.** As the Evidence below describes: option generation is a tested library that the feed server does not use.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> Checked: `heimdall/decision-support/src/adapter.rs:62` · `heimdall/feed-server/Cargo.toml:25`

**Evidence.** `request_options` ([`heimdall/decision-support/src/adapter.rs:62`](../../../heimdall/decision-support/src/adapter.rs)) parses a ranked option set and grounds every citation against the picture; `MimirOptionProvider` (`mimir_bridge.rs`) is a live provider over PQ-mTLS; `tests/ac_h_3.rs` and the six `EV-A8-*` Evals (`evals.rs`) cover it. The tests use `MockOptionProvider`, which returns a fixed string. `request_options` returns an ungrounded option flagged as such; it does not reject the response, so the rejection in step 3 below is new behaviour, not existing behaviour to expose. [`heimdall/feed-server/Cargo.toml`](../../../heimdall/feed-server/Cargo.toml) does not depend on `heimdall-decision-support` or `heimdall-workflow`. `README.md:570`: "Nothing calls Mimir yet… Only `mimir-probe` does"; `:566`: "no classifier is attached".

**Until this lands, the claim reads:** "Advisory option generation — ranked options, each cited and grounded against the operating picture — is implemented and tested as a library (`AC-H-3`, `EV-A8`). The running feed gateway fuses and serves the picture; it does not yet generate options."

**Prompt**

```
Serve advisory options from heimdall-feed, generated by the real inference plane.

1. heimdall-feed gains a RequestOptions call over PQ-mTLS. The caller needs a
   certificate from the enclosure's issuer and a policy grant for the
   `RequestOptions` action; the request names a decision point and a purpose.
   Permit and deny are both audited before anything else happens.
2. The service assembles the A.8 prompt from the current sealed picture — only
   entities the caller is cleared to read — and calls mimir-server through
   MimirOptionProvider using its own mesh identity. Grant heimdall-feed `Infer`
   in the compose and chart policy, and open exactly that one path in the
   network policy.
3. Enforce EV-A8 at run time, not only in tests: a response that does not parse,
   an option with no citation, or a citation that does not resolve to an entity
   in the picture the caller can read rejects the whole response. Insufficient
   grounding returns an explicit "insufficient evidence" result with no options.
4. Advisory is structural: the response type carries no executable action, and
   heimdall-feed gains no path from an option to a fabric mutation. Add the test
   that asserts this by construction (AC-RF-H-4a).
5. Fail closed: Mimir unreachable, the call denied, or the latency budget
   exceeded returns a typed "unavailable" result. Fusion, sealing and picture
   reads are unaffected.
6. Record lineage with each response: picture version, prompt id and version,
   model family and artefact id, the Mimir audit sequence, the caller and the
   time — in the Heimdall ledger, so a response can be reconstructed.
7. Add the client binary heimdall-options and a compose step, run against the
   Granite override: submit observations, request options, assert the options
   are grounded in those observations; stop mimir-server and assert a typed
   unavailable result while picture reads continue. Add it to
   compose-granite-smoke and kind-smoke.
```

**Acceptance criteria**

- **AC-RF-CA-10a — Options come from the real plane.** *Given* the compose enclosure serving a real model and a picture with fused entities, *when* a cleared analyst requests options, *then* ranked options are returned whose every citation resolves to an entity in that picture.
- **AC-RF-CA-10b — Ungrounded output is rejected.** *Given* a model response with an uncited option or a citation to an unknown entity, *when* the service evaluates it, *then* no option is returned and the rejection is audited.
- **AC-RF-CA-10c — Unavailable is explicit.** *Given* `mimir-server` stopped, *when* options are requested, *then* a typed unavailable result is returned and picture reads still succeed.
- **AC-RF-CA-10d — Clearance bounds the prompt.** *Given* a picture holding an entity above the caller's clearance, *when* options are requested, *then* that entity appears in neither the prompt sent to Mimir nor any citation.
- **AC-RF-CA-10e — Responses are reconstructible.** *Given* a past response, *when* its ledger entry is read, *then* picture version, prompt version, model artefact, caller and time are all present.

**Done when:** the compose and kind smokes include an options request answered by a real model, and the ledger row is `operable`.

---

### RF-CA-11 · Admin console alerts on a real backend

**Severity:** High · **Area:** Claim alignment · Admin · **Depends on:** RF-RT-1, RF-PER-4 · **Extends:** RF-AD-1, RF-AD-2, RF-OBS-4

> **Status at 2026-10-12 — Open.** As the Evidence below describes: break-glass alerting is a tested library; the console renders a constant.
> Criteria met 0 of 5; open: a, b, c, d, e. Confidence: high.
>
> Checked: `admin/breakglass/src/alert.rs:84` · `products/admin/ui/src/screens/BreakGlassScreen.tsx:33`

**Evidence.** `AlertBus`, `DurableAlerter` and `SecurityOfficerBus` ([`admin/breakglass/src/alert.rs:73`](../../../admin/breakglass/src/alert.rs), `:84`, `:168`) implement at-least-once break-glass alert delivery, exercised by `admin/audit/tests/ac_ad_6_10_12.rs` with `RecordingAlerter`. No admin crate has a `main.rs`. The console's alert table is a constant: `const ALERTS = [` at [`products/admin/ui/src/screens/BreakGlassScreen.tsx:33`](../../../products/admin/ui/src/screens/BreakGlassScreen.tsx). `README.md:254`: the admin console runs "only against [its] in-browser `MockGateway` fixtures"; `:421`: "No administration console". The only alerting in the tree is break-glass; platform alerting (RF-OBS-4) does not exist.

**Until this lands, the claim reads:** "Break-glass alerting with durable, at-least-once delivery is implemented and tested as a library (`AC-AD-8`). The admin console is a design-complete interface running on fixture data; it is not connected to a backend."

**Prompt**

```
Show real alerts in the admin console, and nothing else under that name.

1. Stand up the admin service: a binary on services/runtime composing
   admin-breakglass, admin-audit and admin-privilege, with a sealed, durable
   alert store. An alert is persisted before the action that raised it takes
   effect; pending and delivered alerts survive restart.
2. Implement AlertBus against that store. Opening, extending, expiring and
   reviewing a break-glass window each raise an alert. A window cannot open if
   its alert cannot be persisted.
3. Expose two calls: ListAlerts (filter by kind, state, time) and
   AcknowledgeAlert. Both require the Security-Officer or Auditor role, are
   authorised by policy and audited. Acknowledgement records who and when and
   never deletes the alert.
4. Reach it from the browser through the gateway edge: GET /api/v1/admin/alerts
   for a logged-in user holding the role, with the same per-request policy and
   audit as every other route. Acknowledgement stays on mg-admin with a passkey
   until the console can produce passkey assertions (RF-UI-1); say so in the UI.
5. Console: in HTTP mode BreakGlassScreen reads ListAlerts and the ALERTS
   constant is deleted. Add loading, empty, error and stale states (RF-UI-2).
   The nav badge shows the real unacknowledged count. In mock mode the screen is
   labelled as demonstration data.
6. Add console-admin's origin to the gateway allowlist and a compose step: open
   a break-glass window with mg-admin, read the alert through the edge as a
   Security-Officer, restart the admin service, read it again.
7. Scope the claim: this is break-glass alerting. General platform alerting —
   service health, security detections, on-call routing — is RF-OBS-4 and is
   not implied by it.
```

**Acceptance criteria**

- **AC-RF-CA-11a — A real event shows in the console.** *Given* the compose enclosure, *when* a break-glass window is opened with `mg-admin`, *then* the admin console in HTTP mode lists that alert with its window id, requester and time.
- **AC-RF-CA-11b — No window without an alert.** *Given* an alert store that refuses writes, *when* a break-glass window is requested, *then* it does not open.
- **AC-RF-CA-11c — Alerts survive restart.** *Given* an unacknowledged alert, *when* the admin service restarts, *then* the alert is still listed and still unacknowledged.
- **AC-RF-CA-11d — Reading is governed.** *Given* a logged-in user without the Security-Officer or Auditor role, *when* the alerts route is requested, *then* it is denied and the denial is audited.
- **AC-RF-CA-11e — No fixture in the shipped path.** *Given* the admin console built for HTTP mode, *when* its bundle is searched, *then* the fixture alert list is absent.

**Done when:** the compose step is green and recorded, the fixture is gone from the HTTP build, and the ledger row is `operable` and scoped to break-glass alerts.

---

### RF-CA-12 · Multimodal ingest — georeferencing, captions, transcripts

**Severity:** High · **Area:** Claim alignment · Forge · **Depends on:** RF-F-2, RF-MI-1, RF-MS-1 · **Extends:** RF-H-5

> **Status at 2026-10-12 — Open.** Nothing exists: no requirement id, no media or geometry type, no captioning or transcription code.
> Criteria met 0 of 6; open: a, b, c, d, e, f. Confidence: high.
>
> Checked: `forge/connector-sdk/src/connectors/file.rs:1`

**Evidence.** No file outside `vendor/` matches `multimodal`, and none implements georeferencing, captioning or transcription. Forge's connectors are the CSV file reader, PostgreSQL, S3 and Kafka (`forge/connector-sdk/src/connectors/`), all producing rows of scalar values. The ontology's property types are `text`, `int`, `float`, `bool` and `timestamp` (`README.md:316`) — there is no geometry type and no media type. No `FR-*` id in the SADs requires any of this. It is new scope, not an unfinished item.

**Until this lands, the claim reads:** "Forge ingests tabular data from files, PostgreSQL, S3-compatible stores and Kafka. Ingest of imagery, audio and video — with georeferencing, captioning and transcription — is specified for delivery in <milestone>; it is not implemented."

**Prompt**

```
Specify multimodal ingest, then build it under the same rules as everything else.

Part 1 — specification, before any code.
1. Raise a design change request against SAD_01 (Forge) and SAD_02 (Mimir):
   requirement ids for media assets, georeferencing, captioning and
   transcription, with their security requirements, and acceptance criteria
   numbered to extend the 72. Until these ids exist nothing can cite them, and
   rule 2 of §0 cannot be met.

Part 2 — media as governed data.
2. dataset-store gains a media asset: a content-addressed, sealed blob plus a
   metadata record (media type detected from the bytes, never from the file name
   or a declared header; size; digest; source lineage). Blobs stream; they are
   never held whole in memory. Unknown or mismatched types are quarantined.
3. Derived artefacts — a location, a caption, a transcript — are separate
   records that carry lineage to the source blob's digest, the tool or model
   that produced them, and its version. They inherit the source's marking,
   compartments and purposes and can never be less restricted than it.

Part 3 — georeferencing.
4. Extract location from the asset itself: EXIF GPS tags, GeoTIFF tags, a
   sidecar world file, or GeoJSON. Normalise to WGS 84, validate ranges, record
   the original reference system and the stated accuracy. A missing or
   implausible location is recorded as absent, never guessed.
5. Add a `geopoint` (and `geoshape`) property type to the ontology file and the
   fabric, with validation at import and at SyncRecords, so a georeferenced
   asset projects onto objects Heimdall can place (RF-H-5).

Part 4 — captions and transcripts, generated locally.
6. Captions: a vision-capable model served by mimir-server through the governed
   inference path — verified manifest, policy check, audit. A caption is stored
   as model-generated, with the model's artefact id and digest, and is never
   presented as a fact about the source.
7. Transcripts: a local speech-recognition engine as a second sidecar type, its
   model staged through mg-model with the same signed manifest and load gate.
   Output carries segment timestamps, detected language and per-segment
   confidence.
8. Nothing leaves the enclosure: no hosted model, no network call. With no
   suitable model staged, the derived artefact is absent and the asset is still
   ingested.
9. Evals for both: a fixed, licence-clean sample set with reference captions and
   transcripts and a pass threshold, run in the Evals harness.

Part 5 — proof.
10. A compose job ingests a sample image with EXIF location, a GeoTIFF and an
    audio clip, and syncs the resulting objects. Assert the stored locations,
    that the caption and transcript exist with their lineage, and that a caller
    without clearance for the source can read none of the derived artefacts.
```

**Acceptance criteria**

- **AC-RF-CA-12a — Requirements exist first.** *Given* the SADs, *when* searched, *then* media, georeferencing, captioning and transcription each have requirement ids and acceptance criteria, and the implementation cites them.
- **AC-RF-CA-12b — Location is extracted, not invented.** *Given* an image with EXIF GPS, a GeoTIFF and an image with no location, *when* ingested, *then* the first two project with the correct WGS 84 coordinates and the third has no location.
- **AC-RF-CA-12c — Derived artefacts are local and attributed.** *Given* an ingest under `--network none`, *when* a caption and a transcript are produced, *then* each records the model artefact that produced it and is labelled model-generated.
- **AC-RF-CA-12d — Markings propagate.** *Given* a source asset with a marking and compartment, *when* a caller without that clearance queries, *then* neither the asset nor its location, caption or transcript is returned.
- **AC-RF-CA-12e — Type is determined from content.** *Given* a file whose name and declared type disagree with its bytes, *when* ingested, *then* it is quarantined.
- **AC-RF-CA-12f — Absence degrades cleanly.** *Given* no captioning or transcription model staged, *when* media is ingested, *then* the asset is stored and georeferenced and the derived artefacts are recorded as not produced.

**Done when:** the SAD change is accepted, the compose job is green under `--network none`, and the Evals meet their thresholds. Until then the ledger row is `spec`.

---

### RF-CA-13 · Likan — state the boundary with the proprietary tool, and prove the interchange

**Severity:** High · **Area:** Claim alignment · **Depends on:** — · **Blocks:** RF-CA-1

> **Status at 2026-10-12 — Open.** The tree does not mention Likan, and the ontology file format has no published schema.
> Criteria met 0 of 4; open: a, b, c; not verifiable by reading: d. Confidence: high.
>
> Checked: `services/http-gateway/src/ontology_file.rs:114`

**Evidence.** Likan is a separate, proprietary tool for data-structure ontologies. It is not part of this repository and is not meant to be: the name does not occur anywhere in the public tree — not in source, the workspace manifest, the SADs, the playbook, the design system or the launch deck's build script. Nothing in the tree says that it exists, that it is separate, or how an ontology produced with it reaches an enclosure. Midgard's own ontology support is public and `operable`: the gateway parses and registers `ontology.json` at every start ([`services/http-gateway/src/ontology_file.rs:114`](../../../services/http-gateway/src/ontology_file.rs), `enclosure.rs:727`) and `mg-admin import` checks every record against it. That file format is described only in prose in the README; it has no published schema, so there is no contract a separate tool can be shown to meet.

**Until this lands, the wording is:** "Likan is a separate, proprietary tool for data-structure ontologies. It is not part of the open-source Midgard repository and is not covered by its licence, its acceptance gate, its SBOMs or its published figures. Midgard loads ontologies from a documented file format and needs no other tool to do so."

**Prompt**

```
State where Midgard ends and Likan begins, and make the join testable.

1. Add a short "Related proprietary tools" note to the README: what Likan is in
   one sentence, that it is proprietary and separate, and that nothing in this
   repository's licence, gate, SBOMs, numbers sheet or claims covers it. No
   public document may describe a Likan capability as a Midgard capability.
2. Give it a row in the claims ledger (RF-CA-1) with the status `external`: a
   status for things that exist outside this repository and that this
   repository makes no verifiable claim about. Add `external` to the status
   vocabulary, with the rule that an `external` row carries no evidence paths
   and may never be counted among Midgard's capabilities or figures.
3. Publish the ontology file format as a contract: a versioned JSON Schema for
   ontology.json under contracts/, generated from or checked against the parser
   in services/http-gateway/src/ontology_file.rs so the two cannot drift, with
   the README section reduced to a pointer plus one example. Do the same for
   the JSON Lines import record.
4. Record, in the ledger row, whether Likan exports that format. If it does,
   add one export — cleared for publication — as a fixture, and a test that it
   validates against the schema, registers in the gateway and accepts an
   import. If it does not, say that the two are not integrated; do not imply a
   pipeline that has not been run.
5. Keep the core independent: an ontology can be written by hand against the
   schema, and the whole acceptance gate passes with no Likan artefact present.
   Add a check that nothing in the workspace depends on, downloads or requires
   a proprietary tool.
```

**Acceptance criteria**

- **AC-RF-CA-13a — The boundary is stated.** *Given* the README, *when* read, *then* it says Likan is a separate, proprietary tool that this repository does not contain or cover.
- **AC-RF-CA-13b — Nothing is attributed across the boundary.** *Given* every document in the repository, *when* searched, *then* no Likan capability is described as part of Midgard, and the ledger row is `external` with no evidence paths.
- **AC-RF-CA-13c — The interchange is a tested contract.** *Given* the published schema, *when* an ontology file violates it, *then* schema validation and the gateway both refuse it; *and* a change to the parser that the schema does not reflect fails CI.
- **AC-RF-CA-13d — Midgard stands alone.** *Given* a fresh clone with no proprietary artefact present, *when* the acceptance gate runs, *then* it passes, and the dependency check finds no proprietary requirement.

**Done when:** the README note and ledger row exist, the schema is published and checked against the parser, and the integration statement in step 4 is recorded either way.

---
# Appendix 1 — Complete item register (182)

Severity: **C**ritical · **H**igh · **M**edium · **L**ow.

| ID | Title | Sev | Depends on | Status | Criteria met |
|---|---|---|---|---|---|
| **A — Supply chain & release integrity** |
| RF-SC-1 | Implement `mg-sign` — ML-DSA signature + DSSE attestation | C | — | Done | 6/6 |
| RF-SC-2 | Implement `mg-verify` — fail-closed verification | C | SC-1 | Done | 5/5 |
| RF-SC-3 | Make the reproducibility check non-vacuous in CI | C | — | Partial | 3/5 |
| RF-SC-4 | Vulnerability scanning and licence allow-list over the SBOM | C | SC-2 | Partial | 2/6 |
| RF-SC-5 | Extend the release gate to every buildable artefact | H | SC-1..4 | Partial | 0/5 |
| RF-SC-6 | Implement the provenance-tag lint | M | — | Partial | 0/4 |
| RF-SC-7 | Vendored-dependency policy and integrity check | M | SC-4 | Open | 0/4 |
| RF-SC-8 | Release key rotation, revocation, trust distribution | H | SC-1, SC-2 | Partial | 1/5 |
| **B — Runtime & process model** |
| RF-RT-1 | Give every service a process: binary, config, lifecycle | C | — | Partial | 1/7 |
| RF-RT-2 | Concurrency and asynchrony model | H | RT-1 | Partial | 0/5 |
| RF-RT-3 | Structured, correlated, redacting logging | H | RT-1 | Partial | 0/5 |
| RF-RT-4 | Time, clocks and determinism | M | RT-1 | Open | 0/5 |
| RF-RT-5 | Randomness, entropy and nonce discipline | H | RT-1 | Partial | 0/5 |
| RF-RT-6 | Resource governance: memory, descriptors, CPU | M | RT-2 | Open | 0/4 |
| RF-RT-7 | Feature flags, safe defaults, hardening baseline | M | RT-1 | Open | 0/4 |
| **C — Persistence & durability** |
| RF-PER-1 | Choose and implement the storage substrate | C | RT-1 | Partial | 2/7 |
| RF-PER-2 | Durable, tamper-evident audit ledger | C | PER-1 | Partial | 1/7 |
| RF-PER-3 | Object fabric storage: indexes, scale, query performance | H | PER-1 | Partial | 1/6 |
| RF-PER-4 | Durable registries: models, artefacts, datasets, config | H | PER-1 | Partial | 1/6 |
| RF-PER-5 | Client-side persistence: store, keystore, sync queue | H | PER-1 | Open | 0/6 |
| RF-PER-6 | Data lifecycle: retention, archival, erasure | H | PER-1, PER-2 | Open | 0/5 |
| RF-PER-7 | Transactionality across service boundaries | H | PER-1, API-1 | Open | 0/4 |
| RF-PER-8 | Storage operations: capacity, compaction, scrubbing | M | PER-1 | Open | 0/4 |
| **D — Transport & API surface** |
| RF-API-1 | Complete the proto contract for every subsystem | H | RT-1 | Partial | 0/6 |
| RF-API-2 | Implement the inter-service RPC layer over PQ-mTLS | H | API-1, RT-1, RT-2 | Partial | 0/6 |
| RF-API-3 | Complete the browser edge for all six products | H | API-1, API-2, IAM-3 | Partial | 0/6 |
| RF-API-4 | API versioning, deprecation, compatibility | M | API-1 | Open | 0/3 |
| RF-API-5 | API documentation and an offline explorer | M | API-1 | Open | 0/4 |
| RF-API-6 | Webhooks, egress control, outbound integration policy | M | API-2, RT-7 | Partial | 0/5 |
| RF-API-7 | Rate limiting, quotas, abuse resistance | M | API-2, API-3 | Partial | 0/4 |
| **E — Identity, authentication & session** |
| RF-IAM-1 | Human authentication: credentials, MFA, enrolment | C | PER-4 | Partial | 0/6 |
| RF-IAM-2 | Session management, token lifecycle, revocation | C | IAM-1, PER-4 | Partial | 0/6 |
| RF-IAM-3 | Authorise the browser edge; server-owned policy context | C | IAM-2, API-3 | Partial | 0/4 |
| RF-IAM-4 | Optional federation: OIDC, SAML, SCIM | M | IAM-1, API-6 | Partial | 0/5 |
| RF-IAM-5 | Role model, least privilege, separation of duties | H | IAM-1 | Partial | 0/5 |
| RF-IAM-6 | Service and machine identity lifecycle | H | API-2 | Partial | 0/4 |
| **F — Observability & operations telemetry** |
| RF-OBS-1 | Metrics for every service and every governed decision | H | RT-1, RT-3 | Partial | 0/6 |
| RF-OBS-2 | Distributed tracing across services, agents and the UI | H | RT-3, API-2 | Partial | 0/5 |
| RF-OBS-3 | Log and telemetry pipeline, retention, governed access | M | RT-3, PER-6 | Open | 0/4 |
| RF-OBS-4 | Alerting, on-call and incident response integration | H | OBS-1, API-6 | Partial | 0/4 |
| RF-OBS-5 | Health, dependency and readiness surfaces | M | RT-1 | Partial | 1/4 |
| **G — Mimir: real inference** |
| RF-MI-1 | Launch and supervise the GPU serving sidecar | H | RT-1, DEP-1 | Partial | 2/6 |
| RF-MI-2 | Model loading, tokenizers, per-family adapters | H | MI-1, MS-1 | Partial | 1/6 |
| RF-MI-3 | Serving performance: batching, KV-cache, admission | M | MI-1, MI-2 | Partial | 0/5 |
| RF-MI-4 | Real tool execution in the governed loop | H | API-2, MI-1 | Partial | 2/6 |
| RF-MI-5 | Memory, sessions and multi-turn context | M | PER-1, MI-2 | Open | 0/5 |
| RF-MI-6 | Evals as a continuous, gating governance system | H | MI-2, OBS-1 | Open | 0/6 |
| RF-MI-7 | Guardrail depth and adversarial resistance | H | MI-6 | Partial | 0/6 |
| RF-MI-8 | Cost, quota and capacity governance for AI usage | M | MI-3, API-7 | Open | 0/4 |
| **H — Model supply chain** |
| RF-MS-1 | End-to-end intake of a real upstream model | H | SC-1, SC-2, PER-4 | Partial | 3/6 |
| RF-MS-2 | Licence, restriction and export-control enforcement | H | MS-1, CMP-5 | Open | 0/4 |
| RF-MS-3 | Model versioning, rollback and deprecation | M | MS-1, MI-6 | Open | 0/4 |
| RF-MS-4 | Fine-tuning, adapters and derived-model governance | M | MS-1, MS-2 | Open | 0/4 |
| RF-MS-5 | Multi-family routing policy and portability certification | M | MI-2, MI-6 | Open | 0/4 |
| **I — Forge: execution depth** |
| RF-F-1 | A real execution backend behind the codegen seam | H | PER-1, RT-2 | Partial | 2/5 |
| RF-F-2 | Real connectors and ingestion | H | F-1, API-6 | Partial | 1/6 |
| RF-F-3 | Pipeline orchestration, scheduling, dependencies | H | F-1, PER-4 | Partial | 0/6 |
| RF-F-4 | Data quality: expectations, profiling, quarantine | M | F-3 | Open | 0/4 |
| RF-F-5 | Dataset versioning, branching, time travel at scale | M | PER-1, F-1 | Partial | 0/4 |
| RF-F-6 | Projection: incremental, consistent, reversible | H | PER-3, F-3 | Partial | 0/6 |
| RF-F-7 | Transform IDE and NL-to-transform, made real | M | MI-4, UID-2 | Partial | 1/5 |
| **J — Bifrost: delivery reality** |
| RF-B-1 | Artefact registry with real content and verification | H | SC-1, SC-2, PER-4 | Partial | 0/5 |
| RF-B-2 | Fleet agent: install, report, roll back on the target | H | B-1, IAM-6 | Partial | 0/6 |
| RF-B-3 | Ring orchestration, progressive delivery, auto-abort | H | B-2, OBS-1 | Open | 0/6 |
| RF-B-4 | Air-gapped bundle workflow, end to end | H | B-1, SC-8 | Partial | 1/6 |
| RF-B-5 | Release-risk intelligence wired to real evidence | M | SC-4, MI-4 | Open | 0/4 |
| RF-B-6 | Configuration and secret delivery | H | B-2, PER-4 | Open | 0/5 |
| **K — Heimdall: fusion reality** |
| RF-H-1 | Real feed ingestion and normalisation | H | API-6, RT-2 | Partial | 0/5 |
| RF-H-2 | Entity resolution, tracking, correlation at scale | H | H-1, PER-3 | Open | 0/5 |
| RF-H-3 | Real-time picture synchronisation across sites | H | API-3, PER-5 | Partial | 0/5 |
| RF-H-4 | Decision workflows and course-of-action support | H | MI-4, H-2 | Partial | 0/6 |
| RF-H-5 | Geospatial and temporal capability | M | H-2 | Open | 0/5 |
| RF-H-6 | Alerting, watch conditions, attention management | M | H-2, OBS-4 | Open | 0/5 |
| **L — Admin Control Panel depth** |
| RF-AD-1 | Wire the Admin console to the real admin backend | H | API-3, IAM-3 | Partial | 0/5 |
| RF-AD-2 | Break-glass: end-to-end operational flow | H | AD-1, IAM-1, OBS-4 | Partial | 1/6 |
| RF-AD-3 | GitOps configuration and drift reconciliation | M | AD-1, B-6 | Partial | 0/5 |
| RF-AD-4 | Approvals, delegation and the operator work queue | H | AD-1, IAM-5 | Open | 0/5 |
| RF-AD-5 | Governance surfaces: policy, markings, purposes, retention | H | AD-1, PER-6 | Open | 0/5 |
| RF-AD-6 | Operational overview: health, posture, integrity | H | OBS-1, OBS-5, RT-7 | Open | 0/5 |
| RF-AD-7 | Admin copilot and audit investigation on real data | M | AD-1, MI-4, PER-2 | Partial | 1/6 |
| **M — UI platform foundations** |
| RF-UI-1 | Authentication, session and identity in the console shell | C | IAM-1..3 | Partial | 0/7 |
| RF-UI-2 | Error, loading, empty and degraded states | H | UI-1 | Open | 0/6 |
| RF-UI-3 | Accessibility to WCAG 2.2 AA across all six consoles | H | UI-2 | Open | 0/7 |
| RF-UI-4 | Internationalisation and localisation | M | UI-2 | Open | 0/6 |
| RF-UI-5 | Navigation, information architecture, cross-console coherence | M | UI-1 | Open | 0/5 |
| RF-UI-6 | Forms, validation and data entry | M | UI-2, UI-3 | Open | 0/5 |
| RF-UI-7 | Performance, bundle discipline, rendering at scale | M | UID-1 | Open | 0/5 |
| RF-UI-8 | Styling architecture, CSP compatibility, theming | M | API-3 | Open | 0/4 |
| RF-UI-9 | Offline, connectivity and resilience in the browser | M | API-3, UI-2 | Open | 0/5 |
| RF-UI-10 | Frontend security hardening | H | UI-1, API-3 | Partial | 0/6 |
| **N — UI data wiring** |
| RF-UID-1 | Replace every mock gateway with a real client | H | API-3, IAM-3 | Partial | 0/5 |
| RF-UID-2 | Forge console on real pipelines and data | H | UID-1, F-1..7 | Open | 0/5 |
| RF-UID-3 | Midgard core console on real platform state | H | UID-1, OBS-1, OBS-5 | Partial | 0/5 |
| RF-UID-4 | Mimir console on the real model plane | H | UID-1, MI-1..7 | Open | 0/5 |
| RF-UID-5 | Bifrost console on the real fleet | H | UID-1, B-1..6 | Open | 0/5 |
| RF-UID-6 | Heimdall console on real feeds and the real picture | H | UID-1, H-1..6 | Open | 0/5 |
| RF-UID-7 | Clients console on the real client core | M | UID-1, CL-1 | Open | 0/3 |
| **O — End-to-end user flows** |
| RF-UX-1 | First-run: enclosure bootstrap and initial administrator | C | DEP-1, IAM-1, AD-1 | Partial | 0/6 |
| RF-UX-2 | Operator onboarding: enrolment to first productive action | H | UX-1, IAM-1, IAM-5 | Partial | 0/5 |
| RF-UX-3 | Analyst: question to grounded, cited, audited answer | H | UID-3, UID-4, MI-4 | Open | 0/6 |
| RF-UX-4 | Data engineer: source to governed operational objects | H | UID-2, F-2, F-6 | Partial | 0/5 |
| RF-UX-5 | Release: commit to production, with rollback | H | SC-5, UID-5, B-3 | Partial | 0/5 |
| RF-UX-6 | Governance: policy change from proposal to enforcement | H | AD-5, AD-3, AD-4 | Partial | 0/5 |
| RF-UX-7 | Incident: alert to resolution to evidence | H | OBS-4, AD-2, AD-7 | Open | 0/5 |
| RF-UX-8 | Operator: monitor, decide, act on the operating picture | H | UID-6, H-4 | Partial | 0/5 |
| RF-UX-9 | Client: enrolment, offline work, sync, revocation | H | CL-1..6, PER-5 | Open | 0/5 |
| RF-UX-10 | Auditor: independent verification without operational access | H | PER-2, AD-7, CMP-1 | Partial | 0/5 |
| RF-UX-11 | Air-gapped site: install, operate, update, report | H | DEP-5, B-4 | Partial | 0/5 |
| RF-UX-12 | Developer: build on the platform | M | SDK-1..5, API-5 | Partial | 0/4 |
| **P — Client platforms** |
| RF-CL-1 | Language bindings: UniFFI and wasm-bindgen | H | PER-5 | Partial | 0/5 |
| RF-CL-2 | Apple shell (iOS, macOS) | H | CL-1 | Open | 0/5 |
| RF-CL-3 | Android shell | H | CL-1 | Open | 0/5 |
| RF-CL-4 | Web shell (SvelteKit + WASM) | H | CL-1, UI-3 | Open | 0/5 |
| RF-CL-5 | Desktop shell (Tauri) | M | CL-1 | Open | 0/5 |
| RF-CL-6 | Linux GTK shell (mobile and desktop) | M | CL-1 | Open | 0/4 |
| RF-CL-7 | Browser extension (broker mode) | M | CL-1, CL-4 | Open | 0/5 |
| RF-CL-8 | Client delivery, update and integrity | H | B-2, CL-2..7 | Open | 0/5 |
| RF-CL-9 | On-device inference and tier selection | M | MI-2, CL-1 | Partial | 1/5 |
| **Q — SDKs & developer experience** |
| RF-SDK-1 | Generate and complete the TypeScript SDK | M | API-1, UID-1 | Open | 0/5 |
| RF-SDK-2 | Rust SDK | M | API-2 | Open | 0/4 |
| RF-SDK-3 | Python SDK | M | API-3 | Partial | 1/5 |
| RF-SDK-4 | Go SDK, or an honest removal | L | API-3 | Partial | 0/3 |
| RF-SDK-5 | Developer environment, samples, extension templates | M | DEP-1, UX-12 | Partial | 0/4 |
| **R — Deployment & topology** |
| RF-DEP-1 | Container images for every service | C | RT-1 | Partial | 0/5 |
| RF-DEP-2 | Kubernetes deployment: charts, policies, HA topology | C | DEP-1, DEP-3 | Partial | 0/5 |
| RF-DEP-3 | Network policy, mesh interaction, default-deny | C | DEP-2, API-2 | Partial | 0/4 |
| RF-DEP-4 | Installer and upgrade tooling | H | DEP-2, SC-2 | Partial | 0/5 |
| RF-DEP-5 | Air-gapped installation bundle | H | DEP-4, B-4 | Partial | 0/5 |
| RF-DEP-6 | Environment strategy and configuration promotion | M | DEP-2, AD-3 | Open | 0/4 |
| RF-DEP-7 | Capacity, sizing and deployment planning guidance | M | SCL-4, OBS-1 | Open | 0/4 |
| **S — Resilience, backup & DR** |
| RF-RES-1 | Backup: complete, verified, restorable | C | PER-1 | Partial | 1/5 |
| RF-RES-2 | Disaster recovery: documented, exercised, measured | C | RES-1, DEP-4 | Partial | 1/5 |
| RF-RES-3 | High availability and graceful degradation | H | DEP-2, RT-2 | Partial | 0/5 |
| RF-RES-4 | Data integrity assurance and corruption recovery | H | PER-2, PER-8 | Partial | 0/4 |
| RF-RES-5 | Business continuity for the air-gapped case | M | RES-2, UX-11 | Partial | 0/4 |
| **T — Security assurance** |
| RF-SEC-1 | Validate the threat models with tests | C | TST-3 | Partial | 0/4 |
| RF-SEC-2 | Fuzzing and property-based testing of every boundary | H | TST-4 | Open | 0/5 |
| RF-SEC-3 | Static and dynamic analysis in CI | H | SC-4 | Partial | 0/5 |
| RF-SEC-4 | Cryptographic assurance and validation evidence | H | RT-5 | Partial | 1/5 |
| RF-SEC-5 | Penetration testing and red teaming | H | SEC-1, DEP-2 | Open | 0/4 |
| RF-SEC-6 | Security monitoring, detection and response | H | OBS-1, OBS-4 | Partial | 0/4 |
| RF-SEC-7 | Vulnerability management and disclosure | M | SC-4 | Partial | 0/5 |
| RF-SEC-8 | Physical and supply-chain security for air-gap | M | B-4, DEP-5 | Partial | 0/4 |
| **U — Test strategy & coverage** |
| RF-TST-1 | Test strategy, taxonomy and coverage targets | M | — | Open | 0/4 |
| RF-TST-2 | Frontend testing from zero | H | UI-2, UID-1 | Open | 0/5 |
| RF-TST-3 | Integration testing across process boundaries | H | RT-1, API-2 | Partial | 1/4 |
| RF-TST-4 | Fuzzing infrastructure | H | SEC-2 | Open | 0/4 |
| RF-TST-5 | Performance and load testing | H | SCL-4, TST-3 | Open | 0/4 |
| RF-TST-6 | Long-running stability and resource testing | M | RT-6 | Open | 0/4 |
| RF-TST-7 | Regression, benchmark and golden-output suites | M | TST-1 | Open | 0/4 |
| RF-TST-8 | Chaos and fault-injection testing | H | RES-3, TST-3 | Partial | 0/4 |
| **V — Compliance & assurance artefacts** |
| RF-CMP-1 | Control framework mapping with automated evidence | H | OBS-1, PER-2 | Partial | 0/4 |
| RF-CMP-2 | Supply-chain attestation (SLSA) and SBOM distribution | H | SC-1..5 | Partial | 0/4 |
| RF-CMP-3 | Privacy: data protection by design and by default | H | PER-6 | Partial | 1/5 |
| RF-CMP-4 | Records management, retention and legal hold | M | PER-6, PER-2 | Open | 0/4 |
| RF-CMP-5 | Export control, sanctions, jurisdictional constraints | M | MS-2, API-6 | Open | 0/4 |
| RF-CMP-6 | Accessibility conformance reporting | M | UI-3, CL-2..7 | Partial | 0/4 |
| **W — Multi-tenancy, scale & performance** |
| RF-SCL-1 | Decide and implement the tenancy model | H | PER-1, IAM-5 | Open | 0/4 |
| RF-SCL-2 | Horizontal scalability of the data plane | H | PER-3, SCL-1 | Partial | 0/5 |
| RF-SCL-3 | Caching strategy with governance correctness | M | PER-3, SCL-1 | Open | 0/4 |
| RF-SCL-4 | Performance engineering and the published envelope | H | TST-5 | Open | 0/4 |
| RF-SCL-5 | Cost model and resource efficiency | L | SCL-4, MI-8 | Open | 0/4 |
| **X — Documentation, design system & drift** |
| RF-DOC-1 | Reconcile every status claim with the tree | M | — | Partial | 0/5 |
| RF-DOC-2 | Operations documentation and runbooks | H | OBS-4, RES-2 | Partial | 0/5 |
| RF-DOC-3 | User documentation and in-product help | M | UX-* | Open | 0/4 |
| RF-DOC-4 | Architecture documentation currency and ADRs | M | — | Open | 0/4 |
| RF-DOC-5 | Contribution, governance and project documentation | L | — | Partial | 0/3 |
| RF-DOC-6 | Design system: single source, coverage, governance patterns | M | UI-3, UI-6, UI-8 | Open | 0/5 |
| RF-DOC-7 | Public-facing site accuracy and brand consistency | L | DOC-1 | Partial | 0/4 |
| **Y — Claim alignment & public release** |
| RF-CA-1 | Claims ledger: one row per published claim, with status and evidence | C | — | Open | 0/4 |
| RF-CA-2 | One numbers sheet, generated from the tag | C | CA-5 | Partial | 0/4 |
| RF-CA-3 | Restore the hermetic build configuration to the public tree | C | — | Open | 0/3 |
| RF-CA-4 | Publish the CI and release workflows the documents cite | H | CA-3 | Open | 0/3 |
| RF-CA-5 | Re-run the acceptance gate at the release commit and record it | C | CA-3 | Open | 0/4 |
| RF-CA-6 | Signed, tagged public release | C | CA-1..5, SC-8 | Open | 0/5 |
| RF-CA-7 | History, classification and publication statements | H | — | Partial | 0/4 |
| RF-CA-8 | Multi-family routing and hot-swap in the running inference plane | H | MI-2, MS-1 | Open | 0/5 |
| RF-CA-9 | Zero-downtime upgrade of a deployed service | H | DEP-2, PER-7 | Open | 0/5 |
| RF-CA-10 | Heimdall option generation in the running service | H | MI-1, H-1 | Open | 0/5 |
| RF-CA-11 | Admin console alerts on a real backend | H | RT-1, PER-4 | Open | 0/5 |
| RF-CA-12 | Multimodal ingest: georeferencing, captions, transcripts | H | F-2, MI-1, MS-1 | Open | 0/6 |
| RF-CA-13 | Likan: state the boundary with the proprietary tool, prove the interchange | H | — | Open | 0/4 |

**Severity totals:** Critical 23 · High 96 · Medium 59 · Low 4 — 182 items, 881 acceptance criteria.

---

# Appendix 2 — Suggested phasing

The dependency graph in §3 constrains order; this is a workable cut of it into shippable phases. Each phase ends with a gate that is a demonstrable capability, not a date.

### Phase R0 — Stop the bleeding (weeks 1–4)

The items where the repository currently asserts something that is not true. Cheap, high-leverage, and they restore the project's credibility before anything else is built on it.

`RF-SC-1` · `RF-SC-2` · `RF-SC-3` · `RF-SC-6` · `RF-DOC-1` · `RF-DOC-5` · `RF-TST-1`

**Gate:** `just gate <artefact>` fails on an unsigned, unverified or non-reproducible artefact; CI's reproducibility job actually builds; every status claim in the repository is true.

### Phase RY — Say only what is true (now; tag due 2026-10-22 18:00)

Added in Revision 2. It runs ahead of, and independently of, the phases below.

`RF-CA-1` · `RF-CA-2` · `RF-CA-3` · `RF-CA-4` · `RF-CA-5` · `RF-CA-6` · `RF-CA-7` · `RF-CA-13`

Suggested order: `RF-CA-3` (nothing can be gated until a fresh clone builds) → `RF-CA-4` and `RF-CA-7` in parallel → `RF-CA-1` and `RF-CA-13` → freeze → `RF-CA-5` → `RF-CA-2` → `RF-CA-6`. The gate takes about two hours from cold; leave room to run it twice.

Optional for the tag, only if green before the freeze: `RF-CA-10`, and `RF-CA-11` through its read path. Otherwise they take their fallback wording.

**Gate:** `v0.1.0-rc.1` is a signed tag on public `main`; a fresh clone builds and passes the gate offline; the gate record, the numbers sheet and the claims ledger all name the tag's commit; no document in circulation states a capability above its ledger status or a figure that differs from the sheet.

**After the tag,** the capability track is delivered in the milestones the claims ledger names: `RF-CA-10` and `RF-CA-11` first (each wires an existing library into an existing service), then `RF-CA-8`, then `RF-CA-9` (which needs the replica-model decision), then `RF-CA-12` (which needs a SAD change first).

### Phase R1 — Make it run (months 2–5)

`RF-RT-1..7` · `RF-PER-1..4` · `RF-DEP-1` · `RF-OBS-1` · `RF-OBS-3` · `RF-OBS-5` · `RF-SC-4` · `RF-SC-7` · `RF-SC-8` · `RF-DOC-4`

**Gate:** every service runs as a configured, observable, gracefully-terminating process; state and the audit chain survive restart and `mg-audit verify` proves it; a single-node enclosure starts from gated container images.

### Phase R2 — Make it reachable (months 5–9)

`RF-API-1..3` · `RF-API-7` · `RF-IAM-1..3` · `RF-IAM-6` · `RF-UI-1` · `RF-UI-2` · `RF-UI-8` · `RF-UI-10` · `RF-UID-1` · `RF-UID-3` · `RF-TST-2` · `RF-TST-3` · `RF-DEP-2` · `RF-DEP-3` · `RF-RES-1`

**Gate:** a human authenticates with a security key, works in the Midgard console against real services with server-owned policy context, and every action is audited; the 72 acceptance criteria pass against the multi-process system (`AC-RF-TST-3b`); backups verify by test restore.

### Phase R3 — Make it capable (months 9–15)

`RF-MI-1..7` · `RF-MS-1..3` · `RF-MS-5` · `RF-F-1..6` · `RF-B-1..4` · `RF-B-6` · `RF-H-1..4` · `RF-AD-1..6` · `RF-UID-2` · `RF-UID-4..6` · `RF-PER-5..8` · `RF-OBS-2` · `RF-OBS-4`

**Gate:** each of the six consoles operates its real subsystem; a signed model answers a grounded question on a GPU node offline; a pipeline projects 10⁸ rows to the fabric; a ring rollout reaches a real fleet and rolls back; live feeds fuse into one picture.

### Phase R4 — Make it usable and operable (months 12–20, overlapping R3)

`RF-UI-3..7` · `RF-UI-9` · `RF-UX-1..8` · `RF-UX-10..11` · `RF-DEP-4..7` · `RF-RES-2..5` · `RF-AD-7` · `RF-B-5` · `RF-H-5` · `RF-H-6` · `RF-F-7` · `RF-DOC-2` · `RF-DOC-3` · `RF-DOC-6` · `RF-TST-5..8`

**Gate:** every journey in Workstream O completes keyboard-only and with a screen reader; install, upgrade, backup and DR are one command each and have been exercised; the air-gapped journey runs under `--network none`.

### Phase R5 — Make it shippable to clients (months 15–24, overlapping)

`RF-CL-1..9` · `RF-UID-7` · `RF-UX-9` · `RF-SDK-1..5` · `RF-UX-12` · `RF-API-4..6` · `RF-MS-4` · `RF-MI-8`

**Gate:** six shells build, ship through their channels, and complete the client journey with correct key custody; four SDKs are generated, documented and exercised.

### Phase R6 — Make it assurable (continuous from R1; completes months 20–28)

`RF-SEC-1..8` · `RF-CMP-1..6` · `RF-SCL-1..5`

**Gate:** every threat has a passing attack test; a full-scope penetration test and AI red team are remediated and retested; control mappings generate evidence from CI; the tenancy decision is implemented and its isolation suite is green.

> **Note on Workstream W.** `RF-SCL-1` (tenancy) is listed in R6 but its **ADR must be written in R1**, before `RF-PER-1` sets the storage keyspace. Deciding tenancy late is the most expensive mistake available in this plan.

---

# Appendix 3 — Acceptance-criteria conventions

New criteria are numbered `AC-RF-<AREA>-<n><letter>`, e.g. `AC-RF-PER-2b`. They **extend** the 72 in [BUILD_PLAYBOOK Appendix B](../../../BUILD_PLAYBOOK.md#appendix-b--consolidated-acceptance-criteria-matrix-72) and never renumber or supersede them.

Where an item re-exercises an existing criterion in a new context, this document says so explicitly rather than inventing a duplicate — for example `AC-RF-UX-6a` re-runs `AC-AD-5` (simulation equals commit) through the whole governance journey, and `AC-RF-TST-3b` re-runs all 72 against a deployed multi-process system rather than in-process libraries.

**Verification convention.** Every `AC-RF-*` must be verified the same way the original 72 are: by a named, executable test, mapped in a `deploy/acceptance/verify-*.sh` script, runnable under `docker run --network none`. An acceptance criterion with no executing test is not satisfied, however convincing the implementation looks.

**Extending the verification suite.** Add `deploy/acceptance/verify-remedial.sh` alongside the existing eight, structured identically (criterion → tests → PASS/FAIL with real output), and grow it per phase. At completion, `bash deploy/acceptance/run-airgapped.sh` should run the original 72 and the `AC-RF-*` set together.

---

# Appendix 4 — How to execute an item

1. **Read the evidence first.** Every item cites the files that establish the gap. Open them. If the evidence no longer holds — the code changed, or the audit was wrong — say so and revise the item rather than building against a stale premise.
2. **Check the dependencies.** An item built out of order usually has to be rebuilt. If a dependency is genuinely not needed, record why.
3. **Copy the prompt verbatim** into the coding agent, adjusting only concrete identifiers. The prompts are written to be self-contained given the rules in §0.
4. **Write the tests from the acceptance criteria before the implementation.** Each criterion is already in Given/When/Then form; it converts directly. A criterion that turns out to be untestable as written is a defect in this document — fix the criterion.
5. **Register the criteria** in `verify-remedial.sh` and confirm they run air-gapped.
6. **Update the traceability matrix** (`RF-SC-6`) so the new implementation and tests are linked to their requirement ids.
7. **Update the status labels** (`RF-DOC-1`) for anything the item moves between `spec`, `reference`, `operable` and `production`.
8. **Close the item here** by marking it done with the commit range, so this document stays an accurate register rather than becoming another artefact that drifts from the tree.

---

# Appendix 5 — What this audit did not cover

Stated so the gaps in the gap analysis are visible:

- **No runtime execution.** The audit read the tree; it did not build, run the test suite, or execute `run-airgapped.sh`. The claim that all 72 acceptance criteria pass is taken from the repository's own record and from reading the tests, not from observing a run. `RF-TST-3` exists partly to establish that independently.
- **No cryptographic review.** The PQC layer was assessed for structure, dependency provenance and API shape — not for protocol correctness. `RF-SEC-4` commissions the review that would.
- **No performance measurement.** Every scalability statement in this document is an inference from data structures and algorithms, not a benchmark. `RF-SCL-4` establishes the real numbers.
- **Design kits reviewed structurally only.** The 45 built screens match the 45 design kits one-for-one by count and name; individual visual fidelity was not compared kit-by-kit.
- **SAD content not re-derived.** The eight SADs were used as the requirement source and checked for traceability, not re-reviewed for whether the design they describe is the right one. `RF-DOC-4` reconciles them with what was built.
- **The working tree is mid-change.** 61 files are modified and uncommitted, including a Kryotech → Veldris rebrand across documentation and both token copies. Findings that touch those files were checked against the working tree, not `HEAD`.

**Revision 2 (Workstream Y) has its own limits:**

- **Nothing was built or run.** Counts were taken from the tracked files of public `main` @ `9ec8659`; capability status was established by reading source, manifests and tests. The offline build was not attempted, so RF-CA-3 states what the tree lacks, not an observed build failure. No test was executed; RF-CA-5 exists to establish the pass counts.
- **Likan was not examined.** It is a separate, proprietary tool and is not in this repository; nothing here describes its behaviour.
- **Workstreams A–X were re-audited by reading, item by item, in seven parallel reviews**, each classifying every acceptance criterion as met, open or not verifiable by reading. The reviews were not cross-checked against each other. The defects table in §1a was confirmed separately by opening each file; other findings rest on the single review that reported them, and each Status note gives the reviewer's confidence. Two items marked open on low confidence (RF-PER-6, RF-PER-7) rest on searches that found nothing, not on a file that proves absence.
- **The launch deck was checked through its build script only**, not the rendered `.pptx` files.

---

*Compiled against `dev` @ `71e2a58`, 2026-08-25; Workstream Y added against public `main` @ `9ec8659`, 2026-10-12. Companion to [BUILD_PLAYBOOK.md](../../../BUILD_PLAYBOOK.md) — the playbook builds the specification; this document completes the product. Source of truth for requirements remains [`docs/dev/markdown/`](../markdown/), chiefly [`SAD_05`](../markdown/SAD_05_Master_Prompt_and_Acceptance_Spec.md).*










