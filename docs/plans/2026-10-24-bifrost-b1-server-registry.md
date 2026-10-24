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
