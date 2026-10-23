import Darwin
import Foundation
import XCTest
@testable import MidgardShellCore

/// Services left running by a shell that crashed: started here by plain `Process`es from a
/// temp bin dir (copies of system tools, re-signed ad hoc so they run from there).
final class ReapOrphansTests: XCTestCase {
    var dir: URL!
    var bin: URL!
    var processes: [Process] = []

    override func setUpWithError() throws {
        dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("midgard-reap-\(UUID().uuidString)", isDirectory: true)
        bin = dir.appendingPathComponent("bin", isDirectory: true)
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        processes.filter(\.isRunning).forEach { kill($0.processIdentifier, SIGKILL) }
        try? FileManager.default.removeItem(at: dir)
    }

    /// Copies a system tool into `bin` (or `folder`) under `name`.
    func install(_ tool: String, as name: String, in folder: URL? = nil) throws -> URL {
        let url = (folder ?? bin).appendingPathComponent(name)
        try FileManager.default.copyItem(at: URL(fileURLWithPath: tool), to: url)
        let sign = Process()
        sign.executableURL = URL(fileURLWithPath: "/usr/bin/codesign")
        sign.arguments = ["-f", "-s", "-", url.path]
        sign.standardOutput = FileHandle.nullDevice
        sign.standardError = FileHandle.nullDevice
        try sign.run()
        sign.waitUntilExit()
        XCTAssertEqual(sign.terminationStatus, 0, "codesign \(name)")
        return url
    }

    func launch(_ executable: URL, _ arguments: [String], output: URL? = nil) throws -> Process {
        let process = Process()
        process.executableURL = executable
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        if let output {
            FileManager.default.createFile(atPath: output.path, contents: nil)
            process.standardOutput = try FileHandle(forWritingTo: output)
        } else {
            process.standardOutput = FileHandle.nullDevice
        }
        try process.run()
        processes.append(process)
        return process
    }

    func testReapsOnlyProcessesFromTheBinDir() async throws {
        let worker = try install("/bin/sleep", as: "forge-worker")
        let replay = try install("/bin/bash", as: "midgard-replay")
        let stubborn = try install("/bin/bash", as: "heimdall-feed")
        let outsider = try launch(URL(fileURLWithPath: "/bin/sleep"), ["300"])

        let a = try launch(worker, ["300"])
        let childFile = dir.appendingPathComponent("child")
        let r = try launch(replay, ["-c", "/bin/sleep 300 & echo $!; wait"], output: childFile)
        let s = try launch(stubborn, ["-c", "trap '' TERM; while :; do /bin/sleep 0.05; done"])
        var child: pid_t?
        for _ in 0..<100 where child == nil {
            child = pid_t((try? String(contentsOf: childFile, encoding: .utf8))?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? "")
            if child == nil { try await Task.sleep(nanoseconds: 20_000_000) }
        }
        let replayChild = try XCTUnwrap(child)
        try await Task.sleep(nanoseconds: 200_000_000)  // let bash install its trap

        let started = Date()
        let reaped = await Supervisor.reapOrphans(binDir: bin, grace: 1)
        XCTAssertLessThan(Date().timeIntervalSince(started), 5)

        let expected = [r.processIdentifier, a.processIdentifier, s.processIdentifier]
        // A superset: the stubborn shell's fork, caught before it execs sleep, counts too.
        XCTAssertTrue(Set(reaped.map(\.pid)).isSuperset(of: expected), "\(reaped)")
        XCTAssertEqual(reaped.first?.pid, r.processIdentifier, "replay first")
        let root = Supervisor.realPath(bin) + "/"
        XCTAssertTrue(reaped.allSatisfy { $0.executable.hasPrefix(root) }, "\(reaped)")
        for pid in expected + [replayChild] {
            let isGone = await gone(pid)
            XCTAssertTrue(isGone, "pid \(pid) still exists")
        }
        XCTAssertTrue(outsider.isRunning, "a process from elsewhere is left alone")
        let none = await Supervisor.reapOrphans(binDir: bin, grace: 1)
        XCTAssertEqual(none, [])
    }

