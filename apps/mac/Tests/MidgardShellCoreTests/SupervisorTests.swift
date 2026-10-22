import Darwin
import Foundation
import XCTest
@testable import MidgardShellCore

/// Every status the supervisor publishes, in order.
final class StatusRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var events: [(String, ServiceStatus)] = []

    func record(_ name: String, _ status: ServiceStatus) {
        lock.lock(); defer { lock.unlock() }
        events.append((name, status))
    }

    func statuses(_ name: String) -> [ServiceStatus] {
        lock.lock(); defer { lock.unlock() }
        return events.filter { $0.0 == name }.map(\.1)
    }

    /// The services that reached `status`, in the order they reached it.
    func names(reaching status: ServiceStatus) -> [String] {
        lock.lock(); defer { lock.unlock() }
        return events.filter { $0.1 == status }.map(\.0)
    }

    /// Polls until `condition` holds for `name`'s statuses, or `timeout` passes.
    func wait(_ name: String, timeout: TimeInterval = 10,
              until condition: ([ServiceStatus]) -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition(statuses(name)) { return true }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        return condition(statuses(name))
    }
}

/// Whether a process with this pid still exists (zombies count), polled until it is gone.
func gone(_ pid: pid_t, within timeout: TimeInterval = 5) async -> Bool {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if kill(pid, 0) != 0 && errno == ESRCH { return true }
        try? await Task.sleep(nanoseconds: 20_000_000)
    }
    return kill(pid, 0) != 0 && errno == ESRCH
}

final class SupervisorTests: XCTestCase {
    var dir: URL!
    var recorder: StatusRecorder!
    var supervisor: Supervisor!

    /// Short delays so restarts and stops finish in well under a second.
    let fast = Supervisor.Timing(backoffBase: 0.05, backoffCap: 0.2, stableAfter: 300,
                                 healthInterval: 0.1, stopGrace: 1)

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("midgard-sup-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        recorder = StatusRecorder()
        makeSupervisor(fast)
    }

    override func tearDown() async throws {
        await supervisor?.stopAll()
        try? FileManager.default.removeItem(at: dir)
    }

    func makeSupervisor(_ timing: Supervisor.Timing) {
        let recorder = recorder!
        supervisor = Supervisor(timing: timing) { recorder.record($0, $1) }
    }

