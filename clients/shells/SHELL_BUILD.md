# Building the shells against `midgard-client-core` (RT-58 / RC-58)

The six shells are **presentation only**; every one links the single shared Rust
core (`midgard-client-core`) and calls it for all crypto/sync/storage (KDD-1). This
document records how the shells bind the core, what is buildable in the offline
toolchain today, and — honestly — what remains gated on the platform toolchains.

## The binding surface (buildable now)

The shells reach the core through language bindings generated from its Rust API:

| Shells | Binding | Generates |
|--------|---------|-----------|
| `apple` (Swift), `android` (Kotlin) | **UniFFI** | Swift / Kotlin over the core |
| `web`, `extension` (WASM) | **wasm-bindgen** | JS/TS over a `wasm32` build |
| `linux-gtk` (Rust/GTK4), `desktop-tauri` (Rust + webview) | **native link** | direct Rust dependency |

A binding cannot carry a borrowed `&[SyncOp]` or the CRDT `SyncOp` type across the
FFI boundary, and — per KDD-1 — a shell must never touch merge internals. So the
core now exposes an **FFI-friendly surface** where CRDT ops cross as **opaque owned
byte blobs**:

- [`SyncOp::encode`](../sync/src/lib.rs) / `SyncOp::decode` — canonical,
  dependency-free, fail-closed op serialisation.
- [`ClientCore::drain_pending_encoded`](../core/src/lib.rs) /
  `merge_remote_encoded` — the FFI-shaped counterparts of `drain_pending` /
  `merge_remote`. A shell drains opaque blobs, ships them to the enclosure relay,
  and feeds returned blobs back, never inspecting them; a malformed blob is
  rejected (`CoreError::MalformedOp`), never applied.

These compile and are tested in the offline toolchain
(`binding_surface_converges_over_opaque_blobs`), so the core presents a linkable,
FFI-crossable surface today. For Apple, `clients/ffi` now exports the core through
UniFFI (see "Apple" below); the `#[wasm_bindgen]` wrappers remain the drop-in step
below.

## What is gated (and why)

RC-58 — *"each shell compiles and links the shared core on its platform"* — is a
**platform-toolchain activity** and is not runnable in the offline `rust:1.82`
build image. To actually compile the shells you need, per platform:

- **`apple`**: macOS + Xcode/Swift, the `aarch64-apple-ios`/`-darwin` targets,
  `uniffi-bindgen` to emit Swift.
- **`android`**: Android SDK + NDK, Kotlin/Gradle, the Android targets,
  `uniffi-bindgen` to emit Kotlin.
- **`web` / `extension`**: Node + `wasm-pack`, the **`wasm32-unknown-unknown`**
  target, and the **`wasm-bindgen`** crate.
- **`linux-gtk`**: GTK4 dev libraries.
- **`desktop-tauri`**: Node + Tauri 2 + the platform webview.

None of these toolchains/targets are present in the reproducible core-build image.
**UniFFI is vendored** (0.28.3, under `/vendor`, with time-limited MPL-2.0 licence
exceptions in `supply-chain/licences.toml`): the crate
[`clients/ffi`](../ffi/) (`midgard-client-ffi`) exports the core through it and
builds in the offline image like any other crate (`just test`, `just lint`), and
the Apple binding is built on a Mac (below). **`wasm-bindgen` is not yet vendored**,
so the `web`/`extension` bindings cannot be produced offline. Building the native
apps is the platform-team build, run on the appropriate CI runners.

## Apple (built)

The Apple binding and the shared Swift package exist; the macOS and iOS apps are
the next step (branches `clients_mac`, `clients_iphone`).

**Prerequisites.** A Mac with Xcode 26.2 (or 16.x), rustup (the repository pins
Rust 1.82.0 in `rust-toolchain.toml`), the Apple targets
(`rustup target add aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios
aarch64-apple-ios-sim --toolchain 1.82.0`), Node 22 (token generation only).

**Layout.**

| Path | What it is |
|---|---|
| `clients/ffi/` | `midgard-client-ffi`: the UniFFI surface over the core (`ClientConfig`, `MidgardClient`, the key-root callback), a `staticlib` for Apple and a Rust `lib` for its tests; `examples/uniffi-bindgen.rs` is the vendored generator |
| `clients/shells/apple/Package.swift` | the Swift package (Swift 5 language mode; macOS 14, iOS 17) |
| `Sources/MidgardCore/` | the generated `midgard_client_ffi.swift` (gitignored, copied in by the build) over the `MidgardCoreFFI` XCFramework (`Frameworks/`, a gitignored link) |
| `Sources/MidgardKit/` | shared SwiftUI layer: design tokens (generated), Secure Enclave key root, sign-in, lock, chips, acknowledgements |
| `Sources/MidgardAuditKit/` | the accessibility audit's base class that each app's UI-test target subclasses |
| `Tests/MidgardKitTests/` | package tests, including the contract test over every exported binding |
| `audit.sh` | the on-device accessibility audit (`tools/ci/run.sh shell-audit apple`) |

