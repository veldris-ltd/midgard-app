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

### Task 4: ModelServer (CPU llama-server)

- **Port `ModelServer.swift`'s arguments:** model path, alias, ctx 8192, threads = physical cores up to 8, `--n-gpu-layers 0`, and parallel/`--cont-batching` as the Mac uses.
- **Locate:** `helpers\llama\llama-server.exe` (fetched in W4). Fall back to `MIDGARD_LLAMA` for dev.
- **Tests:** arguments for 4/8/16 cores.

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
