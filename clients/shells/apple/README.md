# Midgard Apple clients

The macOS and iPhone apps, and the Swift package they share. Both apps are thin
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
| `clients_mac` | `clients_shared` plus `Apps/MidgardMac/` (the apps `MidgardMac`, `MidgardMac3D` and `MidgardMacMaps`) |
| `clients_iphone` | `clients_shared` plus `Apps/MidgardiOS/` (the apps `MidgardiOS`, `MidgardiOS3D` and `MidgardiOSMaps`) |

Shared work is committed on `clients_shared` first and then merged into each app
branch with `git merge --no-ff clients_shared` (signed). It is never cherry-picked, so
the shared base stays one history, and `git diff clients_shared clients_mac` shows only
`Apps/MidgardMac/` (likewise for the iPhone). This README lives on `clients_shared`,
where `Apps/` does not exist: the app sections below describe files that are only on
the app branches.

## Layout

| Path (under `clients/shells/apple/`) | What it is |
|---|---|
| `Package.swift` | the Swift package: products `MidgardKit`, `MidgardAuditKit` and `MidgardGlobeKit`, macOS 14 and iOS 17 |
| `Sources/MidgardCore/` | the generated UniFFI bindings (`midgard_client_ffi.swift`, gitignored) over the `MidgardCoreFFI` XCFramework |
| `Frameworks/` | gitignored link to the XCFramework build in the cache |
| `Sources/MidgardKit/` | everything both apps share: session (`AppModel`, `AppEnvironment`, the UI-testing environment), Secure Enclave key root, storage paths, design tokens and fonts, the console catalogue, sign-in, lock, chips, banner, acknowledgements |
| `Sources/MidgardAuditKit/` | `AccessibilityAuditBase`, the accessibility audit each app's UI-test target subclasses |
| `Sources/MidgardHooks/` | C: the link-time hooks `MidgardKit` reads, as weak defaults (see [Editions](#editions)) |
| `Sources/MidgardGlobeKit/` | the 3D editions' library: Heimdall's "3D view" (a placeholder until Phase I), registered at link time |
| `Tests/MidgardKitTests/` | the package tests, including `ContractTests` over every exported binding |
| `Tests/MidgardGlobeKitTests/` | the 3D library's tests: linking it registers it |
| `Apps/MidgardMac/` | the macOS apps, all three editions (`clients_mac` only) |
| `Apps/MidgardiOS/` | the iPhone apps, all three editions (`clients_iphone` only) |
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
| `check-edition.sh` | builds every app and fails if one carries more or less than its edition allows (see [Editions](#editions)) |
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

Each app has its own derived-data directory, so the two branches' builds never share
one.

macOS (`clients_mac`):

```bash
xcodebuild build -project clients/shells/apple/Apps/MidgardMac/MidgardMac.xcodeproj \
    -scheme MidgardMac -destination platform=macOS \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived-mac
xcodebuild test -project clients/shells/apple/Apps/MidgardMac/MidgardMac.xcodeproj \
    -scheme MidgardMac -destination platform=macOS \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived-mac
```

iPhone (`clients_iphone`):

```bash
xcodebuild build -project clients/shells/apple/Apps/MidgardiOS/MidgardiOS.xcodeproj \
    -scheme MidgardiOS -destination 'platform=iOS Simulator,name=iPhone 17' \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived-ios
xcodebuild test -project clients/shells/apple/Apps/MidgardiOS/MidgardiOS.xcodeproj \
    -scheme MidgardiOS -destination 'platform=iOS Simulator,name=iPhone 17' \
    -derivedDataPath ~/Library/Caches/midgard-build/clients-derived-ios
```

`test` runs the scheme's UI tests (`SessionFlowTests` and `AccessibilityAudit`; add
`-only-testing:MidgardMacUITests/SessionFlowTests`, or `MidgardiOSUITests/...`, for one
class). The UI tests launch the app with `-MidgardUITesting`, which in Debug builds
only selects a key root that never prompts and a fresh temporary store; a Release
build carries none of that code. The macOS scheme skips
`AccessibilityAudit/test_affordance_dynamic_type()`: Dynamic Type is measured on iOS.

The 3D and maps apps build and test the same way with the schemes `MidgardMac3D`,
`MidgardMacMaps`, `MidgardiOS3D` and `MidgardiOSMaps` (UI-test targets
`<scheme>UITests`), in the same derived-data directories.

After editing a `project.yml`, run `xcodegen generate` in that app's directory and
commit the spec and the regenerated project together;
`bash tools/ci/tests/apple-projects.test.sh` fails when they differ, and when a project
does not declare all three editions as below.

## Editions

There are three apps on each platform (owner decision, 22 November 2026), six in all:

| | Midgard | Midgard 3D | Midgard Maps |
|---|---|---|---|
| Content | the signed release `v0.1.0-rc.3` (`ea5899ab`) | the same, plus a 3D view in Heimdall | the 3D edition; Phase I's map tiles will come only here |
| Targets and schemes | `MidgardMac`, `MidgardiOS` | `MidgardMac3D`, `MidgardiOS3D` | `MidgardMacMaps`, `MidgardiOSMaps` |
| Bundle IDs | `io.veldris.midgard.mac`, `.ios` | `io.veldris.midgard3d.mac`, `.ios` | `io.veldris.midgardmaps.mac`, `.ios` |
| Info.plist `MidgardEdition` | `release` | `3d` | `maps` |
| Links `MidgardGlobeKit` | no | yes | yes |
| Network entitlement | none | none | the client one, on the Mac (iOS has no such entitlement) |
| Banners | "No data source connected" | the same, plus "3D edition, not in the signed release" (`mg.banner.edition3d`) | the same, plus "Maps edition, not in the signed release" (`mg.banner.editionmaps`) |

The tile loader itself is Task I2: until then the maps apps are the 3D apps with a
different edition, banner and entitlements.

The editions share their sources and UI tests; each has its own `Info.plist`,
entitlements (ad hoc and team), scheme and UI-test target, whose `AccessibilityAudit`
subclasses the same `AccessibilityAuditBase`. A missing or unknown `MidgardEdition` is
the "app misconfigured" failed state, as with `MidgardDataSource`.

The release apps carry no edition code or wording (owner decision, 22 November 2026):
they read `MidgardEdition` silently and show nothing new. Everything an edition shows
lives in `MidgardGlobeKit`, which only the 3D and maps apps link: the edition banner (at
accessibility text sizes the compact "3D edition · not the signed release" or "Maps
edition · not the signed release", with the full text as its accessibility label), the
About screen (Settings on the Mac, the session menu on the iPhone; it names the edition
and says it is not the signed release), and Heimdall's "3D view" entry on the Operating
Picture. MidgardKit only asks an `EditionProvider` for them, and a package test holds
its sources (and MidgardHooks') to no edition wording, no `Globe` and no Metal import.

The provider is registered by a link-time hook, never a runtime switch: `MidgardHooks`
defines `midgard_edition_hook` weakly, returning nothing, and `MidgardGlobeKit` defines
it strongly, so a binary that links the library gets its provider and one that does not
gets none. Nothing calls a register function at launch.

`tools/check-edition.sh` builds every app present (Debug) and checks each against its
edition. Every app: its `MidgardEdition`, the Mac sandbox, and (when it builds) that its
target signs with its own entitlements file. A release app: no `MidgardGlobeKit` file
or bytes, no `Globe`, no edition wording or provider type in any Mach-O file, the hook
weak or absent (`nm -m`), no `MetalKit.framework` (`otool -L`). A 3D or maps app: the
hook defined strongly. Release and 3D apps: no network, networking or temporary-exception
entitlement. A maps app: on the Mac the client network entitlement, and nothing beyond it.
Ahead of the tile loader (Task I2, its own module `MidgardTileKit`, linked only by the
maps apps), release and 3D apps already fail on a `MidgardTileKit` symbol or on an
`NSAppTransportSecurity`, `NSLocalNetworkUsageDescription` or `NSBonjourServices` key.
`--release`, `--3d` and `--maps <app>...` check built bundles only; B4 and B5 run it so on
the signed Release archives. `audit.sh` runs it first and records nothing when it fails.
`tools/ci/tests/apple-edition.test.sh` runs it against bundles it builds with seeded
failures.

## No data source

The apps ship with no data (owner decision, 19 November 2026). Each app's `Info.plist`
has `MidgardDataSource = none`, so:

- the core signs in one built-in user, `local` (role `operator`, clearance `open`, no
  compartments), the lowest grant there is;
- every collection exists and is empty, and every action is refused with "no data
  source connected";
- the banner above everything reads "No data source connected" (`mg.banner.nosource`)
  in place of the demonstration notice.

The only other accepted value is `bundled` (the signed demo dataset of Phase A, which
the apps do not bundle). A missing or unknown value is a failed state ("app
misconfigured"), never a silent switch between modes. Package tests still use the
Phase A dataset to show rows, withheld rows and markings.

## Signing

Signing is set in `Config/Signing.xcconfig` (one per app). No team ID is ever
committed.

- **Without a team** (the default): Debug builds are signed ad hoc with the no-team
  entitlements (`Config/MidgardMac-AdHoc.entitlements`,
  `Config/MidgardiOS-NoTeam.entitlements`), which leave out `keychain-access-groups`:
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
needs both apps, so it runs only on a tree that merges `clients_mac` and
`clients_iphone`; on either app branch alone it refuses. It builds the XCFramework,
runs the `AccessibilityAudit` UI test of all six apps (three editions of each) on the
`iPhone 17` simulator
(`MIDGARD_AUDIT_SIMULATOR` names another) and on this Mac, with derived data and
result bundles in `~/Library/Caches/midgard-build/clients-derived`, and prints the
affordances that failed. An affordance whose test failed, was skipped or did not run
counts as failed. It refuses UI-test sources that override `record(_:)` or
`invokeTest`.

The affordances are those in `accessibility.toml`. In the latest runs on each app
branch, six of the seven pass on both apps (`dynamic_type` is measured on iOS only and
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
