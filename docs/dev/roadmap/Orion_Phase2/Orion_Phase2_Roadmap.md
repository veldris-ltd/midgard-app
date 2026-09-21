# Midgard — Orion Phase 2 Roadmap (Heimdall SAD Addendum)

*Computer-vision detection, sensor pose, effects-based tasking & simulation — advisory, human-gated, auditable*

**Builds on:** Orion Phase 1 (FR-OR-1..8), Heimdall (FR-H-1..6) · **Source:** direct analysis of the supplied 112s video (28 frames + OCR)
**New:** FR-P2-1..10, NFR-P2-1..4, AC-P2-1..16, prompts A.P2-1..3 · **v1.0** · CONFIDENTIAL — RECIPIENT EYES ONLY

> **Source — analysed, not assumed.** Built from frame-by-frame analysis: 28 frames @ 4s + OCR of on-screen text. Capabilities are evidenced by literal UI text (cited by frame). Audio was music; no ASR available.

> **Posture — unchanged.** Every capability is advisory, human-gated, PQ-audited. The video shows a weapon-pairing recommender; Orion specifies the decision-support software that surfaces options for human authorisation, NOT weapon control, fire-control, or engagement mechanics. No design path from recommendation to executed effect without human authorisation.

## 1. What the video shows (evidence)

Title bar reads **"FMV/SIP Demo Video"** (Palantir Gotham family).

| Capability | Evidence (OCR frame) |
|---|---|
| Live FMV as geo-registered layer | 'Video Source', 'Live Video' (f004) |
| Platform attitude/pose | 'PLATFORM': Heading/Elevation/Roll/Pitch (f005–007) |
| Sensor model + pose | 'FUR SS3 HD', rel. Azimuth/Elevation (f005–007) |
| CV auto-detection & classification | 'Computer Vision Detection': Airplane/Assault Vessel/Vehicle (f010,f012) |
| Detection confidence + tracking | 'scores', 'Tracking' (f010) |
| Target Board | 'Target Board'/'SARA Board' (f010) |
| Deliberate vs dynamic targeting | 'Runway','Deliberate','Dynamic' (f009) |
| Tasking recommender columns | 'Match (Effect)','Time to Target','Distance','Priority' (f021) |
| Effects optimisation weights | 'Optimize': Time on Station × Fuel × Munitions (f021) |
| Top Match card | 'STRYKER1','M2 .50 cal (75.0%)','2x Aimpoints' (f022) |
| Munitions accounting | 'MUNITIONS AVAIL/ALLOC/SPENT' (f022) |
| Time-to-target & distance | '4m 23s','141.8 nm' (f022) |
| Live vs simulation | 'LIVE (updated 0s ago)' vs 'Simulation' (f022) |

**Why Phase 2:** Phase 1 (screenshots) covered FMV, SIP, Target Board, basic recommender. The video reveals four new capabilities: CV auto-detection, full platform/sensor attitude model, effects-based optimisation with munitions accounting, and LIVE/Simulation mode.

## 2. Functional requirements

| ID | Requirement |
|---|---|
| FR-P2-1 | Full platform+sensor attitude/pose model; project sensor footprint for video geo-registration. |
| FR-P2-2 | Harden FMV↔picture registration within tolerance under platform motion. |
| FR-P2-3 | On-device CV detection & classification (airplane/vessel/vehicle) with confidence; advisory, human-confirmable. |
| FR-P2-4 | Associate detections into persistent tracks with confidence/uncertainty. |
| FR-P2-5 | Effects-based tasking optimiser: per pair compute match/effect, time-to-target, distance; operator-weightable; show all metrics. |
| FR-P2-6 | Munitions accounting (avail/alloc/spent); never allocate/expend automatically. |
| FR-P2-7 | Simulation mode models tasking WITHOUT issuing; distinct from LIVE. |
| FR-P2-8 | Deliberate & dynamic targeting workflows with full human-authorised audited transitions. |
| FR-P2-9 | Explicit human authorisation for every consequential action; no autonomy (extends FR-OR-7/FR-H-5). |
| FR-P2-10 | Compute time-to-target & distance per pair from live geometry, with provenance. |

## 3. Non-functional requirements

| ID | Requirement |
|---|---|
| NFR-P2-1 | CV runs on-device & air-gapped; no frame/detection leaves the enclosure. |
| NFR-P2-2 | Every detection/score/recommendation/option carries machine-checkable citations; uncited = defect. |
| NFR-P2-3 | Simulation outputs indelibly labelled; never auto-promoted to live without explicit human step. |
| NFR-P2-4 | CV model provenance via Mimir supply chain (SAD_02b): signed/licensed/evaluated/verify-on-load. |

## 4. Architecture

- **CV Detection service** (on-device): detect → classify → confidence → track association; advisory; human nominates.
- **Platform/sensor pose model**: position+heading+roll+pitch + sensor id/azimuth/elevation → geo-projection → video registration.
- **Effects-Based Tasking Optimiser**: per-pair match/effect + time-to-target + distance; operator weights; munitions shown not committed; LIVE vs Simulation; human authorises.

![CV detection](../diagrams/p2_cv_detection.png)
![Sensor pose](../diagrams/p2_sensor_pose.png)
![Effects tasking](../diagrams/p2_effects_tasking.png)
![Roadmap](../diagrams/p2_roadmap.png)

## 5. Prompts (extend SAD_05 Part A)

- **A.P2-1 CV detection review:** observational; reports class+confidence+track cited; infers no intent; nominates nothing.
- **A.P2-2 Effects-based optimiser:** ranks with weight-explanation; marks constraint/no-munition candidates ineligible; simulation labelled; proposals only; never allocates.
- **A.P2-3 Simulation analyst:** explains simulated outcome; all outputs labelled simulated; separate human authorisation required to go live; recommends nothing.

All emit contract-validated JSON with citations, in Mimir's advisory envelope.

## 6. Acceptance criteria

AC-P2-1..16 covering: sensor-footprint registration; motion coherence; on-device CV; advisory detections (human confirm); persistent tracks; CV model provenance; effects ranking with cited weights; weight updates; munitions reflected-not-committed; ineligible-on-no-munitions; simulation issues nothing; no auto-promote sim→live; deliberate/dynamic audited transitions; human authorisation on every consequential action; citations mandatory; full air-gapped operation.

## 7. Roadmap

Sensing → Perception → Decision-support → Assurance, each capability landing with its safety gate (AC-P2-*). See Figure 7.1.

## 8. Suite integration

Extends Heimdall + Orion Phase 1 · CV model via SAD_02b supply chain · A.P2-1..3 join SAD_05 Part A · **consolidated ACs 92 → 108** · renders through existing clients (SAD_06) and admin (SAD_07).

**Load-bearing boundary:** Orion detects, tracks, computes, ranks, and simulates — then stops, handing an authorised human the decision. No design path from recommendation to executed effect; no weapon-control specification anywhere.
