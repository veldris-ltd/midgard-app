import MidgardCore
import SwiftUI

/// Forge's console frame as the web suite draws it (`ui/shared/src/nav.ts`, product
/// `forge`, at the signed tag): the RESTRICTED · DATA PLANE banner, the header's one
/// seal, and the navigation's two sections in the web's order, with the web's labels,
/// icons and badges, each opening one of the app's screens (`ConsoleCatalog`'s Forge
/// screens), and each screen's context line and inspector width. These are the web
/// console's fixed chrome, not data. [FR-C-1] [DD-7]
public enum ForgeChrome {
    /// `NAV.forge.banner`.
    public static let banner = (level: MarkingLevelChip.restricted, text: "RESTRICTED · DATA PLANE")
    public static let seals: [(WebStatusPill.Status, String)] = [(.secure, "PQ-SEALED")]
    public static let accent = MGWebAccent.forge

    /// One navigation item: the web's id, label, icon and badge, and the screen it opens.
    public struct Item: Identifiable {
        public let id: String
        public let label: String
        public let symbol: String
        public let badge: String?
        public let screen: String
        /// `DATA OPS / …`, the header's context line on this screen (Pipelines names
        /// the selected pipeline's context, `ForgePipeline.context`).
        public let context: String
        /// The screen's inspector rail on the Mac (`asideWidth`, 330 by default).
        public let asideWidth: CGFloat
    }

    public static let sections: [(title: String, items: [Item])] = [
        ("DATA OPS", [
            Item(id: "pipeline", label: "Pipelines", symbol: WebIcon.symbol("workflow"), badge: "38",
                 screen: "Pipeline Builder", context: "PIPELINES / BUILDER", asideWidth: 320),
            Item(id: "connectors", label: "Connectors", symbol: WebIcon.symbol("database"), badge: nil,
                 screen: "Connectors", context: "DATA OPS / CONNECTORS", asideWidth: 330),
            Item(id: "datasets", label: "Datasets", symbol: WebIcon.symbol("table"), badge: "412",
                 screen: "Datasets", context: "DATA OPS / DATASETS", asideWidth: 330),
            Item(id: "health", label: "Data health", symbol: WebIcon.symbol("activity"), badge: "3",
                 screen: "Data Health", context: "DATA OPS / DATA HEALTH", asideWidth: 330),
            Item(id: "schedules", label: "Schedules", symbol: WebIcon.symbol("clock"), badge: nil,
                 screen: "Schedules", context: "DATA OPS / SCHEDULES & TRIGGERS", asideWidth: 330),
            Item(id: "ide", label: "Transform IDE", symbol: WebIcon.symbol("code"), badge: nil,
                 screen: "Transform IDE", context: "DATA OPS / TRANSFORM IDE · DEV/FUSE-FIX", asideWidth: 330),
        ]),
        ("FABRIC", [
            Item(id: "projection", label: "Object projection", symbol: WebIcon.symbol("boxes"), badge: nil,
                 screen: "Object Projection", context: "DATA OPS / OBJECT PROJECTION", asideWidth: 330),
        ]),
    ]

    /// The navigation item that opens `screen` (a `ConsoleCatalog` name).
    public static func item(for screen: String) -> Item? {
        sections.flatMap(\.items).first { $0.screen == screen }
    }

    /// The screens in the web's navigation order.
    public static var screens: [String] { sections.flatMap(\.items).map(\.screen) }
}

/// The status a Forge row carries (`state`, `status`) as a pill: `online` pulses on the
/// web (a live stream), drawn here in the accent's live colour.
func forgePill(_ state: String, _ label: String) -> WebStatusPill {
    let live = MGWebAccent.forge.text
    switch state {
    case "secure": return WebStatusPill(.secure, label, live: live)
    case "online": return WebStatusPill(.online, label, live: live)
    case "healthy": return WebStatusPill(.healthy, label, live: live)
    case "warn": return WebStatusPill(.warn, label, live: live)
    case "danger": return WebStatusPill(.danger, label, live: live)
    case "live": return WebStatusPill(.live, label, live: live)
    default: return WebStatusPill(.offline, label, live: live)
    }
}

