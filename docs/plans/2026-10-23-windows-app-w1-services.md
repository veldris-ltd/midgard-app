# Windows app W1: Rust services on Windows, Windows CI — implementation plan

**Goal:** the suite's Rust services (http-gateway, heimdall-feed, heimdall-source, mimir-server, forge-worker, midgard-replay) build, pass their tests and run end to end on Windows 11 x64. This is proven on GitHub Actions `windows-latest` on every push to `md_windows-app`, which also uploads the Windows binaries as an artefact.

**Architecture:** add `#[cfg(windows)]` code beside the existing `#[cfg(unix)]` / `#[cfg(target_os)]` code. Mac and Linux behaviour stays byte-for-byte the same. Declare the few Win32 functions needed with `extern "system"` blocks (kernel32 is already linked by std) rather than adding `windows-sys`: vendoring it and its per-architecture packages would add tens of MB to a repo whose working folder is at its storage limit. A new workflow, `.github/workflows/windows.yml`, builds, tests and smoke-tests on Windows.

**Tech Stack:** Rust 1.82 (rust-toolchain.toml), MSVC target `x86_64-pc-windows-msvc`, GitHub Actions, Python 3 (smoke runner and stub model server; Python is preinstalled on runners).

Design: `docs/plans/2026-10-23-windows-app-design.md` §2. Mac native run for reference: `tools/mac-app/run-native.sh`, `tools/mac-app/smoke.py`, `tools/mac-app/build-services.sh`.

---

## Background

- **Unix-only code** (from `grep cfg(unix)|libc::|cfg(target_os)`):
  - `services/runtime/src/lock.rs`: directory locks, with dead-holder liveness from the process start time. There is Linux code (`/proc`) and macOS code (`proc_pidinfo`), and none for Windows.
  - `crypto/pqc-agility/src/secret.rs`: `mlock`, which already has a `not(unix)` fallback.
  - Files created with mode 0600 when `cfg(unix)`, and nothing otherwise:
    - `services/fabric/src/durable.rs` (×2)
    - `heimdall/feed-server/src/alerts.rs`
    - `heimdall/store/src/{util,history_store}.rs`
    - `crypto/audit-writer/src/{file,journal}.rs`
  - `crypto/attest/src/keystore.rs`: a permissions check when `cfg(unix)`.
- **Vendored crates:** none depend on `windows-sys` today, and `libc` is the only OS crate. `.cargo/config.toml` forces `/vendor`, so a crate not in `vendor/` fails the build. Keep it that way.
- **Existing CI:** `.github/workflows/ci.yml` runs only on `main` and `dev`, on ubuntu. The repo is public, so Windows runner minutes are free.
- **Branch rules:** signed commits, explicit paths, push to `md_windows-app` only.
- **Storage:** nothing large in `~/Desktop/Midgard` (3.5 GB limit). Local cargo output goes to `CARGO_TARGET_DIR=~/Library/Caches/midgard-build/test-target`. Cargo is at `/opt/homebrew/opt/rustup/bin`.

### Task 1: Windows CI that builds

- **Workflow:** `.github/workflows/windows.yml`, on push to `md_windows-app`. It also has `workflow_dispatch`.
- **Job:** `windows-latest`, with the rust-toolchain.toml toolchain plus the msvc target. It runs `cargo build --offline --locked --release -p http-gateway -p heimdall-feed-server -p mimir-server -p forge-worker -p midgard-replay --bins` and lists the produced `.exe` files.
- **Caching:** cache `target/` keyed on Cargo.lock.
- **Commit and fix loop:** commit it, push, and watch the run (`gh run watch`). Fix compile errors one at a time with `#[cfg(windows)]` code (smallest change; Unix untouched). Lock liveness comes in Task 2: until then, a `#[cfg(windows)]` stub that reports "unknown" (treats the holder as alive, so a lock is never stolen) is fine.
- **Check:** build green. Also confirm macOS still builds and tests pass locally (`cargo test` for the touched crates) and Linux CI is unaffected (no change to ci.yml jobs).

### Task 2: Directory locks on Windows (`services/runtime/src/lock.rs`)

- **Read first:** read the whole module and its tests. The behaviour to keep: one holder per state directory; the lock file records the holder's pid and start time; a dead holder's lock is reclaimed; a foreign-OS lock is refused (see commit c7d8054).
- **Windows implementation:**
  - **Liveness:** use `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`, `GetProcessTimes` (creation time) and `GetExitCodeProcess` (`STILL_ACTIVE`), declared in an `extern "system"` block.
  - **Exclusive hold:** match what the Unix code does. If it relies on an OS file lock, use `LockFileEx`/`UnlockFileEx` via `std::os::windows::io::AsRawHandle`.
  - **OS tag:** record `os=windows` in the lock as Unix records its own.
- **Tests (run on Windows CI):**
  - a second holder is refused;
  - a lock left by a process that has exited is reclaimed;
  - a lock by a live process with a different start time is treated as dead (pid reuse);
  - a foreign-OS lock is refused;
  - the existing tests pass.
