import Darwin
import Foundation
import XCTest
@testable import MidgardShellCore

final class OfflineRulesTests: XCTestCase {
    func testRuleListIsValidJSONBlockingEverythingFirst() throws {
        let rules = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(OfflineRules.json.utf8)) as? [[String: [String: String]]])
        XCTAssertEqual(rules.first?["trigger"]?["url-filter"], ".*")
        XCTAssertEqual(rules.first?["action"]?["type"], "block")
        let allowed = rules.dropFirst().map { $0["trigger"]?["url-filter"] }
        XCTAssertEqual(allowed, OfflineRules.allowed)
        XCTAssertTrue(rules.dropFirst().allSatisfy { $0["action"]?["type"] == "ignore-previous-rules" })
    }

    /// WebKit's url-filter is matched like a regular expression against the whole URL.
    func testPatternsLetThroughOnlyLoopback() throws {
        let patterns = try OfflineRules.allowed.map { try NSRegularExpression(pattern: $0) }
        func allowed(_ url: String) -> Bool {
            patterns.contains { $0.firstMatch(in: url, range: NSRange(url.startIndex..., in: url)) != nil }
        }
        XCTAssertTrue(allowed("http://127.0.0.1:61235/heimdall/index.html"))
        XCTAssertTrue(allowed("about:blank"))
        XCTAssertTrue(allowed("blob:http://127.0.0.1:61235/abc"))
        XCTAssertTrue(allowed("data:image/png;base64,AAAA"))
        XCTAssertFalse(allowed("https://example.com/"))
        XCTAssertFalse(allowed("http://127.0.0.10:80/"))
        XCTAssertFalse(allowed("http://127.0.0.1.evil.example/"))
        XCTAssertFalse(allowed("https://127.0.0.1:443/"))
        XCTAssertFalse(allowed("http://example.com/?http://127.0.0.1:1"))
        XCTAssertFalse(allowed("ws://127.0.0.1:1/"))
        XCTAssertFalse(allowed("http://127.0.0.1:1@example.com/"))
        XCTAssertFalse(allowed("http://127.0.0.1:@example.com/"))
    }
}

final class NavigationTests: XCTestCase {
    let origin = "http://127.0.0.1:61235"
    let gateway = "http://127.0.0.1:7710"

    func allowed(_ url: String) -> Bool {
        Navigation.isAllowed(URL(string: url)!, origin: origin, gatewayOrigin: gateway)
    }

    func testOwnOriginAndGatewayAllowed() {
        XCTAssertTrue(allowed("http://127.0.0.1:61235/"))
        XCTAssertTrue(allowed("http://127.0.0.1:61235/midgard/index.html#/objects"))
        XCTAssertTrue(allowed("http://127.0.0.1:7710/api/v1/health"))
        XCTAssertTrue(allowed("about:blank"))
    }

    func testEverythingElseRefused() {
        XCTAssertFalse(allowed("https://example.com/"))
        XCTAssertFalse(allowed("http://127.0.0.1:9999/"))
        XCTAssertFalse(allowed("https://127.0.0.1:61235/"))
        XCTAssertFalse(allowed("http://localhost:61235/"))
        XCTAssertFalse(allowed("file:///etc/passwd"))
        XCTAssertFalse(allowed("http://127.0.0.1:61235@example.com/"))
        XCTAssertFalse(Navigation.isAllowed(URL(string: "http://127.0.0.1:7710/")!, origin: origin,
                                            gatewayOrigin: nil))
    }
}

final class HeimdallViewTests: XCTestCase {
    func testDefaultByArchitecture() {
        XCTAssertEqual(HeimdallView.preferred(appleSilicon: true), "3d")
        XCTAssertEqual(HeimdallView.preferred(appleSilicon: false), "2d")
        #if arch(arm64)
        XCTAssertTrue(HeimdallView.isAppleSilicon())
        #endif
    }

    func testScriptSetsOnlyAnUnsetKeyOnOwnOrigin() {
        let script = HeimdallView.script(origin: "http://127.0.0.1:5", view: "2d")
        XCTAssertTrue(script.contains("location.origin !== \"http:\\/\\/127.0.0.1:5\""))
        XCTAssertTrue(script.contains("if (false || localStorage.getItem(\"heimdall.cop.view\") === null)"))
        XCTAssertTrue(HeimdallView.script(origin: "http://127.0.0.1:5", view: "2d", force: true)
            .contains("if (true || localStorage"))
        XCTAssertTrue(script.contains("localStorage.setItem(\"heimdall.cop.view\", \"2d\")"))
    }

