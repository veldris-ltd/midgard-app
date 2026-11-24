import MidgardCore
import SwiftUI

/// Mimir's console frame as the web suite draws it (`ui/shared/src/nav.ts`, product
/// `mimir`, at the signed tag): the CONFIDENTIAL banner (the design system's default
/// words for the level), the header's two seals, and the navigation's three sections in
/// the web's order, with the web's labels, icons and badges, each opening one of the
/// app's screens (`ConsoleCatalog`'s Mimir screens), and each screen's context line and
/// inspector width. These are the web console's fixed chrome, not data. [FR-C-1] [DD-7]
public enum MimirChrome {
    /// `NAV.mimir.banner` (level only: `ClassificationBanner`'s default text).
    public static let banner = (level: MarkingLevelChip.confidential, text: "CONFIDENTIAL · RECIPIENT EYES ONLY")
    /// `INFERENCE · LOCAL` pulses on the web (a live seal).
    public static let seals: [(WebStatusPill.Status, String)] = [(.online, "INFERENCE · LOCAL"), (.secure, "NO EGRESS")]
    public static let accent = MGWebAccent.mimir

    /// One navigation item: the web's id, label, icon and badge, and the screen it opens.
    public struct Item: Identifiable {
        public let id: String
        public let label: String
        public let symbol: String
        public let badge: String?
        public let screen: String
        /// `MIMIR / …`, the header's context line on this screen.
        public let context: String
        /// The screen's inspector rail on the Mac (`asideWidth`, 330 by default).
        public let asideWidth: CGFloat
    }

    public static let sections: [(title: String, items: [Item])] = [
        ("BUILD", [
            Item(id: "agents", label: "Agents", symbol: WebIcon.symbol("bot"), badge: "12",
                 screen: "Agent Studio", context: "MIMIR / AGENT STUDIO", asideWidth: 300),
            Item(id: "logic", label: "Logic (no-code)", symbol: WebIcon.symbol("workflow"), badge: nil,
                 screen: "Logic Builder", context: "MIMIR / LOGIC · DISPATCH-01", asideWidth: 330),
            Item(id: "sessions", label: "Session traces", symbol: WebIcon.symbol("terminal"), badge: nil,
                 screen: "Session Traces", context: "MIMIR / SESSION S-8812 · DISPATCH-01", asideWidth: 340),
        ]),
        ("GOVERN", [
            Item(id: "evals", label: "Evals", symbol: WebIcon.symbol("gauge"), badge: nil,
                 screen: "Evals", context: "MIMIR / EVALS", asideWidth: 330),
            Item(id: "guardrails", label: "Guardrails", symbol: WebIcon.symbol("shield-check"), badge: nil,
                 screen: "Guardrails", context: "MIMIR / GUARDRAILS", asideWidth: 330),
            Item(id: "feedback", label: "Governed feedback", symbol: WebIcon.symbol("refresh-cw"), badge: nil,
                 screen: "Feedback", context: "MIMIR / GOVERNED FEEDBACK", asideWidth: 330),
        ]),
        ("SERVE", [
            Item(id: "models", label: "Model plane", symbol: WebIcon.symbol("cpu"), badge: "4",
                 screen: "Model Plane", context: "MIMIR / MODEL PLANE", asideWidth: 330),
        ]),
    ]

    /// The navigation item that opens `screen` (a `ConsoleCatalog` name).
    public static func item(for screen: String) -> Item? {
        sections.flatMap(\.items).first { $0.screen == screen }
    }

    /// The screens in the web's navigation order.
    public static var screens: [String] { sections.flatMap(\.items).map(\.screen) }
}

/// The status a Mimir row carries (`status`) as a pill: `online` and `healthy` read as
/// the web's, a live seal in the accent's live colour.
func mimirPill(_ status: String, _ label: String) -> WebStatusPill {
    let live = MGWebAccent.mimir.text
    switch status {
    case "secure": return WebStatusPill(.secure, label, live: live)
    case "online": return WebStatusPill(.online, label, live: live)
    case "healthy": return WebStatusPill(.healthy, label, live: live)
    case "warn": return WebStatusPill(.warn, label, live: live)
    case "danger": return WebStatusPill(.danger, label, live: live)
    case "gated": return WebStatusPill(.gated, label, live: live)
    default: return WebStatusPill(.offline, label, live: live)
    }
}

/// The Evals table's `PassBar`: a 70 pt bar, warn under the gate or on a drifting
/// suite, secure otherwise, and the figure.
public struct MimirPassBar: View {
    let value: Double
    let warn: Bool

    public init(_ value: Double, warn: Bool) {
        self.value = min(max(value, 0), 100)
        self.warn = warn
    }

    public var body: some View {
        HStack(spacing: 8) {
            ZStack(alignment: .leading) {
                Capsule().fill(MGWeb.surfaceInset)
                Capsule().fill(warn ? MGWeb.warn : MGWeb.secure).frame(width: 70 * value / 100)
            }
            .frame(width: 70, height: 5)
            .accessibilityHidden(true)
            Text("\(mimirPercent(value))%")
                .font(MGWebFont.mono(12, .caption))
                .foregroundStyle(MGWeb.textBody)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("\(mimirPercent(value)) percent"))
        .accessibilityAddTraits(.isStaticText)
    }
}

/// A per cent as the web prints a number: no decimals when whole (`96`), else one (`91.7`).
public func mimirPercent(_ v: Double) -> String {
    v.truncatingRemainder(dividingBy: 1) == 0 ? String(format: "%.0f", v) : String(format: "%.1f", v)
}

/// The Evals run's `MeterBar` (`warnAt 90`, `dangerAt 85`, `invert`: a score, lower is
/// worse): secure from the 90 % gate, warn under it, danger under 85 %; 150 pt wide, the
/// figure after it.
public struct MimirScoreMeter: View {
    let value: Double

    public init(_ value: Double) {
        self.value = min(max(value, 0), 100)
    }

    private var colour: Color {
        if value < 85 { return MGWeb.danger }
        if value < 90 { return MGWeb.warn }
        return MGWeb.secure
    }

    public var body: some View {
        HStack(spacing: 8) {
            ZStack(alignment: .leading) {
                Capsule().fill(MGWeb.surfaceInset)
                Capsule().fill(colour).frame(width: 150 * value / 100)
            }
            .frame(width: 150, height: 5)
            .accessibilityHidden(true)
            Text("\(mimirPercent(value))%")
                .font(MGWebFont.mono(12, .caption))
                .foregroundStyle(MGWeb.textBody)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Score: \(mimirPercent(value)) percent"))
        .accessibilityAddTraits(.isStaticText)
    }
}
