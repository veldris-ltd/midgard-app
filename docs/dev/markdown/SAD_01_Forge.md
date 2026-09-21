# Solutions Architecture Document — Forge

*Data Operations — a post-quantum, AI-assisted data-operations platform*

**Author:** Veldris Ltd (company no. 17366869)  
**Version:** 1.0  ·  **Date:** 2026-07-02  ·  **Classification:** PUBLIC, published with the Midgard repository  
**Document type:** Forward-looking design specification (greenfield). Provenance tags: `[DD-*]` design driver, `[FR-*]/[NFR-*]` requirement, `[STD:*]` external standard.

---


## 1. Executive Summary

Forge is the data-operations product of the Midgard suite. It ingests data from any source, transforms it through low-code and pro-code pipelines, versions every dataset like source control, and projects the result into the Midgard object fabric. Forge goes beyond established platforms in three deliberate ways: native post-quantum encryption of all data at rest and in transit; local AI assistance for pipeline authoring served by the Mimir layer (Mistral Small 3.2) rather than a hosted model; and a sovereignty-first design that runs fully air-gapped. `[DD-1]` `[DD-3]` `[DD-4]` `[DD-7]`

This is a **forward-looking design specification** for a greenfield system; every decision carries a provenance tag (`[DD-n]`, `[FR-n]`, `[NFR-n]`, `[STD:...]`) rather than a repository citation.

> **Standalone and composable.** Forge runs atop Midgard core alone (Midgard + Forge) to deliver data operations. With Mimir present it gains AI-assisted transforms; with Bifrost it gains governed pipeline promotion; with Heimdall its curated objects feed the operational picture. `[DD-6]`


### 1.1 Top design facts

- **Dual-mode pipelines:** a visual low-code builder and a pro-code IDE over the same dataset contracts, interoperable in one pipeline. `[FR-F-1]`
- **Git-style data versioning:** every dataset is versioned with full lineage, branches, and reproducible rebuilds. `[FR-F-3]`
- **PQ-native storage:** datasets encrypted with AES-256-GCM under ML-KEM-1024-wrapped keys. `[NFR-PQ-2]`
- **Local AI transforms:** natural-language-to-transform and data-quality suggestions from Mimir, in-enclosure. `[DD-4]`
- **Object projection:** curated datasets map directly to Midgard object/link types. `[DD-1]`


### 1.2 Top design risks

- **Pipeline health at scale:** complex DAGs can silently drift; mitigated by expectations/health checks on every build. `[FR-F-6]`
- **Connector sprawl:** many source types raise maintenance cost; mitigated by a typed connector SDK. `[FR-F-2]`
- **AI-suggested transforms correctness:** Mimir suggestions must be verifiable; mitigated by preview + human approval before commit. `[NFR-AI-1]`


## 2. Introduction & Scope


### 2.1 Purpose

To define Forge's target architecture: ingestion, transformation, versioning, data health, and projection into the object fabric, plus its dependencies on Midgard core and optional dependencies on Mimir, Bifrost, and Heimdall.


### 2.2 Intended audience

- Data engineers and pipeline authors.
- Platform admins governing data pipelines.
- Security reviewers of the data plane.


### 2.3 Scope

| In scope | Out of scope |
| --- | --- |
| Connectors, pipelines (low/pro-code), versioning, health, object projection | Object fabric internals (Midgard SAD) |
| Forge-specific APIs and UI | Model serving internals (Mimir SAD) |
| Forge deployment within the enclosure | Deployment orchestration internals (Bifrost SAD) |


### 2.4 Definitions

| Term | Meaning |
| --- | --- |
| Forge | Data-operations product. Ingest, transform, version, and model data into the Midgard object fabric. |
| Mimir | Reasoning/AI product. Local, agentic, self-* generative AI on Mistral Small 3.2. Also the shared inference layer for the other products. |
| Object fabric | Midgard's semantic + kinetic data layer: objects, properties, links (semantics) and actions, functions, dynamic security (kinetics). |
| OAG | Ontology-Augmented Generation: grounding the LLM in the object fabric's data, logic, and actions. |
| Seal | The Veldris post-quantum encryption action applied to objects and artefacts. |
| Enclosure | The Midgard trust boundary; the defended, sovereign space within which all products run. |


## 3. Business & Solution Context


