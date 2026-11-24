# Midgard Apple clients

The macOS and iPhone apps (one of each, both the Maps edition), and the Swift package
they share. Both apps are thin
SwiftUI shells over the Rust core (`midgard-client-core`, exported through UniFFI by
`clients/ffi`); every key, store, sync and policy decision is the core's. How the
binding is built, and the release checklist for UniFFI, are in
[`../SHELL_BUILD.md`](../SHELL_BUILD.md#apple-built). [DD-6] [FR-C-1]

Every command below runs from the repository root unless it says otherwise, and every
build output goes under `~/Library/Caches/midgard-build/`, never into the tree. Each
local run ends with

```bash
bash clients/shells/apple/tools/clean.sh
```

so no `clients-*` build output is left behind (see [Cleaning up](#cleaning-up)).

## Branches

| Branch | Holds |
|---|---|
| `clients_shared` | the Rust core and FFI, this package (`MidgardKit`, `MidgardAuditKit`, `MidgardGlobeKit`, tests), `tools/`, `audit.sh`, this README |
| `clients_apple` | `clients_shared` plus `Apps/MidgardMac/` (the app `MidgardMacMaps`) and `Apps/MidgardiOS/` (the app `MidgardiOSMaps`), with the edition check, the projects test and the audit narrowed to them |

`clients_apple` replaces `clients_mac` and `clients_iphone` (owner decision, 23 November
2026: two apps, one branch); those two branches are kept, unchanged, until the owner
decides about them. Shared work is committed on `clients_shared` first and then merged
into `clients_apple` with `git merge --no-ff clients_shared` (signed). It is never
cherry-picked, so the shared base stays one history. The app sections below describe
files that are only on `clients_apple`.

## Layout

| Path (under `clients/shells/apple/`) | What it is |
|---|---|
| `Package.swift` | the Swift package: products `MidgardKit`, `MidgardAuditKit` and `MidgardGlobeKit`, macOS 14 and iOS 17 |
| `Sources/MidgardCore/` | the generated UniFFI bindings (`midgard_client_ffi.swift`, gitignored) over the `MidgardCoreFFI` XCFramework |
| `Frameworks/` | gitignored link to the XCFramework build in the cache |
| `Sources/MidgardKit/` | everything both apps share: session (`AppModel`, `AppEnvironment`, the UI-testing environment), Secure Enclave key root, storage paths, design tokens and fonts, the console catalogue, sign-in, lock, chips, banner, acknowledgements |
| `Sources/MidgardAuditKit/` | `AccessibilityAuditBase`, the accessibility audit each app's UI-test target subclasses |
| `Sources/MidgardHooks/` | C: the link-time hooks `MidgardKit` reads, as weak defaults (see [Edition](#edition)) |
| `Sources/MidgardGlobeKit/` | the edition library, registered at link time: Heimdall's MAP and 3D views and their map tiles (see [Heimdall's MAP and 3D views](#heimdalls-map-and-3d-views-phase-i)), the edition banner and About |
| `Tests/MidgardKitTests/` | the package tests, including `ContractTests` over every exported binding |
| `Tests/MidgardGlobeKitTests/` | the edition library's tests: linking it registers it; the globe's geometry, presets and camera against the web's values; tile maths, decoding, limits, caches and the loader on three recorded tiles (`Fixtures/`) |
| `Apps/MidgardMac/` | the macOS app (`clients_apple` only) |
| `Apps/MidgardiOS/` | the iPhone app (`clients_apple` only) |
| `audit.sh`, `accessibility.toml`, `audits/` | the on-device accessibility audit and the affordances it measures |
| `tools/` | build, check and clean-up scripts (below) |

Each app directory has `project.yml` (the XcodeGen spec), the generated
`<App>.xcodeproj` committed beside it, `Info.plist`, `Config/` (signing and
entitlements), `Sources/` and `UITests/`.

`tools/`:

| Script | Does |
|---|---|
| `ensure-xcframework.sh` | the apps' pre-build step: links an XCFramework built from the Rust sources as they stand, building only when their content or the toolchain changed |
| `build-xcframework.sh` | an unconditional build of the four slices and the bindings, verified by `verify-xcframework.sh` |
| `check-contract.sh` | every exported binding is called in `ContractTests.swift` |
| `check-bindings.sh` | the bindings are a fresh `uniffi-bindgen` run's; acknowledgement present; UniFFI licence exception not expired |
| `gen-tokens.mjs` | design tokens and fonts from `ui/design-system` (`--check` to compare only) |
| `typecheck-ios.sh` | typechecks the package for iOS without a simulator runtime |
| `check-edition.sh` | builds both apps and fails if one carries more or less than its edition allows (see [Edition](#edition)) |
| `audit-failures.js` | reads an audit result bundle's summary for `audit.sh` |
| `clean.sh` | deletes the local build outputs |

## Prerequisites

- A Mac with Xcode 26.2 (the version these apps were built with).
- XcodeGen 2.46 (`xcodegen --version`), only to regenerate a project after editing
  its `project.yml`.
- rustup. The repository pins the toolchain in `rust-toolchain.toml` (Rust 1.82.0);
  add the Apple targets to it:
  `rustup target add aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim --toolchain 1.82.0`.
- Node 22, only for `gen-tokens.mjs`.
- For the iPhone app: an iOS simulator runtime with an `iPhone 17` device
  (`xcrun simctl list devices available`). A runtime is about 8 GB; check `df -h ~`
  and the 10 GB suite budget with the owner before installing one.
- For UI tests on the Mac: Automation Mode. `automationmodetool` prints its state;
  `xcodebuild test` asks to turn it on the first time if it is off.

## First build

Xcode resolves Swift packages before any build phase runs, and `MidgardKit` depends on
the gitignored `Frameworks/` link, so on a fresh checkout or worktree run once:

```bash
bash clients/shells/apple/tools/ensure-xcframework.sh
```

After that the apps' pre-build phase keeps the framework current by itself. The
package on its own:

```bash
cd clients/shells/apple
swift test --scratch-path ~/Library/Caches/midgard-build/clients-swift
```

## Building and testing the apps

Each app has its own derived-data directory.

macOS:

```bash
xcodebuild build -project clients/shells/apple/Apps/MidgardMac/MidgardMac.xcodeproj \
    -scheme MidgardMacMaps -destination platform=macOS \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived-mac
xcodebuild test -project clients/shells/apple/Apps/MidgardMac/MidgardMac.xcodeproj \
    -scheme MidgardMacMaps -destination platform=macOS \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived-mac
```

iPhone:

```bash
xcodebuild build -project clients/shells/apple/Apps/MidgardiOS/MidgardiOS.xcodeproj \
    -scheme MidgardiOSMaps -destination 'platform=iOS Simulator,name=iPhone 17' \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived-ios
xcodebuild test -project clients/shells/apple/Apps/MidgardiOS/MidgardiOS.xcodeproj \
    -scheme MidgardiOSMaps -destination 'platform=iOS Simulator,name=iPhone 17' \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived-ios
```

With a team in `Signing.local.xcconfig` the first signed build may need
`-allowProvisioningUpdates`.

`test` runs the scheme's UI tests (`SessionFlowTests`, `MidgardFlowTests`,
`AdminFlowTests`, `HeimdallFlowTests`, `HeimdallPictureFlowTests`, `HeimdallMapsFlowTests`,
`ForgeFlowTests`, `MimirFlowTests`, `BifrostFlowTests`, `YourDataFlowTests`, `EditionTests` and `AccessibilityAudit`; add
`-only-testing:MidgardMacMapsUITests/HeimdallPictureFlowTests`, or
`MidgardiOSMapsUITests/...`, for one class). The UI tests launch the app with
`-MidgardUITesting`, which in Debug builds only selects a key root that never prompts and
a fresh temporary store; a Release build carries none of that code. The macOS scheme
skips `AccessibilityAudit/test_affordance_dynamic_type()`: Dynamic Type is measured on
iOS.

After editing a `project.yml`, run `xcodegen generate` in that app's directory and
commit the spec and the regenerated project together;
`bash tools/ci/tests/apple-projects.test.sh` fails when they differ, and when a project
does not declare its one app as below.

## Edition

There are two apps, one per platform, both the Maps edition (owner decision, 23
November 2026; it replaces the 22 November decision of three editions per platform, and
drops the release-only and 3D-only apps):

| | Mac | iPhone |
|---|---|---|
| Target and scheme | `MidgardMacMaps` | `MidgardiOSMaps` |
| UI-test target | `MidgardMacMapsUITests` (`UITests`, `UITests-Maps`) | `MidgardiOSMapsUITests` (`UITests`, `UITests-Maps`) |
| Bundle ID | `io.veldris.midgardmaps.mac` | `io.veldris.midgardmaps.ios` |
| Shown as | "Midgard" (`CFBundleDisplayName`; the product is `Midgard.app`) | "Midgard" (`CFBundleDisplayName`) |
| Info.plist `MidgardEdition` | `maps` | `maps` |
| Entitlements | `MidgardMacMaps.entitlements` (team), `MidgardMacMaps-AdHoc.entitlements`: App Sandbox, the client network entitlement and read access to the files the user picks (Your Data) | `MidgardiOSMaps.entitlements` (team), `MidgardiOSMaps-NoTeam.entitlements`; iOS has no network entitlement |
| App Transport Security | the default, plus `NSAllowsLocalNetworking` only (loopback HTTP for a gateway or feed on this Mac) | the default |

Both link `MidgardGlobeKit`. Content: the signed release `v0.1.0-rc.3` (`ea5899ab`),
plus Heimdall's MAP and 3D views with their map tiles (Phase I; the planned tag is
`apple-midgard3dmaps-v0.1.0-rc.3`). The tile loader lives in `MidgardGlobeKit` itself:
with the release-only and 3D-only apps gone there is no app it must be kept out of, so
the separate `MidgardTileKit` of the 22 November notes was not needed. The banners are "Demonstration data. …" (the web consoles' notice) and
"Maps edition, not in the signed release" (`mg.banner.editionmaps`).

Everything the edition shows lives in `MidgardGlobeKit`: the edition banner (at
accessibility text sizes the compact "Maps edition · not the signed release", with the
full text as its accessibility label), the About screen (Settings on the Mac, the session
menu on the iPhone; it names the edition and says it is not the signed release), and
Heimdall's MAP and 3D views behind the Operating Picture's view switch. MidgardKit only asks an
`EditionProvider` for them, and a package test holds its sources (and MidgardHooks') to
no edition wording, no `Globe` and no Metal import. A missing or unknown `MidgardEdition`
is the "app misconfigured" failed state, as with `MidgardDataSource`.

The provider is registered by a link-time hook, never a runtime switch: `MidgardHooks`
defines `midgard_edition_hook` weakly, returning nothing, and `MidgardGlobeKit` defines
it strongly, so a binary that links the library gets its provider. Nothing calls a
register function at launch. (The package keeps this indirection unchanged on
`clients_shared`.)

`tools/check-edition.sh` builds both apps (Debug) and checks each: its `MidgardEdition`
is `maps`; on the Mac it is sandboxed; its target signs with its own entitlements file;
the hook is defined strongly (the edition library is linked); and it has no network
entitlement beyond the client one (no server, `com.apple.developer.networking.*` or
temporary-exception entitlement), with the client one required on the Mac.
`check-edition.sh [--maps] <app>...` checks built bundles only; B4 and B5 run it so on
the signed Release archives. `audit.sh` runs it first and records nothing when it fails.
`tools/ci/tests/apple-edition.test.sh` runs it against bundles it builds with seeded
failures.

## Sample data

The apps show the signed release's own built-in sample data (owner decision, 22
November 2026): exactly what the `v0.1.0-rc.3` web consoles show in mock mode
(`sdk/ts/src/mock/`), with no feeds and no network of its own (the user may add
their own data, a gateway or feeds: [Your data](#your-data-phase-j)). Each app's `Info.plist` has
`MidgardDataSource = sample`, so:

- the data is `clients/demo/sample/<release>/sample.json`, compiled into the core
  (`SampleSource`); the app reads no file for it;
- it is extracted from the mocks by `clients/demo/tools/extract-sample.sh` (which
  extracts the fixtures afresh and drops what the mocks do not serve, the synthetic
  Heimdall hour). `extract-sample.sh --check` is the drift check: the committed copy must
  match a fresh extraction, and every mock it was read from must be the signed tag's
  copy. `tools/ci/tests/sample-data.test.sh` runs it in CI;
- the personas are the ones the mocks name (K. Rao, J. Benjamin, K. Osei, A. Holt), and
  the core's governance applies as it does to the demo dataset: every row keeps its
  marking (its own, or the committed overlay's), rows above a persona are withheld by
  level only, and every action is decided once and audited;
- actions behave as the web mocks do. Heimdall's `heimdall.authoriseWorkflow` (the
  Operating Picture's and Workflows' Authorise), ported from `HeimdallMock` (Phase E, see
  [Heimdall's screens](#heimdalls-screens-phase-e)): a human-gated workflow needs a gate
  token, and its audit row is numbered and stamped as `HeimdallMock` numbers them
  (`D-4473`, from 14:39:04). Midgard's, ported from `MockGateway` (Phase C, see
  [Midgard's screens](#midgards-screens-phase-c)), are numbered as it numbers them
  (`AUD-1043`, from 14:32:10), and Admin's, ported from `AdminMock` (Phase D, see
  [Admin's screens](#admins-screens-phase-d)), as it numbers its ledger (`A-9111`, from
  14:02:13, in the runtime `admin.audit`). The rows' wording stays the core's (`applied ·
  heimdall.authoriseWorkflow`), which the device's audit key checks before signing it.
  Forge's (Phase F) and Mimir's (Phase G, see [Mimir's screens](#mimirs-screens-phase-g))
  are numbered as `ForgeMock` and `MimirMock` number them (`FRG-2044` from 14:02:13;
  `A-77121` from 14:32:10, in the runtime `mimir.audit`), and Bifrost's (Phase H, see
  [Bifrost's screens](#bifrosts-screens-phase-h)) as `BifrostMock` numbers them
  (`AUD-B-1044` from 14:32:10, after its five seed records). One row the mocks do not hold is added:
  Mimir's Agent Studio exchange (`clients/demo/studio.json`, copied once from the web
  screen, served as `mimir.studioExchange`; `extract-sample.sh --check` holds that screen
  to the tag);
- the banner above everything is the web consoles' own demonstration notice,
  "Demonstration data. This console runs on built-in sample data and is not connected to
  an enclosure." (`mg.banner.demo`).

The other accepted values are `none` (no data, one `local` user, "No data source
connected", `mg.banner.nosource`; owner decision, 19 November 2026) and `bundled` (the
signed demo dataset of Phase A, which the apps do not bundle). A missing or unknown value
is a failed state ("app misconfigured"), never a silent switch between modes.

## Screens matched to the web suite

Screens are native SwiftUI matched to their web console (owner decision, 22 November
2026), from `Sources/MidgardKit/WebConsole/`: the web design system's components at the
web's sizes and tokens (`WebComponents.swift`, `WebStyle.swift`), and the reference
screen, Heimdall's Operating Picture (`Heimdall*.swift`, after
`products/heimdall/ui/src/screens/OperatingPictureScreen.tsx`). The Mac app shows it in
the web's frame (classification banner, header with seals, navigation, picture,
inspector); the iPhone app shows the picture first and the inspector as a sheet. Where the
web's colours fail WCAG AA they are lifted to the nearest that passes (`MGWeb`), so the
accessibility audit still holds.

Tables (`WebTable`) follow one width rule on the Mac (`WebTableWidths`): where the web
scrolls a table sideways (`overflowX: auto`), the app shares the column's width among
the table's columns. Drawn chips, badges and bars keep the web's width; text columns give
way first, down to their header's width, and truncate with "…"; the flexible column (an
action's title, a check) takes what is left and gives way last. When even the least does
not fit (a narrow window) the table is drawn as the iPhone's list. So at the default
window width no state chip or column is out of sight.

## Midgard's screens (Phase C)

All eleven of Midgard's screens are built on both apps, matched to the rc.3 web console
(`products/midgard/ui/src/screens/`) over the release's sample data:
`Sources/MidgardKit/Consoles/Midgard/` (the screen models and views) and
`Sources/MidgardKit/WebConsole/` (`MidgardChrome.swift`, the console's frame;
`WebDataComponents.swift`, the design system's Table, Terminal, MeterBar, Card, Button,
Select, Input, Tabs and Tag). The Mac shows each in the web's frame (classification
banner, header with seals and the "Search all object data…" box, the navigation's
FABRIC, GOVERNANCE and PLATFORM sections in the web's order and words, the screen and its
inspector rail); the iPhone lists the screens in the same order and words, and shows each
screen under the console's banner and header with its tables as lists and its inspector
in a sheet ("Inspect …", or a row tapped).

| Screen | What it does here |
|---|---|
| Object Fabric | the registry's types; Act on one object; a human-gated action requested (`midgard.requestApproval`), approved by a second persona (`midgard.decideApproval`), run by the requester with that approval as the gate token (`midgard.applyAction`); every outcome or refusal shown; the latest audit lineage |
| Data Search | type, `*` any-text conditions, order, paging (`midgard.query`), and the header's text search across every type; offline, the last answer to the same search, said to be the last one |
| Search Around | seed type and id, hops, the traversal (`midgard.searchAround`) grouped by hop, the link table; "Explore instances" and "Open in Search-around" open it |
| Policies | the rules, markings and purposes tabs; the dry run (`midgard.simulatePolicy`) against the signed-in persona's subject, clearance and roles |
| Audit | the ledger with its filter and category tags, the lineage trace, Export chain (the Mac's save panel, the iPhone's share sheet) of the rows shown |
| Identity, Keys, Compute, Observability, Resilience, SDK | the sample tables and cards, SAMPLE DATA, their controls disabled with the web's reasons |

The actions are the core's ports of `MockGateway` (`clients/core/src/demo/actions/midgard.rs`,
which lists where the port knowingly differs); approvals are the runtime collection
`midgard.approvals`. Rows above the persona are withheld by count ("N … WITHHELD · ABOVE
YOUR CLEARANCE"): an analyst (K. Rao) does not see Aircraft or Sensor Feed in the registry.

Where the apps knowingly differ from the web console:
- **Signatures are the core's.** The sample's seeded audit rows carry no signature, so
  they show UNSIGNED and LINEAGE SIGNED counts only the rows this device signed for its own
  actions (`ML-DSA ✓`); the web mock labels every row `ML-DSA ✓`.
- **No synthetic live stream.** The web mock adds a record from a pool of four every 4.2 s;
  the apps do not, so the web's "· LIVE", "RECORDS · LIVE" and "STREAM LIVE" labels read
  without "live".
- **Audit wording is the core's** (`applied · midgard.query` on
  `midgard.objectTypes/aircraft`), numbered as the mock numbers its rows.
- **Act on** lists the type's objects from `midgard.objects` (what the web's query
  returns), so opening Object Fabric adds no "Query" row to the ledger.
- **The second person is another persona**, not the mock's simulated approver after 3 s;
  a requester approving their own request is refused (separation of duties).
- **Reads cover what the persona may read**: query conditions apply (the mock ignores
  them), a traversal withholds nodes above the persona and names the seed's own type,
  and the dry run evaluates the rules the persona may read with the persona's own roles.
- **Offline**, a computed read is not queued: Data Search shows the last answer to the
  same search; Search Around and the dry run say they need the enclosure. A requested
  approval or action is queued.
- **The header** has no colour-mode toggle (the apps follow the system's appearance) and
  no notifications button (disabled on the web); its account menu is the persona menu.
  The Mac's navigation also has the row of console tiles, as on Heimdall. On the iPhone
  the header's search box is at the top of Data Search.
- **Tables** wider than the main column scroll sideways (the web's `overflow-x: auto`); on
  the iPhone and at accessibility text sizes they are lists. The Mac keeps Data Search's
  inspector rail, with a hint, before a match is picked.
- **Colour**: Midgard's red numerals are `--midgard-300` in dark mode, where the base red
  is under AA at 11 pt.

`MidgardFlowTests` on each app opens all eleven screens and makes the approval request;
`MidgardConsoleTests` covers the screen models against the core.

## Admin's screens (Phase D)

All eight of Admin's screens are built on both apps, matched to the rc.3 web console
(`products/admin/ui/src/screens/`) over the release's sample data:
`Sources/MidgardKit/Consoles/Admin/` (the screen models and views) and, shared with
Midgard, `Sources/MidgardKit/WebConsole/` (`AdminChrome.swift`, the console's frame;
`WebAdminComponents.swift`, the design system's RiskBadge, GuardedAction and BreakGlass;
`WebConsoleFrame`, the Mac frame both consoles use). Admin has no accent of its own: it
renders under Midgard's, with the web's grey ADMIN mark. The Mac shows each screen in the
web's frame (the management-zone banner, the header with its seals, the screen's toolbar
item and the search box, the navigation's OVERSEE, ADMINISTER, EMERGENCY and ASSIST
sections in the web's order, words and badges, the screen and its inspector at the web's
width); the iPhone lists the screens in the same order and words, and shows each under
the console's banner and header (the toolbar item at the header's end, wrapping under
the mark when it does not fit) with its tables as lists and its inspector in a sheet.

| Screen | What it does here |
|---|---|
| Overview | subsystem health and SLOs, the security events, the break-glass review strip, the panel binding of the selected subsystem; Audit search opens the ledger |
| Approvals | the dual-control queue; Approve · step-up auth decides as the persona signed in (`admin.approve`): a requester's own approval is denied (AC-AD-4) and recorded on the item, a second person's granted; both are audited and head the "AUDIT · INCLUDING DENIALS" tail with their audit ids |
| IAM | roles (scope, ceiling, read-only, holders) and bindings; the selected role's can / cannot split |
| Config | the versioned changes; Simulate (`admin.simulateConfig`), Commit (`admin.commitConfig`, v482, v483, …) and Rollback… (`admin.rollbackConfig`), each signed and audited |
| Registry | the model registry, agent governance, evals gates and releases; the selected model's lifecycle and verification, or the candidate's dual-control card |
| Ledger | the ledger across the subsystems, its search (`admin.searchLedger`, the `actor:`, `subsystem:` and `since:` tokens), the selected entry, the investigation assistant, and the ledger lineage (`admin.audit`) |
| Break-Glass | the state strip and its phases; Request break-glass (`admin.requestBreakGlass`) opens it active, 00:59:59, held by the requester; the post-hoc review |
| Copilot | the advisory thread (message, proposal, refusal with its named code), the three assistants and the refusal codes |

The actions are the core's ports of `AdminMock` (`clients/core/src/demo/actions/admin.rs`,
which lists where the port knowingly differs), numbered as the mock numbers its ledger
(`A-9111` from 14:02:13). Rows above the persona are withheld by count: the break-glass
state is confidential (K. Rao and A. Holt do not see it, and cannot request it), the
ledger restricted (A. Holt does not see it).

Where the apps knowingly differ from the web console:
- **The approver is the persona signed in.** The web screen passes each item's routed
  approver, so its self-approval check never fires from the screen; here J. Benjamin
  approving his own A-9110 is denied, and K. Osei approving it is granted (the item then
  names K. Osei as its approver). A decided approval is not decided again.
- **Deny stays on screen**, as on the web: it is the second controller's own decision,
  not sent and not audited (as Heimdall's Reject, raised in R1 for this phase), and kept
  until the app quits.
- **The ledger lineage is the core's.** It starts with the mock stream's three seed
  records (UNSIGNED: they carry no signature) and gains a row for every Admin action, in
  the core's wording (`refused · admin.approve`); there is no synthetic live stream, so
  "LEDGER LINEAGE · LIVE" reads without "live". The ledger's "SIGNED · TAMPER-EVIDENT"
  figure counts the lineage rows the core checked (the web shows a fixed 100%), and the
  selected entry's `sig` line says UNSIGNED.
- **The tables show what the core holds now**: a commit or rollback changes the change's
  state in the table (the web re-reads nothing), and a rollback says ROLLED BACK · vN.
- **Controls the web leaves without a handler are disabled with a reason**: New binding,
  Export for auditors, Open review · second officer, Export transcript, Simulate → review
  → commit, Discard and Send (the copilot thread is read-only in this release). Audit
  search opens the ledger. The Ledger's filter tags select as the web's do and, as on
  the web, filter nothing; the Registry candidate's Approve and the Break-glass phase
  tags are the web screens' own on-screen state.
- **The header's search box** opens the ledger searched for its text (the web's box does
  nothing); there is no colour-mode toggle or notifications button, and the account menu
  is the persona menu. On the iPhone the screen's toolbar item is in its header, and the
  Ledger's own search box is the only one.
- **Overview's break-glass strip** is the web's fixed text, shown only to a persona who
  may read the break-glass state; otherwise it says the state is withheld.
- **Small type and tone**, for the contrast audit: on the dual-control card the keys
  (REQUESTER, APPROVER), the subsystem and "JUSTIFICATION · MANDATORY" (11 pt, the web's
  9 px) are in the body grey rather than the faint one, and the separation check is in
  medium weight; on the break-glass strip the keys and "SECURITY OFFICERS ALERTED" are
  in the body grey; the Registry's unavailable lifecycle steps are drawn in the muted
  colour rather than at 45 % opacity. The break-glass dot does not pulse. IAM's Holders
  and State columns are wider than the web's hints, as a browser widens them.
- **iPhone accessibility**: a table row shown as a list, and the Inspect button, are one
  element over their drawn text, which they say (so one the screen's edge cuts above
  the console bar is measured whole); a stat block's label is its own element, after
  its figure.

`AdminFlowTests` on each app opens all eight screens and approves A-9110 as K. Osei;
`AdminConsoleTests` covers the screen models against the core and
`clients/core/tests/admin_actions.rs` the actions. The audit covers Admin's Overview on
both apps and its Approvals on the Mac; where it stood after Phase D is in the plan's
"Phase D done" note.

## Heimdall's screens (Phase E)

All six of Heimdall's screens are built on both apps, matched to the rc.3 web console
(`products/heimdall/ui/src/screens/`) over the release's sample data: the Operating
Picture (R1, Phase I's MAP and 3D, Phase J's user layers) and, in
`Sources/MidgardKit/Consoles/Heimdall/` (the screen models and views), Entities, Feeds,
Picture Sync, Workflows and Decision Lineage, with `HeimdallChrome` (the navigation's
items, each screen's context line and inspector width) and the picture's Alerts list
(`HeimdallAlerts.swift`) in `Sources/MidgardKit/WebConsole/`. The Mac shows each screen
in the web's frame (classification banner, the header with the screen's context line,
seals and toolbar item, the navigation's PICTURE and DECIDE sections in the web's order,
words and badges, the screen and its inspector at the web's width); the iPhone lists the
screens in the same order and words, and shows each under the console's banner and
header (the toolbar item at the header's end) with its tables as lists and its
inspector in a sheet.

| Screen | What it does here |
|---|---|
| Operating Picture | the picture, its layers and tracks; the inspector's ranked options, each with its citations, the chosen one authorised through the human gate (rank 1 → W-0912, rank 2 → W-0913, rank 3 "hold" opens none); the header's Alerts list |
| Feeds | the feeds with rate, latency, fusion weight, PQ authentication and state; the quarantined bridge (LEGACY-L16, excluded from fusion) and, listed with them, the user's own feeds (Your Data) as UNVERIFIED and quarantined; the selected feed's authentication and contribution |
| Entities | the tracked entities with class, confidence, corroborating feeds, first seen, marking and state; the web's filters (all, air, maritime, ground, anomalous, low confidence); the selected entity's classifier and track history, and for the anomalous one "Open picture" |
| Picture Sync | the four sites as a topology (PQ-mTLS links, the courier to the air-gap) and a table; the selected site's figures, the marking filter on the RESTRICTED replica, its replication |
| Workflows | the human-gated workflows; the selected one's stages and its gate: Authorise sends `heimdall.authoriseWorkflow` with a gate token made from the press, and the inspector shows the audit id; Reject stays on screen; the toolbar's "N AWAITING AUTHORISATION" |
| Decision Lineage | the decision records; the selected one's replay evidence and outcome; the live lineage tail (`heimdall.audit`) and the chain verification |

The action is the core's port of `HeimdallMock.authoriseWorkflow`
(`clients/core/src/demo/actions/heimdall.rs`, which lists where the port knowingly
differs): refused without a token with `HumanGateRequiredError`'s words ("Human gate
required for action \"<title>\""), applied with one, audited either way in
`heimdall.audit` (`D-4473` from 14:39:04), at the workflow's marking. Rows above the
persona are withheld by count: K. Rao (restricted) sees only the vessel E-0442 among the
entities and none of the OPS-N workflows, decisions or lineage; A. Holt (internal) sees
W-0910 only and no sync site.

Where the apps knowingly differ from the web console:
- **No live stream is invented.** The lineage tail is `heimdall.audit` as the core
  delivers it (the mock's five seed records, then each authorisation and refusal made
  on this device), re-read every 4.2 s while the screen is open; the mock's rotating
  synthetic records are not shown. Signatures are shown as the core checked them: the
  seed records UNSIGNED, this device's own `ML-DSA ✓` (its audit key). The lineage's
  signed figure ("TAIL SIGNED · CHECKED", the web's fixed "100% SIGNED + REPLAYABLE") and
  the chain verification lines count those rows, "—" when none is shown.
- **No check mark the app did not check.** A decision's signature arrives from the core
  as "not checked (demo)" and is shown so; a feed's PQ authentication reads
  "ML-DSA · AUTH" (the web's "ML-DSA ✓"), FAILED, or UNVERIFIED for a user's feed.
- **Feeds counts the user's feeds**: REGISTERED and QUARANTINED add them (the web's 12 and
  1 otherwise); a notice says how many were added on this device. The contribution's
  marking is the feed row's own (the web prints CONFIDENTIAL // OPS-N for every feed).
- **Reject stays on screen**, as on the web: the operator's own decision, not sent and
  not audited (raised in R1).
- **Alerts**: the web's bell has no list behind it; here it opens one built from the
  picture's own rows the persona may see (the anomalous track first, then the
  quarantined and stale feeds), and choosing the track selects it for the inspector.
- **No time scrubber.** The web console has none, and the sample has no synthetic hour
  (`heimdall.motion` is not served, owner decision 22 November 2026), so there is nothing
  to replay.
- **Controls the web leaves without a handler are disabled with a reason**: Register
  feed, Re-key bridge, Cut picture snapshot, Export for review and Replay picture at
  decision time. "Open picture" (Entities) opens the Operating Picture.
- **Connected to a gateway** Heimdall is not served by it: its screens keep the sample
  data and say so ("NOT SERVED BY <host>"), and its action stays on the sample data.
- **Fixed figures** are the web's: 1,204 tracked, 96.8% mean confidence, 4 sites, 0.8s,
  216 filtered, 312 decisions, 0 chain breaks.

`HeimdallFlowTests` on each app opens all six screens and the Alerts list, authorises
option 01 on the picture (D-4473) and W-0913 on Workflows (D-4474), and finds both at
the head of the lineage tail; `HeimdallConsoleTests` covers the screen models against
the core and `clients/core/tests/heimdall_actions.rs` the action. The audit covers
Heimdall's Workflows and Decision Lineage on the Mac and its Feeds and Workflows on the
iPhone, beside the Operating Picture, and the Your Data screen on both.

## Forge's screens (Phase F)

All seven of Forge's screens are built on both apps, matched to the rc.3 web console
(`products/forge/ui/src/screens/`) over the release's sample data:
`Sources/MidgardKit/Consoles/Forge/` (the screen models and views) and, in
`Sources/MidgardKit/WebConsole/`, `ForgeChrome.swift` (the navigation's items, badges,
each screen's context line and inspector width; the pass meter and the lineage chain)
and Forge's accent (`MGWebAccent.forge`). The Mac shows each screen in the web's frame
(the RESTRICTED · DATA PLANE banner, the header with the PQ-SEALED seal and the screen's
toolbar item, the navigation's DATA OPS and FABRIC sections in the web's order, words and
badges, the screen and its inspector); the iPhone lists the screens in the same order and
words, and shows each under the console's banner and header with its tables as lists and
its inspector in a sheet.

| Screen | What it does here |
|---|---|
| Pipelines (Pipeline Builder) | the pipelines as tags, the selected one's graph (sources → transforms → dataset → projection) and the selected node's inspector (version, expectations, version history, encryption); Run build sends `forge.runPipeline` and replays the staged steps it returns into the build log and the BUILD meter, 560 ms apart, as the web does, ending with the audit id; Mimir's suggestion; node settings (label, detail) edited on both apps; on the Mac nodes dragged, added, connected and removed, and a new graph drawn |
| Connectors | the typed inbound-pull sources with kind, cadence, datasets fed, volume and state; the selected one's trust boundary, the expiring certificate's ACTION REQUIRED, and its last pull |
| Datasets | the versioned, branched datasets with marking and state; the selected one's version diff and seal; the lineage chain under the table |
| Data health | the expectations with pass rate, trend and state; the selected one's pass rate, projection gate and pass meter, and Mimir's diagnosis where there is one |
| Schedules | batch and streaming schedules with trigger, last run, next and state; the selected one's trigger rules and the retry note; the run ledger |
| Transform IDE | the transform's files and code with line numbers; Preview run and Commit v43 both preview in the sandbox (`forge.previewTransform`), which opens the human gate; Authorise sends `forge.applyTransform` with a token made from the press and shows the audit id; Reject stays on screen |
| Object projection | the Aircraft projection's source → mapping → target with its marking, and the mapping rules; the selected rule and the governance carry-through |

The actions are the core's ports of `ForgeMock` (`clients/core/src/demo/actions/forge.rs`,
which lists where the port knowingly differs): `forge.runPipeline`,
`forge.previewTransform` and `forge.applyTransform`, the commit refused without a token in
`ForgeHumanGateRequiredError`'s words ("Human gate required to commit
\"fuse_positions\""), all audited in `forge.audit` in the mock's numbering (`FRG-2044` from
14:02:13) at the target's marking. Rows above the persona are withheld by count: K. Rao
(restricted) does not see the OPS-N pipeline flight_tracks_eu, its dataset or the
Aircraft projection, and cannot build that pipeline.

Where the apps knowingly differ from the web console:
- **Graph edits are drafts on this device.** The enclosure has no graph write in this
  release (nor has the web mock), so a moved, renamed, added or connected node, and a new
  graph drawn on the Mac, stay on the device (DRAFT · THIS MAC ONLY · NOT SENT) until
  discarded or the screen closes; a build runs the enclosure's own graph, and a new graph
  is not built. On the iPhone the graph is viewed and a node's settings edited only, as
  the plan has it.
- **The Transform IDE's code can be edited** here before a preview (Edit code; Revert):
  the preview and the commit carry the edited text. The web shows the code read-only.
- **Controls the web leaves without a handler are disabled with a reason**: Add
  connector, Rotate certificate, New branch, Time travel, Open in builder, New
  expectation, Preview quarantine, Dismiss, New schedule, Branch, Preview transform,
  Open projection mapping, Sync now and Open type in Midgard. The Connectors and Schedules
  filter tags and the Datasets tabs are drawn as on the web and, as there, filter
  nothing; the low-code / pro-code tags are the web's fixed ones.
- **No search box, colour-mode toggle, notifications or account icons** in the header
  (the shared `ConsoleToolbar`, which has no handler on the web); the account menu is the
  persona menu.
- **The Mac opens Forge on Connectors**, the catalogue's first screen, where the web
  opens Pipelines at `/`; the navigation and the iPhone's list are in the web's order,
  Pipelines first.
- **Data health draws the web's meter**, not a chart (the plan's "Swift Charts for
  drift"): the web shows a pass meter, warn under 98 %, danger under 95 %.
- **No live audit stream is invented.** The mock's `auditStream` adds a record every
  4.2 s; no Forge screen shows the audit on the web, and `forge.audit` holds the seed and
  what this device's actions add.
- **Imported data** (Your Data) is not listed here: Forge's screens list connectors,
  datasets and projections, not object types, and an import is neither.
- **Accent text on paper** is a darker shade of `--forge-600` (0x8A400C): the web's
  reads 4.3:1 on the light page, under AA.
- **Connected to a gateway** Forge is not served by it: its screens keep the sample data
  and say so ("NOT SERVED BY <host>"), and its actions stay on the sample data.

`ForgeFlowTests` on each app opens all seven screens, builds flight_tracks_eu (FRG-2044)
and commits the transform through the gate (FRG-2046); on the Mac it also draws a node
and discards it. `ForgeConsoleTests` covers the screen models against the core and the
Mac table's widths, and `clients/core/tests/forge_actions.rs` the actions. The audit
covers Forge's Pipelines and Transform IDE on both apps (and its Connectors, the
console's first screen, on the Mac).

## Mimir's screens (Phase G)

All seven of Mimir's screens are built on both apps, matched to the rc.3 web console
(`products/mimir/ui/src/screens/`) over the release's sample data:
`Sources/MidgardKit/Consoles/Mimir/` (the screen models and views) and, in
`Sources/MidgardKit/WebConsole/`, `MimirChrome.swift` (the navigation's items, badges,
each screen's context line and inspector width; the Evals pass bar) and Mimir's accent
(`MGWebAccent.mimir`). The Mac shows each screen in the web's frame (the CONFIDENTIAL ·
RECIPIENT EYES ONLY banner, the header with its INFERENCE · LOCAL and NO EGRESS seals and
the screen's toolbar item, the navigation's BUILD, GOVERN and SERVE sections in the web's
order, words and badges, the screen and its inspector); the iPhone lists the screens in
the same order and words, Agents first, and shows each under the console's banner and
header with its tables as lists, its inspector in a sheet and every control at least
44 pt.

| Screen | What it does here |
|---|---|
| Agents (Agent Studio) | the agents as cards (eval pass, description), the OAG session: the opening question, the grounded answer with its object citations and the proposal "Schedule maintenance · AC-2214 → FAC-DUB" behind the human gate; Authorise sends `mimir.applyProposal` with a token made from the press and the header shows ACTION AUTHORISED · AUDIT #<id>; a question asked gets the follow-up answer after 1.2 s, as on the web; the inspector's eval figures, guardrails and SELF-* loops |
| Logic (no-code) | the dispatch agent's seven steps as the web draws them, the gate outlined in warn with MANDATORY; the selected step's inspector (its MANDATORY STEP note on the gate), its settings (title, detail) and the versions; on the Mac steps are also added, moved and deleted (the gate's Delete is disabled); Save sends `mimir.saveLogic`, and the core refuses any save that deletes a gate or leaves a state-mutating action without one after it, whatever the screen sends |
| Session traces | S-8812's plan–act–observe steps with their timings, the gate PENDING, the selected step and the PQ-encrypted memory records; above the persona's clearance a trace is withheld by count |
| Evals | the suites with pass bar, delta and state, the kind tags (they filter, as on the web); Run (the inspector's, or the header's Run suite) sends `mimir.runEval` and shows the gate's verdict, the score meter, every case and the audit id |
| Guardrails | the rail inventory, a rail's mode (core rails locked), LIVE INTERCEPTION: the three presets or a typed input through `mimir.simulateGuardrail`, the verdict with its refusal code, rail, reason and audit id; the incidents table |
| Governed feedback | the records with provenance and state; the selected one's PATH TO POLICY, or for the poisoning attempt its POISONING DEFENCE · DISCARDED note; the learning ledger |
| Model plane | the four families as cards, the selected one's throughput, weights provenance and canary prompts; the consumers table |

The actions are the core's ports (`clients/core/src/demo/actions/mimir.rs`, which lists
where the port knowingly differs): `mimir.runEval` and `mimir.simulateGuardrail` from
`MimirMock` (its two patterns as literal alternations, no regex engine; golden-tested on
the mock's own answers, `clients/demo/golden/<release>/mimir.json`, recorded by
`clients/demo/tools/extract-golden.sh`), and two the mock does not hold:
`mimir.applyProposal` (refused without a token, "Human gate required to authorise
\"Schedule maintenance · AC-2214 → FAC-DUB\"", and decided once) and `mimir.saveLogic`.
All are audited in `mimir.audit` in the mock's numbering (`A-77121` from 14:32:10), at
the target's marking. Rows above the persona are withheld by count: K. Rao (restricted)
reads neither session trace, J. Benjamin (OPS-N) reads S-8812 only.

Where the apps knowingly differ from the web console:
- **The proposal is decided by the core.** The web's Authorise only hides the proposal
  and shows a fixed "AUDIT #A-77120"; here it is an audited action, so the badge shows the
  id the core gave it (A-77121 for the first action of a session), and a second
  authorisation of the same proposal is a conflict ("already authorised").
- **Logic edits are saved** (in the sample's memory, until the app quits): the web screen
  is read-only and the mock has no write. The plan's canvas edits are steps added, moved
  and deleted in the web's vertical layout, not a free canvas.
- **Evals and guardrail checks are not queued offline**: they are reads the enclosure
  computes, and the screen says so. The Studio exchange still plays offline (it is a
  cached row); the header's AI chip shows AI OFF offline (owner decision 6), not the
  plan's on-device tier (Q6).
- **Controls the web leaves without a handler are disabled with a reason**: the model and
  route selects, Test run, Publish v7, Export trace, Export incident log, the guardrail
  mode switch of a non-core rail, and the proposal's Reject. Settings (an icon with no
  handler) is left out.
- **No search box, colour-mode toggle, notifications or account icons** in the header, as
  on Forge's; the account menu is the persona menu.
- **On the iPhone the Agent Studio's agents are tags** with only the chosen agent's card
  under them (the Mac keeps the web's three cards): stacked, the cards pushed the
  session to the console bar's edge, where the audit read the bar against the cut text;
  the answer's text is one element over its drawn text, as the speaker line is.
- **The Mac opens Mimir on Model plane**, the catalogue's first screen, where the web opens
  Agents at `/`; the navigation and the iPhone's list are in the web's order, Agents first.
- **No live audit stream is invented**: the mock's `auditStream` adds a record every 4.2 s;
  no Mimir screen shows the audit on the web, and `mimir.audit` holds what this device's
  actions add.
- **Accent text on paper** is a darker shade of `--mimir-600` (0x12666F): the web's reads
  4.0:1 on the light page, under AA.
- **Connected to a gateway** Mimir is not served by it: its screens keep the sample data
  and say so ("NOT SERVED BY <host>"), and its actions stay on the sample data.
- **Imported data** (Your Data) is not listed here: Mimir's screens list agents, suites,
  rails and models, not object types.

`MimirFlowTests` on each app opens all seven screens, authorises the Studio's proposal
(A-77121), runs the selected suite from the header (A-77122, the gate blocking) and stops
the injection preset at the input rail (A-77123); on the Mac it also checks the gate's
Delete is disabled and deletes and restores another step. `MimirConsoleTests` covers the
screen models against the core, and `clients/core/tests/mimir_actions.rs` the actions.
The audit covers Mimir's Agent Studio and Guardrails on both apps (and its Model plane,
the console's first, on the Mac).

## Bifrost's screens (Phase H)

All six of Bifrost's screens are built on both apps, matched to the rc.3 web console
(`products/bifrost/ui/src/screens/`) over the release's sample data:
`Sources/MidgardKit/Consoles/Bifrost/` (the screen models and views) and, in
`Sources/MidgardKit/WebConsole/`, `BifrostChrome.swift` (the navigation's items, badges,
each screen's context line and inspector width; the health-gate pill) and Bifrost's
accent (`MGWebAccent.bifrost`). The Mac shows each screen in the web's frame (the
INTERNAL · RELEASE CONTROL PLANE banner, the header with its ARTEFACTS ML-DSA ✓ seal and
the screen's toolbar item, the navigation's DELIVERY and FLEET sections in the web's
order, words and badges, the screen and its inspector); the iPhone lists the screens in
the same order and words, Releases first, and shows each under the console's banner and
header with its tables as lists, its inspector in a sheet and every control at least
44 pt. With Bifrost in, every console's screens are built: neither app has a placeholder
screen any more (the Mac's split-view shell for unbuilt consoles is gone).

| Screen | What it does here |
|---|---|
| Releases (Release Console) | the release's rollout: installations, failed requests, rollback gate, the four rings with their meters (OFFLINE BUNDLE on the air-gap ring), the fleet and the supply-chain terminal; the header's Promote to Ring 2 sends `bifrost.promoteRelease` for the broad ring step after step until it is fully rolled or the gate fails, as the web's loop does, the meter and gate pill (WATCH, then PASS) following each answer and the inspector showing LAST STEP · AUDIT <id> and every step this session made; the inspector's release intelligence, health gates and provenance |
| Artefacts | the registry with reproducibility, critical CVEs and state; the selected artefact's verification, and for the non-reproducible one its BLOCKED · NOTHING WAS PROMOTED note |
| Channels | the three channels as cards (cadence, current, subscribers, next), the promotion history; the selected channel's promotion policy and subscriber mix |
| Offline bundles | the six-stage bundle pipeline (the first two outlined while the selected bundle awaits its courier), the bundles; the selected one's contents and offline verification |
| Fleet & drift | converged, version-drift and config-drift counts, the installations; the selected one's desired and actual versions, its CONFIG DRIFT diff where it has one, and its on-host agent |
| Rollback centre | the standing plans; the header's Run drill plays the web's canned drill into the DRILL LOG at the web's pace; the selected plan's triggers and health gate, and Execute rollback sends `bifrost.rollback` (EXECUTED · AUDIT <id>, and the log's closing lines) |

The actions are the core's ports of `BifrostMock` (`clients/core/src/demo/actions/bifrost.rs`,
which lists where the port knowingly differs), golden-tested on the mock's own answers
and audit records (`clients/demo/golden/<release>/bifrost.json`, recorded by
`clients/demo/tools/extract-golden.sh`), audited in `bifrost.audit` in the mock's
numbering (`AUD-B-1044` from 14:32:10, after its five seed records) at the target's
marking, at least restricted. They keep the mock's gates and words: a promotion step
advances the ring by `max(1, ceil(15 %))` of its installations, WATCH while rolling and
PASS once rolled; a promotion to the air-gap ring is refused, fail closed, as "Promote ·
REL-2050 → ring air-gap (bundle required)"; a rollback runs only while its plan is armed
or available ("Rollback held by gate"). Both need the role `release-manager`: of the
sample personas only A. Holt holds it, and the others are refused ("Role release-manager
required to promote REL-2050").

Where the apps knowingly differ from the web console:
- **Promotions and rollbacks are decided by the core**, for the release-manager role
  only (the mock checks no role and always names `j.benjamin`); the persona signed in is
  the actor, and the refusal is shown in the screen. The air-gap ring's gate is a
  refusal (the mock answers `healthGate: 'fail'`); an unknown ring or plan is refused.
  The rings read back as the core left them, so a second Promote starts from 100 %.
- **A. Holt does not read Bifrost's new audit rows**: the core marks an action's audit
  row at least restricted, above A. Holt's internal clearance; the audit id still comes
  with each answer, which is all the web screens show. K. Osei reads them.
- **Offline a step or a rollback is queued** (they are actions, not reads): Promote
  stops after the first step and says so.
- **The promotion's steps are listed** in the inspector (STEPS · THIS SESSION, each with
  the mock's own record wording, which the core returns as `step`), where the web shows
  only the last audit id.
- **Controls the web leaves without a handler are disabled with a reason**: the channel
  select (left out on the iPhone, so Promote fits the header), Prepare rollback plan,
  Register artefact, New channel, Open release console,
  Cut new bundle, Reconcile all, Revert to desired and Adopt. The Artefacts filter tags
  highlight but filter nothing, as on the web. Run drill is the web's canned rehearsal
  (no action), played here as there.
- **No search box, colour-mode toggle, notifications or account icons** in the header,
  as on the other consoles; the account menu is the persona menu.
- **No live audit stream is invented**: the mock's `auditStream` adds a record every
  4.2 s; no Bifrost screen shows the audit on the web.
- **Accent**: text in the web's own `--bifrost-600` on paper (5.9:1 on the page, 5.0:1
  on the tint) and `--bifrost-300` in dark mode; the section numerals, the base violet on
  the web, are `--bifrost-300` in dark mode (the base reads 4.3:1 on the dark page).
- **Connected to a gateway** Bifrost is not served by it: its screens keep the sample
  data and say so ("NOT SERVED BY <host>"), and its actions stay on the sample data.
- **Imported data** (Your Data) is not listed here: Bifrost's screens list releases,
  artefacts and installations, not object types.

`BifrostFlowTests` on each app signs in as A. Holt, opens all six screens, promotes the
broad ring to 100 % from the header (AUD-B-1044 to AUD-B-1047, GATE PASS), plays the
drill and executes the release rollback plan (AUD-B-1048). `BifrostConsoleTests` covers
the screen models against the core (and the refusals without the role, and the queued
step offline), and `clients/core/tests/bifrost_actions.rs` the actions. The audit covers
Bifrost's Release Console and Rollback centre on the iPhone, and its Release Console
(the console's first), Fleet & drift and Rollback centre on the Mac.

## Heimdall's MAP and 3D views (Phase I)

The Operating Picture has the web console's view switch, **2D / MAP / 3D** (the Mac's
header; the iPhone's navigation bar, each button 44 pt). 2D is the reference layout and
the default; the choice is kept for the window or scene, as the web keeps it for the
browser. MAP and 3D come from `MidgardGlobeKit` (`GlobeRegistration`, through
`AppModel.pictureViews`) and share the picture's model: the LAYERS switches, the
selected track and the inspector are the same in every view (on the iPhone a track
chosen in MAP or 3D opens the inspector sheet). Both views port the behaviour and the
numbers of the web console on `heimdall_3dmaps` (`products/heimdall/ui/src/globe/`,
`map/`, `tiles/`, `tileSource.ts`, `tilePause.ts`), its public demo build.

- **3D** (`Globe/`): Metal, drawn on demand. A lit sphere with day and night shading
  from the subsolar point (`sun.ts`), the graticule, the bundled Natural Earth 5.1.2
  outlines (1:110m far out, 1:50m below 3,000 km; `Resources/Outlines`, the web's own
  files), the atmosphere rim, or a sky inside its shell. The camera has the web's
  limits and easing (`camera.ts`, `view.ts`): 50–40,000 km, down to the 1.5 km close-up
  and 150 m over the terrain while tiles are drawn, tilt to 75° (less far out), heading,
  the sight-line tilt limit over hills, and the presets and REGION list of `presets.ts`
  (SNOWDONIA and SOUTHERN UPLANDS while the terrain tiles are drawn). Drag to pan; pinch
  or scroll to zoom; on the Mac the right button, ⌥ or ⌃ to tilt and turn, a trackpad
  rotate to turn, and arrow keys (Shift to tilt and turn), + and − once the globe has
  focus; on the iPhone a two-finger twist turns and a two-finger move tilts. The tracks
  stand at their positions with altitude stems and ground shadows (HEIGHTS ×1–×20, ×4 by
  default). The styles are CLEAN, TOPOGRAPHIC (tints, hillshade and contours from the
  terrain) and SATELLITE, with terrain relief from zoom 6.
- **MAP** (`Map/`): CoreGraphics. VersaTiles vector tiles (OpenStreetMap, Shortbread)
  over the bundled land, drawn from the nearest loaded ancestor while a tile loads,
  with the web's caps: 500 labels kept per tile, labels cut to 200 characters, 400
  label candidates measured a frame (60 drawn), 96 tiles asked for at once, a 64 MB
  cache besides a 128 MB on-screen cap. The tracks are drawn on top.
- **The sample tracks' positions.** The signed release's sample tracks carry only the
  reference layout's x/y. The five tracks are placed at the `lat`/`lon` the web console's
  build gives them (`SamplePositions`); they are the release's built-in sample data.
- **Sources** (each listed in the always-shown credits strip and the folding ⓘ CREDITS
  with its licence and attribution): EOX Sentinel-2 cloudless 2016 (`s2cloudless_3857`,
  CC BY 4.0, the 2016 layer only), Mapzen terrain tiles (terrarium, AWS Open Data,
  attribution), VersaTiles OpenStreetMap vector tiles (ODbL), Natural Earth (public
  domain, bundled).
- **The tile loader** (`Tiles/`), the app's only network use with fixed hosts (the
  hosts the user adds in Your Data are a separate list, see
  [Your data](#your-data-phase-j)): the three hosts
  `tiles.maps.eox.at`, `s3.amazonaws.com` and `tiles.versatiles.org`, and only the one URL
  spelling each tile has (`TileSet.allowed`); HTTPS only (App Transport Security's
  default, no exception); no cookies, credentials, URL cache or redirects (a redirect is
  refused, never followed); the User-Agent `MidgardMaps/0.1`, naming no user, machine or
  address; 15 s per request; bodies capped as they arrive (1 MB terrain, 2 MB imagery
  and vector tiles, the web's `MAX_TILE_BYTES`; a larger declared length is refused
  before reading); raster tiles checked for format and size (256, or 512 for terrain)
  before and after decoding; vector tiles' features (50,000 a layer, 100,000 a tile) and
  points (1,000,000) counted before any geometry is built; decoding limited to 2 s.
  Requests are made only while MAP or 3D is on screen; closing the view destroys the
  loader and cancels everything queued or in flight. Bodies that passed the checks are
  kept on disk in the app's caches directory (`<Caches>/<bundle id>/tiles`, in the
  sandbox container on the Mac), at most 500 MB, least recently used out first.
- **Paused tiles.** A camera move the tracks decide (the first view, SECTOR NORTH,
  flying to a track) pauses tile loading, with "MAP TILES PAUSED · PAN AWAY OR CHOOSE A
  REGION"; the user's own move (a drag, zoom, tilt or turn, or a geographic preset)
  resumes it, as in the web's public build: a tile request names the area in view.
- **Offline**: with no network, after failed requests, or with the session's OFFLINE
  chip, the views fall back to the plain globe and the bundled land with "MAP TILES NEED
  A CONNECTION", never an error, and probe again later.
- **Accessibility**: the camera readout speaks one sentence (`describeCamera`: altitude,
  position, tilt, heading and the region in view), as does the map's; every control is
  labelled; the track list reaches every track; reduced motion (the system's or
  `MIDGARD_REDUCE_MOTION=1`) turns the fly-to easing off.

Where the apps knowingly differ from the web console: a MAP preset is centred on the
point the region's globe view looks at (the web centres the map on the tilted globe
camera's own position, so its UK & IRELAND map lands on northern Spain); the iPhone
puts REGION, TRACKS (with the LAYERS switches) and STYLE in menus along the bottom, and
HEIGHTS in a sheet, where the web narrows its two panels; the credits strip is always
shown beside the folding ⓘ CREDITS; the camera and map readouts are 11 pt in the
heading colour (the web's 9 px; at 9 pt the Mac's contrast audit failed them); and a
decode past its time limit is abandoned rather than its worker killed, since a Swift
task cannot be stopped from outside.

## Your data (Phase J)

Both apps let the user add their own data, as the guides describe, from one **Your
Data** screen: on the Mac Session ▸ Your Data… (⇧⌘D), its own window, and a Settings
tab, and files dropped on any window are imported; on the iPhone the session menu's
"Your data". It lists:

- **Sample**: the release's signed sample data (always there, never live).
- **Imported files**: pick an `ontology.json` and JSON Lines data files (`.jsonl`, or
  `.json` holding lines) with the system picker (the Mac's open panel, the iPhone's
  Files), as [Import your own data](../../../docs/guides/IMPORT_YOUR_OWN_DATA.md)
  describes. They are checked in the core by the very parsers `mg-admin import` and
  the gateway use (`services/ontology-import`): all or nothing, the first bad line
  refuses the whole import and is named with its file (`bad.jsonl: import refused: line
  1 (transformer/TX-9): "rating_kva" must be an integer, not "500"`), an import never
  replaces an object, links must resolve, and objects without the purpose `operations`
  import but nobody can read them (the summary says how many). The import is the
  audited action `midgard.importData` in Midgard's ledger; removal is
  `midgard.removeImportedData`. The files are kept sealed under the device root
  (`user-data.sealed`, AES-256-GCM under the ML-KEM-wrapped key, as the state files)
  and installed again, checked again, at each sign-in. Imported objects keep their
  marking and compartments: rows above a persona are withheld, as sample rows are.
  They show in Object Fabric (each imported type's "updated" reads `imported ·
  <files>`), Data Search, Search Around (over the imported links, as the gateway walks
  them) and the inspectors; an imported action's effect is applied in memory. Heimdall
  observation files (`samples/4-heimdall-observations/`, and the tab-separated lines
  `convert.py observations` writes) are accepted too.
- **Enclosure gateway**: an address, username, password and, if the account has one,
  an authenticator code, over the gateway's documented API (`/login`, `/whoami`,
  `/object-types`, `/query`, `/search-around`, `/approvals`, `/actions`, `/audit`,
  `/policies`, `/policy/simulate`, the platform read models, `/logout`). The address
  follows the SDK's rule (`https://` anywhere, `http://` only on loopback; on the iPhone
  ATS keeps it to HTTPS). Connected, the banner reads "LIVE · <host> · Midgard's
  screens read this gateway…", Midgard's eleven screens read the gateway (its answers
  checked strictly against the console's read models, the keys a gateway adds beyond
  them left out), its actions and computed reads go to it, and Admin's screens say
  they are not served by it and stay empty. The persona chip names the gateway's
  signed-in subject (its `/whoami`), never a sample persona, and Midgard's header shows
  only what the app knows of the connection, its transport ("LIVE · HTTPS", or
  "LIVE · LOOPBACK" for plain HTTP to this device), in place of the sample's PQ-SEALED
  and AIR-GAP seals; an audit with no records shows "—" for its signed share. Other consoles keep the sample data under
  their own banners. The token is held in memory only, never written, logged or sent
  elsewhere; Disconnect revokes it (`/logout`) and returns to the sample and imported
  data.
- **Feeds**: a name, an `https://` URL serving Heimdall observations (JSON, or JSON
  Lines) and an interval (30 s at least, 60 s by default), read only while Heimdall's
  Operating Picture is open. Not signed by a certified mesh identity, so each is an
  **UNVERIFIED FEED** layer, legend entry and Feeds row (`heimdall.feeds`, `auth`
  false), never fused into a signed track.
- **Addresses**: the gateway's and the feeds' addresses, the only hosts the app may
  reach for the user's data (`UserHTTPClient`, hardened as the tile loader: an
  ephemeral session, no cookies, credentials store or URL cache, the User-Agent
  `MidgardMaps/0.1`, redirects refused, 15 s, bodies capped at 8 MiB). The tile hosts are
  a separate list.

On the picture the user's layers are drawn apart from the tracks: an open diamond in
the warning tone for each observation of an UNVERIFIED FEED, a square in the accent for
each imported object with a float `lat`/`lon` (or `latitude`/`longitude`); MAP and 3D
place them exactly, with a legend; the 2D reference layout, a schematic, places them by
the least-squares fit of the five sample tracks' two layouts (within about 5 %), and
leaves off what falls outside it. The LAYERS list has a switch per layer.

`check-edition.sh` still guarantees that nothing is bundled: it fails an app carrying a
data file (`.jsonl`, `.tsv`, `.csv`, `.mgdemo`, `.geojson`, `.ndjson`) or a resource that
names a leak archive or the local live suite's area, switches or feed hosts, and one
whose ATS asks for more than `NSAllowsLocalNetworking`. Tests: `clients/core/tests/
user_data.rs` (the guide's example and the sample sets accepted, the ontology fixtures
refused, all or nothing, withheld imported rows, observations, the address rule);
`UserDataTests` (a loopback stub: sign-in, a query, redirects, a feed poll) and
`AppModelTests` (import, refusal, restore, removal over the real core); one
`YourDataFlowTests` per app.

Where the apps knowingly differ from the guide: imported data has one ontology (another
is refused until the imported data is removed); an object type the sample data already
declares (`aircraft`, `facility`, `sensor_feed`, `crew_member`, `maintenance_event`) is
refused; no `policy.json` is imported (imported actions follow the sample's personas and
its human gate, and their effects last until the app quits); a file of the same name is
not imported twice; and refusals say "this app" where `mg-admin` says "the enclosure".

## Signing

Signing is set in `Config/Signing.xcconfig` (one per app). No team ID is ever
committed.

- **Without a team** (the default): Debug builds are signed ad hoc with the no-team
  entitlements (`Config/MidgardMacMaps-AdHoc.entitlements`,
  `Config/MidgardiOSMaps-NoTeam.entitlements`), which leave out `keychain-access-groups`:
  that entitlement is restricted, and an ad-hoc app that claims it is killed at launch.
  The app builds, runs and passes its UI tests, but outside UI testing it cannot use
  the data-protection keychain and shows "cannot keep keys". iPhone device builds need
  a team.
- **With a team**: create `Config/Signing.local.xcconfig` beside `Signing.xcconfig`
  (gitignored, included optionally) holding one line,

  ```
  MIDGARD_TEAM_ID = XXXXXXXXXX
  ```

  and signing becomes automatic under that team, with the full entitlements and the
  keychain access group the Secure Enclave key record needs. Xcode needs the team's
  account signed in.
- **Any configuration other than Debug without a team is refused** by the app's first
  build phase ("Refuse a non-Debug build without a team"), because the result could
  not keep keys. To build one anyway, to inspect it and never to ship it, pass
  `MIDGARD_ALLOW_ADHOC_RELEASE=YES` on the `xcodebuild` command line or put that line
  in `Signing.local.xcconfig`.

## Release and upload

Not built yet. The release scripts of the plan's Tasks B4 (Developer ID DMG and
notarisation for the Mac) and B5 (TestFlight archive for the iPhone) are deferred
until a team ID exists, because neither can be checked without one; their archives
will go to `~/Library/Caches/midgard-build/clients-archive/`. Any upload to Apple
(notarisation, TestFlight, App Store Connect) always needs the owner's explicit
go-ahead for that upload.

## Accessibility audit

```bash
bash clients/shells/apple/audit.sh <release> shell-audit/apple-<release>.json
```

or `tools/ci/run.sh shell-audit apple <release>`, which records the result. `audit.sh`
needs both apps, so it runs on `clients_apple`; on a tree with one app it refuses. It
builds the XCFramework, runs the `AccessibilityAudit` UI test of both apps on the
`iPhone 17` simulator
(`MIDGARD_AUDIT_SIMULATOR` names another) and on this Mac, with derived data and
result bundles in `~/Library/Caches/midgard-build/clients-derived`, and prints the
affordances that failed. An affordance whose test failed, was skipped or did not run
counts as failed. It refuses UI-test sources that override `record(_:)` or
`invokeTest`.

The affordances are those in `accessibility.toml`. In the latest runs on the former app
branches, six of the seven pass on both apps (`dynamic_type` is measured on iOS only and
stands for both). The known gap is `keyboard_navigation`:

- **macOS**: Tab reaches every control only with the system setting
  System Settings > Keyboard > Keyboard navigation (the global default
  `AppleKeyboardUIMode`). When it is off, `audit.sh` turns it on for the run, on CI and
  on a local Mac alike (owner approval, 20 November 2026), and puts it back on exit. It
  writes the previous value to `~/Library/Caches/midgard-build/audit-kbd-restore` before
  the change (a later run applies a restore file left by one that did not finish),
  holds the lock `audit-kbd.lock` beside it, and says on stderr what it changed and put
  back. `MIDGARD_AUDIT_KBD=keep` leaves the setting alone, and `keyboard_navigation` is
  then reported failed, as not measured. A plain `xcodebuild test` changes nothing, so
  there the macOS keyboard test fails while the setting is off.
- **iOS**: the test needs the simulator's hardware keyboard and Full Keyboard Access.
  On the iOS 26.3 simulator, turning Full Keyboard Access on made key delivery worse
  (no Tab or ⌘-digit reached the app). Tried again on 22 November 2026 with the
  simulator's hardware keyboard connected (`ConnectHardwareKeyboard`, already on) and
  Full Keyboard Access on in the simulator (`com.apple.Accessibility
  FullKeyboardAccessEnabled`, then a reboot of the simulator): Tab still moved no focus,
  so the iOS test still fails at Tab and `keyboard_navigation` is not measured on iOS.
  The setting was put back. Whether the affordance may be waived on iOS is the owner's
  decision (`accessibility.toml` is unchanged).

### Status line hooks

Each app's root view ends in a visible status line of plain text, whose accessibility
values the UI tests and the audit read (a hidden marker was tried first: on macOS it had
no readable value, and hidden text failed the contrast audit):

| Text | Identifier | Value |
|---|---|---|
| "Animations on" / "Animations off (Reduce Motion)", in every phase | `mg.motion` | `full` / `reduced` |
| "Console n of 6: Name", once signed in | `mg.console.selected` | `1`…`6`, as ⌘1…⌘6 |

Motion is reduced by the system's Reduce Motion or by launching with
`MIDGARD_REDUCE_MOTION=1`. Focusable elements on audited screens carry identifiers
`mg.<n>.<name>` in reading order.

## Cleaning up

```bash
bash clients/shells/apple/tools/clean.sh            # build outputs
bash clients/shells/apple/tools/clean.sh --target   # and the cargo target directory
```

It removes, under `~/Library/Caches/midgard-build/` (or `MIDGARD_BUILD_CACHE`, which
must be an absolute path): `clients-xcframework`, `clients-xcframework.new`,
`clients-xcframework-by-stamp` and its lock, `clients-derived` and every
`clients-derived-*` (so `clients-derived-mac` and `clients-derived-ios`),
`clients-archive` and `clients-swift`; and, in the package, `Frameworks/`, the
generated bindings, `.build/` and `.swiftpm/`. With `--target` it also removes
`target-clients`. While an `ensure-xcframework.sh` build is running it leaves that
build's lock and the by-stamp builds alone and says so. Nothing else in the cache is
touched. The next build after a clean starts with `tools/ensure-xcframework.sh`
again.
