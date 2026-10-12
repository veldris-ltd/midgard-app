# Getting Started with Midgard

This guide describes the **specification-driven workflow** by which the Midgard suite is built with an AI coding agent: feed a complete architecture (`docs/dev/`) and a complete design system (`docs/design/`) to an agent, and turn them into software one requirement at a time.

> **The suite is already implemented.** A 70-crate Rust workspace (plus console UIs and gateway edge) was produced from this exact workflow — its git history maps one commit per [BUILD_PLAYBOOK.md](BUILD_PLAYBOOK.md) prompt, and all 72 acceptance criteria pass air-gapped. To **build, test, or verify** what exists, see [README.md → Running locally](README.md#running-locally). This guide remains the reference for *how* it was built and for extending it — same loop, same gates.

> **Looking for the exhaustive, in-sequence build reference?** [BUILD_PLAYBOOK.md](BUILD_PLAYBOOK.md) lists every build prompt, all 15 runtime AI prompts (verbatim), and all 72 acceptance criteria, phase-by-phase across the whole suite. This guide is the orientation; the playbook is the execution detail.

Read it top to bottom the first time. After that, use it as a map:

1. [How the build works](#1-how-the-build-works)
2. [Prerequisites](#2-prerequisites)
3. [Which documents to read first](#3-which-documents-to-read-first)
4. [Your first prompts (in order)](#4-your-first-prompts-in-order)
5. [Running the design system locally](#5-running-the-design-system-locally)
6. [The design system, in depth](#6-the-design-system-in-depth)
7. [The prompt library (building Mimir's AI)](#7-the-prompt-library-building-mimirs-ai)
8. [The full build path](#8-the-full-build-path)
9. [Acceptance criteria & definition of done](#9-acceptance-criteria--definition-of-done)
10. [House rules & conventions](#10-house-rules--conventions)

---

## 1. How the build works

The product code exists (`just build` / `just test` compile and verify it — see [README.md → Running locally](README.md#running-locally)). But the *way it came to exist* is a **workflow**, not a single script — and it's the workflow you use to extend it:

```
  docs/dev/  (the spec: what & why)          docs/design/  (the look & behaviour)
        │                                              │
        └──────────────┬───────────────────────────────┘
                       ▼
             an AI coding agent
        (your agent of choice)
                       │
                       ▼
        production code for the suite
   (verified against acceptance criteria & Evals)
```

Your job is to feed the agent the right slice of the specification, point it at the design system for anything user-facing, let it generate code, then verify that code against the acceptance criteria in `SAD_05`. You do this product-by-product, screen-by-screen, requirement-by-requirement.

Two principles make this reliable:

- **Provenance in, provenance out.** Every requirement in the SADs carries a tag (`[FR-B-1]`, `[NFR-PQ-2]`, `[STD:NIST FIPS 203]`). Carry those tags into commits, tests, and PRs so every line traces back to a spec.
- **One accent at a time.** Whatever product you're building, work inside its theme scope and its SAD. Don't mix Forge and Heimdall in one session.

---

## 2. Prerequisites

You need very little to *start*, because most of the work is reading and prompting.

**To read the specs**
- A Markdown viewer (any editor). The source of truth is `docs/dev/markdown/*.md`.
- Optional: [draw.io](https://draw.io) / the VS Code Draw.io Integration extension to edit `docs/dev/diagrams/*.drawio`. PNGs render anywhere; `.mmd` renders in any Mermaid viewer.

**To run the design system**
- A static file server (the design system is plain HTML/CSS/JSX, no bundler). Any of:
  - `python -m http.server` (ships with Python 3)
  - `npx serve` (Node)
  - VS Code **Live Server** extension
- A modern browser. React, ReactDOM, and Babel-standalone are loaded from `unpkg` CDN at runtime, so **first load needs internet** (after that the browser caches them). If you must run air-gapped, vendor those three scripts locally and rewrite the `<script src>` tags.

**To build the product**
- An AI coding agent. Any capable agent works.
- Your chosen implementation stack. The SADs specify *requirements and architecture*, not a mandated language; `SAD_05`'s build-capabilities register and each SAD's C4 diagrams guide technology choices.

> **Why a server and not just double-clicking the HTML?** The UI kits and docs load component source with `XMLHttpRequest` and compile it with Babel in the browser. `file://` blocks those requests; serve over `http://` and everything works.

---

## 3. Which documents to read first

Read in this order. Don't skip `SAD_00` or `SAD_05` — they frame everything else.

| Order | Read | Why first |
|---|---|---|
| 1 | [`docs/dev/README.md`](docs/dev/README.md) | The index. What each SAD is, the methodology, the provenance-tag scheme. |
| 2 | [`docs/dev/markdown/SAD_00_Midgard_Core.md`](docs/dev/markdown/SAD_00_Midgard_Core.md) | The enclosure. Identity, object fabric, PQ crypto, governance — every product sits on this. |
| 3 | [`docs/design/readme.md`](docs/design/readme.md) | The design system in full: tokens, components, theming, governance chrome, voice. |
| 4 | [`docs/dev/markdown/SAD_05_Master_Prompt_and_Acceptance_Spec.md`](docs/dev/markdown/SAD_05_Master_Prompt_and_Acceptance_Spec.md) | The master: the deployable prompt library, agent-flow architecture, and **all 72 acceptance criteria**. This is your definition of done. |
| 5 | The SAD for the product you're building | `SAD_01` Forge · `SAD_02` Mimir (+ `SAD_02b`) · `SAD_03` Bifrost · `SAD_04` Heimdall · `SAD_06` Clients · `SAD_07` Admin. |

Each product SAD is self-contained and follows the same arc42/C4 structure: executive summary → drivers → requirements → architecture (context / containers / components) → security → deployment → acceptance. The matching diagrams live in `docs/dev/diagrams/<product>_*` (context, containers, component, ER, sequence, deployment, threat DFD, journey, CI/CD).

---

## 4. Your first prompts (in order)

Below are copy-pasteable starter prompts for an agent working in this repo. Run them in sequence; each builds on the context of the last. Adjust the product name and requirement IDs to your target.

### Prompt 0 — Orient the agent

```
Read docs/dev/README.md and docs/dev/markdown/SAD_00_Midgard_Core.md.
Then read docs/design/readme.md.
Summarise, in your own words: (a) what the Midgard enclosure guarantees,
(b) the five products and their accents, (c) how the design system's theme
scopes and governance chrome work. Do not write any code yet.
```

This grounds the agent in the enclosure and the brand before it touches anything.

### Prompt 1 — Load the design guide

Point the agent at the design system's own instructions so it becomes brand-expert:

```
/midgard-design
```

Otherwise, point the agent at the skill's instructions manually:

```
Read docs/design/SKILL.md and docs/design/readme.md, then load the tokens in
docs/design/tokens/ and the component contracts in docs/design/components/**/*.d.ts
and **/*.prompt.md. You are now the Midgard design authority. Confirm the rules
you must never break (one accent per screen, never emoji, ALL-CAPS mono labels,
provenance tags as chips, ProductMark instead of invented logos).
```

### Prompt 2 — Scope one product

```
We are building <Bifrost>. Read docs/dev/markdown/SAD_03_Bifrost.md end to end
and the diagrams docs/dev/diagrams/bifrost_*.mmd. Produce:
1. A component/container breakdown from the C4 diagrams.
2. The ordered list of functional requirements [FR-B-*] with a one-line build note each.
3. The acceptance criteria for Bifrost from SAD_05 Part C.
Propose an implementation plan and the tech stack you'll use, with trade-offs.
Do not write code until I approve the plan.
```

### Prompt 3 — Build the first vertical slice

```
Implement the first requirement, [FR-B-1] <release channel promotion>, as a
vertical slice: data model, service, API, and UI. For the UI, use the Midgard
design system under the .theme-bifrost scope — reuse the components in
docs/design/components/ (Button, Table, StatusPill, ProvenanceTag,
ClassificationBanner, HumanGate…) and match the proposed screen in
docs/design/ui_kits/bifrost/. Tag every artefact with its requirement id in
comments and commit messages. Then tell me how to run and verify it.
```

### Prompt 4 — Verify against the spec

```
For [FR-B-1], list the relevant acceptance criteria from SAD_05 and write tests
that exercise each. Run them. For anything AI-driven, wire the matching prompt
from SAD_05 Part A and its starter Evals. Report pass/fail with the [FR]/[AC]/[EV]
ids, and show the actual output — don't just assert success.
```

From here you loop Prompts 3–4 across requirements, then move to the next product with Prompt 2.

> **Tip:** keep one product per working session/branch. The design system enforces *one accent on screen at a time*; mirror that discipline in your build so context stays clean.

---

## 5. Running the design system locally

The design system is the fastest thing in the repo to see working. Serve `docs/design/` and open it in a browser.

```bash
# from the repo root
cd docs/design
python -m http.server 8080
#   → open http://localhost:8080/docs/index.html   (the documentation site)
```

Or with Node:

```bash
npx serve docs/design -l 8080
#   → http://localhost:8080/docs/index.html
```

**Where to look:**

| URL (under the served `docs/design/` root) | What it is |
|---|---|
| `docs/index.html` | Design-system home — overview & principles |
| `docs/foundations.html` | Colour (all three modes), type, spacing, shape, motion, icon grid, voice |
| `docs/components.html` | All 28 components: live variants, states, props |
| `docs/patterns.html` | Console anatomy, theming, governance chrome, density, terminal/empty states |
| `docs/screens.html` | The five product consoles embedded |
| `docs/flows.html` | The governed user flows, with gate markers |
| `ui_kits/midgard-console/index.html` | Object Explorer (Midgard core console) |
| `ui_kits/forge/index.html` | Pipeline Builder (Forge) |
| `ui_kits/mimir/index.html` | Agent Studio (Mimir) |
| `ui_kits/bifrost/index.html` | Release Console (Bifrost) |
| `ui_kits/heimdall/index.html` | Operating Picture (Heimdall) |
| `ui_kits/admin/index.html` | Admin control panel (SAD-07) |
| `ui_kits/clients/mobile.html` · `desktop.html` · `web.html` | The client layer (SAD-06) |
| `site/index.html` | The marketing website for the suite |

Use the **mode toggle** (top-right in docs and site) to check every surface in dark / light / system. The UI kits are interactive — collapse the side nav to an icon rail, collapse the inspector, resize below 900px to see the mobile drawers, promote a Bifrost release, select fleet rows, and so on.

---

## 6. The design system, in depth

Full reference: **[docs/design/readme.md](docs/design/readme.md)**. The essentials you must internalise before building any UI:

### Consume it

One CSS entry point pulls in every token:

```html
<link rel="stylesheet" href="/path/to/docs/design/styles.css">
```

`styles.css` imports `tokens/fonts.css`, `colors.css`, `typography.css`, `spacing.css`, `effects.css`, and `base.css`. Components are React (`.jsx`) with `.d.ts` type contracts and a `.prompt.md` usage note each. `ds-loader.js` compiles the `.jsx` sources in the browser for the cards/kits/docs; in production you'd bundle them with your app's toolchain instead.

### Theme scopes (one accent per product)

Put a scope class on a container (usually `<body>` or the console root). Every semantic `--accent-*` token re-points to that product's hue — components need no per-product variants:

```html
<body class="theme-bifrost">   <!-- bridge violet -->
```

Scopes: `.theme-midgard` (enclosure red, default) · `.theme-forge` (ember) · `.theme-mimir` (well teal) · `.theme-bifrost` (bridge violet) · `.theme-heimdall` (watchman gold). **Exactly one accent per screen** — the product you're inside. Status colours (`--secure`, `--warn`, `--info`, `--danger`) are for state only, never decoration.

### Colour modes

Three modes, resolved at runtime and persisted in `localStorage('mg-mode')` by `docs/mode.js`:

- **dark** (default — the platform lives in the dark),
- **light** (`.mode-light` on `<html>`/`<body>` — a paper ramp replaces the ink ramp, the grey text ramp inverts, and `--accent-hover` darkens instead of lightening),
- **system** (follows `prefers-color-scheme`).

Semantic aliases re-point per mode, so components adapt unmodified. Classification-marking colours are **fixed** across theme and mode.

### The 28 components

| Domain | Components |
|---|---|
| `core` | `Button`, `IconButton`, `Badge`, `Tag`, `StatusPill` |
| `forms` | `Input`, `Select`, `Switch` |
| `data` | `Card`, `Table`, `StatBlock`, `SectionLabel`, `Terminal`, `ProvenanceTag`, `MeterBar` |
| `nav` | `Tabs`, `SideNav` |
| `governance` | `ClassificationBanner`, `MarkingChip`, `HumanGate` |
| `clients` | `SyncStatus`, `TierBadge`, `KeyCustodyChip` |
| `admin` | `RiskBadge`, `GuardedAction`, `BreakGlass` |
| `media` | `Icon` (Lucide wrapper), `ProductMark` (typographic logo stand-in) |

Each `.prompt.md` is written to be handed straight to an agent. Example (`Button`):

> Button is the action primitive: accent-filled primary (max one per view), ink secondary, accent outline, and bare ghost. Sizes sm/md/lg. Primary glows on hover; press nudges 1px down. The accent follows the active `.theme-*` scope.

### Governance chrome (non-negotiable in operator UIs)

These aren't decoration — they encode platform requirements (`NFR-SEC-2`):

- **`ClassificationBanner`** — top-of-screen classification banner, configurable per deployment (for example `OFFICIAL`).
- **`MarkingChip`** — marking-based access control tags.
- **`ProvenanceTag`** — the `[DD-1]` / `[NFR-PQ-2]` chips; the suite's core rhetorical device, promoted to UI.
- **`HumanGate`** — the human-in-the-loop approval marker on any state-changing action.
- Admin: **`RiskBadge`** (risk classes 1–5), **`GuardedAction`** (dual-control), **`BreakGlass`** (emergency elevation).

### Voice & house devices

Engineered, declarative, restrained — "an architecture document that grew a UI." ALL-CAPS mono for labels/eyebrows/nav/headers; sentence case for body; Title Case for product & screen names. Numbered section markers (`01 //`), terminal prompts (`> forge build --branch main`), trust seals (`PQ-SEALED · ML-KEM-1024 · NO EGRESS`), middot separators, arrow CTAs (`Open the fabric →`). **Emoji: never.** Fonts: Oxanium (display) / Space Grotesk (sans/body) / JetBrains Mono (labels, prompts, code).

---

## 7. The prompt library (building Mimir's AI)

Every AI touchpoint in the suite — Mimir's agents, the admin advisory copilot, the read-only audit assistant, grounding/query formulation — has a **deployable, verbatim system prompt** in `SAD_05` Part A. Don't write these from scratch; wire in the specified text.

Each entry gives you: (1) the identifier and where it runs, (2) the system prompt verbatim, (3) the runtime-injected context variables (`{{POLICY}}`, `{{GROUNDING}}`, `{{TOOLS}}`, `{{OUTPUT_CONTRACT}}`, …), (4) a binding output contract (usually strict JSON), and (5) a starter Evals set.

Global conventions apply to every prompt (Part A.0):

- **Output contract is binding** — where a JSON schema is given, return only valid JSON, no prose, no fences.
- **Grounding over recall** — every factual claim about tenant data must be grounded in injected OAG context and cited by object id.
- **Least privilege** — an agent uses only the tools injected this turn; never assume a tool exists.
- **Fail visible, not silent** — return a structured refusal with a reason code, never a fabricated result.
- **No secrets, no PII echo** — never emit data above the caller's clearance.

Prompts are **model-portable** across the three supported families (Mistral / Nemotron / Gemma / Granite); family-specific chat templating is applied by the Model Abstraction Layer, not by the prompt author (`SAD_02b` covers model supply & resolution). When you implement an AI feature, implement its prompt *and* its Evals together — the Evals are your regression suite for behaviour.

---

## 8. The full build path

A pragmatic order for standing up the suite from this repo:

1. **Enclosure first (`SAD_00`).** Identity, the object/ontology fabric, storage, the governance service, and the PQ crypto substrate. Everything else depends on these; nothing else is testable without them.
2. **Governance & audit spine.** The append-only, PQ-signed audit ledger and the marking/clearance model — the design system's governance chrome maps directly onto these APIs.
3. **Mimir (`SAD_02` + `SAD_02b`).** The local inference plane and agent runtime, wired to the `SAD_05` prompt library. Every other product's AI calls Mimir, so it comes early.
4. **Forge (`SAD_01`).** Data operations feeding the fabric — you need data in the fabric for Heimdall and Mimir to be interesting.
5. **Bifrost (`SAD_03`).** PQ-signed artefacts, ring rollouts, offline bundles — the delivery mechanism for everything, including Mimir's signed model artefacts.
6. **Heimdall (`SAD_04`).** Operational fusion on top of a populated fabric.
7. **Admin Control Panel (`SAD_07`).** Composes the subsystem admin APIs above — build it once those exist; it must never reimplement them, and it is bound by the same policy/audit it administers (no privileged bypass).
8. **Client Platforms (`SAD_06`).** Mobile / desktop / web against the now-stable core APIs, offline-first with key-custody awareness and tiered inference.

Within each product, work the vertical-slice loop from [§4](#4-your-first-prompts-in-order): one requirement → model + service + API + UI → tests + Evals → verify → commit with provenance tags → next requirement.

Consult the diagrams as you go — for any product, `docs/dev/diagrams/<product>_c4_containers`, `_c4_component`, `_er`, `_seq_primary`, `_deployment`, and `_threat_dfd` give you the structural, data, sequence, deployment, and security-threat views respectively.

---

## 9. Acceptance criteria & definition of done

`SAD_05` Part C is the **consolidated acceptance-criteria matrix — 72 criteria across the whole suite** (core, the four products, model supply, client platforms, and the admin panel; the admin panel alone contributes 13). Treat it as the contract for "done."

For each requirement you implement:

1. Find its acceptance criteria in `SAD_05` Part C (and the per-product SAD's acceptance section).
2. Write tests that exercise each criterion — reference the `[AC-*]` / `[FR-*]` / `[NFR-*]` id in the test name.
3. For AI features, run the matching **Evals** from `SAD_05` Part A (e.g. `EV-A1-ground`, `EV-A1-ungrounded`, `EV-A1-policy`) — these check grounding, refusal codes, schema conformance, and gate behaviour.
4. Report results with the ids and the real output. Don't mark something done on an unverified assertion.

A feature is done when its functional criteria pass, its NFR criteria (PQ crypto, sovereignty/no-egress, least privilege, audit) hold, and — if it's user-facing — it matches the design system under the correct theme scope with the required governance chrome.

### Running the acceptance gates

The 72 criteria are wired into runnable gates under `deploy/acceptance/` — one `verify-<phase>.sh` per phase, each mapping its `AC-*` ids to concrete tests and failing closed on any miss. Run a single phase, or the whole suite fully air-gapped:

```bash
# whole suite, offline — bakes the pinned toolchain image, runs under --network none
bash deploy/acceptance/run-airgapped.sh
#   → tail: ">>> FULL SUITE PASSED, AIR-GAPPED (--network none)."

# a single phase (inside the pinned container, from the workspace root)
bash deploy/acceptance/verify-core.sh     # AC-M-1..9 ; also demonstrates AC-M-5 air-gap
bash deploy/acceptance/verify-mimir.sh    # AC-MI-1..8 + AC-MS-1..9 (+ EV-A1..A5, A9 per family)
```

Running the whole suite under `--network none` is the sovereignty proof: the platform builds and every criterion verifies with the network physically cut (`NFR-PRV-1` / `AC-M-5`). See [README.md → Air-gapped acceptance verification](README.md#air-gapped-acceptance-verification) for the toolchain details and why the image must bake `rustfmt`/`clippy`.

---

## 10. House rules & conventions

Carry these into every prompt, commit, and PR:

- **Provenance or it didn't happen.** Reference the `[DD-*]` / `[FR-*]` / `[NFR-*]` / `[STD:*]` id in code comments, tests, and commit messages. It's how the suite stays traceable to spec.
- **Sovereign by default.** No feature may require an outbound network call to function. If you add a dependency, it must work air-gapped; surface any egress as an explicit, governed choice.
- **Post-quantum visible.** Crypto state (seals, signatures, verification) is UI, not a buried setting. Use the trust-seal and `StatusPill` devices.
- **Human-gated mutations.** State-changing actions are *proposed* and pass a `HumanGate` / dual-control (`GuardedAction`) where the spec requires it. Agents never assert a mutation happened — they propose and report the gate's outcome.
- **One accent per screen.** Work inside a single `.theme-*` scope. Status colours are state-only.
- **Never emoji.** Unicode micro-icons (`→ ↓ ↗ · // >`) and Lucide line icons only. Product names render as `ProductMark` type — no invented logos.
- **These are specs, not history.** The SADs describe intended design. If a memory or note claims a file/flag exists, verify it in the tree before relying on it.

---

*Questions the docs don't answer belong in the SADs first (`docs/dev/markdown/`), then the design guide (`docs/design/readme.md`). When in doubt, ground your prompt in a specific requirement id and let the agent cite it back.*
