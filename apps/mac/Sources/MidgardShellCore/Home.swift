import Darwin
import Foundation

public enum HomeError: Error, Equatable, CustomStringConvertible {
    /// `home` holds something that is not a working copy (no `.midgard-home` marker and
    /// not empty). It is never replaced: the presenter must move it away.
    case notAWorkingCopy(URL)
    case startingStateMissing(URL)
    /// The starting state must be plain files and folders.
    case symlinkInStartingState(URL)
    case insufficientSpace(needed: Int64, available: Int64)
    /// A reset could not make a fresh copy and could not put the previous one back either;
    /// it is at `previousCopy`.
    case resetFailed(underlying: any Error, previousCopy: URL)

    public var description: String {
        switch self {
        case .notAWorkingCopy(let url):
            return "\(url.path) is not a Midgard working copy (no .midgard-home marker); "
                + "move it away and try again"
        case .startingStateMissing(let url):
            return "the starting state is missing at \(url.path)"
        case .symlinkInStartingState(let url):
            return "the starting state contains a symbolic link: \(url.path)"
        case .insufficientSpace(let needed, let available):
            let f = ByteCountFormatter()
            return "not enough free space for the working copy: needs \(f.string(fromByteCount: needed)), "
                + "\(f.string(fromByteCount: available)) available"
        case .resetFailed(let underlying, let previousCopy):
            return "reset failed (\(underlying)); the previous working copy is at \(previousCopy.path)"
        }
    }

    public static func == (a: HomeError, b: HomeError) -> Bool {
        switch (a, b) {
        case (.notAWorkingCopy(let x), .notAWorkingCopy(let y)),
             (.startingStateMissing(let x), .startingStateMissing(let y)),
             (.symlinkInStartingState(let x), .symlinkInStartingState(let y)):
            return x.standardizedFileURL == y.standardizedFileURL
        case (.insufficientSpace(let n1, let a1), .insufficientSpace(let n2, let a2)):
            return n1 == n2 && a1 == a2
        case (.resetFailed(let e1, let p1), .resetFailed(let e2, let p2)):
            return p1 == p2 && String(describing: e1) == String(describing: e2)
        default:
            return false
        }
    }
}

/// The working copy of the starting state: the services' configuration and initial data,
/// copied from the app's Resources into `Paths.home` so a demo can change it and
/// "Reset demo" can put it back.
///
/// A copy counts as prepared only when `home/.midgard-home` exists and holds the starting
/// state's version (`starting/VERSION`, or "dev" without one). The marker is written into
/// a temporary sibling before that sibling is moved into place, so a half-finished copy
/// never counts as prepared.
///
/// `home` is replaced only when it is missing, empty, or a working copy (marked, of any
/// version). Anything else throws `HomeError.notAWorkingCopy` and is left untouched.
///
/// When the app is upgraded and the starting state's version changes, the old working
/// copy is discarded and a fresh one made: any changes made during demos are lost.
public enum Home {
    public static let markerName = ".midgard-home"
    static let copyInfix = ".midgard-copy"
    static let oldInfix = ".midgard-old"

    /// What a reset left behind.
    public struct ResetReport: Sendable, Equatable {
        /// The previous working copy, if it could not be deleted (it can be removed by hand).
        public var leftover: URL?
        /// Why it could not be deleted.
        public var reason: String?
    }

    /// The starting state's version: the first line of `starting/VERSION`, else "dev".
    public static func version(of starting: URL) -> String {
        let file = starting.appendingPathComponent("VERSION")
        guard let text = try? String(contentsOf: file, encoding: .utf8) else { return "dev" }
        let line = text.split(whereSeparator: \.isNewline).first.map(String.init) ?? ""
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        return trimmed.isEmpty ? "dev" : trimmed
    }

    /// Whether `home` is a complete working copy of this version of `starting`.
    public static func isPrepared(starting: URL, home: URL) -> Bool {
        guard let marked = markerVersion(home) else { return false }
        return marked == version(of: starting)
    }

    /// Makes `home` a working copy of `starting` unless it already is one.
    public static func prepare(starting: URL, home: URL) throws {
        if isPrepared(starting: starting, home: home) { return }
        try checkReplaceable(home)

        let fm = FileManager.default
        let source = starting.resolvingSymlinksInPath()
        let size = try inspect(source)
        let parent = home.deletingLastPathComponent()
        try fm.createDirectory(at: parent, withIntermediateDirectories: true)
        if !sameVolume(source, parent) {
            let values = try parent.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
            if let available = values.volumeAvailableCapacityForImportantUsage {
                try checkRoom(needed: size, available: available)
            }
        }
        removeSiblings(of: home, infix: copyInfix)

        let version = version(of: source)
        let temp = sibling(of: home, infix: copyInfix)
        do {
            // copyItem uses copyfile(3): POSIX modes (0600 keystores) are kept.
            try fm.copyItem(at: source, to: temp)
            makeOwnAndWritable(temp)
            try Data(version.utf8).write(to: temp.appendingPathComponent(markerName))
        } catch {
            makeOwnAndWritable(temp)
            try? fm.removeItem(at: temp)
            throw error
        }

        var aside: URL?
        if exists(home) {
            let old = sibling(of: home, infix: oldInfix)
            do {
                try fm.moveItem(at: home, to: old)
            } catch {
                try? fm.removeItem(at: temp)
                throw error
            }
            aside = old
        }
        do {
            try fm.moveItem(at: temp, to: home)
        } catch {
            if let aside { try? fm.moveItem(at: aside, to: home) }
            try? fm.removeItem(at: temp)
            throw error
        }
        // The replaced copy, and any left by an earlier reset whose delete failed.
        removeSiblings(of: home, infix: oldInfix)
    }

