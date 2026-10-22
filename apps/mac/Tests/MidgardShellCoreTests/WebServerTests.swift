import Darwin
import Foundation
import XCTest
@testable import MidgardShellCore

/// A parsed HTTP response from the raw test client.
struct RawResponse {
    let status: Int
    let headers: [String: String]  // lower-cased names
    let body: Data
}

/// Sends `request` verbatim (URLSession would normalise `..` away) and reads to EOF.
func rawRequest(_ request: String, port: UInt16, host: String = "127.0.0.1") throws -> RawResponse {
    let fd = try connectTCP(host: host, port: port)
    defer { close(fd) }
    var timeout = timeval(tv_sec: 5, tv_usec: 0)
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
    let bytes = Array(request.utf8)
    XCTAssertEqual(bytes.withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }, bytes.count)
    var data = Data()
    var buffer = [UInt8](repeating: 0, count: 65536)
    while true {
        let n = read(fd, &buffer, buffer.count)
        if n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK) {
            throw NSError(domain: "raw", code: 2, userInfo: [NSLocalizedDescriptionKey:
                "timed out waiting for the response to \(request.prefix { $0 != "\r" })"])
        }
        if n <= 0 { break }
        data.append(buffer, count: n)
    }
    guard let split = data.range(of: Data("\r\n\r\n".utf8)) else {
        throw NSError(domain: "raw", code: 1, userInfo: [NSLocalizedDescriptionKey: "no header end"])
    }
    let head = String(decoding: data[..<split.lowerBound], as: UTF8.self).components(separatedBy: "\r\n")
    let status = Int(head[0].split(separator: " ")[1]) ?? 0
    var headers: [String: String] = [:]
    for line in head.dropFirst() {
        guard let colon = line.firstIndex(of: ":") else { continue }
        headers[line[..<colon].lowercased()] = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
    }
    return RawResponse(status: status, headers: headers, body: Data(data[split.upperBound...]))
}

func connectTCP(host: String, port: UInt16) throws -> Int32 {
    let fd = socket(AF_INET, SOCK_STREAM, 0)
    if fd == -1 { throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno)) }
    var addr = sockaddr_in()
    addr.sin_family = sa_family_t(AF_INET)
    addr.sin_port = port.bigEndian
    addr.sin_addr.s_addr = inet_addr(host)
    let rc = withUnsafePointer(to: &addr) {
        $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
            Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
        }
    }
    if rc != 0 {
        let code = errno
        close(fd)
        throw NSError(domain: NSPOSIXErrorDomain, code: Int(code))
    }
    return fd
}

/// The first non-loopback IPv4 address of this Mac, if it has one.
func nonLoopbackIPv4() -> String? {
    var list: UnsafeMutablePointer<ifaddrs>?
    guard getifaddrs(&list) == 0, let first = list else { return nil }
    defer { freeifaddrs(list) }
    for ifa in sequence(first: first, next: { $0.pointee.ifa_next }) {
        guard let sa = ifa.pointee.ifa_addr, sa.pointee.sa_family == sa_family_t(AF_INET),
              (ifa.pointee.ifa_flags & UInt32(IFF_LOOPBACK)) == 0,
              (ifa.pointee.ifa_flags & UInt32(IFF_UP)) != 0 else { continue }
        var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
        if getnameinfo(sa, socklen_t(sa.pointee.sa_len), &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0 {
            return String(decoding: host.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }, as: UTF8.self)
        }
    }
    return nil
}

final class WebServerTests: XCTestCase {
    var base: URL!
    var root: URL!
    var server: WebServer!
    var port: UInt16 = 0
    var big = Data()

