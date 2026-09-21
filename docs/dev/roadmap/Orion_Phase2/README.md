# Midgard — Orion Phase 2 Roadmap

Video-evidenced Phase 2 addendum to the Heimdall SAD. Built from direct analysis of
the supplied 112-second video (28 frames sampled + OCR of on-screen interface text).

The video is a Palantir Gotham "FMV/SIP Demo". Phase 2 specifies the capabilities it
shows that Orion Phase 1 (screenshot-based) did not cover:
- Computer-vision auto-detection & classification (airplane/vessel/vehicle) + confidence
- Full platform/sensor attitude-pose model (FLIR SS3 HD-class) with geo-projection
- Effects-based tasking optimisation (match/effect, time-to-target, distance; weightable)
- Munitions accounting (avail/alloc/spent) — read-and-recommend only
- LIVE vs Simulation mode

All additions are advisory, human-gated, and PQ-audited. No weapon-control or
engagement mechanics are specified — Orion is decision-support.

## Contents
- Orion_Phase2_Roadmap.docx           — branded deliverable
- Orion_Phase2_Roadmap.md             — source-of-truth companion
- video_analysis_evidence.md          — the OCR evidence the roadmap is built on
- diagrams/                           — 4 diagrams × (.mmd + .drawio + .png)

## New in this phase
- Requirements: FR-P2-1..10, NFR-P2-1..4
- Acceptance criteria: AC-P2-1..16
- Prompts: A.P2-1 (CV review), A.P2-2 (effects optimiser), A.P2-3 (simulation analyst)
- Consolidated suite acceptance-criteria count: 92 -> 108

## On the source
The video WAS analysed directly (frames + OCR), unlike the Phase 1 addendum which used
screenshots. The audio track was music; no speech recognition was available, so analysis
is visual. Capabilities are cited to specific frames in video_analysis_evidence.md.
