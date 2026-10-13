# Solutions Architecture Document — Mimir

*Local Agentic AI (GLM / Nemotron / Gemma) — a self-* , post-quantum competitor to Palantir AIP*

**Competitor benchmark:** Palantir AIP  
**Author:** Akuma Engineering Ltd. — Architecture Practice — for JB Benjamin, CEO / Founder  
**Version:** 1.0  ·  **Date:** 2026-07-02  ·  **Classification:** PUBLIC, published with the Midgard repository  
**Document type:** Forward-looking design specification (greenfield). Provenance tags: `[DD-*]` design driver, `[FR-*]/[NFR-*]` requirement, `[STD:*]` external standard.

---


## 1. Executive Summary

Mimir is the reasoning and AI product of the Midgard suite and a direct, more capable competitor to Palantir AIP. It differs from AIP in a foundational way: all generative AI, reasoning, and inference are served **locally, in-enclosure**, by GLM 5.2 — there is no dependency on any third-party hosted model. Mimir is also **agentic and self-\***: its agents self-learn from governed feedback, self-heal from failures, and self-defend against adversarial input, all within an evaluation-and-guardrail framework. `[DD-1]` `[DD-4]` `[DD-5]` `[DD-7]`

Mimir is simultaneously a **product** (the AIP competitor) and a **shared service**: the same local-inference plane powers AI features in Forge, Bifrost, and Heimdall. This is a deliberate one-model-plane, many-consumers design. `[DD-6]`

This document is a **forward-looking design specification**; every decision carries a provenance tag rather than a repository citation.

> **Self-\* defined.** *Self-learning*: agents incorporate governed feedback into retrieval and prompt/policy updates. *Self-healing*: the runtime detects degraded inference or failed tools and re-routes, retries, or rolls back. *Self-defending*: inputs and tool calls pass through guardrail and prompt-injection defences before execution. Every self-\* action is audited, evaluated, and reversible or human-gated where it mutates state. `[DD-5]` `[NFR-AI-1]`


### 1.1 Top design facts

- **GLM 5.2 served locally** behind a model-serving runtime with batching and KV-cache; no hosted-LLM dependency. `[DD-4]`
- **Ontology-Augmented Generation (OAG):** the model is grounded in the Midgard object fabric's data, logic, and actions. `[DD-1]`
- **Agent runtime** with tools, memory, planning, and an Evals harness governing every agent. `[DD-5]` `[NFR-AI-1]`
- **No-code + pro-code builders** (Logic / Agent Studio equivalents) for AI functions and agents. `[FR-MI-4]`
- **PQ-native** prompts, memory, and model artefacts encrypted; weights PQ-signed. `[NFR-PQ-2]` `[NFR-PQ-3]`


### 1.2 Top design risks

- **Autonomy vs. control:** agentic self-\* behaviour must never bypass governance; mitigated by mandatory Evals + human-gating on state mutation. `[NFR-AI-1]`
- **Local inference cost:** GPU capacity constrains throughput; mitigated by batching, quantisation, and horizontal GPU scaling. `[NFR-PERF-1]`
- **Prompt-injection / model exfiltration:** mitigated by the self-defending guardrail layer and strict tool-scoping. `[DD-5]`


## 2. Introduction & Scope


### 2.1 Purpose

To define Mimir's target architecture as both the AIP-competitor product and the shared local-inference layer for the suite: model serving, OAG grounding, the agent runtime, the self-\* control loops, and the governance/Evals framework.


### 2.2 Audience

- AI/ML engineers and agent authors.
- Governance and safety reviewers.
- Product teams consuming Mimir inference.


### 2.3 Scope

| In scope | Out of scope |
| --- | --- |
| Model serving, OAG, agent runtime, self-* loops, Evals, guardrails | Object fabric internals (Midgard SAD) |
| Mimir builders (Logic / Agent Studio equivalents), MCP tools | Data pipeline internals (Forge SAD) |