    override func setUpWithError() throws {
        base = FileManager.default.temporaryDirectory
            .appendingPathComponent("midgard-web-\(UUID().uuidString)", isDirectory: true)
        root = base.appendingPathComponent("root", isDirectory: true)
        func put(_ path: String, _ data: Data) throws {
            let url = root.appendingPathComponent(path)
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try data.write(to: url)
        }
        try put("home/index.html", Data("<h1>home</h1>".utf8))
        try put("home/logo.svg", Data("<svg/>".utf8))
        try put("consoles/midgard/index.html", Data("<h1>midgard</h1>".utf8))
        try put("consoles/heimdall/assets/app.js", Data("console.log(1)".utf8))
        try put("consoles/heimdall/assets/app.css", Data("body{}".utf8))
        try put("consoles/forge/docs/index.html", Data("<h1>forge docs</h1>".utf8))
        big = Data(count: 5 * 1024 * 1024)
        big.withUnsafeMutableBytes { arc4random_buf($0.baseAddress, $0.count) }
        try put("consoles/mimir/data/big.i16", big)
        // Outside root: traversal must never reach it.
        try Data("secret".utf8).write(to: base.appendingPathComponent("secret.txt"))

        port = try Ports.free(count: 1)[0]
        server = WebServer(root: root, port: port)
        try server.start()
    }

    override func tearDownWithError() throws {
        server?.stop()
        try? FileManager.default.removeItem(at: base)
    }

    func get(_ path: String, method: String = "GET") throws -> RawResponse {
        try rawRequest("\(method) \(path) HTTP/1.1\r\nHost: 127.0.0.1:\(port)\r\n\r\n", port: port)
    }

    func testServesHtmlAndJsWithTypes() throws {
        let html = try get("/midgard/index.html")
        XCTAssertEqual(html.status, 200)
        XCTAssertEqual(html.headers["content-type"], "text/html; charset=utf-8")
        XCTAssertEqual(String(decoding: html.body, as: UTF8.self), "<h1>midgard</h1>")
        XCTAssertEqual(html.headers["cache-control"], "no-store")
        XCTAssertEqual(html.headers["connection"], "close")
        XCTAssertEqual(html.headers["x-content-type-options"], "nosniff")
        XCTAssertEqual(html.headers["content-length"], "16")

        let js = try get("/heimdall/assets/app.js?v=3#x")
        XCTAssertEqual(js.status, 200)
        XCTAssertEqual(js.headers["content-type"], "text/javascript")
        XCTAssertEqual(String(decoding: js.body, as: UTF8.self), "console.log(1)")

        XCTAssertEqual(try get("/heimdall/assets/app.css").headers["content-type"], "text/css")
        XCTAssertEqual(try get("/home/logo.svg").headers["content-type"], "image/svg+xml")
        XCTAssertEqual(try get("/mimir/data/big.i16", method: "HEAD").headers["content-type"],
                       "application/octet-stream")
    }

    func testRootServesHome() throws {
        let r = try get("/")
        XCTAssertEqual(r.status, 200)
        XCTAssertEqual(String(decoding: r.body, as: UTF8.self), "<h1>home</h1>")
    }

    func testDirectoryServesIndex() throws {
        XCTAssertEqual(String(decoding: try get("/forge/docs/").body, as: UTF8.self), "<h1>forge docs</h1>")
        XCTAssertEqual(String(decoding: try get("/midgard/").body, as: UTF8.self), "<h1>midgard</h1>")
    }

    /// The consoles load `./assets/…`, which only resolves under the directory's own URL.
    func testDirectoryWithoutSlashRedirects() throws {
        let r = try get("/forge/docs?tab=2")
        XCTAssertEqual(r.status, 301)
        XCTAssertEqual(r.headers["location"], "/forge/docs/?tab=2")
        let m = try get("/midgard")
        XCTAssertEqual(m.status, 301)
        XCTAssertEqual(m.headers["location"], "/midgard/")
    }

    func testMissingIs404() throws {
        XCTAssertEqual(try get("/midgard/nope.js").status, 404)
        XCTAssertEqual(try get("/elsewhere/index.html").status, 404)
        XCTAssertEqual(try get("/consoles/midgard/index.html").status, 404)
        XCTAssertEqual(try get("/heimdall/").status, 404)  // a directory without index.html
    }

    func testTraversalIs403() throws {
        for path in ["/../secret.txt", "/midgard/../../secret.txt", "/midgard/%2e%2e/%2E%2E/secret.txt",
                     "/midgard/..%5c..%5csecret.txt", "/midgard/a%5cb", "/midgard/index.html%00.js"] {
            let r = try get(path)
            XCTAssertEqual(r.status, 403, path)
            XCTAssertFalse(String(decoding: r.body, as: UTF8.self).contains("secret"), path)
        }
    }

