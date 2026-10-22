import Darwin
import Foundation

/// What the starting screen shows for one service.
public enum ServiceStatus: Sendable, Equatable {
    case starting
    case ready
    /// Exited after it was ready; launched again after a back-off (attempt counts from 1).
    case restarting(attempt: Int)
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
/// The processes do not outlive the shell on purpose; call `stopAll()` before quitting.
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

    let timing: Timing
    private let onStatus: @Sendable (String, ServiceStatus) -> Void
    private let session: URLSession
    private let lock = NSLock()
    /// Held while a status is decided and delivered, so the callback sees them in order.
    private let publishLock = NSLock()
    // Guarded by `lock`.
    private var services: [Service] = []
    private var latest: [String: ServiceStatus] = [:]
    private var generation = 0
    private var lastStop: Task<Void, Never>?

    /// - Parameter onStatus: called on an arbitrary thread for every status change, one at
    ///   a time and in order; it must return quickly and must not call `start` or `stopAll`.
    public init(timing: Timing = Timing(),
                onStatus: @escaping @Sendable (String, ServiceStatus) -> Void = { _, _ in }) {
        self.timing = timing
        self.onStatus = onStatus
        let configuration = URLSessionConfiguration.ephemeral
        configuration.connectionProxyDictionary = [:]
        session = URLSession(configuration: configuration)
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
    /// `stopAll()`). May be called again after `stopAll()`.
    public func start(_ specs: [ServiceSpec]) async throws {
        let generation = locked { self.generation }
        for spec in specs {
            let service = Service(spec)
            try locked {
                guard generation == self.generation else { throw SupervisorError.stopped }
                services.append(service)
            }
            publish(service, .starting)
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
                if lower.contains("address already in use") || lower.contains("address in use") {
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
            try? await Task.sleep(nanoseconds: UInt64(timing.pollInterval * 1e9))
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
            Self.note(spec.logFile, "\(spec.name) \(why); restarting in \(Self.seconds(delay)) s (attempt \(attempt))")
            publish(service, .restarting(attempt: attempt))
            try? await Task.sleep(nanoseconds: UInt64(delay * 1e9))
            do {
                try await launchAndWait(service)
                return
            } catch SupervisorError.stopped {
                return
            } catch SupervisorError.portInUse {
                why = "could not bind its port"
            } catch SupervisorError.failed(_, let reason) {
                why = "did not come back (\(reason.prefix { $0 != "\n" }))"
            } catch {
                why = "did not come back (\(error))"
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
        // Signals only while Foundation has not reaped it, so the pid cannot be reused.
        @Sendable func signal(_ sig: Int32) -> Bool {
            locked { run.exited ? false : kill(pid, sig) == 0 }
        }
        guard signal(0) else { return }
        var children: [pid_t] = []
        if service.spec.name == "replay" {
            // Paused, it cannot start another heimdall-source; its children are ended
            // while they are still its children.
            _ = signal(SIGSTOP)
            children = Self.children(of: pid)
            children.forEach { kill($0, SIGTERM) }
            _ = signal(SIGTERM)
            _ = signal(SIGCONT)
        } else {
            _ = signal(SIGTERM)
        }
        if await !waitUntil(timing.stopGrace, { self.locked { run.exited } }) {
            Self.note(service.spec.logFile,
                      "\(service.spec.name) did not exit within \(Self.seconds(timing.stopGrace)) s; killing it")
            _ = signal(SIGKILL)
            _ = await waitUntil(5) { self.locked { run.exited } }
        }
        for child in children where await !waitUntil(timing.stopGrace, { kill(child, 0) != 0 }) {
            kill(child, SIGKILL)
        }
    }

    private func waitUntil(_ timeout: TimeInterval, _ condition: @Sendable () -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            if Date() >= deadline { return false }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        return true
    }

    /// The direct children of `pid` (libproc).
    static func children(of pid: pid_t) -> [pid_t] {
        var pids = [pid_t](repeating: 0, count: 256)
        let bytes = pids.withUnsafeMutableBytes {
            proc_listpids(UInt32(PROC_PPID_ONLY), UInt32(pid), $0.baseAddress, Int32($0.count))
        }
        guard bytes > 0 else { return [] }
        return pids.prefix(Int(bytes) / MemoryLayout<pid_t>.size).filter { $0 > 0 }
    }

    // MARK: Helpers

    private func locked<T>(_ body: () throws -> T) rethrows -> T {
        lock.lock()
        defer { lock.unlock() }
        return try body()
    }

    /// Records and delivers `status` unless the service was stopped (only `.stopped`
    /// follows that) or `condition` (run under the lock) says no.
    @discardableResult
    private func publish(_ service: Service, _ status: ServiceStatus,
                         if condition: () -> Bool = { true }) -> Bool {
        publishLock.lock()
        defer { publishLock.unlock() }
        let name = service.spec.name
        let accepted = locked { () -> Bool in
            guard status == .stopped || !service.stopped, condition() else { return false }
            latest[name] = status
            return true
        }
        if accepted { onStatus(name, status) }
        return accepted
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
