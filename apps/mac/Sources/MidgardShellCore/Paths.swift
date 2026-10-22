import Foundation

/// Where the shell finds its bundled files and keeps its working copy and logs.
/// Built from an environment so tests and development runs can point it elsewhere.
public struct Paths: Sendable, Equatable {
    /// The app's Resources folder (consoles/, home/, starting-state/, models/ …).
    public let resources: URL
    /// The service executables: `Contents/Helpers` in the app bundle, `resources/bin` in
    /// a development layout (MIDGARD_APP_RESOURCES, or a shell run outside a bundle).
    public let helpers: URL
    /// The working copy of the starting state (H in the plan).
    public let home: URL
    /// One log per service plus shell.log.
    public let logs: URL

    /// `bundleHelpers` is the bundle's Helpers folder when the shell runs from an app
    /// bundle, nil otherwise.
    public init(environment: [String: String], bundleResources: URL, userHome: URL,
                bundleHelpers: URL? = nil) {
        func pick(_ key: String, _ fallback: URL) -> URL {
            guard let value = environment[key], !value.isEmpty else { return fallback }
            return URL(fileURLWithPath: value, isDirectory: true)
        }
        let library = userHome.appendingPathComponent("Library", isDirectory: true)
        resources = pick("MIDGARD_APP_RESOURCES", bundleResources)
        let devLayout = !(environment["MIDGARD_APP_RESOURCES"] ?? "").isEmpty
        if let bundleHelpers, !devLayout {
            helpers = bundleHelpers
        } else {
            helpers = resources.appendingPathComponent("bin", isDirectory: true)
        }
        home = pick("MIDGARD_APP_HOME",
                    library.appendingPathComponent("Application Support/Midgard", isDirectory: true))
        logs = pick("MIDGARD_APP_LOGS", library.appendingPathComponent("Logs/Midgard", isDirectory: true))
    }

    /// The paths for this process: the main bundle and the user's real home folder.
    public static func current(
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> Paths {
        let bundle = Bundle.main.resourceURL ?? Bundle.main.bundleURL
        let app = Bundle.main.bundleURL
        let helpers = app.pathExtension == "app"
            ? app.appendingPathComponent("Contents/Helpers", isDirectory: true) : nil
        return Paths(environment: environment, bundleResources: bundle,
                     userHome: FileManager.default.homeDirectoryForCurrentUser, bundleHelpers: helpers)
    }
}