    func testHeadHasHeadersButNoBody() throws {
        let r = try get("/midgard/index.html", method: "HEAD")
        XCTAssertEqual(r.status, 200)
        XCTAssertEqual(r.headers["content-length"], "16")
        XCTAssertEqual(r.body.count, 0)
    }

    func testLargeFileRoundTrips() async throws {
        let url = URL(string: "http://127.0.0.1:\(port)/mimir/data/big.i16")!
        let (data, response) = try await URLSession.shared.data(from: url)
        XCTAssertEqual((response as? HTTPURLResponse)?.statusCode, 200)
        XCTAssertEqual(data.count, big.count)
        XCTAssertTrue(data == big)
    }

    func testOtherMethodsAre405() throws {
        let r = try rawRequest(
            "POST /midgard/index.html HTTP/1.1\r\nHost: 127.0.0.1:\(port)\r\nContent-Length: 0\r\n\r\n", port: port)
        XCTAssertEqual(r.status, 405)
        XCTAssertEqual(r.headers["allow"], "GET, HEAD")
    }

    func testOversizedHeadIsRefused() throws {
        let filler = String(repeating: "a", count: 20 * 1024)
        let r = try rawRequest("GET / HTTP/1.1\r\nX-Filler: \(filler)\r\n\r\n", port: port)
        XCTAssertEqual(r.status, 431)
    }

    func testMalformedRequestLineIs400() throws {
        XCTAssertEqual(try rawRequest("NONSENSE\r\n\r\n", port: port).status, 400)
        XCTAssertEqual(try rawRequest("GET midgard HTTP/1.1\r\n\r\n", port: port).status, 400)
    }

    func testListensOnLoopbackOnly() throws {
        guard let address = nonLoopbackIPv4() else { throw XCTSkip("no non-loopback IPv4 address") }
        XCTAssertThrowsError(try connectTCP(host: address, port: port), "reachable on \(address)")
    }

    func testStartFailsWhenPortTaken() throws {
        let second = WebServer(root: root, port: port)
        XCTAssertThrowsError(try second.start())
        second.stop()
    }

    func testHostMustBeThisServer() throws {
        func status(_ host: String?) throws -> Int {
            let header = host.map { "Host: \($0)\r\n" } ?? ""
            return try rawRequest("GET / HTTP/1.1\r\n\(header)\r\n", port: port).status
        }
        XCTAssertEqual(try status("127.0.0.1:\(port)"), 200)
        XCTAssertEqual(try status("LocalHost:\(port)"), 200)
        XCTAssertEqual(try status("evil.example:\(port)"), 421)
        XCTAssertEqual(try status("127.0.0.1:\(port &+ 1)"), 421)
        XCTAssertEqual(try status("127.0.0.1"), 421)
        XCTAssertEqual(try status(nil), 400)
    }

    func testPortZeroBindsAFreePort() throws {
        let any = WebServer(root: root)
        try any.start()
        defer { any.stop() }
        XCTAssertNotEqual(any.boundPort, 0)
        XCTAssertNotEqual(any.boundPort, port)
        let r = try rawRequest("GET / HTTP/1.1\r\nHost: localhost:\(any.boundPort)\r\n\r\n", port: any.boundPort)
        XCTAssertEqual(r.status, 200)
    }

    func testRestartOnTheSamePort() throws {
        for _ in 0..<3 {
            server.stop()
            server = WebServer(root: root, port: port)
            try server.start()
            XCTAssertEqual(try get("/").status, 200)
        }
    }

    func testIdleConnectionIsClosed() throws {
        let quick = WebServer(root: root, headTimeout: 0.3)
        try quick.start()
        defer { quick.stop() }
        let fd = try connectTCP(host: "127.0.0.1", port: quick.boundPort)
        defer { close(fd) }
        var timeout = timeval(tv_sec: 3, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        _ = "GET / HT".withCString { write(fd, $0, 8) }  // a head that never completes
        let started = Date()
        var byte: UInt8 = 0
        let n = read(fd, &byte, 1)
        XCTAssertEqual(n, 0, "expected the server to close the connection (errno \(errno))")
        XCTAssertLessThan(Date().timeIntervalSince(started), 2.5)
    }

    func testStopClosesTheListener() throws {
        server.stop()
        XCTAssertThrowsError(try connectTCP(host: "127.0.0.1", port: port))
    }
}
