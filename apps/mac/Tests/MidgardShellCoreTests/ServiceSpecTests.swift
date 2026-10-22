import Foundation
import XCTest
@testable import MidgardShellCore

final class ServiceSpecTests: XCTestCase {
    let bin = URL(fileURLWithPath: "/R/bin", isDirectory: true)
    let models = URL(fileURLWithPath: "/R/models", isDirectory: true)
    let home = URL(fileURLWithPath: "/H", isDirectory: true)
    let logs = URL(fileURLWithPath: "/L", isDirectory: true)
    let ports = SuitePorts(gatewayHTTP: 1001, gatewayMesh: 1002, mimirMesh: 1003, mimirObserver: 1004,
                           mimirHealth: 1005, heimdallMesh: 1006, heimdallHealth: 1007, llama: 1008)
    let origin = "http://127.0.0.1:2000"

    let appleSilicon = ModelServer.Hardware(arm64: true, physicalCores: 10)

    func specs(recording: URL? = nil, hardware: ModelServer.Hardware? = nil) -> [ServiceSpec] {
        SuiteSpecs.make(bin: bin, models: models, home: home, logs: logs, ports: ports,
                        webOrigin: origin, recording: recording, hardware: hardware ?? appleSilicon)
    }

    func spec(_ name: String, _ all: [ServiceSpec]) throws -> ServiceSpec {
        try XCTUnwrap(all.first { $0.name == name }, name)
    }

    func testStartOrderWithoutRecording() {
        XCTAssertEqual(specs().map(\.name), ["llama", "mimir", "gateway", "heimdall", "forge"])
    }

    func testReplayOnlyWithRecording() throws {
        let all = specs(recording: URL(fileURLWithPath: "/R/recording", isDirectory: true))
        XCTAssertEqual(all.map(\.name), ["llama", "mimir", "gateway", "heimdall", "forge", "replay"])
        let replay = try spec("replay", all)
        XCTAssertEqual(replay.executable.path, "/R/bin/midgard-replay")
        XCTAssertEqual(replay.arguments, [
            "--recording", "/R/recording", "--identity-dir", "/H/airports-mesh/feeds",
            "--forge-dir", "/H/live-feeds/data", "--addr", "127.0.0.1:1006",
        ])
        XCTAssertEqual(replay.readiness, .logLine("\"replay started\""))
        XCTAssertEqual(replay.logFile.path, "/L/replay.log")
    }

    let llamaCommon = [
        "-m", "/R/models/granite/granite-3.3-2b-instruct-Q4_K_M.gguf",
        "--alias", "granite-3.3-2b-instruct-q4km",
        "--host", "127.0.0.1", "--port", "1008", "--ctx-size", "8192", "--parallel", "1",
        "--temp", "0.2", "--json-schema", "{\"type\":\"object\"}",
    ]

    func testLlamaOnAppleSilicon() throws {
        let llama = try spec("llama", specs())
        XCTAssertEqual(llama.executable.path, "/R/bin/llama/arm64/llama-server")
        XCTAssertEqual(llama.arguments, llamaCommon + ["--n-gpu-layers", "99"])
        XCTAssertEqual(llama.readiness, .health(URL(string: "http://127.0.0.1:1008/health")!))
        XCTAssertEqual(llama.readyTimeout, 180)
        XCTAssertEqual(llama.logFile.path, "/L/llama.log")
    }

    func testLlamaOnIntel() throws {
        let llama = try spec("llama", specs(hardware: .init(arm64: false, physicalCores: 4)))
        XCTAssertEqual(llama.executable.path, "/R/bin/llama/x64/llama-server")
        XCTAssertEqual(llama.arguments, llamaCommon + ["--n-gpu-layers", "0", "--threads", "4"])
        XCTAssertEqual(llama.readiness, .health(URL(string: "http://127.0.0.1:1008/health")!))
        XCTAssertEqual(llama.logFile.path, "/L/llama.log")
    }

    func testOnlyLlamaDependsOnTheChip() {
        let intel = specs(hardware: .init(arm64: false, physicalCores: 2))
        XCTAssertEqual(intel.dropFirst(), specs().dropFirst())
    }

