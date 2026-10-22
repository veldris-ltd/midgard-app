import AppKit
import Foundation
import MidgardShellCore
import WebKit

/// The five places the toolbar can show.
enum Destination: String, CaseIterable, Identifiable {
    case home, midgard, heimdall, mimir, forge

    var id: String { rawValue }
    var title: String { rawValue.prefix(1).uppercased() + rawValue.dropFirst() }

    func url(origin: String) -> URL {
        switch self {
        case .home: return URL(string: origin + "/")!
        default: return URL(string: "\(origin)/\(rawValue)/index.html#/")!
        }
    }

    /// The section a page of the app's own web server belongs to.
    init(path: String) {
        let first = path.split(separator: "/").first.map(String.init) ?? ""
        self = Destination(rawValue: first) ?? .home
    }
}

/// What the starting screen shows when something went wrong.
struct StartFailure: Equatable {
    var title: String
    /// Plain-English explanation, then (for a service) its last log lines.
    var details: String
    /// False when trying again cannot help (an unsupported Mac).
    var canRetry = true
}

/// Ties the core together: system check, working copy, web server, services, sign-in,
/// the web view's session, Reset demo, Switch user and quitting.
@MainActor
final class AppModel: ObservableObject {
    enum Phase: Equatable {
        case starting
        case failed(StartFailure)
        case ready
    }

    @Published private(set) var phase: Phase = .starting
    /// What the starting screen says above the services ("Starting Midgard…").
    @Published private(set) var startingTitle = "Starting Midgard…"
    /// A step before the services start ("Preparing the demo data…"), if any.
    @Published private(set) var startingStep: String?
    @Published private(set) var serviceOrder: [String] = []
    @Published private(set) var statuses: [String: ServiceStatus] = [:]
    @Published private(set) var users: [DemoUser] = []
    @Published private(set) var currentUser: DemoUser?
    @Published var section: Destination = .home
    /// A non-blocking note for the presenter (e.g. a leftover from a reset).
    @Published var note: String?
    @Published private(set) var switchingUser = false

    let paths: Paths
    let log: ShellLog
    let web = WebController()
    private let supervisor = Supervisor()
    private var server: WebServer?
    private(set) var origin = ""
    private var ports: SuitePorts?
    private var session: SessionInfo?
    private var replaying = false
    private var prepared = false
    private var reaped = false
    private var startTask: Task<Void, Never>?
    private var refreshTask: Task<Void, Never>?
    private var statusTask: Task<Void, Never>?
    private var instanceLock: InstanceLock?
    private var shuttingDown = false
    private let selfTest = ProcessInfo.processInfo.environment["MIDGARD_APP_SELFTEST"] == "1"

    init(paths: Paths = .current()) {
        self.paths = paths
        log = ShellLog(file: paths.logs.appendingPathComponent("shell.log"))
        web.onPageLoaded = { [weak self] url in self?.pageLoaded(url) }
    }

    private var resources: URL { paths.resources }
    private var bin: URL { resources.appendingPathComponent("bin", isDirectory: true) }
    private var startingState: URL { resources.appendingPathComponent("starting-state", isDirectory: true) }

    // MARK: Launch

    /// Before any window: true if this is the only shell, else the other one has been
    /// brought forward and this one should exit.
    func claimSingleInstance() -> Bool {
        let me = NSRunningApplication.current
        if let id = Bundle.main.bundleIdentifier {
            let others = NSRunningApplication.runningApplications(withBundleIdentifier: id)
                .filter { $0.processIdentifier != me.processIdentifier }
            if let other = others.first {
                log("another Midgard is running (pid \(other.processIdentifier)); activating it and quitting")
                other.activate(options: [.activateAllWindows])
                return false
            }
        }
        // Also without a bundle id (swift run): a lock beside the working copy.
        let file = paths.home.deletingLastPathComponent()
            .appendingPathComponent("\(paths.home.lastPathComponent).instance")
        switch InstanceLock.acquire(file) {
        case .acquired(let lock):
            instanceLock = lock
            return true
        case .heldBy(let pid):
            log("another Midgard holds \(file.path) (pid \(pid.map(String.init) ?? "?")); activating it and quitting")
            if let pid, let other = NSRunningApplication(processIdentifier: pid) {
                other.activate(options: [.activateAllWindows])
            }
            return false
        case .failed(let reason):
            // Better to run than to refuse: the bundle check above still guards the app.
            log("single-instance lock unavailable: \(reason)")
            return true
        }
    }

