import Foundation

/// How to run one native service and how to tell it is ready.
public struct ServiceSpec: Sendable, Equatable {
    public enum Readiness: Sendable, Equatable {
        /// Ready once this text appears in output written after the launch.
        case logLine(String)
        /// Ready once a GET of this URL answers 200.
        case health(URL)
    }

    public var name: String
    public var executable: URL
    public var arguments: [String]
    public var readiness: Readiness
    /// stdout and stderr are appended here (created if needed).
    public var logFile: URL
    /// Seconds from launch to ready before the start counts as failed.
    public var readyTimeout: TimeInterval
    /// Variables set on top of the shell's own environment; nil inherits it unchanged.
    public var environment: [String: String]?

    public init(name: String, executable: URL, arguments: [String], readiness: Readiness,
                logFile: URL, readyTimeout: TimeInterval = 60, environment: [String: String]? = nil) {
        self.name = name
        self.executable = executable
        self.arguments = arguments
        self.readiness = readiness
        self.logFile = logFile
        self.readyTimeout = readyTimeout
        self.environment = environment
    }
}

/// The loopback ports the shell chooses for the suite (the web server picks its own).
public struct SuitePorts: Sendable, Equatable {
    public var gatewayHTTP: UInt16
    public var gatewayMesh: UInt16
    public var mimirMesh: UInt16
    public var mimirObserver: UInt16
    public var mimirHealth: UInt16
    public var heimdallMesh: UInt16
    public var heimdallHealth: UInt16
    public var llama: UInt16

    /// How many ports the suite needs.
    public static let count = 8

    public init(gatewayHTTP: UInt16, gatewayMesh: UInt16, mimirMesh: UInt16, mimirObserver: UInt16,
                mimirHealth: UInt16, heimdallMesh: UInt16, heimdallHealth: UInt16, llama: UInt16) {
        self.gatewayHTTP = gatewayHTTP
        self.gatewayMesh = gatewayMesh
        self.mimirMesh = mimirMesh
        self.mimirObserver = mimirObserver
        self.mimirHealth = mimirHealth
        self.heimdallMesh = heimdallMesh
        self.heimdallHealth = heimdallHealth
        self.llama = llama
    }

    /// From exactly `count` ports, in the order of the properties; nil for any other count.
    public init?(_ ports: [UInt16]) {
        guard ports.count == Self.count else { return nil }
        self.init(gatewayHTTP: ports[0], gatewayMesh: ports[1], mimirMesh: ports[2],
                  mimirObserver: ports[3], mimirHealth: ports[4], heimdallMesh: ports[5],
                  heimdallHealth: ports[6], llama: ports[7])
    }

    /// Fresh free ports (see `Ports.free`).
    public static func free() throws -> SuitePorts {
        // Ports.free returns exactly the count asked for.
        SuitePorts(try Ports.free(count: count))!
    }

    public var all: [UInt16] {
        [gatewayHTTP, gatewayMesh, mimirMesh, mimirObserver, mimirHealth, heimdallMesh, heimdallHealth, llama]
    }
}

/// The suite's services as `tools/mac-app/run-native.sh start` runs them, with the
/// shell's ports. Differences: llama-server is one of them (as `mimir-model.sh` runs
/// it), the model files come from `models` rather than the home, the gateway allows only
/// the shell's web origin and issues 12-hour tokens, and there are no camera analytics.
public enum SuiteSpecs {
    /// In start order: llama, mimir, gateway, heimdall, forge, then replay if `recording`.
    public static func make(bin: URL, models: URL, home: URL, logs: URL, ports: SuitePorts,
                            webOrigin: String, recording: URL?) -> [ServiceSpec] {
        func h(_ path: String) -> String { home.appendingPathComponent(path).path }
        func m(_ path: String) -> String { models.appendingPathComponent(path).path }
        func local(_ port: UInt16) -> String { "127.0.0.1:\(port)" }
        func spec(_ name: String, _ binary: String, _ readiness: ServiceSpec.Readiness,
                  timeout: TimeInterval = 60, _ arguments: [String]) -> ServiceSpec {
            ServiceSpec(name: name, executable: bin.appendingPathComponent(binary), arguments: arguments,
                        readiness: readiness, logFile: logs.appendingPathComponent("\(name).log"),
                        readyTimeout: timeout)
        }

        var specs = [
            spec("llama", "llama-server", .health(URL(string: "http://\(local(ports.llama))/health")!),
                 timeout: 120, [
                    "-m", m("granite/granite-3.3-2b-instruct-Q4_K_M.gguf"),
                    "--alias", "granite-3.3-2b-instruct-q4km",
                    "--host", "127.0.0.1", "--port", "\(ports.llama)", "--ctx-size", "8192",
                    "--parallel", "1", "--temp", "0.2", "--json-schema", "{\"type\":\"object\"}",
                    "--n-gpu-layers", "99",
                 ]),
            spec("mimir", "mimir-server", .logLine("\"serving\""), [
                "--identity-dir", h("airports-mesh/mimir"), "--state-dir", h("mimir-state"),
                "--purposes", "investigation",
                "--bind", local(ports.mimirMesh), "--health-bind", local(ports.mimirHealth),
                "--observe-bind", local(ports.mimirObserver),
                "--engine", "llama-cpp", "--model-dir", m("granite"),
                "--model-trust", m("trust/model-signing.pub"), "--sidecar", local(ports.llama),
            ]),
            spec("gateway", "http-gateway", .logLine("listening on"), [
                "--state-dir", h("airports"), "--bind", local(ports.gatewayHTTP),
                "--allow-origin", webOrigin, "--token-ttl", "43200",
                "--mesh-bind", local(ports.gatewayMesh), "--mesh-identity-dir", h("airports-mesh/gateway"),
                "--heimdall-addr", local(ports.heimdallMesh), "--heimdall-identity-dir", h("airports-mesh/feeds"),
                "--mimir-observe-addr", local(ports.mimirObserver),
                "--mimir-identity-dir", h("airports-mesh/mimir-observer"),
                "--forge-jobs", h("live-feeds/data/jobs.json"),
                "--forge-status", h("live-feeds/worker-state/status.json"),
                "--heimdall-feeds-status", h("heimdall-poc/feeds/state/status.json"),
            ]),
            spec("heimdall", "heimdall-feed", .logLine("\"serving\""), [
                "--identity-dir", h("airports-mesh/heimdall"), "--state-dir", h("heimdall-poc/state"),
                "--bind", local(ports.heimdallMesh), "--health-bind", local(ports.heimdallHealth),
                "--track-ttl", "300",
                "--mimir-addr", local(ports.mimirMesh), "--mimir-identity-dir", h("airports-mesh/heimdall-advisor"),
                "--fabric-addr", local(ports.gatewayMesh), "--fabric-identity-dir", h("airports-mesh/heimdall-fabric"),
            ]),
            // Ready on the first successful job run ("job run failed" does not match).
            spec("forge", "forge-worker", .logLine("\"message\":\"job run\""), [
                "--identity-dir", h("airports-mesh/forge"), "--state-dir", h("live-feeds/worker-state"),
                "--jobs", h("live-feeds/data/jobs.json"), "--gateway", local(ports.gatewayMesh),
            ]),
        ]
        if let recording {
            specs.append(spec("replay", "midgard-replay", .logLine("\"replay started\""), [
                "--recording", recording.path, "--identity-dir", h("airports-mesh/feeds"),
                "--forge-dir", h("live-feeds/data"),
            ]))
        }
        return specs
    }
}
