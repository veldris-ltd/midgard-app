# Windows app W2: the C# shell — implementation plan

**Goal:** `Midgard.exe`, a C#/.NET 10 (LTS) WinForms app hosting WebView2. Like the Mac shell (`apps/mac`), it checks the PC, keeps the working copy, starts and supervises the services, serves the home page, consoles and recording files on loopback, signs the presenter in, and shows everything offline in one window. Quitting leaves nothing running.

**Architecture:** a port of `apps/mac/Sources/MidgardShellCore`, about 4,000 lines of Swift across Paths, Ports, SystemCheck, Home, ServiceSpec/SuiteSpecs, Supervisor, ModelServer, Session, WebServer and Shell. The port is a core library, `Midgard.Core`, with no UI and testable with xUnit on any OS where it can be. The window app (`Midgard`) uses WinForms, WebView2 and Job Objects. Behaviour matches the Swift code unless Windows needs otherwise; read the Swift file before writing each part.

**Tech Stack:** .NET 10 SDK (LTS; the windows-latest runners carry it, picked by `apps/windows/global.json`), C# 14 (net10's default), WinForms, Microsoft.Web.WebView2 (NuGet, pinned version and hash in `packages.lock.json`), xUnit. Publish: `dotnet publish -r win-x64 --self-contained -p:PublishSingleFile=true`. CI goes through `tools/ci/run.sh` jobs (`windows-shell-build`, `windows-shell-test`), mirrored in `.gitlab-ci.yml` per `tools/ci/pipelines.sh`.

Design: `docs/plans/2026-10-23-windows-app-design.md` §3. W1 (services on Windows, `tools/win-app/run-suite.py`, which already uses Job Objects and a working-copy builder): `docs/plans/2026-10-23-windows-app-w1-services.md`.

---

## Background