    func launch() {
        log("launch: resources \(resources.path), home \(paths.home.path), logs \(paths.logs.path)")
        if let problem = SystemCheck.problemOnThisMac() {
            log("system check: \(problem)")
            phase = .failed(StartFailure(title: "Midgard cannot run on this Mac", details: problem, canRetry: false))
            return
        }
        listenToStatuses()
        startTask = Task { await self.bootstrap() }
        snapshotLater("starting", after: 5)
    }

    /// The starting screen's "Try again".
    func retry() {
        guard case .failed(let failure) = phase, failure.canRetry else { return }
        startTask?.cancel()
        startTask = Task {
            await self.supervisor.stopAll()
            await self.bootstrap()
        }
    }

    private func listenToStatuses() {
        guard statusTask == nil else { return }
        let stream = supervisor.statuses
        // Started before any `start`, so no status is missed; the supervisor's only reader.
        statusTask = Task { [weak self] in
            for await (name, status) in stream {
                guard let self else { return }
                self.statusChanged(name, status)
            }
        }
    }

    private func statusChanged(_ name: String, _ status: ServiceStatus) {
        let old = statuses[name]
        statuses[name] = status
        if !serviceOrder.contains(name) { serviceOrder.append(name) }
        if old != status { log("service \(name): \(describe(status))") }
        if phase == .ready { updateHomeStatus() }
    }

    private func bootstrap() async {
        phase = .starting
        startingTitle = "Starting Midgard…"
        if !reaped {
            let found = await Supervisor.reapOrphans(binDir: bin)
            reaped = true
            if found.isEmpty {
                log("no orphaned services")
            } else {
                for p in found { log("ended orphaned service pid \(p.pid): \(p.executable)") }
            }
        }

        if !prepared {
            startingStep = "Preparing the demo data…"
            let starting = startingState, home = paths.home
            let started = Date()
            do {
                try await Task.detached { try Home.prepare(starting: starting, home: home) }.value
                prepared = true
                log("working copy ready at \(home.path) (\(seconds(since: started)))")
            } catch {
                log("working copy failed: \(error)")
                return fail("The demo data could not be prepared", Self.explain(error))
            }
        }

        do {
            users = try loadUsers()
            if currentUser == nil || !users.contains(where: { $0.username == currentUser?.username }) {
                currentUser = users.first
            }
        } catch {
            log("demo users: \(error)")
            return fail("Midgard is incomplete", "The list of demo users could not be read (\(error.localizedDescription)).")
        }

        if web.rulesInstalled == false {
            do {
                try await web.installOfflineRules()
                log("offline content rules compiled")
            } catch {
                log("content rules failed: \(error)")
                return fail("Midgard could not start", "The offline protection for the window could not be set up (\(error.localizedDescription)).")
            }
        }

        if server == nil {
            startingStep = "Starting the web server…"
            do {
                let (server, port) = try await startWebServer()
                self.server = server
                origin = "http://127.0.0.1:\(port)"
                log("web server on \(origin)")
            } catch {
                log("web server failed: \(error)")
                return fail("Midgard could not start", "Its built-in web server could not start (\(error)).")
            }
        }
        await startServices()
    }

