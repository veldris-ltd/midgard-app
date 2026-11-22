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
pnpm --filter @midgard/console-heimdall test:bundle  # builds none, mirror and public twice each; checks dist/

# Map tiles (see Map tiles below); unset is none, no map tiles at all:
VITE_HEIMDALL_TILE_SOURCE=mirror VITE_HEIMDALL_TILE_MIRROR=https://tiles.enclosure.internal/heimdall \
  pnpm --filter @midgard/console-heimdall build    # tiles from the enclosure's own mirror
VITE_HEIMDALL_TILE_SOURCE=public pnpm --filter @midgard/console-heimdall build  # demo only: public hosts
```

Air-gap note: the app builds with `base: './'` and hash routing, so the `dist/`
output serves from any static path with no server rewrites. By default
(`VITE_HEIMDALL_TILE_SOURCE` unset, or `none`) it makes no outbound calls at all, and
that is the build the release signs. A `mirror` build fetches map tiles from the one
mirror origin it was built with, inside the enclosure; only a `public` build, a demo
outside the enclosure, fetches them from the three public hosts under **Map tiles**.

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
a `none` build shows **2D/3D** only.)

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
opened. In a `none` build, that is all the globe ever loads. The build writes `dist/THIRD-PARTY-LICENSES.md`
with the licence text of every bundled dependency. The outlines in
`src/globe/data/` are rebuilt, byte for byte, by
`node tools/heimdall-outlines/build.mjs` (it downloads Natural Earth 5.1.2 and
checks each file's SHA-256), or `--from DIR` with the files already downloaded.

## Map tiles

The tile source is fixed at build time by `VITE_HEIMDALL_TILE_SOURCE`
(`src/tileSource.ts`), and fails closed:

| Value | Tiles from | Use |
|---|---|---|
| unset, or `none` | nowhere: no tile code and no tile host in `dist/` | the default; the signed release artefact |
| `mirror` | `VITE_HEIMDALL_TILE_MIRROR` only, an `https://` origin with an optional base path | an enclosure with its own tile mirror |
| `public` | the three public hosts below | a demo outside the enclosure only |

Any other value, an empty one included, stops the build with an error, and so does a
mirror that is missing, not `https://`, carries credentials, a query or a fragment, or
is not written in its canonical form; so does `VITE_HEIMDALL_TILE_MIRROR` without
`mirror`, and the retired `VITE_HEIMDALL_TILES`. With map tiles (`mirror` or `public`)
the console adds them to the globe and a second 2D mode, **MAP**.

- **Terrain** on the globe, from terrarium-encoded terrain tiles (Mapzen Terrain Tiles),
  raised by the HEIGHTS setting. Detail is fetched only for the area in view, at most
  160 tiles drawn plus wanted at once, and cancelled when the view moves on.
- **STYLE** on the globe: **CLEAN** (the default, the flat token colours),
  **TOPOGRAPHIC** (tints, hillshade and contours drawn from the terrain tiles, light or
  dark with the console's mode) and **SATELLITE** (EOX Sentinel-2 cloudless 2016,
  CC BY 4.0, that layer only), remembered per browser.
- **Close-up**: with tiles drawn, the camera may come down to 1.5 km, and always
  stays 150 m above the terrain under it; surface tracks stand on the terrain. The
  outline lines hide below 150 km, where the imagery or relief shows the coast.
- **Relief, tilted**: tilt, turn and zoom pivot on the ground in view, not on sea
  level under it; the tiles follow a tilted view out towards the horizon, and none
  are fetched behind the camera; the view tips down rather than look into a hill
  between the camera and the point in view. The REGION list adds **SNOWDONIA** and
  **SOUTHERN UPLANDS**, close tilted views of the relief, while tiles are drawn.
- **MAP**: the tracks on a real map from OpenStreetMap vector tiles in the Shortbread
  schema (VersaTiles; ODbL), decoded with `@mapbox/vector-tile` and `pbf` and drawn on
  a canvas in the design tokens' colours, over the bundled Natural Earth land. The
  globe's REGION presets and track list work the same way; arrow keys pan and `+`/`-`
  zoom the focused map; zoom 3 to 16. At most 96 tiles are asked for at once, nearest
  the centre first, and the tiles on screen are never evicted, so a very large screen
  does not keep refetching.
