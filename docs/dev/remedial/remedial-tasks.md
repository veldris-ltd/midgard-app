# Midgard — Launch-Readiness Remedial Tasks

**Author:** Comprehensive codebase audit (5 parallel subsystem auditors + objective build/test/acceptance runs)
**Date:** 2026-07-12
**Scope:** Every workspace crate, service, product, client, SDK, contract, UI, and all build/CI/deploy/ops/docs infrastructure. `vendor/` and `node_modules/` excluded.
**Status of this document:** the single actionable backlog to take Midgard from *end-of-design + core-logic* to *production launch candidate*.

---

## 1. Executive summary

**Verdict: NOT launch-ready. The platform is at the end of its design + core-logic phase.**

The pattern is identical in every subsystem: **real, fail-closed cryptographic and domain logic wrapped around seams whose production implementations — durable persistence, network transport, real compute, real inference, orchestration, deployment, and observability — have not been built.** The SADs are explicitly "forward-looking design specifications for a greenfield system," and the code honestly labels these seams "Phase-1 seam" / "later phase" throughout.

**What is genuinely green today** (verified this audit, all under `docker --network none`):

| Suite | Result |
|---|---|
| verify-core (AC-M-1..9) | ✅ MET |
| verify-mimir (AC-MS-1..9) | ✅ MET |
| verify-forge (AC-F-1..7) | ✅ MET |
| verify-bifrost (AC-B-1..6) | ✅ MET |
| verify-heimdall (AC-H-1..7) | ✅ MET |
| verify-admin (AC-AD-1..13) | ✅ MET |
| verify-clients (AC-C-1..13) | ✅ MET |

**What "green" means and does not mean.** All 72 acceptance criteria pass, and the offline air-gap harness is a genuine, rare strength. **But these certify component-level logic exercised through in-memory mocks.** No acceptance criterion exercises a deployed multi-node system, a real network hop between hosts, a real HSM, real GPU inference, or a durable store that survives a restart. Several operationally-phrased ACs (AC-M-7 "given a running cluster"; AC-B-2 "no downtime"; AC-C-11 accessibility) are asserted against constants or mocks. **Green ≠ production-ready.**

**Tally:** 58 tasks (`RT-01`..`RT-58`) — 24 BLOCKER · 15 HIGH · 10 MEDIUM · 9 LOW. Blockers cluster into six themes: (A) the supply-chain release gate is non-functional theatre; (B) nothing is durable; (C) there is no deployment/orchestration substrate; (D) there is no real inference; (E) there is no real transport/API surface; (F) identity, key custody, and admin enforcement are mocks.

---

## 2. How to read this document

Each task carries: an **ID** (`RT-NN`), **Severity**, **Area**, **Evidence** (`file:line`), the **Gap**, a copy-pasteable **Prompt** (drive an AI coding agent, in the BUILD_PLAYBOOK house style), and **Acceptance** (a Given/When/Then criterion — new `RC-NN` "release-candidate" ids that extend, not replace, the existing AC-* matrix).

**Severity:**
- **BLOCKER** — a production launch is unsafe or impossible until this closes.
- **HIGH** — required for a credible launch; a launch without it carries material risk.
- **MEDIUM** — should close before GA; not an absolute gate.
- **LOW** — hygiene / polish.

**Golden rule for every task:** *do not weaken or delete a passing test to make a real backend fit.* Keep the existing mock-backed tests as component tests; add integration/deployment-tier tests alongside.

---

## 3. BLOCKERS

### Theme A — The supply-chain release gate is non-functional

> This is the most urgent theme: the machinery whose entire job is to *gate* a release currently passes everything.

#### RT-01 — `mg-sign` signs nothing (no-op stub)
- **Severity:** BLOCKER · **Area:** tools / supply-chain
- **Evidence:** `tools/mg-sign/src/main.rs:7,13-20` ("Phase 0.b: no-op stub"); prints "would ML-DSA-sign" and returns `SUCCESS`.
- **Gap:** No KMS call, no ML-DSA signature, no `.sig`/`.att` emitted. NFR-PQ-3 (PQ-signed releases) is unmet; the real signer never landed even though `bifrost-artefact` proves the crypto is available.
- **Prompt:**
  ```
  Implement tools/mg-sign for real. It must: resolve the ML-DSA signing key from
  the runtime KMS/HSM identity (never from the repo); compute the artefact digest
  and its SBOM digest; produce a detached ML-DSA signature plus a DSSE/in-toto
  attestation binding {digest, sbom_digest, toolchain hash, source rev}; write
  <artefact>.sig and <artefact>.att. Fail closed (non-zero exit) if the KMS is
  unreachable or the algorithm is unavailable. Reuse crypto/pqc-agility ML-DSA
  (FIPS 204) — do not re-roll crypto. Add a test proving a signature is produced
  and verifies, and that KMS-unreachable exits non-zero.
  ```
- **Acceptance — RC-01:** Given an artefact, when `mg-sign` runs with a reachable signing identity, then a detached ML-DSA signature + DSSE attestation are emitted and verify against the release key; and when the signing identity is unreachable, `mg-sign` exits non-zero and writes nothing.

#### RT-02 — `mg-verify` fails **open** (advertises fail-closed)
- **Severity:** BLOCKER · **Area:** tools / supply-chain
- **Evidence:** `tools/mg-verify/src/main.rs:8,13-19` — prints STUB, returns `SUCCESS` for any input including one with no `.sig`/`.sbom`/`.att`.
- **Gap:** This binary is the gate for CI publish, Bifrost pre-deploy (AC-B-1), and model load (AC-MS-2). All of those currently pass everything.
- **Prompt:**
  ```
  Implement tools/mg-verify for real: verify the detached ML-DSA signature over the
  artefact digest, verify the SBOM digest matches, and verify the DSSE/in-toto
  provenance attestation, against a pinned release trust key. Exit non-zero on ANY
  missing or invalid component (fail closed). Add negative tests: absent signature,
  tampered payload, tampered SBOM, wrong key, absent attestation — each must exit
  non-zero. Reuse crypto/pqc-agility verify.
  ```
- **Acceptance — RC-02:** Given a correctly signed+attested artefact, `mg-verify` exits 0; given any tampered, unsigned, or attestation-missing artefact, it exits non-zero. A negative test suite proves each rejection path.

#### RT-03 — The release workflow cannot execute its first stage (path-vs-package bug) and its crypto stages call the stubs
- **Severity:** BLOCKER · **Area:** CI / release
- **Evidence:** `.github/workflows/release.yml:21-55`; `tools/mg-repro/src/main.rs:47` runs `cargo build -p tools/mg-sign` (a **path**, not a package name — `-p` requires `mg-sign`); `main.rs:63` reads `target/release/tools/mg-sign` (wrong path). Sign/verify stages call RT-01/RT-02 stubs.
- **Gap:** The "fail-closed" release gate has never produced a green, meaningful run.
- **Prompt:**
  ```
  Fix the release gate end to end. In mg-repro, accept a package NAME and use the
  correct target/release/<name>[EXE_SUFFIX] path. In release.yml, pass package
  names in the matrix. Once RT-01/RT-02 land, run repro→sbom→scan→sign→provenance→
  verify per artefact and prove the gate FAILS when handed an unsigned artefact
  (add a deliberately-unsigned matrix smoke that must fail the job).
  ```
