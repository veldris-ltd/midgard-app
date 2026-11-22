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
pnpm --filter @midgard/console-heimdall test    # the pure modules (node:test), on recorded tiles
pnpm --filter @midgard/console-heimdall test:bundle  # builds with the tiles switch off and on; checks dist/
VITE_HEIMDALL_TILES=off pnpm --filter @midgard/console-heimdall build  # no map tiles at all
```

Air-gap note: the app builds with `base: './'` and hash routing, so the `dist/`
output serves from any static path with no server rewrites. Built with
`VITE_HEIMDALL_TILES=off` it makes no outbound calls at all; with the map tiles on
(the default) it fetches map tiles from the three hosts under **Map tiles** below,
and nothing else.

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

The Operating picture has a **2D · MAP · 3D** switch in its toolbar. 2D, the
reference layout, is the default; the choice is remembered per browser. All three
views share the layer switches and the inspector selection. (MAP needs the map tiles;
a build with them off shows **2D/3D** only.)

The globe (`src/globe/`) is drawn with [three.js](https://threejs.org) 0.187.0
(MIT) in a "clean" style: flat land and sea in the design tokens' colours, a
graticule, a soft atmosphere edge, and day and night shading from the subsolar
point. Coastlines and land borders come from Natural Earth (public domain), 1:110m
for the world and 1:50m below 3,000 km. Tracks that carry `lat`/`lon` are placed by
layer at their altitude, read from `alt` (`FL340` is 34,000 ft, `SURFACE` and `GND`
are 0) and exaggerated ×4 by default (the HEIGHTS slider, ×1–×20), with a stem down to
a shadow on the ground so the height reads at any tilt; aircraft and vessels point
along their heading; anomalous tracks
are `--warn`, the rest `--accent`, as in 2D. The mock's tracks sit over northern
Britain and the North Sea, and the footer still says so: **SYNTHETIC PICTURE ·
REFERENCE LAYOUT · NOT REAL POSITIONS**. The tooltip's RNG row is the same as in 2D:
it is measured on the reference layout's x/y, not from the globe positions.

| Control | Mouse or touch | Keyboard |
|---|---|---|
| Pan | drag | arrow keys, with the globe focused |
| Tilt (0° straight down to 75°) and turn | right-drag, or Ctrl/⌥-drag; two fingers up or down to tilt, twist to turn; **TILT +** / **TILT −** | Shift + arrow keys |
| North up, straight down | the compass button, which shows the heading | Tab to it, Enter |
| Zoom (to 40,000 km; down to 1.5 km with the map tiles, see below; without them 50 km, the floor rising to stay 25 km above the highest marker) | wheel, pinch, **+** / **−** buttons | `+` / `-` |
| Fly to a region | the REGION list | Tab to it, Enter |
| Inspect a track | hover, click | the track list: focus shows the tooltip, Enter selects and flies to it |

The camera tilts about the point in the middle of the view: fully (75°) close in,
less far out, so the horizon stays in the frame. WORLD looks straight down; the other
regions open tilted 45–60°, so the curve of the Earth, the horizon and the sky show.

Fly-to is instant under `prefers-reduced-motion`, and the camera's position, tilt,
heading and region are announced to screen readers when it settles. Without WebGL the view says
**3D VIEW UNAVAILABLE · WEBGL** and 2D carries on.

three.js is bundled in the globe's own chunk, and the outlines ship as two hashed JSON
assets beside it; the console fetches both from its own files, only when 3D is
opened. With the map tiles off, that is all the globe ever loads. The build writes `dist/THIRD-PARTY-LICENSES.md`
with the licence text of every bundled dependency. The outlines in
`src/globe/data/` are rebuilt, byte for byte, by
`node tools/heimdall-outlines/build.mjs` (it downloads Natural Earth 5.1.2 and
checks each file's SHA-256), or `--from DIR` with the files already downloaded.

## Map tiles

With `VITE_HEIMDALL_TILES` on (the default; `off`, `false`, `0` or `no` turn it off
at build time) the console adds map tiles to the globe and a second 2D mode, **MAP**.

- **Terrain** on the globe, from Mapzen Terrain Tiles on AWS Open Data (terrarium
  PNGs), raised by the HEIGHTS setting. Detail is fetched only for the area in view,
  at most 160 tiles drawn plus wanted at once, and cancelled when the view moves on.
- **STYLE** on the globe: **CLEAN** (the default, the flat token colours),
  **TOPOGRAPHIC** (tints, hillshade and contours drawn from the terrain tiles, light or
  dark with the console's mode) and **SATELLITE** (EOX Sentinel-2 cloudless 2016,
  CC BY 4.0, the `s2cloudless_3857` layer only), remembered per browser.
- **Close-up**: with tiles drawn, the camera may come down to 1.5 km, and always
  stays 150 m above the terrain under it; surface tracks stand on the terrain. The
  outline lines hide below 150 km, where the imagery or relief shows the coast.
- **Relief, tilted**: tilt, turn and zoom pivot on the ground in view, not on sea
  level under it; the tiles follow a tilted view out towards the horizon, and none
  are fetched behind the camera; the view tips down rather than look into a hill
  between the camera and the point in view. The REGION list adds **SNOWDONIA** and
  **SOUTHERN UPLANDS**, close tilted views of the relief, while tiles are drawn.
- **MAP**: the tracks on a real map from VersaTiles vector tiles (OpenStreetMap data,
  ODbL), decoded with `@mapbox/vector-tile` and `pbf` and drawn on a canvas in the
  design tokens' colours, over the bundled Natural Earth land. The globe's REGION
  presets and track list work the same way; arrow keys pan and `+`/`-` zoom the
  focused map; zoom 3 to 16.
- **ⓘ CREDITS**, in the side panel of the globe and the map, lists every source in
  use with its licence and attribution. `NOTICE` records them all.
- **Offline**: when the browser says it is offline, after three network failures in a
  row, or when nothing has loaded yet and every tile request has failed, the globe
  falls back to the plain 1a globe and both views say **MAP TILES NEED A
  CONNECTION**; the map keeps the bundled coastlines and the globe's STYLE buttons are
  marked unavailable. Thirty seconds later the loader asks the view to try again and
  lets a single tile request through as a probe; if it answers, tiles return, and if
  not, the next probe is another 30 s away. A server error (5xx or 429) is not offline:
  that tile is retried after 30 s.

Every request goes through one loader (`src/tiles/loader.ts`): only URLs on the
allow-list in `src/tiles/hosts.ts` (HTTPS, the exact host and path, a tile in range,
no port, credentials, query or fragment), sent with no cookies (`credentials:
'omit'`), no referrer and no redirects. Tile requests reveal the area being viewed to
the tile hosts; the residual-risk register records this
(`docs/compliance/RESIDUAL_RISK_REGISTER.md`, SAD_04). For an enclosure, build with the
switch off.

**CSP.** A page served with a Content Security Policy must allow these origins in
`connect-src` (tiles are fetched, then decoded from memory, so no `img-src` entry is
needed):

| Host | What | Path |
|---|---|---|
| `https://s3.amazonaws.com` | Mapzen Terrain Tiles, terrarium | `/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png` |
| `https://tiles.maps.eox.at` | Sentinel-2 cloudless 2016 | `/wmts/1.0.0/s2cloudless_3857/default/g/{z}/{y}/{x}.jpg` |
| `https://tiles.versatiles.org` | VersaTiles OSM vector tiles | `/tiles/osm/{z}/{x}/{y}` |

