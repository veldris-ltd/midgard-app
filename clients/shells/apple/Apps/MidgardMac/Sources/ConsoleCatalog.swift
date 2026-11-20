import Foundation

/// One console in the shell's sidebar: its screens are the web console's, by the names
/// in the plan's Phase C–H tables, and `phase` is the phase that builds them. [FR-C-1]
struct Console: Identifiable, Equatable {
    let id: String
    let name: String
    let phase: String
    let screens: [String]
}

/// What a window shows: a console (0…5, the sidebar order) and one of its screens.
/// Each window keeps its own (`WindowGroup(for: ConsoleRoute.self)`), so a new window
/// opens where asked and a restored one where it was. [FR-C-1]
struct ConsoleRoute: Codable, Hashable {
    var console = 0
    var screen = 0

    /// In range for the catalogue: a route restored from another build never traps.
    var clamped: ConsoleRoute {
        let c = min(max(console, 0), ConsoleCatalog.consoles.count - 1)
        let s = min(max(screen, 0), ConsoleCatalog.consoles[c].screens.count - 1)
        return ConsoleRoute(console: c, screen: s)
    }

    /// The value of `mg.console.selected`, the status line's "Console n of 6" text: 1…6,
    /// as ⌘1…⌘6.
    var selectedValue: String { String(clamped.console + 1) }

    /// Selecting a console opens its first screen.
    mutating func select(console index: Int) {
        self = ConsoleRoute(console: index, screen: 0).clamped
    }
}

/// The six consoles, in the design's sidebar order (⌘1…⌘6). [FR-C-1]
enum ConsoleCatalog {
    static let consoles: [Console] = [
        Console(id: "midgard", name: "Midgard", phase: "C",
                screens: ["Object Fabric", "Data Search", "Search Around", "Policies", "Identity", "Keys", "Audit",
                          "Compute", "Observability", "Resilience", "SDK"]),
        Console(id: "admin", name: "Admin", phase: "D",
                screens: ["Overview", "Approvals", "IAM", "Config", "Ledger", "Registry", "Break-Glass", "Copilot"]),
        Console(id: "heimdall", name: "Heimdall", phase: "E",
                screens: ["Operating Picture", "Entities", "Feeds", "Picture Sync", "Workflows", "Decision Lineage"]),
        Console(id: "forge", name: "Forge", phase: "F",
                screens: ["Connectors", "Datasets", "Data Health", "Pipeline Builder", "Transform IDE", "Schedules",
                          "Object Projection"]),
        Console(id: "mimir", name: "Mimir", phase: "G",
                screens: ["Model Plane", "Session Traces", "Agent Studio", "Logic Builder", "Evals", "Guardrails",
                          "Feedback"]),
        Console(id: "bifrost", name: "Bifrost", phase: "H",
                screens: ["Release Console", "Channels", "Fleet", "Artefacts", "Bundles", "Rollback"]),
    ]

    /// A screen name as an identifier part: lower case, words joined by "-".
    static func slug(_ name: String) -> String {
        name.lowercased().split(whereSeparator: { !$0.isLetter && !$0.isNumber }).joined(separator: "-")
    }

    static func console(_ route: ConsoleRoute) -> Console { consoles[route.clamped.console] }

    static func screenName(_ route: ConsoleRoute) -> String {
        let r = route.clamped
        return consoles[r.console].screens[r.screen]
    }

    /// The placeholder a screen shows until its phase builds it.
    static func placeholder(for route: ConsoleRoute) -> String {
        let console = console(route)
        return "\(screenName(route)) arrives in Phase \(console.phase) (\(console.name))."
    }
}
