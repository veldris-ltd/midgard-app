# clients/ — the client layer (Phase 8, SAD_06)

Seven targets over one shared core. Offline-first, key-custody-aware, tiered
inference. Delivered via Bifrost (AC-C-7).

- `core/` — shared **Rust** logic (sync, crypto, key custody, offline store,
  console read models, the demo enclosure and the session), compiled to native
  (via UniFFI) and to WASM.
- `ffi/` — the UniFFI 0.28.3 bindings over the core (Swift now, Kotlin later).
- `demo/` — the signed demo dataset, its fixtures, personas and pinned demo key.
- `sync/`, `store/`, `keystore/`, `broker/`, `delivery/`, `enclosure/`,
  `inference/` — the crates the core is built from.
- `shells/` — the six platform shells (`shells/README.md`): `apple` (macOS and
  iOS, Swift), `android` (Kotlin), `linux-gtk`, `desktop-tauri`, `web` (WASM) and
  `extension` (broker-mode, TS)

> **Status.** The Rust crates are built and tested (acceptance AC-C-1..13,
> `deploy/acceptance/verify-clients.sh`). Of the shells, only `apple` has code: the
> UniFFI binding, the XCFramework build and the shared Swift package `MidgardKit`
> (`shells/SHELL_BUILD.md`). The macOS and iOS apps, and every other shell, are
> not built yet.
