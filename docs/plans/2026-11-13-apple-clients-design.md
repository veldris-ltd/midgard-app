# Apple clients (macOS and iOS): design

Date: 2026-11-13. Base: signed release `v0.1.0-rc.3` (`ea5899ab`).
Sources: `docs/design/extracted/SAD_06_Client_Platforms.txt`, `clients/README.md`,
`clients/shells/README.md`, `clients/shells/apple/manifest.toml`, `products/clients/ui`.

## Goal

Native Midgard clients for macOS and iOS with full parity with the six release consoles
(Midgard, Heimdall, Forge, Mimir, Bifrost, Admin), built as SAD_06 specifies: one
Swift/SwiftUI shell (`clients/shells/apple`) over the shared Rust core
(`midgard-client-core`), bound with UniFFI. Version 1 runs on a bundled, signed demo
dataset; no server connection.

## Decisions

| Decision | Choice | Source |
|---|---|---|
| Shell technology | Swift / SwiftUI for both macOS and iOS | SAD_06 §5.1, `apple/manifest.toml` |
| Core binding | UniFFI over `midgard-client-core` | SAD_06 KDD-1, DD-6 |
| Security logic | Only in the Rust core; Swift never handles crypto, keys or sync merge | KDD-1, FR-C-1 |
| Scope v1 | Full parity with the six release consoles (42 screens) | User, 13 Nov |
| Data | Bundled, signed demo dataset; no server | User, 13 Nov |
| Signing | Apple Developer account; automatic signing; the user signs in to Xcode | User, 13 Nov |
| Branches | `clients_shared` from `v0.1.0-rc.3`; `clients_mac` and `clients_iphone` from `v0.1.0-rc.3`, each merging `clients_shared` | User, 13 Nov |
| Publishing | Local branches only until the user says otherwise | User, 13 Nov |

## 1. Structure

**Branches.** `clients_shared` holds everything both apps use. `clients_mac` and
`clients_iphone` start from `v0.1.0-rc.3`, merge `clients_shared`, and add only their
own app target. Signed commits.

**Layers.**

1. **Rust core (`clients/core`, extended).** Keeps sync, the sealed clearance-scoped
   cache and key custody, and gains read models for each console: Midgard objects and
   actions, the Heimdall picture and alerts, Forge jobs, Mimir sessions and asks, Bifrost
   releases and rings, Admin approvals, IAM and ledger.
2. **Demo enclosure (new, in the core).** A data source that serves the bundled demo
   dataset behind the same interface a real gateway connection would use. The apps do
   not know it is demo data; pointing them at a real server later is a configuration
   change.
3. **Bindings (`clients/ffi`, new crate).** UniFFI generates the Swift API. One
   XCFramework covers macOS (arm64 and x86_64), iOS devices and the iOS simulator.
4. **Apple key root.** A Secure Enclave key, gated by Face ID / Touch ID, wraps the
   core's data key. Swift implements only the core's key-root callback (seal and unseal
   inside the Enclave); key material never leaves it.
5. **`clients/shells/apple`.**
   - `MidgardKit`: a Swift package with the shared SwiftUI screens and the design
     system's colours and type.
   - The macOS app (on `clients_mac`) and the iOS app (on `clients_iphone`). Each shapes
     navigation to its platform.

## 2. Screens

| Console | Screens |
|---|---|
| Midgard (11) | Object Fabric, Data Search, Search Around, Policies, Identity, Keys, Audit, Compute, Observability, Resilience, SDK |
| Heimdall (6) | Operating Picture (MapKit: tracks, alerts, layers), Entities, Feeds, Picture Sync, Workflows, Decision Lineage |
| Forge (7) | Connectors, Datasets, Data Health, Pipeline Builder, Transform IDE, Schedules, Object Projection |
| Mimir (7) | Model Plane, Session Traces, Agent Studio, Logic Builder, Evals, Guardrails, Feedback |
| Bifrost (6) | Release Console, Channels, Fleet, Artefacts, Bundles, Rollback |
| Admin (8) | Overview, Approvals, IAM, Config, Ledger, Registry, Break-Glass, Copilot |

- **macOS:** sidebar to switch console, list or table, detail inspector; multiple
  windows, menus, keyboard shortcuts, Swift Charts, sortable tables.
- **iOS:** tab bar with the consoles; list → detail stacks, sheets, pull to refresh,
  search. The graph editors (Pipeline Builder, Logic Builder, Transform IDE) show the
  graph and let a user edit node settings; drawing new graphs is macOS only.
- **Actions** (approve, run a Forge job, promote a release, and so on) run against the
  demo enclosure through the core's offline queue and sync, as real edits would.
- **On every screen:** marking chips on each row; nothing above the user's clearance is
  cached or shown; sync-status and inference-tier chips; biometric lock after idle.
- **Build order:** shared base → Midgard and Admin → Heimdall → Forge → Mimir → Bifrost.
  Each console is finished on both apps before the next starts.

## 3. Data, security and errors

**Demo dataset.**
- Built from the release's deterministic demo fixtures (the ones the web consoles' demo
  mode uses), covering all six consoles, plus the recorded Heimdall hour.
- A build step turns them into one dataset file per release, signed with the release's
  demo key. The core verifies the signature when the app opens the dataset and refuses
  a tampered file.
- Bundled in the app; nothing is fetched.
- Contains no data from any local suite: no live feeds and no Mauritius Leaks data. The
  build step checks for this.

**Sign-in.** Demo personas with different clearances and roles (analyst, operator,
approver, release manager). Switching persona shows the picture change with clearance.
First use creates the Secure Enclave key (Face ID / Touch ID); each unlock needs
biometrics, with the device passcode as fallback.

**Local store.** The core seals everything cached with AES-256-GCM under a data key
wrapped by the Enclave key. Swift sees neither key. "Reset demo" wipes the store and the
key.

**Errors.** Core errors arrive through UniFFI as typed Swift errors: above clearance,
signature failure, conflict, locked, storage full. Screens show the matching state
(refused, retry, locked), never a crash. Above-clearance data shows as a "withheld" row.

**Offline.** The demo enclosure can simulate going offline: edits queue, the inference
tier drops to on-device, and reconnecting runs the merge.

## 4. Testing, release gate and delivery

- **Rust:** unit tests for the console read models, the demo enclosure, dataset
  signature and refusal, and the bindings, under the workspace's usual `cargo test`.
- **Contract test:** a small Swift test that calls every binding, so drift on either
  side fails.
- **Swift:** XCTest for each console's view models in `MidgardKit`; XCUITest for each
  console's main flows on both apps (sign in, browse, open detail, take an action, see
  it sync).
- **Accessibility gate:** XCUITest measures the seven affordances in
  `clients/shells/apple/accessibility.toml` on the running app and writes
  `clients/shells/apple/audits/<release>.toml` (schema: `clients/shells/AUDIT_SCHEMA.md`),
  run by `.github/workflows/shell-audit.yml` on a macOS runner and an iOS simulator, as
  `deploy/acceptance/verify-shell-audits.sh` requires.
- **Local builds:** output under `~/Library/Caches/midgard-build/clients-*`, deleted
  after each run. The iOS simulator runtime (about 8 GB) is installed after a disk-space
  check.
- **Signing and delivery:** automatic signing with the team ID. macOS: notarised DMG.
  iOS: TestFlight archive. Nothing is uploaded to Apple without the user's go-ahead each
  time.

## Out of scope for version 1

Connecting to a real server (the enclosure client is switched on later), iPad-specific
layouts (the iPhone app runs scaled on iPad), Android, Windows, web, and App Store
submission.
