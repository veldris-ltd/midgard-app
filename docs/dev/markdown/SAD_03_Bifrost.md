# Solutions Architecture Document — Bifrost

*Continuous Delivery — a post-quantum, AI-assisted continuous-delivery platform*

**Author:** Veldris Ltd (company no. 17366869)  
**Version:** 1.0  ·  **Date:** 2026-07-02  ·  **Classification:** PUBLIC, published with the Midgard repository  
**Document type:** Forward-looking design specification (greenfield). Provenance tags: `[DD-*]` design driver, `[FR-*]/[NFR-*]` requirement, `[STD:*]` external standard.

---


## 1. Executive Summary

Bifrost is the continuous-delivery product of the Midgard suite. It is the bridge across the enclosure boundary: it orchestrates zero-downtime deployment, release management, and fleet upgrades for every product in the suite. Bifrost goes beyond established delivery platforms with native post-quantum artefact signing, air-gap-native release bundles, and AI-assisted release intelligence served by the local Mimir layer rather than a hosted model. `[DD-1]` `[DD-3]` `[DD-4]` `[DD-7]`

This document is a **forward-looking design specification**; every decision carries a provenance tag rather than a repository citation.

> **Standalone and composable.** Midgard + Bifrost delivers CD for the platform itself. With Mimir, Bifrost gains release-risk analysis and anomaly detection; it is the mechanism by which Forge, Mimir, and Heimdall are themselves promoted and upgraded. `[DD-6]`


### 1.1 Top design facts

- **Zero-downtime rolling upgrades** with automatic health-gated rollback across the fleet. `[FR-B-2]` `[NFR-AVA-1]`
- **PQ-signed artefacts + SBOM attestation** verified before every deploy. `[NFR-PQ-3]` `[NFR-OSS-1]`
- **Air-gap-native bundles:** signed offline release bundles for disconnected sites. `[FR-B-5]` `[DD-7]`
- **Declarative release channels** and fleet/installation management. `[FR-B-3]`
- **AI release intelligence** (risk scoring, anomaly detection) via local Mimir. `[FR-B-6]` `[DD-4]`


### 1.2 Top design risks

- **Bad release propagation:** a faulty release could roll out widely; mitigated by staged rings + automatic rollback. `[FR-B-2]`
- **Supply-chain trust:** artefacts must be provably built and signed; mitigated by reproducible builds + PQ signatures. `[NFR-OSS-1]` `[NFR-PQ-3]`
- **Air-gap update lag:** disconnected sites update slowly; mitigated by signed bundle workflow. `[FR-B-5]`


## 2. Introduction & Scope


### 2.1 Purpose

To define Bifrost's target architecture: artefact management, release orchestration, fleet/upgrade management, rollback, and AI-assisted release intelligence, plus dependencies on Midgard core and optional Mimir.


### 2.2 Audience

- Release/platform engineers.
- SREs operating the fleet.
- Security reviewers of the software supply chain.


### 2.3 Scope

| In scope | Out of scope |
| --- | --- |
| Artefacts, releases, rings, rollout/rollback, fleet mgmt, release intelligence | Object fabric internals (Midgard SAD) |
| Bifrost APIs/UI and offline bundles | Model serving internals (Mimir SAD) |


### 2.4 Definitions

| Term | Meaning |
| --- | --- |
| Mimir | Reasoning/AI product. Local, agentic, self-* generative AI on Mistral Small 3.2. Also the shared inference layer for the other products. |
| Bifrost | Continuous-delivery product. Orchestrates zero-downtime deployment across the suite. |
| ML-DSA | Module-Lattice Digital Signature Algorithm (Dilithium), NIST FIPS 204. |
| Enclosure | The Midgard trust boundary; the defended, sovereign space within which all products run. |


## 3. Business & Solution Context


### 3.1 Competitive positioning

