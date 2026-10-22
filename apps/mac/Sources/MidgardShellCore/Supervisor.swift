import Darwin
import Foundation

/// What the starting screen shows for one service.
public enum ServiceStatus: Sendable, Equatable {
    case starting
    case ready
    /// Exited after it was ready; launched again after a back-off (attempt counts from 1).
    /// `reason`: how it exited, or for a later attempt why the last relaunch failed.
    case restarting(attempt: Int, reason: String?)
    /// "port in use" when it could not bind, else why, with its last log lines.
    case failed(String)
    case stopped
}

public enum SupervisorError: Error, Equatable, CustomStringConvertible {
    /// The service exited before it was ready because a port was taken: choose fresh
    /// ports, `stopAll()`, and start again.
    case portInUse(service: String)
    case failed(service: String, reason: String)
    /// `stopAll()` was called while starting.
    case stopped

    public var description: String {
        switch self {
        case .portInUse(let service): return "\(service): port in use"
        case .failed(let service, let reason): return "\(service): \(reason)"
        case .stopped: return "stopped"
        }
    }
}

/// Starts the suite's services in order, waits for each to be ready, restarts one that
/// exits after it was ready, and stops them all in reverse order, as
/// `tools/mac-app/run-native.sh` does. It does not sweep lock files: the services
/// reclaim a lock left by a dead macOS process themselves.
///
/// Call `stopAll()` before quitting. If the shell crashed instead, its services are left
/// running; `reapOrphans(binDir:)` ends them at the next launch.
public final class Supervisor: @unchecked Sendable {
    public struct Timing: Sendable {
        /// The first restart waits this long, each later one twice as long …
        public var backoffBase: TimeInterval
        /// … but never longer than this.
        public var backoffCap: TimeInterval
        /// A service up this long since it was last ready starts again from the first delay.
        public var stableAfter: TimeInterval
        /// How often a health URL is asked.
        public var healthInterval: TimeInterval
        /// How often the log is read for the ready line and the process checked.
        public var pollInterval: TimeInterval
        /// How long a stopped service has after SIGTERM before SIGKILL.
        public var stopGrace: TimeInterval

        public init(backoffBase: TimeInterval = 2, backoffCap: TimeInterval = 60,
                    stableAfter: TimeInterval = 300, healthInterval: TimeInterval = 0.5,
                    pollInterval: TimeInterval = 0.1, stopGrace: TimeInterval = 15) {
            self.backoffBase = backoffBase
            self.backoffCap = backoffCap
            self.stableAfter = stableAfter
            self.healthInterval = healthInterval
            self.pollInterval = pollInterval
            self.stopGrace = stopGrace
        }

        /// The delay before restart `attempt` (1, 2, …): 2, 4, 8 … s up to the cap.
        public func backoff(_ attempt: Int) -> TimeInterval {
            min(backoffCap, backoffBase * pow(2, Double(max(1, attempt) - 1)))
        }
    }

    static let tailLines = 20
    static let tailBytes = 64 * 1024
    /// A log longer than this at `start` is moved to `<name>.log.1` first.
    static let logLimit: UInt64 = 20 * 1024 * 1024

    let timing: Timing
    /// Every status change as (service name, status), in order. One reader only.
    public let statuses: AsyncStream<(String, ServiceStatus)>
    private let continuation: AsyncStream<(String, ServiceStatus)>.Continuation
    private let session: URLSession
    private let lock = NSLock()
    // Guarded by `lock`.
    private var services: [Service] = []
    private var latest: [String: ServiceStatus] = [:]
    private var generation = 0
    private var lastStop: Task<Void, Never>?

    public init(timing: Timing = Timing()) {
        self.timing = timing
        (statuses, continuation) = AsyncStream.makeStream(of: (String, ServiceStatus).self)
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        session = URLSession(configuration: configuration)
    }

    deinit {
        session.invalidateAndCancel()
        continuation.finish()
    }

    /// The last status published for the service called `name`.
    public func status(of name: String) -> ServiceStatus? {
        locked { latest[name] }
    }

    /// The pid of the service called `name` while its process runs.
    public func pid(of name: String) -> pid_t? {
        locked {
            guard let run = services.last(where: { $0.spec.name == name })?.run, !run.exited else { return nil }
            return run.pid
        }
    }

