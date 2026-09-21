# Mimir — Model-Supply Gap Resolution

*Design-change record: sovereign multi-model provenance for Mistral, Nemotron & Gemma*

**Author:** Veldris Ltd (company no. 17366869)  
**Version:** 1.0 · **Date:** 2026-07-02 · **Classification:** PUBLIC, published with the Midgard repository  
**Document type:** Forward-looking design-change record (greenfield). Provenance tags: `[DD-*]`, `[FR-*]/[NFR-*]`, `[STD:*]`.

---


## 1. Purpose & Change Summary

This is a design-change record (DCR) against the Mimir Solutions Architecture Document (SAD_02). It resolves the open design gap recorded in Mimir SAD §11.3 / §15 and Midgard core SAD §11.12 — *local model-weight provenance* — and simultaneously implements an approved capability expansion: users may run the most advanced locally-runnable variants of **three model families — Mistral, NVIDIA Nemotron, Google Gemma and IBM Granite** — inside the enclosure, selectable per workflow. `[DD-4]` `[DD-5]` `[DD-7]`

As with the SADs, this is a **forward-looking design specification**. Every decision carries a provenance tag (`[DD-n]`, `[FR-n]`, `[NFR-n]`, `[STD:...]`); it describes intended design, not verified implementation.

> **Resolution in one sentence.** The gap is closed by making model supply a *cryptographic custody-transfer problem solved with the machinery the suite already has*: models are fetched and verified in a network-connected staging enclave, re-signed under the Veldris post-quantum ML-DSA key, delivered across the air gap as signed Bifrost bundles, recorded in a new Model Registry, and verified-on-load by the inference runtime — which fails closed on any signature, licence, or hardware-eligibility mismatch. `[NFR-PQ-3]` `[FR-B-4]`


### 1.1 Change classification

| Attribute | Value |
| --- | --- |
| Change type | Architecture change — gap resolution + capability addition |
| Affected products | Mimir (primary); Forge, Bifrost, Heimdall (consumers, via unchanged contract) |
| Gap closed | Local model-weight provenance (was: medium severity, open) |
| New severity | Closed (residual items tracked in §9) |
| New subsystems | Model Registry & Provenance; Model Abstraction Layer |
| Contract impact | None — Mimir inference API is unchanged; consumers unaffected `[DD-6]` |


### 1.2 Why this was the priority

Mimir is the shared inference dependency for Forge, Bifrost, and Heimdall. An unresolved model-supply gap in Mimir is therefore a latent gap in three other products at once. Closing it — and doing so in a way that preserves the unchanged inference contract — de-risks the whole suite from a single point of change. `[DD-6]`


## 2. The Gap, Decomposed

The original finding bundled three distinct problems. A complete resolution must close all three; the multi-model decision expands the third.

| Sub-problem | Question it poses | Resolved by |
| --- | --- | --- |
| G1 — Acquisition | How do weights enter an air-gapped enclosure with no hosted dependency? | Two-stage air-gap supply chain via Bifrost bundles (§3) |
| G2 — Provenance & integrity | How does the enclosure prove a weight file is authentic, unmodified, and licence-clean before load? | Staging-side verification + Veldris re-signing + verify-on-load (§3, §4) |
| G3 — Lifecycle & agility | How are models versioned, rotated, and hot-swapped across families without breaking consumers? | Model Registry + Model Abstraction Layer (§4, §5) |

> **Why the multi-model decision changes G3 specifically.** Supporting one model is a provenance problem; supporting three families — each with different formats, tokenizers, chat templates, quantisation schemes, and licences — is a *catalogue-and-abstraction* problem. This is why the resolution introduces the Model Abstraction Layer, which did not exist in the original Mimir SAD. `[DD-4]`


## 3. Air-Gap Model Supply Chain

The core mechanism is a two-stage supply chain with cryptographic custody transfer. The enclosure never trusts an upstream source directly; it trusts only the Veldris signing root, which it already trusts for every Bifrost artefact. Acquisition thus collapses into the same trust model as a software release.

