import Foundation
import XCTest
@testable import MidgardShellCore

final class ServiceSpecTests: XCTestCase {
    let bin = URL(fileURLWithPath: "/R/bin", isDirectory: true)
    let models = URL(fileURLWithPath: "/R/models", isDirectory: true)
    let home = URL(fileURLWithPath: "/H", isDirectory: true)
    let logs = URL(fileURLWithPath: "/L", isDirectory: true)
    let ports = SuitePorts(gatewayHTTP: 1001, gatewayMesh: 1002, mimirMesh: 1003, mimirObserver: 1004,
                           mimirHealth: 1005, heimdallMesh: 1006, heimdallHealth: 1007, llama: 1008,
                           analytics: 1009)
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

    func testNoAnalyticsWithoutTheBundledAnalytics() {
        let all = specs(recording: URL(fileURLWithPath: "/R/recording"))
        for spec in all {
            XCTAssertFalse(spec.arguments.contains { $0.contains("analytics") }, spec.name)
        }
    }

    // MARK: camera analytics

    let recording = URL(fileURLWithPath: "/R/recording", isDirectory: true)
    var analytics: CameraAnalytics {
        CameraAnalytics(root: URL(fileURLWithPath: "/R/analytics", isDirectory: true),
                        models: URL(fileURLWithPath: "/R/models/yolox", isDirectory: true), recording: recording)
    }

    func withAnalytics(_ hardware: ModelServer.Hardware? = nil) -> [ServiceSpec] {
        SuiteSpecs.make(bin: bin, models: models, home: home, logs: logs, ports: ports, webOrigin: origin,
                        recording: recording, hardware: hardware ?? appleSilicon, analytics: analytics)
    }

    func analyticsCommon(_ tail: [String]) -> [String] {
        ["-I", "-B", "/R/analytics/analytics.py", "--serve",
         "--port", "1009", "--token-file", "/H/heimdall-poc/analytics/secret",
         "--state-dir", "/H/heimdall-poc/analytics", "--log", "-", "--config", "/R/analytics/config.json",
         "--model-dir", "/R/models/yolox"] + tail + [
         "--frames-helper", "/R/bin/midgard-frames", "--recording", "/R/recording",
         "--source-bin", "/R/bin/heimdall-source", "--source-identity-dir", "/H/airports-mesh/feeds",
         "--source-addr", "127.0.0.1:1006", "--nice", "10"]
    }

    func testAnalyticsStartLastAndOptional() {
        let all = withAnalytics()
        XCTAssertEqual(all.map(\.name),
                       ["llama", "mimir", "gateway", "heimdall", "forge", "replay", "analytics"])
        XCTAssertEqual(all.map(\.readyTimeout), Array(repeating: 180, count: 7))
        XCTAssertEqual(all.filter(\.optional).map(\.name), ["analytics"])
        XCTAssertTrue(specs(recording: recording).allSatisfy { !$0.optional })
    }

    func testAnalyticsOnAppleSiliconUseTheArm64RuntimeWithCoreML() throws {
        let a = try spec("analytics", withAnalytics())
        XCTAssertEqual(a.executable.path, "/R/analytics/runtime/arm64/bin/python3.13")
        XCTAssertEqual(a.arguments, analyticsCommon(["--provider", "coreml", "--threads", "2"]))
        XCTAssertEqual(a.readiness, .logLine("SERVE pid="))
        XCTAssertEqual(a.logFile.path, "/L/analytics.log")
        XCTAssertNil(a.environment)
    }

    func testAnalyticsOnIntelUseTheX64RuntimeOnTheCPU() throws {
        let a = try spec("analytics", withAnalytics(.init(arm64: false, physicalCores: 8)))
        XCTAssertEqual(a.executable.path, "/R/analytics/runtime/x64/bin/python3.13")
        XCTAssertEqual(a.arguments, analyticsCommon(["--provider", "cpu", "--threads", "4"]))
        let small = try spec("analytics", withAnalytics(.init(arm64: false, physicalCores: 2)))
        XCTAssertEqual(small.arguments, analyticsCommon(["--provider", "cpu", "--threads", "2"]))
        // MIDGARD_FORCE_INTEL_MODEL on Apple Silicon picks the x64 runtime, as the model server.
        let forced = ModelServer.Hardware.from(
            sysctl: { ["hw.optional.arm64": 1, "hw.physicalcpu": 10][$0] }, activeProcessors: 10,
            environment: ["MIDGARD_FORCE_INTEL_MODEL": "1", "MIDGARD_MODEL_THREADS": "4"])
        let f = try spec("analytics", withAnalytics(forced))
        XCTAssertEqual(f.executable.path, "/R/analytics/runtime/x64/bin/python3.13")
        XCTAssertEqual(f.arguments, analyticsCommon(["--provider", "cpu", "--threads", "4"]))
    }

