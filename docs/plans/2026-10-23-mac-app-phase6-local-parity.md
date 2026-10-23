# Mac app phase 6: everything the local build shows, offline — implementation plan

**Goal:** the installed app shows everything the local (live) Heimdall build shows: cameras with their images and clips, camera analysis, rain radar, road traffic flow and close-up globe imagery, all from files in the app (no internet).

**Architecture:** media and tiles are captured once on the dev Mac into the recording folder (`~/Library/Application Support/Midgard-dev/recordings/hour-1/`), bundled by `build-app.sh` into `Contents/Resources/recording/`, and served by the app's loopback web server. Observations and the console point at app-relative URLs (`/media/…`, `/radar/…`, `/traffic/…`, `/tiles/…`), so the WKWebView loads them from the app origin. Camera analysis is precomputed with the real analytics code and model over the stored clips and replayed as `land-camera-ai` observations; the gateway's camera routes are answered from those results.

**Tech Stack:** Python 3 (recorder, capture scripts), Rust (midgard-replay, gateway), Swift (WebServer), TypeScript/React (Heimdall console offline paths), bash (build-app.sh).

User decisions (23 Oct): everything from the local build goes in; camera analysis precomputed. Storing TomTom, RainViewer and EOX data may not be allowed by those providers' terms for distribution outside the company: credit every source on the install sheet and in the app; distribution is the user's call.

Branch: `md_mac-installer` only. Commits signed. Storage: nothing new in `~/Desktop/Midgard` (suite folder ≤ 3.5 GB); captures under the recording folder or `~/Library/Caches/midgard-build`. Secrets (TomTom key, etc.) never printed or committed.

### Task 1: Cameras and their media

- `tools/mac-app/record.py`: keep camera batches (`land-camera`) when packing; add `capture-media` (or a pack step) that downloads, for each camera in the recording, its latest still (`image_url`) and, for TfL, its clip (`video_url`) into `<recording>/media/`, and rewrites those attributes in the packed batches to `/media/<file>`. Re-pack hour-1 from its raw/ (still on disk).
- App web server (`apps/mac/Sources/MidgardShellCore/WebServer.swift`): serve `<resources>/recording/media/` at `/media/` (same rules as the consoles: GET/HEAD, no traversal, content types for jpg/png/mp4).
- `build-app.sh`: bundles media with the recording (already copies the recording; check size and the raw/ exclusion).
- Check: app shows camera icons in London, NZ, BC; clicking a TfL camera shows its still and plays its clip.

### Task 2: Precomputed camera analysis

- Run the real analytics (`heimdall-poc/analytics/analytics.py`, YOLOX-s, its venv) over the stored TfL clips; save the `land-camera-ai` observations (and any annotated frames the console uses) into the recording as a feed played at the start of each loop.
- The app's gateway camera routes (`POST …/cameras/{id}/analyse`, `/watch`, `GET …/trend`) answered from the stored results by a small local responder (e.g. in midgard-replay behind `--analytics-addr`), so Analyse works instantly.

### Task 3: Rain radar

- Capture RainViewer frames (the manifest and the tiles the console's radar overlay uses over the UK, at its zooms) into `<recording>/radar/`; the console's offline build reads a local manifest from `/radar/` and replays the frames on a loop with times shifted to now. RADAR chip shown in the offline build.

### Task 4: Road traffic flow

- Capture TomTom flow tiles (the console's traffic overlay: UK, its zooms) into `<recording>/traffic/` using the key from the dev setup (never printed or bundled); offline build reads them from `/traffic/`. TRAFFIC chip shown.

### Task 5: Close-up globe imagery

- Pre-fetch the terrain (AWS terrarium) and imagery (EOX s2cloudless) tiles `detailPatch.ts` reads, for the UK and Ireland at the zooms it uses, into `<recording>/tiles/`; offline build reads them from `/tiles/` instead of keeping the bundled map. Report size; cap at ~300 MB.

### Task 6: Build, credits, tests

- Install sheet and Heimdall credits list every source. Rebuild app and DMG at a clean HEAD; fresh-install test: cameras (still + clip + Analyse), radar, traffic, close-up imagery, all layers; no network requests leave the Mac (offline content rules). Record results here.