![Figure 3.1 — Two-stage air-gap model supply chain with custody transfer under Veldris ML-DSA re-signing.](../../diagrams/mimir_model_supply.png)
*Figure 3.1 — Two-stage air-gap model supply chain with custody transfer under Veldris ML-DSA re-signing.*


### 3.1 Stage 1 — Staging enclave (network-connected, controlled)

1. **Fetch** the target variant from upstream (Hugging Face, NVIDIA NGC, vendor CDN). `[FR-MS-1]`
2. **Verify native provenance** where it exists: vendor GPG signatures, Hugging Face commit hashes, published SHA-256 digests. Record what was and was not verifiable. `[FR-MS-2]` `[STD:NIST SA-12]`
3. **Capture licence** and run the licence-policy check (see §6); block artefacts whose terms are incompatible with the intended deployment. `[FR-MS-6]`
4. **Generate a model SBOM** (CycloneDX) describing weights, tokenizer, config, and declared lineage. `[NFR-OSS-1]`
5. **Run baseline Evals** and record a **hardware fingerprint** (min VRAM, GPU count, multi-node flag). `[FR-MS-5]`
6. **Re-sign** the assembled bundle under the Veldris ML-DSA (FIPS 204) signing key — the custody-transfer step. `[NFR-PQ-3]`


### 3.2 Stage 2 — Enclosure import (air-gapped)

1. The signed `model.mbundle` (weights + manifest + SBOM + ML-DSA signature) crosses the air gap via **Bifrost's existing signed-offline-bundle workflow** — no new transport is invented. `[FR-B-5]`
2. Bifrost **verifies the Veldris signature** on import and rejects any bundle that fails (fail closed). `[FR-B-4]` `[NFR-PQ-3]`
3. The verified artefact is registered in the **Model Registry** with status `approved`. `[FR-MS-3]`
4. The **inference runtime verifies the signature again on load** and refuses to place unverified or hardware-ineligible weights into GPU memory. `[FR-MS-4]`

> **The load-time invariant that closes the gap.** A model that is not in the registry, whose ML-DSA signature does not verify, whose licence gate is unmet, or whose hardware profile the node cannot satisfy, *cannot be loaded*. This single, testable invariant is what converts the finding from open to closed. `[FR-MS-4]`


## 4. Model Registry & Provenance Subsystem

The Model Registry is the in-enclosure system of record for every model the platform may load. It is the durable answer to provenance (G2) and the foundation of lifecycle management (G3).

![Figure 4.1 — Model Registry data model.](../../diagrams/mimir_model_registry_er.png)
*Figure 4.1 — Model Registry data model.*


### 4.1 Entity catalogue

| Entity | Purpose | Key attributes |
| --- | --- | --- |
| ModelFamily | One of the supported families | id, name (Mistral\|Nemotron\|Gemma), steward |
| ModelVariant | A specific variant within a family | id, family_id, label, modality |
| ModelArtefact | A packaged, signed, loadable build | id, version, quantisation, tokenizer_ref, sha256, mldsa_signature, sbom_ref, status |
| Provenance | How the artefact was obtained/verified | upstream_source, native_sig, staged_by, staged_at |
| Licence | Governing licence + restrictions | spdx_or_name, osi_approved, use_restrictions, redistribution_ok |
| EvalScore | Baseline/ongoing eval result | suite, score, run_at |
| HwProfile | Hardware eligibility | min_vram_gb, min_gpus, multinode |
| LoadEvent | Audited load attempt | loaded_at, sig_verified, node |


### 4.2 Artefact lifecycle states

| State | Meaning | Transition |
| --- | --- | --- |
| staged | Assembled and re-signed in staging, not yet imported | → approved on verified import |
| approved | Verified in-enclosure; loadable | → deprecated or revoked |
| deprecated | Superseded; loadable but flagged | → revoked |
| revoked | Withdrawn (e.g. licence/CVE/eval regression); not loadable | terminal |