### 3.1 Competitive positioning

| Capability | Established platforms | Forge (design intent) |
| --- | --- | --- |
| Pipelines | Pipeline Builder + Code Repositories | Low-code builder + pro-code IDE, same contracts `[FR-F-1]` |
| Crypto | Standard cloud encryption | Native PQ at rest + in transit `[NFR-PQ-2]` |
| AI assist | Hosted LLM features | Local Mimir (Mistral Small 3.2), air-gap capable `[DD-4]` |
| Deployment | Cloud / hybrid | Self-host / air-gap first `[DD-7]` |
| Licensing | Proprietary | Open-source core `[DD-2]` |


### 3.2 System context (C4 L1)

![Figure 3.1 — Forge system context within the Midgard enclosure.](../../diagrams/forge_c4_context.png)
*Figure 3.1 — Forge system context within the Midgard enclosure.*


### 3.3 Actors & external systems

| Actor / system | Interaction |
| --- | --- |
| Data engineer | Builds and runs pipelines via UI/SDK. |
| External data sources | Pulled by connectors; inbound only. `[FR-F-2]` |
| Midgard object fabric | Receives projected objects/links. `[DD-1]` |
| Mimir (optional) | Serves AI transform/quality suggestions. `[DD-4]` |


## 4. Architecture Drivers


### 4.1 Functional requirements

| ID | Requirement |
| --- | --- |
| FR-F-1 | Provide a low-code visual pipeline builder and a pro-code IDE over identical dataset contracts. |
| FR-F-2 | Provide a typed connector SDK; ship reference connectors for databases, object stores, APIs, files, and streams. |
| FR-F-3 | Version every dataset with full lineage, branching, and reproducible rebuilds. |
| FR-F-4 | Project curated datasets into Midgard object/link types via a mapping layer. |
| FR-F-5 | Support batch and streaming pipelines with schedule/trigger management. |
| FR-F-6 | Enforce data-health expectations on every build; block promotion on failure. |
| FR-F-7 | Offer AI-assisted transforms (NL→transform, quality suggestions) via Mimir, preview-before-commit. |


### 4.2 Inherited NFRs

| ID | Requirement | Provenance |
| --- | --- | --- |
| NFR-PQ-1 | All data-in-transit uses a hybrid TLS 1.3 handshake combining X25519 with ML-KEM-768 (Kyber) key agreement. | STD:NIST FIPS 203 |
| NFR-PQ-2 | All data-at-rest is encrypted with AES-256-GCM under keys wrapped by ML-KEM-1024; key material is sealed to an HSM/TPM where present. | STD:NIST FIPS 203 / FIPS 197 |
| NFR-SEC-2 | Role-, marking-, and purpose-based access control is enforced at the data and object layer, with full audit lineage. | STD:NIST AC-3 / AC-16 |
| NFR-PRV-1 | The suite runs fully offline / air-gapped with no outbound network dependency for any core function. | DD-7 |
| NFR-OSS-1 | All core components build reproducibly from source; an SBOM (CycloneDX) is emitted per release. | DD-2 |
| NFR-AI-1 | Every agentic action is traceable, evaluated against an Evals suite, and reversible or human-gated where it mutates state. | DD-5 |


## 5. Solution Strategy

| Concern | Decision | Provenance |
| --- | --- | --- |
| Transform engine | Distributed batch + stream engine behind a builder abstraction | `[FR-F-1]` `[FR-F-5]` |
| Versioning | Immutable dataset snapshots + branch refs (content-addressed) | `[FR-F-3]` |
| Connectors | Typed connector SDK; sandboxed execution | `[FR-F-2]` |
| AI assist | Mimir functions invoked with preview + approval | `[FR-F-7]` `[NFR-AI-1]` |
| Crypto | Inherited from Midgard KMS (PQ) | `[NFR-PQ-2]` |

> **KDD — AI never silently mutates data.** Every Mimir-suggested transform is rendered as a previewable, diffable change requiring human approval before it is committed to a dataset version. `[NFR-AI-1]`


## 6. Logical Architecture (C4 L2)

![Figure 6.1 — Forge containers.](../../diagrams/forge_c4_containers.png)
*Figure 6.1 — Forge containers.*


#### 6.1 Connector service

Runs typed connectors in sandboxes; pulls source data into raw datasets; manages sync schedules and credentials (via Midgard KMS). `[FR-F-2]`