    /// Launches each spec in order and waits for it to be ready. On the first that fails,
    /// stops it, starts no more and throws (those already ready keep running; call
    /// `stopAll()`). A log over 20 MB is moved to `<name>.log.1` first. Cancelling the
    /// task stops the waiting (CancellationError); what was launched is left for
    /// `stopAll()`. May be called again after `stopAll()`.
    public func start(_ specs: [ServiceSpec]) async throws {
        let generation = locked { self.generation }
        for spec in specs {
            let service = Service(spec)
            try locked {
                guard generation == self.generation else { throw SupervisorError.stopped }
                services.append(service)
            }
            publish(service, .starting)
            Self.rotate(spec.logFile)
            do {
                try await launchAndWait(service)
            } catch SupervisorError.portInUse(let name) {
                publish(service, .failed("port in use"))
                throw SupervisorError.portInUse(service: name)
            } catch SupervisorError.failed(let name, let reason) {
                publish(service, .failed(reason))
                throw SupervisorError.failed(service: name, reason: reason)
            }
        }
    }

    /// Stops every service in reverse start order: SIGTERM (for `replay`, as run-native.sh's
    /// `kill_replay`: pause it, end its children, end it, resume it), SIGKILL after
    /// `stopGrace`. Nothing restarts afterwards. Safe to call again or concurrently: a
    /// later call returns once the earlier one has finished.
    public func stopAll() async {
        let task: Task<Void, Never> = locked {
            generation += 1
            let list = services
            services = []
            for service in list {
                service.stopped = true
                service.restart?.cancel()
            }
            let previous = lastStop
            let task = Task {
                await previous?.value
                await self.stop(list.reversed())
            }
            lastStop = task
            return task
        }
        await task.value
    }

    // MARK: Running one service

    /// One service from a start; guarded by the supervisor's lock.
    private final class Service: @unchecked Sendable {
        let spec: ServiceSpec
        var run: Run?
        /// The current run has been declared ready (and has not exited since).
        var ready = false
        var readyAt: Date?
        var attempt = 0
        var stopped = false
        var restart: Task<Void, Never>?
        init(_ spec: ServiceSpec) { self.spec = spec }
    }

    /// One launch of a service; guarded by the supervisor's lock.
    private final class Run: @unchecked Sendable {
        let process: Process
        /// Where the log ended at launch: only output after it counts.
        let logOffset: UInt64
        var pid: pid_t = 0
        var exited = false
        var exitDescription = ""
        init(_ process: Process, logOffset: UInt64) {
            self.process = process
            self.logOffset = logOffset
        }
    }

    private func launch(_ service: Service) throws -> Run {
        let spec = service.spec
        try? FileManager.default.createDirectory(at: spec.logFile.deletingLastPathComponent(),
                                                 withIntermediateDirectories: true)
        let fd = open(spec.logFile.path, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0o644)
        guard fd >= 0 else {
            throw SupervisorError.failed(service: spec.name, reason:
                "cannot open \(spec.logFile.path): \(String(cString: strerror(errno)))")
        }
        let output = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        defer { try? output.close() }
        let offset = UInt64(max(0, lseek(fd, 0, SEEK_END)))

        let process = Process()
        process.executableURL = spec.executable
        process.arguments = spec.arguments
        if let extra = spec.environment {
            process.environment = ProcessInfo.processInfo.environment.merging(extra) { $1 }
        }
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = output
        process.standardError = output
        let run = Run(process, logOffset: offset)
        process.terminationHandler = { [weak self] process in
            self?.exited(service, run, process)
        }
        // Under the lock, so stopAll either sees this run or this launch sees stopAll.
        try locked {
            guard !service.stopped else { throw SupervisorError.stopped }
            do {
                try process.run()
            } catch {
                throw SupervisorError.failed(service: spec.name, reason:
                    "cannot start \(spec.executable.path): \(error.localizedDescription)")
            }
            run.pid = process.processIdentifier
            service.run = run
            service.ready = false
        }
        return run
    }

