# Midgard Platform Design System

> **The sovereign enclosure.** The design system for Midgard, Veldris Ltd's sovereign, open-source, post-quantum platform suite, and its four child products: **Forge**, **Mimir**, **Bifrost**, and **Heimdall**.

Midgard is the core platform ("the enclosure"): identity, the object/ontology fabric, storage, compute, governance, and post-quantum cryptography by default. Atop it sit four independently useful, composable products, each independently useful:

| Product | Domain | Accent |
|---|---|---|
| **Midgard** | Core platform, identity, object fabric, governance | Enclosure red `#CA1D25` |
| **Forge** | Data operations, ingest, transform, version, project to fabric | Ember orange `#D96E1E` |
| **Mimir** | Local agentic AI (Mistral / Nemotron / Gemma / Granite), OAG, agents, Evals | Well teal `#23A8B6` |
| **Bifrost** | Continuous delivery, releases, rings, fleet, offline bundles | Bridge violet `#7D5CE6` |
| **Heimdall** | Operational fusion, feeds, common picture, decision workflows | Watchman gold `#C99A22` |

The visual language extends the **Veldris** house style; the suite shares Veldris's "Security by Design" lineage, and the *Seal* PQ-encryption action is Veldris's. Pure black surfaces, ink ramp, monospace command prompts, numbered section markers, blueprint grids, extended here with a five-hue product accent system and governance chrome (classification banners, markings, provenance tags).

---

## Sources

- **Seven Solutions Architecture Documents** (Veldris Ltd, v1.0, 2026-07-02, CONFIDENTIAL, greenfield design specs): `uploads/SAD_00_Midgard_Core.docx`, `SAD_01_Forge.docx`, `SAD_02_Mimir.docx`, `SAD_03_Bifrost.docx`, `SAD_04_Heimdall.docx`, `SAD_06_Client_Platforms.docx` (native mobile / desktop / web client layer), `SAD_07_Admin_Control_Panel.docx` (unified admin console: dual-control, break-glass, config-as-code, advisory copilot). Plain-text extractions live in `extracted/`.
- **Veldris Design System** (bound to this project): the visual-style parent. Ink ramp, house red, Oxanium / Space Grotesk / JetBrains Mono, terminal + label devices.
- No product UI exists yet (greenfield): the UI kits here are **proposed designs** derived from the SADs' functional requirements, not recreations.
- **No logos were provided** for Midgard or any child product (Midgard is built by Veldris Ltd; the SAD documents were authored by Veldris Ltd). No marks were invented: wherever a logo would sit, the product name is set in plain type (see `ProductMark`, a purely typographic tile). Supply real marks and we'll wire them in.

---

## Content fundamentals

**Tone.** Engineered, declarative, restrained. The suite speaks like an architecture document that grew a UI: every claim has provenance, every action has lineage. It sells *sovereignty* and *trust*, not features. Statements are short and verifiable.

**Voice & pronoun.** Corporate "we" is rare; the platform mostly speaks in the imperative and the passive-verified ("Artefact verified", "Policy enforced at query time"). Direct "you" for the operator. First-person terminal prompts (`> whoami`, `> midgard status`) are a house device.

**Casing.**
- **ALL-CAPS mono** for eyebrows, section labels, nav, table headers, stat labels, classification banners (configurable per deployment, for example `OFFICIAL`).
- **Sentence case** for body copy, descriptions, empty states.
- **Title Case** for product names and screen titles ("Object Explorer", "Release Channels").

**Signature devices (deliberate, not everywhere):**
- Provenance tags from the SADs (`[DD-4]`, `[NFR-PQ-2]`, `[FR-H-1]`, `[STD:NIST FIPS 203]`), rendered as mono chips (`ProvenanceTag`).
- Numbered section markers: `01 //`, `02 //` (`SectionLabel`).
- Terminal prompts: `> forge build --branch main`, `fn crypto()`.
- Classification banners top-of-screen, configurable per deployment (for example `OFFICIAL`).
- Trust seals: `PQ-SEALED · ML-KEM-1024 · NO EGRESS`, `AIR-GAP READY`.
- Big stat counters with mono labels: `99.9% / CONTROL-PLANE SLO`.
- Terse directional cues: `Explore the fabric →`.

