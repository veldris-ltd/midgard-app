# Client shells

The six platform-native shell codebases (SAD_06 §3.1). Each is **presentation
only** — navigation, platform integration (notifications, share sheets, deep
links), accessibility, and input. Every shell links the shared Rust core
(`midgard-client-core`) and calls it for everything security- or data-related.
No shell implements cryptography, key handling, or sync-merge logic (**KDD-1**).

| Shell | Technology | Consumes core via | Delivery targets |
|---|---|---|---|
| [`apple`](apple/) | Swift / SwiftUI | UniFFI (Swift) | iOS (App Store), macOS (DMG/App Store) |
| [`android`](android/) | Kotlin / Jetpack Compose | UniFFI (Kotlin) | Android (Play + APK) |
| [`linux-gtk`](linux-gtk/) | Rust / GTK4 | native link | Linux mobile + desktop (Flatpak/AUR) |
| [`desktop-tauri`](desktop-tauri/) | Tauri 2.x + SvelteKit | native link + IPC | Windows (MSI), macOS, Linux desktop |
| [`web`](web/) | SvelteKit + WASM | wasm-bindgen | Enclosure-hosted browser app |
| [`extension`](extension/) | Minimal TS + WASM | wasm-bindgen + broker | Chrome / Firefox stores |

## What is verifiable here, and what is not

These shells require native toolchains (Xcode/Swift, Android/Kotlin, GTK, Node,
the wasm target, Tauri) that the reproducible `rust:1.82` build environment does
not carry, so their UI cannot be compiled or rendered in CI.

What **is** machine-checked in CI (`verify-shells-accessibility.sh`, AC-C-11):

- each shell declares the full required accessibility affordance set
  (`accessibility.toml`) for its OS's guideline set, and
- each shell's `manifest.toml` binds the single shared core (`links_core`),
  never re-implementing crypto/sync (KDD-1).

What runs **off** the deterministic build path (per-platform, on a real
runner/device): the interaction and visual audits — XCUITest, Espresso,
Playwright, the Tauri/GTK harnesses — and manual HIG review.

### On-device audits, recorded per release (RC-49)

Those audits are no longer only a promise. [`.github/workflows/shell-audit.yml`](../../.github/workflows/shell-audit.yml)
runs each shell's audit on the appropriate real runner (macOS + iOS simulator for
`apple`, an Android emulator for `android`, Chromium for `web`/`extension`, the
Tauri webview and AT-SPI2 for the desktop/GTK shells), **measures** the seven
affordances on the running UI (not the declaration), and writes a record to
`clients/shells/<shell>/audits/<release>.toml` (schema:
[`AUDIT_SCHEMA.md`](AUDIT_SCHEMA.md)). [`deploy/acceptance/verify-shell-audits.sh`](../../deploy/acceptance/verify-shell-audits.sh)
then **fails the release closed** unless every shell has a fresh, passing record
for that release. The web-family audit ([`web/a11y/axe-audit.mjs`](web/a11y/axe-audit.mjs),
Playwright + axe-core WCAG 2.1 AA) is the concrete reference; the native audits
follow the same record contract and activate as each shell's UI lands. Records are
written by CI on a real runner — a hand-authored or placeholder record is rejected
by the gate, so a pass cannot be fabricated.

## Auth & inference

Biometric / WebAuthn unlock is provided by the core
(`midgard_client_keystore::auth`): the shell presents the platform prompt and
passes the resulting assertion to the core, which verifies it — no secret ever
crosses into shell code (AC-C-9). Inference tiering
(`midgard_client_inference`) is likewise a core concern; the shell renders the
result and the tier badge, nothing more.
