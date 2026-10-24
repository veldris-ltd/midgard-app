# Bifrost B2: the release orchestrator and the suite agent — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** releases go out for real. `bifrost-server` holds releases and channels and runs ring-by-ring rollouts with health gates and automatic rollback. `bifrost-agent` in suite mode installs the desired release's verified builds into the native suite on this Mac, restarts each service, and reports health.

**Architecture:** see `docs/plans/2026-10-24-bifrost-live-design.md` §1–§3.
- The orchestrator is a module in bifrost-server, built on `bifrost-rollout` (ring planner, health gate, rollback controller) and `bifrost-fleet` (desired vs actual, reconciliation), persisted in the server's sealed state.
- The agent extends `bifrost-agent` with a long-running `--suite` mode:
  - It checks in every 10 s, reporting per-service version and health, and gets its desired versions back.
  - It downloads with `client::download_verified`, installs into `releases/<version>/` slots with an `active` pointer, and restarts services through the native runner.
  - It keeps the previous slot and journals every swap.

**Tech Stack:** Rust (workspace crates, offline), bash (`run-native.sh` integration).

Rules: commits signed (`git commit -S`), **no Claude attribution** (organisation rule), branch `md_bifrost` only. Cargo goes in `~/Library/Caches/midgard-build/bifrost-target`. Never print secrets. mg-admin runs against `~/Desktop/Midgard/airports` only when the suite is stopped.

---

### Task 1: Registry carry-overs from B1

- **I-5, read cost:**
  - Split the read actions: `DownloadArtefact` for agents; `ReadArtefacts` (list) and `VerifyArtefact` for operators and release managers.
  - Cache the verification result per immutable sealed record, so a download re-checks only the AEAD, not the whole signature, each time.
  - Use a small LRU of verified copies keyed by id@version.
  - Update the policy files and run-local's setup.
- **M-9:** check the reproducible-build digest if `bifrost-artefact` supports it. If not, record why and what it would take.
- Tests for both.

### Task 2: Releases, channels and the orchestrator in bifrost-server

- **Model:**
  - **Release:** version and the set of artefact ids@version, every one of which must be verified in the registry.
  - **Channel:** name and current release.
  - **Ring:** ordered list of service names with a soak time and readiness timeout. The rings are canary forge-worker → ring 2 midgard-replay + heimdall-feed + heimdall-source → ring 3 mimir-server → last http-gateway.
  - **RolloutRun:** release, rings, state per ring, start/end, outcome and reason.
- **Routes,** authorised and audited, actions named in policy:
  - `CreateRelease` and `ListReleases` (release-manager, operator read).
  - `Promote` (release-manager), which starts a RolloutRun to a channel.
  - `Rollback` (release-manager, with a reason).
  - `ListRollouts` / `GetRollout`.
  - `CheckIn` extended so an installation reports per-service versions and health, and gets back per-service desired versions.
- **Orchestrator loop** (in-process timer):
  1. Raise the desired version ring by ring.
  2. A ring passes when every service in it reports the release version and ready for the soak time (default 60 s; configurable).
  3. A ring fails if not ready by the readiness timeout (default 180 s) or unhealthy after reporting.
  4. On failure, roll every changed service back to the previous release and record the reason.
  5. A manual Rollback does the same.
  6. Only one run per channel at a time.
  7. After a restart, a run in progress resumes from its persisted state.
- **Tests:** a pure state-machine test (fake time, fake reports) for the pass, fail and rollback paths, manual rollback and restart-resume, plus route tests.

### Task 3: The suite agent

- **Mode:** `bifrost-agent --suite --identity-dir … --addr … --install-id <id> --suite-root <dir> --runner <cmd>`.
- **Slots:** `<suite-root>/releases/<version>/<service>` with an `active/<service>` link to the current slot's binary. The first run adopts the existing `dist/mac/bin` binaries as the release given by `--initial-version`, after checking their digests against that release's artefacts in the registry. If they don't match, it refuses and explains.
- **Install loop, per service whose desired version differs:**
  1. `download_verified` into a staging folder and verify again.
  2. Move it into the slot and journal the intent (`swap.journal`).
  3. Point `active` at it.
  4. Restart the service through the runner (`run-native.sh restart <service>`; add that subcommand) and wait for readiness the way run-native does.
  5. Report.
- **Keep** the previous slot until the channel's next release fully passes.
- **Crash recovery:** on start, if `swap.journal` shows an unfinished swap, finish or revert it before anything else.
- **Control plane unreachable:** keep running and retry with backoff; never change anything while unreachable.
- **`run-native.sh`:**
  - Start each service from `active/<service>` when `MIDGARD_SUITE_ROOT` (or `active/`) exists, falling back to `dist/mac/bin`.
  - Add `restart <service>`.
  - Keep the existing behaviour unchanged when not managed.
- **Tests:**
  - slot install and swap, journal recovery (crash at each step), refusing an unverified download, and adoption digest mismatch;
  - an end-to-end test with fake services: a tiny test program that becomes ready or not on cue, started by a test runner, never the real suite.

### Task 4: End to end on this Mac

1. Agent adoption:
   1. `run-local.sh` gains `agent start|stop` (suite mode, install id `mac-suite`).
   2. Adopt the running `dist/mac/bin` as 0.1.0. Rebuild `dist/mac/bin` from the 0.1.0 commit first if the digests don't match, since release builds and `build-services.sh` builds may differ; investigate and make them match, or adopt by registering the dist binaries.
2. With the suite running (hour-1 replay), create release 0.2.0 from a newer commit (`release.sh`), then promote. Watch each ring pass, gateway last. Heimdall stays up meanwhile (poll its picture through the gateway, which will briefly restart). The fleet ends converged on 0.2.0.
3. Promote 0.1.1 (the bad build). The canary fails its readiness gate and everything rolls back to 0.2.0 automatically, with the reason recorded.
4. Do a manual rollback to 0.1.0 and then forward again.
5. Record results in this plan, with timings.
