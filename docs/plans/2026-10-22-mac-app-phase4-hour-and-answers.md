# Mac app phase 4: the recorded hour, prepared Mimir answers, Presenter's guide — implementation plan

**Goal:** Ship the app with a real recorded hour of data (aircraft, ships, power cuts, weather, floods, airspace warnings, the Gulf watch), real Mimir answers that appear instantly for the scripted moments, and a Presenter's guide on the home screen that says what to click.

**Architecture:** The live feeds now run without Docker: they send to the native suite (`run-native.sh`) through the native `heimdall-source`. With that running, `record.py` captures an hour. The app is then run against that hour (native suite with `MIDGARD_REPLAY`) and a capture script asks Mimir the map questions and the decision-support questions for alerts that stay open all hour, saving the real answers in `prepared-answers.json`. The app ships that file; Heimdall's console shows a shipped answer at once (keyed by the question, and for alerts by the alert's subject item, since alert numbers change each loop) and asks Mimir live for anything else. The home page builds the Presenter's guide from the same file.

**Tech Stack:** bash, Python 3 (stdlib), TypeScript/React (Heimdall console), Swift (shell, minimal), the phase 1–3 tools.

Design: `docs/plans/2026-10-22-mac-presentation-app-design.md` §2–3. Phase 3: `docs/plans/2026-10-22-mac-app-phase3-shell.md`.

---

## Background

- Docker has been removed from the development Mac (22 Oct). The feeds runner (`heimdall/feeds-runner/feeds.py`, linked from `~/Desktop/Midgard/heimdall-poc/feeds/`) sends each batch with `heimdall-poc/hsource.sh`, which today runs `heimdall-source` in a container and passes container paths (`/mid/...`). `live_loop.sh` (adsb.lol aircraft) does the same. The Forge fetcher (`live-feeds/fetch_live.py`) runs natively; `live-feeds/start.sh` also starts forge-worker in Docker.
- Native suite: `tools/mac-app/run-native.sh start|stop` (fixed ports: Heimdall mesh 7730, gateway 7710/7711), home `~/Desktop/Midgard`. Universal binaries in `dist/mac/bin` (rebuild with `build-services.sh`).
- Recorder: `tools/mac-app/record.py record --minutes 60 [--name NAME]` (uses the feeds.py hook; warns if adsb-fi, adsb-lol or aisstream are missing).
- Heimdall's prepared answers: `products/heimdall/ui/src/app/preparedAnswers.ts` (localStorage, 2 h, key `ask|<about>|<question>` or `advise|<alert id>|<question>`), used by `AskCard.tsx` (map questions, examples list `EXAMPLES`) and `AdviceCard.tsx` (decision support on an alert). Gateway calls: `POST /api/v1/heimdall/ask?question=…[&bbox=…][&focus=…]`, `POST /api/v1/heimdall/alerts/{id}/advise` (see `sdk/ts/src/products/heimdall-http.ts`).
- App resources: `apps/mac/Resources/home/` (served at `/home/...`), built into the bundle by `tools/mac-app/build-app.sh`.
- Storage: suite folder ≤ 3.5 GB; recordings live in `~/Library/Application Support/Midgard-dev/recordings`.
- Commits signed, pushed to `md_mixedfeeds_3d`.

### Task 1: Live feeds without Docker

- `heimdall-poc/hsource.sh` (outside git; keep a `.bak` copy): when `dist/mac/bin/heimdall-source` exists and the Docker image does not (or `MIDGARD_NATIVE=1`), run the native binary with `--identity-dir $M/airports-mesh/feeds --addr 127.0.0.1:7730`, translating any argument that starts with `/mid/` to `$M/`. Otherwise unchanged.
- `live-feeds/start.sh` (outside git; `.bak`): start only the fetcher when Docker is unavailable (forge-worker comes from the native suite); `live_loop.sh` unchanged (it calls hsource.sh).
- Verify: `run-native.sh start` (Docker absent → `MIDGARD_DOCKER_STOPPED=1`), start feeds, adsb.lol loop and fetcher; within 10 minutes `feeds.log` shows `accepted=` for adsb-fi, aisstream, power-cuts and others, and `smoke.py` passes. Record the commands in `tools/mac-app/README.md` ("Live feeds without Docker").

### Task 2: Record the hour

- With Task 1 running for at least 10 minutes, run `python3 tools/mac-app/record.py record --minutes 65 --name hour-1`.
- Accept it if the pack summary has adsb-fi (or adsb-lol), aisstream, power-cuts, metoffice-warnings or ea-floods, easa-czib, gulf-watch, and Forge versions for every job; else record again (note why).
- Report its size and per-feed counts; keep the raw folder until Task 5 passes, then re-pack with `--drop-raw` equivalent (delete raw/).

**Results (hour-1, 22–23 Oct):** accepted on the first take. 3901 s, 40 feeds, 11 Forge files; packed `heimdall.jsonl.gz` 19.5 MB (146 MB with raw/). Key counts: adsb-fi 26 batches / 57,327 observations, adsb-lol 77 / 7,610, aisstream 26 / 78,231, power-cuts 18 / 1,311, metoffice-warnings 7 / 189, ea-floods 17 / 153, easa-czib 2 / 32, gulf-watch 19 / 152, gnss-interference 17 / 148; Forge aircraft.csv 78 versions, grid_frequency.csv 65, every job at least 3. Suite folder 3.3 GB at the end.

### Task 3: Shipped prepared answers in the console