    /// The previous launch's port first, so the consoles' saved preferences (stored per
    /// origin) survive a relaunch; any free port if that one is taken.
    private func startWebServer() async throws -> (WebServer, UInt16) {
        let memory = paths.home.deletingLastPathComponent()
            .appendingPathComponent("\(paths.home.lastPathComponent).webport")
        let remembered = (try? String(contentsOf: memory, encoding: .utf8))
            .flatMap { UInt16($0.trimmingCharacters(in: .whitespacesAndNewlines)) }
        let root = resources
        let result: (WebServer, UInt16) = try await Task.detached {
            if let remembered, remembered > 0 {
                let server = WebServer(root: root, port: remembered)
                if (try? server.start()) != nil { return (server, server.boundPort) }
            }
            let server = WebServer(root: root)
            try server.start()
            return (server, server.boundPort)
        }.value
        try? Data("\(result.1)\n".utf8).write(to: memory)
        return result
    }

    private func loadUsers() throws -> [DemoUser] {
        // The working copy has the same file; the bundled one is the fallback.
        let inHome = paths.home.appendingPathComponent("demo-users.json")
        let file = FileManager.default.fileExists(atPath: inHome.path)
            ? inHome : startingState.appendingPathComponent("demo-users.json")
        let users = try DemoUsers.load(from: file)
        guard !users.isEmpty else { throw CocoaError(.fileReadCorruptFile) }
        return users
    }

    // MARK: Services

    /// Everything `SuiteSpecs.make` needs besides the ports.
    private struct Launch: Sendable {
        var bin: URL, models: URL, home: URL, logs: URL, origin: String, recording: URL?, llama: URL

        func specs(_ ports: SuitePorts) -> [ServiceSpec] {
            SuiteSpecs.make(bin: bin, models: models, home: home, logs: logs, ports: ports,
                            webOrigin: origin, recording: recording)
                .map { spec in
                    var spec = spec
                    if spec.name == "llama" { spec.executable = llama }
                    return spec
                }
        }
    }

    private func llamaServer() -> URL? {
        let fm = FileManager.default
        let candidates = [bin.appendingPathComponent("llama-server"),
                          URL(fileURLWithPath: "/opt/homebrew/bin/llama-server"),
                          URL(fileURLWithPath: "/usr/local/bin/llama-server")]
        return candidates.first { fm.isExecutableFile(atPath: $0.path) }
    }

    private func recording() -> URL? {
        let dir = resources.appendingPathComponent("recording", isDirectory: true)
        let meta = dir.appendingPathComponent("meta.json")
        return FileManager.default.fileExists(atPath: meta.path) ? dir : nil
    }

    private func startServices() async {
        phase = .starting
        startingStep = nil
        statuses = [:]
        guard let llama = llamaServer() else {
            log("llama-server not found in \(bin.path) or Homebrew")
            return fail("Midgard is incomplete",
                        "The model server (llama-server) is missing from this copy of Midgard.")
        }
        let launch = Launch(bin: bin, models: resources.appendingPathComponent("models", isDirectory: true),
                            home: paths.home, logs: paths.logs, origin: origin, recording: recording(),
                            llama: llama)
        replaying = launch.recording != nil
        serviceOrder = launch.specs(SuitePorts(gatewayHTTP: 0, gatewayMesh: 0, mimirMesh: 0, mimirObserver: 0,
                                               mimirHealth: 0, heimdallMesh: 0, heimdallHealth: 0, llama: 0))
            .map(\.name)
        log("starting services \(serviceOrder.joined(separator: ", ")) (llama-server \(llama.path)"
            + (launch.recording.map { ", replaying \($0.path)" } ?? ", no recording") + ")")
        let supervisor = supervisor
        let log = log
        let started = Date()
        do {
            ports = try await SuiteStart.run(
                choosePorts: {
                    let ports = try SuitePorts.free()
                    log("ports: gateway \(ports.gatewayHTTP), llama \(ports.llama), all \(ports.all)")
                    return ports
                },
                start: { ports in try await supervisor.start(launch.specs(ports)) },
                stopAll: { await supervisor.stopAll() },
                onRetry: { service, retry in log("\(service): port in use; retry \(retry) with fresh ports") })
        } catch is CancellationError {
            log("start cancelled")
            return
        } catch let error as SupervisorError {
            log("start failed: \(error)")
            await supervisor.stopAll()
            switch error {
            case .failed(let service, let reason):
                return fail("Midgard could not start \(ServiceNames.friendly(service))", reason)
            case .portInUse(let service):
                return fail("Midgard could not start \(ServiceNames.friendly(service))",
                            "Its network port was taken by another program several times in a row.")
            case .stopped:
                return
            }
        } catch {
            log("start failed: \(error)")
            await supervisor.stopAll()
            return fail("Midgard could not start", "\(error)")
        }
        log("all services ready (\(seconds(since: started)))")

        guard let user = currentUser else { return }
        startingStep = "Signing in…"
        do {
            try await signIn(as: user)
        } catch {
            log("sign-in as \(user.username) failed: \(error)")
            await supervisor.stopAll()
            return fail("Midgard could not sign in", "The gateway refused the demo sign-in (\(error)).")
        }
        startingStep = nil
        phase = .ready
        log("ready")
        web.load(Destination.home.url(origin: origin))
        section = .home
    }

