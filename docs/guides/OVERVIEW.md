# Midgard overview

What the suite is made of, the principles it is built to, the specification it implements, and the design system. The [README](../../README.md) is the place to start; the [claims ledger](../compliance/CLAIMS.md) says what runs today.

## The suite

Midgard is the core platform, *the enclosure*: identity, the object/ontology fabric, storage, compute, governance, and post-quantum cryptography applied by default. Atop it sit four independently useful, composable products.

| Product | Domain | Accent |
|---|---|---|
| **Midgard** | Core platform, identity, object fabric, governance, PQ crypto substrate | Enclosure red `#CA1D25` |
| **Forge** | Data operations: ingest, transform, version, project to the fabric | Ember orange `#D96E1E` |
| **Mimir** | Local agentic AI (Mistral / Nemotron / Gemma / Granite), OAG grounding, agents, Evals | Well teal `#23A8B6` |
| **Bifrost** | Continuous delivery: PQ-signed artefacts, rings, fleet, offline bundles | Bridge violet `#7D5CE6` |
| **Heimdall** | Operational fusion: feeds, one common picture, decision workflows | Watchman gold `#C99A22` |

Each domain above is what that product's SAD specifies. What runs today is narrower. A Midgard edge serves the object fabric. A Forge worker syncs files, PostgreSQL and S3-compatible stores into it. Mimir serves inference from one signed model on CPU. Bifrost records the fleet and packs signed offline bundles. A Heimdall feed gateway fuses signed observations into one picture. Model-family routing, grounded retrieval, agents, evals, the compute engine, ring rollout and decision support are implemented and tested as libraries, not yet in a running service. The [claims ledger](../compliance/CLAIMS.md) gives each capability's status and evidence.

Two cross-cutting layers complete the suite:

- **Admin Control Panel**: specified in `SAD_07` as one self-governing console over IAM, Governance, KMS, Fabric, Mimir, Bifrost and Observability, with role-separated RBAC (no super-admin), dual control on high-risk actions, time-boxed break-glass and versioned configuration. Dual control, break-glass and signed configuration are implemented and tested as libraries. In the running services dual control applies to principal enrolment only, and the admin console runs on fixture data.
- **Client Platforms**: specified in `SAD_06`: native mobile (iOS / Android / Linux), native desktop (Tauri: macOS / Windows / Linux), and web (WASM app + broker-mode browser extension), offline-first and key-custody-aware. The shared client core is implemented and tested as a library. None of the six shells is built: they are scaffolded as manifests and accessibility declarations.

### Design principles

- **Sovereign by construction.** No core function requires an outbound network call. The platform runs fully offline and air-gapped; the acceptance suite proves it under `--network none`.
- **Post-quantum by default.** Hybrid X25519 + ML-KEM-768 in transit; AES-256-GCM under ML-KEM-1024-wrapped keys at rest; ML-DSA / SLH-DSA signatures for service auth and artefact signing. `[STD:NIST FIPS 203/204/205]`
- **Object fabric at the centre.** A decision-centric semantic + kinetic layer (objects, properties, links; actions, functions, dynamic security).
- **Local AI native.** Inference runs inside the enclosure, in Mimir, and nothing calls a hosted model. Today only `mimir-probe` calls it; no product routes AI requests to it yet.
- **Provenance everywhere.** Every claim has a source, every action has lineage. Provenance tags, append-only audit, and human gates are first-class, not afterthoughts.
- **Open-source core.** Reproducible builds, CycloneDX SBOM per release, OSI-approved licence for the core.


## Run the design system

Static HTML, CSS and JSX: any static file server works. React/Babel load from CDN on first paint, then cache.

```bash
# from the repository root
python -m http.server 8099        # or: npx serve -l 8099 .
# open http://localhost:8099/docs/design/docs/index.html
```

The pages take their tokens and fonts from `ui/design-system/src`, the one tracked copy, so the server root must be the repository root.

