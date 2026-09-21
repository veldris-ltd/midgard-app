# Solutions Architecture Document — Heimdall

*Operational Fusion — a post-quantum, AI-assisted operational-fusion platform*

**Author:** Veldris Ltd (company no. 17366869)  
**Version:** 1.0  ·  **Date:** 2026-07-02  ·  **Classification:** PUBLIC, published with the Midgard repository  
**Document type:** Forward-looking design specification (greenfield). Provenance tags: `[DD-*]` design driver, `[FR-*]/[NFR-*]` requirement, `[STD:*]` external standard.

---


## 1. Executive Summary

Heimdall is the operational-fusion product of the Midgard suite. It is the warden of the enclosure boundary: it fuses many data feeds into a single, live operational picture, and drives AI-assisted decision workflows on top of it. Heimdall goes beyond established platforms with native post-quantum encryption across all feeds and the common picture, and with decision intelligence served by the local Mimir layer (Mistral Small 3.2) instead of a hosted model — enabling operation in disconnected and air-gapped environments. `[DD-1]` `[DD-3]` `[DD-4]` `[DD-7]`

This document is a **forward-looking design specification**; every decision carries a provenance tag rather than a repository citation.

> **Human-in-the-loop by design.** Heimdall provides decision support — fusion, classification, option generation, and workflow — with humans authorising any consequential action. AI assists; people decide. `[NFR-AI-1]` `[DD-5]`

> **Standalone and composable.** Midgard + Heimdall delivers fusion and a common operating picture. With Mimir it gains AI classification and option generation; with Forge it consumes curated object feeds; Bifrost keeps it upgraded. `[DD-6]`


### 1.1 Top design facts

- **Multi-feed fusion** into one live, shared operational picture with layer toggles. `[FR-H-1]`
- **Entity tracking + classification** over the object fabric. `[FR-H-2]`
- **AI-assisted option generation** (ranked courses of action) via local Mimir, advisory. `[FR-H-4]` `[DD-4]`
- **Human-gated action workflows** with full audit lineage. `[FR-H-5]` `[NFR-AI-1]`
- **PQ-native** feeds, picture, and inter-node sync. `[NFR-PQ-1]` `[NFR-PQ-2]`


### 1.2 Top design risks

- **Automation bias:** operators may over-trust AI options; mitigated by explainable, cited options + mandatory human authorisation. `[NFR-AI-1]`
- **Feed integrity:** spoofed feeds corrupt the picture; mitigated by PQ-authenticated feed sources. `[NFR-PQ-3]`
- **Latency at scale:** many high-rate feeds strain fusion; mitigated by streaming fusion + horizontal scale. `[NFR-AVA-1]`


## 2. Introduction & Scope


### 2.1 Purpose

To define Heimdall's target architecture: feed integration, fusion, entity tracking/classification, common operating picture, AI-assisted decision workflows, and dependencies on Midgard core and optional Mimir/Forge.


### 2.2 Audience

- Operations analysts and decision-makers.
- Integrators connecting feeds.
- Security and safety reviewers.


### 2.3 Scope

| In scope | Out of scope |
| --- | --- |
| Feeds, fusion, tracking, classification, common picture, decision workflows | Object fabric internals (Midgard SAD) |
| Heimdall APIs/UI | Model internals (Mimir SAD); pipeline internals (Forge SAD) |


### 2.4 Definitions

| Term | Meaning |
| --- | --- |
| Mimir | Reasoning/AI product. Local, agentic, self-* generative AI on Mistral Small 3.2. Also the shared inference layer for the other products. |
| Heimdall | Operational fusion product. Fuses feeds into a single operational picture with AI-assisted decision workflows. |
| Object fabric | Midgard's semantic + kinetic data layer: objects, properties, links (semantics) and actions, functions, dynamic security (kinetics). |
| Enclosure | The Midgard trust boundary; the defended, sovereign space within which all products run. |


## 3. Business & Solution Context


### 3.1 Competitive positioning