    private var gatewayAPI: String? { ports.map { "http://127.0.0.1:\($0.gatewayHTTP)/api/v1" } }
    private var gatewayOrigin: String? { ports.map { "http://127.0.0.1:\($0.gatewayHTTP)" } }

    /// Signs in and installs the session for every later page load.
    private func signIn(as user: DemoUser) async throws {
        guard let api = gatewayAPI, let url = URL(string: api) else { throw SessionError.transport("no gateway") }
        let info = try await Session.login(gateway: url, user: user)
        session = info
        currentUser = user
        log("signed in as \(info.subject) (clearance \(info.clearance ?? "none"), expires \(info.expiresAt))")
        installScripts(info)
        scheduleRefresh(Session.refreshDate(for: info))
    }

    private func injection(_ info: SessionInfo) -> String {
        Session.injectionScript(origin: origin, gatewayURL: gatewayAPI ?? "",
                                midgardConsoleURL: "\(origin)/midgard/index.html#/", session: info)
    }

    private func installScripts(_ info: SessionInfo) {
        let view = HeimdallView.preferred(appleSilicon: HeimdallView.isAppleSilicon())
        web.configure(scripts: [injection(info), HeimdallView.script(origin: origin, view: view)]) { [origin, gatewayOrigin] url in
            Navigation.isAllowed(url, origin: origin, gatewayOrigin: gatewayOrigin)
        }
    }

    private func scheduleRefresh(_ date: Date) {
        refreshTask?.cancel()
        log("next sign-in refresh at \(date)")
        refreshTask = Task { [weak self] in
            let delay = max(0, date.timeIntervalSinceNow)
            try? await Task.sleep(nanoseconds: UInt64(delay * 1e9))
            guard !Task.isCancelled else { return }
            await self?.refreshSession()
        }
    }

    private func refreshSession() async {
        guard phase == .ready, let user = currentUser,
              let api = gatewayAPI, let url = URL(string: api) else { return }
        do {
            let info = try await Session.login(gateway: url, user: user)
            session = info
            installScripts(info)
            // The open page keeps its token until it reloads: give it the new one now.
            web.evaluate(injection(info))
            log("session refreshed for \(info.subject), expires \(info.expiresAt)")
            scheduleRefresh(Session.refreshDate(for: info))
        } catch {
            log("session refresh failed: \(error); trying again in 60 s")
            scheduleRefresh(Date().addingTimeInterval(60))
        }
    }

    // MARK: Presenter actions

    func go(_ section: Destination) {
        log("show \(section.rawValue)")
        self.section = section
        guard phase == .ready else { return }
        web.load(section.url(origin: origin))
    }

    func switchUser(_ user: DemoUser) {
        guard phase == .ready, !switchingUser, user != currentUser else { return }
        switchingUser = true
        Task {
            defer { self.switchingUser = false }
            do {
                try await self.signIn(as: user)
                self.web.reload()
            } catch {
                self.log("switch to \(user.username) failed: \(error)")
                self.note = "Could not sign in as \(user.label). Still signed in as \(self.currentUser?.label ?? "before")."
            }
        }
    }

