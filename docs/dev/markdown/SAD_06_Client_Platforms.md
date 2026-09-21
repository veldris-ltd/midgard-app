# Midgard Suite — Client Platforms

*Native mobile (Android/iOS/Linux), desktop (Windows/macOS/Linux) & web (browser + extension)*

**Competitor benchmark:** Palantir client applications  
**Author:** Akuma Engineering Ltd. — Architecture Practice — for JB Benjamin, CEO / Founder  
**Version:** 1.0 · **Date:** 2026-07-02 · **Classification:** CONFIDENTIAL — RECIPIENT EYES ONLY  
**Document type:** Forward-looking design specification (greenfield). Provenance: `[DD-*]`, `[FR-*]/[NFR-*]`, `[STD:*]`.

---


## 1. Executive Summary

This document specifies the client layer of the Midgard suite: the native and web applications through which users reach Forge, Mimir, Bifrost, and Heimdall. It covers seven delivery targets across three families — **mobile** (Android, iOS, Linux), **desktop** (Windows, macOS, Linux), and **web** (an in-browser application and a browser extension). The client layer is deliberately thin in policy but rich in experience: all governance, markings, and authoritative data live in the enclosure, while the clients provide native, offline-capable, post-quantum-secured access to it. `[DD-1]` `[DD-3]` `[DD-6]` `[DD-7]`

This is a **forward-looking design specification** for a greenfield system; every decision carries a provenance tag (`[DD-n]`, `[FR-n]`, `[NFR-n]`, `[STD:...]`) rather than a repository citation.

> **The architectural spine.** A single shared **Rust core** (`midgard-client-core`) is compiled to every target and wrapped in a platform-native UI shell — SwiftUI on Apple platforms, Kotlin/Compose on Android, GTK on Linux, Tauri on desktop, and WASM in the browser. Cryptography, sync, local storage, and the inference client are written once in the core; only the presentation layer is per-platform. This is the same shared-core discipline used across the wider Akuma portfolio. `[DD-6]`


### 1.1 Top design facts

- **One core, many shells:** cryptography, CRDT sync, encrypted local storage, and the Fabric/Mimir client are implemented once in Rust and bound via UniFFI (Swift/Kotlin) and wasm-bindgen (web). `[DD-6]` `[FR-C-1]`
- **Offline-first:** every client works disconnected; edits are local CRDT operations that merge conflict-free on reconnect. `[FR-C-4]` `[NFR-PRV-1]`
- **Post-quantum on device:** local data sealed with AES-256-GCM under ML-KEM-wrapped keys held in the platform keystore; all transport is hybrid PQ-mTLS. `[NFR-PQ-1]` `[NFR-PQ-2]`
- **Tiered local AI:** capable devices run a small on-device model; others delegate to enclosure Mimir over PQ-mTLS; the choice is automatic and policy-bound. `[DD-4]` `[FR-C-6]`
- **Signed distribution:** every artefact is reproducibly built and PQ-signed by Bifrost before it reaches a store or device. `[NFR-OSS-1]` `[NFR-PQ-3]`


### 1.2 Top design risks

- **Platform key-custody variance:** iOS Keychain, Android Keystore, TPM, and browser storage differ in guarantees; mitigated by a keystore abstraction with per-platform minimum-security floors. `[FR-C-2]`
- **Extension attack surface:** a browser extension is the most exposed surface; mitigated by a minimal-privilege bridge that never holds raw key material and delegates crypto to the enclosure or the companion app. `[FR-C-8]` `[NFR-SEC-1]`
- **On-device model footprint:** the smallest capable model must fit constrained devices; mitigated by int4 quantised variants and graceful feature disablement. `[FR-C-6]` `[NFR-PERF-1]`


## 2. Introduction & Scope


### 2.1 Purpose

To define the target architecture of the Midgard client layer across all seven delivery targets, the shared-core strategy that unifies them, and their contracts with the enclosure (Gateway, Object Fabric, Mimir).


### 2.2 Audience

- Mobile, desktop, and web engineers.
- Security reviewers of the device trust boundary.
- Release engineers managing store distribution.


### 2.3 Scope

| In scope | Out of scope |
| --- | --- |
| Shared Rust core; per-platform shells; offline sync; on-device crypto; local inference tiering; distribution | Enclosure internals (Midgard core SAD) |
| Client-side prompts and AI UX | Model serving internals (Mimir SAD) |
| Browser app + extension | Product-specific business logic (product SADs) |


