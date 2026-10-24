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

### Task 3: release.sh

`tools/bifrost/release.sh <commit> --version X.Y.Z [--bad-build]`:
1. Build the suite services for the commit in a temporary worktree, with build-services' approach and universal binaries.
2. Write an SBOM per binary (`mg-sbom`) and sign each (`mg-sign`) with the release key. The key comes from a dedicated release keystore made once under `~/Desktop/Midgard/bifrost-local/release-key/` and never committed; its public half is installed into the server's `trust/release-signers/`.
3. Upload each to bifrost-server with `RegisterArtefact`, via `bifrost-agent` or a small `bifrost-cli` binary in the crate; choose and justify.
4. `--bad-build` builds a variant whose readiness never comes, for the demo rollback in B2. Use an environment or argument the service honours, or a wrapper. Choose the least invasive; it must not touch production code paths.
5. Print what was registered.

### Task 4: Run it

- **Setup:** `tools/bifrost/run-local.sh` gains `setup --real-enclosure`. It issues the bifrost-server and agent identities in `~/Desktop/Midgard/airports`, which needs the gateway stopped (check that it is; refuse otherwise). This adds the roles `release-manager` and `fleet-agent`/`fleet-manager`, and the policy rules.
- **Run:** `start` runs `bifrost-server`. Then run `release.sh` for the current HEAD as 0.1.0 and for a bad build, and show the registry listing: verified, then rejected for a tampered copy.
- **Record** results in this plan.