| Capability | Established platforms | Heimdall (design intent) |
| --- | --- | --- |
| Fusion | Multi-source single picture | Multi-feed single picture + PQ-authenticated feeds `[NFR-PQ-3]` |
| Decision support | AI classification + COA | Local-Mimir classification + ranked options `[FR-H-4]` |
| Autonomy | Human-in-the-loop | Human-gated actions, cited options `[NFR-AI-1]` |
| Deployment | Cloud/classified cloud | Air-gap capable, PQ-native `[DD-7]` |
| Licensing | Proprietary | Open-source core `[DD-2]` |


### 3.2 System context (C4 L1)

![Figure 3.1 — Heimdall system context within the enclosure.](../../diagrams/heimdall_c4_context.png)
*Figure 3.1 — Heimdall system context within the enclosure.*


## 4. Architecture Drivers


### 4.1 Functional requirements

| ID | Requirement |
| --- | --- |
| FR-H-1 | Ingest and fuse multiple live feeds into one shared operational picture with layer control. |
| FR-H-2 | Track and classify entities over time against the object fabric. |
| FR-H-3 | Authenticate feed sources with PQ signatures; reject unverified feeds. |
| FR-H-4 | Generate AI-assisted, explainable, cited ranked options via local Mimir (advisory). |
| FR-H-5 | Drive human-gated action workflows with full audit lineage. |
| FR-H-6 | Synchronise the common picture across nodes/sites over PQ-mTLS. |


### 4.2 Inherited NFRs

| ID | Requirement | Provenance |
| --- | --- | --- |
| NFR-PQ-1 | All data-in-transit uses a hybrid TLS 1.3 handshake combining X25519 with ML-KEM-768 (Kyber) key agreement. | STD:NIST FIPS 203 |
| NFR-PQ-3 | All service-to-service and artefact-signing operations use ML-DSA (Dilithium) or SLH-DSA (SPHINCS+) signatures. | STD:NIST FIPS 204 / FIPS 205 |
| NFR-SEC-2 | Role-, marking-, and purpose-based access control is enforced at the data and object layer, with full audit lineage. | STD:NIST AC-3 / AC-16 |
| NFR-AVA-1 | Core control-plane services target 99.9% availability; the platform supports zero-downtime rolling upgrades. | DD-1 |
| NFR-PRV-1 | The suite runs fully offline / air-gapped with no outbound network dependency for any core function. | DD-7 |
| NFR-AI-1 | Every agentic action is traceable, evaluated against an Evals suite, and reversible or human-gated where it mutates state. | DD-5 |


## 5. Solution Strategy

| Concern | Decision | Provenance |
| --- | --- | --- |
| Fusion | Streaming fusion engine over object fabric | `[FR-H-1]` `[NFR-AVA-1]` |
| Feed trust | PQ-signed feed sources; verify on ingest | `[FR-H-3]` `[NFR-PQ-3]` |
| Decision support | Mimir options: explainable, cited, advisory | `[FR-H-4]` `[NFR-AI-1]` |
| Action | Human-gated workflow with audit | `[FR-H-5]` |
| Sync | PQ-mTLS multi-node picture replication | `[FR-H-6]` |

> **KDD — AI proposes, humans dispose.** Heimdall never auto-executes a consequential action; Mimir generates ranked, cited options and a human authorises. Every option and decision is audited. `[NFR-AI-1]` `[FR-H-5]`


## 6. Logical Architecture (C4 L2)

![Figure 6.1 — Heimdall containers.](../../diagrams/heimdall_c4_containers.png)
*Figure 6.1 — Heimdall containers.*


#### 6.1 Feed gateway

Ingests feeds; verifies PQ source signatures; normalises to fabric objects. `[FR-H-1]` `[FR-H-3]`


#### 6.2 Fusion engine

Correlates feeds into tracked entities; maintains the live picture. `[FR-H-1]` `[FR-H-2]`


#### 6.3 Classification service

Classifies/labels entities (rules + Mimir); writes back with provenance. `[FR-H-2]`


#### 6.4 Decision-support service

Requests ranked, cited options from Mimir; presents them advisory. `[FR-H-4]`


