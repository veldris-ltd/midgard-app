# Midgard — Orion Phase 3 Roadmap (Heimdall SAD Addendum)

*Networked common picture · full 3D environmental simulation · ground-operator augmentation · force-multiplication to Midgard*

**Builds on:** Orion Phase 1 (FR-OR-*), Phase 2 (FR-P2-*), Heimdall (FR-H-*)
**New:** FR-P3-1..11, NFR-P3-1..4, AC-P3-1..16, prompts A.P3-1..3 · **v1.0** · PUBLIC, published with the Midgard repository
**Operator target:** shared 3D core → desktop command-post + mobile/wearable

> **SCOPE BOUNDARY — read first.** Phase 3 delivers the networked common picture, full 3D environmental simulation, ground-operator augmentation, and force-multiplication to Midgard. It deliberately does **NOT** specify effects execution, weapon/fire control, closed-loop engagement, or offensive 'forward-push' cyber reach into connected assets. Those were requested and are excluded by design. Orion remains decision-support: it builds the picture and hands an authorised human the decision.

## 1. Scope decision

| Requested | Decision | Where |
|---|---|---|
| Networked complete common picture | **IN SCOPE** | §3 Federation |
| Full 3D simulation of environments/maps/interactions | **IN SCOPE** | §4 3D core |
| Operator augmentation on the ground | **IN SCOPE** | §5 Operator |
| Advantages back to Midgard | **IN SCOPE** | §6 Force-multiplication |
| Actual effects execution | **OUT OF SCOPE** | §1.2 excluded |
| Forward-push cyber into connected assets | **OUT OF SCOPE** | §1.2 excluded |

**Why excluded:** effects execution + Phase 2's weapon-pairing/munitions/time-to-target would constitute the automated-engagement design of a weapons system; Orion is architected as decision-support and Phase 3 hardens that boundary rather than crossing it. Forward-push into connected assets is offensive cyber capability; the networked picture is built only from feeds assets authentically publish (PQ-authenticated), never by pushing into them.

**Reframing:** the genuine force-multiplier — one networked 3D operational truth from command post to dismounted operator — does not require effects execution or offensive cyber to be powerful. It requires federation, 3D simulation, and augmentation done well.

## 2. Functional requirements

| ID | Requirement |
|---|---|
| FR-P3-1 | Federate nodes into one shared picture via CRDT over PQ-mTLS; nodes still standalone; unverified rejected. |
| FR-P3-2 | Filter shared picture per node by marking/need-to-know; federation never widens access. |
| FR-P3-3 | Shared 3D scene core: terrain (DEM) + structures + networked entities as navigable 3D. |
| FR-P3-4 | Line-of-sight / viewshed across terrain and structures from any point. |
| FR-P3-5 | Register live FMV/sensor feeds onto 3D geometry (extends FR-P2-1). |
| FR-P3-6 | Simulate environment over time (movement, lighting, coverage); labelled simulation. |
| FR-P3-7 | Deliver 3D core to desktop command-post console (picture, LoS, planning). |
| FR-P3-8 | Deliver 3D core to mobile/wearable operator (SA, navigation, LoS, advisory alerts); offline-first. |
| FR-P3-9 | Feed networked 3D picture + operator observations back into Midgard fabric + Mimir grounding. |
| FR-P3-10 | Expose NO effect-execution/weapon-control/asset-push interface; human authorisation + audit on every consequential action. |
| FR-P3-11 | Invariant tests proving no path yields an executed effect, weapon-control action, or asset-push. |

## 3. Non-functional requirements

| ID | Requirement |
|---|---|
| NFR-P3-1 | 3D core on-device/air-gapped; mobile degrades 3D fidelity gracefully. |
| NFR-P3-2 | Federation offline-first, partition-tolerant, no central master. |
| NFR-P3-3 | Simulation vs live indelibly distinguished; never auto-promoted. |
| NFR-P3-4 | Federated data carries provenance + marking end-to-end; fully auditable. |

## 4. Architecture

- **Federation:** CRDT merge + PQ authentication + per-node marking → one shared picture replicated back.
- **Shared 3D core (one Rust implementation):** scene graph, line-of-sight/viewshed, sensor-feed registration, environment simulation → feeds both desktop and mobile shells (reuses SAD_06 shared-core pattern).
- **Operator augmentation:** SA, terrain-aware navigation, 'what can see me / I can see', advisory alerts; observations flow back to Midgard.
- **Force-multiplication:** richer fabric, better Mimir grounding, cross-site analytics, spatially-aware options — all still advisory.

![Federation](../diagrams/p3_federation.png)
![3D core](../diagrams/p3_3d_core.png)
![Operator](../diagrams/p3_operator.png)
![Control invariant](../diagrams/p3_control_invariant.png)
![Roadmap](../diagrams/p3_roadmap.png)

## 5. The control invariant (INV-ORION)

**No Orion component, in any phase, may execute a consequential action.** Orion senses, fuses, federates, builds the 3D picture, analyses, ranks, simulates — then terminates at a mandatory, audited human-authorisation gate. No code path from an Orion output to a delivered effect, to weapon/fire control, or into another system. Enforced by: no execution interface exists (FR-P3-10); every consequential step human-gated+audited; adversarial invariant tests (FR-P3-11, AC-P3-14/15); marking never widens (FR-P3-2).

## 6. Prompts (extend SAD_05 Part A)

- **A.P3-1 3D scene/LoS assistant:** answers spatial questions (intervisibility, coverage, cover), cited; no intent/action; sim labelled.
- **A.P3-2 Operator SA assistant:** grounded situational awareness for the dismounted operator; advisory alerts as information not orders; refuses engagement guidance.
- **A.P3-3 Federation integrity monitor:** surfaces unverified-node/marking/divergence anomalies for human review; changes nothing.

All emit contract-validated JSON with citations, in Mimir's advisory envelope. Includes cross-cutting Eval EV-INV-noexec: no phrasing across any Phase 3 prompt executes an effect, controls a weapon, or pushes to an asset.

## 7. Acceptance criteria

AC-P3-1..16 covering: conflict-free federation with standalone nodes; unverified-node rejection; marking never widens; navigable 3D scene; LoS/viewshed; feed registration; labelled simulation; desktop console; mobile offline-first with graceful fidelity; operator observations enrich fabric; Mimir 3D grounding; partition re-merge; end-to-end provenance; **no execution/weapon/asset-push interface (AC-P3-14); adversarial no-execution proof (AC-P3-15);** human authorisation on every consequential action (AC-P3-16).

## 8. Suite integration

Extends Heimdall + Orion 1–2 · shared 3D core reuses SAD_06 · enriches SAD_00/02 · governed via SAD_07 dual-control · A.P3-1..3 + AC-P3-1..16 join SAD_05 (**consolidated ACs 108 → 124**).

**Closing:** Phase 3 completes the picture Orion was building toward — one networked, three-dimensional operational truth from command post to operator, feeding back to make Midgard smarter. It stops exactly where it should: at the human. The most capable version of this system advises rather than acts.
