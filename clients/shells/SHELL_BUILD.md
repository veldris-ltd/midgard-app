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
FFI-crossable surface today. Adding the `#[uniffi::export]` / `#[wasm_bindgen]`
attributes over this surface is the drop-in step below.

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

None of these toolchains/targets are present in the reproducible core-build image,
and the binding crates (`uniffi`, `wasm-bindgen`) are **not yet vendored** under
`/vendor` — so the real generated bindings cannot be produced or compiled offline.
Building the six native apps is the platform-team build, run on the appropriate
CI runners.

## Where the shell compile is gated in CI

It already is: [`.github/workflows/shell-audit.yml`](../../.github/workflows/shell-audit.yml)
(RT-49) has per-platform jobs on real runners that **build each shell** (`xcodebuild`,
`gradlew`, `npm build`, the Tauri/GTK builds) before running its accessibility audit,
and **fail closed** when a shell has no buildable project yet. A release is blocked
until every shell compiles and passes its on-device audit. So "the shell compiles
and links the core" is enforced at release time by that gate — it simply cannot be
demonstrated in the offline core-build image.

## Drop-in steps once the toolchains/vendoring land

1. Vendor `uniffi` (+ `uniffi-bindgen`) and `wasm-bindgen` under `/vendor`; add the
   `wasm32-unknown-unknown` and Apple/Android targets to the shell CI images.
2. Annotate the core's binding surface: `#[uniffi::export]` on the `ClientCore`
   impl and its FFI-friendly methods (Apple/Android); `#[wasm_bindgen]` wrappers for
   `web`/`extension`.
3. Land each shell's app project (`apple/`, `android/`, `linux-gtk/`,
   `desktop-tauri/`, `web/`, `extension/`) linking the generated bindings — the
   shell-audit jobs' `exit 1` guards flip to real builds automatically.

Until then, RC-58 stands as: the shared core and its FFI-crossable binding surface
are built and tested; the per-platform app compilation is the gated, CI-enforced
platform build.