    /// Puts `home` back to `starting`. The caller has stopped the services. If a fresh copy
    /// cannot be made the previous one is put back and the error rethrown (or, if even that
    /// fails, `HomeError.resetFailed` says where it is). A previous copy that cannot be
    /// deleted is left beside `home` and reported.
    @discardableResult
    public static func reset(starting: URL, home: URL) throws -> ResetReport {
        let fm = FileManager.default
        try checkReplaceable(home)

        var old: URL?
        if exists(home) {
            let target = sibling(of: home, infix: oldInfix)
            try fm.moveItem(at: home, to: target)
            old = target
        }
        do {
            try prepare(starting: starting, home: home)
        } catch {
            guard let old else { throw error }
            do {
                if exists(home) { try fm.removeItem(at: home) }
                try fm.moveItem(at: old, to: home)
            } catch {
                throw HomeError.resetFailed(underlying: error, previousCopy: old)
            }
            throw error
        }
        // prepare's sweep normally deleted it already.
        guard let old, exists(old) else { return ResetReport() }
        do {
            try fm.removeItem(at: old)
            return ResetReport()
        } catch {
            return ResetReport(leftover: old, reason: error.localizedDescription)
        }
    }

    static func checkRoom(needed: Int64, available: Int64) throws {
        if needed > available { throw HomeError.insufficientSpace(needed: needed, available: available) }
    }

    // MARK: - Helpers

    private static func markerVersion(_ home: URL) -> String? {
        let marker = home.appendingPathComponent(markerName)
        guard let text = try? String(contentsOf: marker, encoding: .utf8) else { return nil }
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// `home` may be replaced only if it is missing, an empty folder, or a marked copy.
    private static func checkReplaceable(_ home: URL) throws {
        guard exists(home) else { return }
        if isSymlink(home) { throw HomeError.notAWorkingCopy(home) }
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: home.path, isDirectory: &isDir), isDir.boolValue else {
            throw HomeError.notAWorkingCopy(home)
        }
        if markerVersion(home) != nil { return }
        let contents = (try? FileManager.default.contentsOfDirectory(atPath: home.path)) ?? ["?"]
        if contents.allSatisfy({ $0 == ".DS_Store" }) { return }
        throw HomeError.notAWorkingCopy(home)
    }

    /// Checks the starting state is a folder of plain files and folders; returns its size.
    private static func inspect(_ source: URL) throws -> Int64 {
        var isDir: ObjCBool = false
        guard FileManager.default.fileExists(atPath: source.path, isDirectory: &isDir), isDir.boolValue else {
            throw HomeError.startingStateMissing(source)
        }
        let keys: [URLResourceKey] = [.isSymbolicLinkKey, .totalFileAllocatedSizeKey, .fileSizeKey]
        guard let walk = FileManager.default.enumerator(at: source, includingPropertiesForKeys: keys)
        else { throw HomeError.startingStateMissing(source) }
        var total: Int64 = 0
        for case let url as URL in walk {
            let values = try url.resourceValues(forKeys: Set(keys))
            if values.isSymbolicLink == true { throw HomeError.symlinkInStartingState(url) }
            total += Int64(values.totalFileAllocatedSize ?? values.fileSize ?? 0)
        }
        return total
    }

    private static func sameVolume(_ a: URL, _ b: URL) -> Bool {
        let key: Set<URLResourceKey> = [.volumeIdentifierKey]
        guard let x = try? a.resourceValues(forKeys: key).volumeIdentifier as? NSObject,
              let y = try? b.resourceValues(forKeys: key).volumeIdentifier as? NSObject
        else { return false }
        return x.isEqual(y)
    }

    /// The copy belongs to the demo: no Gatekeeper quarantine flags (a starting state
    /// unpacked from a download carries them), and every folder is u+rwx so services can
    /// write and a later reset can delete it.
    private static func makeOwnAndWritable(_ root: URL) {
        func fix(_ path: String) {
            removexattr(path, "com.apple.quarantine", XATTR_NOFOLLOW)
            var st = stat()
            if lstat(path, &st) == 0, (st.st_mode & S_IFMT) == S_IFDIR {
                chmod(path, (st.st_mode & 0o7777) | 0o700)
            }
        }
        fix(root.path)
        guard let walk = FileManager.default.enumerator(atPath: root.path) else { return }
        for case let relative as String in walk {
            fix(root.appendingPathComponent(relative).path)
        }
    }

    static func sibling(of home: URL, infix: String) -> URL {
        home.deletingLastPathComponent()
            .appendingPathComponent("\(home.lastPathComponent)\(infix)-\(UUID().uuidString)", isDirectory: true)
    }

    /// Deletes, as far as it can, every `<home>.midgard-…` sibling with this infix.
    static func removeSiblings(of home: URL, infix: String) {
        let fm = FileManager.default
        let parent = home.deletingLastPathComponent()
        let prefix = home.lastPathComponent + infix
        guard let names = try? fm.contentsOfDirectory(atPath: parent.path) else { return }
        for name in names where name.hasPrefix(prefix) {
            try? fm.removeItem(at: parent.appendingPathComponent(name))
        }
    }

    private static func exists(_ url: URL) -> Bool {
        FileManager.default.fileExists(atPath: url.path) || isSymlink(url)
    }

    private static func isSymlink(_ url: URL) -> Bool {
        (try? FileManager.default.destinationOfSymbolicLink(atPath: url.path)) != nil
    }
}