Revocation is enforced at load time: a `revoked` artefact fails the load invariant even if its signature is otherwise valid, giving a fast kill-switch for a model found to be compromised, licence-breaching, or unsafe. `[FR-MS-4]` `[NFR-AI-1]`


## 5. Model Abstraction Layer & Routing

The Model Abstraction Layer preserves Mimir's unchanged inference contract while internally serving three heterogeneous families. Consumers (Forge, Bifrost, Heimdall, and Mimir's own agent runtime) call one API; the layer resolves which model actually answers.

![Figure 5.1 — Multi-model abstraction and policy routing behind the unchanged inference API.](../../diagrams/mimir_model_router.png)
*Figure 5.1 — Multi-model abstraction and policy routing behind the unchanged inference API.*


### 5.1 Per-family adapters

| Adapter | Handles | Notes |
| --- | --- | --- |
| Mistral adapter | Mistral chat template, tokenizer, serving quirks | Reasoning-strong default `[FR-MS-7]` |
| Nemotron adapter | Nemotron template/tokenizer, NVIDIA formats | Strong code/agentic tasks `[FR-MS-7]` |
| Gemma adapter | Gemma template/tokenizer, GGUF/quantised formats | Efficient small-footprint option `[FR-MS-7]` |


### 5.2 The router

