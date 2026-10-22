import Foundation
import XCTest
@testable import MidgardShellCore

final class ModelServerTests: XCTestCase {
    let helpers = URL(fileURLWithPath: "/App/Contents/Helpers", isDirectory: true)
    let models = URL(fileURLWithPath: "/App/Contents/Resources/models", isDirectory: true)

    func choose(arm64: Bool, cores: Int) -> ModelServer {
        ModelServer.choose(hardware: .init(arm64: arm64, physicalCores: cores),
                           helpers: helpers, models: models, port: 4321)
    }

    func tail(_ server: ModelServer) -> [String] {
        let i = server.arguments.firstIndex(of: "--n-gpu-layers")!
        return Array(server.arguments[i...])
    }

    func testAppleSiliconUsesTheArm64BuildWithMetal() {
        let server = choose(arm64: true, cores: 10)
        XCTAssertEqual(server.executable.path, "/App/Contents/Helpers/llama/arm64/llama-server")
        XCTAssertEqual(tail(server), ["--n-gpu-layers", "99"])
        XCTAssertFalse(server.arguments.contains("--threads"))
    }

    func hardware(_ values: [String: Int], active: Int = 99) -> ModelServer.Hardware {
        ModelServer.Hardware.from(sysctl: { values[$0] }, activeProcessors: active)
    }

    func testAppleSiliconNative() {
        let hw = hardware(["hw.optional.arm64": 1, "sysctl.proc_translated": 0, "hw.physicalcpu": 10])
        XCTAssertEqual(hw, .init(arm64: true, physicalCores: 10))
        XCTAssertEqual(ModelServer.choose(hardware: hw, helpers: helpers, models: models, port: 1).executable.path,
                       "/App/Contents/Helpers/llama/arm64/llama-server")
    }

    func testAppleSiliconUnderRosettaStillUsesArm64() {
        // A translated process may not see hw.optional.arm64; proc_translated says Rosetta.
        for values in [["sysctl.proc_translated": 1, "hw.physicalcpu": 8],
                       ["hw.optional.arm64": 0, "sysctl.proc_translated": 1, "hw.physicalcpu": 8]] {
            let hw = hardware(values)
            XCTAssertEqual(hw, .init(arm64: true, physicalCores: 8))
            let server = ModelServer.choose(hardware: hw, helpers: helpers, models: models, port: 1)
            XCTAssertEqual(server.executable.path, "/App/Contents/Helpers/llama/arm64/llama-server")
            XCTAssertEqual(tail(server), ["--n-gpu-layers", "99"])
        }
    }

    func testIntelHardware() {
        // Intel Macs have no hw.optional.arm64; proc_translated is 0.
        XCTAssertEqual(hardware(["sysctl.proc_translated": 0, "hw.physicalcpu": 2]),
                       .init(arm64: false, physicalCores: 2))
        XCTAssertEqual(hardware(["hw.physicalcpu": 4]), .init(arm64: false, physicalCores: 4))
        XCTAssertEqual(hardware([:], active: 4), .init(arm64: false, physicalCores: 4))
    }

    func testIntelUsesTheX64BuildOnTheCPU() {
        for cores in [2, 4] {
            let server = choose(arm64: false, cores: cores)
            XCTAssertEqual(server.executable.path, "/App/Contents/Helpers/llama/x64/llama-server")
            XCTAssertEqual(tail(server), ["--n-gpu-layers", "0", "--threads", "\(cores)"])
        }
    }

    func testAtLeastOneThread() {
        XCTAssertEqual(tail(choose(arm64: false, cores: 0)), ["--n-gpu-layers", "0", "--threads", "1"])
    }

    func testCommonArguments() {
        let server = choose(arm64: false, cores: 2)
        XCTAssertEqual(Array(server.arguments.prefix(2)),
                       ["-m", "/App/Contents/Resources/models/granite/granite-3.3-2b-instruct-Q4_K_M.gguf"])
        XCTAssertTrue(server.arguments.contains("4321"))
        // Reaping matches the model server by this basename and the models path.
        XCTAssertTrue(Supervisor.isModelServer(server.executable.path))
    }

    func testThisMac() throws {
        let hw = ModelServer.Hardware.thisMac()
        XCTAssertGreaterThan(hw.physicalCores, 0)
        #if arch(arm64)
        XCTAssertTrue(hw.arm64)
        #endif
        // A translated (x86_64) test run on Apple Silicon must still report arm64 hardware.
        if ModelServer.sysctlInt("sysctl.proc_translated") == 1 { XCTAssertTrue(hw.arm64) }
    }
}
