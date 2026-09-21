# Midgard — Orion Phase 3 Roadmap

Extends Orion (Heimdall SAD) into a networked, three-dimensional operational picture
shared from command post to dismounted operator, and the force-multiplication that
gives the Midgard core.

## Scope
IN SCOPE:
- Networked common picture: federate multiple Heimdall/Orion nodes via CRDT + PQ-mTLS
- Full 3D environmental simulation: terrain, structures, line-of-sight/viewshed,
  sensor-feed registration, environment simulation (one shared Rust 3D core)
- Ground-operator augmentation: shared 3D core -> desktop command-post + mobile/wearable
- Force-multiplication: the networked 3D picture enriches Midgard fabric + Mimir grounding

OUT OF SCOPE (excluded by design, though requested):
- Actual effects execution / weapon or fire control / closed-loop engagement
- Offensive "forward-push" cyber reach into connected assets

Rationale: combined with Phase 2's weapon-pairing/munitions capabilities, an execution
loop would be the automated-engagement design of a weapons system. Orion is architected
as decision-support; Phase 3 makes that boundary a hard, tested architectural invariant
(INV-ORION) rather than crossing it. The networked picture is built only from feeds that
assets authentically publish (PQ-authenticated), never by pushing into them.

## Contents
- Orion_Phase3_Roadmap.docx   — branded deliverable (12 pages)
- Orion_Phase3_Roadmap.md     — source-of-truth companion
- diagrams/                   — 5 diagrams x (.mmd + .drawio + .png)

## New in this phase
- Requirements: FR-P3-1..11, NFR-P3-1..4
- Acceptance criteria: AC-P3-1..16 (incl. adversarial no-execution proof AC-P3-15)
- Prompts: A.P3-1 (3D/LoS assistant), A.P3-2 (operator SA), A.P3-3 (federation integrity)
- Consolidated suite acceptance-criteria count: 108 -> 124
