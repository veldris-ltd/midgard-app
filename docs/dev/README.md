# Midgard Suite — Complete Solutions Architecture Documentation

Forward-looking design specifications (greenfield) for an open-source, post-quantum-encrypted
alternative to the Palantir platform. Imhotep methodology (arc42 + TOGAF + C4; security aligned
to NIST SP 800-53 / OWASP ASVS / ISO 27001). Provenance tags: [DD-*] driver, [FR-*]/[NFR-*]
requirement, [STD:*] standard. Design specs, not evidence of an implemented system.

## documents/  (branded Akuma Engineering Ltd. DOCX)
- SAD_00_Midgard_Core.docx                       — sovereign core platform / enclosure
- SAD_01_Forge.docx                              — data operations (vs Palantir Foundry)
- SAD_02_Mimir.docx                              — local agentic AI (vs Palantir AIP)
- SAD_02b_Mimir_Model_Supply_Resolution.docx     — DCR: multi-model provenance (GLM/Nemotron/Gemma)
- SAD_03_Bifrost.docx                            — continuous delivery (vs Palantir Apollo)
- SAD_04_Heimdall.docx                           — operational fusion (vs Palantir Maven)
- SAD_05_Master_Prompt_and_Acceptance_Spec.docx  — CONSOLIDATING MASTER: prompt library +
                                                   agent-flow architecture + ALL acceptance criteria
                                                   (72 across the suite) + build-capabilities register
- SAD_06_Client_Platforms.docx                   — native mobile/desktop + web (browser+extension)
- SAD_07_Admin_Control_Panel.docx                — NEW: unified, self-governing admin console

## The Admin Control Panel SAD (SAD_07) covers
- One unified console over IAM, Governance, KMS, Fabric, Mimir, Bifrost, and Observability —
  composing existing subsystem admin APIs, never reimplementing them.
- Role-separated admin RBAC (no all-powerful role; read-only Auditor); dual-control on all
  high-risk actions (key rotation, privilege escalation, model revocation, fleet release,
  break-glass); time-boxed break-glass with mandatory review.
- Versioned (GitOps) configuration with policy simulation / dry-run before commit.
- PQ-signed, append-only admin audit ledger; self-governance (the panel is bound by the
  same policy/audit it administers — no privileged bypass).
- 13 acceptance criteria + 3 admin prompts (advisory copilot, simulation explainer,
  read-only audit assistant) with Evals — also folded into the Master Spec (SAD_05).

## markdown/   — source-of-truth companions (one per document)
## diagrams/   — every diagram as .mmd + .drawio + .png (63 diagrams x 3 formats)