    /// A fake service: a shell script in the temp dir.
    func fake(_ name: String, _ body: String) throws -> URL {
        let url = dir.appendingPathComponent(name)
        try "#!/bin/sh\n\(body)\n".write(to: url, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
        return url
    }

    func spec(_ name: String, _ body: String, readiness: ServiceSpec.Readiness = .logLine("listening on"),
              timeout: TimeInterval = 10, arguments: [String] = []) throws -> ServiceSpec {
        ServiceSpec(name: name, executable: try fake(name, body), arguments: arguments,
                    readiness: readiness, logFile: dir.appendingPathComponent("logs/\(name).log"),
                    readyTimeout: timeout)
    }

    func log(_ name: String) -> String {
        (try? String(contentsOf: dir.appendingPathComponent("logs/\(name).log"), encoding: .utf8)) ?? ""
    }

    func testReadyByLogLine() async throws {
        let s = try spec("svc", "echo \"args: $*\"; sleep 0.2; echo 'listening on 1'; exec sleep 300",
                         arguments: ["--a", "b c"])
        try await supervisor.start([s])
        XCTAssertEqual(recorder.statuses("svc"), [.starting, .ready])
        XCTAssertEqual(supervisor.status(of: "svc"), .ready)
        XCTAssertTrue(log("svc").contains("args: --a b c"))
        XCTAssertNotNil(supervisor.pid(of: "svc"))
    }

    /// A ready line left in the log by an earlier run does not count.
    func testOldLogLinesDoNotCount() async throws {
        let s = try spec("svc", "exec sleep 300", timeout: 0.5)
        try FileManager.default.createDirectory(at: s.logFile.deletingLastPathComponent(),
                                                withIntermediateDirectories: true)
        try "listening on (old run)\n".write(to: s.logFile, atomically: true, encoding: .utf8)
        do {
            try await supervisor.start([s])
            XCTFail("expected a failure")
        } catch SupervisorError.failed(let service, _) {
            XCTAssertEqual(service, "svc")
        }
        XCTAssertTrue(log("svc").hasPrefix("listening on (old run)\n"), "log is appended to")
    }

    func testReadyByHealthURL() async throws {
        let root = dir.appendingPathComponent("web", isDirectory: true)
        try FileManager.default.createDirectory(at: root.appendingPathComponent("home"),
                                                withIntermediateDirectories: true)
        try Data("ok".utf8).write(to: root.appendingPathComponent("home/health"))
        let port = try Ports.free(count: 1)[0]
        let health = URL(string: "http://127.0.0.1:\(port)/home/health")!
        let s = try spec("llama", "exec sleep 300", readiness: .health(health))
        let server = WebServer(root: root, port: port)
        defer { server.stop() }
        // The health URL starts answering only after a while.
        let starter = Task.detached {
            try await Task.sleep(nanoseconds: 600_000_000)
            try server.start()
        }
        try await supervisor.start([s])
        try await starter.value
        XCTAssertEqual(recorder.statuses("llama"), [.starting, .ready])
    }

    func testNeverReadyFailsWithLogTail() async throws {
        let never = try spec("never", "i=1; while [ $i -le 30 ]; do echo line $i; i=$((i+1)); done; exec sleep 300",
                             timeout: 2)
        let after = try spec("after", "touch \(dir.path)/after-ran; exec sleep 300")
        do {
            try await supervisor.start([never, after])
            XCTFail("expected a failure")
        } catch SupervisorError.failed(let service, let reason) {
            XCTAssertEqual(service, "never")
            XCTAssertTrue(reason.contains("line 30"), reason)
            XCTAssertTrue(reason.contains("line 11"), reason)
            XCTAssertFalse(reason.contains("line 10\n"), reason)
        }
        guard case .failed(let reason)? = recorder.statuses("never").last else {
            return XCTFail("not failed: \(recorder.statuses("never"))")
        }
        XCTAssertTrue(reason.contains("line 30"))
        XCTAssertEqual(recorder.statuses("after"), [], "nothing after a failure is started")
        XCTAssertFalse(FileManager.default.fileExists(atPath: dir.appendingPathComponent("after-ran").path))
        XCTAssertNil(supervisor.pid(of: "never"), "the service that never became ready is stopped")
    }

    func testExitBeforeReadyFails() async throws {
        let s = try spec("crash", "echo boom; exit 3")
        do {
            try await supervisor.start([s])
            XCTFail("expected a failure")
        } catch SupervisorError.failed(_, let reason) {
            XCTAssertTrue(reason.contains("boom"), reason)
        }
    }

    func testPortInUse() async throws {
        for message in ["Error: Address already in use (os error 48)", "bind: ADDRESS IN USE"] {
            let s = try spec("clash", "echo '\(message)' >&2; exit 1")
            do {
                try await supervisor.start([s])
                XCTFail("expected port in use")
            } catch SupervisorError.portInUse(let service) {
                XCTAssertEqual(service, "clash")
            }
            XCTAssertEqual(recorder.statuses("clash").last, .failed("port in use"))
            await supervisor.stopAll()
        }
    }

    func testRestartsWithBackoff() async throws {
        let s = try spec("flaky", "sleep 0.1; echo 'listening on'; sleep 0.3; exit 1")
        try await supervisor.start([s])
        let restarted = await recorder.wait("flaky") { $0.contains(.restarting(attempt: 3)) }
        XCTAssertTrue(restarted, "\(recorder.statuses("flaky"))")
        let seen = recorder.statuses("flaky")
        XCTAssertEqual(Array(seen.prefix(6)), [.starting, .ready, .restarting(attempt: 1), .ready,
                                               .restarting(attempt: 2), .ready])
        XCTAssertTrue(log("flaky").contains("restarting in"), log("flaky"))

        await supervisor.stopAll()
        let count = recorder.statuses("flaky").count
        XCTAssertEqual(recorder.statuses("flaky").last, .stopped)
        try await Task.sleep(nanoseconds: 800_000_000)
        XCTAssertEqual(recorder.statuses("flaky").count, count, "no restarts after stopAll")
    }

    func testBackoffResetsAfterStayingUp() async throws {
        makeSupervisor(Supervisor.Timing(backoffBase: 0.05, backoffCap: 0.2, stableAfter: 0.1,
                                         healthInterval: 0.1, stopGrace: 1))
        let s = try spec("flaky", "echo 'listening on'; sleep 0.3; exit 1")
        try await supervisor.start([s])
        let restarted = await recorder.wait("flaky") {
            $0.filter { $0 == .restarting(attempt: 1) }.count >= 3
        }
        XCTAssertTrue(restarted, "\(recorder.statuses("flaky"))")
        XCTAssertFalse(recorder.statuses("flaky").contains(.restarting(attempt: 2)))
    }

    func testBackoffDelays() {
        let timing = Supervisor.Timing()
        XCTAssertEqual((1...8).map(timing.backoff), [2, 4, 8, 16, 32, 60, 60, 60])
        XCTAssertEqual(timing.stableAfter, 300)
        XCTAssertEqual(timing.stopGrace, 15)
        XCTAssertEqual(timing.healthInterval, 0.5)
    }

    func testStopAllLeavesNoProcesses() async throws {
        let a = try spec("a", "echo 'listening on'; exec sleep 300")
        let b = try spec("b", "trap '' TERM; echo 'listening on'; while :; do sleep 0.05; done")
        let replay = try spec("replay", """
            sleep 300 &
            echo "child $!"
            echo '"replay started"'
            wait
            """, readiness: .logLine("\"replay started\""))
        try await supervisor.start([a, b, replay])
        let pids = try ["a", "b", "replay"].map { try XCTUnwrap(supervisor.pid(of: $0)) }
        let line = try XCTUnwrap(log("replay").split(separator: "\n").first { $0.hasPrefix("child ") })
        let child = try XCTUnwrap(pid_t(line.dropFirst(6)))
        XCTAssertEqual(kill(child, 0), 0)

        let started = Date()
        await supervisor.stopAll()
        XCTAssertLessThan(Date().timeIntervalSince(started), 5)
        for pid in pids + [child] {
            let isGone = await gone(pid)
            XCTAssertTrue(isGone, "pid \(pid) still exists")
        }
        for name in ["a", "b", "replay"] {
            XCTAssertEqual(recorder.statuses(name).last, .stopped, name)
            XCTAssertNil(supervisor.pid(of: name))
        }
        XCTAssertEqual(recorder.names(reaching: .stopped), ["replay", "b", "a"], "reverse start order")
        await supervisor.stopAll()
        await supervisor.stopAll()
        XCTAssertEqual(recorder.statuses("a").filter { $0 == .stopped }.count, 1)
    }

    func testStopAllConcurrentlyAndAfterwardsStartAgain() async throws {
        let a = try spec("a", "echo 'listening on'; exec sleep 300")
        try await supervisor.start([a])
        let first = try XCTUnwrap(supervisor.pid(of: "a"))
        let supervisor = supervisor!
        async let one: Void = supervisor.stopAll()
        async let two: Void = supervisor.stopAll()
        _ = await (one, two)
        let isGone = await gone(first, within: 0.5)
        XCTAssertTrue(isGone)

        try await supervisor.start([a])
        XCTAssertEqual(supervisor.status(of: "a"), .ready)
        XCTAssertNotEqual(supervisor.pid(of: "a"), first)
    }

    func testStopAllDuringStartStopsIt() async throws {
        let slow = try spec("slow", "sleep 0.5; echo 'listening on'; exec sleep 300")
        let next = try spec("next", "echo 'listening on'; exec sleep 300")
        let supervisor = supervisor!
        let starting = Task { try await supervisor.start([slow, next]) }
        _ = await recorder.wait("slow") { $0 == [.starting] }
        try await Task.sleep(nanoseconds: 100_000_000)
        let pid = try XCTUnwrap(supervisor.pid(of: "slow"))
        await supervisor.stopAll()
        do {
            try await starting.value
            XCTFail("expected start to stop")
        } catch SupervisorError.stopped {}
        let isGone = await gone(pid)
        XCTAssertTrue(isGone)
        XCTAssertEqual(recorder.statuses("slow").last, .stopped)
        XCTAssertEqual(recorder.statuses("next"), [])
    }
}