#### 6.5 Workflow engine

Human-gated action workflows with audit lineage. `[FR-H-5]`


#### 6.6 Picture sync

Replicates the common picture across nodes over PQ-mTLS. `[FR-H-6]`


## 7. Component Architecture (C4 L3)

![Figure 7.1 — Fusion engine components.](../../diagrams/heimdall_c4_component.png)
*Figure 7.1 — Fusion engine components.*

| Component | Responsibility |
| --- | --- |
| Correlator | Associates feed observations to entities. `[FR-H-1]` |
| Tracker | Maintains entity state over time. `[FR-H-2]` |
| Classifier adapter | Rules + Mimir classification. `[FR-H-2]` |
| Option requester | Cited, ranked options from Mimir. `[FR-H-4]` |
| Action gate | Human authorisation + audit. `[FR-H-5]` |


## 8. Data Architecture

![Figure 8.1 — Heimdall data model (feeds, entities, tracks, options, actions).](../../diagrams/heimdall_er.png)
*Figure 8.1 — Heimdall data model (feeds, entities, tracks, options, actions).*

| Entity | Purpose |
| --- | --- |
| Feed | A PQ-authenticated source. `[FR-H-3]` |
| Observation | One datum from a feed. |
| TrackedEntity | Fused entity over time. `[FR-H-2]` |
| Classification | Label + provenance. `[FR-H-2]` |
| OptionSet | Ranked, cited options from Mimir. `[FR-H-4]` |
| ActionRecord | Human-authorised action + audit. `[FR-H-5]` |


### 8.2 Data stores

| Store | Contents | Crypto |
| --- | --- | --- |
| Picture store | Tracked entities, current picture | PQ-wrapped `[NFR-PQ-2]` |
| Feed/audit store | Observations, decisions | PQ-signed, encrypted `[NFR-PQ-3]` |


## 9. Integration Architecture

| Surface | Protocol | Purpose |
| --- | --- | --- |
| Feed API/SDK | PQ-mTLS | Connect + sign feeds `[FR-H-3]` |
| Picture API | gRPC/REST | Query the common picture |
| Workflow API | gRPC/REST | Drive human-gated actions `[FR-H-5]` |
| Mimir calls | gRPC (internal) | Classification + options `[FR-H-4]` |


### 9.2 Primary sequence

![Figure 9.1 — Feed verify → fuse → classify → request options (Mimir) → human authorises → audited action.](../../diagrams/heimdall_seq_primary.png)
*Figure 9.1 — Feed verify → fuse → classify → request options (Mimir) → human authorises → audited action.*


### 9.3 User journey

![Figure 9.2 — Operations analyst primary journey.](../../diagrams/heimdall_journey.png)
*Figure 9.2 — Operations analyst primary journey.*


## 10. Infrastructure & Deployment

![Figure 10.1 — Heimdall deployment (fusion workers + picture sync).](../../diagrams/heimdall_deployment.png)
*Figure 10.1 — Heimdall deployment (fusion workers + picture sync).*

Heimdall runs streaming fusion workers and a picture store on the mesh; supports multi-node/site replication over PQ-mTLS; air-gap capable with local Mimir. `[FR-H-6]` `[NFR-PRV-1]`


### 10.2 CI/CD

![Figure 10.2 — Heimdall build/test/sign/deploy pipeline (via Bifrost).](../../diagrams/heimdall_cicd.png)
*Figure 10.2 — Heimdall build/test/sign/deploy pipeline (via Bifrost).*


## 11. Security Architecture

![Figure 11.1 — Heimdall STRIDE data-flow diagram.](../../diagrams/heimdall_threat_dfd.png)
*Figure 11.1 — Heimdall STRIDE data-flow diagram.*

| Crossing | STRIDE | Control |
| --- | --- | --- |
| Feed → Gateway | S, T | PQ source signature verify `[FR-H-3]` `[NFR-PQ-3]` |
| Fusion → Picture store | T, I | PQ at rest, marking-aware `[NFR-PQ-2]` |
| Decision → Mimir | T, R | Internal mTLS, cited/audited options `[NFR-AI-1]` |
| Workflow → Action | R, E | Human gate, policy point, audit `[FR-H-5]` |


