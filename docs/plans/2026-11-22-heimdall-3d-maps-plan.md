# Heimdall 3D globe and maps in the core release

**Goal:** add a 3D globe and map tiles to the web Heimdall console
(`products/heimdall/ui`), written fresh on the signed release.

**Base:** `main`, which is `v0.1.0-rc.3` plus #97. No other branch is read, merged or
ported. The console's existing data model is the only data, so nothing new is fetched
apart from the Stage 1b map tiles.

## Owner's stages (22 Nov 2026)

| Stage | What | Where |
|---|---|---|
| **1a: 3D** | Built on `heimdall_3d` from `main`: the globe, with bundled data only and no network | Kept local |
| **1b: Maps** | Built on `heimdall_maps`, on `heimdall_3d`: map tiles and a 2D geographic map | Kept local |
| **1: combined** | Branch `heimdall_3dmaps` = 1a + 1b in one build (owner, 22 Nov) | Pushed; **one** pull request into `dev` |
| **2** | Apple clients: the signed release plus 3D, built from 1a | `clients_*` branches |
| **3** | Apple clients: Stage 2 plus maps, built from 1b | The same branches, as an update |

## Decisions

- **Provenance tags:** `[FR-H-1]` (one shared operational picture with layer control),
  `[AC-H-1]` where layer toggles are involved, and `[AC-25.2]` for mock fixtures.
- **Default view:** 2D stays the default. The globe is opt-in through a 2D/3D switch.
- **Height exaggeration:** ×4 by default, with a HEIGHTS slider.
- **Tiles switch (1b):** `VITE_HEIMDALL_TILES`, on by default. With it off, the built
  bundle contains no tile host names.
- **Licences:** ISC goes on the allow-list in `supply-chain/licences.toml` if a 1b
  dependency needs it (owner sign-off). Each data source's licence is checked when it is
  added, and recorded in `NOTICE`.
- **Imagery:** EOX Sentinel‑2 cloudless, the 2016 layer only (CC BY 4.0). The later
  yearly layers are not used.
- **Installer CSP:** unchanged, because the installer serves only the Midgard console.
  The README lists the hosts a CSP must allow.

## Stage 1a: `heimdall_3d`

**Behaviour:**
1. **2D/3D switch.** It sits in the operating picture's toolbar and is remembered per
   browser. The layer switches and the inspector selection are shared by both views.
2. **Globe.**
   - A WebGL globe built with `three` (MIT), pinned to an exact version.
   - Bundled land and sea outlines and country borders from Natural Earth (public
     domain): 1:110m for the world and 1:50m close in.
   - A graticule, a soft atmosphere edge, and day and night shading from the subsolar
     point.
   - A "clean" style: flat land and sea colours from the design tokens, with no imagery.
3. **Tracks.**
   - **Data:** `main`'s Heimdall tracks get optional `lat`/`lon` in the SDK (additive and
     type-only). The five mock fixtures get plausible positions over the UK and its seas.
   - **Placement:** markers by layer (air, maritime, ground) at their altitude, parsed
     from `alt` (`FL340` → 34,000 ft; `SURFACE`/`GND` → 0) and exaggerated by the
     HEIGHTS setting.
   - **Colours:** anomalous tracks use `--warn`, the rest `--accent`, as in 2D.
   - **Interaction:** hover shows the same rows as the 2D tooltip, and click selects the
     track in the shared inspector.
4. **Camera.**
   - Drag to orbit, scroll or pinch to zoom, between 50 km and 40,000 km.
   - Eased fly-to.
   - Presets: WORLD, UK & IRELAND, NORTH SEA, CHANNEL, and SECTOR NORTH (on the mock
     tracks).
   - A REGION list built from plain buttons, which fits the window and scrolls.
5. **Accessibility.**
   - Keyboard: every control is reachable, with a visible focus ring, and a list of the
     tracks gives keyboard access to each one.
   - Fly-to respects `prefers-reduced-motion`.
   - A text summary of the camera's position and region is available to screen readers.
6. **Failure.** Without WebGL the view shows "3D VIEW UNAVAILABLE · WEBGL" and the 2D view
   still works.
7. **Footer.** "REFERENCE LAYOUT · NOT REAL POSITIONS", the same honesty note the 2D
   reference layout uses.
8. **Performance.** `three` and the bundled data load lazily, in their own chunk, only
   when 3D is opened.

