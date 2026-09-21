# Solutions Architecture Document — Midgard

*Core Platform — the sovereign, post-quantum enclosure for the suite*

**Competitor benchmark:** Palantir platform core (Ontology, Foundry substrate, Apollo-managed infrastructure)  
**Author:** Akuma Engineering Ltd. — Architecture Practice — for JB Benjamin, CEO / Founder  
**Version:** 1.0  ·  **Date:** 2026-07-02  ·  **Classification:** CONFIDENTIAL — RECIPIENT EYES ONLY  
**Document type:** Forward-looking design specification (greenfield). Provenance tags: `[DD-*]` design driver, `[FR-*]/[NFR-*]` requirement, `[STD:*]` external standard.

---


## 1. Executive Summary

Midgard is the core platform and umbrella suite of a sovereign, open-source, post-quantum-encrypted alternative to Palantir's product family. It is the *enclosure*: the defended trust boundary within which four products operate — Forge (data operations), Mimir (local agentic AI), Bifrost (continuous delivery), and Heimdall (operational fusion). Midgard itself provides the shared substrate every product depends upon: identity, the object/ontology fabric, storage, compute scheduling, governance, and post-quantum cryptography applied by default. `[DD-1]` `[DD-6]` `[DD-7]`

This document is a **forward-looking design specification** for a greenfield system. In keeping with the Imhotep doctrine that no claim be made without provenance, and because there is no repository to cite, every architectural decision here traces to a Design Driver (`[DD-n]`), a numbered requirement (`[FR-n]`/`[NFR-n]`), or an external standard (`[STD:...]`). It describes intended design, not verified implementation.

> **What Midgard is.** A self-hostable, air-gappable operating system for data-and-decision workflows. It unifies fragmented data into a semantic + kinetic object fabric (objects, properties, links; actions, functions, dynamic security), and exposes that fabric to humans and AI agents under one zero-trust, post-quantum security model. Each product is independently useful atop Midgard and compounds in value when combined. `[DD-6]`


### 1.1 Top design facts