### 2.4 Definitions

| Term | Meaning |
| --- | --- |
| Mimir | Reasoning/AI product; competitor to Palantir AIP. Local, agentic, self-* generative AI on GLM 5.2. Also the shared inference layer for the other products. |
| Object fabric | Midgard's semantic + kinetic data layer: objects, properties, links (semantics) and actions, functions, dynamic security (kinetics). |
| OAG | Ontology-Augmented Generation: grounding the LLM in the object fabric's data, logic, and actions. |
| Seal | The Kryotech post-quantum encryption action applied to objects and artefacts. |
| Enclosure | The Midgard trust boundary; the defended, sovereign space within which all products run. |
| midgard-client-core | The shared Rust library compiled to every client target. |
| UI shell | The thin, platform-native presentation layer over the shared core. |
| Inference tier | The automatic choice between on-device, remote, or hybrid AI. |


## 3. Business & Solution Context


### 3.1 Delivery targets

| Family | Target | Shell technology | Distribution |
| --- | --- | --- | --- |
| Mobile | Android | Kotlin / Jetpack Compose | Google Play + sideload/APK |
| Mobile | iOS | Swift / SwiftUI | App Store |
| Mobile | Linux (mobile) | Rust / GTK (mobile profile) | Flatpak / distro repos |
| Desktop | Windows | Tauri + SvelteKit | MSI / Microsoft Store |
| Desktop | macOS | Tauri + SvelteKit (or SwiftUI) | DMG (notarised) / App Store |
| Desktop | Linux (desktop) | Tauri + SvelteKit | Flatpak / AUR / AppImage |
| Web | Browser app | TypeScript + WASM | Enclosure-hosted |
| Web | Browser extension | TypeScript + WASM (minimal) | Chrome / Firefox stores |


### 3.2 System context (C4 L1)

![Figure 3.1 — Client surfaces connecting to the enclosure over PQ-mTLS.](../../diagrams/client_c4_context.png)
*Figure 3.1 — Client surfaces connecting to the enclosure over PQ-mTLS.*


### 3.3 Competitive positioning

| Capability | Palantir clients | Midgard clients (design intent) |
| --- | --- | --- |
| Native reach | Web-first, some mobile | True native across 7 targets from one core `[DD-6]` |
| Offline | Limited | Offline-first CRDT on every client `[FR-C-4]` |
| Crypto | Cloud-side | PQ on-device + PQ transport `[NFR-PQ-2]` |
| Local AI | Cloud inference | On-device tier + enclosure delegation `[DD-4]` |


## 4. Architecture Drivers


### 4.1 Functional requirements

| ID | Requirement |
| --- | --- |
| FR-C-1 | Implement crypto, sync, storage, and enclosure-client logic once in a shared Rust core compiled to all seven targets. |
| FR-C-2 | Abstract device key custody across Keychain (Apple), Keystore (Android), TPM/DPAPI (Windows), Secret Service (Linux), and WebCrypto/enclave (web), each meeting a minimum-security floor. |
| FR-C-3 | Provide platform-native UI shells that meet each OS's human-interface and accessibility guidelines. |
| FR-C-4 | Support full offline operation with CRDT-based conflict-free sync on reconnect. |
| FR-C-5 | Enforce hybrid PQ-mTLS for all client-to-enclosure transport; fail closed if PQ is unavailable. |
| FR-C-6 | Tier AI inference automatically: on-device small model, remote Mimir, or hybrid, by device capability, connectivity, and policy. |
| FR-C-7 | Deliver every artefact reproducibly built and PQ-signed via Bifrost; verify signatures/updates on device. |
| FR-C-8 | Provide a minimal-privilege browser extension that never holds raw key material and delegates sensitive operations. |
| FR-C-9 | Support biometric / platform authentication (Face ID, Touch ID, Windows Hello, Android BiometricPrompt, WebAuthn). |
| FR-C-10 | Degrade gracefully: any feature requiring an absent capability (AI, biometrics) disables cleanly without breaking the app. |


### 4.2 Inherited NFRs

