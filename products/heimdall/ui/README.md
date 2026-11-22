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
pnpm --filter @midgard/console-heimdall test    # the globe's pure modules (node:test)
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

## 3D globe

The Operating picture has a **2D/3D** switch in its toolbar. 2D, the reference
layout, is the default; the choice is remembered per browser. Both views share the
layer switches and the inspector selection.

The globe (`src/globe/`) is drawn with [three.js](https://threejs.org) 0.187.0
(MIT) in a "clean" style: flat land and sea in the design tokens' colours, a
graticule, a soft atmosphere edge, and day and night shading from the subsolar
point. Coastlines and land borders come from Natural Earth (public domain), 1:110m
for the world and 1:50m below 3,000 km. Tracks that carry `lat`/`lon` are placed by
layer at their altitude, read from `alt` (`FL340` is 34,000 ft, `SURFACE` and `GND`
are 0) and exaggerated ×4 by default (the HEIGHTS slider, ×1–×20); anomalous tracks
are `--warn`, the rest `--accent`, as in 2D. The mock's tracks sit over northern
Britain and the North Sea, and the footer still says so: **REFERENCE LAYOUT · NOT
REAL POSITIONS**.

| Control | Mouse or touch | Keyboard |
|---|---|---|
| Orbit | drag | arrow keys, with the globe focused |
| Zoom (50 km to 40,000 km) | wheel, pinch, **+** / **−** buttons | `+` / `-` |
| Fly to a region | the REGION list | Tab to it, Enter |
| Inspect a track | hover, click | the track list: focus shows the tooltip, Enter selects and flies to it |

Fly-to is instant under `prefers-reduced-motion`, and the camera's position and
region are announced to screen readers when it settles. Without WebGL the view says
**3D VIEW UNAVAILABLE · WEBGL** and 2D carries on.

Still no outbound calls: three.js and the outlines are bundled, in their own chunk,
which the console fetches from its own files only when 3D is opened. The outlines in
`src/globe/data/` are rebuilt, byte for byte, by
`node tools/heimdall-outlines/build.mjs` (it downloads Natural Earth 5.1.2 and
checks each file's SHA-256), or `--from DIR` with the files already downloaded.

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
- `src/app/` — `routes.tsx` (hash router), `ConsolePage` (router-aware shell wrapper), `ConsoleToolbar`, the 2D/3D `ViewSwitch` and the shared `TrackTooltip`.
- `src/screens/` — one component per screen (six, named exports); each sources
  data through `useGateway<HeimdallGateway>()` + `useAsync<T, HeimdallGateway>`.
- `src/globe/` — the 3D globe: pure modules with tests (projection, altitude,
  camera, presets, track items, outlines, sun), the three.js renderer, and the
  lazily loaded `GlobeView`.
```