- **Sovereign by construction.** No core function requires an outbound network call; the platform runs fully offline / air-gapped. `[NFR-PRV-1]` `[DD-7]`
- **Post-quantum by default.** Hybrid X25519 + ML-KEM-768 in transit; AES-256-GCM under ML-KEM-1024-wrapped keys at rest; ML-DSA / SLH-DSA signatures for all service auth and artefact signing. `[NFR-PQ-1]` `[NFR-PQ-2]` `[NFR-PQ-3]` `[STD:NIST FIPS 203/204/205]`
- **Object fabric at the centre.** A decision-centric semantic layer (the answer to Palantir's Ontology), backed by a horizontally-scalable object store with incremental indexing. `[DD-1]`
- **Local AI native.** The Mimir layer (GLM 5.2) provides all inference in-enclosure; other products call Mimir rather than any hosted LLM. `[DD-4]`
- **Open-source core.** Reproducible builds, CycloneDX SBOM per release, OSI-approved licence for the core. `[DD-2]` `[NFR-OSS-1]`


### 1.2 Top design risks

- **PQ performance overhead.** Larger key/ciphertext sizes and signature costs of ML-KEM/ML-DSA add latency and bandwidth versus classical crypto; mitigated by session resumption, hybrid negotiation, and hardware offload. `[NFR-PQ-4]`
- **Model-supply sovereignty.** Local GLM 5.2 weights must be acquired, verified, and updated without a hosted dependency; mitigated by signed model artefacts distributed through Bifrost. `[DD-4]` `[DD-7]`
- **Ontology sprawl.** A powerful object fabric can accrete inconsistent modelling; mitigated by shared property types, interfaces, and governed change management. `[DD-1]`


## 2. Introduction & Scope


### 2.1 Purpose

To define the target architecture of the Midgard core platform in sufficient depth to drive implementation planning, security review, and investment decisions, and to establish the shared contracts on which Forge, Mimir, Bifrost, and Heimdall depend.


### 2.2 Intended audience

- Engineering leadership and architects implementing the suite.
- Security and compliance reviewers assessing the post-quantum and zero-trust posture.
- Executive stakeholders evaluating competitive positioning against Palantir.


### 2.3 Scope

| In scope | Out of scope (covered elsewhere / future) |
| --- | --- |
| Core identity, object fabric, storage, compute, governance, PQ crypto substrate | Per-product internals (see the four product SADs) |
| Shared APIs/SDK and inter-product contracts | Physical datacentre design and hardware procurement |
| Reference deployment topologies (self-host, air-gap, HA) | Commercial pricing and licensing terms |
| Cross-cutting security architecture | Third-party connector catalogue beyond reference set |


### 2.4 Definitions & acronyms

| Term | Meaning |
| --- | --- |
| Midgard | The core platform and umbrella suite; the sovereign 'enclosure' providing identity, storage, compute, governance, and the object/ontology fabric. |
| Forge | Data-operations product; competitor to Palantir Foundry. Ingest, transform, version, and model data into the Midgard object fabric. |
| Mimir | Reasoning/AI product; competitor to Palantir AIP. Local, agentic, self-* generative AI on GLM 5.2. Also the shared inference layer for the other products. |
| Bifrost | Continuous-delivery product; competitor to Palantir Apollo. Orchestrates zero-downtime deployment across the suite. |
| Heimdall | Operational fusion product; competitor to Palantir Maven. Fuses feeds into a single operational picture with AI-assisted decision workflows. |
| GLM 5.2 | A large language model served locally by Mimir. As of the model-supply resolution, Mimir supports the most advanced locally-runnable variants of three families — GLM, NVIDIA Nemotron, and Google Gemma — selectable per workflow. |
| Object fabric | Midgard's semantic + kinetic data layer: objects, properties, links (semantics) and actions, functions, dynamic security (kinetics). |
| ML-KEM | Module-Lattice Key-Encapsulation Mechanism (Kyber), NIST FIPS 203. |
| ML-DSA | Module-Lattice Digital Signature Algorithm (Dilithium), NIST FIPS 204. |
| SLH-DSA | Stateless Hash-based Digital Signature Algorithm (SPHINCS+), NIST FIPS 205. |
| OAG | Ontology-Augmented Generation: grounding the LLM in the object fabric's data, logic, and actions. |
| Seal | The Kryotech post-quantum encryption action applied to objects and artefacts. |
| Enclosure | The Midgard trust boundary; the defended, sovereign space within which all products run. |


### 2.5 References

- arc42 template §1–§12; TOGAF ADM Phases B–D. `[DD-8]`
- NIST FIPS 203 (ML-KEM), 204 (ML-DSA), 205 (SLH-DSA); NIST SP 800-207 (Zero Trust); SP 800-53 Rev.5. `[STD]`
- OWASP ASVS 4.0; ISO/IEC 27001:2022 Annex A; C4 model for decomposition. `[DD-8]`


## 3. Business & Solution Context


### 3.1 Business goals

1. **DD-1 — Direct competitive parity-plus with Palantir.** Each product must be a direct and more capable competitor to its Palantir counterpart (Forge→Foundry, Mimir→AIP, Bifrost→Apollo, Heimdall→Maven), with improved design and usability.
2. **DD-2 — Open source.** The suite is open-source-first: the core of every product is released under an OSI-approved licence, with an optional commercial tier for advanced modules.
3. **DD-3 — Native post-quantum encryption.** Post-quantum cryptography is native and default across data-at-rest, data-in-transit, and inter-service authentication — not an add-on.
4. **DD-4 — Local AI processing via the Mimir layer.** Generative AI, reasoning, and inference are served locally by the Mimir layer. Users may run the most advanced locally-runnable variants of GLM, NVIDIA Nemotron, and Google Gemma, selectable per workflow. No product depends on a third-party hosted LLM to function.
5. **DD-5 — Agentic, self-* AI.** Mimir's AI is agentic and exhibits self-learning, self-healing, and self-defending behaviours, governed by an evaluation and guardrail framework.
6. **DD-6 — Independent yet composable.** Each product operates standalone atop Midgard (Midgard+Forge, Midgard+Mimir, Midgard+Bifrost, Midgard+Heimdall) and operates optimally when consumed together.
7. **DD-7 — Sovereignty and data control.** Users retain sovereign control of data and models; the architecture favours on-premises / self-hosted / air-gapped deployment (the 'Midgard enclosure').
8. **DD-8 — Standards-aligned assurance.** Structure follows arc42 + TOGAF ADM; security aligns to NIST SP 800-53 Rev.5, OWASP ASVS 4.0, ISO/IEC 27001:2022; component decomposition uses the C4 model.


### 3.2 Stakeholders

| Stakeholder | Concern |
| --- | --- |
| Data / Ops analyst | Fuse, explore, and act on data through the object fabric and applications. |
| Platform admin | Identity, governance, markings, upgrades, and enclosure integrity. |
| Developer | Build pipelines, functions, agents, and applications on the SDK. |
| Security officer | Zero-trust enforcement, PQ posture, audit lineage, sovereignty. |


### 3.3 System context (C4 Level 1)

The context below shows Midgard as the enclosure hosting the four products and integrating only optional, sovereignty-preserving external systems (data sources, an optional enterprise IdP, and an HSM/TPM for key sealing).

![Figure 3.1 — C4 Level 1 System Context: Midgard core hosting the four products within the enclosure.](../../diagrams/midgard_c4_context.png)
*Figure 3.1 — C4 Level 1 System Context: Midgard core hosting the four products within the enclosure.*


### 3.4 External systems & actors

| External | Nature | Coupling |
| --- | --- | --- |
| External data sources | Databases, APIs, files, streaming feeds | Inbound, pull-based, via Forge connectors; no reverse dependency. |
| Enterprise IdP (optional) | OIDC / SAML provider | Optional federation; Midgard has a self-contained IdP for air-gap. |
| HSM / TPM | Hardware key sealing | Optional; wraps root key material where present. `[NFR-PQ-2]` |


### 3.5 Constraints

- **Offline-capable:** no core function may depend on outbound connectivity. `[NFR-PRV-1]`
- **PQ-default:** classical-only cryptography is not an allowed configuration for new data. `[NFR-PQ-4]`
- **Open-core:** the core must build reproducibly from source. `[NFR-OSS-1]`


## 4. Architecture Drivers


### 4.1 Quality goals (cross-cutting NFRs)

| ID | Category | Requirement | Provenance |
| --- | --- | --- | --- |
| NFR-PQ-1 | Cryptography | All data-in-transit uses a hybrid TLS 1.3 handshake combining X25519 with ML-KEM-768 (Kyber) key agreement. | STD:NIST FIPS 203 |
| NFR-PQ-2 | Cryptography | All data-at-rest is encrypted with AES-256-GCM under keys wrapped by ML-KEM-1024; key material is sealed to an HSM/TPM where present. | STD:NIST FIPS 203 / FIPS 197 |
| NFR-PQ-3 | Cryptography | All service-to-service and artefact-signing operations use ML-DSA (Dilithium) or SLH-DSA (SPHINCS+) signatures. | STD:NIST FIPS 204 / FIPS 205 |
| NFR-PQ-4 | Cryptography | Cryptographic agility: algorithms are negotiated, versioned, and hot-swappable without data migration; a classical+PQ hybrid mode is the default. | STD:NIST SP 1800-38 |
| NFR-SEC-1 | Security | Zero-trust: every request between components is mutually authenticated and authorised on identity, device posture, and purpose. | STD:NIST SP 800-207 |
| NFR-SEC-2 | Security | Role-, marking-, and purpose-based access control is enforced at the data and object layer, with full audit lineage. | STD:NIST AC-3 / AC-16 |
| NFR-AVA-1 | Availability | Core control-plane services target 99.9% availability; the platform supports zero-downtime rolling upgrades. | DD-1 |
| NFR-PRV-1 | Privacy/Sovereignty | The suite runs fully offline / air-gapped with no outbound network dependency for any core function. | DD-7 |
| NFR-OSS-1 | Openness | All core components build reproducibly from source; an SBOM (CycloneDX) is emitted per release. | DD-2 |
| NFR-PERF-1 | Performance | Local inference first-token latency target < 800 ms on reference hardware (1×80GB GPU class); batch throughput scales horizontally. | DD-4 |
| NFR-AI-1 | AI Governance | Every agentic action is traceable, evaluated against an Evals suite, and reversible or human-gated where it mutates state. | DD-5 |


### 4.2 Architecturally significant requirements

- The object fabric must enforce role-, marking-, and purpose-based access at query time across humans and agents. `[NFR-SEC-2]`
- Cryptographic algorithms must be negotiated and hot-swappable without data migration. `[NFR-PQ-4]`
- The platform must support zero-downtime rolling upgrades orchestrated by Bifrost. `[NFR-AVA-1]`


### 4.3 Assumptions

- Reference hardware includes at least one GPU of the 80GB class for Mimir inference. `[NFR-PERF-1]`
- Deployments possess a trust root (HSM/TPM or software-sealed) for key custody. `[NFR-PQ-2]`


## 5. Solution Strategy


### 5.1 Technology decisions

| Concern | Decision | Rationale / provenance |
| --- | --- | --- |
| Inter-service transport | gRPC over hybrid-PQ mTLS (X25519+ML-KEM-768) | Zero-trust, PQ-in-transit. `[NFR-PQ-1]` `[NFR-SEC-1]` |
| Object store | Columnar + inverted-index store with incremental indexing | Fast object query at scale; parity-plus vs Palantir OSv2. `[DD-1]` |
| Compute | Kubernetes-based autoscaling mesh; batch + stream | Zero-downtime upgrades; horizontal scale. `[NFR-AVA-1]` |
| Identity | Self-contained OIDC IdP with optional federation | Air-gap capability. `[NFR-PRV-1]` |
| Crypto library | Vetted PQC implementation (liboqs-class) behind an agility shim | FIPS 203/204/205 with hot-swap. `[NFR-PQ-4]` |
| Packaging | OCI images, reproducible builds, CycloneDX SBOM | Open-source assurance. `[NFR-OSS-1]` |


### 5.2 Architectural patterns

- **Enclosure / trust-boundary** pattern: everything runs inside one defended boundary with a controlled ingress/egress surface. `[DD-7]`
- **Semantic + kinetic layering:** data (nouns) is paired with actions/functions (verbs) so decisions, not just data, are modelled. `[DD-1]`
- **Sidecar security:** a PQ-mTLS + policy sidecar mediates every service call (zero-trust data plane). `[NFR-SEC-1]`
- **Ports-and-adapters:** products depend on Midgard contracts, not implementations, enabling standalone operation. `[DD-6]`


### 5.3 Key design decisions

> **KDD-1 — Post-quantum is a platform property, not a product feature.** The PQ crypto substrate lives in Midgard and is inherited by every product, so no product re-implements cryptography. `[DD-3]` `[NFR-PQ-4]`

> **KDD-2 — Mimir is both a product and a service.** The same local-inference layer that is sold as the AIP competitor is the shared inference dependency for Forge, Bifrost, and Heimdall — one model plane, many consumers. `[DD-4]` `[DD-6]`

> **KDD-3 — Composability via contracts.** Each product declares a hard dependency only on Midgard core contracts; inter-product links are optional capabilities discovered at runtime. `[DD-6]`


## 6. Logical Architecture (C4 Level 2)


### 6.1 Container diagram

![Figure 6.1 — C4 Level 2 Containers: the deployable units of Midgard core.](../../diagrams/midgard_c4_containers.png)
*Figure 6.1 — C4 Level 2 Containers: the deployable units of Midgard core.*


### 6.2 Container descriptions


#### 6.2.1 Gateway & PQ-mTLS termination

Public ingress for human and SDK traffic. Terminates hybrid-PQ TLS, performs identity assertion, and forwards to the control plane over the mesh. Responsibilities: TLS 1.3 hybrid handshake, session management, rate limiting, request signing. `[NFR-PQ-1]` `[NFR-SEC-1]`


#### 6.2.2 Identity & Access service

Self-contained OIDC provider issuing PQ-signed tokens; manages users, groups, roles, markings, and purpose declarations. Federates to an enterprise IdP when configured, but functions fully offline. `[NFR-SEC-2]` `[NFR-PRV-1]`


#### 6.2.3 Object Fabric service (OFS)

The semantic + kinetic core: object types, properties, link types (semantics); actions, functions, dynamic security (kinetics). Backed by the object store; exposes typed query, search-around, and action APIs. This is the direct answer to Palantir's Ontology system. `[DD-1]`


#### 6.2.4 Object store & indexer

Columnar primary store plus inverted index; incremental indexing keeps object queries current as sources update. Writes are orchestrated through a write-funnel that reconciles source data and user/agent edits. `[DD-1]`


#### 6.2.5 Governance & audit service

Enforces role/marking/purpose policy at query time and records immutable, PQ-signed audit lineage of every read, write, and action by humans and agents. `[NFR-SEC-2]` `[NFR-AI-1]`


#### 6.2.6 Crypto & key-management service (KMS)

Central custody of PQ key material; performs envelope encryption, key wrapping under ML-KEM-1024, rotation, and the agility shim that negotiates and versions algorithms. Seals roots to HSM/TPM where present. `[NFR-PQ-2]` `[NFR-PQ-4]`


#### 6.2.7 Compute scheduler

Schedules batch and streaming jobs (pipelines, indexing, model inference requests) across the autoscaling mesh; enforces resource quotas and priority. `[NFR-AVA-1]`


#### 6.2.8 SDK / API surface

Generated, typed SDKs (TypeScript, Python) plus a CLI and an MCP endpoint, giving developers and agents governed access to objects, actions, and functions. `[DD-6]`


## 7. Component Architecture (C4 Level 3)

The Object Fabric service is the most architecturally significant container; its internal components are shown below. (Per-product component diagrams appear in each product SAD.)

![Figure 7.1 — Object Fabric logical entities and their relationships (see also §8).](../../diagrams/midgard_er.png)
*Figure 7.1 — Object Fabric logical entities and their relationships (see also §8).*


### 7.1 Object Fabric components

| Component | Responsibility | Provenance |
| --- | --- | --- |
| Type registry (OMS) | Defines object types, link types, action types, interfaces, shared properties, value types. | `[DD-1]` |
| Query engine | Typed object/link queries, search-around, aggregations under policy. | `[DD-1]` `[NFR-SEC-2]` |
| Action executor | Applies validated, function-backed edits with side-effects; writes audit lineage. | `[NFR-SEC-2]` `[NFR-AI-1]` |
| Function runtime | Executes business logic (rules, ML calls, LLM calls to Mimir) as typed functions. | `[DD-4]` |
| Write funnel | Reconciles source data + edits into the indexed store; keeps indexes current. | `[DD-1]` |
| Policy point | Evaluates role/marking/purpose at query and action time. | `[NFR-SEC-2]` |


## 8. Data Architecture


### 8.1 Logical data model

The object fabric models an organisation as typed objects connected by typed links, mutated by typed actions, and constrained by markings and policies. The entity model below is the platform-level schema; products extend it with domain object types.

![Figure 8.1 — Midgard core logical data model.](../../diagrams/midgard_er.png)
*Figure 8.1 — Midgard core logical data model.*


### 8.2 Entity catalogue

| Entity | Purpose | Key attributes |
| --- | --- | --- |
| ObjectType | Schema of a real-world entity/event | id, api_name, title, properties[], interfaces[] |
| ObjectInstance | One instance of an ObjectType | id (PK), type_id (FK), property_values, markings[] |
| LinkType | Schema of a relationship | id, source_type, target_type, cardinality |
| ActionType | Schema of a permitted mutation | id, parameters[], rules[], side_effects[], function_id |
| Function | Business logic unit | id, signature, runtime, code_ref, model_binding |
| Marking | Mandatory classification tag | id, name, level, compartments[] |
| Policy | Role/marking/purpose rule | id, subject, resource, purpose, effect |
| AuditRecord | Immutable lineage entry | id, actor, action, resource, ts, pq_signature |


### 8.3 Data stores

| Store | Type | Contents | Crypto at rest |
| --- | --- | --- | --- |
| Object store | Columnar + inverted index | Object instances, indexes | AES-256-GCM / ML-KEM-1024-wrapped `[NFR-PQ-2]` |
| Metadata store | Relational | Types, links, actions, policies | AES-256-GCM / ML-KEM-1024-wrapped |
| Audit store | Append-only ledger | PQ-signed audit lineage | ML-DSA-signed, encrypted `[NFR-PQ-3]` |
| Key store | KMS-managed / HSM-sealed | Wrapped key material | Sealed to HSM/TPM `[NFR-PQ-2]` |


### 8.4 Data flow & lineage

Source data enters via Forge connectors, is transformed into datasets, and is funnelled into object instances. Every subsequent read, edit (via Action), or agent operation is recorded in the append-only, PQ-signed audit store, providing complete lineage from source to decision. `[NFR-SEC-2]` `[NFR-AI-1]`


### 8.5 Data classification & retention

Markings drive mandatory access control and retention policy; data may be sealed (Kryotech Seal) for at-rest confidentiality with per-object key wrapping, and retention windows are enforced by the governance service. `[NFR-SEC-2]`


## 9. Integration Architecture


### 9.1 API surface

| Surface | Protocol | Purpose |
| --- | --- | --- |
| Object API | gRPC + REST over PQ-mTLS | CRUD/query on objects, links; search-around |
| Action API | gRPC + REST | Apply actions; returns audit id |
| Function API | gRPC | Invoke functions (incl. Mimir-backed) |
| Admin API | REST | Types, policies, markings, upgrades |
| MCP endpoint | MCP over PQ-mTLS | Governed agent/tool access to the fabric `[DD-4]` |


### 9.2 Asynchronous messaging

An internal event bus carries object-change events, index-update events, and audit events. Products subscribe to relevant topics; all payloads are encrypted and event provenance is signed. `[NFR-PQ-3]`


### 9.3 External integrations

External coupling is deliberately minimal and inbound-only: data-source connectors (Forge), optional IdP federation, and optional HSM/TPM. No core function calls an external service. `[NFR-PRV-1]`


### 9.4 Webhooks & callbacks

Outbound webhooks are an opt-in governance-gated capability (e.g. notify an external ticketing system) and are disabled by default in air-gapped deployments. `[DD-7]`


### 9.5 Authentication & authorisation flow

The sequence below shows the hybrid post-quantum handshake and token issuance that precedes every authenticated session.

![Figure 9.1 — Post-quantum authenticated session establishment (hybrid X25519 + ML-KEM-768).](../../diagrams/midgard_seq_pq_handshake.png)
*Figure 9.1 — Post-quantum authenticated session establishment (hybrid X25519 + ML-KEM-768).*


## 10. Infrastructure & Deployment


### 10.1 Deployment topology

![Figure 10.1 — Reference deployment topology (self-hosted HA within the enclosure).](../../diagrams/midgard_deployment.png)
*Figure 10.1 — Reference deployment topology (self-hosted HA within the enclosure).*


### 10.2 Compute platform

Kubernetes-based mesh; control-plane services run as stateful and stateless workloads with autoscaling; GPU node pool serves Mimir inference. Supports single-node (dev), HA cluster (prod), and air-gapped installations. `[NFR-AVA-1]` `[NFR-PRV-1]`


### 10.3 Networking

A default-deny network policy governs pod-to-pod traffic; every internal hop is PQ-mTLS; ingress is limited to the gateway; egress is denied by default and allow-listed only where a connector requires it. `[NFR-SEC-1]`


### 10.4 Storage

Persistent volumes for the object, metadata, and audit stores; encrypted at rest with PQ-wrapped keys; snapshotted for backup. `[NFR-PQ-2]`


### 10.5 Environments

| Environment | Purpose | Shape |
| --- | --- | --- |
| dev | Single-node developer install | 1 node, no HA, self-signed trust root |
| staging | Pre-production validation | HA cluster, full PQ, synthetic data |
| prod | Production / air-gapped | HA cluster, HSM-sealed roots, egress-deny |


### 10.6 CI/CD pipeline

![Figure 10.2 — Suite CI/CD pipeline (reproducible build, SBOM, PQ-signing, staged deploy via Bifrost).](../../diagrams/midgard_cicd.png)
*Figure 10.2 — Suite CI/CD pipeline (reproducible build, SBOM, PQ-signing, staged deploy via Bifrost).*


### 10.7 Release process

Every release is reproducibly built, SBOM-attested (CycloneDX), PQ-signed (ML-DSA), and promoted through environments by Bifrost with zero-downtime rolling upgrades and automatic rollback on health-check failure. `[NFR-OSS-1]` `[NFR-AVA-1]`


## 11. Security Architecture


### 11.1 Threat model (STRIDE DFD)

![Figure 11.1 — Data-flow diagram with trust boundaries for STRIDE analysis.](../../diagrams/midgard_threat_dfd.png)
*Figure 11.1 — Data-flow diagram with trust boundaries for STRIDE analysis.*

| Boundary crossing | STRIDE | Control |
| --- | --- | --- |
| Actor → Gateway | S, T, I, D | Hybrid-PQ mTLS, token auth, rate limiting `[NFR-PQ-1]` |
| Gateway → Fabric | T, R, E | Sidecar mTLS, policy point, signed audit `[NFR-SEC-2]` |
| Fabric → Object store | T, I | Encrypted at rest, integrity tags `[NFR-PQ-2]` |
| Any → KMS | I, E | HSM sealing, least-privilege, purpose checks `[NFR-PQ-2]` |


### 11.2 Identity & access management

Self-contained OIDC IdP; users, groups, roles, markings, purposes. PQ-signed tokens; optional federation. `[NFR-SEC-2]` `[STD:NIST AC-2/IA-2]`


### 11.3 Authentication mechanisms

Password auth uses Argon2id; MFA via WebAuthn/passkeys and TOTP; service auth uses ML-DSA-signed workload identities. `[STD:OWASP ASVS V2]` `[NFR-PQ-3]`


### 11.4 Authorisation model

Role-, marking-, and purpose-based access control evaluated at query and action time by the policy point; deny-by-default. `[NFR-SEC-2]` `[STD:NIST AC-3/AC-16]`


### 11.5 Cryptography

| Use | Primitive | Standard |
| --- | --- | --- |
| Key agreement (transit) | X25519 + ML-KEM-768 (hybrid) | FIPS 203 `[NFR-PQ-1]` |
| Bulk at rest | AES-256-GCM, keys wrapped ML-KEM-1024 | FIPS 197 / 203 `[NFR-PQ-2]` |
| Signatures | ML-DSA (Dilithium) / SLH-DSA (SPHINCS+) | FIPS 204 / 205 `[NFR-PQ-3]` |
| Password hashing | Argon2id | OWASP ASVS V2 |
| Agility | Negotiated, versioned, hot-swappable | NIST SP 1800-38 `[NFR-PQ-4]` |


### 11.6 Secrets management

No secrets in source or images; all secrets issued at runtime by the KMS and sealed to HSM/TPM; short-lived, rotated, PQ-wrapped. `[STD:OWASP ASVS V14.3]`


### 11.7 Input validation & output encoding

Typed value-types and schema validators constrain all writes at the fabric boundary; parameterised queries only; output encoding at every rendering surface. `[STD:OWASP ASVS V5]`


### 11.8 Dependency security

Reproducible builds; lockfiles; automated dependency and CVE scanning in CI; CycloneDX SBOM per release. `[NFR-OSS-1]` `[STD:OWASP ASVS V14.2]`


### 11.9 Transport & network security

TLS 1.3 hybrid-PQ only; default-deny network policy; egress-deny; HSTS on all HTTP surfaces. `[NFR-SEC-1]`


### 11.10 Logging, audit, monitoring

Immutable, PQ-signed audit lineage for every operation; structured logs; metrics and traces exported to the in-enclosure observability stack. `[NFR-AI-1]`


### 11.11 Compliance posture

The architecture is designed to support ISO/IEC 27001:2022, NIST SP 800-53 Rev.5, and OWASP ASVS 4.0 controls. No compliance certification is asserted; this is a design-intent mapping only. `[DD-8]`


### 11.12 Known design gaps

| Gap | Severity | Recommendation |
| --- | --- | --- |
| PQ hardware acceleration not yet ubiquitous | Medium | Provide software fallback + offload where HSM supports PQ. |
| Model-weight provenance for GLM 5.2 | Medium | Distribute signed weights via Bifrost; verify ML-DSA signature on load. |
| Air-gap update workflow | Low | Define signed offline update bundles. |


## 12. Observability

- **Logging:** structured, correlation-ID-tagged, in-enclosure aggregation. `[NFR-AI-1]`
- **Metrics:** control-plane and inference metrics; SLO dashboards for the 99.9% target. `[NFR-AVA-1]`
- **Tracing:** distributed traces across the mesh for request and action paths.
- **Error reporting:** in-enclosure sink; no external telemetry by default. `[NFR-PRV-1]`


## 13. Operational Concerns

- **Backup & recovery:** encrypted snapshots of object/metadata/audit stores; tested restore. `[NFR-PQ-2]`
- **DR posture:** HA within a site; cross-site replication optional and PQ-encrypted.
- **Performance:** first-token inference target < 800 ms on reference hardware. `[NFR-PERF-1]`
- **Scalability:** horizontal scale of fabric query nodes and compute mesh. `[NFR-AVA-1]`


## 14. Quality & Testing

- Contract tests for every inter-product interface (ensures standalone + composable operation). `[DD-6]`
- Cryptographic test vectors validated against FIPS 203/204/205 known-answer tests. `[NFR-PQ-4]`
- Policy-enforcement tests (role/marking/purpose) as executable specifications. `[NFR-SEC-2]`
- Chaos and upgrade tests to prove zero-downtime rolling upgrades. `[NFR-AVA-1]`


## 15. Risks & Technical Debt

| Risk | Severity | Mitigation |
| --- | --- | --- |
| PQ performance overhead | Medium | Session resumption, hybrid negotiation, hardware offload. `[NFR-PQ-4]` |
| Ontology sprawl | Medium | Shared properties, interfaces, governed change management. `[DD-1]` |
| Single model plane as shared dependency | Medium | Mimir HA + graceful degradation for products that can run without AI. `[DD-6]` |
| Open-core / commercial boundary drift | Low | Clear module licensing and SBOM discipline. `[DD-2]` |


## 16. Roadmap & Recommendations

1. Deliver Midgard core (identity, fabric, KMS, governance) as the first milestone — every product depends on it. `[DD-6]`
2. Stand up Mimir second — it is a shared dependency for Forge, Bifrost, and Heimdall. `[DD-4]`
3. Certify the PQ crypto substrate against FIPS known-answer tests before any GA. `[NFR-PQ-4]`
4. Publish the open-source core with reproducible builds and SBOM from day one. `[DD-2]`


## 17. Acceptance Criteria

Acceptance is expressed as verifiable, testable statements. Each must pass before Midgard core is considered GA-ready.

| ID | Given / When / Then | Provenance |
| --- | --- | --- |
| AC-M-1 | Given a new session, when a client connects, then the handshake negotiates hybrid X25519+ML-KEM-768 and fails closed if PQ is unavailable. | `[NFR-PQ-1]` |
| AC-M-2 | Given data at rest, when written, then it is AES-256-GCM encrypted under an ML-KEM-1024-wrapped key sealed to the trust root. | `[NFR-PQ-2]` |
| AC-M-3 | Given any service call, when made, then it is mutually authenticated and authorised on identity, posture, and purpose; unauthenticated calls are denied. | `[NFR-SEC-1]` |
| AC-M-4 | Given a query touching marked data, when executed, then role/marking/purpose policy is enforced and the decision is audited with a PQ signature. | `[NFR-SEC-2]` |
| AC-M-5 | Given no outbound connectivity, when all core functions are exercised, then every function succeeds (full air-gap). | `[NFR-PRV-1]` |
| AC-M-6 | Given a release, when built twice from source, then artefacts are bit-identical and a CycloneDX SBOM is emitted. | `[NFR-OSS-1]` |
| AC-M-7 | Given a running cluster, when a rolling upgrade is applied, then no request fails and rollback triggers automatically on health-check failure. | `[NFR-AVA-1]` |
| AC-M-8 | Given the algorithm registry, when a new PQ algorithm is enabled, then it is negotiated without data migration (crypto-agility). | `[NFR-PQ-4]` |
| AC-M-9 | Given any product installed standalone atop Midgard, when Mimir is absent, then the product operates in a defined degraded mode without error. | `[DD-6]` |


## Appendix A — Design Driver Index

| ID | Driver | Statement |
| --- | --- | --- |
| DD-1 | Direct competitive parity-plus with Palantir | Each product must be a direct and more capable competitor to its Palantir counterpart (Forge→Foundry, Mimir→AIP, Bifrost→Apollo, Heimdall→Maven), with improved design and usability. |
| DD-2 | Open source | The suite is open-source-first: the core of every product is released under an OSI-approved licence, with an optional commercial tier for advanced modules. |
| DD-3 | Native post-quantum encryption | Post-quantum cryptography is native and default across data-at-rest, data-in-transit, and inter-service authentication — not an add-on. |
| DD-4 | Local AI processing via the Mimir layer | Generative AI, reasoning, and inference are served locally by the Mimir layer. Users may run the most advanced locally-runnable variants of GLM, NVIDIA Nemotron, and Google Gemma, selectable per workflow. No product depends on a third-party hosted LLM to function. |
| DD-5 | Agentic, self-* AI | Mimir's AI is agentic and exhibits self-learning, self-healing, and self-defending behaviours, governed by an evaluation and guardrail framework. |
| DD-6 | Independent yet composable | Each product operates standalone atop Midgard (Midgard+Forge, Midgard+Mimir, Midgard+Bifrost, Midgard+Heimdall) and operates optimally when consumed together. |
| DD-7 | Sovereignty and data control | Users retain sovereign control of data and models; the architecture favours on-premises / self-hosted / air-gapped deployment (the 'Midgard enclosure'). |
| DD-8 | Standards-aligned assurance | Structure follows arc42 + TOGAF ADM; security aligns to NIST SP 800-53 Rev.5, OWASP ASVS 4.0, ISO/IEC 27001:2022; component decomposition uses the C4 model. |


## Appendix B — Cross-cutting NFR Index

| ID | Category | Requirement | Provenance |
| --- | --- | --- | --- |
| NFR-PQ-1 | Cryptography | All data-in-transit uses a hybrid TLS 1.3 handshake combining X25519 with ML-KEM-768 (Kyber) key agreement. | STD:NIST FIPS 203 |
| NFR-PQ-2 | Cryptography | All data-at-rest is encrypted with AES-256-GCM under keys wrapped by ML-KEM-1024; key material is sealed to an HSM/TPM where present. | STD:NIST FIPS 203 / FIPS 197 |
| NFR-PQ-3 | Cryptography | All service-to-service and artefact-signing operations use ML-DSA (Dilithium) or SLH-DSA (SPHINCS+) signatures. | STD:NIST FIPS 204 / FIPS 205 |
| NFR-PQ-4 | Cryptography | Cryptographic agility: algorithms are negotiated, versioned, and hot-swappable without data migration; a classical+PQ hybrid mode is the default. | STD:NIST SP 1800-38 |
| NFR-SEC-1 | Security | Zero-trust: every request between components is mutually authenticated and authorised on identity, device posture, and purpose. | STD:NIST SP 800-207 |
| NFR-SEC-2 | Security | Role-, marking-, and purpose-based access control is enforced at the data and object layer, with full audit lineage. | STD:NIST AC-3 / AC-16 |
| NFR-AVA-1 | Availability | Core control-plane services target 99.9% availability; the platform supports zero-downtime rolling upgrades. | DD-1 |
| NFR-PRV-1 | Privacy/Sovereignty | The suite runs fully offline / air-gapped with no outbound network dependency for any core function. | DD-7 |
| NFR-OSS-1 | Openness | All core components build reproducibly from source; an SBOM (CycloneDX) is emitted per release. | DD-2 |
| NFR-PERF-1 | Performance | Local inference first-token latency target < 800 ms on reference hardware (1×80GB GPU class); batch throughput scales horizontally. | DD-4 |
| NFR-AI-1 | AI Governance | Every agentic action is traceable, evaluated against an Evals suite, and reversible or human-gated where it mutates state. | DD-5 |


## Appendix C — Glossary

| Term | Definition |
| --- | --- |
| Midgard | The core platform and umbrella suite; the sovereign 'enclosure' providing identity, storage, compute, governance, and the object/ontology fabric. |
| Forge | Data-operations product; competitor to Palantir Foundry. Ingest, transform, version, and model data into the Midgard object fabric. |
| Mimir | Reasoning/AI product; competitor to Palantir AIP. Local, agentic, self-* generative AI on GLM 5.2. Also the shared inference layer for the other products. |
| Bifrost | Continuous-delivery product; competitor to Palantir Apollo. Orchestrates zero-downtime deployment across the suite. |
| Heimdall | Operational fusion product; competitor to Palantir Maven. Fuses feeds into a single operational picture with AI-assisted decision workflows. |
| GLM 5.2 | A large language model served locally by Mimir. As of the model-supply resolution, Mimir supports the most advanced locally-runnable variants of three families — GLM, NVIDIA Nemotron, and Google Gemma — selectable per workflow. |
| Object fabric | Midgard's semantic + kinetic data layer: objects, properties, links (semantics) and actions, functions, dynamic security (kinetics). |
| ML-KEM | Module-Lattice Key-Encapsulation Mechanism (Kyber), NIST FIPS 203. |
| ML-DSA | Module-Lattice Digital Signature Algorithm (Dilithium), NIST FIPS 204. |
| SLH-DSA | Stateless Hash-based Digital Signature Algorithm (SPHINCS+), NIST FIPS 205. |
| OAG | Ontology-Augmented Generation: grounding the LLM in the object fabric's data, logic, and actions. |
| Seal | The Kryotech post-quantum encryption action applied to objects and artefacts. |
| Enclosure | The Midgard trust boundary; the defended, sovereign space within which all products run. |


## Appendix D — Diagram Source Index

Every diagram in this document is provided as editable Mermaid (`.mmd`) and draw.io (`.drawio`) source alongside the rendered PNG, in the `diagrams/` directory.

| Diagram | Mermaid | draw.io |
| --- | --- | --- |
| System context | midgard_c4_context.mmd | midgard_c4_context.drawio |
| Containers | midgard_c4_containers.mmd | midgard_c4_containers.drawio |
| Data model | midgard_er.mmd | midgard_er.drawio |
| PQ handshake | midgard_seq_pq_handshake.mmd | midgard_seq_pq_handshake.drawio |
| Deployment | midgard_deployment.mmd | midgard_deployment.drawio |
| CI/CD | midgard_cicd.mmd | midgard_cicd.drawio |
| Threat DFD | midgard_threat_dfd.mmd | midgard_threat_dfd.drawio |
| User journey | midgard_journey.mmd | midgard_journey.drawio |
