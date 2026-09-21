# Bifrost release console (`@midgard/console-bifrost`)

The continuous-delivery console for the Midgard suite — PQ-signed artefacts,
progressive rings, fleet convergence, signed offline bundles, and rehearsed
rollback. Vite + React + TypeScript, built on the ported design system and the
shared `ConsoleShell`. Accent: bridge-violet (`theme-bifrost`).

## Run

```bash
pnpm install                                   # from the repo root (once)
pnpm --filter @midgard/console-bifrost dev     # http://localhost:5174
pnpm --filter @midgard/console-bifrost build   # typecheck + production bundle
pnpm --filter @midgard/console-bifrost preview # serve the built bundle
```

Air-gap note: the app builds with `base: './'` and hash routing, so `dist/`
serves from any static path with no server rewrites and no outbound calls.

## Screens (6)

`/` Release console · `/artefacts` Artefact registry · `/channels` Release
channels · `/bundles` Offline bundles · `/fleet` Fleet & drift · `/rollback`
Rollback centre. Nav, collapse-to-rail, and `<900px` drawers come from the
shared `ConsoleShell` (`NAV.bifrost`).

The flagship **Release console** drives the `promoteRelease` flow: the Promote
button advances the broad (PROD-EU) ring one health-gated step at a time —
canary → broad — animating a per-ring `MeterBar` and a health-gate `StatusPill`
and surfacing the signed audit id of every step. The **Rollback centre** runs a
health-gated `rollback` action (executable only while a plan is armed and its
gate is clear) alongside the signed drill flow.

## Data flows — the swap-later seam

Every screen reads data through the `BifrostGateway` interface (`@midgard/sdk`)
via the `DataProvider` hooks (`useAsync`, `useGateway`) — never a concrete
client. Screens type the seam explicitly:

```ts
const g = useGateway<BifrostGateway>();
const { data } = useAsync<BifrostRing[], BifrostGateway>((c) => c.listRings());
```

The app is wired to `BifrostMock` in [`src/main.tsx`](src/main.tsx):

```ts
const gateway: BifrostGateway = new BifrostMock();
```

The mock models real release-plane behaviour: staged ring promotion under a
health gate, reproducible-build + CycloneDX SBOM + ML-DSA (NIST FIPS 204/205)
artefact verification (fail-closed), desired-vs-actual fleet drift, signed
offline bundles, and health-gated rollback — each mutating call emits an
ML-DSA-signed `AuditRecord`. When a browser-reachable bridge exists, swap that
one line for an `HttpGateway` implementing `BifrostGateway`. No screen changes.

## Structure

- `src/main.tsx` — bootstrap: tokens CSS, mode init, DataProvider, router.
- `src/app/` — `routes.tsx` (hash router), `ConsolePage` (router-aware shell wrapper, `product="bifrost"`), `ConsoleToolbar`.
- `src/screens/` — one component per screen (named exports).