- **CI:** add `cargo test --offline --locked -p service-runtime` (the runtime crate's real package name is in its Cargo.toml) to the workflow.

### Task 3: The whole Rust test suite on Windows

- **Workflow:** add `cargo test --offline --locked --workspace --exclude <crates that cannot apply on Windows, each with a reason in a comment>`. Prefer fixing over excluding.
- **Expected hazards:**
  - **File-in-use:** delete or rename of an open file fails on Windows, so close before replace.
  - **Rename-over:** `std::fs::rename` replaces on Windows (MoveFileExW with REPLACE_EXISTING) but fails across volumes and when the target is open.
  - **Paths:** test expectations with `/` in paths; CRLF in fixtures (add a `.gitattributes` `eol=lf` for fixture and data files if a test reads them byte for byte).
  - **Temp and ports:** temp directory paths and long paths; tests binding fixed ports.
  - **Permissions:** tests that assume Unix permissions (gate them `#[cfg(unix)]` with a one-line reason).
- **0600-on-Unix files:** on Windows the file inherits its folder's ACL. In the app that folder is `%LOCALAPPDATA%\Midgard`, private to the user by default. Write that down in each place as a one-line comment. No ACL code: the W2 shell checks the working-copy folder's ACL once at start.
- **keystore.rs:** on Windows, skip the Unix mode check with the same comment.
- **Check:** CI green with the full test step. The macOS local `cargo test` for every touched crate also passes.

### Task 4: Fixture recording and end-to-end smoke on Windows

- **Fixture recording** (`tools/win-app/fixture/`, tiny, committed, open data only):
  - a `meta.json` and `heimdall.jsonl.gz` with about 20 observations across air, sea, land, events and utilities;
  - one land-camera with a `/media/` still: a small generated JPEG, our own;
  - one Forge CSV.
  - It must be made by a script (`tools/win-app/make-fixture.py`) from synthetic data, with no recorded third-party data.
- **Runner** (`tools/win-app/run-suite.py`, cross-platform Python): the Windows counterpart of `run-native.sh` for CI.
  - **Working copy:** creates a temporary working copy from the repo's starting-state builder or a minimal identity set. Find how `tools/mac-app` builds the starting state, and reuse it (it may need Python only).
  - **Services:** starts mimir-server against a **stub model server** (`tools/win-app/stub_llm.py`: answers the HTTP API mimir-server calls on llama-server with a fixed, valid A.1 JSON reply), then gateway, heimdall-feed, forge-worker, and midgard-replay on the fixture.
  - **Checks:**
    - runs `tools/mac-app/smoke.py` (or a Windows-safe copy of it) against the gateway;
    - checks the picture has the fixture's layers;
    - checks an ask returns the stub's answer;
    - checks the camera entity is present;
    - quits every process and checks nothing is left running.
- **CI:** run it on Windows; on failure, upload the services' logs as an artefact.

### Task 5: Windows binaries artefact

- The workflow uploads `midgard-services-windows-x64-<sha>` (the six `.exe` files plus a `BUILD.txt` with the commit, toolchain and date), kept 30 days.
- **`tools/win-app/fetch-artifacts.sh <sha>`** (runs on the Mac, uses `gh run download`):
  - downloads that artefact into `~/Library/Caches/midgard-build/win/<sha>/`;
  - checks `BUILD.txt` names the commit;
  - lists the files.
- **Check:** run the script for the green commit; the files arrive and `file` reports PE32+ x86-64.

### Task 6: Record results

Record results in this plan: CI run links, test counts, anything excluded and why, smoke output. Update `docs/plans/2026-10-23-windows-app-design.md` if anything changed.

## Results (23 Oct 2026)

W1 is done. Every task's check passed on GitHub Actions `windows-latest` (Windows Server, x86_64-pc-windows-msvc, Rust 1.82.0, Git Bash).

- **CI runs:**
  - [40010805091](https://github.com/veldris-ltd/midgard-app/actions/runs/40010805091) on 5bce18c: Tasks 1 to 4 green (build, smoke, tests).
  - [40012836561](https://github.com/veldris-ltd/midgard-app/actions/runs/40012836561) on 4be06ce: the same, plus the review follow-ups and the binaries artefact (Task 5).
- **Commits:**
  - Task 1: 3795ff5.
  - Task 2: 1b6080d.
  - Task 3: b346c70, ac7df50, 1b241c2, f356f3b, 0b6ff06, c578ec9, 7a153f1.
  - Task 4: 5bce18c.
  - Review follow-ups: 9362e80, fbbb306.
  - Task 5: 4be06ce.
- **Tests on Windows:** 1073 passed, 0 failed, 1 ignored on 5bce18c, and 1079 / 0 / 1 on 4be06ce with the new replay tests. Counted over every `test result:` line of the windows-test step, doc-tests included. The ignored one is `time_a_sync_sized_batch`, a timing probe that is run by hand on every platform.
  - `windows-test` runs the justfile's 11 `cargo clippy` and 50 `cargo test` lines. Then it runs the workspace members those lines leave out, with their default features:
    - lint: mg-model, midgard-replay, telemetry, mimir-bridge, heimdall-store, admin-orchestrator;
    - test: the same, plus mg-bundle and mg-register.
  - A recipe that yields no line fails the job.
- **Excluded:** nothing. `WINDOWS_TEST_EXCLUDE` is empty. Unix-only checks inside tests are gated `#[cfg(unix)]`, each with a one-line reason:
  - the 0600 modes;
  - the keystore mode check;
  - gzip in replay's tests;
  - the pgrep leftover check.
- **Fixes the Windows tests found:**
  - **Fabric store** (c578ec9, Windows only):
    - an append-only handle cannot truncate, so a failed write's cut-back opens a writing handle of its own;
    - Windows will not rename over an open file, so the journal is closed (the store wedged meanwhile) before the compacted one replaces it.
  - **Fabric store** (7a153f1):
    - Windows: compaction's rename-over is MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH), retried for about 2 s while another handle blocks it, and the reopened journal is synced;
    - Windows: a cut-back replaces the journal handle;
    - **all platforms:** the store now stays wedged from the rename until the new journal is open. Before, a failed reopen could leave writes going to the replaced journal, which on Unix is unlinked.
  - **Fleet check-in test:** each accepted socket is set blocking. It inherits the listener's non-blocking mode on macOS and Windows.
  - **Directory locks** (Task 2): liveness and start time come from kernel32 (OpenProcess, GetExitCodeProcess, GetProcessTimes) in an `extern "system"` block. No new crate.
- **Review follow-ups** (9362e80, fbbb306):
  - **heimdall-source path:** replay's default `--source-bin` is `heimdall-source` + `EXE_SUFFIX` beside it.
  - **Forge file rename:** the rename-over is retried for about 3 s (10 tries, 50 to 500 ms) on a Windows sharing violation or access denied, while forge-worker has the CSV open. Unix tries once.
  - **Fake heimdall-source in tests:** the tests whose fake is a batch file running Git's `sh` are skipped, with a message, when there is no `sh`.
  - **Recipe lines:** `recipe_cargo_lines` matches `cargo <sub>` (or `@cargo`) as the first two words.
- **Recordings on Windows:** they ship **uncompressed** (`heimdall.jsonl`). Windows has no gzip for replay to read `heimdall.jsonl.gz` through. Given a `.gz`, replay on Windows refuses it: "Windows reads plain heimdall.jsonl; unpack the recording (gzip -d heimdall.jsonl.gz) before playing it". The packaging step (`build-zip.sh`, design §4) must unpack it. Mac recordings are unchanged.
- **Working copy for the smoke test:** `make-starting-state.sh` copies a live enclosure home, which CI does not have. So `tools/win-app/run-suite.py` builds a minimal working copy, the way `deploy/compose/init.sh` first makes an enclosure:
  - `mg-admin init`;
  - the gateway policy and an ontology for the fixture;
  - one analyst with a random password;
  - mesh identities per service and per recorded feed;
  - Heimdall's and Mimir's policies;
  - one Forge job;
  - a stub model directory signed by `mg-model`.

  Each service runs in its own Job Object on Windows.
- **Smoke output** (run 40012836561, the same as 40010805091):

```
fixture: target/windows-smoke/fixture: 19 observations in 11 batches from 6 feeds, 1 camera still, 1 Forge file
run-suite: working copy made: keys, identities, policies, Forge job, signed stub model
run-suite: stub-llm ready / mimir ready / gateway ready / heimdall ready / forge ready / replay ready
ok   sign in
ok   picture has the fixture's layers (10 entities; layers air,events,land,sea,utilities)
ok   camera entity with its /media/ still (land-camera, with /media/CAM-TEST.jpg)
ok   forge job ran on the CSV (3 site objects in the fabric)
  smoke.py: ok   sign in / fabric object types (3) / heimdall picture (10 entities) / mimir model plane (granite-stub) / forge overview (1 jobs)
ok   tools/mac-app/smoke.py (exit 0)
ok   ask is grounded with the stub's answer (grounded=True, citations=['AREA-SUMMARY'], considered=10)
run-suite: replay stopped / forge stopped / heimdall stopped / gateway stopped / mimir stopped / stub-llm stopped
ok   every process stopped, none left running
run-suite: 7 of 7 checks passed
```

- **Binaries artefact:** `midgard-services-windows-x64-4be06ce3bf382abe1fcdbbbf91160886a9aaa891`, 4.0 MB zipped, kept to 22 Nov 2026. `bash tools/win-app/fetch-artifacts.sh 4be06ce` put it in `~/Library/Caches/midgard-build/win/4be06ce…/`. `BUILD.txt` names the commit, and `file` reports `PE32+ executable (console) x86-64, for MS Windows` for each file:

  | File | Bytes |
  |---|---|
  | http-gateway.exe | 3,119,616 |
  | heimdall-feed.exe | 2,470,400 |
  | heimdall-source.exe | 1,251,840 |
  | mimir-server.exe | 1,747,456 |
  | forge-worker.exe | 1,837,056 |
  | midgard-replay.exe | 497,152 |
