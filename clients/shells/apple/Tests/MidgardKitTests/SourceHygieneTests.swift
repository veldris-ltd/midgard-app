import Foundation
import XCTest

/// MidgardKit logs nothing: no `print`, `NSLog`, `os_log` or `Logger` anywhere in its
/// sources, so no key material, sealed bytes or persona data can reach a log.
/// [FR-C-9] [AC-C-2]
final class SourceHygieneTests: XCTestCase {
    func testMidgardKitHasNoLogging() throws {
        let sources = repositoryRoot().appendingPathComponent("clients/shells/apple/Sources/MidgardKit")
        let files = try XCTUnwrap(FileManager.default.enumerator(at: sources, includingPropertiesForKeys: nil))
            .compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" }
        XCTAssertGreaterThan(files.count, 10)
        let pattern = try NSRegularExpression(pattern: #"\b(print|debugPrint|NSLog|os_log|dump)\s*\(|\bLogger\s*\(|import\s+os\b"#)
        for file in files {
            let text = try String(contentsOf: file, encoding: .utf8)
            let hits = pattern.numberOfMatches(in: text, range: NSRange(text.startIndex..., in: text))
            XCTAssertEqual(hits, 0, "\(file.lastPathComponent) logs")
        }
    }

    /// MidgardKit reaches the network in one place only: `UserHTTPClient`, for the hosts
    /// the user adds in Your Data (the tile loader is the edition library's). No other
    /// source makes a session, a task or a socket. [NFR-PRV-1]
    func testMidgardKitReachesTheNetworkOnlyThroughUserHTTP() throws {
        let sources = repositoryRoot().appendingPathComponent("clients/shells/apple/Sources/MidgardKit")
        let files = try XCTUnwrap(FileManager.default.enumerator(at: sources, includingPropertiesForKeys: nil))
            .compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" }
        let pattern = try NSRegularExpression(pattern: #"\bURLSession\b|\bNWConnection\b|\bNWListener\b|\bCFStream|\bdataTask\(|contentsOf:\s*URL\(string"#)
        var users: [String] = []
        for file in files {
            let text = try String(contentsOf: file, encoding: .utf8)
            if pattern.numberOfMatches(in: text, range: NSRange(text.startIndex..., in: text)) > 0 {
                users.append(file.lastPathComponent)
            }
        }
        XCTAssertEqual(users, ["UserHTTP.swift"])
    }
}
