import Foundation
import Security

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
    /// Seconds from launch to ready before the start counts as failed (180 s, as
    /// run-native.sh allows: a first start on a slow Mac can take a while).
    public var readyTimeout: TimeInterval
    /// Variables set on top of the shell's own environment; nil inherits it unchanged.
    public var environment: [String: String]?

    public init(name: String, executable: URL, arguments: [String], readiness: Readiness,
                logFile: URL, readyTimeout: TimeInterval = 180, environment: [String: String]? = nil) {
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
    /// The camera analytics' HTTP port (the gateway forwards to it).
    public var analytics: UInt16

    /// How many ports the suite needs.
    public static let count = 9

    public init(gatewayHTTP: UInt16, gatewayMesh: UInt16, mimirMesh: UInt16, mimirObserver: UInt16,
                mimirHealth: UInt16, heimdallMesh: UInt16, heimdallHealth: UInt16, llama: UInt16,
                analytics: UInt16) {
        self.gatewayHTTP = gatewayHTTP
        self.gatewayMesh = gatewayMesh
        self.mimirMesh = mimirMesh
        self.mimirObserver = mimirObserver
        self.mimirHealth = mimirHealth
        self.heimdallMesh = heimdallMesh
        self.heimdallHealth = heimdallHealth
        self.llama = llama
        self.analytics = analytics
    }

    /// From exactly `count` ports, in the order of the properties; nil for any other count.
    public init?(_ ports: [UInt16]) {
        guard ports.count == Self.count else { return nil }
        self.init(gatewayHTTP: ports[0], gatewayMesh: ports[1], mimirMesh: ports[2],
                  mimirObserver: ports[3], mimirHealth: ports[4], heimdallMesh: ports[5],
                  heimdallHealth: ports[6], llama: ports[7], analytics: ports[8])
    }

    /// Fresh free ports (see `Ports.free`).
    public static func free() throws -> SuitePorts {
        // Ports.free returns exactly the count asked for.
        SuitePorts(try Ports.free(count: count))!
    }

    public var all: [UInt16] {
        [gatewayHTTP, gatewayMesh, mimirMesh, mimirObserver, mimirHealth, heimdallMesh, heimdallHealth, llama,
         analytics]
    }
}

/// The camera analytics as the app bundles them (heimdall/analytics/README.md): the code
/// and a Python runtime per chip in `Resources/analytics`, the model in
/// `Resources/models/yolox`, the cameras and their stored media from the recording.
public struct CameraAnalytics: Sendable, Equatable {
    /// `analytics.py`, `config.json` and `runtime/{arm64,x64}/bin/python3.13`.
    public var root: URL
    /// The folder with `yolox_s.onnx`.
    public var models: URL
    /// The recording the replay plays: its land-camera observations are the cameras.
    public var recording: URL

    public init(root: URL, models: URL, recording: URL) {
        self.root = root
        self.models = models
        self.recording = recording
    }

    /// Where the runtimes are (`reapOrphans` also ends processes running from here).
    public static func runtimes(root: URL) -> URL {
        root.appendingPathComponent("runtime", isDirectory: true)
    }

    /// The arm64 runtime on Apple Silicon (also under Rosetta), the x64 one on Intel or with
    /// MIDGARD_FORCE_INTEL_MODEL=1, as the model server.
    public static func python(hardware: ModelServer.Hardware, root: URL) -> URL {
        runtimes(root: root).appendingPathComponent(hardware.arm64 ? "arm64" : "x64", isDirectory: true)
            .appendingPathComponent("bin/python3.13")
    }

    /// The bundled analytics for `hardware` if everything is there (code, config, runtime,
    /// model) and there is a recording; nil otherwise.
    public static func locate(resources: URL, recording: URL?, hardware: ModelServer.Hardware,
                              isExecutable: (String) -> Bool,
                              exists: (String) -> Bool) -> CameraAnalytics? {
        guard let recording else { return nil }
        let root = resources.appendingPathComponent("analytics", isDirectory: true)
        let models = resources.appendingPathComponent("models/yolox", isDirectory: true)
        guard isExecutable(python(hardware: hardware, root: root).path),
              exists(root.appendingPathComponent("analytics.py").path),
              exists(root.appendingPathComponent("config.json").path),
              exists(models.appendingPathComponent("yolox_s.onnx").path) else { return nil }
        return CameraAnalytics(root: root, models: models, recording: recording)
    }

    /// The gateway's bearer token for the service, in the working copy (where run-native.sh
    /// keeps the development setup's).
    public static func tokenFile(home: URL) -> URL {
        home.appendingPathComponent("heimdall-poc/analytics/secret")
    }