- **Acceptance — RC-03:** Given the release workflow, when it runs on a tag, then every matrix stage executes (no path/package errors) and the job fails closed if any artefact is unsigned/tampered.

#### RT-04 — No `flake.lock`: the hermetic/offline/reproducible build is unpinned
- **Severity:** BLOCKER · **Area:** build / reproducibility
- **Evidence:** no `flake.lock` at root; `flake.nix:5-10` ("run fully offline once flake.lock is populated"); `flake.nix` declares `packages = { }` (no `nix build` derivations); `ci.yml:53-58` builds only via `nix develop`.
- **Gap:** Floating `nixpkgs`/`rust-overlay` inputs must resolve from the network on every clean runner — contradicting AC-M-6 (bit-reproducible) and AC-M-5/NFR-PRV-1 (air-gap). Reproducibility rests entirely on cargo, not Nix.
- **Prompt:**
  ```
  Commit a flake.lock pinning all flake inputs. Add real per-artefact `nix build`
  derivations (or, if the Docker path in docs/dev/BUILD_ENV.md is the authoritative
  one, switch CI to Docker and drop the Nix reproducibility claim). Prove a clean,
  network-isolated runner can build the workspace with no input resolution.
  ```
- **Acceptance — RC-04:** Given a clean runner with no network, when the build runs, then it resolves all toolchain/inputs from committed locks/vendored sources and succeeds; a byte-for-byte rebuild is identical (AC-M-6, for real).

#### RT-05 — CI reproducibility smoke check is a guaranteed no-op pass
- **Severity:** BLOCKER · **Area:** CI · **Evidence:** `ci.yml:57-58` runs `just repro workspace`; `mg-repro/src/main.rs:23-26` special-cases `"workspace"` → prints "placeholder", returns `SUCCESS`.
- **Gap:** The only always-on AC-M-6 check builds nothing twice.
- **Prompt:**
  ```
  Remove the `workspace` soft-pass in mg-repro, or make it fan out to the concrete
  package set and actually build-twice-compare each. Update ci.yml to run repro over
  a representative artefact (or all). The check must be able to FAIL on a
  non-reproducible artefact.
  ```
- **Acceptance — RC-05:** Given CI, the reproducibility step builds at least one real artefact twice and compares bytes; it fails if they differ.

#### RT-06 — Release CVE/licence scan, SLSA provenance, and bundle assembly are `echo TODO`
- **Severity:** HIGH→BLOCKER (for a security product) · **Area:** CI / release
- **Evidence:** `release.yml:40-43` (CVE+licence), `:49-51` (provenance), `:57-59` (bundle); `ci.yml:27-30` (provenance-tag lint stub).
- **Gap:** No vuln/licence gate over the SBOM (FR-MS-6), no SLSA provenance, no enforced provenance-tag discipline.
- **Prompt:**
  ```
  Implement release.yml stages 4/6 and the ci.yml guard: run osv-scanner + a licence
  allow-list over the CycloneDX SBOM (fail closed on critical CVE or non-OSI licence
  in the OSS core); emit in-toto/DSSE provenance (source rev, toolchain hash,
  hermetic=true); implement the "changed source cites [FR/NFR/DD]" lint as an
  enforced check, not an echo.
  ```
- **Acceptance — RC-06:** Given a release, when a critical CVE or disallowed licence is present in the SBOM, the job fails closed; a valid release emits verifiable in-toto provenance; a PR touching a source file with no provenance tag fails the guard.

#### RT-07 — Release matrix covers only the 4 `mg-*` tools, not the shippable services/products
- **Severity:** HIGH · **Area:** CI / release · **Evidence:** `release.yml:21-26`.
- **Gap:** None of gateway, iam, kms, fabric, governance, scheduler, mimir, forge, bifrost, heimdall, admin, clients flow through the gate.
- **Prompt:**
  ```
  Expand the release matrix to every buildable shippable artefact (all service and
  product binaries), each through repro→sbom→scan→sign→provenance→verify. Add a
  check asserting the matrix covers the workspace's [[bin]]/publishable set.
  ```
- **Acceptance — RC-07:** Given the release workflow, when it runs, every shippable artefact is built, SBOM'd, signed, and verified; a new binary with no matrix entry fails a coverage check.

#### RT-08 — No "a real backend is required" compile guard — a `test-backend` production build is possible
- **Severity:** BLOCKER (security) · **Area:** crypto · **Evidence:** `crypto/pqc-agility/src/backend/mod.rs:6-13` — `compile_error!` only blocks `test-backend`+`backend-fips` *together*; nothing blocks a non-test build with only `test-backend` (the INSECURE deterministic `SeedRng` backend).
- **Gap:** A production artefact could ship forgeable crypto.
- **Prompt:**
  ```
  Add a compile guard so a non-test build MUST select exactly one production backend:
  `#[cfg(all(not(test), not(feature="backend-fips"), not(feature="test-backend")))]
  compile_error!("a production backend feature is required")` plus a guard that
  test-backend is never enabled in a non-test build. Add a CI check that no non-test
  crate enables `test-backend`.
  ```
- **Acceptance — RC-08:** Given a non-test build with no `backend-fips`, compilation fails; given `test-backend` requested for a shippable artefact, CI rejects it.

### Theme B — Nothing is durable (platform-wide)

> Every stateful store is in-memory. A restart loses data, audit lineage, config history, and the governance record — disqualifying for a sovereign platform.

#### RT-09 — Durable, encrypted, append-only audit ledger (`FileLedger`)
- **Severity:** BLOCKER · **Area:** crypto/audit-writer, governance, admin
- **Evidence:** `crypto/audit-writer/src/ledger.rs:5-7` ("durable FileLedger … is the Phase-1 seam; InMemoryLedger serves now"); `services/governance/src/lib.rs:88` and all `admin/*` stores wire `InMemoryLedger`.
- **Gap:** AC-M-4 and SAD_00 §8.3/§11.10 require durable, immutable, PQ-signed lineage for *every* operation. The hash-chain + PQ signature (tamper-evidence) is real, but nothing persists — the trail evaporates on restart.
- **Prompt:**
  ```
  Implement the durable FileLedger behind the existing LedgerStore trait: append-only
  open (O_APPEND), per-entry fsync, restrictive file perms; seal entries at rest via
  KMS (AES-256-GCM under ML-KEM-1024); on boot, load and re-verify the full hash
  chain and fail closed on any break. Repoint Governance and every admin/* store at
  it. Add a restart test: append N entries, drop the process, reopen, and re-verify
  the chain + read every entry back.
  ```