| Capability | Established platforms | Bifrost (design intent) |
| --- | --- | --- |
| Delivery | Continuous, zero-downtime | Continuous, zero-downtime + PQ-signed `[NFR-PQ-3]` |
| Supply chain | Managed signing | Reproducible build + PQ signature + SBOM `[NFR-OSS-1]` |
| Air-gap | Cloud-mediated | Native signed offline bundles `[FR-B-5]` |
| Intelligence | Rules-based | Local Mimir risk/anomaly analysis `[FR-B-6]` |


### 3.2 System context (C4 L1)

![Figure 3.1 — Bifrost system context within the enclosure.](../../diagrams/bifrost_c4_context.png)
*Figure 3.1 — Bifrost system context within the enclosure.*


## 4. Architecture Drivers


### 4.1 Functional requirements

| ID | Requirement |
| --- | --- |
| FR-B-1 | Manage signed, versioned deployable artefacts with SBOM attestation. |
| FR-B-2 | Perform zero-downtime rolling upgrades with automatic health-gated rollback. |
| FR-B-3 | Provide declarative release channels and fleet/installation management. |
| FR-B-4 | Verify PQ signatures and SBOM before any deploy; fail closed on mismatch. |
| FR-B-5 | Produce and consume signed offline release bundles for air-gapped sites. |
| FR-B-6 | Provide AI-assisted release intelligence (risk scoring, anomaly detection) via Mimir. |


### 4.2 Inherited NFRs

| ID | Requirement | Provenance |
| --- | --- | --- |
| NFR-PQ-3 | All service-to-service and artefact-signing operations use ML-DSA (Dilithium) or SLH-DSA (SPHINCS+) signatures. | STD:NIST FIPS 204 / FIPS 205 |
| NFR-SEC-1 | Zero-trust: every request between components is mutually authenticated and authorised on identity, device posture, and purpose. | STD:NIST SP 800-207 |
| NFR-AVA-1 | Core control-plane services target 99.9% availability; the platform supports zero-downtime rolling upgrades. | DD-1 |
| NFR-PRV-1 | The suite runs fully offline / air-gapped with no outbound network dependency for any core function. | DD-7 |
| NFR-OSS-1 | All core components build reproducibly from source; an SBOM (CycloneDX) is emitted per release. | DD-2 |
| NFR-AI-1 | Every agentic action is traceable, evaluated against an Evals suite, and reversible or human-gated where it mutates state. | DD-5 |


## 5. Solution Strategy

| Concern | Decision | Provenance |
| --- | --- | --- |
| Rollout | Staged rings (canary→broad) with health gates | `[FR-B-2]` |
| Trust | Reproducible build + ML-DSA signature + SBOM verify | `[NFR-PQ-3]` `[NFR-OSS-1]` |
| Air-gap | Signed bundle export/import workflow | `[FR-B-5]` |
| Intelligence | Mimir functions score release risk; advisory + gate | `[FR-B-6]` `[NFR-AI-1]` |

> **KDD — deploys fail closed on trust.** No artefact is deployed unless its PQ signature verifies and its SBOM matches; unknown or tampered artefacts are rejected automatically. `[FR-B-4]` `[NFR-PQ-3]`


## 6. Logical Architecture (C4 L2)

![Figure 6.1 — Bifrost containers.](../../diagrams/bifrost_c4_containers.png)
*Figure 6.1 — Bifrost containers.*


#### 6.1 Artefact registry

Stores signed, versioned artefacts + SBOMs; verifies signatures on ingest and retrieval. `[FR-B-1]` `[NFR-PQ-3]`


#### 6.2 Release orchestrator

Executes ring-based rollouts with health gates and automatic rollback. `[FR-B-2]`


#### 6.3 Fleet manager

Tracks installations, channels, and desired vs actual state across the fleet. `[FR-B-3]`


#### 6.4 Bundle service

Builds/imports signed offline release bundles for air-gapped sites. `[FR-B-5]`


#### 6.5 Release-intelligence adapter

Calls Mimir for risk scoring and anomaly detection; advisory or gating. `[FR-B-6]`


