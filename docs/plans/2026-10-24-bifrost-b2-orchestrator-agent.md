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

**As built (24 Oct 2026).**
- **I-5, actions** (commit 133d5c6). Each read route now has its own policy action:
  - `ListArtefacts` needs `ReadArtefacts`;
  - `GetArtefact` needs `DownloadArtefact`;
  - `VerifyArtefact` needs `VerifyArtefact`.
- **Policy** (`deploy/compose/fleet-policy.json`):
  - `fleet-agent`: `DownloadArtefact` only. It no longer lists or re-verifies.
  - `fleet-operator` and `release-manager`: `ReadArtefacts` and `VerifyArtefact`, and no download.
  - A runtime verification failure is now audited under the action that found it: "DownloadArtefact verify failed: …" or "VerifyArtefact verify failed: …", where it used to be "ReadArtefacts …".
- **run-local.sh `state`** (run by both setups): a policy.json without `DownloadArtefact` is moved to `policy.json.pre-b2` and replaced with the compose file. An existing `.pre-b2` is not overwritten; the new copy gets a date suffix. bifrost-server reads its policy only at start, so the script says to restart.
- **I-5, read cost** (`registry.rs`, "Reads" in the module docs):
  - Each record keeps the SHA-256 of the exact sealed bytes that the full gate last passed. That is set at the commit, at the open, and by any later full check. A failure clears it.
  - **`Recheck::Seal`** is a download's first slice of each part. The sealed file is read and unsealed every time (the AEAD check). If its digest is the verified one, that is enough. Any other bytes go through the whole gate again, including a different but valid seal of the same artefact, and pass only if the gate passes.
  - **`Recheck::Cached`** is a download's later slices. They come from a verified copy of exactly those sealed bytes. The copies are held in an LRU keyed by id@version and that digest, with 4 entries and 256 MiB between them (twice the largest artefact, so one always fits). A copy over the byte bound is not kept. On a miss, the slice is served as `Seal`.
  - **`Recheck::Full`** is `VerifyArtefact`, which always runs the whole gate.
  - Nothing is served without a seal check on this read, or on the read whose copy it is. An artefact marked unverifiable is not served from the cache either.
  - **Lock:** the file is read, unsealed and checked outside the registry lock. `begin_fetch` (under the lock) produces a `ReadCheck`, `run` happens unlocked, and `finish_fetch` (under the lock) records the result. If the record is marked, withdrawn or replaced in between, the read is discarded.
  - `full_verifications()` counts gate runs, for the tests.
- **Client:** the method names are unchanged. `client.rs` documents which action each call needs, and a missing one comes back as DENIED naming the action.
- **Tests (TDD):**
  - Unit tests (16, all passing):
    - after a commit or a restart, a download does no full verification; `Full` always does one;
    - a resealed (valid, different) file is verified in full once, then not again;
    - a flipped byte is refused by the seal check while a verified copy is cached, and the artefact is never served again;
    - a later slice of an artefact pushed out of the LRU re-checks the seal, not the gate;
    - the LRU is bounded by entries and by bytes, its eviction order is right, it serves only a copy with the matching digest, and it does not keep a copy that is too large.
  - The end-to-end test (`tests/registry.rs`) now also checks the split with an agent identity:
    - an operator's download is DENIED (`DownloadArtefact`);
    - an agent's list (`ReadArtefacts`) and verify (`VerifyArtefact`) are DENIED;
    - the agent downloads, twice, and after the restart with `download_verified`;
    - after a copy at rest is spoiled and restored, the agent's download is still refused.
  - `cargo test -p bifrost-fleet-server`: 16 unit and 2 integration tests, all passing. clippy `-D warnings` and `fmt --check` are clean for the crate. `verify-bifrost.sh`: 5/5.
- **M-9, the reproducible-build digest: not enforced, because `bifrost-artefact` does not model one.**
  - **What the crate has:**
    - `ArtefactManifest` holds `id`, `version`, `component`, `payload_sha256` and `sbom_sha256`.
    - `verify` checks the payload digest, the SBOM digest, the ML-DSA-65 signature in the `BIFROST_ARTEFACT` domain, and the SBOM's structure.
    - The only reproducibility in the crate is the SBOM's own: it is sorted and timestamp-free.
  - **Why nothing is compared:** design §2 step 2 says the registry checks a reproducible-build digest, but nothing produces a second one.
    - release.sh builds each commit once, on this Mac, and the release key signs that build's digest.
    - A "rebuild digest" from the same builder and key would equal `payload_sha256` by construction and prove nothing.
    - In B1, 0.1.0 and 0.1.1 had the same five digests only because the build cache was reused, not because two builds were made independently.
  - **What it would take:**
    1. **Reproducible builds first.** Build one commit twice from clean target dirs at different paths and compare. Fix what differs:
       - `--remap-path-prefix` for `$HOME`, the cargo registry and the worktree;
       - a pinned toolchain (`rust-toolchain.toml`);
       - `SOURCE_DATE_EPOCH`, if anything embeds a time;
       - whether `lipo` and ld64's ad-hoc code signature are deterministic for identical inputs.

       release.sh's fixed worktree path already removes one source of difference.
    2. **A rebuilder.** An independent second build, on another machine or at least a clean build under a separate identity. Its own ML-DSA-65 key would be anchored in `trust/rebuilders/` and must differ from the release signer. It would sign `{id, version, source commit, payload_sha256}` in a new domain such as `BIFROST_REBUILD`.
    3. **A separate attestation.** Carry it as an attestation uploaded with the artefact (one more field in `RegisterArtefact`, stored in the sealed record), not as a manifest field. Changing `ArtefactManifest::encode` would invalidate every existing signature, including the 12 live ones, and would need a versioned encoding.
    4. **The gate at commit.** Require at least one anchored rebuilder attestation whose `payload_sha256` equals the manifest's, from a signer that is not the release signer. Add refusals `rebuild-missing` and `rebuild-mismatch`.
    5. **Rollout.** Start in record-only mode, listing artefacts as "not reproduced", so existing releases stay valid. Enforce once release.sh produces attestations.
  - **A cheaper related check, not done:** the mg-sbom SBOM carries the binary's self-hash. Nothing yet checks that it equals `payload_sha256`. It is a consistency check, not a reproducibility check.
- **Live** (the dev instance on 127.0.0.1:7750, state `~/Desktop/Midgard/bifrost-local`):
  1. `run-local.sh setup --real-enclosure` rebuilt the binaries. All identities already existed, so the enclosure was not touched. It replaced `fleet-state/policy.json` and kept the old one as `policy.json.pre-b2`.
  2. `run-local.sh stop` stopped pid 97297 and removed its `fleet.lock`; the lock handling worked.
  3. `run-local.sh start` started pid 7686, with a new lock (`os=macos`) and `/readyz` 200. It logged `artefacts 12, rejected 1, release_signers bifrost-release-1`.
  4. `bifrost-release list` as `release-manager` (from mesh.conf's clients) showed the same listing as before the change: six services at 0.1.0 and six at 0.1.1, all verified, and `forge-worker 0.1.0-tampered` rejected (digest-mismatch).
  5. `fleet-operator` lists 12 verified. `fleet-agent-1`'s list is denied: "policy does not permit this identity to ReadArtefacts". `run-local.sh status` still answers.
  6. A download on the live server was not exercised, because there is no CLI for it yet. The end-to-end test covers it.

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