- **Acceptance — RC-09:** Given a running ledger, when the process restarts, then every prior entry is recovered, the chain re-verifies, entries are encrypted at rest, and a tampered on-disk entry is detected on load.

#### RT-10 — Durable, at-rest-encrypted object store (Fabric)
- **Severity:** BLOCKER · **Area:** services/fabric · **Evidence:** `services/fabric/src/store.rs:1-6,29-38` (`InMemoryStore` over `BTreeMap`; distributed backend is trait-only); `lib.rs:39`.
- **Gap:** SAD_00 §8.3 requires a durable columnar+inverted-index store, KMS-sealed at rest. All fabric data is lost on restart; objects are not encrypted at rest.
- **Prompt:**
  ```
  Implement a durable ObjectStore adapter behind the existing trait: persistent
  storage with crash recovery, incremental type + inverted-property indexes rebuilt
  or journaled on boot, and every stored object/metadata sealed at rest via KMS.
  Keep InMemoryStore for tests. Add restart + at-rest-encryption tests.
  ```
- **Acceptance — RC-10:** Given committed objects, when the fabric restarts, then all objects, links, and indexes are recovered; on-disk bytes are ciphertext; queries return identical results pre/post restart.

#### RT-11 — Durable dataset/version store (Forge)
- **Severity:** BLOCKER · **Area:** forge/dataset-store · **Evidence:** `forge/dataset-store/src/store.rs:34-48` (`versions`/`branches` `BTreeMap`, empty in `new()`); sealed blobs never flushed.
- **Gap:** SAD_01 §13 ("snapshots are backups by construction") contradicted — restart loses every dataset, version, and branch ref.
- **Prompt:**
  ```
  Add a durable content-addressed backing store for forge-dataset-store: persist
  sealed version blobs and branch refs with atomic ref updates and crash recovery.
  Keep the crypto/content-addressing unchanged. Add a restart test that checks out a
  branch head after reopen and byte-compares.
  ```
- **Acceptance — RC-11:** Given a committed pipeline output, when the store restarts, then `checkout_branch` returns the identical dataset and lineage.

#### RT-12 — Durable artefact registry + fleet state (Bifrost)
- **Severity:** HIGH · **Area:** bifrost/artefact, bifrost/fleet · **Evidence:** `bifrost/artefact/src/registry.rs:14-17` (`BTreeMap`); `bifrost/fleet/src/lib.rs:65-134` (two `BTreeMap`s).
- **Prompt:**
  ```
  Persist the Bifrost artefact registry and fleet/installation state to durable,
  integrity-protected storage with recovery on boot. Verify-on-ingest logic stays.
  Add restart tests.
  ```
- **Acceptance — RC-12:** Given registered artefacts and fleet check-ins, when Bifrost restarts, both are recovered and re-verified.

#### RT-13 — Durable picture / feed / workflow store (Heimdall)
- **Severity:** BLOCKER · **Area:** heimdall/picture, workflow · **Evidence:** `heimdall/picture/src/picture.rs:14-19`; `heimdall/workflow/src/engine.rs:56`.
- **Prompt:**
  ```
  Implement a durable PQ-wrapped-at-rest picture store and a persistent workflow/feed
  audit store behind the existing types; recover the common picture and in-flight
  workflows on boot. Add restart tests.
  ```
- **Acceptance — RC-13:** Given a fused picture and in-flight workflows, when Heimdall restarts, the picture and workflow state are recovered intact and at-rest bytes are ciphertext.