### 2.4 Definitions

| Term | Meaning |
| --- | --- |
| Mimir | Reasoning/AI product; competitor to Palantir AIP. Local, agentic, self-* generative AI on GLM 5.2. Also the shared inference layer for the other products. |
| GLM 5.2 | A large language model served locally by Mimir. As of the model-supply resolution, Mimir supports the most advanced locally-runnable variants of three families — GLM, NVIDIA Nemotron, and Google Gemma — selectable per workflow. |
| Object fabric | Midgard's semantic + kinetic data layer: objects, properties, links (semantics) and actions, functions, dynamic security (kinetics). |
| OAG | Ontology-Augmented Generation: grounding the LLM in the object fabric's data, logic, and actions. |
| Enclosure | The Midgard trust boundary; the defended, sovereign space within which all products run. |


## 3. Business & Solution Context


### 3.1 Competitive positioning

| Capability | Palantir AIP | Mimir (design intent) |
| --- | --- | --- |
| Model access | k-LLM, third-party hosted models | Local GLM 5.2, in-enclosure only `[DD-4]` |
| Grounding | Ontology-augmented | OAG on Midgard object fabric `[DD-1]` |
| Agents | Agent Studio, Logic, Evals | Agent runtime + self-* loops + Evals `[DD-5]` |
| Autonomy | Human-in-the-loop | Agentic self-learn/heal/defend, governed `[DD-5]` |
| Sovereignty | Cloud-mediated | Air-gap capable, PQ-native `[DD-7]` `[NFR-PQ-2]` |


### 3.2 System context (C4 L1)

![Figure 3.1 — Mimir as product and shared inference service within the enclosure.](../../diagrams/mimir_c4_context.png)
*Figure 3.1 — Mimir as product and shared inference service within the enclosure.*


## 4. Architecture Drivers


### 4.1 Functional requirements

| ID | Requirement |
| --- | --- |
| FR-MI-1 | Serve models locally with batching, KV-cache, and quantisation; expose a typed inference API. Supported families: GLM, NVIDIA Nemotron, Google Gemma (see the Model-Supply Resolution DCR). |
| FR-MI-2 | Ground generation in the object fabric (OAG): retrieve objects/logic/actions with citations. |
| FR-MI-3 | Provide an agent runtime with tools, memory, planning, and per-agent Evals. |
| FR-MI-4 | Provide no-code (Logic) and pro-code builders for AI functions and agents. |
| FR-MI-5 | Self-learning: incorporate governed feedback into retrieval and prompt/policy state. |
| FR-MI-6 | Self-healing: detect degraded inference / failed tools and re-route, retry, or roll back. |
| FR-MI-7 | Self-defending: filter prompt-injection and adversarial input; scope tools; block exfiltration. |
| FR-MI-8 | Expose inference to Forge, Bifrost, and Heimdall over internal PQ-mTLS. |


### 4.2 Inherited NFRs

| ID | Requirement | Provenance |
| --- | --- | --- |
| NFR-PQ-2 | All data-at-rest is encrypted with AES-256-GCM under keys wrapped by ML-KEM-1024; key material is sealed to an HSM/TPM where present. | STD:NIST FIPS 203 / FIPS 197 |
| NFR-PQ-3 | All service-to-service and artefact-signing operations use ML-DSA (Dilithium) or SLH-DSA (SPHINCS+) signatures. | STD:NIST FIPS 204 / FIPS 205 |
| NFR-SEC-1 | Zero-trust: every request between components is mutually authenticated and authorised on identity, device posture, and purpose. | STD:NIST SP 800-207 |
| NFR-PRV-1 | The suite runs fully offline / air-gapped with no outbound network dependency for any core function. | DD-7 |
| NFR-PERF-1 | Local inference first-token latency target < 800 ms on reference hardware (1×80GB GPU class); batch throughput scales horizontally. | DD-4 |
| NFR-AI-1 | Every agentic action is traceable, evaluated against an Evals suite, and reversible or human-gated where it mutates state. | DD-5 |