/// The Data Health inspector's `MeterBar` with `warnAt`, `dangerAt` and `invert` (a pass
/// rate: lower is worse): secure from 98 %, warn under it, danger under 95 %; the label
/// before the bar, the figure after it.
public struct ForgePassMeter: View {
    let value: Double
    let label: String
    let width: CGFloat

    public init(_ value: Double, label: String = "PASS", width: CGFloat = 150) {
        self.value = min(max(value, 0), 100)
        self.label = label
        self.width = width
    }

    private var colour: Color {
        if value < 95 { return MGWeb.danger }
        if value < 98 { return MGWeb.warn }
        return MGWeb.secure
    }

    public var body: some View {
        HStack(spacing: 8) {
            Text(label)
                .font(MGWebFont.mono(10))
                .mgTracking(0.12, size: MGWebFont.monoSize(10))
                .foregroundStyle(MGWeb.textFaint)
            ZStack(alignment: .leading) {
                Capsule().fill(MGWeb.surfaceInset)
                Capsule().fill(colour).frame(width: width * value / 100)
            }
            .frame(width: width, height: 5)
            .accessibilityHidden(true)
            Text(String(format: value.truncatingRemainder(dividingBy: 1) == 0 ? "%.0f%%" : "%.1f%%", value))
                .font(MGWebFont.mono(12, .caption))
                .foregroundStyle(MGWeb.textBody)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("\(label.capitalized): \(String(format: "%.1f", value)) percent"))
    }
}

/// A chain of hops as the web draws lineage (`LINEAGE`, the projection's source →
/// mapping → target): each hop a card with its icon and mono label, chevrons between,
/// one highlighted in the accent; wrapping where the row does not fit.
public struct ForgeHopChain: View {
    public struct Hop: Hashable {
        public let symbol: String
        public let label: String
        public let highlighted: Bool
        /// A plain caption between hops (the projection's "MAPPING · 5 RULES").
        public let caption: Bool

        public init(symbol: String, label: String, highlighted: Bool = false, caption: Bool = false) {
            self.symbol = symbol
            self.label = label
            self.highlighted = highlighted
            self.caption = caption
        }
    }

    let hops: [Hop]
    let identifier: String

    public init(_ hops: [Hop], identifier: String) {
        self.hops = hops
        self.identifier = identifier
    }

    public var body: some View {
        FlowRow(spacing: 8) {
            ForEach(Array(hops.enumerated()), id: \.offset) { i, hop in
                HStack(spacing: 8) {
                    if hop.caption {
                        Text(hop.label)
                            .font(MGWebFont.mono(10))
                            .mgTracking(0.1, size: MGWebFont.monoSize(10))
                            .foregroundStyle(MGWeb.textFaint)
                    } else {
                        HStack(spacing: 7) {
                            Image(systemName: hop.symbol).font(MGWebFont.sans(13))
                                .foregroundStyle(MGWebAccent.forge.text).accessibilityHidden(true)
                            Text(hop.label).font(MGWebFont.mono(11)).foregroundStyle(MGWeb.grey100)
                        }
                        .padding(.horizontal, 11)
                        .padding(.vertical, 7)
                        .background(RoundedRectangle(cornerRadius: 6).fill(MGWeb.surfaceCard))
                        .overlay(RoundedRectangle(cornerRadius: 6)
                            .strokeBorder(hop.highlighted ? MGWebAccent.forge.fill : MGWeb.border))
                    }
                    if i < hops.count - 1 {
                        Image(systemName: "chevron.right").font(MGWebFont.sans(11))
                            .foregroundStyle(MGWeb.decorFaint).accessibilityHidden(true)
                    }
                }
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.bgSunken))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(hops.map(\.label).joined(separator: ", then ")))
        .accessibilityIdentifier(identifier)
    }
}