#### RT-14 — Durable admin governance stores (config / privilege / identity / break-glass)
- **Severity:** BLOCKER · **Area:** admin/* · **Evidence:** `admin/config/src/store.rs:11-16`, `admin/privilege/src/service.rs:85-88`, `admin/identity/src/directory.rs:14-17`, `admin/breakglass/src/service.rs:10-13` — all in-memory.
- **Gap:** For a sovereign platform the audit ledger, break-glass record, and config history are the legal/forensic backbone; none survive a restart.
- **Prompt:**
  ```
  Back every admin/* store with the durable FileLedger/persistent store from RT-09,
  with boot-time chain re-verification. Add a restart test per store (config history,
  privilege grants, identity bindings, break-glass grants).
  ```
- **Acceptance — RC-14:** Given admin actions across all stores, when the panel restarts, every record is recovered and re-verified; nothing is lost.

### Theme C — No deployment / orchestration substrate

#### RT-15 — Real cluster rolling upgrade + auto-rollback (Scheduler; AC-M-7 for real)
- **Severity:** BLOCKER · **Area:** services/scheduler · **Evidence:** `services/scheduler/src/lib.rs:10-11` ("no Kubernetes dependency"); availability is an integer counter (`:89-178`); AC-M-7 test uses mock `HealthCheck`s over an in-memory integer — no pod, no traffic.
- **Gap:** AC-M-7's core clause ("no request fails during the rollout") is never exercised; SAD_00 §5/§10 specify Kubernetes.
- **Prompt:**
  ```
  Implement the rollout driver against a real orchestrator behind the HealthCheck
  seam (Kubernetes Deployment surge/maxUnavailable + readiness gates, or the mesh's
  real replica controller). Drive live readiness probes; compute min-available from
  observed live replicas. Add an integration test that sends real requests through
  the fleet during an upgrade and asserts zero failed requests + automatic rollback
  on an injected unhealthy version.
  ```
- **Acceptance — RC-15:** Given a running multi-replica deployment, when a rolling upgrade is applied, then zero in-flight requests fail and an unhealthy new version triggers automatic rollback — observed against real replicas, not a counter.

#### RT-16 — Bifrost rollout actually deploys, health-checks, and rolls back
- **Severity:** BLOCKER (false safety guarantee today) · **Area:** bifrost/rollout · **Evidence:** `bifrost/rollout/src/orchestrator.rs:70` `min_available = total` **hardcoded** → `zero_downtime()` (`:41-43`) is always true; rollback branch (`:83-91`) constructs a report but reverts nothing; `gate.rs` `HealthGate` has only test impls.
- **Prompt:**
  ```
  Implement real ring rollout: an on-host PQ-mTLS deploy agent that applies/reverts
  an artefact, a production HealthGate that probes live instances, and re-verification
  (bifrost-artefact::verify) before each ring promotion. Compute min_available from
  observed live counts. Wire real blue-green promotion/revert. Add an integration
  test proving a failing ring auto-reverts with measured availability.
  ```
- **Acceptance — RC-16:** Given a real fleet, when a ring's health gate fails, then the previously-deployed version is actually restored and measured availability never drops below the configured floor.

#### RT-17 — Real fleet agents (Bifrost)
- **Severity:** BLOCKER · **Area:** bifrost/fleet · **Evidence:** `bifrost/fleet/src/lib.rs:65-134` (pure in-memory model; "clock-free").
- **Prompt:**
  ```
  Implement on-host fleet agents that check in over PQ-mTLS and report installed
  versions/health; back the fleet model with real agent records and staleness
  detection. Add an integration test with ≥2 simulated hosts.
  ```
- **Acceptance — RC-17:** Given real host agents, when they check in, then fleet state reflects live installed versions and flags stale/absent nodes.

#### RT-18 — Deployable artifacts: per-service container images + Helm/IaC
- **Severity:** BLOCKER · **Area:** deploy · **Evidence:** only `deploy/acceptance/Dockerfile` (test toolchain, no service COPY/entrypoint); no k8s/Helm/Terraform/systemd anywhere; `deploy/README.md` promises `helm/` (absent).
- **Gap:** There is no way to deploy any Midgard service.
- **Prompt:**
  ```
  Produce a minimal, reproducible container image per shippable service (built from
  the signed artefact, non-root, read-only rootfs) and a Helm chart (or equivalent)
  for the core services + products, plus IaC for the cluster. Wire image build + sign
  into release.yml. Include readiness/liveness probes and resource requests/limits.
  ```
- **Acceptance — RC-18:** Given the deploy tooling, when applied to a cluster, then every service starts, passes readiness, and is reachable over PQ-mTLS; images are PQ-signed and verified before run.

#### RT-19 — On-disk signed air-gap bundle producer/consumer + `deploy/` layers
- **Severity:** BLOCKER · **Area:** bifrost/bundle, deploy · **Evidence:** `bifrost/bundle/src/bundle.rs:32-96` (`build_bundle` returns a struct; `import` takes `&OfflineBundle`, not bytes/path); `deploy/README.md` promises `air-gap-bundle/` (absent); `release.yml:59` TODO.
- **Gap:** The air-gap transit medium (FR-B-5; AC-B-3/AC-MS-3/AC-C-7 all depend on it) produces/consumes nothing physical.
- **Prompt:**
  ```
  Define an on-disk bundle container format; serialize build_bundle output to a signed
  file and make import read+verify it from disk fully offline. Build deploy/helm and
  deploy/air-gap-bundle tooling that produces a portable, signed offline release and
  installs it on a disconnected site. Wire into release.yml.
  ```
- **Acceptance — RC-19:** Given a produced bundle file transported to an air-gapped host, when imported with no network, then its signature and every contained artefact verify and deploy; a tampered file is rejected.

### Theme D — No real inference (Mimir)

#### RT-20 — Real model backend + weights + latency re-validation
- **Severity:** BLOCKER · **Area:** mimir/model-server · **Evidence:** `mimir/model-server/src/backends/local.rs:82-102` (echoes reversed prompt words); `backends/gpu_docker.rs:116-124` (always `Err(Unavailable)`; builds a `docker run` arg vector it never executes); `Cargo.toml` has no inference deps; no `*.safetensors`/`*.gguf` weights exist anywhere.
- **Gap:** GLM 5.2 / Nemotron / Gemma cannot be served at all. NFR-PERF-1 (<800ms first token) is "met" only by a hardcoded constant (`local.rs:22-27,119-124`).
- **Prompt:**
  ```
  Implement a real ModelBackend behind the existing trait: either spawn the vLLM/
  TensorRT-LLM sidecar the launch_spec already describes and stream tokens over the
  loopback port, or embed candle/llama.cpp for a signed-GGUF path. Materialise real
  verified weights (RT-21) into the engine on load. Replace the synthetic
  first_token_ms with measured latency. Re-validate NFR-PERF-1 on target GPU hardware.
  ```
- **Acceptance — RC-20:** Given a verified model on GPU hardware, when inference is requested, then real tokens are generated with measured first-token latency, and the <800ms SLO is validated (or the SLO revised with evidence).
- **Status (2026-09-14):** real serving is in place, but not yet on GPU hardware.
  - `SidecarBackend` streams OpenAI-compatible completions from llama.cpp (CPU) or vLLM (GPU) on loopback, measuring first-token latency.
  - Weights load only through a signed manifest, and are streamed to the signed digest before anything is served (`ModelLoader::verify_dir`).
  - Proven live on CPU with `just compose-granite-smoke`, IBM Granite 3.3 2B Instruct Q4_K_M on llama.cpp (arm64, 12 cores). A certified PQ-mTLS call was answered with 44 generated tokens, 325 ms to the first token. mimir-server was ready 23 s after start, including verifying the 1.5 GB of weights.
  - RC-20 stays open: the vLLM path has not run on a GPU, so the <800 ms SLO is not validated on target hardware.

#### RT-21 — Real upstream fetch + real provenance verification (Staging)
- **Severity:** BLOCKER (security) · **Area:** mimir/staging · **Evidence:** `mimir/staging/src/provenance.rs:133-158` marks `Verified` merely because a vendor-signature/commit *string* is non-empty (no bytes checked); `fetch.rs:135-142` only `ReferenceFetcher` (in-memory fixtures) — no real upstream fetch.
- **Gap:** A staged bundle is recorded as having a "verified" vendor signature that was never actually checked — a false assurance presented to the human licence gate.
- **Prompt:**
  ```
  Implement the real network Fetcher (HuggingFace/NGC/CDN) that carries detached
  signature bytes, and make the vendor-signature and hf-commit checks perform actual
  cryptographic/commit verification. Until a check is real, it MUST report
  Unverifiable, never Verified. Keep the SHA-256 weights check. Add tests with a
  valid and a tampered upstream signature.
  ```
- **Acceptance — RC-21:** Given an upstream model, when staged, then vendor-signature/commit verification is cryptographic and a bad signature yields Unverifiable/Mismatch (never Verified); no unchecked assurance reaches the licence gate.
- **Status (2026-09-14):** found unfinished. `provenance.rs` still reported a present vendor-signature or commit string as `Verified`, and that is now corrected:
  - A vendor signature is recorded as present and unchecked (`Unverifiable`).
  - The commit check is `Verified` only when every fetched file matches an operator-supplied listing of the pinned revision (`DeclaredProvenance::pinned_revision_files`). A changed file is a `Mismatch` (`mimir/staging/tests/rt_21.rs`).
  - Still missing: no verifier for any vendor signature format, and no network fetcher. RC-21 holds only because no check is ever claimed that did not happen.

#### RT-22 — Real self-defending guardrail classifier + sanitiser
- **Severity:** HIGH · **Area:** mimir/agent-runtime · **Evidence:** `agent-runtime/src/defend.rs:86-128` detection is a fixed substring blocklist; `:254-256` `Sanitise` is downgraded to `Allow` (advisory no-op).
- **Gap:** Any injection phrasing outside the hardcoded lists passes (trivially bypassed by paraphrase); sanitisation does nothing.
- **Prompt:**
  ```
  Back the InputGuardrail seam with the real A.3 classifier model (served via
  model-server) so detection generalises beyond a keyword list; implement actual
  sanitisation rewriting of the assembled text before the Sanitise verdict is
  trusted. Keep the deterministic blocklist as a fast pre-filter. Re-run EV-A3-*
  against the real classifier including paraphrased attacks.
  ```
- **Acceptance — RC-22:** Given paraphrased prompt-injection not in the blocklist, when received, then it is detected and blocked (or sanitised with real rewriting) before any tool executes.

#### RT-23 — Re-run all Evals + guardrails against each real model family
- **Severity:** HIGH · **Area:** mimir/* · **Evidence:** `agent-runtime/src/evals.rs:376-390` — `per_family` runs the same family-invariant reference closure three times, relabelled GLM/Nemotron/Gemma; every eval scores a deterministic stand-in.
- **Prompt:**
  ```
  Once RT-20 lands, wire the EV-A* suites (A1..A9, A.C*, A.AD*) to run against each
  real served model family, not a stand-in. Make per_family actually exercise three
  distinct models. Record pass/fail per (eval, family). This is the true AI
  acceptance gate.
  ```
- **Acceptance — RC-23:** Given the three real model families, when the full Eval suite runs, then each eval is scored against each real model and the results are reported per (eval, family).

### Theme E — No real transport / API surface

#### RT-24 — Secure the HTTP/JSON gateway edge (the consoles' real backend)
- **Severity:** BLOCKER (if this fronts the launch API) · **Area:** services/http-gateway · **Evidence:** `services/http-gateway/src/lib.rs` header + `data.rs:8-12` — serves **seeded** state; no auth/authz on any route (`/query`, `/actions`, `/keys`, `/identities`); permissive CORS; action "auth" is a plaintext string compared in the mock; does not forward to the enclosure.
- **Prompt:**
  ```
  Make http-gateway production-real: verify the PQ-signed IdP token on every request
  (via iam), authorize via the policy PDP, and forward to the real fabric/governance/
  kms crates instead of the seeded State. Lock CORS to configured origins. Front it
  with PQ-mTLS termination (or TLS 1.3 hybrid + HSTS) that forwards to the enclosure.
  Until then, bind loopback-only. Add tests: unauthenticated request rejected,
  above-clearance query denied+audited.
  ```
- **Acceptance — RC-24:** Given the edge, when an unauthenticated or above-clearance request arrives, it is rejected and audited; authenticated requests are served from live enclosure state over an encrypted, origin-locked channel.

#### RT-25 — Gateway session management, resumption, routing, rate limiting
- **Severity:** HIGH · **Area:** services/gateway · **Evidence:** `services/gateway/src/lib.rs:83-110` handles exactly one request per connection (no session, no router, one `EchoHandler`); no `rate limit`/`throttle`/`quota` anywhere in `services/`.
- **Prompt:**
  ```
  Add to the gateway: a session lifecycle with PQ-mTLS session resumption over the
  record layer, connection reuse/multiplexing, a real request router mapping methods
  to fabric/governance/KMS handlers, and per-identity/per-connection rate limiting +
  resource quotas. Add load + resumption tests.
  ```
- **Acceptance — RC-25:** Given repeated requests on one session, then the handshake is amortised via resumption, requests route to the correct handler, and an abusive client is rate-limited.

#### RT-26 — Real feed ingestion transport + cross-node picture-sync (Heimdall)
- **Severity:** BLOCKER · **Area:** heimdall/feed-gateway, picture-sync · **Evidence:** `feed-gateway/src/authenticator.rs:81` authenticates an already-built observation (no listener); `picture-sync/src/link.rs:22-52` only `LoopbackLink` (`Rc<RefCell<..>>`, not even `Arc`).
- **Prompt:**
  ```
  Implement a PQ-mTLS feed listener + feed SDK that ingests signed observations from
  real sources, and a DeltaLink that replicates the common picture across nodes over
  a real pq-mtls Session with peer/key management and a replication loop. Keep
  signature verify + merge. Add a 2-node replication integration test.
  ```
- **Acceptance — RC-26:** Given two Heimdall nodes over a real link, when a feed delivers a signed observation to one, then the fused picture converges on both and unsigned feeds are rejected.

#### RT-27 — Real Forge connectors + compute backend + IR breadth
- **Severity:** BLOCKER · **Area:** forge/connector-sdk, forge/engine · **Evidence:** `forge/connector-sdk/src/connectors/mod.rs:6-10` (only `file`/`inmem`; `file.rs:23-37` takes an in-memory string, never opens a file; `SandboxPolicy::check_egress` never called); `forge/engine/src/reference.rs:25-88` (single-threaded row interpreter); IR has no join/group-by/aggregate/sort/window (`plan.rs:83-114`).
- **Prompt:**
  ```
  Implement real sandboxed connectors (Database/ObjectStore/Api/File/Stream) that
  perform genuine I/O and enforce SandboxPolicy egress checks. Implement a real
  ComputeBackend (Polars/DuckDB in-process first, then distributed with partitioned
  columnar storage, network shuffle, worker orchestration, spill-to-disk) and expand
  the transform IR to joins/aggregations/sort/window. Add integration tests against a
  real source and a multi-partition dataset.
  ```
- **Acceptance — RC-27:** Given a real external source, when a pipeline with a join+aggregate runs, then it ingests, executes on the real engine, and projects to the fabric — and an egress-violating connector is blocked.

### Theme F — Identity, key custody, admin enforcement

#### RT-28 — Hardware (HSM/TPM) KMS trust root + operational key rotation
- **Severity:** HIGH · **Area:** services/kms · **Evidence:** `services/kms/src/lib.rs:37-53` — only `SoftwareTrustRoot` (root ML-KEM secret in process memory); HSM/TPM is an unimplemented trait seam.
- **Prompt:**
  ```
  Provide at least one hardware TrustRoot (TPM 2.0 / PKCS#11 HSM) where the root
  secret never leaves the device, plus root attestation and an operated key-rotation
  procedure. Keep SoftwareTrustRoot for dev. Document residual risk if launching
  software-only.
  ```
- **Acceptance — RC-28:** Given a hardware root, when data is sealed/unsealed, then the root secret never enters process memory and rotation re-wraps new data without touching old blobs.

#### RT-29 — Real IAM: principal authentication, MFA, OIDC surface, durable directory, federation
- **Severity:** HIGH · **Area:** services/iam · **Evidence:** `services/iam/src/idp.rs:122-145` issues a token with no credential verification; `store.rs:1-3` in-memory directory; no OIDC endpoints/JWKS, no Argon2id, no WebAuthn/TOTP, no federation.
- **Prompt:**
  ```
  Implement real principal authentication before token issuance: Argon2id password
  verification and MFA (WebAuthn/passkeys + TOTP); a durable identity directory; the
  OIDC surface (/authorize, /token, JWKS) or an explicit SAD rescope; optional
  enterprise-IdP federation. Keep the ML-DSA-signed offline token format.
  ```
- **Acceptance — RC-29:** Given a user, when they authenticate, then credentials + MFA are verified before a PQ-signed token is minted; the directory survives restart; federation works when configured.

#### RT-30 — Admin real WebAuthn verification (not an enum-set mock)
- **Severity:** BLOCKER · **Area:** admin/identity · **Evidence:** `admin/identity/src/session.rs:16-43` — MFA = "≥2 enum variants in a set"; WebAuthn is a bare enum variant with no challenge/signature/credential check.
- **Prompt:**
  ```
  Replace the enum-set MFA model with real WebAuthn assertion verification (reuse the
  client keystore auth gate pattern) and real step-up. An AdminSession must not be
  constructable as "authenticated" without a verified assertion. Add negative tests
  (forged/absent assertion denied).
  ```
- **Acceptance — RC-30:** Given an admin, when they authenticate, then a real WebAuthn assertion is verified; no session is authenticated without one.

#### RT-31 — Admin guarded-action orchestrator (dual-control wired to commits)
- **Severity:** BLOCKER · **Area:** admin/* · **Evidence:** `admin/privilege/src/control.rs:14-15` ("enforced by the action orchestrator in a later phase"); `admin/config/src/store.rs:55-83` `commit` requires only a non-empty author — no dual-control, no audit link.
- **Gap:** A high-risk config change can commit with one author, no approval, no audit.
- **Prompt:**
  ```
  Build the guarded-action orchestrator crate composing identity + privilege +
  sim + config + audit into ONE enforced transaction: authenticate → simulate →
  require dual-control approval (distinct approver for risk≥threshold) via a single-
  use time-boxed token → commit → audit. Route every mutating admin action through it.
  Add tests: single-author high-risk commit denied; dual-approved commit audited.
  ```
- **Acceptance — RC-31:** Given a high-risk config change, when committed, then it is refused unless simulated, dual-approved with a distinct approver, and audited as one linked transaction.

#### RT-32 — Break-glass real alerts + enforced grants
- **Severity:** BLOCKER · **Area:** admin/breakglass · **Evidence:** `admin/breakglass/src/alert.rs:33-60` only `RecordingAlerter` (pushes to a `Vec`); `service.rs:72-74` `is_active` has no caller outside tests — a grant issues no token and gates nothing.
- **Prompt:**
  ```
  Bind a real Alerter to the in-enclosure alert bus with delivery durability so a
  break-glass grant raises a real-time security-officer alert. Issue a scoped,
  time-boxed capability token on grant and have the PDP/subsystems honor it, so the
  grant actually elevates access and auto-expires. Add tests: grant alerts + elevates;
  expiry revokes.
  ```
- **Acceptance — RC-32:** Given a break-glass grant, when issued, then a real-time alert is delivered and a time-boxed capability actually elevates the grantee's access, expiring automatically and audited throughout.

---

## 4. HIGH

#### RT-33 — Observability: logging, metrics, tracing, health/readiness
- **Severity:** HIGH · **Evidence:** no `tracing`/`opentelemetry`/`prometheus`/`metrics`/`log` in any manifest; only `GET /api/v1/health` returns a static `{"status":"ok"}`.
- **Prompt:** `Introduce an in-enclosure telemetry stack behind a seam respecting NFR-PRV-1 (no egress): structured logging, metrics (/metrics), distributed tracing, and per-service liveness/readiness probes. Instrument the availability and first-token SLOs.`
- **Acceptance — RC-33:** Given a running service, then it exposes real liveness/readiness + metrics, and SLO breaches are observable; no telemetry egresses the enclosure.

#### RT-34 — Backup / restore / DR + runbooks + on-call
- **Severity:** HIGH · **Evidence:** SAD §13 bullets only; no DR plan, backup procedure, or incident/on-call doc exists.
- **Prompt:** `Author operational runbooks, a DR plan with a tested restore drill, backup/snapshot procedures for every durable store (RT-09..14), and an incident-response/on-call escalation policy.`
- **Acceptance — RC-34:** Given a simulated node loss, when the DR runbook is followed, then a verified restore returns the platform to service within the stated RTO/RPO.

#### RT-35 — CA / PKI / certificate lifecycle operations
- **Severity:** HIGH · **Evidence:** `services/kms` "rotation" is crypto-agility re-wrap, not operated cert issuance/renewal/revocation; `release.yml:16-17` assumes an unprovisioned `[self-hosted, kms]` signing runner.
- **Prompt:** `Define and document CA/PKI bootstrap, workload cert issuance/renewal/revocation for the PQ-mTLS mesh, HSM key ceremony, rotation cadence, and (optionally) threshold signing. Provision the signing runner.`
- **Acceptance — RC-35:** Given the mesh, when a workload cert nears expiry, then it is renewed without downtime and revocation is honored on the next handshake.

#### RT-36 — Complete the contract surface (`contracts/`)
- **Severity:** HIGH · **Evidence:** `contracts/README.md` claims protos for "every inter-service call and every generated SDK"; only `common.proto` + `object_fabric.proto` exist (ObjectFabric.Query/ApplyAction). No protos for Mimir/Forge/Bifrost/Heimdall/Admin/IAM/KMS/Governance/Scheduler.
- **Note:** reconcile with the built reality — services use the lean `pq-mtls` binary codec, not gRPC. Decide whether contracts are the SDK-edge source of truth (then complete them) or documentation only (then rescope the README).
- **Prompt:** `Either author the proto contracts for every product/service API the SDKs expose and wire buf generate into the build, or rescope contracts/README to what protos actually govern. Ensure DD-6 (typed contract source of truth) is either true or not claimed.`
- **Acceptance — RC-36:** Given the SDK surface, then every RPC a client can call has a governing contract, and `buf breaking` gates changes to it.

#### RT-37 — Product consoles talk to a real backend by default
- **Severity:** HIGH · **Evidence:** `products/*/ui/src/main.tsx` default to `new MockGateway()`; real transport is `VITE_GATEWAY=http` → the seeded, unauth `http-gateway` (RT-24).
- **Prompt:** `Once RT-24 lands, make the product consoles default to the authenticated HTTP/JSON edge against live enclosure state, with a login flow that obtains a PQ-signed token; keep MockGateway for offline demos only.`
- **Acceptance — RC-37:** Given a console, when launched in production mode, then it authenticates and renders live enclosure data, not seeded fixtures.

#### RT-38 — Repo governance & legal hygiene
- **Severity:** HIGH · **Evidence:** no `LICENSE` file despite `Cargo.toml` `license = "Apache-2.0"` and "open-source core" claims; no `SECURITY.md`, `CONTRIBUTING.md`, `CODEOWNERS`, `CHANGELOG.md`, threat-model doc, or compliance/data-residency matrix.
- **Prompt:** `Add LICENSE (Apache-2.0 text) + NOTICE; SECURITY.md (PQ-aware vuln-disclosure policy); CONTRIBUTING; CODEOWNERS; CHANGELOG; a threat-model document; and a compliance/data-residency matrix mapping the NIST/ASVS/ISO claims to controls.`
- **Acceptance — RC-38:** Given the repo, then the OSS core is lawfully distributable (LICENSE present), a disclosure path exists, and the compliance claims trace to documented controls.

#### RT-39 — Key-material zeroization hardening
- **Severity:** MEDIUM→HIGH · **Evidence:** `crypto/pqc-agility/src/secret.rs:1-24` — home-rolled volatile-write, honestly documented as not covering registers/stack spills/`Vec` realloc; promised `mlock`/guard-page hardening was tied to the cancelled liboqs backend.
- **Prompt:** `Adopt the vendored zeroize crate for secret types and add mlock/guard-page memory locking for long-lived key material, or formally accept and document the residual risk. Delete the orphaned liboqs reference.`
- **Acceptance — RC-39:** Given a dropped secret, then its memory is zeroized via a vetted path and long-lived keys are locked against swap.

#### RT-40 — Deployment-level acceptance tier
- **Severity:** HIGH · **Evidence:** all 72 ACs are unit/offline; none exercises a deployed multi-node system, real HSM, or GPU.
- **Prompt:** `Add a deployment-tier acceptance suite (real cluster, real PQ-mTLS between hosts, real HSM signing, real GPU inference smoke) distinct from the current offline tier; run it in a staging environment before any release is promoted.`
- **Acceptance — RC-40:** Given a staging cluster, then a deployment-tier suite validates clustering, cross-host PQ-mTLS, HSM signing, and live inference — separate from `just test`.

#### RT-41 — SBOM component integrity data
- **Severity:** MEDIUM→HIGH · **Evidence:** `tools/mg-sbom` `render_cyclonedx` emits name/version/purl only — no component hashes, no self-hash.
- **Prompt:** `Extend mg-sbom to include per-component hashes and a self-hash of the described artefact in the CycloneDX output, so mg-verify can bind the SBOM to concrete content.`
- **Acceptance — RC-41:** Given a generated SBOM, then every component carries an integrity hash and the SBOM binds to the artefact digest.

---

## 5. MEDIUM / LOW

#### RT-42 (MEDIUM) — Fabric/objects at-rest encryption wired through KMS
Covered structurally by RT-10; track the "objects sealed at rest" clause of SAD_00 §8.3 explicitly. **RC-42:** stored objects are ciphertext at rest under a KMS-wrapped key.

#### RT-43 (MEDIUM) — Scheduler resource quotas & priority
- **Evidence:** scheduler does only rollout; SAD_00 §6.2.7 quotas/priority unimplemented. **Prompt:** `Implement resource quotas + priority scheduling.` **RC-43:** an over-quota workload is throttled/queued by priority.

#### RT-44 (MEDIUM) — Heimdall workflow actuator
- **Evidence:** `heimdall/workflow/src/engine.rs:133-164` "execute" records a caller string, no actuator. **Prompt:** `Wire workflow execution to real, human-gated, audited actuators behind a seam.` **RC-44:** an approved workflow action invokes a real actuator and is audited.

#### RT-45 (MEDIUM) — Standalone Mimir bridges (Forge/Bifrost/Heimdall/Admin AI-assist)
- **Evidence:** all product AI-assist crates depend only on a trait + Mock/Unavailable; the live Mimir bridge is unbuilt (advisory, fails closed — good). **Prompt:** `Implement the production OptionProvider/Suggester/Advisor/Copilot bridges to Mimir over PQ-mTLS behind the existing seams; degraded paths already work.` **RC-45:** with Mimir present, each product's AI-assist returns live advisory output; without it, the degraded path is unchanged.

#### RT-46 (MEDIUM) — Reconcile `go.work` / `pyproject.toml` member-less scaffolds
- **Evidence:** `go.work` has no members (comment: controllers "added as each phase lands"); `pyproject.toml` `members=["mimir/*"]` but Mimir is Rust. **Prompt:** `Either implement the Go controllers / Python members these workspaces anticipate, or remove the empty workspaces to avoid implying capability that doesn't exist.` **RC-46:** every declared workspace has real members or is removed.

