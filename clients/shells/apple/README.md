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
| `clients_shared` | the Rust core and FFI, this package (`MidgardKit`, `MidgardAuditKit`, tests), `tools/`, `audit.sh`, this README |
| `clients_mac` | `clients_shared` plus `Apps/MidgardMac/` |
| `clients_iphone` | `clients_shared` plus `Apps/MidgardiOS/` |

Shared work is committed on `clients_shared` first and then merged into each app
branch with `git merge --no-ff clients_shared` (signed). It is never cherry-picked, so
the shared base stays one history, and `git diff clients_shared clients_mac` shows only
`Apps/MidgardMac/` (likewise for the iPhone). This README lives on `clients_shared`,
where `Apps/` does not exist: the app sections below describe files that are only on
the app branches.

## Layout

| Path (under `clients/shells/apple/`) | What it is |
|---|---|
| `Package.swift` | the Swift package: products `MidgardKit` and `MidgardAuditKit`, macOS 14 and iOS 17 |
| `Sources/MidgardCore/` | the generated UniFFI bindings (`midgard_client_ffi.swift`, gitignored) over the `MidgardCoreFFI` XCFramework |
| `Frameworks/` | gitignored link to the XCFramework build in the cache |
| `Sources/MidgardKit/` | everything both apps share: session (`AppModel`, `AppEnvironment`, the UI-testing environment), Secure Enclave key root, storage paths, design tokens and fonts, the console catalogue, sign-in, lock, chips, banner, acknowledgements |
| `Sources/MidgardAuditKit/` | `AccessibilityAuditBase`, the accessibility audit each app's UI-test target subclasses |
| `Tests/MidgardKitTests/` | the package tests, including `ContractTests` over every exported binding |
| `Apps/MidgardMac/` | the macOS app (`clients_mac` only) |
| `Apps/MidgardiOS/` | the iPhone app (`clients_iphone` only) |
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

After editing a `project.yml`, run `xcodegen generate` in that app's directory and
commit the spec and the regenerated project together;
`bash tools/ci/tests/apple-projects.test.sh` fails when they differ.

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
runs each app's `AccessibilityAudit` UI test on the `iPhone 17` simulator
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
  `AppleKeyboardUIMode`). On a CI runner `audit.sh` turns it on for the run and puts
  the previous value back; on anyone else's Mac it never changes the setting, and
  reports `keyboard_navigation` failed, as not measured.
- **iOS**: the test needs the simulator's hardware keyboard and Full Keyboard Access.
  On the iOS 26.3 simulator, turning Full Keyboard Access on made key delivery worse
  (no Tab or ⌘-digit reached the app), so the iOS test still fails at Tab until a
  working setting is found and automated in `audit.sh`.

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
