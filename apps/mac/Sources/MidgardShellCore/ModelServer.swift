import Foundation

/// Which bundled llama-server the shell runs and how: the arm64 build with Metal on Apple
/// Silicon, the x64 build on the CPU on Intel. Kept pure (the hardware facts are inputs)
/// so the choice is testable; `Hardware.thisMac()` reads the real ones.
public struct ModelServer: Sendable, Equatable {
    /// `Helpers/llama/<arch>/llama-server` (its dylibs sit next to it).
    public var executable: URL
    /// Every llama-server argument (model, port, GPU layers, threads …).
    public var arguments: [String]

    /// The model file, relative to the models folder.
    public static let model = "granite/granite-3.3-2b-instruct-Q4_K_M.gguf"

    /// The facts the choice depends on.
    public struct Hardware: Sendable, Equatable {
        /// True on Apple Silicon even when the shell runs under Rosetta (Metal works there
        /// only for the arm64 build).
        public var arm64: Bool
        /// Physical cores: the Intel build's thread count.
        public var physicalCores: Int

        public init(arm64: Bool, physicalCores: Int) {
            self.arm64 = arm64
            self.physicalCores = physicalCores
        }

        /// This Mac's (see `from`).
        public static func thisMac() -> Hardware {
            from(sysctl: sysctlInt, activeProcessors: ProcessInfo.processInfo.activeProcessorCount)
        }

        /// Apple Silicon if `hw.optional.arm64` is 1 or the process is translated by Rosetta
        /// (`sysctl.proc_translated` 1: only Apple Silicon runs Rosetta); cores from
        /// `hw.physicalcpu`, else `activeProcessors`. `sysctl` returns nil for a missing name.
        static func from(sysctl: (String) -> Int?, activeProcessors: Int) -> Hardware {
            Hardware(arm64: sysctl("hw.optional.arm64") == 1 || sysctl("sysctl.proc_translated") == 1,
                     physicalCores: sysctl("hw.physicalcpu") ?? activeProcessors)
        }
    }

    /// Apple Silicon: `helpers/llama/arm64/llama-server`, all layers on the GPU (Metal).
    /// Intel: `helpers/llama/x64/llama-server`, CPU only, one thread per physical core
    /// (at least one).
    public static func choose(hardware: Hardware, helpers: URL, models: URL, port: UInt16) -> ModelServer {
        let arch = hardware.arm64 ? "arm64" : "x64"
        let executable = helpers.appendingPathComponent("llama", isDirectory: true)
            .appendingPathComponent(arch, isDirectory: true)
            .appendingPathComponent("llama-server")
        var arguments = [
            "-m", models.appendingPathComponent(model).path,
            "--alias", "granite-3.3-2b-instruct-q4km",
            "--host", "127.0.0.1", "--port", "\(port)", "--ctx-size", "8192",
            "--parallel", "1", "--temp", "0.2", "--json-schema", "{\"type\":\"object\"}",
        ]
        if hardware.arm64 {
            arguments += ["--n-gpu-layers", "99"]
        } else {
            arguments += ["--n-gpu-layers", "0", "--threads", "\(max(1, hardware.physicalCores))"]
        }
        return ModelServer(executable: executable, arguments: arguments)
    }

    /// A 32-bit integer sysctl, nil if the name does not exist on this Mac.
    static func sysctlInt(_ name: String) -> Int? {
        var value: Int32 = 0
        var size = MemoryLayout<Int32>.size
        guard sysctlbyname(name, &value, &size, nil, 0) == 0 else { return nil }
        return Int(value)
    }
}