## 7. Component Architecture (C4 L3)

![Figure 7.1 — Release orchestrator components.](../../diagrams/bifrost_c4_component.png)
*Figure 7.1 — Release orchestrator components.*

| Component | Responsibility |
| --- | --- |
| Ring planner | Computes canary→broad rollout plan. `[FR-B-2]` |
| Health gate | Evaluates SLO/health signals between rings. `[NFR-AVA-1]` |
| Rollback controller | Reverts on gate failure. `[FR-B-2]` |
| Signature verifier | PQ signature + SBOM check. `[FR-B-4]` |
| State reconciler | Drives fleet to desired state. `[FR-B-3]` |


## 8. Data Architecture

![Figure 8.1 — Bifrost data model (artefacts, releases, rings, installations).](../../diagrams/bifrost_er.png)
*Figure 8.1 — Bifrost data model (artefacts, releases, rings, installations).*

| Entity | Purpose |
| --- | --- |
| Artefact | Signed deployable + SBOM ref. `[FR-B-1]` |
| Release | A versioned set of artefacts for a channel. |
| Channel | Named release stream (e.g. stable, edge). `[FR-B-3]` |
| Ring | Rollout stage with a health gate. `[FR-B-2]` |
| Installation | One deployed instance in the fleet. |
| RolloutRun | One orchestrated release execution. |
| Bundle | Signed offline release package. `[FR-B-5]` |


### 8.2 Data stores

| Store | Contents | Crypto |
| --- | --- | --- |
| Artefact registry | Artefacts, SBOMs, signatures | ML-DSA-signed, encrypted `[NFR-PQ-3]` |
| Fleet state store | Installations, desired state | PQ-wrapped `[NFR-PQ-2]` |


## 9. Integration Architecture

| Surface | Protocol | Purpose |
| --- | --- | --- |
| Release API | gRPC/REST over PQ-mTLS | Define/trigger releases |
| Fleet API | gRPC/REST | Query/manage installations |
| Agent (on-host) | PQ-mTLS | Applies changes on each node |
| Mimir calls | gRPC (internal) | Release risk/anomaly `[FR-B-6]` |


### 9.2 Primary sequence

![Figure 9.1 — Verify signature → canary ring → health gate → broad rollout → auto-rollback on failure.](../../diagrams/bifrost_seq_primary.png)
*Figure 9.1 — Verify signature → canary ring → health gate → broad rollout → auto-rollback on failure.*


### 9.3 User journey

![Figure 9.2 — Release engineer primary journey.](../../diagrams/bifrost_journey.png)
*Figure 9.2 — Release engineer primary journey.*


## 10. Infrastructure & Deployment

![Figure 10.1 — Bifrost deployment (control plane + on-host agents).](../../diagrams/bifrost_deployment.png)
*Figure 10.1 — Bifrost deployment (control plane + on-host agents).*

Bifrost runs a control plane in the enclosure and a PQ-authenticated agent on each managed node; it deploys itself and every other product; egress-deny with signed bundle import for air-gap. `[NFR-SEC-1]` `[FR-B-5]`


### 10.2 CI/CD

