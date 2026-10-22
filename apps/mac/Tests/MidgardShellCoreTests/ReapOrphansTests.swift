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

    /// Copies a system tool into `bin` under `name`.
    func install(_ tool: String, as name: String) throws -> URL {
        let url = bin.appendingPathComponent(name)
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

    /// The model server runs from outside the bin dir (Homebrew's llama-server): it is
    /// found by an argument holding the bundle's models path. Only the arguments count,
    /// not the environment (the services inherit the shell's).
    func testReapsByArgumentMarkerButNotByEnvironment() async throws {
        let marker = "/tmp/midgard-models-\(UUID().uuidString)/granite/model.gguf"
        let sh = URL(fileURLWithPath: "/bin/sh")
        let model = try launch(sh, ["-c", "while :; do /bin/sleep 0.2; done", "-m", marker])
        let other = try launch(sh, ["-c", "while :; do /bin/sleep 0.2; done", "-m", "/elsewhere/model.gguf"])
        let byEnv = Process()
        byEnv.executableURL = sh
        byEnv.arguments = ["-c", "while :; do /bin/sleep 0.2; done"]
        byEnv.environment = ["MIDGARD_TEST_MODEL": marker]
        byEnv.standardOutput = FileHandle.nullDevice
        try byEnv.run()
        processes.append(byEnv)
        try await Task.sleep(nanoseconds: 200_000_000)

        XCTAssertEqual(Supervisor.arguments(of: model.processIdentifier),
                       ["/bin/sh", "-c", "while :; do /bin/sleep 0.2; done", "-m", marker])
        let reaped = await Supervisor.reapOrphans(binDir: bin, argumentContaining: marker, grace: 1)
        XCTAssertEqual(reaped.map(\.pid), [model.processIdentifier], "\(reaped)")
        let isGone = await gone(model.processIdentifier)
        XCTAssertTrue(isGone)
        XCTAssertTrue(other.isRunning, "another model path is left alone")
        XCTAssertTrue(byEnv.isRunning, "the marker in the environment does not count")
    }
}