## 5. Solution Strategy

| Concern | Decision | Provenance |
| --- | --- | --- |
| Serving | Dedicated inference runtime on GPU pool; continuous batching | `[FR-MI-1]` `[NFR-PERF-1]` |
| Grounding | OAG retriever over object fabric with citation binding | `[FR-MI-2]` `[DD-1]` |
| Agency | Plan–act–observe loop with typed tools + memory store | `[FR-MI-3]` |
| Governance | Mandatory Evals + guardrails on every agent action | `[NFR-AI-1]` |
| Self-* | Closed control loops around learn/heal/defend | `[DD-5]` |

> **KDD — self-\* is bounded autonomy, not unbounded.** Every self-learning update, self-healing reroute, and self-defending block is (a) logged with provenance, (b) checked against the Evals suite, and (c) reversible; state-mutating actions require human gating unless explicitly pre-authorised for a narrow, evaluated scope. `[NFR-AI-1]` `[DD-5]`


## 6. Logical Architecture (C4 L2)

![Figure 6.1 — Mimir containers.](../../diagrams/mimir_c4_containers.png)
*Figure 6.1 — Mimir containers.*


#### 6.1 Inference runtime

Serves GLM 5.2 with continuous batching, KV-cache, quantisation; exposes typed completion/embedding APIs; scales across the GPU pool. `[FR-MI-1]` `[NFR-PERF-1]`


#### 6.2 OAG retriever

Retrieves grounding context (objects, links, functions, actions) from the fabric and binds citations to generated content. `[FR-MI-2]` `[DD-1]`


#### 6.3 Agent runtime

Executes plan–act–observe loops with typed tools (fabric queries, actions, functions, MCP), a memory store, and per-step Evals. `[FR-MI-3]`


#### 6.4 Guardrail & defence layer

Screens inputs and tool calls for prompt-injection, policy violations, and exfiltration attempts before execution. `[FR-MI-7]`


#### 6.5 Evals & feedback service

Runs test cases, scores agent behaviour, and feeds governed feedback into self-learning. `[FR-MI-5]` `[NFR-AI-1]`


#### 6.6 Builders (Logic / Agent Studio)

No-code and pro-code authoring of AI functions and agents. `[FR-MI-4]`


## 7. Component Architecture (C4 L3)

![Figure 7.1 — Agent runtime components.](../../diagrams/mimir_c4_component.png)
*Figure 7.1 — Agent runtime components.*


### 7.1 The self-\* control loops

The self-learning, self-healing, and self-defending behaviours are implemented as explicit closed control loops around the agent runtime, each observable, evaluated, and reversible.

![Figure 7.2 — Self-learning / self-healing / self-defending control loops.](../../diagrams/mimir_self_loop.png)
*Figure 7.2 — Self-learning / self-healing / self-defending control loops.*

| Loop | Sensor | Controller | Actuator | Guard |
| --- | --- | --- | --- | --- |
| Self-learning | User/Evals feedback | Feedback service | Retrieval + prompt/policy update | Evals gate `[FR-MI-5]` |
| Self-healing | Health/latency/error signals | Runtime supervisor | Reroute / retry / rollback | Circuit breaker `[FR-MI-6]` |
| Self-defending | Input + tool-call inspection | Guardrail layer | Block / sanitise / quarantine | Policy point `[FR-MI-7]` |


## 8. Data Architecture

![Figure 8.1 — Mimir data model (agents, tools, sessions, memory, evals, feedback).](../../diagrams/mimir_er.png)
*Figure 8.1 — Mimir data model (agents, tools, sessions, memory, evals, feedback).*