**Vocabulary.** enclosure, sovereign, air-gapped, post-quantum, ML-KEM, ML-DSA, seal/sealed, object fabric, semantic + kinetic, lineage, provenance, marking, purpose, zero-trust, human-gated, self-learning/healing/defending, zero-downtime, ring, bundle, fused picture. Prefer *protect* and *govern* over vague *secure*.

**Punctuation.** Middot separators (`·`), arrows (`→ ↓ ↗`) for actions, slashes in section numbers (`//`), square brackets for provenance (`[DD-1]`). **Emoji: never.**

**Example copy that's on-brand:**
- Eyebrow: `02 // OBJECT FABRIC`
- Headline: "Decisions, not just data."
- Body: "Every read, write, and agent action is recorded in an append-only, PQ-signed lineage. Nothing leaves the enclosure."
- CTA: "Open the fabric →"
- Status: `SEALED · ML-KEM-1024` / `HUMAN GATE PENDING`

---

## Visual foundations

**Colour.** Black + a cool ink ramp (`--ink-950 → --ink-500`) for every surface; cool grey text ramp. **Three colour modes**: dark (default, the platform lives in the dark), light (`.mode-light` on `<html>`/`<body>`, a paper ramp replaces ink and the grey text ramp inverts), and system (resolved live from `prefers-color-scheme` by `docs/mode.js`, persisted in `localStorage('mg-mode')`). Semantic aliases re-point per mode so components adapt unmodified. **One accent hue per product**, at matched chroma, re-pointed through semantic `--accent-*` aliases by a theme scope class (`.theme-forge`, `.theme-mimir`, …; default Midgard red); in light mode `--accent-hover` darkens instead of lightening. *One accent on screen at a time*: the product you're inside. A red LED becomes an ember, a teal well, a violet bridge, a gold eye. Functional colours (`--secure`, `--warn`, `--info`, `--danger`) are status-only. Classification markings have fixed colours regardless of theme or mode. No decorative gradients; the only glow is the accent halo on primary CTAs and focus.

**Type.** `--font-display` (Oxanium) for headings and big numerics; `--font-sans` (Space Grotesk) for UI/body; `--font-mono` (JetBrains Mono) for labels, prompts, code, table meta, provenance. Consoles default to `--fs-sm` (14px) body density. Headlines tight-tracked and heavy; mono labels wide-tracked ALL-CAPS (`--ls-label`).

**Spacing & layout.** 4px grid. Marketing surfaces use generous `--section-y`; operator consoles are dense (`--row-h` 40px, compact 32px, `--toolbar-h` 52px, `--sidenav-w` 232px). Left-aligned starts, numbered markers, asymmetric composition. Console anatomy: classification banner → top bar → side nav → work canvas → inspector rail.

**Backgrounds.** Solid black/ink. Faint blueprint grid (`.mg-grid-bg`, 48px cells) behind heroes, empty canvases, and map/graph surfaces. No photography by default; imagery reads cool, high-contrast, near-monochrome with a single accent spot.

**Borders & corners.** Hairline `--border` defines surfaces; elevation = lighter surface + stronger border, not drop shadows. `--bw-2` accent rules mark active/selected. Radii crisp: `--radius-md` 10px default, `--radius-lg` 16px panels, pills only for status chips/tags/markings.

**Elevation.** page `--ink-900` → card `--ink-800` → raised/hover `--ink-700`; wells/code sink to `--ink-850/950`. Real overlays (menus, dialogs) may use `--shadow-md/lg`.

**Transparency & blur.** Sticky chrome and floating panels only (`--blur-panel`); never decorative.

**Motion.** Precise and quick: `--ease-out`, 120–360ms, fades + short translate-ups; accent-glow bloom on focus. No bounce, no elastic. Respect `prefers-reduced-motion`.