    /// The camera analytics' Python runs from Resources/analytics/runtime, outside the
    /// helpers: it is ended when that folder is given, and only then.
    func testReapsProcessesUnderTheAnalyticsRuntimes() async throws {
        let runtimes = dir.appendingPathComponent("analytics/runtime", isDirectory: true)
        let arm = runtimes.appendingPathComponent("arm64/bin", isDirectory: true)
        try FileManager.default.createDirectory(at: arm, withIntermediateDirectories: true)
        let python = try install("/bin/sleep", as: "python3.13", in: arm)
        let p = try launch(python, ["300"])
        try await Task.sleep(nanoseconds: 100_000_000)
        let without = await Supervisor.reapOrphans(binDir: bin, grace: 1)
        XCTAssertEqual(without, [])
        XCTAssertTrue(p.isRunning)
        let reaped = await Supervisor.reapOrphans(binDir: bin, alsoUnder: [runtimes], grace: 1)
        XCTAssertEqual(reaped.map(\.pid), [p.processIdentifier])
        let isGone = await gone(p.processIdentifier)
        XCTAssertTrue(isGone)
    }

    /// The model server runs from outside the bin dir (Homebrew's llama-server): it is
    /// found by its name, llama-server, together with an argument that starts with the
    /// bundle's models path. Anything else stays: a user's editor, shasum or shell on a
    /// model file, a llama-server on another model, a marker only in the environment.
    func testReapsOnlyALlamaServerGivenTheModelsPath() async throws {
        let marker = "/tmp/midgard-models-\(UUID().uuidString)/"
        let model = marker + "granite/model.gguf"
        let elsewhere = dir.appendingPathComponent("homebrew", isDirectory: true)
        try FileManager.default.createDirectory(at: elsewhere, withIntermediateDirectories: true)
        let llama = try install("/bin/bash", as: "llama-server", in: elsewhere)
        let viewer = try install("/bin/bash", as: "less", in: elsewhere)
        let loop = "while :; do /bin/sleep 0.2; done"

        let server = try launch(llama, ["-c", loop, "llama-server", "-m", model])
        let otherModel = try launch(llama, ["-c", loop, "llama-server", "-m", "/elsewhere/model.gguf"])
        let embedded = try launch(llama, ["-c", loop, "llama-server", "--x=" + model])
        let notLlama = try launch(viewer, ["-c", loop, "less", model])
        let shell = try launch(URL(fileURLWithPath: "/bin/sh"), ["-c", loop, "sh", model])
        let byEnv = Process()
        byEnv.executableURL = llama
        byEnv.arguments = ["-c", loop]
        byEnv.environment = ["MIDGARD_TEST_MODEL": model]
        byEnv.standardOutput = FileHandle.nullDevice
        try byEnv.run()
        processes.append(byEnv)
        try await Task.sleep(nanoseconds: 300_000_000)

        XCTAssertEqual(Supervisor.arguments(of: server.processIdentifier),
                       [llama.path, "-c", loop, "llama-server", "-m", model])
        let reaped = await Supervisor.reapOrphans(binDir: bin, modelsPrefix: marker, grace: 1)
        XCTAssertEqual(reaped.map(\.pid), [server.processIdentifier], "\(reaped)")
        let isGone = await gone(server.processIdentifier)
        XCTAssertTrue(isGone)
        for (process, what) in [(otherModel, "a llama-server on another model"),
                                (embedded, "the path inside an argument, not at its start"),
                                (notLlama, "another program on the model file"),
                                (shell, "a shell given the model path"),
                                (byEnv, "the path only in the environment")] {
            XCTAssertTrue(process.isRunning, "\(what) is left alone")
        }
    }
}