    /// Launches the service and returns once it is ready (status `.ready` published).
    private func launchAndWait(_ service: Service) async throws {
        let spec = service.spec
        let run = try launch(service)
        let deadline = Date().addingTimeInterval(spec.readyTimeout)
        var scanner = LogScanner(file: spec.logFile, offset: run.logOffset)
        var nextHealth = Date()
        while true {
            if locked({ service.stopped }) { throw SupervisorError.stopped }  // stopAll ends it
            try Task.checkCancellation()
            var isReady = false
            switch spec.readiness {
            case .logLine(let line):
                isReady = scanner.contains(line)
            case .health(let url):
                if Date() >= nextHealth {
                    isReady = await healthy(url)
                    nextHealth = Date().addingTimeInterval(timing.healthInterval)
                }
            }
            if isReady && markReady(service, run) { return }

            if let how = locked({ run.exited ? run.exitDescription : nil }) {
                let output = Self.read(spec.logFile, from: run.logOffset)
                let lower = output.lowercased()
                if ["address already in use", "address in use", "couldn't bind", "could not bind"]
                    .contains(where: lower.contains) {
                    throw SupervisorError.portInUse(service: spec.name)
                }
                throw SupervisorError.failed(service: spec.name, reason:
                    "\(spec.name) \(how) before it was ready." + Self.tail(output))
            }
            if Date() >= deadline {
                await terminate(service, run)
                let output = Self.read(spec.logFile, from: run.logOffset)
                throw SupervisorError.failed(service: spec.name, reason:
                    "\(spec.name) was not ready after \(Self.seconds(spec.readyTimeout)) s." + Self.tail(output))
            }
            try await Task.sleep(nanoseconds: UInt64(timing.pollInterval * 1e9))
        }
    }

    /// Declares `run` ready unless it has exited or the service was stopped meanwhile.
    private func markReady(_ service: Service, _ run: Run) -> Bool {
        publish(service, .ready) {
            guard service.run === run, !run.exited else { return false }
            service.ready = true
            service.readyAt = Date()
            return true
        }
    }