- **Layout of the extracted ZIP:** `Midgard\Midgard.exe`, `Midgard\helpers\*.exe` (the services, llama, analytics runtime and frames helper in W3), and `Midgard\resources\` (the consoles, home, starting-state, models, recording, licences). `Paths` finds them from the exe's folder.
- **The app's folder:** `%LOCALAPPDATA%\Midgard`, which is private to the user by default. Check its ACL once at start: refuse if Everyone, Users or Authenticated Users can read it, and explain. It holds three folders, so Reset demo can wipe the working copy without touching the logs or the browser profile:
  - `Home\`: the working copy (`MIDGARD_APP_HOME` overrides this one only);
  - `Logs\`: one log per service plus shell.log (`MIDGARD_APP_LOGS` overrides this one only);
  - `WebView\`: the WebView2 user-data folder (Task 6).
- **Starting state:** the same `resources\starting-state` as the Mac bundle, built on the Mac by `tools/mac-app/make-starting-state.sh` and copied by the W4 packager. W2 tests use the W1 minimal working copy from run-suite.py's builder.
- **Readiness patterns, arguments and order:** follow `SuiteSpecs.make` (ServiceSpec.swift) and `tools/mac-app/run-native.sh`. Analytics starts last and is optional; it is added in W3.
- **NuGet:** use a lock file (`RestorePackagesWithLockFile`) and `--locked-mode` restore in CI. No packages beyond WebView2, the WinForms SDK and the test packages. Record each package's licence in `resources\licenses`.
- **Rules:** signed commits, explicit paths, `md_windows-app` only. Keep the repo small (the suite folder is at its 3.5 GB limit). Commit no `bin/` or `obj/`, and add `.gitignore` entries.

### Task 1: Solution, CI and Paths/Ports/SystemCheck

- **Projects:** `apps/windows/Midgard.sln` with `Midgard.Core` (net10.0 class library), `Midgard` (net10.0-windows WinForms exe, which can show a blank window for now) and `Midgard.Core.Tests` (xUnit).
- **Port these:**
  - `Paths`, the bundle layout plus the working copy, logs and WebView2 folder, overridable with `MIDGARD_APP_HOME` and `MIDGARD_APP_LOGS` like the Mac app;
  - `Ports`, free loopback ports;
  - `SystemCheck`: Windows 11 build ≥ 22000, x64, ≥ 8 GB RAM, ≥ 4 GB free disk on the working-copy drive, WebView2 runtime version present. It should read like the Mac messages.
- **CI:** `windows-shell-build` and `windows-shell-test` jobs on windows-latest run restore (locked), build, test and publish win-x64 single-file, then list the output size.

### Task 2: Home (working copy) and the ACL check

- **Port `Home.swift`:** prepare (first run copies starting-state with an integrity check), the `.midgard-home` marker, refusing a folder that isn't a working copy, reset (Reset demo), the version check, and the instance lock (one app per working copy, a stale lock reclaimed; match the Mac behaviour).
- **ACL check:** from the working copy's security descriptor. Tests use temp folders.
- *As built:*
  - `Midgard.Core/Home.cs`. Reset makes the fresh copy in a sibling (`Home.midgard-copy-…`) while Home is still in place. Only then does it swap the copy in and delete the old one, so a failure mid-copy leaves Home as it was. `Home.Reset` takes `servicesStopped` and refuses without it: Windows won't move a folder while a file in it is open. The copy is checked against the starting state (same file count and bytes) before it is marked. The room check is always made (NTFS has no clones). Copies lose the read-only attribute and the `Zone.Identifier` stream. Symbolic links and junctions in the starting state are refused. Long paths need nothing special (.NET adds `\\?\`).
  - `InstanceLock.cs`: `<Home>.instance` beside Home, held open with `FileShare.Read` for the app's life. It holds the pid, then `stopping`. A second launch brings that pid's window forward (`Midgard/SingleInstance.cs`) or waits for a quitting app. A crashed holder's handle is closed by Windows, so its lock is free at once.
  - `FolderAccess.cs` holds the decision, a pure function. `WindowsAcl.cs` reads the DACL (Windows only). The check covers Home, Logs and the WebView folder's parent, or each one's nearest existing ancestor. Any allow entry giving Everyone, Users or Authenticated Users read access (read data, generic read or generic all) is refused, and so is an ACL that can't be read. `Program` checks it after the system check.

### Task 3: Supervisor and SuiteSpecs with Job Objects

- **Port `ServiceSpec`/`SuiteSpecs` and `Supervisor`:**
  - start order and arguments;
  - readiness (a log line or an HTTP check, as in the Swift code);
  - timeouts, one retry, a status stream for the starting screen, optional services;
  - stop in reverse order;
  - a final sweep;
  - reaping leftovers from a crash by executable path under `helpers\`.
- **Job Objects:** every child goes in one Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, through P/Invoke to kernel32. Do it the way run-suite.py does.
- **Tests:**
  - spec order;
  - a fake service (a tiny test exe or `cmd /c`) that becomes ready, one that never does (timeout, then the optional one doesn't fail the start), and one that crashes and is retried once;
  - after `StopAll` and after killing the shell process, nothing is left (job close).
- *As built:*
  - `Midgard.Core/ServiceSpec.cs`: `SuiteSpecs.Make` takes the model server's command (`Command`, from ModelServer in Task 4) and runs the rest from `helpers\*.exe` with Swift's arguments and readiness, as Windows paths. Replay gets no `--source-bin`: it runs `heimdall-source.exe` from its own folder. The analytics spec and the gateway's analytics arguments are W3; `ServiceSpec.Optional` is in place for them.
  - `Supervisor.cs` follows Supervisor.swift (log-line or HTTP 200 readiness on output after the launch, 180 s timeout, restarts with back-off after ready, optional services, reverse-order stop, `CloseAsync`, 20 MB rotation to `.log.1`). Statuses come as `IAsyncEnumerable<ServiceEvent>` (one reader). **One retry:** a service that exits before it is ready is launched once more after the first back-off (published `Restarting(1, why)`), then the start fails with the Swift message. A taken port isn't retried here (SuiteStart picks fresh ports), and neither is a timeout. Windows' "Only one usage of each socket address … (os error 10048)" counts as port in use.
  - `JobLauncher.cs`: each service is created with CreateProcessW, `CREATE_SUSPENDED | CREATE_NO_WINDOW`, then assigned to the app's job and to its own job (nested in the app's), then resumed. That closes the gap where a child (replay's heimdall-source) could start before the assignment. Both jobs are `KILL_ON_JOB_CLOSE`. The app's job is a static held for the process's life, so if Midgard.exe dies Windows ends every service and its children. Only NUL (stdin) and the log (stdout and stderr, opened for append) are inherited, through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`. Arguments are quoted for the C runtime's parser (`CommandLine`).
  - **Stopping:** `TerminateJobObject` on the service's job, then a wait of up to `StopGrace` for the job to empty. None of the services has a graceful stop to ask for. They install no signal or console-control handler, so on the Mac SIGTERM already ends them at once (run-native.sh notes the lock files a stop leaves), and run-suite.py stops them the same way. The stores are built for a kill: the fabric journal and Heimdall's stores flush each acknowledged write and replace files by rename, a torn history tail is dropped, and a dead holder's directory lock is taken over (`services/runtime/src/lock.rs`).
  - `Orphans.ReapAsync(helpers, alsoUnder)`: TerminateProcess on every process whose image (QueryFullProcessImageNameW) is under those folders, replay first. Each folder is matched as given, long-named (the runner's temp folder is `RUNNER~1`) and fully resolved. Unlike the Mac, a llama-server outside the helpers isn't matched by its model argument: Windows doesn't expose another process's command line.
  - Tests (Windows CI, 125 in all with Tasks 1–2): the specs, their port wiring and the quoting, and the Swift Supervisor cases. The fake service is the test assembly's own exe (`FakeService`, its Main). Further tests cover the retry, a killed "shell" (the fake running a Supervisor) taking both services and a grandchild with it, and reaping by path with renamed copies of the fake.

### Review fixes (Tasks 1–3)

- *As built:*
  - **Kill and Dispose race:** a service process's Kill, ActiveProcesses and Dispose take one small lock, and a Job Object call on a closed handle is a no-op. The exit wait has its own copy of the process handle (DuplicateHandle), owned by the wait handle, so the thread pool never waits on a handle that has been closed. Stop runs each service's stop in a try/catch: if one fails, its handles are closed (which ends its job) and the rest still stop. The app's job is made on first use, without caching a failure. The per-poll linked cancellation source is disposed.
  - **Refused input:** the launcher starts `.exe` files only. CreateProcessW runs `.bat` and `.cmd` through cmd.exe, which parses arguments by its own rules ("BatBadBut"). The environment block refuses an extra variable whose name is empty or holds `=` or NUL, or whose value holds NUL.
  - **Status stream:** bounded to 1024 unread changes, dropping the oldest. `StatusOf` always has the latest.
  - **Orphans:** candidates are only processes in this Windows session that run as this user. A process counts as reaped only if TerminateProcess succeeded and it then exited (WaitForSingleObject). One that couldn't be opened or ended is returned in `SweepOutcome.NotEnded`, for shell.log. `Orphans.SweepAsync(helpers, holdsInstanceLock, …)` is the shell's call; Task 6 wires it into Program. It skips the sweep when the instance lock isn't held, or when another `Midgard.exe` from the same install (beside `helpers\`) runs in this session. Two working copies from one install (MIDGARD_APP_HOME) share `helpers\`, so the second's sweep would otherwise end the first's services.
  - **FolderAccess:** refuses INTERACTIVE, Guests, any domain's Domain Users (RID 513) and any other S-1-5-21 account or group as well. Write-data, WRITE_DAC, WRITE_OWNER and generic-write grants count as well as read. Deny entries are ignored, which only errs towards refusing. The folder Home is in is checked too, which matters with MIDGARD_APP_HOME. Program passes the presenter's SID.
  - **SingleInstance:** a held lock counts as another app's only if its pid is a running Midgard.exe (checked by image path). Otherwise it is retried for 2 s, and then the app runs without the lock, so the sweep is skipped.
  - **CI:** the shell job has `timeout-minutes: 30`. `dotnet test` runs with `--blame-hang-timeout 5m` and a .trx logger into `target/windows-shell-test/`, uploaded on failure. `global.json` pins the 10.0.4xx band (`10.0.400`, `latestPatch`).

### Task 4: ModelServer (CPU llama-server)

- **Port `ModelServer.swift`'s arguments:** model path, alias, ctx 8192, threads = physical cores up to 8, `--n-gpu-layers 0`, and parallel/`--cont-batching` as the Mac uses.
- **Locate:** `helpers\llama\llama-server.exe` (fetched in W4). Fall back to `MIDGARD_LLAMA` for dev.
- **Tests:** arguments for 4/8/16 cores.
- *As built:*
  - `Midgard.Core/ModelServer.cs`: `Make(executable, models, port, threads)` gives the `Command` `SuiteSpecs.Make` takes, with the Mac's Intel arguments: `-m <models>\granite\granite-3.3-2b-instruct-Q4_K_M.gguf`, `--alias granite-3.3-2b-instruct-q4km`, 127.0.0.1 and the port, `--ctx-size 8192`, `--parallel 1`, `--temp 0.2`, the `{"type":"object"}` JSON schema, `--n-gpu-layers 0` and `--threads`. The Mac passes no `--cont-batching` (llama-server's default), so neither does this.
  - **Threads:** `Threads(physicalCores, logicalProcessors, environment)`: the physical cores (the logical processors when unknown), clamped to 1–8. `PhysicalCores()` counts GetLogicalProcessorInformationEx's `RelationProcessorCore` records (null off Windows). MIDGARD_MODEL_THREADS, a positive number, is honoured as given, past the cap too, as on the Mac.
  - **Locate:** in a package only `helpers\llama\llama-server.exe`, null if missing (as the Mac refuses to fall back in a bundle). A development layout (`Paths.DevLayout`) runs MIDGARD_LLAMA when it is set, null if that file is missing, else the bundled one, else `helpers\llama-server.exe`.
  - Tests: 4/8/16 (and 2/6) cores, at least one thread, unknown cores, the override and bad values, the arguments in full, SuiteSpecs running the command, locating in a package and a dev layout, and this PC's core count.