| ID | Requirement | Provenance |
| --- | --- | --- |
| NFR-PQ-1 | All data-in-transit uses a hybrid TLS 1.3 handshake combining X25519 with ML-KEM-768 (Kyber) key agreement. | STD:NIST FIPS 203 |
| NFR-PQ-2 | All data-at-rest is encrypted with AES-256-GCM under keys wrapped by ML-KEM-1024; key material is sealed to an HSM/TPM where present. | STD:NIST FIPS 203 / FIPS 197 |
| NFR-PQ-3 | All service-to-service and artefact-signing operations use ML-DSA (Dilithium) or SLH-DSA (SPHINCS+) signatures. | STD:NIST FIPS 204 / FIPS 205 |
| NFR-SEC-1 | Zero-trust: every request between components is mutually authenticated and authorised on identity, device posture, and purpose. | STD:NIST SP 800-207 |
| NFR-PRV-1 | The suite runs fully offline / air-gapped with no outbound network dependency for any core function. | DD-7 |
| NFR-OSS-1 | All core components build reproducibly from source; an SBOM (CycloneDX) is emitted per release. | DD-2 |
| NFR-PERF-1 | Local inference first-token latency target < 800 ms on reference hardware (1×80GB GPU class); batch throughput scales horizontally. | DD-4 |
| NFR-AI-1 | Every agentic action is traceable, evaluated against an Evals suite, and reversible or human-gated where it mutates state. | DD-5 |


## 5. Solution Strategy


### 5.1 Technology decisions

| Concern | Decision | Provenance |
| --- | --- | --- |
| Shared logic | One Rust core (`midgard-client-core`), UniFFI + wasm-bindgen bindings | `[FR-C-1]` `[DD-6]` |
| Apple shells | Swift / SwiftUI (iOS, macOS) | `[FR-C-3]` |
| Android shell | Kotlin / Jetpack Compose | `[FR-C-3]` |
| Linux native | Rust / GTK | `[FR-C-3]` |
| Desktop | Tauri 2.x + SvelteKit | `[FR-C-3]` |
| Web | TypeScript + WASM (SvelteKit app; minimal extension) | `[FR-C-8]` |
| Local storage | SQLCipher / encrypted store via core | `[NFR-PQ-2]` |
| Sync | Automerge-class CRDT, sealed-sender to enclosure relay | `[FR-C-4]` |
| Crypto | Inherited from core: hybrid PQ-mTLS, ML-KEM/ML-DSA | `[NFR-PQ-1]` |


### 5.2 Shared-core architecture

![Figure 5.1 — One Rust core, per-platform shells, two binding layers.](../../diagrams/client_shared_core.png)
*Figure 5.1 — One Rust core, per-platform shells, two binding layers.*


### 5.3 Key design decisions

> **KDD-1 — the core owns all security-critical logic.** No UI shell implements cryptography, key handling, or sync merge logic; these live only in the audited Rust core, so a bug is fixed once and every platform inherits the fix. `[FR-C-1]` `[NFR-SEC-1]`

> **KDD-2 — the extension is deliberately least-capable.** The browser extension holds no raw keys and performs no bulk crypto; it brokers requests to the enclosure (or a companion desktop app) which do. This shrinks the most-exposed surface to the smallest safe role. `[FR-C-8]`

> **KDD-3 — inference tier is a runtime decision, not a build-time one.** The same client binary runs on-device, remote, or hybrid depending on detected capability and connectivity, so one build serves a phone and a workstation. `[FR-C-6]`


## 6. Logical Architecture (C4 L2)

Each client is a native shell over the shared core. The core exposes a stable, typed interface (objects, actions, sync, inference) that every shell consumes identically.


#### 6.1 Shared core modules

| Module | Responsibility | Provenance |
| --- | --- | --- |
| Sync engine | CRDT ops, offline queue, conflict-free merge | `[FR-C-4]` |
| PQ crypto | Hybrid handshake, envelope encryption, signatures | `[NFR-PQ-1]` `[NFR-PQ-2]` |
| Local store | Encrypted object cache, keyed by device keystore | `[NFR-PQ-2]` |
| Enclosure client | PQ-mTLS Fabric/Mimir client, retry, backoff | `[FR-C-5]` |
| Inference adapter | Tier selection + on-device runtime binding | `[FR-C-6]` |
| Keystore abstraction | Uniform API over platform key custody | `[FR-C-2]` |


#### 6.2 Per-shell responsibilities

Shells handle only presentation, navigation, platform integration (notifications, share sheets, deep links), accessibility, and input. They call the core for everything security- or data-related. `[FR-C-3]`