#### RT-47 (MEDIUM) — BUILD_ENV ↔ CI (Nix vs Docker) reconciliation
- **Evidence:** `docs/dev/BUILD_ENV.md:42-45` says the Nix devshell is unusable ("use Docker"); `ci.yml` runs only Nix. **Prompt:** `Make the documented and automated build paths agree (fix the Nix overlay + commit flake.lock per RT-04, or switch CI to Docker); state which is authoritative.` **RC-47:** the build path in docs matches CI and both are green on a clean runner.

#### RT-48 (MEDIUM) — CI provenance-tag lint enforcement
Covered by RT-06; tracked separately as it is the traceability guarantee the playbook relies on. **RC-48:** a PR touching source without an `[FR/NFR/DD]` tag fails CI.

#### RT-49 (MEDIUM) — AC-C-11 on-device shell audits
- **Evidence:** shells can't compile in CI; AC-C-11 is a static conformance gate only. **Prompt:** `Stand up per-platform shell CI (XCUITest/Espresso/Playwright/Tauri harness) on real runners and record HIG/accessibility audit results per release.` **RC-49:** each shell's accessibility audit runs on a real device/runner and is recorded per release.

#### RT-50 (LOW) — Fallible constructors instead of `expect()` on keygen/sign paths
- **Evidence:** `services/iam/src/idp.rs:56,58`, `services/governance/src/lib.rs:98,100`, `admin/service-identity/src/identities.rs:38,47` (keygen `expect` at construction); `bifrost/artefact/src/signed.rs:49`, `bifrost/bundle/src/bundle.rs:49`, `mimir/.../artifact.rs:204` (`.expect("ml-dsa signing")`). **Prompt:** `Convert construction-time keygen and signing expects to fallible constructors/functions returning typed errors, so a crypto failure surfaces as an error rather than a panic.` **RC-50:** a simulated keygen/sign failure returns an error, not a panic.

