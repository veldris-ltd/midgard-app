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
    /// False when trying again cannot help (an unsupported Mac, a damaged install).
    var canRetry = true
    /// True when a working copy exists, so Reset demo is offered too.
    var canReset = false
}

/// Ties the core together: system check, working copy, web server, services, sign-in,
/// the web view's session, Reset demo, Switch user and quitting.
///
/// Starting, "Try again" and Reset demo each run as one task tagged with a generation
/// number; a newer one cancels and waits for the older, and an older one never changes
/// what the window shows.
@MainActor
final class AppModel: ObservableObject {
    enum Phase: Equatable {
        case starting
        case failed(StartFailure)
        case ready
        /// Quitting: services are being stopped.
        case stopping
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
    /// A non-blocking note for the presenter (a service that stopped, a reset leftover …).
    @Published var note: String? {
        didSet { if note != nil, note != oldValue { noteCount += 1; snapshotLater("note-\(noteCount)", after: 1) } }
    }
    private var noteCount = 0
    @Published private(set) var switchingUser = false
    /// The Reset demo confirmation is showing.
    @Published var confirmingReset = false

    let paths: Paths
    let log: ShellLog
    let web = WebController()
    private let supervisor = Supervisor()
    private var server: WebServer?
    private(set) var origin = ""
    private var ports: SuitePorts?
    private var session: SessionInfo?
    /// When the session is to be renewed, by the wall clock.
    private var refreshAt: Date?
    private var replaying = false
    private var prepared = false
    private var reaped = false
    private var generation = 0
    private var startTask: Task<Void, Never>?
    private var refreshTask: Task<Void, Never>?
    private var statusTask: Task<Void, Never>?
    private var instanceLock: InstanceLock?
    private(set) var shuttingDown = false
    private var crashBudget = ReloadBudget()
    /// The next Heimdall load is forced to 2D (its 3D page crashed).
    private var forceHeimdall2D = false
    private var wakeObserver: NSObjectProtocol?
    private let selfTest: Bool = {
        #if DEBUG
        let debug = true
        #else
        let debug = false
        #endif
        return ProcessInfo.processInfo.environment["MIDGARD_APP_SELFTEST"] == "1"
            && (debug || Bundle.main.bundleIdentifier == nil)
    }()