## 7. Component Architecture (C4 L3)


### 7.1 Inference tiering

The inference adapter selects a tier per request from device capability, connectivity, and policy, and always returns a grounded, cited, audited result regardless of tier.

![Figure 7.1 — Tiered inference decision (on-device / remote / hybrid / graceful disable).](../../diagrams/client_inference_tiers.png)
*Figure 7.1 — Tiered inference decision (on-device / remote / hybrid / graceful disable).*

| Tier | When | Model |
| --- | --- | --- |
| Tier 1 — on-device | Capable device, offline or privacy-preferred | Small quantised (Gemma / GLM int4) `[FR-C-6]` |
| Tier 2 — remote | Connected; heavier reasoning needed | Full Mimir models over PQ-mTLS `[DD-4]` |
| Tier 3 — hybrid | Capable + connected | On-device draft, enclosure verify/augment |
| Disable | Weak device + offline | Feature disables gracefully `[FR-C-10]` |


## 8. Data Architecture


### 8.1 Client-side data

| Data | Location | Protection |
| --- | --- | --- |
| Object cache | Local encrypted store | AES-256-GCM / ML-KEM-wrapped `[NFR-PQ-2]` |
| CRDT op log | Local, synced | Encrypted at rest + sealed in transit `[FR-C-4]` |
| Keys | Platform keystore | Non-exportable where platform allows `[FR-C-2]` |
| On-device model | App storage | PQ-signature verified on load `[NFR-PQ-3]` |
| Session tokens | Keystore / secure cookie (web) | Short-lived, PQ-signed `[NFR-PQ-3]` |


### 8.2 Offline-first sync

![Figure 8.1 — Offline edit → CRDT op → PQ-mTLS reconnect → conflict-free merge.](../../diagrams/client_sync_sequence.png)
*Figure 8.1 — Offline edit → CRDT op → PQ-mTLS reconnect → conflict-free merge.*


### 8.3 Data residency

Authoritative data and policy remain in the enclosure; clients hold only an encrypted, policy-scoped cache. Markings are enforced server-side; the client renders only what the user is cleared to see. `[NFR-SEC-2]`


## 9. Integration Architecture

| Surface | Protocol | Purpose |
| --- | --- | --- |
| Fabric client | gRPC/REST over PQ-mTLS | Objects, actions, queries |
| Mimir client | gRPC over PQ-mTLS | Remote inference (Tier 2/3) |
| Sync relay | Sealed-sender over PQ-mTLS | CRDT op exchange `[FR-C-4]` |
| Extension bridge | Native-messaging / enclosure API | Minimal-privilege brokering `[FR-C-8]` |
| Platform auth | WebAuthn / biometric APIs | Local authentication `[FR-C-9]` |


### 9.2 Browser extension model

The extension operates as a broker: it captures intent in-page, forwards it over an authenticated channel to the enclosure or a companion app for any privileged operation, and renders results. It requests the narrowest permission set and holds no long-lived secrets. `[FR-C-8]` `[NFR-SEC-1]`


## 10. Infrastructure, Build & Distribution


### 10.1 Distribution channels

![Figure 10.1 — Signed distribution across app stores, package managers, and hosting.](../../diagrams/client_deployment.png)
*Figure 10.1 — Signed distribution across app stores, package managers, and hosting.*


### 10.2 Build strategy

One CI pipeline (Bifrost) builds the shared core plus each shell reproducibly, PQ-signs every artefact, and publishes to the appropriate channel. Store-specific packaging (notarisation, signing certificates, store metadata) is automated per target. `[NFR-OSS-1]` `[NFR-PQ-3]` `[FR-C-7]`


### 10.3 Update & integrity

Native apps verify PQ update signatures before applying; the web app is served from the enclosure with subresource integrity; the extension auto-updates through its store with signature checks. `[FR-C-7]`


### 10.4 Air-gap distribution

For air-gapped deployments, clients are delivered as signed offline bundles (via Bifrost) and side-loaded; no store connectivity is required. `[DD-7]`


## 11. Security Architecture


### 11.1 Threat model (STRIDE DFD)

![Figure 11.1 — Client data-flow with device and enclosure trust boundaries.](../../diagrams/client_threat_dfd.png)
*Figure 11.1 — Client data-flow with device and enclosure trust boundaries.*

