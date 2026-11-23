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
`HeimdallPictureFlowTests`, `HeimdallMapsFlowTests`, `EditionTests` and
`AccessibilityAudit`; add
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
| Entitlements | `MidgardMacMaps.entitlements` (team), `MidgardMacMaps-AdHoc.entitlements`: App Sandbox and the client network entitlement | `MidgardiOSMaps.entitlements` (team), `MidgardiOSMaps-NoTeam.entitlements`; iOS has no network entitlement |

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
(`sdk/ts/src/mock/`), with no feeds and no network. Each app's `Info.plist` has
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
  Operating Picture's Authorise): a human-gated workflow needs a gate token, and its
  audit row is numbered and stamped as `HeimdallMock` numbers them (`D-4473`, from
  14:39:04). Midgard's, ported from `MockGateway` (Phase C, see
  [Midgard's screens](#midgards-screens-phase-c)), are numbered as it numbers them
  (`AUD-1043`, from 14:32:10). The rows' wording stays the core's (`applied ·
  heimdall.authoriseWorkflow`), which the device's audit key checks before signing it.
  The other consoles' actions come with their phases and are refused as unknown until
  then;
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
- **The tile loader** (`Tiles/`), the app's only network use: the three hosts
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
shown beside the folding ⓘ CREDITS; and a decode past its time limit is abandoned rather
than its worker killed, since a Swift task cannot be stopped from outside.

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