    func resetDemo() {
        guard phase == .ready || { if case .failed = phase { return true } else { return false } }() else { return }
        log("reset demo")
        refreshTask?.cancel()
        startTask?.cancel()
        phase = .starting
        section = .home
        startingTitle = "Resetting the demo…"
        startingStep = "Stopping the services…"
        web.load(URL(string: "about:blank")!)
        startTask = Task {
            await self.supervisor.stopAll()
            self.startingStep = "Restoring the starting data…"
            let starting = self.startingState, home = self.paths.home
            let started = Date()
            do {
                let report = try await Task.detached { try Home.reset(starting: starting, home: home) }.value
                self.log("working copy reset (\(self.seconds(since: started)))")
                if let leftover = report.leftover {
                    self.log("reset left \(leftover.path): \(report.reason ?? "")")
                    self.note = "The demo was reset, but the previous demo data could not be removed. "
                        + "It is at \(leftover.path) and can be deleted by hand."
                }
            } catch {
                self.log("reset failed: \(error)")
                self.prepared = false
                return self.fail("The demo could not be reset", Self.explain(error))
            }
            self.startingTitle = "Starting Midgard…"
            await self.startServices()
        }
    }

    // MARK: Quitting

    /// Stops every service and the web server. Safe to call more than once.
    func shutdown() async {
        guard !shuttingDown else { return }
        shuttingDown = true
        log("quit: stopping services")
        startTask?.cancel()
        refreshTask?.cancel()
        let started = Date()
        await supervisor.stopAll()
        if let server {
            await Task.detached { server.stop() }.value
        }
        server = nil
        instanceLock?.release()
        log("quit: stopped (\(seconds(since: started)))")
    }

    // MARK: Page status

    private func pageLoaded(_ url: URL?) {
        guard let url, Navigation.origin(of: url) == origin else { return }
        section = Destination(path: url.path)
        if section == .home { updateHomeStatus() }
        if selfTest && !selfTestRan { Task { await self.runSelfTest() } }
    }

    /// The home page's status line.
    var statusLine: String {
        let names = serviceOrder
        if let bad = names.first(where: { if case .failed = statuses[$0] { true } else { false } }) {
            return "\(ServiceNames.friendly(bad)) has stopped"
        }
        if let restarting = names.first(where: { if case .restarting = statuses[$0] { true } else { false } }) {
            return "\(ServiceNames.friendly(restarting)) is restarting…"
        }
        guard names.allSatisfy({ statuses[$0] == .ready }) else { return "Starting…" }
        return "All services running" + (replaying ? " · replaying recorded hour" : "")
    }

    private func updateHomeStatus() {
        guard section == .home else { return }
        web.evaluate(PageScript.call("midgardStatus", statusLine))
    }

    // MARK: Development self-test (MIDGARD_APP_SELFTEST=1)

    private var selfTestRan = false

    /// For dev runs checked from the log: whether the offline rules block an external
    /// fetch, the session and Heimdall's view reached the page, Switch user and (with
    /// MIDGARD_APP_SELFTEST_RESET=1) Reset demo; window snapshots go to
    /// MIDGARD_APP_SNAPSHOTS if set.
    private func runSelfTest() async {
        guard selfTest, !selfTestRan else { return }
        selfTestRan = true
        func pause(_ s: Double) async { try? await Task.sleep(nanoseconds: UInt64(s * 1e9)) }
        await pause(4)
        await probe("home"); await snapshot("home")
        go(.heimdall)
        await pause(40)
        await probe("heimdall"); await snapshot("heimdall")
        if let other = users.first(where: { $0 != currentUser }) {
            switchUser(other)
            await pause(10)
            await probe("heimdall as \(other.username)"); await snapshot("heimdall-\(other.username)")
        }
        guard ProcessInfo.processInfo.environment["MIDGARD_APP_SELFTEST_RESET"] == "1" else { return }
        resetDemo()
        let deadline = Date().addingTimeInterval(300)
        await pause(2)
        while phase != .ready, Date() < deadline { await pause(0.5) }
        await pause(4)
        await probe("after reset"); await snapshot("after-reset")
    }

