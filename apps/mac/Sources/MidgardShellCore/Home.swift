import Foundation

/// The working copy of the starting state: the services' configuration and initial data,
/// copied from the app's Resources into `Paths.home` so a demo can change it and
/// "Reset demo" can put it back.
///
/// A copy counts as prepared only when `home/.midgard-home` exists and holds the starting
/// state's version (`starting/VERSION`, or "dev" without one). The marker is written into
/// a temporary sibling before that sibling is moved into place, so a half-finished copy
/// never counts as prepared.
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
        let marker = home.appendingPathComponent(markerName)
        guard let text = try? String(contentsOf: marker, encoding: .utf8) else { return false }
        return text.trimmingCharacters(in: .whitespacesAndNewlines) == version(of: starting)
    }

    /// Makes `home` a working copy of `starting` unless it already is one. Anything else at
    /// `home` (an unmarked copy from a broken install, or one from an older version) is
    /// replaced.
    public static func prepare(starting: URL, home: URL) throws {
        if isPrepared(starting: starting, home: home) { return }
        let fm = FileManager.default
        let parent = home.deletingLastPathComponent()
        try fm.createDirectory(at: parent, withIntermediateDirectories: true)
        removeSiblings(of: home, infix: copyInfix)

        let version = version(of: starting)
        let temp = sibling(of: home, infix: copyInfix)
        do {
            // copyItem uses copyfile(3): POSIX modes (0600 keystores) are kept.
            try fm.copyItem(at: starting, to: temp)
            try Data(version.utf8).write(to: temp.appendingPathComponent(markerName))
        } catch {
            try? fm.removeItem(at: temp)
            throw error
        }

        var aside: URL?
        if fm.fileExists(atPath: home.path) || isSymlink(home) {
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
        if let aside { try? fm.removeItem(at: aside) }
    }

    /// Puts `home` back to `starting`. The caller has stopped the services. Fails only if
    /// a fresh copy could not be made (the previous copy is then put back); a previous copy
    /// that cannot be deleted is left beside `home` and reported.
    @discardableResult
    public static func reset(starting: URL, home: URL) throws -> ResetReport {
        let fm = FileManager.default
        // Leftovers from earlier resets whose delete failed.
        removeSiblings(of: home, infix: oldInfix)

        var old: URL?
        if fm.fileExists(atPath: home.path) || isSymlink(home) {
            let target = sibling(of: home, infix: oldInfix)
            try fm.moveItem(at: home, to: target)
            old = target
        }
        do {
            try prepare(starting: starting, home: home)
        } catch {
            if let old, !fm.fileExists(atPath: home.path) { try? fm.moveItem(at: old, to: home) }
            throw error
        }
        guard let old else { return ResetReport() }
        do {
            try fm.removeItem(at: old)
            return ResetReport()
        } catch {
            return ResetReport(leftover: old, reason: error.localizedDescription)
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

    private static func isSymlink(_ url: URL) -> Bool {
        (try? FileManager.default.destinationOfSymbolicLink(atPath: url.path)) != nil
    }
}