    func testGatewayIsGivenTheAnalytics() throws {
        let gateway = try spec("gateway", withAnalytics())
        XCTAssertEqual(Array(gateway.arguments.suffix(4)), [
            "--analytics-addr", "127.0.0.1:1009", "--analytics-token-file", "/H/heimdall-poc/analytics/secret",
        ])
        let plain = try spec("gateway", specs(recording: recording))
        XCTAssertEqual(gateway.arguments.dropLast(4), plain.arguments[...])
        // Everything else is as without the analytics.
        let others = withAnalytics().filter { !["analytics", "gateway"].contains($0.name) }
        XCTAssertEqual(others, specs(recording: recording).filter { $0.name != "gateway" })
    }

    func testLocateNeedsEverythingAndARecording() {
        let resources = URL(fileURLWithPath: "/R", isDirectory: true)
        let files: Set<String> = ["/R/analytics/analytics.py", "/R/analytics/config.json",
                                  "/R/models/yolox/yolox_s.onnx"]
        let executables: Set<String> = ["/R/analytics/runtime/arm64/bin/python3.13",
                                        "/R/bin/midgard-frames", "/R/bin/heimdall-source"]
        func locate(recording: URL?, hardware: ModelServer.Hardware, files: Set<String> = files,
                    executables: Set<String> = executables) -> CameraAnalytics? {
            CameraAnalytics.locate(resources: resources, helpers: bin, recording: recording, hardware: hardware,
                                   isExecutable: { executables.contains($0) }, exists: { files.contains($0) })
        }
        XCTAssertEqual(locate(recording: recording, hardware: appleSilicon), analytics)
        for helper in ["/R/bin/midgard-frames", "/R/bin/heimdall-source"] {
            XCTAssertNil(locate(recording: recording, hardware: appleSilicon,
                                executables: executables.subtracting([helper])), helper)
        }
        XCTAssertNil(locate(recording: nil, hardware: appleSilicon))
        XCTAssertNil(locate(recording: recording, hardware: .init(arm64: false, physicalCores: 4)), "no x64 runtime")
        for missing in files {
            XCTAssertNil(locate(recording: recording, hardware: appleSilicon, files: files.subtracting([missing])), missing)
        }
        XCTAssertEqual(CameraAnalytics.runtimes(root: analytics.root).path, "/R/analytics/runtime")
    }

    func testTokenIsCreatedOnceWithOwnerOnlyAccess() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("token-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = CameraAnalytics.tokenFile(home: dir)
        XCTAssertEqual(file.path, dir.appendingPathComponent("heimdall-poc/analytics/secret").path)
        try CameraAnalytics.ensureToken(at: file)
        let token = try String(contentsOf: file, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
        XCTAssertEqual(token.count, 48)
        XCTAssertTrue(token.allSatisfy { $0.isHexDigit })
        let mode = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int
        XCTAssertEqual(mode, 0o600)
        try CameraAnalytics.ensureToken(at: file)
        XCTAssertEqual(try String(contentsOf: file, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines), token)
        // An empty file is replaced.
        try Data().write(to: file)
        try CameraAnalytics.ensureToken(at: file)
        XCTAssertEqual(try String(contentsOf: file, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines).count, 48)
        // A kept token's file is tightened to 0600.
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        let kept = try String(contentsOf: file, encoding: .utf8)
        try CameraAnalytics.ensureToken(at: file)
        XCTAssertEqual(try String(contentsOf: file, encoding: .utf8), kept)
        XCTAssertEqual(try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int, 0o600)
    }

    /// A symlink at the token's path is replaced by a file; its target is not touched.
    func testTokenReplacesASymlinkAndRefusesAFolder() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("token-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let file = CameraAnalytics.tokenFile(home: dir)
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        let target = dir.appendingPathComponent("elsewhere")
        try Data("victim-token\n".utf8).write(to: target)
        try FileManager.default.createSymbolicLink(at: file, withDestinationURL: target)
        try CameraAnalytics.ensureToken(at: file)
        let attrs = try FileManager.default.attributesOfItem(atPath: file.path)
        XCTAssertEqual(attrs[.type] as? FileAttributeType, .typeRegular)
        XCTAssertEqual(attrs[.posixPermissions] as? Int, 0o600)
        XCTAssertEqual(try String(contentsOf: target, encoding: .utf8), "victim-token\n")
        XCTAssertNotEqual(try String(contentsOf: file, encoding: .utf8), "victim-token\n")
        // A folder there is an error, not something to delete.
        try FileManager.default.removeItem(at: file)
        try FileManager.default.createDirectory(at: file, withIntermediateDirectories: false)
        XCTAssertThrowsError(try CameraAnalytics.ensureToken(at: file))
        XCTAssertTrue(FileManager.default.fileExists(atPath: file.path))
    }

    func testPortsFromAList() throws {
        XCTAssertEqual(SuitePorts.count, 9)
        let made = try XCTUnwrap(SuitePorts([1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009]))
        XCTAssertEqual(made, ports)
        XCTAssertNil(SuitePorts([1, 2, 3]))
        let free = try SuitePorts.free()
        XCTAssertEqual(Set(free.all).count, 9)
    }
}
