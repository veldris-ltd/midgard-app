import Darwin
import Foundation

// The window's logic that needs no window: the shell's log, the single-instance lock,
// the offline rules, which navigations are allowed, Heimdall's first view, and the
// port-retry loop around `Supervisor.start`.

/// The shell's own steps, one timestamped line each, appended to `shell.log` (and to
/// stderr, for `swift run`).
public final class ShellLog: @unchecked Sendable {
    public let file: URL
    private let lock = NSLock()
    private let formatter: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()
    private let echo: Bool

    public init(file: URL, echo: Bool = true) {
        self.file = file
        self.echo = echo
        try? FileManager.default.createDirectory(at: file.deletingLastPathComponent(),
                                                 withIntermediateDirectories: true)
    }

    public func callAsFunction(_ message: String) {
        lock.lock(); defer { lock.unlock() }
        let line = "\(formatter.string(from: Date())) \(message)\n"
        let fd = open(file.path, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0o644)
        if fd >= 0 {
            _ = Array(line.utf8).withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
            close(fd)
        }
        if echo { FileHandle.standardError.write(Data(line.utf8)) }
    }
}

/// An exclusive `flock` held for the life of the process, so a second shell knows the
/// first is running (and which pid it is) even without a bundle identifier. The file
/// descriptor is close-on-exec, so the services never inherit the lock.
public final class InstanceLock: @unchecked Sendable {
    public enum Outcome {
        case acquired(InstanceLock)
        /// Another process holds it; its pid if it wrote one.
        case heldBy(pid_t?)
        case failed(String)
    }

    public let file: URL
    private var fd: Int32

    private init(file: URL, fd: Int32) {
        self.file = file
        self.fd = fd
    }

    deinit { release() }

    /// Takes the lock at `file` (creating it and its folder), writing this pid into it.
    public static func acquire(_ file: URL) -> Outcome {
        try? FileManager.default.createDirectory(at: file.deletingLastPathComponent(),
                                                 withIntermediateDirectories: true)
        let fd = open(file.path, O_RDWR | O_CREAT | O_CLOEXEC, 0o644)
        guard fd >= 0 else { return .failed("cannot open \(file.path): \(String(cString: strerror(errno)))") }
        if flock(fd, LOCK_EX | LOCK_NB) != 0 {
            let code = errno
            var buffer = [UInt8](repeating: 0, count: 32)
            let count = pread(fd, &buffer, buffer.count, 0)
            close(fd)
            guard code == EWOULDBLOCK else { return .failed("cannot lock \(file.path): \(String(cString: strerror(code)))") }
            let text = count > 0 ? String(decoding: buffer.prefix(count), as: UTF8.self) : ""
            return .heldBy(pid_t(text.trimmingCharacters(in: .whitespacesAndNewlines)))
        }
        ftruncate(fd, 0)
        let pid = Array("\(getpid())\n".utf8)
        _ = pid.withUnsafeBytes { pwrite(fd, $0.baseAddress, $0.count, 0) }
        return .acquired(InstanceLock(file: file, fd: fd))
    }

    public func release() {
        guard fd >= 0 else { return }
        flock(fd, LOCK_UN)
        close(fd)
        fd = -1
    }
}

/// The offline guarantee for the web view: a content rule list that blocks every load,
/// then lets through only the loopback address (the app's own web server and the
/// gateway) and the in-page schemes the consoles use.
public enum OfflineRules {
    public static let identifier = "midgard.offline.v1"

    /// Patterns (WebKit's restricted regex dialect) that stay allowed.
    public static let allowed = ["^http://127\\.0\\.0\\.1:", "^about:", "^blob:", "^data:"]

    /// The rule list as WKContentRuleListStore compiles it.
    public static var json: String {
        var rules: [[String: Any]] = [
            ["trigger": ["url-filter": ".*"], "action": ["type": "block"]],
        ]
        for pattern in allowed {
            rules.append(["trigger": ["url-filter": pattern], "action": ["type": "ignore-previous-rules"]])
        }
        let data = try! JSONSerialization.data(withJSONObject: rules, options: [.sortedKeys])
        return String(decoding: data, as: UTF8.self)
    }
}