**Interaction states.**
- *Hover:* surface lightens one ink step; accent shifts to `--accent-hover`. Never opacity-dim.
- *Press:* accent deepens to `--accent-press`; 1px downward nudge, no scale-shrink (small icon buttons may scale 0.96).
- *Focus:* `--ring-focus` (3px accent halo).
- *Disabled:* `--text-faint` on `--surface-inset`, `cursor: not-allowed`, no accent.
- *Selected/active:* `--bw-2` accent rule or `--accent-tint` fill.

**Cards.** Flat `--ink-800`, 1px `--border`, `--radius-md/lg`, no shadow at rest; hover lightens + optional hairline accent top-rule. Content-first, low chrome.

---

## Iconography

- **Line icons, single stroke ~1.75–2px, no fill.** No icon assets exist in the sources; the system uses **[Lucide](https://lucide.dev)** from CDN, wrapped by the `Icon` component (the same substitution the parent Veldris system makes). *Flagged: swap for a licensed set if one exists.*
- Domain glyphs carry meaning. Platform: `lock`, `shield-check`, `key-round`, `boxes`, `network`, `database`, `server`, `terminal`, `fingerprint`; Forge: `hammer`, `git-branch`, `workflow`, `table`; Mimir: `brain-circuit`, `bot`, `sparkles`, `gauge`; Bifrost: `rocket`, `package`, `radio-tower`, `undo-2`; Heimdall: `eye`, `radar`, `map`, `crosshair`, `layers`.
- **Emoji: never.** Unicode glyphs as micro-icons are on-brand: `→ ↓ ↗ · // >`.
- **No logos exist** (see Sources). `ProductMark` renders a typographic initial tile + wordmark; it is intentionally plain type, not a drawn mark.

---

## Index / manifest

**Root**: `styles.css` (global entry; consumers link this), `readme.md` (this guide), `SKILL.md` (Agent-Skills entry point), `ds-loader.js` (component namespace loader for cards/kits/docs).

**`docs/`**: the multi-page documentation experience for designers & engineers, with a light/dark/system mode toggle. `index.html` (overview & principles), `foundations.html` (colour incl. both mode ramps, type, spacing, shape, motion, full icon grid, voice), `components.html` (all 28 components: live variants, states, props), `patterns.html` (console anatomy, theming, governance chrome, density, terminal/empty states), `screens.html` (the five consoles embedded), `flows.html` (twenty-eight governed user flows with gate markers). `mode.js` is the mode bootstrap; `shell.jsx` the shared docs chrome.

**`tokens/`**: `fonts.css`, `colors.css` (ink ramp, 5 product accents + theme scopes, status, markings), `typography.css`, `spacing.css` (incl. console density), `effects.css`, `base.css` (helpers `.mg-label`, `.mg-grid-bg`).

**`guidelines/`**: foundation specimen cards (Design System tab).

**`components/`**
- `core/`: `Button`, `IconButton`, `Badge`, `Tag`, `StatusPill`
- `forms/`: `Input`, `Select`, `Switch`
- `data/`: `Card`, `Table`, `StatBlock`, `SectionLabel`, `Terminal`, `ProvenanceTag`, `MeterBar`
- `nav/`: `Tabs`, `SideNav`
- `governance/`: `ClassificationBanner`, `MarkingChip`, `HumanGate`
- `clients/`: `SyncStatus`, `TierBadge`, `KeyCustodyChip` (client-layer chips: offline-first sync, AI tiers, key custody)
- `admin/`: `RiskBadge`, `GuardedAction`, `BreakGlass` (admin panel: risk classes 1\u20135, dual-control approvals, emergency elevation)
- `media/`: `Icon`, `ProductMark`

**`ui_kits/`**: the five product consoles, 35 interactive screens total, all mobile-responsive with collapsible panels: the side nav collapses to an icon rail (toggle in the top bar, persisted), the inspector rail collapses fully, and below 900px both become overlay drawers (proposed greenfield designs from the SADs). `kit-shell.jsx` is the shared console frame (banner, top bar with seals, side nav with real cross-page links, inspector rail).
- `midgard-console/`: Object Explorer (`index.html`), Search-around, Identity & Access, Keys & Crypto, Policies & Markings, Audit Lineage, Compute, SDK & APIs, Observability.
- `forge/`: Pipeline Builder (`index.html`), Connectors, Datasets, Data Health, Object Projection, Schedules & Triggers, Transform IDE.
- `mimir/`: Agent Studio (`index.html`), Evals, Guardrails, Model Plane, Logic Builder, Session Traces, Governed Feedback.
- `bifrost/`: Release Console (`index.html`), Artefacts, Channels, Offline Bundles, Rollback Centre, Fleet & Drift.
- `heimdall/`: Operating Picture (`index.html`), Feeds, Entities, Action Workflows, Decision Lineage, Picture Sync.
- `admin/`: the admin control panel (SAD-07), eight screens: `index.html` (suite oversight), `approvals.html` (dual-control queue with live separation-of-duties denial), `iam.html` (role-separated RBAC, no super-admin), `config.html` (GitOps config: diff \u00b7 simulate \u00b7 commit \u00b7 rollback), `breakglass.html` (emergency lifecycle), `registry.html` (model registry / agent governance / Evals gates / release administration, FR-AD-4), `ledger.html` (cross-subsystem audit search with the read-only investigation assistant), `copilot.html` (advisory-only copilot with refusal codes).
- `clients/`: the client layer (SAD-06), seven targets in three screens: `mobile.html` (iOS / Android / Linux bezels over one shared screen), `desktop.html` (Tauri app with macOS / Windows / Linux chrome), `web.html` (WASM web app + broker-mode extension popup).

**`site/`**: the marketing website for the suite · `index.html` (midgard.eu suite home), `forge.html` / `mimir.html` / `bifrost.html` / `heimdall.html` (product subdomain pages, each in its own theme scope), `pricing.html` (open-core tiers + plain-answer FAQ), `architecture.html` (public layer-by-layer architecture summary), `github.html` (source page: official veldris-ltd GitHub repos, expectation-setting, verify-and-build instructions), `about.html` (Veldris Ltd, mission & commitments), `contact.html` (channels incl. DPO + security disclosure, local-sealed contact form), `privacy.html` (GDPR/UK-GDPR privacy notice: controller, storage register, lawful bases, all data-subject rights), `terms.html` (website terms), `legal.html` (legal notice / imprint with placeholder registration fields), `site-shell.jsx` (shared chrome: domain-aware header with burger fallback and a light/dark/system mode toggle, early-access signup that persists locally and issues a sealed receipt, GDPR storage-notice banner, footer with company/legal columns), `fabric-bg.js` (the interactive "object fabric" canvas backdrop behind every page: drifting nodes + hairlines in the page accent, pointer-linked, click-pulsed, scroll-parallaxed; mode-aware, honours `prefers-reduced-motion`). Marketing register of the same language: blueprint-grid heroes, terminal proof blocks, stat counters, trust seals; one accent per page.

**`extracted/`**: plain-text SAD extractions.

---

## Intentional additions

No component inventory exists in the sources (greenfield), so the set is authored from the SADs' own devices plus the parent Veldris kit:
- **`ProvenanceTag`**: the `[DD-1]` / `[NFR-PQ-2]` provenance chips; the documents' core rhetorical device, promoted to UI.
- **`ClassificationBanner`** / **`MarkingChip`**: classification and marking-based access control are platform-level requirements (`NFR-SEC-2`), so they get dedicated chrome.
- **`Terminal`**, **`SectionLabel`**, **`StatBlock`**, **`StatusPill`**: inherited Veldris house devices.
- **`Table`**, **`Tabs`**, **`SideNav`**, **`Select`**, **`Switch`**: the products are data-dense operator consoles; these are the minimum console furniture.
- **`ProductMark`**: typographic stand-in for absent logos.
