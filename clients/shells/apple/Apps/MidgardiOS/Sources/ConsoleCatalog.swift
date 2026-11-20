import Foundation

/// The six consoles and their 45 screens, in the shell's order (⌘1…⌘6), with the phase
/// each console's screens arrive in (plan, Phases C to H). Until then every screen is a
/// placeholder that says so. [FR-C-1]
struct Console: Identifiable, Hashable {
    /// 1…6: the tab's tag, its ⌘ number and `mg.console.selected`'s value.
    let number: Int
    let name: String
    let symbol: String
    let phase: String
    let screens: [String]

    var id: Int { number }
    /// "midgard", "break-glass"…: the identifier part of a name.
    static func slug(_ name: String) -> String {
        name.lowercased().split { !$0.isLetter && !$0.isNumber }.joined(separator: "-")
    }

    var slug: String { Console.slug(name) }

    static let all: [Console] = [
        Console(number: 1, name: "Midgard", symbol: "square.stack.3d.up", phase: "C",
                screens: ["Object Fabric", "Data Search", "Search Around", "Policies", "Identity", "Keys", "Audit",
                          "Compute", "Observability", "Resilience", "SDK"]),
        Console(number: 2, name: "Admin", symbol: "gearshape.2", phase: "D",
                screens: ["Overview", "Approvals", "IAM", "Config", "Ledger", "Registry", "Break-Glass", "Copilot"]),
        Console(number: 3, name: "Heimdall", symbol: "scope", phase: "E",
                screens: ["Operating Picture", "Entities", "Feeds", "Picture Sync", "Workflows", "Decision Lineage"]),
        Console(number: 4, name: "Forge", symbol: "hammer", phase: "F",
                screens: ["Connectors", "Datasets", "Data Health", "Pipeline Builder", "Transform IDE", "Schedules",
                          "Object Projection"]),
        Console(number: 5, name: "Mimir", symbol: "brain", phase: "G",
                screens: ["Model Plane", "Session Traces", "Agent Studio", "Logic Builder", "Evals", "Guardrails",
                          "Feedback"]),
        Console(number: 6, name: "Bifrost", symbol: "shippingbox", phase: "H",
                screens: ["Release Console", "Channels", "Fleet", "Artefacts", "Bundles", "Rollback"]),
    ]
}