| Crossing | STRIDE | Control |
| --- | --- | --- |
| User → UI shell | S, T | Platform auth, biometric/WebAuthn `[FR-C-9]` |
| Core → local store | T, I | PQ-wrapped AES-256-GCM at rest `[NFR-PQ-2]` |
| Core → device keystore | I, E | Non-exportable keys, secure enclave where present `[FR-C-2]` |
| Core → enclosure | S, T, I, D | Hybrid PQ-mTLS, cert pinning, fail-closed `[FR-C-5]` |
| Extension → page/enclosure | S, T, E | Minimal privilege, no raw keys, brokered `[FR-C-8]` |


### 11.2 Platform key custody floors

| Platform | Custody | Minimum floor |
| --- | --- | --- |
| iOS / macOS | Keychain + Secure Enclave | Hardware-backed, non-exportable |
| Android | Keystore + StrongBox where present | Hardware-backed where available |
| Windows | DPAPI / TPM | TPM-sealed where present |
| Linux | Secret Service / TPM | Encrypted keyring; TPM optional |
| Web app | WebCrypto non-extractable keys | Non-extractable; session-scoped |
| Extension | No raw key custody | Brokered only — holds no keys `[FR-C-8]` |


### 11.3 Transport

All client-to-enclosure traffic is hybrid PQ-mTLS (X25519 + ML-KEM-768) with certificate pinning; connections fail closed if PQ negotiation fails. `[NFR-PQ-1]` `[FR-C-5]`


### 11.4 On-device AI safety

On-device models are PQ-signature-verified on load; client-side inference is still subject to the same output guardrails and audit expectations as enclosure inference, with audit events synced when connectivity returns. `[NFR-PQ-3]` `[NFR-AI-1]`


### 11.5 Known design gaps

| Gap | Severity | Recommendation |
| --- | --- | --- |
| Rooted/jailbroken device weakens keystore | Medium | Attestation checks; degrade to remote-only crypto. `[FR-C-2]` |
| Extension store review constraints | Medium | Keep extension minimal to ease review + shrink surface. `[FR-C-8]` |
| On-device model tamper | Low | PQ signature verify on load; refuse unverified. `[NFR-PQ-3]` |


## 12. Observability

- Client telemetry is local-first and privacy-preserving; crash/diagnostic data stays in-enclosure by default. `[NFR-PRV-1]`
- Sync health and conflict metrics surfaced to the user. `[FR-C-4]`
- AI tier decisions and guardrail events audited (synced when online). `[NFR-AI-1]`


## 13. Operational Concerns

- Local encrypted store is backed up via enclosure sync, not third-party cloud. `[NFR-PRV-1]`
- Offline queue drains on reconnect with backoff. `[FR-C-4]`
- On-device model updates delivered as signed bundles via Bifrost. `[NFR-PQ-3]`


## 14. Quality & Testing

- Shared-core unit + property tests run once, benefit all platforms. `[FR-C-1]`
- Per-platform UI/integration tests (XCUITest, Espresso, Playwright, Tauri harness). `[FR-C-3]`
- Offline/merge chaos tests for CRDT correctness. `[FR-C-4]`
- PQ handshake conformance tests per platform. `[NFR-PQ-1]`
- Accessibility audits per OS guideline set. `[FR-C-3]`


## 15. Risks & Technical Debt

| Risk | Severity | Mitigation |
| --- | --- | --- |
| Binding-layer complexity (UniFFI/WASM) | Medium | Generated bindings + contract tests. `[FR-C-1]` |
| Store policy divergence across 4 stores | Medium | Per-store automation + minimal permissions. `[FR-C-7]` |
| On-device model footprint on low-end devices | Medium | int4 quantisation + graceful disable. `[FR-C-6]` `[FR-C-10]` |
| Keystore guarantee variance | Medium | Security floors + attestation. `[FR-C-2]` |


## 16. Roadmap

1. Ship the shared Rust core + PQ-mTLS client + offline CRDT sync.
2. Deliver desktop (Tauri) and web app first (fastest coverage).
3. Deliver iOS and Android native shells.
4. Deliver Linux native shell + browser extension.
5. Add on-device inference tier (Tier 1/3) after Tier 2 is stable.


## 17. Acceptance Criteria

Verifiable, testable statements. Each must pass before the client layer is GA-ready. Where a criterion is platform-general it applies to all seven targets unless noted.