    private func healthy(_ url: URL) async -> Bool {
        let request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData,
                                 timeoutInterval: max(0.5, timing.healthInterval * 2))
        guard let (_, response) = try? await session.data(for: request) else { return false }
        return (response as? HTTPURLResponse)?.statusCode == 200
    }

    /// The termination handler: after ready, and unless stopped, schedules the restarts.
    private func exited(_ service: Service, _ run: Run, _ process: Process) {
        let how = process.terminationReason == .uncaughtSignal
            ? "was killed by signal \(process.terminationStatus)"
            : "exited with status \(process.terminationStatus)"
        process.terminationHandler = nil
        locked {
            run.exited = true
            run.exitDescription = how
            guard service.run === run, service.ready, !service.stopped else { return }
            service.ready = false
            if let readyAt = service.readyAt, Date().timeIntervalSince(readyAt) >= timing.stableAfter {
                service.attempt = 0
            }
            service.restart = Task { [weak self] in await self?.restart(service, because: how) }
        }
    }

    private func restart(_ service: Service, because how: String) async {
        let spec = service.spec
        var why = how
        while true {
            let attempt: Int? = locked {
                guard !service.stopped else { return nil }
                service.attempt += 1
                return service.attempt
            }
            guard let attempt else { return }
            let delay = timing.backoff(attempt)
            let firstLine = why.prefix { $0 != "\n" }
            Self.note(spec.logFile, "\(spec.name): \(firstLine); restarting in \(Self.seconds(delay)) s (attempt \(attempt))")
            publish(service, .restarting(attempt: attempt, reason: why))
            try? await Task.sleep(nanoseconds: UInt64(delay * 1e9))
            do {
                try await launchAndWait(service)
                return
            } catch SupervisorError.stopped {
                return
            } catch is CancellationError {
                return
            } catch SupervisorError.portInUse {
                why = "port in use"
            } catch SupervisorError.failed(_, let reason) {
                why = reason
            } catch {
                why = "\(error)"
            }
        }
    }

    // MARK: Stopping

    private func stop(_ list: [Service]) async {
        for service in list {
            if let run = locked({ service.run }) { await terminate(service, run) }
            publish(service, .stopped)
        }
    }

    /// SIGTERM, then SIGKILL after `stopGrace`; `replay` as run-native.sh's `kill_replay`.
    private func terminate(_ service: Service, _ run: Run) async {
        let pid = run.pid
        // Signals only until Foundation has reaped it, so it is not another process by then.
        @Sendable func signal(_ sig: Int32) -> Bool {
            locked { run.exited ? false : kill(pid, sig) == 0 }
        }
        var children: [ReapedProcess] = []
        if service.spec.name == "replay" {
            // Paused, it cannot start another heimdall-source; its children are ended
            // while they are still its children. Listed even if it has just exited (and is
            // not reaped yet), so leftovers are ended too.
            _ = signal(SIGSTOP)
            children = locked { run.exited ? [] : Self.children(of: pid) }
            children.forEach { kill($0.pid, SIGTERM) }
        }
        let alive = signal(SIGTERM)
        if service.spec.name == "replay" { _ = signal(SIGCONT) }
        if alive, await !waitUntil(timing.stopGrace, { self.locked { run.exited } }) {
            Self.note(service.spec.logFile,
                      "\(service.spec.name) did not exit within \(Self.seconds(timing.stopGrace)) s; killing it")
            _ = signal(SIGKILL)
            _ = await waitUntil(5) { self.locked { run.exited } }
        }
        await Self.end(children, grace: timing.stopGrace)
    }

    private func waitUntil(_ timeout: TimeInterval, _ condition: @Sendable () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() >= deadline { return false }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        return true
    }

    // MARK: Orphans

    /// A process `reapOrphans` ended.
    public struct ReapedProcess: Sendable, Equatable {
        public let pid: pid_t
        public let executable: String
    }

    /// Ends every process whose executable is inside `binDir`: services a crashed shell
    /// left running, which would hold the state directories' locks. `midgard-replay`
    /// first, with its children (as `stopAll`); SIGTERM, then SIGKILL after `grace`.
    /// Returns the processes from `binDir` it ended, replay first.
    ///
    /// Run it only after the single-instance check: a second shell would otherwise end
    /// the services of the first.
    public static func reapOrphans(binDir: URL, grace: TimeInterval = 10) async -> [ReapedProcess] {
        let root = realPath(binDir) + "/"
        let me = getpid()
        let found = allPids().compactMap { pid -> ReapedProcess? in
            guard pid > 0, pid != me, let path = executablePath(of: pid), path.hasPrefix(root) else { return nil }
            return ReapedProcess(pid: pid, executable: path)
        }
        let replays = found.filter { isReplay($0.executable) }
        let others = found.filter { !isReplay($0.executable) }
        var children: [ReapedProcess] = []
        for replay in replays {
            kill(replay.pid, SIGSTOP)
            let kids = Self.children(of: replay.pid).filter { kid in !found.contains { $0.pid == kid.pid } }
            kids.forEach { kill($0.pid, SIGTERM) }
            children += kids
            kill(replay.pid, SIGTERM)
            kill(replay.pid, SIGCONT)
        }
        others.forEach { kill($0.pid, SIGTERM) }
        await end(replays + others + children, grace: grace)
        return replays + others
    }

    /// The path with every symlink resolved, as libproc reports executables (unlike
    /// `resolvingSymlinksInPath`, which turns /private/var back into /var).
    static func realPath(_ url: URL) -> String {
        guard let resolved = realpath(url.path, nil) else { return url.standardizedFileURL.path }
        defer { free(resolved) }
        return String(cString: resolved)
    }

    static func isReplay(_ executable: String) -> Bool {
        (executable as NSString).lastPathComponent == "midgard-replay"
    }

    /// Waits up to `grace` for each process to go, then SIGKILLs any that still runs the
    /// same executable (so a reused pid is left alone).
    static func end(_ processes: [ReapedProcess], grace: TimeInterval) async {
        func running(_ p: ReapedProcess) -> Bool { executablePath(of: p.pid) == p.executable }
        let deadline = Date().addingTimeInterval(grace)
        while processes.contains(where: running), Date() < deadline {
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        let survivors = processes.filter(running)
        survivors.forEach { kill($0.pid, SIGKILL) }
        let killDeadline = Date().addingTimeInterval(5)
        while survivors.contains(where: running), Date() < killDeadline {
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
    }

    /// Every pid on this Mac (libproc).
    static func allPids() -> [pid_t] {
        let needed = proc_listpids(UInt32(PROC_ALL_PIDS), 0, nil, 0)
        guard needed > 0 else { return [] }
        var pids = [pid_t](repeating: 0, count: Int(needed) / MemoryLayout<pid_t>.size + 64)
        let bytes = pids.withUnsafeMutableBytes {
            proc_listpids(UInt32(PROC_ALL_PIDS), 0, $0.baseAddress, Int32($0.count))
        }
        guard bytes > 0 else { return [] }
        return Array(pids.prefix(Int(bytes) / MemoryLayout<pid_t>.size))
    }

    /// The direct children of `pid` (libproc), with their executables.
    static func children(of pid: pid_t) -> [ReapedProcess] {
        var pids = [pid_t](repeating: 0, count: 256)
        let bytes = pids.withUnsafeMutableBytes {
            proc_listpids(UInt32(PROC_PPID_ONLY), UInt32(pid), $0.baseAddress, Int32($0.count))
        }
        guard bytes > 0 else { return [] }
        return pids.prefix(Int(bytes) / MemoryLayout<pid_t>.size).filter { $0 > 0 }
            .map { ReapedProcess(pid: $0, executable: executablePath(of: $0) ?? "") }
    }

    /// The executable a process runs (libproc), nil if it is gone.
    static func executablePath(of pid: pid_t) -> String? {
        var buffer = [UInt8](repeating: 0, count: 4 * Int(MAXPATHLEN))  // PROC_PIDPATHINFO_MAXSIZE
        let length = buffer.withUnsafeMutableBytes {
            proc_pidpath(pid, $0.baseAddress, UInt32($0.count))
        }
        guard length > 0 else { return nil }
        return String(decoding: buffer.prefix(Int(length)), as: UTF8.self)
    }

    // MARK: Helpers

    private func locked<T>(_ body: () throws -> T) rethrows -> T {
        lock.lock()
        defer { lock.unlock() }
        return try body()
    }

    /// Records and yields `status` unless the service was stopped (only `.stopped`
    /// follows that) or `condition` (run under the lock) says no. Yielded under the lock,
    /// so `statuses` sees changes in the order they happened.
    @discardableResult
    private func publish(_ service: Service, _ status: ServiceStatus,
                         if condition: () -> Bool = { true }) -> Bool {
        let name = service.spec.name
        return locked {
            guard status == .stopped || !service.stopped, condition() else { return false }
            latest[name] = status
            continuation.yield((name, status))
            return true
        }
    }

    /// Moves a log over `logLimit` to `<name>.log.1`, replacing an older one.
    static func rotate(_ file: URL) {
        guard let size = (try? FileManager.default.attributesOfItem(atPath: file.path))?[.size] as? NSNumber,
              size.uint64Value > logLimit else { return }
        let old = file.appendingPathExtension("1")
        // rename(2) replaces the older .1 in one step.
        _ = rename(file.path, old.path)
    }

    /// Appends one line from the shell to a service's log.
    static func note(_ file: URL, _ line: String) {
        let fd = open(file.path, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0o644)
        guard fd >= 0 else { return }
        defer { close(fd) }
        let bytes = Array("[midgard shell] \(line)\n".utf8)
        _ = bytes.withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
    }

    /// The log from `offset` on (at most the last `tailBytes`).
    static func read(_ file: URL, from offset: UInt64) -> String {
        guard let handle = try? FileHandle(forReadingFrom: file) else { return "" }
        defer { try? handle.close() }
        let end = (try? handle.seekToEnd()) ?? 0
        try? handle.seek(toOffset: max(offset, end > UInt64(tailBytes) ? end - UInt64(tailBytes) : 0))
        return String(decoding: (try? handle.readToEnd()) ?? Data(), as: UTF8.self)
    }

    /// The last `tailLines` lines, for a failure message.
    static func tail(_ output: String) -> String {
        let lines = output.split(separator: "\n", omittingEmptySubsequences: false)
            .map { $0.hasSuffix("\r") ? $0.dropLast() : $0 }
        let trimmed = lines.last == "" ? lines.dropLast() : lines[...]
        guard !trimmed.isEmpty else { return " It wrote nothing." }
        return " Last log lines:\n" + trimmed.suffix(tailLines).joined(separator: "\n")
    }

    static func seconds(_ value: TimeInterval) -> String { String(format: "%g", value) }
}

/// Reads a log as it grows and looks for a line, including across two reads.
struct LogScanner {
    let file: URL
    var offset: UInt64
    private var carry = Data()

    init(file: URL, offset: UInt64) {
        self.file = file
        self.offset = offset
    }

    mutating func contains(_ text: String) -> Bool {
        let pattern = Data(text.utf8)
        guard !pattern.isEmpty else { return true }
        guard let handle = try? FileHandle(forReadingFrom: file) else { return false }
        defer { try? handle.close() }
        guard (try? handle.seek(toOffset: offset)) != nil,
              let data = try? handle.readToEnd(), !data.isEmpty else { return false }
        offset += UInt64(data.count)
        let window = carry + data
        if window.range(of: pattern) != nil { return true }
        carry = Data(window.suffix(pattern.count - 1))
        return false
    }
}