| Surface | URL (under `/docs/design`) |
|---|---|
| **Design-system docs** (foundations, 28 components, patterns, screens, flows) | `/docs/index.html` |
| **Midgard console**: Object Explorer | `/ui_kits/midgard-console/index.html` |
| **Forge** · **Mimir** · **Bifrost** · **Heimdall** | `/ui_kits/forge/index.html` · `/mimir/` · `/bifrost/` · `/heimdall/` (each `…/index.html`) |
| **Admin**: Control Panel | `/ui_kits/admin/index.html` |
| **Clients**: mobile / desktop / web | `/ui_kits/clients/mobile.html` · `desktop.html` · `web.html` |
| **Marketing site** | `/site/index.html` |

> **Troubleshooting.** Blank page / "Failed to fetch" → you opened `file://`; serve over `http://`. 404 on a token or font file → your server root isn't the repository root. Fully offline → vendor `react`, `react-dom` and `babel` locally and repoint the three `unpkg` `<script>` tags.

These UI kits are **reference designs**. The console UIs live under `ui/` and `products/`. The Midgard console can be pointed at the gateway edge; the other six run on built-in sample data.

---

## The specification

`docs/dev/markdown/` holds the Solutions Architecture Documents, authored to the **Imhotep** methodology (arc42 + TOGAF + C4; security aligned to NIST SP 800-53 / OWASP ASVS / ISO 27001). Every architectural decision carries a provenance tag: `[DD-*]` design driver, `[FR-*]`/`[NFR-*]` requirement, `[STD:*]` external standard. The SADs are the design authority. The code implements them in part, as the claims ledger records, and every acceptance criterion traces back to a requirement here.

| Document | Covers |
|---|---|
| `SAD_00_Midgard_Core` | The sovereign core platform / enclosure |
| `SAD_01_Forge` | Data operations |
| `SAD_02_Mimir` | Local agentic AI |
| `SAD_02b_Mimir_Model_Supply_Resolution` | DCR: multi-model provenance (Mistral / Nemotron / Gemma / Granite) |
| `SAD_03_Bifrost` | Continuous delivery |
| `SAD_04_Heimdall` | Operational fusion |
| `SAD_05_Master_Prompt_and_Acceptance_Spec` | **Consolidating master:** deployable prompt library + agent-flow architecture + all 72 acceptance criteria + build-capabilities register |
| `SAD_06_Client_Platforms` | Native mobile / desktop + web (browser + extension) |
| `SAD_07_Admin_Control_Panel` | Unified, self-governing admin console |

The directory also carries two **forward-looking** module specifications, `SAD_08_Gungnir` and `SAD_08b_Forseti`, which have no implementation in this workspace, no acceptance coverage, and no place in the build playbook. `SAD_09_Completion_Plan` is the engineering backlog from reference implementation to operable product: every item a prompt, `AC-RF-*` acceptance criteria and a definition of done.

The same documents ship as branded `.docx` in `docs/dev/documents/`; the 73 diagrams are rendered three ways each: Mermaid (`.mmd`), editable draw.io (`.drawio`) and PNG, in `docs/dev/diagrams/`.

---

## Caveats

- **Model weights are not vendored.** Mimir serves a real model only through a signed manifest whose weights stream to the signed digest, behind an OpenAI-compatible engine on loopback: llama.cpp on CPU, vLLM on GPU. The operator fetches the weights (see [Serve a real model](RUNNING.md#serve-a-real-model)). The acceptance suites still run against the deterministic backend.
- **The TypeScript tier is not gated.** `just build-ts` tolerates failure and `just test` runs no TypeScript, so the consoles are not covered by CI. They typecheck clean today (`pnpm -r typecheck`), but nothing enforces that.
- **No logos exist.** Midgard and its SADs are Veldris Ltd property. No marks were provided or invented: wherever a logo would sit, the product name is set in plain type (`ProductMark`). Supply real marks and they can be wired in.

## Related proprietary tools

Likan is a separate, proprietary tool for data-structure ontologies. It is not part of this repository and is not covered by its licence, its acceptance gate, its SBOMs or its published figures. Nothing here depends on it, downloads it or needs it.

Midgard loads an ontology from a documented file format, [`contracts/schema/ontology.v1.schema.json`](../../contracts/schema/ontology.v1.schema.json), which can be written by hand or produced by any tool. Likan is in active development and is intended to export that format. No file produced by Likan has been validated against it in this repository yet, and no pipeline between the two has been run here.