![Figure 10.2 — Bifrost's own build/sign/publish pipeline.](../../diagrams/product_cicd.png)
*Figure 10.2 — Bifrost's own build/sign/publish pipeline.*


## 11. Security Architecture

![Figure 11.1 — Bifrost STRIDE data-flow diagram.](../../diagrams/bifrost_threat_dfd.png)
*Figure 11.1 — Bifrost STRIDE data-flow diagram.*

| Crossing | STRIDE | Control |
| --- | --- | --- |
| CI → Artefact registry | T, R | Reproducible build, PQ signature, audit `[NFR-PQ-3]` |
| Registry → Orchestrator | T, S | Signature verify, workload identity `[FR-B-4]` |
| Orchestrator → On-host agent | S, T, E | PQ-mTLS, least-privilege agent `[NFR-SEC-1]` |
| Bundle import (air-gap) | T, S | Signed bundle, offline verify `[FR-B-5]` |


### 11.2 Supply-chain security

- Reproducible builds; PQ-signed artefacts; SBOM per release. `[NFR-OSS-1]` `[NFR-PQ-3]`
- Fail-closed signature/SBOM verification before deploy. `[FR-B-4]`
- Least-privilege on-host agents with signed instructions. `[NFR-SEC-1]`


### 11.3 Known design gaps

| Gap | Severity | Recommendation |
| --- | --- | --- |
| Compromised signing key | High | HSM-sealed keys, rotation, threshold signing. `[NFR-PQ-2]` |
| Canary blind spots | Medium | Mimir anomaly detection augmenting health gates. `[FR-B-6]` |


## 12. Observability

- Rollout progress + ring health metrics. `[FR-B-2]`
- Fleet drift dashboards. `[FR-B-3]`
- Signature-verification audit. `[NFR-PQ-3]`


## 13. Operational Concerns

- Automatic rollback is the primary recovery path. `[FR-B-2]`
- Offline bundle workflow for DR at air-gapped sites. `[FR-B-5]`
- Control-plane HA. `[NFR-AVA-1]`


## 14. Quality & Testing

- Chaos/upgrade tests proving zero-downtime + rollback. `[NFR-AVA-1]`
- Signature-verification negative tests (tampered artefact rejected). `[FR-B-4]`
- Contract tests vs Midgard/Mimir. `[DD-6]`


## 15. Risks & Technical Debt

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Faulty release fan-out | High | Staged rings + auto-rollback + Mimir risk gate. `[FR-B-2]` `[FR-B-6]` |
| Signing-key compromise | High | HSM sealing + rotation + threshold signatures. `[NFR-PQ-2]` |


## 16. Roadmap

1. Artefact registry + PQ signing.
2. Ring-based rollout + rollback.
3. Fleet/channel management.
4. Offline bundles + Mimir release intelligence.


## 17. Acceptance Criteria

| ID | Given / When / Then | Provenance |
| --- | --- | --- |
| AC-B-1 | Given an unsigned or tampered artefact, when a deploy is attempted, then it is rejected (fail closed). | `[FR-B-4]` `[NFR-PQ-3]` |
| AC-B-2 | Given a rollout, when a ring's health gate fails, then rollback occurs automatically with no downtime. | `[FR-B-2]` `[NFR-AVA-1]` |
| AC-B-3 | Given an air-gapped site, when a signed bundle is imported, then its signature is verified offline before deploy. | `[FR-B-5]` |
| AC-B-4 | Given a release, when published, then a CycloneDX SBOM and ML-DSA signature are attached and verifiable. | `[NFR-OSS-1]` `[NFR-PQ-3]` |
| AC-B-5 | Given Mimir is present, when a release is planned, then a risk score is produced and can gate the rollout. | `[FR-B-6]` |
| AC-B-6 | Given Mimir is absent, when releasing, then rule-based gates still enforce safe rollout (standalone mode). | `[DD-6]` |


## Appendix A — Diagram Source Index

| Diagram | Mermaid | draw.io |
| --- | --- | --- |
| Context | bifrost_c4_context.mmd | bifrost_c4_context.drawio |
| Containers | bifrost_c4_containers.mmd | bifrost_c4_containers.drawio |
| Components | bifrost_c4_component.mmd | bifrost_c4_component.drawio |
| Data model | bifrost_er.mmd | bifrost_er.drawio |
| Primary sequence | bifrost_seq_primary.mmd | bifrost_seq_primary.drawio |
| Journey | bifrost_journey.mmd | bifrost_journey.drawio |
| Deployment | bifrost_deployment.mmd | bifrost_deployment.drawio |
| CI/CD | product_cicd.mmd | product_cicd.drawio |
| Threat DFD | bifrost_threat_dfd.mmd | bifrost_threat_dfd.drawio |