#### RT-51 (LOW) — `OsRng` Result variant
- **Evidence:** `crypto/pqc-agility/src/rng.rs:64` panics on entropy failure (deliberate fail-closed). **Prompt:** `Offer a Result-returning RNG constructor so a service can degrade gracefully instead of crashing on entropy failure.` **RC-51:** entropy failure is recoverable by the caller.

#### RT-52 (LOW) — `http-gateway` request-path lock poisoning
- **Evidence:** `services/http-gateway/src/data.rs:79,193` `lock().unwrap()` on the request path (moot once RT-24 replaces the mock). **RC-52:** a poisoned lock does not crash the connection handler.

#### RT-53 (LOW) — Stale "Phase-1 seam" doc comments
- **Evidence:** `crypto/pq-mtls/src/lib.rs:13` calls the byte framing / record layer a "Phase-1 seam," but `wire.rs`/`record.rs` exist and are exported. **RC-53:** doc comments match the implemented state.

#### RT-54 (LOW) — Dead `unreachable!()` in Forge reference engine
- **Evidence:** `forge/engine/src/reference.rs:153`. **RC-54:** provably-unreachable code is removed or documented.

#### RT-55 (LOW) — Forge connector `SandboxPolicy` unused
Folded into RT-27 (connectors never call `check_egress`); tracked so the sandbox is exercised. **RC-55:** every connector enforces egress policy.