| Entity | Purpose |
| --- | --- |
| Agent | Definition: prompt, tools, policies, model binding. |
| Tool | Typed capability (fabric query, action, function, MCP). |
| Session | One agent execution with plan/steps. |
| MemoryRecord | Retrievable agent memory (PQ-encrypted). `[NFR-PQ-2]` |
| EvalCase / EvalRun | Test case and its scored result. `[NFR-AI-1]` |
| FeedbackRecord | Governed feedback for self-learning. `[FR-MI-5]` |
| GuardrailEvent | A self-defending block/sanitise event. `[FR-MI-7]` |


### 8.2 Data stores

| Store | Contents | Crypto |
| --- | --- | --- |
| Model store | GLM 5.2 weights + adapters | PQ-signed, encrypted `[NFR-PQ-3]` |
| Vector/memory store | Embeddings, agent memory | PQ-wrapped `[NFR-PQ-2]` |
| Evals store | Cases, runs, scores | PQ-wrapped |


## 9. Integration Architecture

| Surface | Protocol | Purpose |
| --- | --- | --- |
| Inference API | gRPC over PQ-mTLS | Completions/embeddings for the suite `[FR-MI-8]` |
| Agent API | gRPC/REST | Invoke/monitor agents |
| Logic/Function API | gRPC | Run AI-backed functions (called by Forge/Heimdall) |
| MCP tools | MCP over PQ-mTLS | Governed tool access to the fabric `[DD-4]` |


### 9.2 Primary sequence

![Figure 9.1 — OAG-grounded agent turn: retrieve → plan → guard → act → evaluate → respond.](../../diagrams/mimir_seq_primary.png)
*Figure 9.1 — OAG-grounded agent turn: retrieve → plan → guard → act → evaluate → respond.*


### 9.3 User journey

![Figure 9.2 — Agent author primary journey.](../../diagrams/mimir_journey.png)
*Figure 9.2 — Agent author primary journey.*


## 10. Infrastructure & Deployment

![Figure 10.1 — Mimir deployment (GPU inference pool + agent/eval workers).](../../diagrams/mimir_deployment.png)
*Figure 10.1 — Mimir deployment (GPU inference pool + agent/eval workers).*

Inference runs on a GPU node pool; agent, guardrail, and eval services are horizontally scalable CPU workloads; the model store is PQ-signed and verified on load. `[NFR-PERF-1]` `[NFR-PQ-3]`


### 10.2 CI/CD

![Figure 10.2 — Mimir pipeline incl. model-artefact signing and Evals gate.](../../diagrams/product_cicd.png)
*Figure 10.2 — Mimir pipeline incl. model-artefact signing and Evals gate.*


## 11. Security Architecture

![Figure 11.1 — Mimir STRIDE data-flow diagram.](../../diagrams/mimir_threat_dfd.png)
*Figure 11.1 — Mimir STRIDE data-flow diagram.*

| Crossing | STRIDE | Control |
| --- | --- | --- |
| Consumer → Inference API | S, T, D | PQ-mTLS, workload identity, quotas `[NFR-SEC-1]` |
| Prompt → Agent runtime | T, E | Guardrail/injection defence, tool-scoping `[FR-MI-7]` |
| Agent → Fabric action | R, E | Policy point, human-gate, audit `[NFR-AI-1]` |
| Model store → Runtime | T | PQ signature verification on load `[NFR-PQ-3]` |


### 11.2 AI-specific security

- Prompt-injection and jailbreak filtering at input and tool-call boundaries. `[FR-MI-7]`
- Tool-scoping: agents receive least-privilege, purpose-bound tool grants. `[NFR-SEC-2]`
- Exfiltration defence: output scanning + marking-aware redaction. `[FR-MI-7]`
- Model-weight integrity: PQ-signed, verified on load. `[NFR-PQ-3]`


### 11.3 Known design gaps

| Gap | Severity | Recommendation |
| --- | --- | --- |
| Novel jailbreaks evade static guards | Medium | Continuous red-team Evals feeding self-defending loop. `[DD-5]` |
| Autonomous action blast radius | High | Default human-gate on state mutation; narrow pre-authorised scopes only. `[NFR-AI-1]` |