for example `connect-src 'self' https://s3.amazonaws.com https://tiles.maps.eox.at
https://tiles.versatiles.org`. The installer's CSP is unchanged: the installer serves
only the Midgard console.

**The switch.** `__HEIMDALL_TILES__` is fixed at build time (`vite.config.ts`), and
every module that fetches tiles is reached only behind it, so a build with it off
behaves as the globe did before the tiles and its `dist/` holds no tile code and no
tile host name. `pnpm test:bundle` (`scripts/tiles-switch.test.mjs`) builds it both
ways and checks exactly that.

**Tests** run on three recorded tiles in `fixtures/tiles/` (one of each kind, with
their licences, credits and checksums in `CREDITS.md`) through an injected `fetch`;
no test touches the network.

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
- `src/app/` — `routes.tsx` (hash router), `ConsolePage` (router-aware shell wrapper), `ConsoleToolbar`, the 2D · MAP · 3D `ViewSwitch`, the shared `TrackTooltip` and the `Credits` control.
- `src/screens/` — one component per screen (six, named exports); each sources
  data through `useGateway<HeimdallGateway>()` + `useAsync<T, HeimdallGateway>`.
- `src/globe/` — the 3D globe: pure modules with tests (projection, altitude,
  camera, presets, track items, outlines, sun), the three.js renderer, and the
  lazily loaded `GlobeView`.
- `src/tiles/` — the map tiles, behind the switch: tile maths, the host allow-list,
  the loader and its cache, tile selection within the budget, terrarium decoding, the
  terrain mesh and the topographic style (each with tests), and the globe's tile layer.
- `src/map/` — the 2D geographic map: its camera and the vector-tile features (with
  tests), the canvas renderer, and the lazily loaded `MapView`.
```
