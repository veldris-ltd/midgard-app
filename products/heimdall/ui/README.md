# Heimdall console (`@midgard/console-heimdall`)

Operational fusion for the enclosure — the common operating picture, live feeds,
tracked entities, marking-aware picture sync, human-gated action workflows, and
signed decision lineage. Vite + React + TypeScript, watchman-gold accent under
the `theme-heimdall` scope.

## Run

```bash
pnpm install                                    # from the repo root (once)
pnpm --filter @midgard/console-heimdall dev     # http://localhost:5176
pnpm --filter @midgard/console-heimdall build   # typecheck + production bundle
pnpm --filter @midgard/console-heimdall preview # serve the built bundle
```

Air-gap note: the app builds with `base: './'` and hash routing, so the `dist/`
output serves from any static path with no server rewrites and no outbound calls.

## Screens (6)

`/` Operating picture · `/feeds` Feeds · `/entities` Entities · `/sync` Picture
sync · `/workflows` Action workflows · `/lineage` Decision lineage. Nav,
collapse-to-rail, and `<900px` drawers come from the shared `ConsoleShell`.

The flagship **Operating picture** fuses positioned tracks, toggleable layers,
and range rings onto a blueprint-grid (`.mg-grid-bg`) canvas region, with
AI-ranked advisory options in the inspector — each authorised through a human
gate. **Action workflows** is the canonical human-gated flow (see below);
**Decision lineage** replays signed decision records and tails a live lineage
stream via `useAuditStream`.

## Data flows — the swap-later seam

Every screen reads data through the `HeimdallGateway` interface (`@midgard/sdk`)
via the `DataProvider` hooks (`useAsync`, `useGateway`, `useAuditStream`) — never
a concrete client. The app is wired to `HeimdallMock` in
[`src/main.tsx`](src/main.tsx):

```ts
const gateway: HeimdallGateway = new HeimdallMock();
```

The mock models real enclosure behaviour: PQ source-signature verification on
feeds (ML-DSA / SLH-DSA, unverified feeds quarantined out of fusion),
classification + markings on entities and tracks, marking-filtered picture sync,
and — the key flow — **human-gated** `authoriseWorkflow`. Called without a gate
token for a gated workflow it throws `HumanGateRequiredError`; with a token it
emits an ML-DSA-signed `AuditRecord` and returns its id, which the
`HumanGate` component surfaces as `AUTHORISED · SIGNED · AUDIT #…`.

When a browser-reachable bridge exists (a runnable Rust service exposing
HTTP/JSON or gRPC-web over the enclosure crates), swap that one line for a
Heimdall HTTP gateway implementing the same interface. No screen changes.

## Structure

- `src/main.tsx` — bootstrap: tokens CSS, mode init, DataProvider, router.
- `src/app/` — `routes.tsx` (hash router), `ConsolePage` (router-aware shell wrapper), `ConsoleToolbar`.
- `src/screens/` — one component per screen (six, named exports); each sources
  data through `useGateway<HeimdallGateway>()` + `useAsync<T, HeimdallGateway>`.
```