#### 6.2 Pipeline service

Compiles builder graphs and pro-code transforms into execution plans on the batch/stream engine; enforces type-safety and health checks. `[FR-F-1]` `[FR-F-6]`


#### 6.3 Dataset & versioning store

Content-addressed, immutable dataset snapshots with branch refs and lineage; PQ-encrypted at rest. `[FR-F-3]` `[NFR-PQ-2]`


#### 6.4 Projection service

Maps curated datasets to Midgard object/link types and drives the write funnel. `[FR-F-4]`


#### 6.5 AI-assist adapter

Bridges pipeline authoring to Mimir functions for NL→transform and quality suggestions, with preview/approval. `[FR-F-7]`


## 7. Component Architecture (C4 L3)

![Figure 7.1 — Pipeline service components.](../../diagrams/forge_c4_component.png)
*Figure 7.1 — Pipeline service components.*

| Component | Responsibility |
| --- | --- |
| Graph compiler | Builder graph → typed execution plan. `[FR-F-1]` |
| Transform library | Reusable, type-checked transforms. |
| Health engine | Expectations/checks per build. `[FR-F-6]` |
| Scheduler adapter | Batch/stream triggers via Midgard compute. `[FR-F-5]` |
| Lineage recorder | Emits lineage + audit to governance. `[NFR-SEC-2]` |


## 8. Data Architecture

![Figure 8.1 — Forge data model (datasets, versions, pipelines, mappings).](../../diagrams/forge_er.png)
*Figure 8.1 — Forge data model (datasets, versions, pipelines, mappings).*

| Entity | Purpose |
| --- | --- |
| Connector | Source binding + credentials ref. |
| Dataset | Named, versioned data artefact. |
| DatasetVersion | Immutable content-addressed snapshot. `[FR-F-3]` |
| Pipeline | Graph/pro-code transform definition. |
| BuildRun | One execution with health results. `[FR-F-6]` |
| ObjectMapping | Dataset→object/link projection. `[FR-F-4]` |


### 8.2 Data stores

| Store | Contents | Crypto |
| --- | --- | --- |
| Dataset store | Snapshots, branches | PQ-wrapped AES-256-GCM `[NFR-PQ-2]` |
| Pipeline metadata | Graphs, schedules, health | PQ-wrapped |


### 8.3 Data flow

Source → connector → raw dataset → transforms (versioned) → curated dataset → projection → object fabric, with lineage recorded at each hop. `[FR-F-4]` `[NFR-SEC-2]`


## 9. Integration Architecture

| Surface | Protocol | Purpose |
| --- | --- | --- |
| Pipeline API | gRPC/REST over PQ-mTLS | Create/run/monitor pipelines |
| Dataset API | gRPC/REST | Read/write/version datasets |
| Connector SDK | Library | Author custom connectors `[FR-F-2]` |
| Mimir function calls | gRPC (internal) | AI transform/quality assist `[FR-F-7]` |


### 9.2 Primary sequence

![Figure 9.1 — Author → preview (AI-assisted) → health-check → commit version → project to objects.](../../diagrams/forge_seq_primary.png)
*Figure 9.1 — Author → preview (AI-assisted) → health-check → commit version → project to objects.*


### 9.3 User journey

![Figure 9.2 — Data engineer primary journey.](../../diagrams/forge_journey.png)
*Figure 9.2 — Data engineer primary journey.*


## 10. Infrastructure & Deployment

![Figure 10.1 — Forge deployment within the enclosure.](../../diagrams/forge_deployment.png)
*Figure 10.1 — Forge deployment within the enclosure.*

Forge runs as stateless pipeline/connector workers plus stateful dataset stores on the Midgard mesh; scales workers horizontally; inherits egress-deny and PQ-mTLS. `[NFR-AVA-1]` `[NFR-SEC-1]`


### 10.2 CI/CD

![Figure 10.2 — Forge build/test/sign/deploy pipeline (via Bifrost).](../../diagrams/forge_cicd.png)
*Figure 10.2 — Forge build/test/sign/deploy pipeline (via Bifrost).*


## 11. Security Architecture

![Figure 11.1 — Forge STRIDE data-flow diagram.](../../diagrams/forge_threat_dfd.png)
*Figure 11.1 — Forge STRIDE data-flow diagram.*

