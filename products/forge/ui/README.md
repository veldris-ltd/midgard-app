# Forge console (`@midgard/console-forge`)

The data-operations console for the Midgard enclosure — ingest, transform,
version, and project data into the object fabric. The second product console
built on the ported design system (ember-orange accent, `theme-forge` scope).
Vite + React + TypeScript.

## Run

```bash
pnpm install                                 # from the repo root (once)
pnpm --filter @midgard/console-forge dev     # http://localhost:5174
pnpm --filter @midgard/console-forge build   # typecheck + production bundle
pnpm --filter @midgard/console-forge preview # serve the built bundle
```

Air-gap note: the app builds with `base: './'` and hash routing, so the `dist/`
output serves from any static path with no server rewrites and no outbound calls
(fonts are the only CDN import in the token layer — vendor them locally for a
fully offline build; see `ui/design-system/src/tokens/fonts.css`).

## Screens (7)

`/` Pipeline builder · `/connectors` Connectors · `/datasets` Datasets ·
`/health` Data health · `/schedules` Schedules · `/ide` Transform IDE ·
`/projection` Object projection. Nav, collapse-to-rail, and `<900px` drawers come
from the shared `ConsoleShell`.

The flagship is the **pipeline builder**: a DAG of sources → transforms → dataset
→ projection, with an interactive `runPipeline` build that steps through staged
phases (MeterBar + StatusPill) and streams a signed build log. The **Transform
IDE** demonstrates preview-before-commit — `previewTransform` renders a sandboxed
diff and the commit is a **human-gated** `applyTransform` (`ForgeHumanGateRequiredError`).

## Data flows — the swap-later seam

Every screen reads data through the `ForgeGateway` interface (`@midgard/sdk`) via
the `DataProvider` hooks (`useAsync`, `useGateway`, `useAuditStream`) — never a
concrete client. The app is wired to `ForgeMock` in
[`src/main.tsx`](src/main.tsx):

```ts
const gateway: ForgeGateway = new ForgeMock();
```

The mock is deterministic (no `Date.now()`/`Math.random()` at module load) and
models real enclosure behaviour: staged builds, previewed-then-gated transform
commits that emit ML-DSA-signed `AuditRecord`s, and a live audit stream. When a
browser-reachable bridge exists (a runnable Rust service exposing HTTP/JSON or
gRPC-web over the enclosure crates — see `sdk/ts/src/http/HttpGateway.ts` for
what that requires), swap that one line for a `createHttpGateway`-style adapter
that implements `ForgeGateway`. No screen changes.

## Structure

- `src/main.tsx` — bootstrap: tokens CSS, mode init, DataProvider, router.
- `src/app/` — `routes.tsx` (hash router), `ConsolePage` (router-aware shell wrapper), `ConsoleToolbar`.
- `src/screens/` — one component per screen; the flagship reference is `PipelineBuilderScreen.tsx`.