    init(paths: Paths = .current()) {
        self.paths = paths
        log = ShellLog(file: paths.logs.appendingPathComponent("shell.log"))
        web.onPageLoaded = { [weak self] url in self?.pageLoaded(url) }
        web.onContentCrash = { [weak self] url in self?.contentCrashed(url) }
        web.onLoadFailed = { [weak self] url, error in self?.loadFailed(url, error) }
        wakeObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didWakeNotification, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.didWake() }
        }
    }

    private var resources: URL { paths.resources }
    private var bin: URL { paths.helpers }
    private var startingState: URL { resources.appendingPathComponent("starting-state", isDirectory: true) }
    /// In the model server's arguments (it runs from outside the helpers, e.g. Homebrew's).
    private var modelsMarker: String { resources.appendingPathComponent("models", isDirectory: true).path + "/" }

    // MARK: Launch

    /// Before any window: true if this is the only shell, else the other one has been
    /// brought forward and this one should exit. A shell that is quitting is waited for.
    func claimSingleInstance() -> Bool {
        let file = paths.home.deletingLastPathComponent()
            .appendingPathComponent("\(paths.home.lastPathComponent).instance")
        switch InstanceLock.acquire(file) {
        case .acquired(let lock):
            instanceLock = lock
        case .heldBy(let pid, let stopping):
            let who = "pid \(pid.map(String.init) ?? "?")"
            if stopping {
                log("another Midgard (\(who)) is quitting; waiting for it")
                if case .acquired(let lock) = InstanceLock.acquire(file, waitingUpTo: 30) {
                    instanceLock = lock
                    break
                }
            }
            log("another Midgard holds \(file.path) (\(who)); activating it and quitting")
            if let pid { NSRunningApplication(processIdentifier: pid)?.activate(options: [.activateAllWindows]) }
            return false
        case .failed(let reason):
            // Better to run than to refuse: the bundle check below still guards the app.
            log("single-instance lock unavailable: \(reason)")
        }
        // A bundled instance without the lock has released it at the end of quitting (or
        // uses another working copy): give it a moment to finish.
        if let id = Bundle.main.bundleIdentifier {
            let me = NSRunningApplication.current.processIdentifier
            let others = NSRunningApplication.runningApplications(withBundleIdentifier: id)
                .filter { $0.processIdentifier != me }
            for other in others {
                let pid = other.processIdentifier
                let deadline = Date().addingTimeInterval(5)
                while kill(pid, 0) == 0, Date() < deadline { usleep(100_000) }
                if kill(pid, 0) == 0 {
                    log("another Midgard is running (pid \(pid)); activating it and quitting")
                    other.activate(options: [.activateAllWindows])
                    instanceLock?.release()
                    return false
                }
            }
        }
        return true
    }

    func launch() {
        log("launch: resources \(resources.path), helpers \(bin.path), home \(paths.home.path), logs \(paths.logs.path)")
        if let problem = SystemCheck.problemOnThisMac() {
            log("system check: \(problem)")
            phase = .failed(StartFailure(title: "Midgard cannot run on this Mac", details: problem, canRetry: false))
            return
        }
        listenToStatuses()
        run { gen in await self.bootstrap(gen) }
        snapshotLater("starting", after: 5)
    }

    /// The starting screen's "Try again".
    func retry() {
        guard !shuttingDown, case .failed(let failure) = phase, failure.canRetry else { return }
        log("try again")
        showStarting("Starting Midgard…", step: "Stopping the services…")
        run { gen in
            await self.supervisor.stopAll()
            guard self.isCurrent(gen) else { return self.interrupted(gen) }
            await self.bootstrap(gen)
        }
    }

    /// Runs `work` as the one start or reset in flight, after cancelling and waiting for
    /// the previous one.
    private func run(_ work: @escaping @MainActor (Int) async -> Void) {
        let previous = startTask
        previous?.cancel()
        generation += 1
        let gen = generation
        startTask = Task {
            await previous?.value
            guard self.isCurrent(gen) else { return }
            await work(gen)
        }
    }

    /// Whether the task of generation `gen` may still change what the window shows.
    private func isCurrent(_ gen: Int) -> Bool {
        gen == generation && !shuttingDown && !Task.isCancelled
    }

    /// A start that stopped part-way: unless something newer took over or Midgard is
    /// quitting, say so rather than leave "Starting…" on screen.
    private func interrupted(_ gen: Int) {
        guard gen == generation, !shuttingDown else { return }
        log("start interrupted")
        fail("Midgard stopped starting", "Starting was interrupted before it finished.")
    }

    private func showStarting(_ title: String, step: String?) {
        phase = .starting
        startingTitle = title
        startingStep = step
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
        guard phase == .ready else { return }
        updateHomeStatus()
        if let message = ServiceAlert.message(service: name, status: status), note != message {
            log("note: \(message)")
            note = message
        } else if status == .ready, let note,
                  note == ServiceAlert.message(service: name, status: .failed("")) {
            // It came back: the warning no longer holds.
            log("note cleared: \(ServiceNames.friendly(name)) is running again")
            self.note = nil
        }
    }

    private func bootstrap(_ gen: Int) async {
        showStarting("Starting Midgard…", step: nil)
        if !reaped {
            let found = await Supervisor.reapOrphans(binDir: bin, argumentContaining: modelsMarker)
            reaped = true
            if found.isEmpty {
                log("no orphaned services")
            } else {
                for p in found { log("ended orphaned service pid \(p.pid): \(p.executable)") }
            }
            guard isCurrent(gen) else { return interrupted(gen) }
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
                guard gen == generation, !shuttingDown else { return }
                return fail("The demo data could not be prepared", Self.explain(error))
            }
            guard isCurrent(gen) else { return interrupted(gen) }
        }

        do {
            users = try loadUsers()
            if currentUser == nil || !users.contains(where: { $0.username == currentUser?.username }) {
                currentUser = users.first
            }
        } catch {
            log("demo users: \(error)")
            return fail("Midgard is incomplete",
                        "The list of demo users could not be read. Install Midgard again.", canRetry: false)
        }

        if web.rulesInstalled == false {
            do {
                try await web.installOfflineRules()
                log("offline content rules compiled")
            } catch {
                log("content rules failed: \(error)")
                guard gen == generation, !shuttingDown else { return }
                return fail("Midgard could not start",
                            "The offline protection for the window could not be set up (\(error.localizedDescription)).")
            }
            guard isCurrent(gen) else { return interrupted(gen) }
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
                guard gen == generation, !shuttingDown else { return }
                return fail("Midgard could not start", "Its built-in web server could not start (\(error)).")
            }
            guard isCurrent(gen) else { return interrupted(gen) }
        }
        await startServices(gen)
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

    private func startServices(_ gen: Int) async {
        showStarting(startingTitle, step: nil)
        statuses = [:]
        guard let llama = llamaServer() else {
            log("llama-server not found in \(bin.path) or Homebrew")
            return fail("Midgard is incomplete",
                        "The model server (llama-server) is missing from this copy of Midgard. Install Midgard again.",
                        canRetry: false)
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
            return interrupted(gen)
        } catch SupervisorError.stopped {
            log("start stopped")
            return interrupted(gen)
        } catch {
            log("start failed: \(error)")
            await supervisor.stopAll()
            guard gen == generation, !shuttingDown else { return }
            switch error as? SupervisorError {
            case .failed(let service, let reason):
                return fail("Midgard could not start \(ServiceNames.friendly(service))", reason)
            case .portInUse(let service):
                return fail("Midgard could not start \(ServiceNames.friendly(service))",
                            "Its network port was taken by another program several times in a row.")
            default:
                return fail("Midgard could not start", "\(error)")
            }
        }
        log("all services ready (\(seconds(since: started)))")
        guard isCurrent(gen) else { return interrupted(gen) }

        guard let user = currentUser else { return }
        startingStep = "Signing in…"
        do {
            let info = try await login(user)
            guard isCurrent(gen) else { return interrupted(gen) }
            adopt(info, for: user)
        } catch {
            log("sign-in as \(user.username) failed: \(error)")
            await supervisor.stopAll()
            guard gen == generation, !shuttingDown else { return }
            return fail("Midgard could not sign in", "The gateway refused the demo sign-in (\(error)).")
        }
        startingStep = nil
        phase = .ready
        log("ready")
        section = .home
        web.load(Destination.home.url(origin: origin))
    }

    // MARK: Session

    private var gatewayAPI: String? { ports.map { "http://127.0.0.1:\($0.gatewayHTTP)/api/v1" } }
    private var gatewayOrigin: String? { ports.map { "http://127.0.0.1:\($0.gatewayHTTP)" } }

    private func login(_ user: DemoUser) async throws -> SessionInfo {
        guard let api = gatewayAPI, let url = URL(string: api) else { throw SessionError.transport("no gateway") }
        return try await Session.login(gateway: url, user: user)
    }

    /// Makes `info` the session for every later page load and schedules its renewal.
    private func adopt(_ info: SessionInfo, for user: DemoUser) {
        session = info
        currentUser = user
        log("signed in as \(info.subject) (clearance \(info.clearance ?? "none"), expires \(info.expiresAt))")
        installScripts()
        scheduleRefresh(Session.refreshDate(for: info))
    }

    private func injection(_ info: SessionInfo) -> String {
        Session.injectionScript(origin: origin, gatewayURL: gatewayAPI ?? "",
                                midgardConsoleURL: "\(origin)/midgard/index.html#/", session: info)
    }

    private func installScripts() {
        guard let session else { return }
        let view = forceHeimdall2D ? "2d" : HeimdallView.preferred(appleSilicon: HeimdallView.isAppleSilicon())
        web.configure(scripts: [injection(session),
                                HeimdallView.script(origin: origin, view: view, force: forceHeimdall2D)]) {
            [origin, gatewayOrigin] url in
            Navigation.isAllowed(url, origin: origin, gatewayOrigin: gatewayOrigin)
        }
    }

    /// Renews the session at `date` by the wall clock: checked at least once a minute, so
    /// time asleep counts (and on wake, see `didWake`).
    private func scheduleRefresh(_ date: Date) {
        refreshTask?.cancel()
        refreshAt = date
        log("next sign-in refresh at \(date)")
        refreshTask = Task { [weak self] in
            while !Task.isCancelled {
                let remaining = date.timeIntervalSinceNow
                if remaining <= 0 {
                    await self?.refreshSession()
                    return
                }
                try? await Task.sleep(nanoseconds: UInt64(min(remaining, 60) * 1e9))
            }
        }
    }

    private func didWake() {
        guard phase == .ready, let session, let refreshAt else { return }
        let action = SessionTiming.action(now: Date(), refreshAt: refreshAt, expiresAt: session.expiresAt)
        log("woke from sleep: session \(action)")
        guard action != .wait else { return }
        refreshTask?.cancel()
        refreshTask = Task { [weak self] in await self?.refreshSession() }
    }

    private func refreshSession() async {
        guard !shuttingDown, phase == .ready, let user = currentUser, let old = session else { return }
        let action = SessionTiming.action(now: Date(), refreshAt: refreshAt ?? .distantPast, expiresAt: old.expiresAt)
        do {
            let info = try await login(user)
            guard !Task.isCancelled, !shuttingDown, phase == .ready, currentUser == user else { return }
            session = info
            installScripts()
            if action == .refreshAndReload {
                // The page's token has expired: it must load again with the new one.
                web.reload()
            } else {
                // The open page keeps its token until it reloads: give it the new one now.
                web.evaluate(injection(info))
            }
            log("session refreshed for \(info.subject), expires \(info.expiresAt)"
                + (action == .refreshAndReload ? "; page reloaded" : ""))
            scheduleRefresh(Session.refreshDate(for: info))
        } catch {
            guard !Task.isCancelled, !shuttingDown, phase == .ready, currentUser == user else { return }
            log("session refresh failed: \(error); trying again in 60 s")
            scheduleRefresh(Date().addingTimeInterval(60))
        }
    }

    // MARK: Presenter actions

    func go(_ section: Destination) {
        guard !shuttingDown, phase == .ready else { return }
        log("show \(section.rawValue)")
        self.section = section
        web.load(section.url(origin: origin))
    }

    func switchUser(_ user: DemoUser) {
        guard !shuttingDown, phase == .ready, !switchingUser, user != currentUser else { return }
        switchingUser = true
        refreshTask?.cancel()
        Task {
            defer { self.switchingUser = false }
            do {
                let info = try await self.login(user)
                guard !self.shuttingDown, self.phase == .ready else { return }
                self.adopt(info, for: user)
                self.web.reload()
            } catch {
                guard !self.shuttingDown, self.phase == .ready else { return }
                self.log("switch to \(user.username) failed: \(error)")
                self.note = "Could not sign in as \(user.label). Still signed in as \(self.currentUser?.label ?? "before")."
                if let session = self.session { self.scheduleRefresh(Session.refreshDate(for: session)) }
            }
        }
    }

    /// Whether Reset demo may be offered now.
    var canReset: Bool {
        guard !shuttingDown, !switchingUser else { return false }
        switch phase {
        case .ready: return true
        case .failed(let failure): return failure.canReset
        default: return false
        }
    }

    func resetDemo() {
        guard canReset else { return }
        log("reset demo")
        refreshTask?.cancel()
        note = nil
        showStarting("Resetting the demo…", step: "Stopping the services…")
        section = .home
        web.load(URL(string: "about:blank")!)
        run { gen in
            await self.supervisor.stopAll()
            guard self.isCurrent(gen) else { return self.interrupted(gen) }
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
                guard gen == self.generation, !self.shuttingDown else { return }
                return self.fail("The demo could not be reset", Self.explain(error))
            }
            // The consoles' stored state belongs to the old data too; Heimdall's view
            // default is seeded again by its script.
            await self.web.clearWebsiteData()
            self.log("web view data cleared")
            guard self.isCurrent(gen) else { return self.interrupted(gen) }
            self.startingTitle = "Starting Midgard…"
            await self.bootstrap(gen)
        }
    }

    // MARK: Quitting

    /// Called synchronously when the app is asked to quit: shows "Stopping Midgard…" and
    /// stops every other action from starting.
    func beginShutdown() {
        guard !shuttingDown else { return }
        shuttingDown = true
        phase = .stopping
        startingTitle = "Stopping Midgard…"
        startingStep = nil
        confirmingReset = false
        instanceLock?.markStopping()
        log("quit: stopping services")
        snapshotLater("stopping", after: 0.05)
    }

    /// Stops every service and the web server; no start can run afterwards.
    func shutdown() async {
        beginShutdown()
        let started = Date()
        let start = startTask
        start?.cancel()
        refreshTask?.cancel()
        await start?.value
        await supervisor.close()
        // A last sweep: anything still running from the helpers, or a model server given
        // this bundle's models, should not outlive the app. Normally it finds nothing.
        let leftovers = await Supervisor.reapOrphans(binDir: bin, argumentContaining: modelsMarker, grace: 5)
        if leftovers.isEmpty {
            log("quit: final sweep found nothing running")
        } else {
            for p in leftovers { log("quit: final sweep ended pid \(p.pid): \(p.executable)") }
        }
        if let server {
            await Task.detached { server.stop() }.value
        }
        server = nil
        log("quit: stopped (\(seconds(since: started)))")
        instanceLock?.release()
    }

    // MARK: Web view events

    private func pageLoaded(_ url: URL?) {
        guard let url, Navigation.origin(of: url) == origin else { return }
        section = Destination(path: url.path)
        if forceHeimdall2D {
            forceHeimdall2D = false
            installScripts()
        }
        if section == .home { updateHomeStatus() }
        if selfTest && !selfTestRan { Task { await self.runSelfTest() } }
    }

    /// The page's web content process ended (a crash, or memory pressure on an 8 GB Mac).
    private func contentCrashed(_ url: URL?) {
        guard phase == .ready, !shuttingDown else { return }
        let target = url.flatMap { Navigation.origin(of: $0) == origin ? $0 : nil } ?? section.url(origin: origin)
        let page = Destination(path: target.path)
        log("web content process ended on \(target.path)")
        guard crashBudget.take() else {
            log("page crashed again; going home")
            note = "The \(page.title) page stopped working, so Midgard went back to the home screen."
            section = .home
            web.load(Destination.home.url(origin: origin))
            return
        }
        if page == .heimdall {
            // The 3D globe is the heaviest page: reopen Heimdall in 2D.
            forceHeimdall2D = true
            installScripts()
            log("reopening Heimdall in 2D")
        }
        web.load(target)
    }

    private func loadFailed(_ url: URL?, _ error: Error) {
        let ns = error as NSError
        // Cancelled loads (a newer navigation, our own policy refusals) are not failures.
        if ns.domain == NSURLErrorDomain && ns.code == NSURLErrorCancelled { return }
        if ns.domain == "WebKitErrorDomain" && ns.code == 102 { return }
        log("page load failed for \(url?.absoluteString ?? "?"): \(ns.domain) \(ns.code) \(ns.localizedDescription)")
        guard phase == .ready, !shuttingDown else { return }
        note = "The page could not be opened (\(ns.localizedDescription))."
    }

    // MARK: Page status

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

    // MARK: Development self-test (MIDGARD_APP_SELFTEST=1, debug or unbundled builds only)

    private var selfTestRan = false

    /// For dev runs checked from the log: whether the offline rules block external loads,
    /// the session and Heimdall's view reached the page, Switch user and (with
    /// MIDGARD_APP_SELFTEST_RESET=1) Reset demo; window snapshots go to
    /// MIDGARD_APP_SNAPSHOTS if set. MIDGARD_APP_SELFTEST_CRASH=1 also ends Heimdall's
    /// web content process to exercise the crash recovery.
    private func runSelfTest() async {
        guard selfTest, !selfTestRan else { return }
        selfTestRan = true
        let env = ProcessInfo.processInfo.environment
        func pause(_ s: Double) async { try? await Task.sleep(nanoseconds: UInt64(s * 1e9)) }
        if let sequence = env["MIDGARD_APP_SELFTEST_SEQUENCE"], !sequence.isEmpty {
            // Steps in order: reset, switch, quit (as Cmd-Q), wait:<seconds>, probe, kill:<service>.
            for step in sequence.split(separator: ",").map(String.init) {
                log("self-test step \(step)")
                switch step {
                case "reset":
                    resetDemo()
                    await pause(2)
                    let deadline = Date().addingTimeInterval(300)
                    while phase != .ready, Date() < deadline { await pause(0.5) }
                case "switch":
                    if let other = users.first(where: { $0 != currentUser }) { switchUser(other) }
                    await pause(3)
                case "quit":
                    // As a menu Cmd-Q does: from the run loop, not from inside this task
                    // (terminate's wait would hold the main actor the shutdown needs).
                    NSApp.perform(#selector(NSApplication.terminate(_:)), with: nil, afterDelay: 0)
                    return
                case "probe":
                    await probe("sequence")
                case _ where step.hasPrefix("kill:"):
                    // kill:<service>: SIGKILL it, as a crash; the supervisor restarts it.
                    let name = String(step.dropFirst(5))
                    if let pid = supervisor.pid(of: name) { kill(pid, SIGKILL) }
                    await pause(1)
                    let deadline = Date().addingTimeInterval(60)
                    while statuses[name] != .ready, Date() < deadline { await pause(0.5) }
                default:
                    if step.hasPrefix("wait:"), let s = Double(step.dropFirst(5)) { await pause(s) }
                }
            }
            return
        }
        await pause(4)
        await probe("home"); await snapshot("home")
        go(.heimdall)
        await pause(40)
        await probe("heimdall"); await snapshot("heimdall")
        if env["MIDGARD_APP_SELFTEST_CRASH"] == "1" {
            for round in 1...3 {
                let pid = web.contentProcessIdentifier
                log("self-test: ending web content process \(pid.map(String.init) ?? "?") (round \(round))")
                if let pid { kill(pid, SIGKILL) }
                await pause(8)
                await probe("after crash \(round)")
            }
            await snapshot("after-crashes")
            go(.home)
            await pause(3)
        }
        if let other = users.first(where: { $0 != currentUser }) {
            switchUser(other)
            await pause(10)
            await probe("as \(other.username)"); await snapshot("as-\(other.username)")
        }
        guard env["MIDGARD_APP_SELFTEST_RESET"] == "1" else { return }
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
                             ['userinfo', 'http://127.0.0.1:1@example.com/'], ['own', location.origin + '/']];
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
            out.localStorageKeys = localStorage.length;
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

    private func fail(_ title: String, _ details: String, canRetry: Bool = true) {
        startingStep = nil
        phase = .failed(StartFailure(title: title, details: details, canRetry: canRetry,
                                     canReset: canRetry && prepared))
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