**Build and test** (outputs go only to `~/Library/Caches/midgard-build/`):

```bash
cd clients/shells/apple
bash tools/build-xcframework.sh   # 4 slices (macOS arm64+x86_64, iOS, iOS simulator) + bindings, offline and locked;
                                  # runs tools/verify-xcframework.sh before linking anything
bash tools/ensure-xcframework.sh  # the apps' pre-build step: links a build of the Rust sources as they
                                  # stand, building only when their content changed (stamp in clients-xcframework/)
swift test --scratch-path ~/Library/Caches/midgard-build/clients-swift
bash tools/check-contract.sh      # every exported function is exercised by ContractTests.swift
bash tools/check-bindings.sh      # the bindings are a fresh uniffi-bindgen run's; acknowledgement; licence expiry
node tools/gen-tokens.mjs --check # design tokens and fonts match ui/design-system
bash tools/typecheck-ios.sh       # typecheck for iOS without a simulator runtime
bash tools/clean.sh [--target]    # delete every clients-* output (and the cargo target dir)
```

`tools/ci/tests/apple-xcframework.test.sh` checks these scripts statically (and
`check-bindings.sh` and `clean.sh` against scratch trees) in CI `selftest`;
`tools/ci/tests/shell-audit-apple.test.sh` checks `audit.sh` with stub tools;
`tools/ci/tests/apple-ensure-xcframework.test.sh` checks `ensure-xcframework.sh` (stamp,
lock, stale lock, input list) with a stand-in build; `tools/ci/tests/apple-projects.test.sh`
checks that each committed `Apps/<App>/<App>.xcodeproj` is what its `project.yml` generates
(skipped where XcodeGen is not installed).

**Accessibility audit.** `clients/shells/apple/audit.sh <release> <report>` builds
both apps, runs each app's `AccessibilityAudit` UI test (XCUITest +
`XCUIAccessibilityAudit`) on an iPhone simulator and on the Mac, and names the
devices it audited; `tools/ci/run.sh shell-audit apple` records the result. It
refuses a tree without both apps, so until the apps exist it records nothing and the
release gate stays closed.

**Apple release checklist** (MPL-2.0, UniFFI):

1. Both apps show the acknowledgement "UniFFI 0.28.3, Mozilla Public License 2.0,
   source: https://github.com/mozilla/uniffi-rs/tree/v0.28.3" (Settings →
   Acknowledgements, from `MidgardKit.Acknowledgements`), and the release notes'
   third-party list carries the same line.
2. The bundled Swift bindings are a fresh `uniffi-bindgen` run's, never edited
   (MPL-2.0 §3.2): `bash clients/shells/apple/tools/check-bindings.sh` after the
   release build.
3. The UniFFI licence exceptions in `supply-chain/licences.toml` expire on
   **2027-05-13**: renew (with the owner's approval) or remove them before then.
   `check-bindings.sh` warns from 60 days before and fails after; the release
   licence gate fails on an expired exception in any case.

## Where the shell compile is gated in CI

It already is: [`.github/workflows/shell-audit.yml`](../../.github/workflows/shell-audit.yml)
(RT-49) has per-platform jobs on real runners that **build each shell** (`xcodebuild`,
`gradlew`, `npm build`, the Tauri/GTK builds) before running its accessibility audit,
and **fail closed** when a shell has no buildable project yet. A release is blocked
until every shell compiles and passes its on-device audit. So "the shell compiles
and links the core" is enforced at release time by that gate — it simply cannot be
demonstrated in the offline core-build image.

## Drop-in steps for the other shells

1. Vendor `wasm-bindgen` under `/vendor` (UniFFI is vendored); add the
   `wasm32-unknown-unknown` and Android targets to the shell CI images.
2. Export the core's binding surface: Android reuses `clients/ffi` (UniFFI emits
   Kotlin from the same crate); `#[wasm_bindgen]` wrappers for `web`/`extension`.
3. Land each shell's app project (`apple/`, `android/`, `linux-gtk/`,
   `desktop-tauri/`, `web/`, `extension/`) linking the generated bindings — the
   shell-audit jobs' `exit 1` guards flip to real builds automatically.

Until then, RC-58 stands as: the shared core and its FFI-crossable binding surface
are built and tested; the per-platform app compilation is the gated, CI-enforced
platform build.