    /// Creates the token file (0600, 48 hex digits from the system's random source) unless it
    /// holds one already; the gateway reads it when it starts, so this runs first.
    public static func ensureToken(at file: URL) throws {
        let fm = FileManager.default
        if let text = try? String(contentsOf: file, encoding: .utf8),
           !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return }
        try fm.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true,
                               attributes: [.posixPermissions: 0o700])
        var bytes = [UInt8](repeating: 0, count: 24)
        guard SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes) == errSecSuccess else {
            throw CocoaError(.fileWriteUnknown)
        }
        let token = bytes.map { String(format: "%02x", $0) }.joined()
        try? fm.removeItem(at: file)
        guard fm.createFile(atPath: file.path, contents: Data((token + "\n").utf8),
                            attributes: [.posixPermissions: 0o600]) else {
            throw CocoaError(.fileWriteUnknown)
        }
    }
}

/// The suite's services as `tools/mac-app/run-native.sh start` runs them, with the
/// shell's ports. Differences: llama-server is one of them (as `mimir-model.sh` runs
/// it), the model files come from `models` rather than the home, the gateway allows only
/// the shell's web origin and issues 12-hour tokens, and the camera analytics (when
/// bundled) run offline on the recording's stored media.
public enum SuiteSpecs {
    /// In start order: llama, mimir, gateway, heimdall, analytics if `analytics`, forge,
    /// then replay if `recording`. llama is the bundled server `ModelServer.choose` picks for
    /// `hardware` under `bin` (`bin/llama/{arm64,x64}/llama-server`); analytics runs the
    /// runtime `CameraAnalytics.python` picks for it, and the gateway is given its address
    /// and token file.
    public static func make(bin: URL, models: URL, home: URL, logs: URL, ports: SuitePorts,
                            webOrigin: String, recording: URL?,
                            hardware: ModelServer.Hardware,
                            analytics: CameraAnalytics? = nil) -> [ServiceSpec] {
        func h(_ path: String) -> String { home.appendingPathComponent(path).path }
        func m(_ path: String) -> String { models.appendingPathComponent(path).path }
        func local(_ port: UInt16) -> String { "127.0.0.1:\(port)" }
        func spec(_ name: String, _ binary: String, _ readiness: ServiceSpec.Readiness,
                  _ arguments: [String]) -> ServiceSpec {
            spec(name, at: bin.appendingPathComponent(binary), readiness, arguments)
        }
        func spec(_ name: String, at executable: URL, _ readiness: ServiceSpec.Readiness,
                  _ arguments: [String]) -> ServiceSpec {
            ServiceSpec(name: name, executable: executable, arguments: arguments,
                        readiness: readiness, logFile: logs.appendingPathComponent("\(name).log"),
                        readyTimeout: 180)  // llama-server too
        }

        let llama = ModelServer.choose(hardware: hardware, helpers: bin, models: models, port: ports.llama)
        var specs = [
            spec("llama", at: llama.executable, .health(URL(string: "http://\(local(ports.llama))/health")!),
                 llama.arguments),
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
            ] + (analytics == nil ? [] : [
                "--analytics-addr", local(ports.analytics),
                "--analytics-token-file", CameraAnalytics.tokenFile(home: home).path,
            ])),
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
        if let analytics {
            func a(_ path: String) -> String { analytics.root.appendingPathComponent(path).path }
            // CoreML on Apple Silicon; on Intel the CPU, up to 4 threads (the model server
            // has the cores when it answers; the analytics run only when asked).
            let cpu = ["--provider", "cpu", "--threads", "\(min(4, max(1, hardware.physicalCores)))"]
            let index = specs.firstIndex { $0.name == "heimdall" }! + 1
            specs.insert(spec("analytics", at: CameraAnalytics.python(hardware: hardware, root: analytics.root),
                              .logLine("SERVE pid="), [
                // Isolated (no PYTHON* variables, no user site) and never writing .pyc
                // into the signed bundle.
                "-I", "-B", a("analytics.py"), "--serve",
                "--port", "\(ports.analytics)", "--token-file", CameraAnalytics.tokenFile(home: home).path,
                "--state-dir", h("heimdall-poc/analytics"), "--log", "-", "--config", a("config.json"),
                "--model-dir", analytics.models.path,
            ] + (hardware.arm64 ? ["--provider", "coreml", "--threads", "2"] : cpu) + [
                "--frames-helper", bin.appendingPathComponent("midgard-frames").path,
                "--recording", analytics.recording.path,
                "--source-bin", bin.appendingPathComponent("heimdall-source").path,
                "--source-identity-dir", h("airports-mesh/feeds"), "--source-addr", local(ports.heimdallMesh),
                "--nice", "10",
            ]), at: index)
        }
        if let recording {
            specs.append(spec("replay", "midgard-replay", .logLine("\"replay started\""), [
                "--recording", recording.path, "--identity-dir", h("airports-mesh/feeds"),
                "--forge-dir", h("live-feeds/data"),
                // Heimdall's mesh port: replay's default (7730) is run-native.sh's fixed port.
                "--addr", local(ports.heimdallMesh),
            ]))
        }
        return specs
    }
}