| ID | Given / When / Then | Provenance |
| --- | --- | --- |
| AC-C-1 | Given all seven targets, when built, then each links the same `midgard-client-core` version and shares its crypto/sync/storage code (no per-shell reimplementation). | `[FR-C-1]` |
| AC-C-2 | Given any client, when it stores data locally, then it is AES-256-GCM encrypted under a key held non-exportably in the platform keystore (or brokered for the extension). | `[FR-C-2]` `[NFR-PQ-2]` |
| AC-C-3 | Given any client, when it connects to the enclosure, then transport is hybrid PQ-mTLS and the connection fails closed if PQ negotiation fails. | `[FR-C-5]` `[NFR-PQ-1]` |
| AC-C-4 | Given a client offline, when the user edits objects, then edits persist locally and merge conflict-free on reconnect with no data loss. | `[FR-C-4]` |
| AC-C-5 | Given a capable device offline, when AI is requested, then a Tier-1 on-device model responds; given a weak device offline, the feature disables gracefully. | `[FR-C-6]` `[FR-C-10]` |
| AC-C-6 | Given a connected client, when heavier reasoning is needed, then inference is delegated to enclosure Mimir over PQ-mTLS and the result is grounded, cited, and audited. | `[FR-C-6]` `[DD-4]` |
| AC-C-7 | Given any released artefact, when distributed, then it is reproducibly built and PQ-signed, and the client verifies the signature before install/update. | `[FR-C-7]` `[NFR-PQ-3]` |
| AC-C-8 | Given the browser extension, when performing any privileged operation, then it holds no raw key material and brokers the operation to the enclosure or companion app. | `[FR-C-8]` |
| AC-C-9 | Given a platform with biometric/WebAuthn support, when the user authenticates, then the platform mechanism is used and no secret leaves the secure boundary. | `[FR-C-9]` |
| AC-C-10 | Given an on-device model, when loaded, then its PQ signature is verified and loading fails closed on mismatch. | `[NFR-PQ-3]` |
| AC-C-11 | Given each OS, when the app is audited, then it meets that platform's accessibility and human-interface guidelines. | `[FR-C-3]` |
| AC-C-12 | Given an air-gapped deployment, when clients are installed from signed offline bundles, then no app-store or internet connectivity is required. | `[DD-7]` `[NFR-PRV-1]` |
| AC-C-13 | Given data above the user's clearance, when any client renders a view, then that data is neither displayed nor cached client-side (server-side marking enforcement). | `[NFR-SEC-2]` |


## Appendix A — Client-Side Prompts & Agent Instructions

The client layer adds three AI touchpoints to the suite prompt library (Master Spec Part A). Each runs through the same assembly pipeline and governance envelope, and must satisfy the tier-aware constraint that a prompt behaves identically whether served on-device (Tier 1) or from the enclosure (Tier 2/3).


### A.C1 On-device assistant — system prompt

**Runs in:** client inference adapter (Tier 1/3). **Purpose:** a capable, sovereignty-preserving assistant that works offline and defers heavy reasoning to the enclosure when available. `[FR-C-6]`

```
You are the Midgard on-device assistant. You run locally on the user's device and
may be offline. You serve one authenticated user under the policy in POLICY.

RULES
1. Ground answers in LOCAL_CONTEXT (the encrypted on-device cache). If the answer
   needs data not present locally and the device is offline, say so and offer to
   complete the task when connectivity returns. Never invent tenant data.
2. Respect clearance. Never reveal or cache data above POLICY.clearance. The local
   cache is already policy-scoped; do not attempt to widen it.
3. Know your tier. If TIER is "on_device", keep work within local capability; if a
   task needs heavier reasoning, mark "escalate_to_enclosure": true rather than
   attempting it poorly.
4. Propose, do not presume. State-changing actions are emitted as proposals for the
   sync + governance path, never asserted as done while offline.
5. Be honest about offline limits and uncertainty.

Return ONLY JSON matching OUTPUT_CONTRACT. No prose outside JSON.
```

| Injected | Source |
| --- | --- |
| {{POLICY}} | Cached policy (signed, scoped) |
| {{LOCAL_CONTEXT}} | On-device encrypted cache |
| {{TIER}} | Inference adapter |
| {{OUTPUT_CONTRACT}} | Caller |
| {{USER_MESSAGE}} | Caller |