**Tasks (TDD for the pure modules):**
1. Dependencies: `three` and `@types/three`, the lockfile, the npm advisory refresh, a
   node:test `test` script.
2. SDK `lat`/`lon`, and the fixture positions.
3. Pure modules: projection (lat/lon/alt to xyz and back), the altitude parser, camera
   limits, presets and regions, track-to-globe items.
4. Bundled outline data, with a small reproducible script under `tools/` that builds it
   from the Natural Earth downloads and records their versions and checksums.
5. The globe renderer.
6. The React view, the switch, the REGION list and the track list.
7. `NOTICE` (Natural Earth) and the Heimdall README (3D, keys, still no outbound calls).
8. Verify: build, tests, provenance, signed commits with DCO, a bit-identical double
   build, a manual WebGL check in Safari or Chromium, an axe pass.

## Stage 1b: `heimdall_maps`, on `heimdall_3d`

**Behaviour:**
1. **Terrain** on the globe from Mapzen Terrain Tiles on AWS Open Data (terrarium
   encoding, attribution required). Detail is fetched only for the area in view, within a
   tile budget.
2. **Styles:** CLEAN (from 1a), TOPOGRAPHIC (hillshade and contours from the terrain
   tiles), and SATELLITE (EOX Sentinel‑2 cloudless 2016).
3. **Close-up.** Zoom down to 1.5 km above the ground, with the camera kept at least
   150 m above the terrain under it.
4. **2D geographic map.**
   - A second 2D mode beside the reference layout: tracks on a real map from VersaTiles
     vector tiles (OpenStreetMap data, ODbL).
   - Vector-tile decoding uses well-known packages; check each one's licence.
5. **Credits.**
   - A compact ⓘ CREDITS control on both views that lists every source in use with its
     licence.
   - `NOTICE` entries for each source.
6. **Tiles switch.** `VITE_HEIMDALL_TILES` (on by default):
   - With it off, 1b behaves exactly like 1a, and a test checks that no tile host appears
     in the built bundle.
   - The README lists the CSP hosts.
   - The residual-risk register gets an entry for the tile requests, which reveal the
     area being viewed.
7. **Offline.** With no network, it falls back to the 1a globe with "Map tiles need a
   connection".

**Tasks:**
1. Tile loader: the host allow-list, HTTPS only, no cookies, a cache, a budget, cancel on
   view change. Tests on recorded fixtures.
2. Terrain mesh and the close-up.
3. The TOPOGRAPHIC and SATELLITE styles.
4. The 2D geographic map.
5. Credits, `NOTICE`, the residual-risk entry, the README.
6. The switch and its bundle test.
7. Verify as for 1a.

## Owner decisions after the first build (22 Nov 2026)

- **Real 3D.** The globe tilts (pitch 0–75°, less allowed far out) and turns (heading),
  with a compass reset. The regional presets open tilted. Airborne tracks stand on stems
  to a ground shadow, and the horizon, sky and atmosphere show.
- **Close-in terrain presets.** With tiles on, SNOWDONIA and SOUTHERN UPLANDS are added
  so the relief can be seen.
- **One build.** 1a and 1b are combined in one branch, `heimdall_3dmaps`, and reach
  `dev` as a single pull request. `heimdall_3d` and `heimdall_maps` stay local.
- **Security audit.** A full security audit of this core build runs before the pull
  request.

## After the security audit (22 Nov 2026)

`VITE_HEIMDALL_TILES` above is superseded by `VITE_HEIMDALL_TILE_SOURCE`:

| Value | Behaviour |
|---|---|
| `none` | The default and the signed artefact: no tile code and no egress, so THREAT_MODEL "no egress" and NFR-PRV-1 hold. |
| `mirror` | Tiles only from `VITE_HEIMDALL_TILE_MIRROR`, an internal HTTPS mirror. |
| `public` | Demo only: the three public hosts. Tile loading pauses after any camera move driven by the tracks. |

Any other value fails the build.

Untrusted tiles are decoded in a same-origin worker, with byte, dimension, feature and
time limits.

The audit found no Critical or High issues. Its Medium findings are fixed, and the
residual-risk entry is rewritten. The dev signing key is **not** added to
`deploy/trust/allowed_signers`, because that file is also the release-tag signer list;
commits are verified through GitHub.