#### RT-56 (LOW) — `.exe` suffix in `mg-repro` binary path
- **Evidence:** `tools/mg-repro/src/main.rs:63` omits `EXE_SUFFIX`. **RC-56:** repro works cross-platform via `env::consts::EXE_SUFFIX`.

#### RT-57 (LOW) — Client `remote` inference seam not yet bridged to enclosure
- **Evidence:** `midgard-client-inference` `RemoteInference` is a trait + mock; the live bridge to `clients/enclosure` is unbuilt (by design for Phase 8). **RC-57:** with connectivity, Tier-2 requests delegate over a real PQ-mTLS session.

#### RT-58 (LOW) — Client shells are scaffolds (no compiled UI)
- **Evidence:** `clients/shells/*` are manifests + accessibility declarations; no Swift/Kotlin/GTK/Tauri/WASM app is built. **Prompt:** `Build the six shell apps against the UniFFI/wasm-bindgen bindings of midgard-client-core.` **RC-58:** each shell compiles and links the shared core on its platform.

---

## 6. SAD-declared residual risks (from the design docs themselves)

Extracted verbatim-in-substance from each SAD's *Known design gaps* (§11.x), *Risks & Technical Debt* (§15), and *Roadmap* (§16). These are the authors' own admissions and should be tracked as accepted-risk or converted to tasks above.

