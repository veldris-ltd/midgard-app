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
        XCTAssertTrue(script.contains("localStorage.getItem(\"heimdall.cop.view\") === null"))
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
        guard case .heldBy(let pid) = InstanceLock.acquire(file) else { return XCTFail("second acquired") }
        XCTAssertEqual(pid, getpid())
        first.release()
        guard case .acquired(let again) = InstanceLock.acquire(file) else { return XCTFail("not re-acquired") }
        again.release()
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
