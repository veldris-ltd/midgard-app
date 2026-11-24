import Foundation

/// One console of the shell: its screens are the web console's, by the names in the
/// plan's Phase C–H tables, and `phase` is the phase that builds them. Shared by both
/// apps; each app adds only its own symbol for it. [FR-C-1]
public struct Console: Identifiable, Hashable, Sendable {
    /// 1…6, the sidebar and console-bar order: the console's ⌘ number and the value of
    /// `mg.console.selected`.
    public let number: Int
    /// The identifier part of the name ("midgard", "bifrost"): stable across builds.
    public let id: String
    public let name: String
    public let phase: String
    public let screens: [String]

    init(_ number: Int, _ name: String, phase: String, screens: [String]) {
        self.number = number
        self.id = ConsoleCatalog.slug(name)
        self.name = name
        self.phase = phase
        self.screens = screens
    }

    /// The console's identifier part (its `id`).
    public var slug: String { id }
}

/// What a window or screen shows: a console (0…5, the catalogue's order) and one of its
/// screens. The Mac keeps one per window (`WindowGroup(for: ConsoleRoute.self)`), so a
/// new window opens where asked and a restored one where it was. [FR-C-1]
public struct ConsoleRoute: Codable, Hashable, Sendable {
    public var console: Int
    public var screen: Int

    public init(console: Int = 0, screen: Int = 0) {
        self.console = console
        self.screen = screen
    }

    /// In range for the catalogue: a route restored from another build never traps.
    public var clamped: ConsoleRoute {
        let c = min(max(console, 0), ConsoleCatalog.consoles.count - 1)
        let s = min(max(screen, 0), ConsoleCatalog.consoles[c].screens.count - 1)
        return ConsoleRoute(console: c, screen: s)
    }

    /// The value of `mg.console.selected`, the status line's "Console n of 6" text: 1…6,
    /// as ⌘1…⌘6.
    public var selectedValue: String { String(ConsoleCatalog.consoles[clamped.console].number) }

    /// Selecting a console opens its first screen.
    public mutating func select(console index: Int) {
        self = ConsoleRoute(console: index, screen: 0).clamped
    }
}

/// The six consoles and their 45 screens, in the design's order (⌘1…⌘6), each built by
/// its phase (C to H) and matched to its web console. [FR-C-1]
public enum ConsoleCatalog {
    public static let consoles: [Console] = [
        Console(1, "Midgard", phase: "C",
                screens: ["Object Fabric", "Data Search", "Search Around", "Policies", "Identity", "Keys", "Audit",
                          "Compute", "Observability", "Resilience", "SDK"]),
        Console(2, "Admin", phase: "D",
                screens: ["Overview", "Approvals", "IAM", "Config", "Ledger", "Registry", "Break-Glass", "Copilot"]),
        Console(3, "Heimdall", phase: "E",
                screens: ["Operating Picture", "Entities", "Feeds", "Picture Sync", "Workflows", "Decision Lineage"]),
        Console(4, "Forge", phase: "F",
                screens: ["Connectors", "Datasets", "Data Health", "Pipeline Builder", "Transform IDE", "Schedules",
                          "Object Projection"]),
        Console(5, "Mimir", phase: "G",
                screens: ["Model Plane", "Session Traces", "Agent Studio", "Logic Builder", "Evals", "Guardrails",
                          "Feedback"]),
        Console(6, "Bifrost", phase: "H",
                screens: ["Release Console", "Channels", "Fleet", "Artefacts", "Bundles", "Rollback"]),
    ]

    /// A name as an identifier part: lower case, words joined by "-" ("break-glass").
    public static func slug(_ name: String) -> String {
        name.lowercased().split(whereSeparator: { !$0.isLetter && !$0.isNumber }).joined(separator: "-")
    }

    /// The console with ⌘ number `number`, clamped to 1…6.
    public static func console(number: Int) -> Console {
        consoles[min(max(number, 1), consoles.count) - 1]
    }

    public static func console(_ route: ConsoleRoute) -> Console { consoles[route.clamped.console] }

    public static func screenName(_ route: ConsoleRoute) -> String {
        let r = route.clamped
        return consoles[r.console].screens[r.screen]
    }
}