- **ⓘ CREDITS**, in the side panel of the globe and the map, lists every source in
  use with its licence and attribution. `NOTICE` records them all.
- **Paused after track moves (`public` only)**: a public tile request names the area in
  view, so a view the tracks decide would tell the tile hosts where the tracks are. In
  a `public` build the first view (SECTOR NORTH, fitted to the tracks), flying to a
  track, and any preset fitted to track positions pause tile loading, and both views
  say **MAP TILES PAUSED · MOVE THE VIEW TO LOAD**; tiles already loaded still show.
  Loading resumes only on the user's own move that is not centred on a track: a drag,
  a zoom, a tilt or turn, the arrow keys, or a geographic preset such as UK & IRELAND
  or SNOWDONIA (`src/tilePause.ts`). A `mirror` build never pauses.
- **Offline**: when the browser says it is offline, after three network failures in a
  row, or when nothing has loaded yet and every tile request has failed, the globe
  falls back to the plain 1a globe and both views say **MAP TILES NEED A
  CONNECTION**; the map keeps the bundled coastlines and the globe's STYLE buttons are
  marked unavailable. Thirty seconds later the loader asks the view to try again and
  lets a single tile request through as a probe; if it answers, tiles return, and if
  not, the next probe is another 30 s away (a request sent before the network went,
  failing only later, does not push the probe back). A server error (5xx or 429) is not offline:
  that tile is retried after 30 s.

Every request goes through one loader (`src/tiles/loader.ts`): only URLs on the
build's allow-list (`src/tiles/hosts.ts`: HTTPS, the exact origin and path, a tile in
range, no other port, no credentials, query or fragment), sent with no cookies
(`credentials: 'omit'`), no referrer and no redirects, and ended after 15 s (a timeout
is a network failure). Tiles are untrusted input, so:

- **Size.** The body is read as a stream and stopped at 2 MB for imagery and vector
  tiles and 1 MB for terrain; a larger `Content-Length` is refused before reading. An
  oversize tile is a permanent "missing" tile.
- **Dimensions.** A raster tile's PNG or JPEG header must say exactly 256 × 256 (or
  512 × 512 for terrain) before it is decoded, and the decoded bitmap is checked again
  before its pixels are read or it becomes a texture.
- **Complexity.** A vector tile may have at most 50,000 features in a layer, 100,000
  in all and 1,000,000 points, counted from the encoded commands before any geometry
  is built (a repeated ClosePath can claim millions of points in a few bytes);
  otherwise it is refused as too complex.
- **Worker.** Decoding runs in a same-origin module worker (`src/tiles/decodeWorker.ts`,
  its own file in `dist/`, never a `blob:` URL), about 2 s per tile; a worker that runs
  over is terminated and replaced, and the tile is retried after 30 s.

Public tile requests reveal the area being viewed, and with it the client's address,
the console's origin and the times, to the tile hosts and to DNS, and leave the tiles
in the browser's HTTP cache; the residual-risk register records this
(`docs/compliance/RESIDUAL_RISK_REGISTER.md`). For an enclosure, use `none` or a
`mirror`.

**CSP.** A page served with a Content Security Policy must allow the tile origin in
`connect-src` (tiles are fetched, then decoded from memory, so no `img-src` entry is
needed) and the decode worker in `worker-src 'self'`. For a `mirror` build that is
just the mirror's origin, for example `connect-src 'self'
https://tiles.enclosure.internal`. A `public` build needs the three public hosts:

| Host | What | Path |
|---|---|---|
| `https://s3.amazonaws.com` | Mapzen Terrain Tiles, terrarium | `/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png` |
| `https://tiles.maps.eox.at` | Sentinel-2 cloudless 2016 | `/wmts/1.0.0/s2cloudless_3857/default/g/{z}/{y}/{x}.jpg` |
| `https://tiles.versatiles.org` | VersaTiles OSM vector tiles | `/tiles/osm/{z}/{x}/{y}` |

for example `connect-src 'self' https://s3.amazonaws.com https://tiles.maps.eox.at
https://tiles.versatiles.org`. The installer's CSP is unchanged: the installer serves
only the Midgard console.

