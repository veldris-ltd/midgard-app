import Foundation

/// Where the shell finds its bundled files and keeps its working copy and logs.
/// Built from an environment so tests and development runs can point it elsewhere.
public struct Paths: Sendable, Equatable {
    /// The app's Resources folder (bin/, consoles/, home/, starting-state/ …).
    public let resources: URL
    /// The working copy of the starting state (H in the plan).
    public let home: URL
    /// One log per service plus shell.log.
    public let logs: URL

    public init(environment: [String: String], bundleResources: URL, userHome: URL) {
        func pick(_ key: String, _ fallback: URL) -> URL {
            guard let value = environment[key], !value.isEmpty else { return fallback }
            return URL(fileURLWithPath: value, isDirectory: true)
        }
        let library = userHome.appendingPathComponent("Library", isDirectory: true)
        resources = pick("MIDGARD_APP_RESOURCES", bundleResources)
        home = pick("MIDGARD_APP_HOME",
                    library.appendingPathComponent("Application Support/Midgard", isDirectory: true))
        logs = pick("MIDGARD_APP_LOGS", library.appendingPathComponent("Logs/Midgard", isDirectory: true))
    }

    /// The paths for this process: the main bundle and the user's real home folder.
    public static func current(
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> Paths {
        let bundle = Bundle.main.resourceURL ?? Bundle.main.bundleURL
        return Paths(environment: environment, bundleResources: bundle,
                     userHome: FileManager.default.homeDirectoryForCurrentUser)
    }
}