| Crossing | STRIDE | Control |
| --- | --- | --- |
| Source → Connector | S, T, I | Sandboxed connector, credential from KMS, TLS `[NFR-PQ-1]` |
| Pipeline → Dataset store | T, I | PQ at rest, integrity tags `[NFR-PQ-2]` |
| AI adapter → Mimir | T, R | Internal mTLS, audited calls, preview gate `[NFR-AI-1]` |
| Projection → Fabric | T, E | Policy point, marking propagation `[NFR-SEC-2]` |


### 11.2 Cryptography

All at-rest and in-transit crypto inherited from Midgard: PQ-hybrid transit, PQ-wrapped at rest. `[NFR-PQ-1]` `[NFR-PQ-2]`


### 11.3 Access control

Pipeline and dataset operations authorised by role/marking/purpose; markings propagate through transforms into projected objects. `[NFR-SEC-2]`


### 11.4 Known design gaps

| Gap | Severity | Recommendation |
| --- | --- | --- |
| Third-party connector trust | Medium | Signed connector packages; sandboxed execution. `[FR-F-2]` |
| Large-dataset PQ overhead | Low | Chunked envelope encryption; hardware offload. `[NFR-PQ-4]` |


## 12. Observability

- Per-build health metrics and lineage. `[FR-F-6]`
- Pipeline run traces; failure alerting.
- In-enclosure log aggregation. `[NFR-PRV-1]`


## 13. Operational Concerns

- Dataset snapshots are backups by construction. `[FR-F-3]`
- Reproducible rebuild from lineage for recovery.
- Horizontal worker scaling for throughput. `[NFR-AVA-1]`


## 14. Quality & Testing

- Transform unit tests + golden datasets.
- Contract tests against Midgard object APIs. `[DD-6]`
- Health-check regression suite. `[FR-F-6]`


## 15. Risks & Technical Debt

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Connector maintenance burden | Medium | Typed SDK + community contributions. `[FR-F-2]` |
| Pipeline drift | Medium | Mandatory health checks + alerts. `[FR-F-6]` |


## 16. Roadmap

1. Ship low-code builder + core connectors.
2. Add pro-code IDE + versioning.
3. Add Mimir AI-assist (preview/approve).
4. Add streaming pipelines.


## 17. Acceptance Criteria

| ID | Given / When / Then | Provenance |
| --- | --- | --- |
| AC-F-1 | Given a source, when a connector syncs, then a raw dataset is created and encrypted with a PQ-wrapped key. | `[FR-F-2]` `[NFR-PQ-2]` |
| AC-F-2 | Given a pipeline, when built twice from the same inputs, then output dataset versions are content-identical. | `[FR-F-3]` |
| AC-F-3 | Given failing health expectations, when a build runs, then promotion is blocked and the failure is reported. | `[FR-F-6]` |
| AC-F-4 | Given a curated dataset, when projected, then the corresponding Midgard objects/links appear with markings propagated. | `[FR-F-4]` `[NFR-SEC-2]` |
| AC-F-5 | Given an AI-suggested transform, when proposed, then it is shown as a preview diff and is not committed without approval. | `[FR-F-7]` `[NFR-AI-1]` |
| AC-F-6 | Given Mimir is absent, when authoring, then all non-AI pipeline features remain fully functional (standalone mode). | `[DD-6]` |
| AC-F-7 | Given no outbound connectivity, when pipelines run, then all core data operations succeed. | `[NFR-PRV-1]` |


## Appendix A — Diagram Source Index

| Diagram | Mermaid | draw.io |
| --- | --- | --- |
| Context | forge_c4_context.mmd | forge_c4_context.drawio |
| Containers | forge_c4_containers.mmd | forge_c4_containers.drawio |
| Components | forge_c4_component.mmd | forge_c4_component.drawio |
| Data model | forge_er.mmd | forge_er.drawio |
| Primary sequence | forge_seq_primary.mmd | forge_seq_primary.drawio |
| Journey | forge_journey.mmd | forge_journey.drawio |
| Deployment | forge_deployment.mmd | forge_deployment.drawio |
| CI/CD | forge_cicd.mmd | forge_cicd.drawio |
| Threat DFD | forge_threat_dfd.mmd | forge_threat_dfd.drawio |