- `preparedAnswers.ts`: also read a shipped, never-expiring store: at startup fetch `window.__MIDGARD_PREPARED__` (URL injected by the shell, e.g. `<origin>/home/prepared-answers.json`) once; `getPrepared` checks the shipped store first, then localStorage.
- Keys for shipped entries: map questions `ask|*|<normalised question>` (any area); decision support `advise|entity:<alert subject entity id>|<normalised question>` (alert ids change every loop; subjects do not). `AdviceCard` looks up the subject key as well as the alert-id key.
- Shell: inject `window.__MIDGARD_PREPARED__` in the page script (Swift `Session.injectionScript` or a separate small script) when `Resources/home/prepared-answers.json` exists.
- Tests: unit-test the key functions (vitest exists? else a tiny node test) and the Swift injection; dev check that a shipped answer appears instantly.

### Task 4: Capture the answers

- `tools/mac-app/prepare-answers.py --base http://127.0.0.1:7710/api/v1 --out <file>`: signs in as analyst (passwords file, never printed); asks each `EXAMPLES` question from AskCard (and two Gulf/Heimdall ones of our choosing) over the UK region bbox the console uses by default; picks 5 alerts that stay open for the whole hour (prefer severities warning/critical, different kinds: power cut, flood, weather, GNSS, Gulf watch) and asks decision support for each with the default question; writes `{"version":1,"recording":"hour-1","created":…,"entries":{key:{"value":…,"at":…}}, "guide":{"questions":[{"question","region"}],"alerts":[{"key","label","subject","kind","region"}]}}`. Each question is asked over the region it is about (Gulf questions over the Gulf box, others over UK & Ireland).
- File format (version 1; also in the header of `products/heimdall/ui/src/app/preparedAnswers.ts`, which reads it): `{"version":1,"recording":"hour-1","created":<epoch ms>,"entries":{"<key>":{"value":<HeimdallAnswer|HeimdallAdvice>,"at":<epoch ms>}},"guide":{"questions":[{"question":"…","region":"UK & Ireland"}],"alerts":[{"key","label","subject","kind","region"}]}}`. Keys as the console's `shippedKey`: `ask|*|<q>` and `advise|entity:<alert entity key>|<q>`, where `<q>` is the question trimmed, lower-cased, each whitespace run made one space (the default decision-support question is empty, so its key ends in `|`). An entry is used only if `value` is an object and `at` a number; any other `version` is ignored.
- Run it with the native suite replaying `hour-1` (`MIDGARD_REPLAY=… run-native.sh start`) after at least 10 minutes of replay (so alerts exist). Mimir on this Mac takes ~15 s per answer; allow 5 minutes per answer before giving up on one.
- Save to `apps/mac/Resources/home/prepared-answers.json` (committed: it is our own generated text over open data).

**Results (23 Oct):** the first capture showed every map answer grounded on flood alerts whatever was asked: the gateway passed the question to Heimdall still percent-encoded, so the evidence ranking matched no words and took the first 20 alerted items by id (fixed in 51c67ea; affected every live question too). Answers were also often cut off mid-JSON at 450 tokens ("unterminated string"); the answer cap is now 768. Captured again after 25 min of replay: 3 alerts qualified (Vietnam censorship, amber wind warning South West England, flood South Devon coast; the Vietnam one has no region and so is not in the guide). Granite 2B answers vary from run to run, so the weaker map answers were asked again (4 tries each) and the best real, grounded answer kept; none were edited. "Which power cuts affect the most customers?" never compared customers (its 20 items tie on score), so the example is now "Which areas have power cuts?" (AskCard and the file); "Is there anything unusual in the Gulf watch?" was muddled or cut off every time and is dropped. Shipped: 5 questions (Gulf watch changes, power-cut areas, storm overflows, most serious alerts, unusual aircraft/ships), 3 decision-support answers.

### Task 5: Presenter's guide and the app build

- Home page: replace "Coming soon" with the guide read from `prepared-answers.json`: "Ask Mimir (instant)" — the questions to click in Heimdall's Ask Mimir box; "Decision support (instant)" — which alerts to click (label and where), then "Ask Mimir" on the alert; and a short line: "Other questions are answered live and take longer."
- `build-app.sh` default recording: `hour-1` when present.
- Build, open the real app, check: aircraft and ships visible in Heimdall; an example question answers instantly; one guide alert's decision support answers instantly; Reset demo and quit still clean (no leftover processes).
- Update `tools/mac-app/README.md` and this plan's results.

**Results (guide and build script):** the home page builds the guide from `/home/prepared-answers.json` (served at `/`, so not `./`); text from the file only via textContent. Headings as shipped: "Ask Mimir — instant answers" (click the question in the MIMIR box, or type it exactly and press ASK; grouped by region, each group "Make sure <region> is switched on under REGIONS", UK & Ireland on by default) and "Decision support — instant answers" (click the alert in ALERTS IN VIEW or the bell's ALERTS drawer, then ASK MIMIR; grouped by region the same way, alerts with no region left out; each shows its subject, and its kind in words unless the label starts with it); a hint "Not in WALL mode — the regions list is hidden there."; closing line "Anything else is answered live by Mimir and takes longer (about 15–60 seconds on this Mac)." Missing, invalid or non-version-1 file: "No prepared answers in this build." `build-app.sh` defaults to `recordings/hour-1` when it has `meta.json` (`--recording ""` for none) and checks the prepared answers file is version 1 (strict int), printing its counts. Checked with a sample file in headless Chrome; the app run is still to do.