The router selects an artefact per request from three inputs: **workload class** (e.g. fast-transform, deep-reasoning, code-analysis), **hardware eligibility** (from the artefact's HwProfile against the node), and **governance policy** (which families/variants are permitted for which markings, purposes, and tenants). Selection is deterministic, logged, and overridable by explicit request. `[FR-MS-8]` `[NFR-SEC-2]`

> **Composability preserved.** Because routing lives behind the existing inference API, Forge/Bifrost/Heimdall are unchanged: they still call one contract. Per-workflow model choice becomes a platform capability, not a per-product integration. `[DD-6]`


### 5.3 Worked routing examples (design intent)

| Consumer / task | Workload class | Typical selection |
| --- | --- | --- |
| Forge — NL→transform suggestion | fast-transform | Efficient Gemma variant (low latency) |
| Heimdall — course-of-action options | deep-reasoning | Largest eligible Mistral variant |
| Bifrost — release-risk analysis | code-analysis | Nemotron variant |
| Mimir agent — general | balanced | Policy default; overridable per agent |

These are illustrative defaults, not hard bindings; the actual mapping is policy-configured by the platform admin. `[FR-MS-8]`


## 6. Licence Governance (a first-class concern)

The three families carry materially different licences. This is not a footnote: the suite's open-source-first driver (`[DD-2]`) requires that non-OSI-licensed models are never folded into the OSS core. They are handled as separately-licensed, user-fetched artefacts with terms surfaced and accepted at install time.

| Family | Licence character (design-time assumption) | Handling |
| --- | --- | --- |
| Mistral | Varies by variant; some open, some restricted | Per-variant licence field; gate on redistribution_ok `[FR-MS-6]` |
| Nemotron | NVIDIA community/open model licences; conditions vary by model | Capture exact terms; surface use restrictions at install |
| Gemma | Google Gemma Terms of Use — custom, not OSI-approved, use restrictions apply | Never bundled into OSS core; user fetches + accepts terms |

> **Design rule.** The Mimir OSS core ships the *registry, adapters, router, and supply-chain tooling* — all Veldris-authored and OSI-licensed — but ships **no third-party model weights**. Weights are fetched by the operator into staging, where their licence is recorded and enforced. This keeps the open-source boundary clean while letting users run models they are entitled to use. `[DD-2]` `[FR-MS-6]`

Licence status is enforced at load, not merely displayed: an artefact whose licence policy is unmet for a given deployment is treated like a `revoked` artefact by the load invariant. `[FR-MS-4]`


## 7. Hardware Eligibility & Graceful Degradation

“Most advanced variant” is hardware-bound. The largest Mistral and Nemotron variants exceed the single-80GB-GPU reference hardware in `[NFR-PERF-1]` and require multi-GPU or multi-node serving. The registry records a HwProfile per artefact, and the runtime refuses to load a model a node cannot serve — offering the largest *eligible* variant instead.

| Node class (illustrative) | Eligible tier (design intent) |
| --- | --- |
| Single 24–48 GB GPU | Quantised small/mid variants (e.g. efficient Gemma, int4 Mistral/Nemotron) |
| Single 80 GB GPU (reference) | Mid/large single-GPU variants |
| Multi-GPU node | Large variants with tensor/pipeline parallelism |
| Multi-node cluster | Largest “most advanced” variants |

> **Fleet behaviour.** Across heterogeneous hardware the capability degrades gracefully rather than failing: every node runs the best model it can, and the router never offers a consumer a model the serving node cannot load. `[FR-MS-5]` `[NFR-PERF-1]`


## 8. Updated & New Acceptance Criteria

The original Mimir acceptance criterion **AC-MI-7** (single-model signature verification) is superseded by **AC-MS-2** below, generalised across three families. The remaining criteria are new and must all pass for the gap to be certified closed.

| ID | Given / When / Then | Provenance |
| --- | --- | --- |
| AC-MS-1 | Given an upstream model, when staged, then native provenance is verified where available, a CycloneDX SBOM is generated, the licence is captured, and the bundle is re-signed under the Veldris ML-DSA key. | `[FR-MS-1]` `[FR-MS-2]` `[NFR-PQ-3]` |
| AC-MS-2 | Given any model artefact of any supported family, when loaded, then its ML-DSA signature is verified and loading fails closed on mismatch (supersedes AC-MI-7). | `[FR-MS-4]` |
| AC-MS-3 | Given a signed bundle, when imported to an air-gapped enclosure, then it transits only via Bifrost's signed-bundle workflow and is rejected if the Veldris signature fails. | `[FR-B-4]` `[FR-B-5]` |
| AC-MS-4 | Given three families are registered (Mistral, Nemotron, Gemma, Granite), when a consumer calls the unchanged inference API, then the router selects an eligible, policy-permitted artefact and the consumer requires no code change. | `[FR-MS-7]` `[FR-MS-8]` `[DD-6]` |
| AC-MS-5 | Given a node that cannot satisfy a variant's HwProfile, when that variant is requested, then it is not loaded and the largest eligible variant is offered instead. | `[FR-MS-5]` |
| AC-MS-6 | Given a model whose licence policy is unmet for the deployment, when a load is attempted, then it is refused exactly as a revoked artefact is. | `[FR-MS-6]` |
| AC-MS-7 | Given an artefact set to revoked (licence/CVE/eval regression), when any load is attempted, then it fails closed regardless of signature validity. | `[FR-MS-4]` `[NFR-AI-1]` |
| AC-MS-8 | Given no outbound connectivity, when models are used, then all inference proceeds from locally-registered, verified artefacts (no hosted dependency). | `[DD-4]` `[NFR-PRV-1]` |
| AC-MS-9 | Given a model swap between families, when performed, then it is hot-swappable without changing the inference contract or migrating consumer data (crypto/model agility). | `[NFR-PQ-4]` `[DD-6]` |


## 9. New Functional Requirements

| ID | Requirement |
| --- | --- |
| FR-MS-1 | Fetch model variants from upstream sources in the staging enclave only. |
| FR-MS-2 | Verify native upstream provenance where present; record verifiability status. |
| FR-MS-3 | Maintain an in-enclosure Model Registry as the system of record for all loadable artefacts. |
| FR-MS-4 | Verify the Veldris ML-DSA signature, licence policy, and hardware eligibility on every load; fail closed. |
| FR-MS-5 | Record a hardware fingerprint per artefact; enforce hardware eligibility and offer the largest eligible variant. |
| FR-MS-6 | Capture and enforce per-model licence terms; never bundle non-OSI weights into the OSS core. |
| FR-MS-7 | Support Mistral, Nemotron, Gemma and Granite via per-family adapters behind the unchanged inference contract. |
| FR-MS-8 | Route inference per request by workload class, hardware eligibility, and governance policy; log every selection. |


## 10. Residual Risks After Resolution

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Staging enclave compromise (it touches untrusted upstream) | Medium | Isolate staging from the enclosure; only signed bundles cross; staging has no enclosure credentials. `[NFR-SEC-1]` |
| Upstream weight tampering before fetch | Medium | Verify native signatures/hashes; re-sign only after verification; record unverifiable status for risk acceptance. `[FR-MS-2]` |
| Licence drift (upstream re-licences a model) | Low | Licence captured at stage time is immutable in the registry; re-evaluation on any refresh. `[FR-MS-6]` |
| Model-level safety regressions across families | Medium | Per-family baseline + ongoing Evals; revoke on regression. `[NFR-AI-1]` |
| Signing-key compromise (custody root) | High | HSM-sealed Veldris signing key, rotation, threshold signing (shared with Bifrost). `[NFR-PQ-2]` |

> **Net effect.** The original *medium-severity, open* gap (“local model-weight provenance”) is **closed**. Its residuals are lower-order and are managed by controls the suite already specifies (HSM-sealed keys, Bifrost signing, Evals, zero-trust isolation). `[NFR-PQ-2]` `[NFR-PQ-3]`


## 11. Impact on Forge, Bifrost & Heimdall

Because the resolution lives behind Mimir's unchanged inference contract, the consuming products are unaffected in their integration surface. The only additions are capabilities they inherit for free:

- **Forge** may now request a fast, efficient model for transform suggestions and a stronger model for complex mappings — same API. `[FR-MS-8]`
- **Bifrost** gains a second role: it is the transport for signed model bundles (reusing its signed-bundle workflow) in addition to consuming inference for release analysis. `[FR-B-5]`
- **Heimdall** may route course-of-action generation to the largest eligible reasoning model while classification uses a faster one — same API, governed by policy. `[FR-MS-8]`
- **All three** retain their defined degraded mode: if no model is loadable, they operate without AI exactly as specified in their SADs. `[DD-6]`


## 12. Glossary Delta

| Term | Definition |
| --- | --- |
| Model bundle (.mbundle) | A signed package of weights + manifest + SBOM + ML-DSA signature that crosses the air gap via Bifrost. |
| Staging enclave | A network-connected, isolated environment that fetches, verifies, licences, evaluates, and re-signs models before they enter the enclosure. |
| Model Registry | The in-enclosure system of record for all loadable model artefacts and their provenance, licence, evals, and hardware eligibility. |
| Model Abstraction Layer | The adapter+router layer that serves Mistral, Nemotron, Gemma and Granite behind Mimir's unchanged inference contract. |
| Verify-on-load invariant | The rule that no model loads unless signature, licence, revocation status, and hardware eligibility all pass. |
| Custody transfer | Re-signing an externally-sourced artefact under the Veldris key so the enclosure trusts only the Veldris root, not the upstream. |
| Mimir | Reasoning/AI product. Local, agentic, self-* generative AI on Mistral Small 3.2. Also the shared inference layer for the other products. |
| Mistral Small 3.2 | A large language model served locally by Mimir. As of the model-supply resolution, Mimir supports the most advanced locally-runnable variants of four families — Mistral, NVIDIA Nemotron, Google Gemma and IBM Granite — selectable per workflow. |
| ML-DSA | Module-Lattice Digital Signature Algorithm (Dilithium), NIST FIPS 204. |


## Appendix A — New Diagram Source Index

| Diagram | Mermaid | draw.io |
| --- | --- | --- |
| Model supply chain | mimir_model_supply.mmd | mimir_model_supply.drawio |
| Model Registry data model | mimir_model_registry_er.mmd | mimir_model_registry_er.drawio |
| Model abstraction & routing | mimir_model_router.mmd | mimir_model_router.drawio |
