# @midgard/client-shells

The **client platforms** app — the offline-first Midgard client experience
rendered inside device frames (mobile, desktop, web + browser-extension popup),
built on the shared design system and the `@midgard/sdk` gateway seam. It
reproduces the three client prototypes in `docs/design/ui_kits/clients/` as a
real React + TypeScript Vite app.

## What it shows

**One core, many shells** (`[DD-6]`). A single offline-first screen —
[`src/ClientApp.tsx`](src/ClientApp.tsx) — renders identically inside every
platform's chrome. Only the surrounding frame differs. It demonstrates the
`midgard-client-core` model end-to-end:

- **Clearance-scoped cached picture** — visible objects are filtered to policy
  clearance; nothing above it is cached or rendered (`[AC-C-13]`). Each row
  carries a `MarkingChip`.
- **Offline-first CRDT edits** (`[FR-C-4]`) — editing a field writes to the
  sealed local store and **queues a conflict-free pending op**. Queued fields
  and objects show a pending marker.
- **Sync engine** — a `SyncStatus` chip plus a **Sync** button that drains the
  queue with a conflict-free merge. Flip the **offline** switch and watch the
  queue grow while offline, then merge on reconnect.
- **PQ-sealed local store** (`[NFR-PQ-2]`) — AES-256-GCM under an ML-KEM-1024
  data key, custodied by the platform keystore (`KeyCustodyChip`).
- **Inference tiering** (`[FR-C-6]`) — a `TierBadge` shows on-device / hybrid /
  remote; going offline forces the on-device tier (graceful degradation).
- **Minimal-privilege extension** (`[FR-C-8]`) —
  [`src/ExtensionPopup.tsx`](src/ExtensionPopup.tsx) holds no keys and brokers
  the user-shared selection to the enclosure.

## Targets (routes)

Hash routing, so the built app serves from any static path (air-gap-first).

- `/` — **web**: `ChromeWindow` around the core, plus the extension popup.
- `/mobile` — **mobile**: iOS, Android and Linux-mobile bezels over the core.
- `/desktop` — **desktop**: macOS / Windows / Linux window chrome over the core.

A top switcher moves between targets.

## Data seam

Everything flows through the `ClientGateway` interface from `@midgard/sdk`
(`sdk/ts/src/products/clients.ts`). Today the only implementation is the
deterministic in-memory `ClientMock`, injected once at the root via
`DataProvider`. Screens read it with `useGateway<ClientGateway>()` +
`useAsync<T, ClientGateway>()` and never know whether the source is the mock or
a real client-core bridge.

## Develop

```
pnpm --filter @midgard/client-shells dev       # vite dev server on :5179
pnpm --filter @midgard/client-shells typecheck  # tsc --noEmit
pnpm --filter @midgard/client-shells build      # typecheck + vite build
```

House rules: one accent per screen (`.theme-midgard`), never emoji, ALL-CAPS
mono labels, provenance tags as chips, `ProductMark` instead of invented logos,
PQ/governance state visible in the UI.
