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

**As built (24 Oct 2026).**
- **Where the logic lives:**
  - `bifrost-rollout` gains `runs` (`bifrost/rollout/src/runs.rs`): the ring planner, health gate and rollback controller as a clock-free state machine. `Rollouts` holds releases, channels and runs; the server steps it with `tick(now, reports)`. No I/O, no clock, so the tests drive it with fake time and fake reports.
  - `bifrost-fleet-server` gains `releases.rs` (the sealed store, `rings.json`, the JSON) and the routes in `lib.rs`. `server.rs` steps active runs every second on an `orchestrator` thread.
- **Model:**
  - **Release:** a version and, for each of the six ring services, an artefact version (the artefact id is the service name). It must cover exactly the ring services and is immutable (a second create is CONFLICT). By default every service ships the release's own version; `artefacts` may mix versions.
  - **Channel:** current release and previous one. `channels.json` only seeds a channel the orchestrator does not know yet; after that the sealed state decides. The fleet's channel desire (FleetStatus drift) follows the current release.
  - **Rings:** `<state>/rings.json` (shipped as `deploy/compose/fleet-rings.json`, copied by run-local's `state`), default canary forge-worker → ring-2 midgard-replay + heimdall-feed + heimdall-source → ring-3 mimir-server → last http-gateway, soak 60 s, readiness timeout 180 s, report window 30 s. Without the file the same defaults apply. A run copies the rings it starts with.
  - **RolloutRun:** `run-<n>`, kind (promote, or a manual rollback with no run in progress), channel, from, release, rings with phase (pending, waiting, soaking, passed, failed, rolled-back) and times (started, deadline, soak since, ended), state (running, completed, rolled-back), who started and ended it, and the reason. The last 200 finished runs are kept.
- **Gate:**
  - **Desired:** a service whose ring has started is desired at the target's artefact version; every other service at the current release's.
  - **Participants:** installations that sent a per-service check-in on the channel within the report window.
  - **Ready:** at least one participant, and every one reports every ring service at the target version and healthy. The ring then soaks; after `soakSecs` of readiness it passes and the next ring starts. The last ring passing completes the run, and the target becomes current.
  - **Fails:** not ready by the deadline (the reason lists what is not ready, e.g. "ring canary: not ready within 180 s (forge-worker on mac-suite unhealthy)"). Also fails if a service that reported ready on the target then reports unhealthy or another version.
  - **Rollback:** a failure rolls the run back. The current release is unchanged, so every service the run changed is desired back at once. `endedBy` is `bifrost-orchestrator`.
  - **Silence is not failure:** a ring that loses readiness only because a participant went quiet stops soaking and waits again, with a fresh readiness window. After a restart this is what happens: the run and its rings are restored, the reports are not, so a soaking ring soaks again from the next check-ins.
  - **Manual rollback:** `Rollback` with a reason (stored as "manual: …"). With a run in progress it rolls the run back. With none, it moves the channel to `to` or to its previous release, recorded as a run of kind rollback.
  - **One run per channel:** a second Promote is CONFLICT. So is promoting the current release.
- **Routes** (all JSON, through the same authorise-and-audit spine as the registry; the spine is renamed `json_call`):
  - `CreateRelease` `{"version", "artefacts"?}` (release-manager). Every artefact must be held and verified, and passes the whole gate again (`Recheck::Full`). Otherwise it is refused and audited ("CreateRelease refused: artefacts not verified"): REJECTED if any is unverifiable or was rejected (naming the reason, e.g. digest-mismatch), NOT_FOUND if they are only missing. A wrong shape is MALFORMED.
  - `ListReleases` `{}` (operator, release-manager): releases newest first, channels with current, previous, active run, target and per-service desired versions, and the rings.
  - `Promote` `{"channel", "release"}` (release-manager): refused if any of the release's artefacts no longer verifies. Answers the run.
  - `Rollback` `{"channel", "reason", "to"?}` (release-manager): answers the run.
  - `ListRollouts` `{"channel"?, "limit"?}` and `GetRollout` `{"id"}` (operator, release-manager).
  - Every orchestrator event (run started, ring started, ready, passed, failed, completed, rolled back) is recorded in the signed audit ledger, as the caller for Promote and Rollback, and as `bifrost-orchestrator` for the timer's own steps.
- **CheckIn compatibility: a new route, `CheckInServices`, beside the old one.**
  - **Why a new route:** the old `CheckIn` body is the binary `AgentReport` (length-prefixed fields). A JSON body cannot be told apart from it reliably (a `{` is also a valid first length byte), and changing the codec would break `bifrost-agent`'s simple mode, the compose smoke and `verify-bifrost.sh`. So `CheckIn` is unchanged.
  - **`CheckInServices`:** JSON, so the agent and a future bridge share it. Request `{"installId", "channel", "services": {name: {"version", "healthy"}}}`; answer `{"installId", "channel", "release", "run", "target", "desired": {name: version}}`.
  - **Policy:** it is authorised as the same action, `CheckIn`, so agents need no new rule. An agent may report only for its own installation (DENIED and audited otherwise).
  - **Fleet view:** it also records an ordinary fleet check-in, with the services' common version (or "mixed") and all-healthy, so FleetStatus keeps working.
- **Persistence: one sealed snapshot, not a journal.**
  - **What:** `<state>/releases.sealed` holds the whole state as JSON, sealed with the service's data sealer (AES-256-GCM under ML-KEM-1024, as `DurableFleet` and the registry), under a fixed AAD.
  - **How:** every change is sealed and written first (temp file 0600, fsync, rename, directory fsync) and takes effect in memory only after. A crash leaves the old or the new snapshot. One that does not unseal or decode stops the start.
  - **Why a snapshot:** the state is small, and a snapshot needs one codec instead of one op per transition.
  - **Known limit:** like a truncated journal, an older snapshot sealed by the same service would still open. The seal proves origin and integrity, not freshness. Needs write access to the state directory.
  - The server's `serving` log line now has `releases` and `active_rollouts`.
- **Client** (`client.rs`): `create_release`, `list_releases`, `promote`, `rollback`, `list_rollouts`, `get_rollout`, and `check_in_services`, which returns `DesiredState`, for the B2 agent and the B3 bridge.
- **CLI:** `bifrost-release release create|list|promote|rollback|rollouts` (`--service` defaults to release-manager), and `run-local.sh release …` runs it as release-manager against the dev instance.
- **Policy** (`deploy/compose/fleet-policy.json`): release-manager gets `CreateRelease`, `ListReleases`, `Promote`, `Rollback`, `ListRollouts` and `GetRollout`. fleet-operator gets `ListReleases`, `ListRollouts` and `GetRollout`. Agents keep `CheckIn` and `DownloadArtefact` only. run-local's `state` moves a policy without `CreateRelease` to `policy.json.pre-b2t2` (date-suffixed if one exists) and copies `rings.json` when absent. The private-enclosure move also takes `releases.sealed`.
- **Tests (TDD):**
  - `bifrost/rollout/tests/runs.rs`, 12 tests, fake clock and fake reports:
    - the default rings;
    - a release covering exactly the ring services, and immutable;
    - all four rings passing in order after their soak, with the channel moving;
    - soak measured from readiness (59 s does not pass, 60 s does);
    - the readiness timeout failing ring 2, with every changed service, the passed canary included, desired back;
    - no reporting installation timing out;
    - unhealthy after ready, and another version after ready, failing at once;
    - a quiet agent pausing the soak without failing it;
    - a manual rollback mid-run;
    - a manual rollback with no run, and then forward again;
    - one run per channel and the promote refusals;
    - restart-resume mid-ring.
  - `releases.rs` unit tests (2):
    - a run mid-ring survives a reopen exactly, and a flipped byte stops the open;
    - `rings.json` is read strictly, and the shipped file equals the defaults.
  - `bifrost/fleet-server/tests/releases.rs`, end to end with real processes and the shipped policy, a fast rings file (1 s soak, 4 s timeout), and six artefacts at 0.1.0 and 0.2.0:
    - **Policy refusals:** the operator's CreateRelease, Promote and Rollback; the agent's ListReleases, ListRollouts and GetRollout; the release manager's CheckInServices.
    - **CreateRelease refusals:** only partly registered (NOT_FOUND, naming what is missing); a rejected tampered artefact (REJECTED, digest-mismatch); a missing service (MALFORMED); a duplicate (CONFLICT).
    - **CheckInServices:** an agent checking in for another installation is DENIED.
    - **A full rollout,** with a server kill and restart while ring 2 soaks: the run resumes, completes, and FleetStatus converges on 0.2.0.
    - **A canary that never becomes healthy:** rolled back after 4 s with the reason, everything desired back on 0.2.0.
    - **Manual rollbacks:** one mid-run, then one of the channel with no run in progress.
    - **The rest:** a concurrent Promote is CONFLICT; ListRollouts is newest first; an unknown run is NOT_FOUND; the old binary `CheckIn` still succeeds.
  - **Results:** `cargo test -p bifrost-rollout` 16 tests and `-p bifrost-fleet-server` 18 unit and 3 integration tests, all passing. clippy `-D warnings` and `fmt --check` are clean for both crates. `verify-bifrost.sh`: 5/5.
  - **A failure that is not from this task:** `bifrost-fleet`'s `agents_pqmtls` test fails ("agent handshake: Transport(Io(failed to fill whole buffer))"). It fails the same way on a clean checkout of 6d4ae65, before this task. That crate is untouched.
- **Live** (127.0.0.1:7750, state `~/Desktop/Midgard/bifrost-local`):
  1. `run-local.sh setup --real-enclosure` rebuilt the binaries. The enclosure was not touched (all identities exist). The old policy was moved to `policy.json.pre-b2t2`, and `rings.json` was added.
  2. `stop` and `start`: `readyz` 200; the serving line shows `releases 0`.
  3. `run-local.sh release create --version 0.1.0` and `--version 0.1.1` both created releases from the six verified artefacts each.
     - Creating 0.1.1 again: CONFLICT.
     - `0.1.0-tampered`: REJECTED ("forge-worker@0.1.0-tampered was rejected (digest-mismatch); heimdall-feed@0.1.0-tampered is not in the registry; …").
  4. `release list`:
     ```
     channel stable current=0.1.0 previous=-
       desired: forge-worker=0.1.0 heimdall-feed=0.1.0 heimdall-source=0.1.0 http-gateway=0.1.0 midgard-replay=0.1.0 mimir-server=0.1.0
     release 0.1.1 by release-manager: forge-worker@0.1.1 heimdall-feed@0.1.1 heimdall-source@0.1.1 http-gateway@0.1.1 midgard-replay@0.1.1 mimir-server@0.1.1
     release 0.1.0 by release-manager: forge-worker@0.1.0 heimdall-feed@0.1.0 heimdall-source@0.1.0 http-gateway@0.1.0 midgard-replay@0.1.0 mimir-server@0.1.0
     rings: canary[forge-worker] -> ring-2[midgard-replay+heimdall-feed+heimdall-source] -> ring-3[mimir-server] -> last[http-gateway]
     ```
  5. **Policy, live:** fleet-operator lists releases but its Promote is denied. fleet-agent-1's ListReleases is denied.
  6. **A promote, rolled back by hand** (no agent exists until Task 3):
     - `release promote --channel stable --release 0.1.1` started run-1 with the canary waiting (deadline 180 s).
     - A second promote was refused ("run-1 is already rolling stable out to 0.1.1; one run per channel at a time").
     - `release rollback --reason "B2 Task 2 live check: no suite agent until Task 3"` rolled run-1 back after 3 s. The channel stayed on 0.1.0; every desired version is 0.1.0.
  7. **After another stop and start:** the serving line shows `releases 2`, `release rollouts` still lists run-1 as rolled back with its reason, and `status` still answers (converged on 0.1.0).

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