    func testPageScriptEscapesText() {
        XCTAssertEqual(PageScript.call("midgardStatus", "a \"b\" </script>"),
                       "if (typeof window.midgardStatus === 'function') { window.midgardStatus("
                       + "\"a \\\"b\\\" \\u003c\\/script\\u003e\"); }")
    }

    func testFriendlyNames() {
        XCTAssertEqual(["llama", "mimir", "gateway", "heimdall", "forge", "replay"].map(ServiceNames.friendly),
                       ["Model", "Mimir", "Gateway", "Heimdall", "Forge", "Replay"])
    }
}

final class InstanceLockTests: XCTestCase {
    func testSecondAcquireSeesFirstPidUntilReleased() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("lock-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("Midgard.instance")
        guard case .acquired(let first) = InstanceLock.acquire(file) else { return XCTFail("first not acquired") }
        guard case .heldBy(let pid, let stopping) = InstanceLock.acquire(file) else { return XCTFail("second acquired") }
        XCTAssertEqual(pid, getpid())
        XCTAssertFalse(stopping)
        first.markStopping()
        guard case .heldBy(let pid2, let stopping2) = InstanceLock.acquire(file) else { return XCTFail("acquired") }
        XCTAssertEqual(pid2, getpid())
        XCTAssertTrue(stopping2)
        DispatchQueue.global().asyncAfter(deadline: .now() + 0.3) { first.release() }
        guard case .acquired(let waited) = InstanceLock.acquire(file, waitingUpTo: 5) else {
            return XCTFail("did not get the lock after the holder released it")
        }
        waited.release()
        guard case .acquired(let again) = InstanceLock.acquire(file) else { return XCTFail("not re-acquired") }
        again.release()
    }

    func testRewriteReplacesLongerContentWithoutLeftovers() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("lock-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent("Midgard.instance")
        try "1234567890 stopping and some leftover text\n".write(to: file, atomically: false, encoding: .utf8)
        guard case .acquired(let lock) = InstanceLock.acquire(file) else { return XCTFail("not acquired") }
        XCTAssertEqual(try String(contentsOf: file, encoding: .utf8), "\(getpid())\n")
        lock.markStopping()
        XCTAssertEqual(try String(contentsOf: file, encoding: .utf8), "\(getpid()) stopping\n")
        lock.release()
    }

    func testOnlyTheFirstLineIsRead() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("lock-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = dir.appendingPathComponent("Midgard.instance")
        guard case .acquired(let lock) = InstanceLock.acquire(file) else { return XCTFail("not acquired") }
        // As a reader could see it between rewrite's write and its truncate.
        let fd = open(file.path, O_WRONLY)
        let text = Array("\(getpid())\n99 stopping\n".utf8)
        _ = text.withUnsafeBytes { pwrite(fd, $0.baseAddress, $0.count, 0) }
        close(fd)
        guard case .heldBy(let pid, let stopping) = InstanceLock.acquire(file) else { return XCTFail("acquired") }
        XCTAssertEqual(pid, getpid())
        XCTAssertFalse(stopping)
        lock.release()
    }
}

final class SuiteStartTests: XCTestCase {
    /// Records the calls in order.
    final class Calls: @unchecked Sendable {
        private let lock = NSLock()
        private var list: [String] = []
        private var next: UInt16 = 1000
        func add(_ s: String) { lock.lock(); list.append(s); lock.unlock() }
        var all: [String] { lock.lock(); defer { lock.unlock() }; return list }
        func ports() -> SuitePorts {
            lock.lock(); defer { lock.unlock() }
            next += 10
            return SuitePorts(Array(next..<next + 8))!
        }
    }

    func testRetriesWithFreshPortsAfterPortInUse() async throws {
        let calls = Calls()
        let ports = try await SuiteStart.run(
            choosePorts: { calls.ports() },
            start: { ports in
                calls.add("start \(ports.gatewayHTTP)")
                if ports.gatewayHTTP < 1030 { throw SupervisorError.portInUse(service: "gateway") }
            },
            stopAll: { calls.add("stopAll") },
            onRetry: { service, retry in calls.add("retry \(retry) \(service)") })
        XCTAssertEqual(ports.gatewayHTTP, 1030)
        XCTAssertEqual(calls.all, ["start 1010", "stopAll", "retry 1 gateway",
                                   "start 1020", "stopAll", "retry 2 gateway", "start 1030"])
    }