### Task 5: WebServer

- **Port `WebServer.swift` exactly:**
  - loopback only, port 0;
  - the Host check (421) and GET/HEAD only;
  - routes `/` → home, `/home/…`, `/<product>/…` for the consoles with the 301 to a trailing slash, and the recording folders `/media/`, `/radar/`, `/traffic/` and `/tiles/`;
  - no traversal (including `\`, `%5c`, drive letters, `:` streams and reserved names like `CON`/`NUL`);
  - dot files refused;
  - symlinks and junctions must resolve inside their folder;
  - byte ranges (206/416);
  - content types;
  - the offline console rules.
- **Implementation:** HttpListener needs URL ACLs or admin rights for anything other than localhost, so use a TcpListener-based minimal server like the Swift one. Bind only to 127.0.0.1.
- **Tests:** port the Swift WebServerTests cases one for one, plus the Windows-specific path cases.
- *As built:*
  - `Midgard.Core/WebServer.cs` follows WebServer.swift: a `Socket` bound to 127.0.0.1 (port 0 by default, no SO_REUSEADDR), one request per connection, a 16 KB head limit (431), the 15 s head timeout and the 30 s stall timeout re-armed for each 256 KB chunk, the Host check (400 without one, 421 for another), GET/HEAD only (405 with `Allow`), the routes and recording folders, the 301 to a trailing slash, the byte ranges (206/416, suffix ranges, HEAD) and the content types. Like NWListener it sets no limit on connections. Unlike the Swift server it reads and drops what the client still sends after the response (up to 1 MB for 2 s) before closing, because closing a Windows socket with unread data resets the connection under the response (the 431 case).
  - **Windows paths:** besides `..`, NUL and `\` (also `%5c`/`%5C`, and `%2f` decoded before the split), a segment is refused with 403 if it holds `:` (drive letters, alternate data streams), another character Windows forbids in names or a control character, ends in a dot or a space (Windows strips them, so `..%20` would be `..`), or is a reserved device name with or without an extension (`CON`, `NUL`, `COM0`–`COM9`, `LPT0`–`LPT9`, the superscript ones, `CONIN$`, `CONOUT$`). A lone `.` is allowed under home and the consoles, and is a dot file (404) in the recording folders, as on the Mac. Malformed escapes or bytes that aren't UTF-8 get 400, as `removingPercentEncoding` would.
  - **Links:** `RealPath` resolves a path through a handle (CreateFileW follows every symbolic link and junction; GetFinalPathNameByHandleW names the result, long names, real case). The file must be inside its folder's resolved path, compared without regard to case. It's checked during resolution and again on the handle of the file actually opened, so a link swapped in between is caught. In the recording folders a real name starting with a dot is refused too (404), so an 8.3 short name (`HIDDEN~1.JPG`) can't reach a dot file. The orphan sweep uses the same `RealPath`.
  - Tests: every WebServerTests.swift case in its order, then the Windows path cases over HTTP and as a pure rule, malformed escapes, junctions out of the folder and out of root (a junction staying inside is served), 8.3 names, letter case, and the listener's address (`LocalEndPoint`, the system's TCP listener table, and no `::1`).

### Task 6: Session and the window

- **Port `Session.swift`:**
  - sign in from `demo-users.json`;
  - refresh before expiry;
  - the injection script, with the same contents and checks: `location.origin` guard, gateway URL, console URL, prepared-answers URL, session token in sessionStorage.
- **Window (`Midgard`):** WinForms with a WebView2 control.
  - **Profile:** the WebView2 user-data folder is `Paths.WebView` (`%LOCALAPPDATA%\Midgard\WebView`), outside the working copy.
  - **Session script:** `AddScriptToExecuteOnDocumentCreatedAsync`.
  - **Offline rules:**
    - `AddWebResourceRequestedFilter("*")`, which refuses any request outside the app's origin (127.0.0.1:<webport>) or the gateway (127.0.0.1:<gatewayport>);
    - `NavigationStarting` keeps navigation on the app's origin;
    - `NewWindowRequested` is refused (handled, no window);
    - downloads are refused.
  - **Toolbar:** Home, Reset demo (with a confirm), Quit.
  - **Starting screen:** each service's status, the way StartingView does.
  - **First run:** offers a Start-menu shortcut, a `.lnk` in `%APPDATA%\Microsoft\Windows\Start Menu\Programs`, via the WScript.Shell COM object or IShellLink.
  - **Close:** closing the window quits (stop all), with a second launch focusing the running one.
- **Tests:** the injection script (same cases as the Swift tests) and the request filter's decision function (a pure function: URL and allowed origins in, allow or block out).

### Task 7: Headless end-to-end on Windows CI

- **Headless mode:** `Midgard.exe --headless-check` (hidden; tests only) does prepare, start every service with the W1 fixture and stub model, sign in, fetch `/` and `/heimdall/` from its own web server, call the gateway picture, then quit. It exits 0 only if each step passed and no child is left, writing a JSON report.
- **CI:** a `windows-shell-e2e` job builds the services (from the W1 job outputs or by building) and runs it with `MIDGARD_APP_HOME` in a temp folder. Upload the logs on failure.
- Record results in this plan.