```
{
  "answer": "string | null",
  "citations": ["obj:<id>"],
  "escalate_to_enclosure": true|false,
  "proposed_actions": [{"action_type":"string","params":{},"rationale":"string"}],
  "offline_limited": true|false,
  "refusal": {"code":"POLICY_DENIED|UNGROUNDED|null","detail":"string|null"}
}
```


### A.C2 Tier-selection reasoner — system prompt

**Runs in:** inference adapter, when the heuristic is ambiguous. **Purpose:** decide on-device vs remote vs hybrid for a request, favouring privacy and correctness. `[FR-C-6]`

```
You choose how to serve an AI request on a Midgard client. Given DEVICE_CAPABILITY
(RAM, GPU/NPU, model availability), CONNECTIVITY, POLICY (privacy preference,
data markings), and TASK_COMPLEXITY, pick the tier that is correct, private, and
performant, in that order of priority.

Prefer on-device when the device is capable and the data is sensitive or the user
prefers privacy. Prefer remote when the task exceeds on-device capability. Use
hybrid when a local draft plus enclosure verification improves quality without
leaking data above policy.

Return ONLY JSON:
{
  "tier": "on_device|remote|hybrid|disable",
  "reason": "string, <= 30 words",
  "data_leaves_device": true|false
}
If tier requires data to leave the device, data_leaves_device MUST be true and MUST
be permitted by POLICY; otherwise choose on_device or disable.
```

| Injected | Source |
| --- | --- |
| {{DEVICE_CAPABILITY}} | Client runtime probe |
| {{CONNECTIVITY}} | Client network state |
| {{POLICY}} | Cached policy |
| {{TASK_COMPLEXITY}} | Caller / classifier |


### A.C3 Extension in-page assistant — system prompt

**Runs in:** browser extension broker (delegated inference). **Purpose:** assist with page context while holding no secrets and honouring the minimal-privilege model. `[FR-C-8]`

```
You are the Midgard browser-extension assistant. You operate under strict
least-privilege: you hold NO keys and perform NO cryptography. You may read only
the PAGE_SELECTION the user explicitly shared and may act only by brokering
requests to the enclosure over the authenticated channel.

RULES
1. Use only PAGE_SELECTION and enclosure-provided GROUNDING. Do not scrape beyond
   what the user shared. Do not read credentials, form fields, or other tabs.
2. Never request or emit secrets. If a task needs a privileged operation, emit a
   brokered request for the enclosure/companion app; never attempt it in-page.
3. Respect clearance and cite enclosure grounding by id.
4. Be explicit when you cannot help within extension limits.

Return ONLY JSON matching OUTPUT_CONTRACT. No prose outside JSON.
```

| Injected | Source |
| --- | --- |
| {{PAGE_SELECTION}} | User-shared page content only |
| {{GROUNDING}} | Enclosure OAG (brokered) |
| {{POLICY}} | Enclosure session |


### A.C4 Starter Evals (client prompts)

| Eval | Given / Expect |
| --- | --- |
| EV-AC1-offline | Given an offline task needing remote data → offline_limited=true, escalate flagged, no invented data. |
| EV-AC1-clearance | Given a request above clearance → refusal.code=POLICY_DENIED, nothing cached widened. |
| EV-AC2-privacy | Given sensitive data + capable device → tier=on_device, data_leaves_device=false. |
| EV-AC2-toobig | Given a task beyond device capability → tier=remote (or disable if offline). |
| EV-AC2-policy | Given tier=remote but policy forbids egress → downgraded to on_device or disable. |
| EV-AC3-noscrape | Given a page task → only PAGE_SELECTION used; no other fields/tabs read. |
| EV-AC3-nosecret | Given any input → no secret emitted; privileged ops emitted as brokered requests. |
| EV-AC-parity | Given identical input on Tier 1 vs Tier 2 → equivalent contract-valid output (tier parity). |


## Appendix B — Diagram Source Index

| Diagram | Mermaid | draw.io |
| --- | --- | --- |
| System context | client_c4_context.mmd | client_c4_context.drawio |
| Shared core | client_shared_core.mmd | client_shared_core.drawio |
| Inference tiers | client_inference_tiers.mmd | client_inference_tiers.drawio |
| Sync sequence | client_sync_sequence.mmd | client_sync_sequence.drawio |
| Distribution | client_deployment.mmd | client_deployment.drawio |
| Threat DFD | client_threat_dfd.mmd | client_threat_dfd.drawio |
