import Darwin

/// Free TCP ports on the loopback interface, for the services the shell starts.
public enum Ports {
    public struct Failure: Error, CustomStringConvertible {
        public let call: String
        public let code: Int32
        public var description: String { "\(call) failed: \(String(cString: strerror(code)))" }
    }

    /// `count` distinct free ports on 127.0.0.1. Every probe socket stays open until all
    /// ports are chosen, so the kernel cannot hand out the same port twice; they are
    /// closed before returning. (A port may still be taken by someone else afterwards;
    /// the supervisor treats a failed bind as a service start failure.)
    public static func free(count: Int) throws -> [UInt16] {
        var sockets: [Int32] = []
        defer { sockets.forEach { close($0) } }
        var ports: [UInt16] = []
        for _ in 0..<max(0, count) {
            let fd = socket(AF_INET, SOCK_STREAM, 0)
            guard fd >= 0 else { throw Failure(call: "socket", code: errno) }
            sockets.append(fd)
            var addr = sockaddr_in()
            addr.sin_family = sa_family_t(AF_INET)
            addr.sin_port = 0
            addr.sin_addr.s_addr = inet_addr("127.0.0.1")
            var len = socklen_t(MemoryLayout<sockaddr_in>.size)
            let bound = withUnsafeMutablePointer(to: &addr) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                    bind(fd, sa, len) == 0 && getsockname(fd, sa, &len) == 0
                }
            }
            guard bound else { throw Failure(call: "bind", code: errno) }
            ports.append(UInt16(bigEndian: addr.sin_port))
        }
        return ports
    }
}
