# Bifrost B1: bifrost-server and the artefact registry — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** a running `bifrost-server`, the successor of `bifrost-fleet`, with a durable, encrypted artefact registry. The registry accepts only signed, SBOM-attested service builds and refuses tampered ones. A `release.sh` builds, signs and registers the native suite's services for a git commit.

**Architecture:**
- Extend `bifrost/fleet-server`, keeping its crate, mesh spine, policy and audit, into the control plane of `docs/plans/2026-10-24-bifrost-live-design.md` §1.
- Add mesh routes for the registry (upload, list, get, verify) backed by `bifrost-artefact`, with signatures from `mg-sign` (the enclosure's ML-DSA release key) and SBOMs from `mg-sbom`.
- Bring the native-run tools onto this branch so the suite can be built and run here.

**Tech Stack:** Rust (workspace crates, offline vendored), bash, the existing tools (mg-admin, mg-sign, mg-sbom, mg-register).

Design: `docs/plans/2026-10-24-bifrost-live-design.md`. Spec: `docs/dev/markdown/SAD_03_Bifrost.md`. Rules: commits signed (`git commit -S`), **no Claude co-author or attribution lines** (organisation rule), branch `md_bifrost` only, Cargo output in `~/Library/Caches/midgard-build/bifrost-target` (CARGO_TARGET_DIR), and cargo at `/opt/homebrew/opt/rustup/bin`.

---

### Task 1: Native tools on this branch

- **Native tools:** bring `tools/mac-app/build-services.sh`, `run-native.sh`, `smoke.py` and anything they need from `origin/md_mac-installer`. That includes the service-runtime macOS lock fixes ("service-runtime: reclaim a dead holder's directory lock on macOS", "take the macOS process start time from libc", "refuse foreign-OS locks"). Find them by message, since commit IDs changed in the history rewrite. Cherry-pick them, or copy if cherry-picks conflict, and say which.
- **Add `bifrost-fleet-server`:** add it to `build-services.sh`'s package list (bifrost-fleet, bifrost-agent).
- **Check:** `build-services.sh` builds, and `run-native.sh start` / `stop` work on this branch with the hour-1 replay (`MIDGARD_REPLAY=…/recordings/hour-1 MIDGARD_DOCKER_STOPPED=1`). Then stop it again.

**Result (24 Oct 2026, done).**
- **Cherry-picked cleanly:** the three lock fixes (c4b42b3, 6ed0dd6, 2596976) and the five tool commits for build-services, run-native and smoke (17660fb, 1e7b21b, 829aebf, 7ff9909, fb2729b).
- **Without its README change:** "run the replay with the native suite" (7ace501). The Mac app README is not on this branch.
- **`midgard-replay`, copied:** `tools/replay` was copied as it stands on md_mac-installer (f560909), not cherry-picked, because its 17 commits also touch the Mac app and console. It is brought because build-services builds it and the check uses the replay. run-native skips the replay cleanly when `MIDGARD_REPLAY` is unset.
- **bifrost-fleet-server:** added to build-services (f9dd7f6).
- **Rebase:** the branch was first cut from dev_mac, whose services lack run-native's options (`--heimdall-addr`, `--fabric-addr`, …). It was re-based onto md_mixedfeeds_3d (f13fa05). On that base, the services differ from md_mac-installer only by the gateway's `--token-ttl` option, with no state-format change.
- **Build:** `MIDGARD_BUILD_DIR=~/Library/Caches/midgard-build/bifrost-target bash tools/mac-app/build-services.sh` builds all 8 binaries as x86_64 + arm64 into `dist/mac/bin`, which is gitignored.
- **Run:** `run-native.sh start` with the hour-1 replay brought mimir, gateway, heimdall, forge and replay to ready. No errors in the logs.
- **Smoke:** `smoke.py` passed:
  - sign in;
  - 22 fabric object types;
  - Heimdall picture, 27,635 entities;
  - Mimir granite-3.3-2b-instruct-q4km;
  - Forge, 11 jobs.
- **Stop:** `stop` ended every service and removed the four locks. Afterwards only analytics (7740), bifrost-fleet (7750/9750) and llama-server (8080) were still listening.

### Task 2: The registry module in bifrost-server

- **Read first:** `bifrost/artefact` (manifest, SBOM, registry, verify gate; commit "admit only gate-verified artefacts") and `bifrost/fleet-server` (routes, authorise-and-audit spine, `CheckIn`/`FleetStatus`).
- **New mesh routes,** each authorised by policy and audited like the existing ones:
  - `RegisterArtefact`: a manifest, SBOM, signature and bytes, chunked if needed. Verified with the gate and stored encrypted under the state dir, in the same sealing style as `DurableFleet`.
  - `ListArtefacts`, `GetArtefact` (bytes plus manifest, re-verified on the way out) and `VerifyArtefact`.
  - Refusals (bad signature, SBOM mismatch, unknown signer, digest mismatch) are recorded with their reason and listed as Rejected.
- **Trust anchor:** the release-signing public key(s) the registry accepts are in the state dir (`trust/release-signers/`). An unknown key is refused.
- **Binary name:** add `bifrost-server` as a binary that serves the same as `bifrost-fleet`, plus the registry. Keep `bifrost-fleet` working as an alias for compatibility (`deploy/compose`, `verify-bifrost`).
- **Tests:** register good, bad signature, bad SBOM, unknown signer and tampered bytes; list; get re-verifies; restart durability; policy refusal for a role without RegisterArtefact.

**As built (24 Oct 2026).**
- **Binaries:** `bifrost-server` is new; `bifrost-fleet` is kept as an alias. Both are two-line wrappers around `bifrost_fleet_server::server::main`, so they serve exactly the same. `--service` defaults to the program's name. The ledger, journal and registry ids do not depend on that name, so an existing `bifrost-fleet` state directory opens under `bifrost-server` unchanged. `build-services.sh` now also copies `bifrost-server`.
- **Registry module:** `bifrost/fleet-server/src/registry.rs`, backed by the `bifrost-artefact` gate (`bifrost_artefact::verify`, the same check as "admit only gate-verified artefacts"). What is signed is the Bifrost `ArtefactManifest` (id, version, component, payload SHA-256, SBOM SHA-256), in the `BIFROST_ARTEFACT` domain, with ML-DSA-65.
- **Trust anchor:** `<state>/trust/release-signers/*.pub`, in `<alg> <key-id> <hex>` lines (what `mg-sign keygen --export-verifying-key` writes).
  - An upload names its signer by key id. A key id that is not anchored is refused as `unknown-signer`.
  - A non-ML-DSA-65 anchor stops the start with the reason, rather than making every upload fail.
  - With no directory, the server starts and refuses every upload. compose and verify-bifrost need no change.
  - The serving log line lists `release_signers`, `artefacts` and `rejected`.
- **Chunking:** the transport frame is 1 MiB (`pq-mtls` `MAX_FRAME`, which applies to sealed records too), so a ~6 MB universal binary does not fit in one call. An upload is three mesh methods over one PQ-mTLS session, each authorised and audited as the policy action `RegisterArtefact`:
  1. `RegisterArtefact` `{manifest{id,version,component,payloadSha256,sbomSha256}, sbom, signature(hex), signer, size}` returns `{uploadId, chunkBytes}`. An unknown signer is refused here, before any bytes are sent. An id@version that is already registered is a conflict.
  2. `PutArtefactChunk` `{uploadId, offset, data(base64)}`, in order, at most 512 KiB of bytes (about 683 KiB as base64), returns `{received}`.
  3. `CommitArtefact` `{uploadId}`: the gate runs on the whole artefact. It is admitted (audited "RegisterArtefact verified", then sealed and stored), or refused, recorded and audited "RegisterArtefact refused: <reason>".

  Uploads are held in memory, owned by the identity that began them, bounded (128 MiB each, 8 in flight, 256 MiB in all) and dropped after 15 minutes idle. Requests and replies are JSON, so the B3 bridge can pass them through.
- **Reads** (action `ReadArtefacts`):
  - `ListArtefacts` `{}` returns `{signers, artefacts[]}`. Each item has a status of `verified`, `unverifiable` (held, but it no longer verifies, e.g. its signer was removed; never served) or `rejected` with `reason` (`bad-signature`, `sbom-mismatch`, `sbom-invalid`, `unknown-signer`, `digest-mismatch`) and `detail`.
  - `GetArtefact` `{id, version, offset?, length?}` returns the manifest, signer, alg, signature, size and one slice of up to 512 KiB of bytes. The SBOM comes with the first slice, which is shortened by the SBOM's size to stay inside the frame. Offset 0 re-reads the sealed copy and re-verifies it; later slices come from the copy that check verified.
  - `VerifyArtefact` `{id, version}` re-verifies the copy at rest now.
  - New outcome codes: 4 not found, 5 rejected, 6 conflict.
- **At rest** (`<state>/registry/`):
  - `artefacts/<hex(id\0version)>.sealed` holds one file per verified artefact. Each is sealed with the service's data sealer (the `DurableFleet` sealer: AES-256-GCM under an ML-KEM-1024-wrapped key), its AAD bound to the id and version, and written atomically.
  - `rejected.journal` is a sealed, sequence-bound, append-only journal of refusals: metadata and reason only, never bytes.
  - On start, every artefact is unsealed and re-verified. A file that does not unseal fails the start closed.
- **Client:** `bifrost_fleet_server::client` (`register`, `list`, `download`, `verify`) is what the tests use. It is meant for Task 3's uploader and B2's agent.
- **Policy:** `deploy/compose/fleet-policy.json` adds:
  - `ReadArtefacts` for `fleet-agent`, `fleet-operator` and `release-manager`;
  - `RegisterArtefact` and `ReadFleet` for `release-manager`.

  `run-local.sh setup` issues a `release-manager` identity and makes `fleet-state/trust/release-signers/`. It replaces an older policy.json that lacks the registry actions, keeping the old one as `policy.json.pre-registry`. `start` runs `bifrost-server --service bifrost-fleet` with the existing identity. This was checked in a throwaway `BIFROST_HOME` on ports 17750/19750 (setup, start, demo, a read by release-manager, stop). The running `~/Desktop/Midgard/bifrost-local` instance was left alone.
- **Tests:**
  - Unit (`registry.rs`, `b64.rs`): admit and reopen, bytes not in the clear; each refusal reason recorded and surviving a reopen; a flipped byte at rest fails the open; a removed signer is listed but never served; upload order, ownership, size bounds, idle expiry and conflict; `.pub` loading and the ML-DSA-65-only rule; base64 RFC 4648 vectors and strictness.
  - End to end (`tests/registry.rs`, the real `bifrost-server` process):
    - a 3 MB build in 7 chunks is admitted;
    - bad signature, SBOM mismatch, unknown signer and tampered bytes are each refused with their reason, and a duplicate is a conflict;
    - the listing shows 1 verified and 4 rejected;
    - an operator, who lacks `RegisterArtefact`, is DENIED;
    - a download reassembles and verifies against the anchor, VerifyArtefact passes, and an unknown version is not found;
    - after a hard kill, the same listing and download come back;
    - nothing is stored in the clear, and a flipped byte stops the start.
  - The existing `serve.rs` still runs the `bifrost-fleet` alias.
- **Checks:**
  - `cargo test -p bifrost-fleet-server`: 8 unit, 2 integration, all passing.
  - clippy `-D warnings` and `fmt --check` are clean for the crate. Workspace-wide `fmt --check` has existing drift in `mimir/`, which was not touched.
  - `verify-bifrost` passes natively (`just` is not installed here, so the script ran directly with `bash`; no Docker needed): 5/5.
- **For Task 3:**
  - The registry verifies the Bifrost manifest signature, not `mg-sign`'s `.sig`/`.att` release envelope (`RELEASE_ARTEFACT` domain, ML-DSA-87 by default). release.sh should therefore make an ML-DSA-65 release keystore (`mg-sign keygen --alg ML-DSA-65 --export-verifying-key …`), install the `.pub` in `trust/release-signers/`, and sign each manifest through the custodian (`build_with_signer` with the keystore's signing root, as `mg-register` does), uploading with `client::register`.
  - `mg-sign`'s envelope can still be made and shipped as `attestation` artefacts, as `mg-register` does, if wanted.