    private func probe(_ label: String) async {
        let body = """
            const out = { page: location.pathname + location.hash };
            // no-cors: without the content rules an external fetch would succeed (opaque).
            const targets = [['external', 'https://example.com/'], ['externalHttp', 'http://example.com/'],
                             ['own', location.origin + '/']];
            for (const [name, target] of targets) {
              try { const r = await fetch(target, { cache: 'no-store', mode: 'no-cors' }); out[name] = 'loaded ' + r.type + ' ' + r.status; }
              catch (e) { out[name] = 'blocked (' + e.message + ')'; }
            }
            out.image = await new Promise(done => {
              const img = new Image();
              img.onload = () => done('loaded'); img.onerror = () => done('blocked');
              img.src = 'https://www.google.com/favicon.ico?' + Date.now();
            });
            out.token = !!sessionStorage.getItem('midgard.session.token');
            out.who = sessionStorage.getItem('midgard.session.who');
            out.gateway = window.__MIDGARD_GATEWAY__ || null;
            out.heimdallView = localStorage.getItem('heimdall.cop.view');
            const status = document.getElementById('status');
            if (status) out.status = status.textContent;
            return JSON.stringify(out);
            """
        let result: String = await withCheckedContinuation { done in
            web.callAsync(body) { done.resume(returning: $0) }
        }
        log("self-test \(label): \(result)")
    }

    private func snapshot(_ name: String) async {
        guard let dir = ProcessInfo.processInfo.environment["MIDGARD_APP_SNAPSHOTS"], !dir.isEmpty else { return }
        let file = URL(fileURLWithPath: dir).appendingPathComponent("\(name).png")
        log("snapshot \(name): \(await WindowSnapshot.save(webView: web.webView, to: file))")
    }

    /// Snapshots of the starting and failure screens (self-test only).
    private func snapshotLater(_ name: String, after seconds: Double) {
        guard selfTest else { return }
        Task {
            try? await Task.sleep(nanoseconds: UInt64(seconds * 1e9))
            await self.snapshot(name)
        }
    }

    // MARK: Helpers

    private func fail(_ title: String, _ details: String) {
        startingStep = nil
        phase = .failed(StartFailure(title: title, details: details))
        snapshotLater("failure", after: 1)
    }

    private func seconds(since date: Date) -> String {
        String(format: "%.1f s", Date().timeIntervalSince(date))
    }

    private func describe(_ status: ServiceStatus) -> String {
        switch status {
        case .starting: return "starting"
        case .ready: return "ready"
        case .restarting(let attempt, let reason):
            return "restarting (attempt \(attempt))" + (reason.map { ": \($0.prefix { $0 != "\n" })" } ?? "")
        case .failed(let reason): return "failed: \(reason.prefix { $0 != "\n" })"
        case .stopped: return "stopped"
        }
    }

    /// A working-copy error in words a presenter can act on.
    static func explain(_ error: Error) -> String {
        guard let error = error as? HomeError else {
            return "Something went wrong while copying the demo data: \(error.localizedDescription)"
        }
        switch error {
        case .notAWorkingCopy(let url):
            return "The folder \(url.path) already holds other files, so Midgard will not put its demo data "
                + "there. Move that folder somewhere else, then try again."
        case .startingStateMissing, .symlinkInStartingState:
            return "This copy of Midgard is incomplete: its starting demo data is missing or damaged. "
                + "Install Midgard again."
        case .insufficientSpace(let needed, let available):
            let f = ByteCountFormatter()
            return "There is not enough free disk space for the demo data. It needs "
                + "\(f.string(fromByteCount: needed)), and \(f.string(fromByteCount: available)) is free. "
                + "Free some space, then try again."
        case .resetFailed(_, let previous):
            return "The demo could not be reset. The previous demo data was kept at \(previous.path)."
        }
    }
}
