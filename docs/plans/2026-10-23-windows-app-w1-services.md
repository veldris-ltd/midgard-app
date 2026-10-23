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