### 11.2 Decision-safety controls

- Options are explainable and cited to feed evidence. `[FR-H-4]`
- Consequential actions require human authorisation. `[FR-H-5]`
- Full audit lineage of options presented and decisions taken. `[NFR-AI-1]`
- Feed sources PQ-authenticated to prevent spoofing. `[NFR-PQ-3]`


### 11.3 Known design gaps

| Gap | Severity | Recommendation |
| --- | --- | --- |
| Automation bias | High | Explainable cited options + mandatory human gate + training. `[NFR-AI-1]` |
| Feed spoofing | High | PQ-signed feeds; reject unverified. `[NFR-PQ-3]` |


## 12. Observability

- Fusion latency + feed-health metrics. `[NFR-AVA-1]`
- Decision audit trail. `[FR-H-5]`
- Classification accuracy monitoring. `[FR-H-2]`


## 13. Operational Concerns

- Multi-node picture replication for resilience. `[FR-H-6]`
- Degraded mode without Mimir (rules-only classification). `[DD-6]`
- Horizontal fusion scaling. `[NFR-AVA-1]`


## 14. Quality & Testing

- Fusion correctness on replay corpora. `[FR-H-1]`
- Feed-spoofing negative tests. `[FR-H-3]`
- Decision-workflow audit completeness. `[FR-H-5]`


## 15. Risks & Technical Debt

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Over-trust in AI options | High | Explainability + human gate + audit. `[NFR-AI-1]` |
| Feed integrity | High | PQ-authenticated sources. `[NFR-PQ-3]` |
| High-rate feed load | Medium | Streaming fusion + horizontal scale. `[NFR-AVA-1]` |


## 16. Roadmap

1. Feed gateway + fusion + common picture.
2. Entity tracking + classification.
3. Mimir-assisted cited options.
4. Human-gated workflows + multi-node sync.


## 17. Acceptance Criteria

| ID | Given / When / Then | Provenance |
| --- | --- | --- |
| AC-H-1 | Given multiple feeds, when ingested, then a single fused picture is produced with per-layer toggles. | `[FR-H-1]` |
| AC-H-2 | Given an unverified feed source, when it connects, then its data is rejected from the picture. | `[FR-H-3]` `[NFR-PQ-3]` |
| AC-H-3 | Given a decision point, when options are requested, then Mimir returns ranked options each citing the evidence used. | `[FR-H-4]` |
| AC-H-4 | Given a consequential action, when proposed, then it is not executed without explicit human authorisation and is fully audited. | `[FR-H-5]` `[NFR-AI-1]` |
| AC-H-5 | Given multi-node deployment, when the picture updates, then it replicates across nodes over PQ-mTLS. | `[FR-H-6]` |
| AC-H-6 | Given Mimir is absent, when operating, then fusion and rules-based classification remain fully functional (standalone mode). | `[DD-6]` |
| AC-H-7 | Given no outbound connectivity, when operating, then fusion, picture, and workflows all function (air-gap). | `[NFR-PRV-1]` |


## Appendix A — Diagram Source Index

| Diagram | Mermaid | draw.io |
| --- | --- | --- |
| Context | heimdall_c4_context.mmd | heimdall_c4_context.drawio |
| Containers | heimdall_c4_containers.mmd | heimdall_c4_containers.drawio |
| Components | heimdall_c4_component.mmd | heimdall_c4_component.drawio |
| Data model | heimdall_er.mmd | heimdall_er.drawio |
| Primary sequence | heimdall_seq_primary.mmd | heimdall_seq_primary.drawio |
| Journey | heimdall_journey.mmd | heimdall_journey.drawio |
| Deployment | heimdall_deployment.mmd | heimdall_deployment.drawio |
| CI/CD | heimdall_cicd.mmd | heimdall_cicd.drawio |
| Threat DFD | heimdall_threat_dfd.mmd | heimdall_threat_dfd.drawio |