## 12. Observability

- Per-agent traces (plan/act/observe). `[FR-MI-3]`
- Inference latency/throughput SLOs. `[NFR-PERF-1]`
- Guardrail + eval event metrics. `[FR-MI-7]`


## 13. Operational Concerns

- Model artefact backup + signature registry. `[NFR-PQ-3]`
- Graceful degradation: consumers fall back to non-AI mode if inference is down. `[DD-6]`
- GPU autoscaling for load. `[NFR-PERF-1]`


## 14. Quality & Testing

- Evals suite as executable acceptance for agent behaviour. `[NFR-AI-1]`
- Red-team corpus for self-defending. `[FR-MI-7]`
- Regression on OAG citation accuracy. `[FR-MI-2]`


## 15. Risks & Technical Debt

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Autonomy overreach | High | Bounded autonomy: Evals + human-gate + reversibility. `[NFR-AI-1]` |
| Inference capacity | Medium | Batching, quantisation, horizontal GPU scale. `[NFR-PERF-1]` |
| Feedback poisoning of self-learning | Medium | Governed, provenance-checked feedback only. `[FR-MI-5]` |


## 16. Roadmap

1. Local GLM 5.2 serving + inference API.
2. OAG grounding + citations.
3. Agent runtime + Evals.
4. Self-* control loops (learn/heal/defend).


## 17. Acceptance Criteria

| ID | Given / When / Then | Provenance |
| --- | --- | --- |
| AC-MI-1 | Given no outbound connectivity, when inference is requested, then GLM 5.2 responds locally (no hosted-model dependency). | `[DD-4]` `[NFR-PRV-1]` |
| AC-MI-2 | Given an OAG query, when answered, then the response cites the object-fabric evidence it used. | `[FR-MI-2]` |
| AC-MI-3 | Given an agent action that mutates state, when executed, then it is Evals-checked and human-gated unless pre-authorised for an evaluated scope. | `[NFR-AI-1]` |
| AC-MI-4 | Given governed feedback, when applied, then retrieval/policy updates occur and are logged with provenance (self-learning). | `[FR-MI-5]` |
| AC-MI-5 | Given degraded inference or a failed tool, when detected, then the runtime reroutes/retries/rolls back automatically (self-healing). | `[FR-MI-6]` |
| AC-MI-6 | Given a prompt-injection attempt, when received, then it is detected and blocked before any tool executes (self-defending). | `[FR-MI-7]` |
| AC-MI-7 | Given model weights of any supported family (GLM/Nemotron/Gemma), when loaded, then the Akuma ML-DSA signature, licence policy, and hardware eligibility are verified and loading fails closed on any mismatch. (Superseded/expanded by AC-MS-2 in the Model-Supply Resolution.) | `[NFR-PQ-3]` |
| AC-MI-8 | Given a consuming product (Forge/Bifrost/Heimdall), when it calls inference, then the call is over internal PQ-mTLS and audited. | `[FR-MI-8]` `[NFR-SEC-1]` |


## Appendix A — Diagram Source Index

| Diagram | Mermaid | draw.io |
| --- | --- | --- |
| Context | mimir_c4_context.mmd | mimir_c4_context.drawio |
| Containers | mimir_c4_containers.mmd | mimir_c4_containers.drawio |
| Components | mimir_c4_component.mmd | mimir_c4_component.drawio |
| Self-* loops | mimir_self_loop.mmd | mimir_self_loop.drawio |
| Data model | mimir_er.mmd | mimir_er.drawio |
| Primary sequence | mimir_seq_primary.mmd | mimir_seq_primary.drawio |
| Journey | mimir_journey.mmd | mimir_journey.drawio |
| Deployment | mimir_deployment.mmd | mimir_deployment.drawio |
| CI/CD | product_cicd.mmd | product_cicd.drawio |
| Threat DFD | mimir_threat_dfd.mmd | mimir_threat_dfd.drawio |
