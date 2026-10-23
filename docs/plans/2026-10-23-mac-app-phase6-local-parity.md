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

### Task 2: Camera analysis, live on the Mac

User decision (23 Oct, revised): analysis really runs in the app, not precomputed.

- Bundle the analytics service (`heimdall-poc/analytics/analytics.py`, brought into git under `heimdall/analytics/` without its venv, logs or state) and run it under the app's supervisor as one more service (`analytics`), with the gateway started with `--analytics-addr` and `--analytics-token-file` as in the live setup, so Analyse, Watch and Trend work as they do locally.
- Runtime, per architecture (arm64 and x86_64, chosen like the model server): a pinned, relocatable CPython (python-build-standalone `install_only`, SHA-256 in a lock file like `llama.lock`), with pinned onnxruntime and numpy wheels installed into it at build time; the YOLOX-s ONNX model (`models/yolox_s.onnx`, Apache-2.0, SHA-256 in MODEL.md) in Resources with its licence. CoreML execution provider on Apple Silicon, CPU on Intel (as analytics.py already falls back).
- Clip decoding without Homebrew: replace the ffmpeg/ffprobe calls with a small bundled Swift helper (AVFoundation: probe size, decode ~4 fps to raw BGR frames resized for the letterbox) with the same output analytics.py reads, so no ffmpeg build (and its licence) ships. Keep ffmpeg as the path when the helper is absent (the dev setup).
- Clips come from Task 1's stored media (local paths or `/media/…` on the app's web server), never the internet.
- Everything Mach-O signed one by one before the bundle (as the llama helpers); verify under Rosetta that the x86_64 runtime analyses a clip.

### Task 3: Rain radar

- Capture RainViewer frames (the manifest and the tiles the console's radar overlay uses over the UK, at its zooms) into `<recording>/radar/`; the console's offline build reads a local manifest from `/radar/` and replays the frames on a loop with times shifted to now. RADAR chip shown in the offline build.

### Task 4: Road traffic flow

- Capture TomTom flow tiles (the console's traffic overlay: UK, its zooms) into `<recording>/traffic/` using the key from the dev setup (never printed or bundled); offline build reads them from `/traffic/`. TRAFFIC chip shown.

### Task 5: Close-up globe imagery

- Pre-fetch the terrain (AWS terrarium) and imagery (EOX s2cloudless) tiles `detailPatch.ts` reads, for the UK and Ireland at the zooms it uses, into `<recording>/tiles/`; offline build reads them from `/tiles/` instead of keeping the bundled map. Report size; cap at ~300 MB.

### Task 6: Build, credits, tests

- Install sheet and Heimdall credits list every source. Rebuild app and DMG at a clean HEAD; fresh-install test: cameras (still + clip + Analyse), radar, traffic, close-up imagery, all layers; no network requests leave the Mac (offline content rules). Record results here.

#### Task 6 results (23 Oct)

Done: capture-radar.py keeps one frame per time and refuses off-host redirects; capture-traffic.py refuses off-host redirects (the URL holds the key) and counts an empty 200 or a 204 as "no roads"; build-consoles.sh fails on any http(s) URL not on its reviewed allowlist (XML namespaces, React's error-decoder text, a licence comment, the loopback defaults, and the RainViewer, TomTom, EOX, CC BY 4.0, Terrain Tiles and Northern Powergrid attribution/licence links). Credits: `tools/mac-app/CREDITS.txt` (every recorded source and the bundled software), bundled as `Resources/licenses/CREDITS.txt` and `/home/credits.txt` (the home page's "Data and credits" panel, plus a short footer); a compact credits block on the install sheet (still one A4 page); Heimdall's camera card credits TfL, DriveBC, NZTA and LTA.

Fresh install from Midgard-1.0.dmg (app 1ee22f1), partial: the build was then found to have a console bug (the LAYERS switch list misses Airspace, Land, Maritime and Space after a fresh start; being fixed in `sdk/ts/src/products/heimdall-http.ts`), so it must be rebuilt and the test repeated. Seen before stopping: all services ready in 10.1 s, analytics 1.6 s after; within 2 min every layer had entities in the gateway picture (air 2448, airspace 10040, events 8772, infrastructure 14574, land 3281, sea 1825, space 418, utilities 2297) and 2106 land cameras, all with /media/ URLs (TfL 811 stills and clips, DriveBC 1040, NZTA 240, LTA 8); a TfL still (image/jpeg) and clip (video/mp4, range 206) loaded; Analyse returned detections in 1.4 s (YOLOX-s, CoreML), a land-camera-ai entity appeared and Trend returned the point; /radar/, /traffic/, /tiles/ manifests and a tile of each returned 200; in headless Chrome, 3D over London with RADAR, TRAFFIC and the satellite close-up and their credits, the TfL camera card (still, clip playing, "POWERED BY TFL OPEN DATA"), a prepared Mimir question answered in 0.26 s with no /heimdall/ask request; 444 page requests, none off 127.0.0.1; no app process (nor the app's WebKit networking process) had a non-loopback TCP connection. Not finished: the live Mimir question (the app was quit by an outside quit request during it, so the gateway returned 502), and the TERM quit check (that quit stopped cleanly: final sweep found nothing).

#### Licence notes (for the distribution decision)

| Source | Licence / terms | Storing and redistributing |
|---|---|---|
| RainViewer (rain radar) | RainViewer API terms (rainviewer.com/api.html) | Free for personal, educational and small-scale community use only; attribution ("Weather data by RainViewer", link) mandatory. Commercial or wider distribution of stored frames is not covered. |
| TomTom (traffic flow) | TomTom developer terms (evaluation/free tier) | Evaluation use; clause 11.4 forbids storing Results beyond client caching within their cache headers; 17.3 attribution from the Copyright API. A stored snapshot in a distributed app goes beyond the terms: needs TomTom's agreement. |
| EOX s2cloudless 2016 (satellite imagery) | CC BY 4.0 (the 2016 layer only; later years are CC BY-NC-SA) | Storing and redistributing allowed with the attribution "EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH (Contains modified Copernicus Sentinel data 2016)"; EOX asks offline/bulk users to contact them. |
| Terrain Tiles (Mapzen, AWS Open Data) | Per-source attribution list (tilezen/joerd attribution.md) | Reuse allowed with the attribution list: Environment Agency (OGL), EU-DEM (Copernicus), SRTM and GMTED2010 (USGS), ETOPO1 (NOAA), and others outside the captured area. |
| TfL JamCams | TfL open data terms ("Powered by TfL Open Data") | Reuse allowed with attribution, subject to TfL's terms. |
| NZTA cameras | NZTA traffic data, published for reuse with attribution | Reuse with attribution. |
| DriveBC cameras | Open Government Licence – British Columbia | Reuse with attribution. |
| LTA cameras (data.gov.sg) | Singapore Open Data Licence | Reuse with attribution. |
| Other feeds | See CREDITS.txt (OGL, CC BY 4.0, ODbL, public domain; Cloudflare Radar CC BY-NC 4.0 is non-commercial) | Attribution as listed; note the non-commercial ones. |
| Granite 3.3 2B, YOLOX-s, llama.cpp | Apache-2.0, Apache-2.0, MIT | Redistributable with licence texts (Granite's Apache-2.0 text is not shipped separately; CREDITS.txt points to the Apache text in YOLOX-LICENSE.txt). |
