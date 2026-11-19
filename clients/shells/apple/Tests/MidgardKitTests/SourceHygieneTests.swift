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
}