    func testMimir() throws {
        let mimir = try spec("mimir", specs())
        XCTAssertEqual(mimir.executable.path, "/R/bin/mimir-server")
        XCTAssertEqual(mimir.arguments, [
            "--identity-dir", "/H/airports-mesh/mimir", "--state-dir", "/H/mimir-state",
            "--purposes", "investigation",
            "--bind", "127.0.0.1:1003", "--health-bind", "127.0.0.1:1005", "--observe-bind", "127.0.0.1:1004",
            "--engine", "llama-cpp", "--model-dir", "/R/models/granite",
            "--model-trust", "/R/models/trust/model-signing.pub", "--sidecar", "127.0.0.1:1008",
        ])
        XCTAssertEqual(mimir.readiness, .logLine("\"serving\""))
        XCTAssertEqual(mimir.readyTimeout, 180)
    }

    func testGateway() throws {
        let gateway = try spec("gateway", specs())
        XCTAssertEqual(gateway.executable.path, "/R/bin/http-gateway")
        XCTAssertEqual(gateway.arguments, [
            "--state-dir", "/H/airports", "--bind", "127.0.0.1:1001",
            "--allow-origin", "http://127.0.0.1:2000", "--token-ttl", "43200",
            "--mesh-bind", "127.0.0.1:1002", "--mesh-identity-dir", "/H/airports-mesh/gateway",
            "--heimdall-addr", "127.0.0.1:1006", "--heimdall-identity-dir", "/H/airports-mesh/feeds",
            "--mimir-observe-addr", "127.0.0.1:1004", "--mimir-identity-dir", "/H/airports-mesh/mimir-observer",
            "--forge-jobs", "/H/live-feeds/data/jobs.json",
            "--forge-status", "/H/live-feeds/worker-state/status.json",
            "--heimdall-feeds-status", "/H/heimdall-poc/feeds/state/status.json",
        ])
        XCTAssertEqual(gateway.readiness, .logLine("listening on"))
    }

    func testHeimdall() throws {
        let heimdall = try spec("heimdall", specs())
        XCTAssertEqual(heimdall.executable.path, "/R/bin/heimdall-feed")
        XCTAssertEqual(heimdall.arguments, [
            "--identity-dir", "/H/airports-mesh/heimdall", "--state-dir", "/H/heimdall-poc/state",
            "--bind", "127.0.0.1:1006", "--health-bind", "127.0.0.1:1007", "--track-ttl", "300",
            "--mimir-addr", "127.0.0.1:1003", "--mimir-identity-dir", "/H/airports-mesh/heimdall-advisor",
            "--fabric-addr", "127.0.0.1:1002", "--fabric-identity-dir", "/H/airports-mesh/heimdall-fabric",
        ])
        XCTAssertEqual(heimdall.readiness, .logLine("\"serving\""))
    }

    func testForge() throws {
        let forge = try spec("forge", specs())
        XCTAssertEqual(forge.executable.path, "/R/bin/forge-worker")
        XCTAssertEqual(forge.arguments, [
            "--identity-dir", "/H/airports-mesh/forge", "--state-dir", "/H/live-feeds/worker-state",
            "--jobs", "/H/live-feeds/data/jobs.json", "--gateway", "127.0.0.1:1002",
        ])
        XCTAssertEqual(forge.readiness, .logLine("\"message\":\"job run\""))
    }

    func testEveryServiceHas180SecondsToBeReady() {
        XCTAssertEqual(specs(recording: URL(fileURLWithPath: "/R/recording")).map(\.readyTimeout),
                       Array(repeating: 180, count: 6))
        XCTAssertEqual(ServiceSpec(name: "x", executable: bin, arguments: [], readiness: .logLine("x"),
                                   logFile: logs).readyTimeout, 180)
    }

    func testNoAnalyticsArguments() {
        let all = specs(recording: URL(fileURLWithPath: "/R/recording"))
        for spec in all {
            XCTAssertFalse(spec.arguments.contains { $0.contains("analytics") }, spec.name)
        }
    }

    func testPortsFromAList() throws {
        XCTAssertEqual(SuitePorts.count, 8)
        let made = try XCTUnwrap(SuitePorts([1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008]))
        XCTAssertEqual(made, ports)
        XCTAssertNil(SuitePorts([1, 2, 3]))
        let free = try SuitePorts.free()
        XCTAssertEqual(Set(free.all).count, 8)
    }
}