/// Which main-frame and frame navigations the web view may make.
public enum Navigation {
    /// True for the app's own web server (`origin`, e.g. `http://127.0.0.1:61235`), the
    /// gateway's origin, and `about:blank`-style pages; false for everything else.
    public static func isAllowed(_ url: URL, origin: String, gatewayOrigin: String?) -> Bool {
        switch url.scheme?.lowercased() {
        case "about", "blob", "data": return true
        case "http":
            guard let own = Self.origin(of: url) else { return false }
            return own == origin || (gatewayOrigin != nil && own == gatewayOrigin)
        default:
            return false
        }
    }

    /// `scheme://host:port` of an http URL (no trailing slash), nil without a port.
    public static func origin(of url: URL) -> String? {
        guard let scheme = url.scheme?.lowercased(), let host = url.host?.lowercased(),
              let port = url.port else { return nil }
        return "\(scheme)://\(host):\(port)"
    }
}

/// Heimdall's operating picture remembers 2D or 3D in localStorage. A new presenter
/// gets 3D on Apple Silicon and 2D on Intel (where the globe is slow); a choice they
/// made since is kept.
public enum HeimdallView {
    /// `VIEW_KEY` in products/heimdall/ui/src/screens/OperatingPictureScreen.tsx.
    public static let storageKey = "heimdall.cop.view"

    public static func preferred(appleSilicon: Bool) -> String { appleSilicon ? "3d" : "2d" }

    /// True on Apple Silicon, also when this process runs translated under Rosetta.
    public static func isAppleSilicon() -> Bool {
        var value: Int32 = 0
        var size = MemoryLayout<Int32>.size
        return sysctlbyname("hw.optional.arm64", &value, &size, nil, 0) == 0 && value == 1
    }

    /// Document-start script: sets the key to `view` only if it is not set yet, and only
    /// on the app's own origin.
    public static func script(origin: String, view: String) -> String {
        """
        (function () {
          if (typeof location === 'undefined' || location.origin !== \(Session.literal(origin))) { return; }
          try {
            if (localStorage.getItem(\(Session.literal(storageKey))) === null) {
              localStorage.setItem(\(Session.literal(storageKey)), \(Session.literal(view)));
            }
          } catch (e) {}
        })();
        """
    }
}

/// JavaScript the shell runs in a page.
public enum PageScript {
    /// `window.<function>(<text>)` if the page defines it; `text` as a safe literal.
    public static func call(_ function: String, _ text: String) -> String {
        "if (typeof window.\(function) === 'function') { window.\(function)(\(Session.literal(text))); }"
    }
}

/// The presenter-facing names of the services.
public enum ServiceNames {
    public static func friendly(_ name: String) -> String {
        switch name {
        case "llama": return "Model"
        case "mimir": return "Mimir"
        case "gateway": return "Gateway"
        case "heimdall": return "Heimdall"
        case "forge": return "Forge"
        case "replay": return "Replay"
        default: return name.prefix(1).uppercased() + name.dropFirst()
        }
    }
}

/// Starting the suite with ports chosen just before: if a service finds its port taken
/// meanwhile, stop everything and try again with fresh ports.
public enum SuiteStart {
    /// Calls `start` with ports from `choosePorts`; on `SupervisorError.portInUse` calls
    /// `stopAll`, reports the retry and tries again with new ports, at most `retries`
    /// more times. Returns the ports that worked. Any other error is rethrown as it is
    /// (the caller decides whether to stop what is running); if the task is cancelled,
    /// `stopAll` runs and `CancellationError` is thrown.
    public static func run(
        retries: Int = 3,
        choosePorts: @Sendable () throws -> SuitePorts,
        start: @Sendable (SuitePorts) async throws -> Void,
        stopAll: @Sendable () async -> Void,
        onRetry: @Sendable (_ service: String, _ retry: Int) async -> Void = { _, _ in }
    ) async throws -> SuitePorts {
        var retry = 0
        while true {
            let ports = try choosePorts()
            do {
                try Task.checkCancellation()
                try await start(ports)
                try Task.checkCancellation()
                return ports
            } catch SupervisorError.portInUse(let service) where retry < retries {
                retry += 1
                await stopAll()
                await onRetry(service, retry)
            } catch is CancellationError {
                await stopAll()
                throw CancellationError()
            } catch {
                if Task.isCancelled {
                    await stopAll()
                    throw CancellationError()
                }
                throw error
            }
        }
    }
}