**The build constants.** `vite.config.ts` turns the setting into `__HEIMDALL_TILES__`,
`__HEIMDALL_TILE_PUBLIC__` and `__HEIMDALL_TILE_MIRROR__` (`src/tilesSwitch.d.ts`).
Every module that fetches tiles is reached only behind `__HEIMDALL_TILES__`, so a
`none` build behaves as the globe did before the tiles and its `dist/` holds no tile
code and no tile host name; the public hosts (`src/tiles/publicHosts.ts`) only behind
`__HEIMDALL_TILE_PUBLIC__`, so a `mirror` build holds none of them. `pnpm test:bundle`
(`scripts/tile-source.test.mjs`) builds each mode twice, checks exactly that, checks
the two builds are byte-identical, and checks that bad settings fail the build.

### Filling a tile mirror

A mirror is any HTTPS static file server inside the enclosure. The console asks it for
exactly these paths under the mirror URL it was built with, and nothing else:

| Path | Format | Zooms the console asks for | Upstream source | Licence |
|---|---|---|---|---|
| `<mirror>/terrarium/{z}/{x}/{y}.png` | terrarium-encoded PNG, 256 × 256 (or 512 × 512) | 0–13 | Mapzen Terrain Tiles on the AWS Open Data registry, the public bucket `s3://elevation-tiles-prod/terrarium/` (same layout) | open data with attribution; the full list is in `NOTICE` |
| `<mirror>/s2cloudless-2016/{z}/{x}/{y}.jpg` | JPEG, 256 × 256, Web Mercator | 0–14 | EOX Sentinel-2 cloudless **2016** (`s2cloudless_3857`, tile matrix set `g`); note that EOX's WMTS path is `{z}/{y}/{x}`, row first, so the mirror swaps them | CC BY 4.0 (the 2016 layer only: later years are CC BY-NC-SA) |
| `<mirror>/osm/{z}/{x}/{y}.pbf` | Mapbox Vector Tile, Shortbread 1.x schema | 0–14 | OpenStreetMap vector tiles from VersaTiles: its planet container (`osm.versatiles`, from download.versatiles.org) unpacked to a `{z}/{x}/{y}.pbf` tree with the `versatiles` tool, for the whole planet or an area | ODbL 1.0 (© OpenStreetMap contributors) |

- Fetch the upstream data once, outside the enclosure, for the area and zooms the
  enclosure needs, and carry it in like any other import; respect each provider's
  terms and rate limits for bulk downloads. Keep the credits: the console shows them
  (ⓘ CREDITS), and `NOTICE` and `fixtures/tiles/CREDITS.md` give the full text.
- A tile the mirror does not hold must answer **404** (remembered as missing, not
  retried); 5xx and 429 are retried after 30 s. No redirects: the console refuses them.
- Vector tiles may be stored gzip-compressed if the server sends them with
  `Content-Encoding: gzip`; the 2 MB cap applies to the inflated bytes.
- Tiles over the caps above (1 MB terrain, 2 MB imagery and vector), raster tiles of
  another size, and vector tiles past the complexity limits are refused.
- The server must allow the console's origin by CORS (`Access-Control-Allow-Origin`;
  requests carry no credentials), and its certificate must be one the enclosure's
  browsers trust.

**Tests** run on three recorded tiles in `fixtures/tiles/` (one of each kind, with
their licences, credits and checksums in `CREDITS.md`), and on hostile tiles the tests
generate (a large-dimension PNG, a vector-tile decompression bomb, a gzip bomb, a slow
body), through an injected `fetch`; no test touches the network.

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
- `src/tileSource.ts` — the tile source setting; `src/tilePause.ts` — the public
  build's pause after track moves (both with tests).
- `src/tiles/` — the map tiles, only in a build with them: tile maths, the sources and
  allow-lists (`hosts.ts`, `publicHosts.ts`, the build's own set in `active.ts`), the
  loader and its cache, the decode limits, worker and pool, tile selection within the
  budget, terrarium decoding, the terrain mesh and the topographic style (each with
  tests), and the globe's tile layer.
- `src/map/` — the 2D geographic map: its camera and the vector-tile features (with
  tests), the canvas renderer, and the lazily loaded `MapView`.
```
