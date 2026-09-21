# clients/ — the client layer (Phase 8, SAD_06)

Seven targets over one shared core. Offline-first, key-custody-aware, tiered
inference. Delivered via Bifrost (AC-C-7).

- `core/` — shared **Rust** logic (sync, crypto, key custody, offline store),
  compiled to native (via UniFFI) and to WASM.
- `shells/`
  - `desktop/` — Tauri (macOS / Windows / Linux chrome)
  - `mobile/` — native iOS (Swift) / Android (Kotlin) / Linux over the shared core
  - `web/` — WASM web app
  - `extension/` — broker-mode browser extension (TS)

> **Status: scaffold only.** Built in Phase 8 against the now-stable core
> contracts. Acceptance: AC-C-1..13.
