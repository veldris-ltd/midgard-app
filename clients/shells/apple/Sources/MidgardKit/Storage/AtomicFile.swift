import Foundation

/// Atomic file replacement: write a temporary file in the same directory, flush it
/// (`F_FULLFSYNC`, else `fsync`), rename it over the target, then flush the directory;
/// a failed directory flush is reported, so the caller does not move a generation. A crash leaves either the
/// old file or the new one, never a torn file. Files are 0600, and on iOS get
/// complete file protection. [FR-C-4] [AC-C-2]
public enum AtomicFile {
    public struct Failure: Error, Equatable {
        public let step: String
        public let errno: Int32
    }

    public static func write(_ data: Data, to url: URL) throws {
        let directory = url.deletingLastPathComponent()
        let temp = directory.appendingPathComponent(".\(url.lastPathComponent).\(UUID().uuidString).tmp")
        let fd = open(temp.path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0o600)
        guard fd >= 0 else { throw Failure(step: "open", errno: errno) }
        var renamed = false
        defer { if !renamed { unlink(temp.path) } }
        do {
            defer { close(fd) }
            try data.withUnsafeBytes { raw in
                var offset = 0
                while offset < raw.count {
                    let n = Darwin.write(fd, raw.baseAddress! + offset, raw.count - offset)
                    if n < 0 {
                        if errno == EINTR { continue }
                        throw Failure(step: "write", errno: errno)
                    }
                    offset += n
                }
            }
            guard flush(fd) else { throw Failure(step: "fsync", errno: errno) }
        }
        #if os(iOS)
        try FileManager.default.setAttributes([.protectionKey: FileProtectionType.complete], ofItemAtPath: temp.path)
        #endif
        guard rename(temp.path, url.path) == 0 else { throw Failure(step: "rename", errno: errno) }
        renamed = true
        let dirfd = open(directory.path, O_RDONLY | O_CLOEXEC)
        guard dirfd >= 0 else { throw Failure(step: "open directory", errno: errno) }
        defer { close(dirfd) }
        guard flush(dirfd) else { throw Failure(step: "fsync directory", errno: errno) }
    }

    /// Push `fd` to stable storage: `F_FULLFSYNC` (Apple's `fsync` stops at the drive's
    /// cache), falling back to `fsync` where the file system does not support it.
    static func flush(_ fd: Int32) -> Bool {
        fcntl(fd, F_FULLFSYNC) == 0 || fsync(fd) == 0
    }

    /// Remove `url` if it exists.
    public static func remove(_ url: URL) throws {
        if unlink(url.path) != 0, errno != ENOENT { throw Failure(step: "unlink", errno: errno) }
    }
}