> **Closed out in [`docs/compliance/RESIDUAL_RISK_REGISTER.md`](../../compliance/RESIDUAL_RISK_REGISTER.md):** every item below is resolved to MITIGATED (with the implemented control), ACCEPTED (with rationale + compensating control), or OPEN. RT-01…58 is complete, and the one unbuilt §6 item — SAD_07 config-drift / GitOps reconciliation — is now built (**RT-59**, `admin/config/src/reconcile.rs`). The only remaining OPEN items are the two external activities: FIPS 140-3 certification (lab) and deployment-tier assurance of the HSM/GPU/cluster controls (gated by RT-40).

- **SAD_00 (Core):** PQ hardware acceleration not ubiquitous (Med); GLM 5.2 weight provenance (Med); air-gap update workflow (Low); PQ performance overhead (Med); ontology sprawl (Med); single model plane as shared dependency (Med); open-core boundary drift (Low). Roadmap explicitly requires **FIPS KAT certification before GA**.
- **SAD_01 (Forge):** third-party connector trust (Med); large-dataset PQ overhead (Low); connector maintenance burden (Med); pipeline drift (Med).
- **SAD_02 (Mimir):** novel jailbreaks evade static guards (Med) → RT-22; **autonomous action blast radius (High)**; **autonomy overreach (High)**; inference capacity (Med) → RT-20; feedback poisoning (Med).
- **SAD_03 (Bifrost):** **compromised signing key (High)** → RT-28/RT-35; canary blind spots (Med); **faulty release fan-out (High)** → RT-16.
- **SAD_04 (Heimdall):** **automation bias (High)**; **feed spoofing (High)** → RT-26; **over-trust in AI options (High)**; **feed integrity (High)**; high-rate feed load (Med).
- **SAD_06 (Clients):** rooted/jailbroken device weakens keystore (Med); extension store review (Med); on-device model tamper (Low); binding-layer complexity (Med); store policy divergence (Med); on-device model footprint (Med); keystore guarantee variance (Med).
- **SAD_07 (Admin):** approver collusion (Med); break-glass over-use (Med) → RT-32; copilot over-trust (Med); **privilege concentration (High)**; **self-governance bypass (High)** → RT-31; config drift vs subsystems (Med, "GitOps reconciliation" has no implementation).

---

## 7. Suggested remediation roadmap

Ordered so each phase unblocks the next. Each phase should end with the relevant new `RC-*` criteria passing.

- **R0 — Make the gate real (weeks).** RT-01..08 (release gate + reproducibility + backend guard). *Nothing should ship until the gate can reject a bad artefact.*
- **R1 — Persist everything (weeks).** RT-09..14 (durable, encrypted, recoverable stores). *A sovereign platform must survive a restart.*
- **R2 — Deploy & orchestrate (months).** RT-15..19, RT-18 images/Helm, air-gap bundles, real rollout/fleet/scheduler.
- **R3 — Real transport & identity (months).** RT-24..27 (secured edge, gateway, feeds, connectors) + RT-28..32 (HSM, IAM, admin enforcement).
- **R4 — Real inference (months, hardware-gated).** RT-20..23 (model backend, provenance, guardrail, per-family evals).
- **R5 — Operate & govern (parallel).** RT-33..41 (observability, DR, PKI ops, contracts, consoles, legal hygiene, deployment-tier acceptance).
- **R6 — Polish.** RT-42..58.

**Do not GA until:** R0–R4 close, RT-33/34/35/38/40 (observability, DR, PKI, legal, deployment-tier acceptance) close, and SAD_00's own bar — **FIPS KAT certification** — is met.

---

## 8. What is already production-grade (do not redo)

The audit found genuine, well-engineered strengths worth protecting:

- **PQ cryptography:** real FIPS 203/204/205 + X25519 (`crypto/pqc-agility/backend/fips.rs`); correct KEM-DEM envelope (ML-KEM-1024 + HKDF + AES-256-GCM), self-describing blobs for agility, fail-closed unseal.
- **PQ-mTLS handshake & record layer:** transcript-bound signatures, explicit downgrade detection, constant-time key confirmation, mandatory mutual auth, PDP authorization with no path to a `Session` that skipped a check; directional keys + monotonic nonce + fail-closed exhaustion + replay/reorder rejection. Runs over real TCP.
- **Audit chain:** real hash-chained, per-entry PQ-signed, tamper-evident lineage (only the *durable store* is missing — RT-09).
- **Policy PDP:** real Bell-LaPadula + purpose + role deny-by-default; used identically by the handshake, governance, and admin simulation.
- **Governed agent loop:** real plan→ground→assemble(8-layer)→guardrail→evals→reversibility→human-gate→audit→execute, fail-closed by default (only the model it governs is a stub — RT-20).
- **Domain logic across all products:** type systems, content-addressing, reproducible sealing, health-gating, projection markings, dual-control/separation-of-duties, policy simulation, service-identity certs, CRDT sync, tiered-inference decisioning, minimal-privilege broker, verify-before-install — all real and fail-closed.
- **Air-gap discipline:** `deploy/acceptance/run-airgapped.sh` runs the whole suite under `docker --network none`; 72 ACs each map to a real test. This is a rare and genuine strength.

**The core is real. The seams are not. This document is the list of seams.**