    func testGivesUpAfterThreeRetries() async {
        let calls = Calls()
        do {
            _ = try await SuiteStart.run(
                choosePorts: { calls.ports() },
                start: { _ in calls.add("start"); throw SupervisorError.portInUse(service: "mimir") },
                stopAll: { calls.add("stopAll") })
            XCTFail("did not throw")
        } catch {
            XCTAssertEqual(error as? SupervisorError, .portInUse(service: "mimir"))
        }
        XCTAssertEqual(calls.all.filter { $0 == "start" }.count, 4)
        XCTAssertEqual(calls.all.filter { $0 == "stopAll" }.count, 3)
    }

    func testOtherFailureIsNotRetried() async {
        let calls = Calls()
        do {
            _ = try await SuiteStart.run(
                choosePorts: { calls.ports() },
                start: { _ in calls.add("start"); throw SupervisorError.failed(service: "forge", reason: "x") },
                stopAll: { calls.add("stopAll") })
            XCTFail("did not throw")
        } catch {
            XCTAssertEqual(error as? SupervisorError, .failed(service: "forge", reason: "x"))
        }
        XCTAssertEqual(calls.all, ["start"])
    }

    func testCancellationStopsAll() async {
        let calls = Calls()
        let task = Task {
            try await SuiteStart.run(
                choosePorts: { calls.ports() },
                start: { _ in
                    calls.add("start")
                    try await Task.sleep(nanoseconds: 10_000_000_000)
                },
                stopAll: { calls.add("stopAll") })
        }
        try? await Task.sleep(nanoseconds: 100_000_000)
        task.cancel()
        let result = await task.result
        XCTAssertThrowsError(try result.get()) { XCTAssertTrue($0 is CancellationError) }
        XCTAssertEqual(calls.all, ["start", "stopAll"])
    }
}

final class ReloadBudgetTests: XCTestCase {
    func testTwoReloadsPerMinute() {
        var budget = ReloadBudget()
        let t0 = Date(timeIntervalSince1970: 1000)
        XCTAssertTrue(budget.take(now: t0))
        XCTAssertTrue(budget.take(now: t0.addingTimeInterval(10)))
        XCTAssertFalse(budget.take(now: t0.addingTimeInterval(20)))
        XCTAssertFalse(budget.take(now: t0.addingTimeInterval(59)))
        XCTAssertTrue(budget.take(now: t0.addingTimeInterval(60)))  // the first has aged out
        XCTAssertFalse(budget.take(now: t0.addingTimeInterval(65)))
    }
}

final class ServiceAlertTests: XCTestCase {
    func testOnlyRepeatedRestartsAndFailuresAlert() {
        XCTAssertNil(ServiceAlert.message(service: "heimdall", status: .ready))
        XCTAssertNil(ServiceAlert.message(service: "heimdall", status: .starting))
        XCTAssertNil(ServiceAlert.message(service: "heimdall", status: .stopped))
        XCTAssertNil(ServiceAlert.message(service: "heimdall", status: .restarting(attempt: 2, reason: nil)))
        XCTAssertEqual(ServiceAlert.message(service: "heimdall", status: .restarting(attempt: 3, reason: "x")),
                       "Heimdall has stopped. Try Reset demo, or quit and reopen Midgard.")
        XCTAssertEqual(ServiceAlert.message(service: "llama", status: .failed("boom")),
                       "Model has stopped. Try Reset demo, or quit and reopen Midgard.")
    }
}

final class SessionTimingTests: XCTestCase {
    func testWakeDecisionsByWallClock() {
        let refreshAt = Date(timeIntervalSince1970: 1000)
        let expiresAt = Date(timeIntervalSince1970: 1600)
        func action(_ t: TimeInterval) -> SessionTiming.Action {
            SessionTiming.action(now: Date(timeIntervalSince1970: t), refreshAt: refreshAt, expiresAt: expiresAt)
        }
        XCTAssertEqual(action(999), .wait)
        XCTAssertEqual(action(1000), .refresh)
        XCTAssertEqual(action(1599), .refresh)
        XCTAssertEqual(action(1600), .refreshAndReload)
        XCTAssertEqual(action(99_999), .refreshAndReload)
    }
}