**Review fixes (after b95056f).** Where these change a point above, they replace it.
- **I-1 SBOM size vs frame:**
  - The SBOM no longer rides in the first payload slice. `GetArtefact` takes `part`: `"payload"` (the default) or `"sbom"`, each sliced and base64-encoded the same way.
  - `RegisterArtefact` takes the SBOM as base64, and `MAX_SBOM_BYTES` is now 512 KiB (one chunk's budget).
  - Every message carrying bytes is at most 512 KiB as base64 (699,052 bytes), plus identifiers and hex, so it stays under 10 KB over that. JSON escaping cannot inflate it, because no free text travels with the bytes; signer ids are now limited to the identifier character set too. The budget is written down in `registry.rs`.
  - Test: a 400–512 KiB SBOM full of escaped quotes registers and downloads.
- **I-2 long names:**
  - Files are named `hex(sha256(id\0version)).sealed`: 64 characters, whatever the name. The AAD binds the sealed file to that hash, and the unsealed id and version must hash to it.
  - A commit is now stored first and audited second. If the audit fails, the stored artefact is withdrawn (file and record), so the ledger never says "verified" for something not held, and nothing held is unaudited.
  - Tests: 128-character id and version; withdraw.
- **I-3 JSON parser** (`services/runtime/src/json.rs`):
  - Nesting deeper than 64 levels is refused with "JSON nests deeper than 64 levels". 200,000 levels gives the error, not a stack overflow.
  - M-1: `\u` needs exactly four hex digits; surrogate pairs decode; a lone surrogate is refused.
  - Tests: service-runtime. The JSON users http-gateway, heimdall-feed-server, mimir-server, forge-worker, midgard-replay and gateway all pass (194 tests).
- **I-4 runtime verification failure:**
  - A `GetArtefact` or `VerifyArtefact` whose re-verification fails now audits "ReadArtefacts verify failed: <reason>" on `artefact:id@ver` and marks the record unverifiable.
  - From then on it is never served, from disk or from the cache, until a restart re-verifies it. The mark is in memory only; a restart re-checks everything.
  - Tests: unit test, and end to end (spoil the file while the server runs, restore it, still refused, listed as unverifiable).
- **I-6 memory:**
  - An upload's bytes live in the `SignedArtefact` they will become: reserved once with `try_reserve_exact` on the first chunk, then verified by reference and moved into the store, with no clones.
  - The admitted copy becomes the read cache.
- **M-3 uploads:** idle expiry also runs on chunk and take, and an identity may have at most 2 uploads in flight. Tested.
- **M-4 listing and refusals:**
  - `ListArtefacts` is paged. It takes `{cursor?, limit?}` (default 100, at most 500) and returns `next` (null at the end). The cursor is stable because refusals are append-only. `client::list` walks every page.
  - The refusal journal keeps at most 10,000 entries, and at most 10 unknown-signer refusals per identity per hour. Beyond either, the upload is still refused and audited, just not listed; the reply says so. Tested.
- **M-8:** `client::download_verified(session, id, version, anchors)` verifies against the caller's own anchors before returning; a signer the caller doesn't anchor is an error. Tested.
- **Left for B2:**
  - I-5: read cost. Each download's first slice per part unseals and re-verifies the whole artefact under the registry lock, and reads are one `ReadArtefacts` action. Consider splitting list from download, and holding the lock less.
  - M-9: the reproducible-build digest check from the design (§2.2) is not done yet.

### Task 3: release.sh

`tools/bifrost/release.sh <commit> --version X.Y.Z [--bad-build]`:
1. Build the suite services for the commit in a temporary worktree, with build-services' approach and universal binaries.
2. Write an SBOM per binary (`mg-sbom`) and sign each (`mg-sign`) with the release key. The key comes from a dedicated release keystore made once under `~/Desktop/Midgard/bifrost-local/release-key/` and never committed; its public half is installed into the server's `trust/release-signers/`.
3. Upload each to bifrost-server with `RegisterArtefact`, via `bifrost-agent` or a small `bifrost-cli` binary in the crate; choose and justify.
4. `--bad-build` builds a variant whose readiness never comes, for the demo rollback in B2. Use an environment or argument the service honours, or a wrapper. Choose the least invasive; it must not touch production code paths.
5. Print what was registered.

**As built (24 Oct 2026).**
- **Command:** `tools/bifrost/release.sh <commit> --version X.Y.Z [--bad-build] [--server HOST:PORT] [--identity DIR] [--service NAME] [--key DIR] [--trust-dir DIR] [--out DIR]`, and `release.sh --init-key` to make and install the key only.
  - Defaults come from `BIFROST_HOME` (`~/Desktop/Midgard/bifrost-local`), so they match `run-local.sh`: server `$BIFROST_MESH` or `127.0.0.1:7750`, identity `$BIFROST_HOME/fleet-clients` as `release-manager`, key `$BIFROST_HOME/release-key`, trust dir `$BIFROST_HOME/fleet-state/trust/release-signers`.
  - `--out` keeps the binaries, SBOMs and signed manifests. Otherwise they go in a temporary folder that is removed.
- **Order:**
  1. It builds its tools from this checkout: `bifrost-release`, `mg-sign` and `mg-sbom`, in `CARGO_TARGET_DIR` (default `bifrost-target`).
  2. If there is no key, it makes one (see below).
  3. **Preflight:** `bifrost-release preflight` checks that the server answers, that the identity may read the registry, and that the key's id is one of the registry's signers. This happens before anything is built. A key the server does not trust stops the release, with the instruction to install the `.pub` and restart.
  4. It checks that at least 5 GB is free.
- **Build:**
  - A `mkdir` lock allows one release at a time.
  - The commit is checked out with `git worktree add --detach` at a fixed path, `~/Library/Caches/midgard-build/bifrost-release/worktree`, and removed on exit, including on failure (trap). The path is fixed, so rebuilding a commit is incremental and the binaries do not depend on where they were built.
  - The build is build-services.sh's loop, in the script itself so it works for any commit with these packages: `cargo build --offline --locked --release --target {aarch64,x86_64}-apple-darwin` for http-gateway, heimdall-feed-server, mimir-server, forge-worker and midgard-replay, then `lipo` into `dist/mac/bin` inside the worktree.
  - Build output goes to `MIDGARD_BUILD_DIR` (default `~/Library/Caches/midgard-build/bifrost-release/target`), apart from the other builds.
  - Bifrost itself is not part of a release.
- **SBOM:** `mg-sbom --out … dist/mac/bin/<name>`, run in the worktree, so each SBOM comes from that commit's Cargo.lock and carries the binary's self-hash.
  - mg-sbom lists the whole workspace lockfile, not one binary's closure. Each SBOM therefore names the same components; only its subject and hash differ.
- **Release key:**
  - A dedicated ML-DSA-65 keystore: `mg-sign keygen --alg ML-DSA-65 --key-id bifrost-release-1 --keystore <key>/release.keystore --export-verifying-key <key>/release.pub`. The directory is mode 700 and lives outside the repository.
  - The `.pub` is copied to `<trust-dir>/bifrost-release-1.pub` if it differs. bifrost-server reads its signers only at start, so it must then be restarted, and the script says so.
  - `mg-sign`'s own `.sig`/`.att` envelope is not made. It is ML-DSA-87 in the `RELEASE_ARTEFACT` domain, which the registry does not check, so it would add a second key for nothing at this stage.
- **`bifrost-release`** (new bin in `bifrost/fleet-server/src/bin/bifrost-release.rs`; the server code is untouched). Its subcommands are `sign`, `upload`, `list [--version V] [--json]` and `preflight`.
  - **`sign`** needs only the keystore. It unseals the key inside the custodian (`attest::Keystore` → `KmsSigner`), refuses any algorithm but ML-DSA-65, and signs the manifest with `build_with_signer`, as mg-register does. It writes `<name>.bifrost.json` with `{manifest, signer, alg, signature}`.
  - **`upload`** sends that signed manifest with the payload and SBOM files as they are now, using `client::register` (RegisterArtefact, PutArtefactChunk, CommitArtefact). It does not re-check them; the registry's gate does.
  - **Why separate steps:** signing works offline, and a test can sign a build and change it afterwards.
  - **Why a new binary** rather than bifrost-agent: the agent is the on-host installation client, while this holds release-manager concerns (the release keystore). Keeping it apart means the agent never links a signer.
  - **Cargo.toml:** the crate's `attest` dependency adds the `custody` feature. It was already on through service-runtime, so `Cargo.lock` is unchanged.
- **Bad build** (`--bad-build`):
  - forge-worker, the canary, is replaced by `tools/bifrost/unready-forge-worker`. This is a std-only stand-in in the release tooling: its own one-package workspace (not a member of the repository's), built from the checkout release.sh runs from, for both architectures, and joined with lipo (762 KB).
  - It accepts forge-worker's command line and ignores it, except `--health-bind`, on which `/healthz` answers 200 and `/readyz` 503. It never logs `"message":"job run"` (what run-native.sh waits for), logs "not ready" every 30 s, and runs until signalled.
  - It is SBOM'd (mg-sbom in a staging copy of its Cargo.toml and Cargo.lock, so its SBOM names `unready-forge-worker`), signed and registered as `forge-worker@X.Y.Z` like the rest. The other five services are the real builds.
  - **Why a stand-in:** forge-worker has no option or environment that keeps it unready. Its `/readyz` toggle is always on, and a dead `--gateway` gives failing job runs, not missing readiness. A wrapper script would need the real binary's path inside the agent's slot, which B2 has not defined. The stand-in is one self-contained file, and the services' code is not touched. It is alive but unready, so the gate's 3-minute readiness timeout is what fails, not a crash.
- **Test:** `tools/bifrost/test-release.sh [<commit>]`, a script-level end-to-end test.
  - **Setup:** a temporary `BIFROST_HOME` with `run-local.sh setup` (a private enclosure, identities and state), and bifrost-server on 127.0.0.1:17760, health 19760.
  - **Checks:**
    1. A release whose key the server does not yet trust is stopped by the preflight.
    2. After `--init-key` and a restart, HEAD released as 0.1.0 lists six universal (`x86_64 arm64`) services, all verified.
    3. forge-worker signed as `0.1.0-tampered` and then changed by one byte is refused as `digest-mismatch` and listed as rejected.
    4. `--bad-build` as 0.1.1 lists six verified; its forge-worker differs, its SBOM names the stand-in, and when run it answers 200 on `/healthz` and 503 on `/readyz` and logs no job run.
    5. The build worktree is gone, and git no longer lists it.
  - **Result:** all 5 checks passed (HEAD 3d473ef).
  - **Times:** each release took about 34 s, of which the build was about 30 s (15 s per architecture). The build cache is 169 MB.
  - **Sizes (universal):** http-gateway 6.4 MB, heimdall-feed 5.4 MB, heimdall-source 3.1 MB, mimir-server 4.1 MB, forge-worker 4.2 MB, midgard-replay 1.4 MB. The second release of the same commit reused the build, so the five real services have the same digests in 0.1.0 and 0.1.1.
- **Checks:** clippy `-D warnings` and fmt are clean for the bins (run against HEAD plus these files, while registry.rs was being changed by review in parallel), and clippy is clean for the stand-in.
- **Not done here:** the running `bifrost-local` instance (port 7750) still runs the old `bifrost-fleet` binary without the registry, so nothing was released to it. That is Task 4.

### Task 4: Run it

- **Setup:** `tools/bifrost/run-local.sh` gains `setup --real-enclosure`. It issues the bifrost-server and agent identities in `~/Desktop/Midgard/airports`, which needs the gateway stopped (check that it is; refuse otherwise). This adds the roles `release-manager` and `fleet-agent`/`fleet-manager`, and the policy rules.
- **Run:** `start` runs `bifrost-server`. Then run `release.sh` for the current HEAD as 0.1.0 and for a bad build, and show the registry listing: verified, then rejected for a tampered copy.
- **Record** results in this plan.

**Result (24 Oct 2026, done).**
- **`run-local.sh setup --real-enclosure`** (eb56fb4):
  - **Service name:** the server's new identity is `bifrost-server` (the program's default `--service`). The old `bifrost-fleet` name stays only for a private-enclosure home, whose identity was issued under it. Clients do not pin the server's name, and the state ids do not depend on it (Task 2).
  - **Identities,** issued with `mg-admin mesh issue` from `~/Desktop/Midgard/airports` into `~/Desktop/Midgard/airports-mesh/bifrost/` (mode 700):
    - `server/`: `bifrost-server` (fleet-manager);
    - `clients/`: `fleet-agent-1..3` (fleet-agent), `fleet-operator` (fleet-operator) and `release-manager` (release-manager).
    - An identity already there is kept, and mg-admin refuses to overwrite one anyway. When all of them exist, the enclosure is not touched at all (no checks, no backup).
  - **Precondition, fail-closed.** It refuses if any of these hold:
    - a suite binary runs: `pgrep -x` for http-gateway, heimdall-feed, heimdall-source, mimir-server, forge-worker, midgard-replay or mg-admin, wherever they were started from;
    - any process runs from a `dist/mac/bin/`;
    - the enclosure holds any `*.lock` file. `mg-admin` and the gateway share `airports/midgard.lock` (`StateLock`, `services/runtime/src/lock.rs`). The script refuses any lock, live or stale, from any OS. A stale one is for an operator to remove after checking. mg-admin then takes the same lock itself, so a gateway that starts in between fails to lock rather than interleaving;
    - Docker cannot be queried or runs any container. `MIDGARD_DOCKER_STOPPED=1` skips this when Docker Desktop is quit, as run-native does.
  - **Backup:** before issuing, the enclosure is tarred to `~/Library/Caches/midgard-build/airports-backup-<YYYYMMDD-HHMMSS>.tar.gz`. The file count is checked, and only then are the previous `airports-backup-*` removed, so one is kept.
  - **`mesh.conf`:** the choice is recorded in `$BIFROST_HOME/mesh.conf` (enclosure, server identity, service, clients). `start`, `demo`, `status` and `release.sh` (default `--identity`) read it.
  - **Private state moved aside:** the server's state is sealed under its identity's audit keystore (`ServiceIdentity::data_sealer`), so the new identity cannot open the old `fleet.journal`/`fleet.ledger`. The first switch therefore moves the private enclosure, its identities and that sealed state to `$BIFROST_HOME/private-enclosure/`, and refuses while the server runs. Plain `setup` then refuses for that home.
  - **Also changed:**
    - `stop` waits for the exit and removes the lock of the process it stopped. The old `bifrost-fleet` build had written a lock with no `os=` line, which the new start refused as foreign. It was removed once, by hand, after checking that pid 48480 was gone.
    - setup always runs cargo (a no-op when fresh), so a stale binary is never started.
  - **Tests:** checked in a throwaway home and enclosure: refusal while the server runs, with Docker unqueryable, with a lock present, and with a process named `http-gateway`; the issue, backup rotation and archive; a second run that touches nothing; plain setup refused; start and demo on the new identity.
- **Run:**
  1. `run-local.sh stop` ended the old `bifrost-fleet` (pid 48480, 7750/9750).
  2. `MIDGARD_DOCKER_STOPPED=1 run-local.sh setup --real-enclosure` (Docker Desktop was quit) issued the six identities. Backup: `~/Library/Caches/midgard-build/airports-backup-20261024-214043.tar.gz` (242 MB; there was no earlier one). The enclosure's governance ledger grew by six ISSUE entries (31 KB), and no lock was left.
  3. The old `policy.json` (without registry actions) was replaced and kept as `policy.json.pre-registry`.
  4. `release.sh --init-key` made `bifrost-local/release-key/` (bifrost-release-1, ML-DSA-65) and installed its `.pub`, and the server was started (and restarted) on it. It logs `release_signers: bifrost-release-1`.
- **Releases** (HEAD eb56fb4), as release-manager against 127.0.0.1:7750:
  - 0.1.0: six verified, 32 s (build 28 s).
  - 0.1.1 `--bad-build`: six verified. forge-worker is the 762 KB stand-in; the other five have the same digests as 0.1.0.
  - A forge-worker signed as `0.1.0-tampered` and changed by one byte was refused: `digest-mismatch: the bytes do not match the digest in the signed manifest`.
  - The listing shows 12 verified (6 at 0.1.0 and 6 at 0.1.1) and 1 rejected (`forge-worker 0.1.0-tampered`).
- **The enclosure is unharmed:**
  - `run-native.sh start` with the hour-1 replay brought mimir, gateway, heimdall, forge and replay to ready. These were the `dist/mac/bin` binaries from Task 1, not rebuilt.
  - `smoke.py` passed: sign in; 22 fabric object types; Heimdall picture, 29,106 entities; Mimir granite-3.3-2b-instruct-q4km; Forge, 11 jobs.
  - `run-native.sh stop` stopped them all, and no lock was left in `airports`.
- **Left running:** bifrost-server, the user's dev instance (127.0.0.1:7750, health :9750/readyz 200). Stop it with `bash tools/bifrost/run-local.sh stop`.
- **Storage:** `bifrost-local` is 45 MB (the registry holds 12 sealed artefacts). The suite folder measured 4.6 GB, over the 3.5 GB budget. That is mostly older items: `midgard-app.pre-rewrite` 724 MB, `midgard-console.pre-rewrite` 268 MB, and this worktree 307 MB. They were not touched here.

### B1 summary

- **Done:** bifrost-server runs as the suite's dev instance on the real enclosure's mesh identities. It has a durable, sealed artefact registry that admits only builds signed by an anchored ML-DSA-65 release key with a matching SBOM, and lists everything else as rejected with its reason.
- **Release:** `release.sh` builds a commit's six services universal, SBOMs, signs and registers them (about 30 s), with a `--bad-build` canary for B2.
- **Native tools:** the suite runs natively on this branch (build-services, run-native, smoke).
- **Open for B2:**
  - I-5, read cost and lock hold in GetArtefact;
  - M-9, the reproducible-build digest check;
  - agent installs from the registry;
  - the canary gate and rollback with 0.1.1.
