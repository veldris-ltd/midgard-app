import MidgardCore
import SwiftUI

/// Midgard's console frame as the web suite draws it (`ui/shared/src/nav.ts`, product
/// `midgard`, at the signed tag): the classification banner, the header's seals and the
/// navigation's three sections in the web's order, with the web's labels and icons,
/// each opening one of the app's screens (`ConsoleCatalog`'s Midgard screens), and each
/// screen's context line and whether it shows the `midgard status` footer. These are
/// the web console's fixed chrome, not data. [DD-1] [DD-7]
public enum MidgardChrome {
    /// `ClassificationBanner level="confidential"` with its default text.
    public static let banner = (level: MarkingLevelChip.confidential, text: "CONFIDENTIAL · RECIPIENT EYES ONLY")
    /// The web console's seals over the sample data (its own fixed chrome).
    public static let seals: [(WebStatusPill.Status, String)] = [(.secure, "PQ-SEALED"), (.online, "AIR-GAP")]

    /// The seals the header shows: the web's over the sample data; connected to a
    /// gateway, only what this app knows of the connection, its transport ("HTTPS", or
    /// "LOOPBACK" for plain HTTP to this device). A gateway reached over HTTPS or
    /// loopback is neither PQ-sealed nor air-gapped from the app's point of view, so
    /// those seals are not shown then. [AC-C-13]
    public static func seals(live: LiveSession?) -> [(WebStatusPill.Status, String)] {
        guard let live else { return seals }
        return [(.online, "LIVE · \(live.transport)")]
    }
    public static let accent = MGWebAccent.midgard

    /// One navigation item: the web's id, label and icon, and the screen it opens.
    public struct Item: Identifiable {
        public let id: String
        public let label: String
        public let symbol: String
        public let screen: String
        /// `ENCLOSURE / …`, the header's context line on this screen.
        public let context: String
        /// Whether the screen shows the `midgard status` terminal under the navigation.
        public let statusFooter: Bool
    }

    public static let sections: [(title: String, items: [Item])] = [
        ("FABRIC", [
            Item(id: "objects", label: "Object types", symbol: WebIcon.symbol("boxes"), screen: "Object Fabric",
                 context: "ENCLOSURE / OBJECT FABRIC", statusFooter: true),
            Item(id: "data", label: "Data search", symbol: WebIcon.symbol("filter"), screen: "Data Search",
                 context: "ENCLOSURE / DATA SEARCH", statusFooter: true),
            Item(id: "search", label: "Search-around", symbol: WebIcon.symbol("search"), screen: "Search Around",
                 context: "ENCLOSURE / SEARCH-AROUND", statusFooter: true),
        ]),
        ("GOVERNANCE", [
            Item(id: "policies", label: "Policies & markings", symbol: WebIcon.symbol("shield-check"), screen: "Policies",
                 context: "ENCLOSURE / GOVERNANCE", statusFooter: true),
            Item(id: "audit", label: "Audit lineage", symbol: WebIcon.symbol("scroll-text"), screen: "Audit",
                 context: "ENCLOSURE / AUDIT LINEAGE", statusFooter: true),
        ]),
        ("PLATFORM", [
            Item(id: "identity", label: "Identity & access", symbol: WebIcon.symbol("fingerprint"), screen: "Identity",
                 context: "ENCLOSURE / IDENTITY & ACCESS", statusFooter: true),
            Item(id: "kms", label: "Keys & crypto", symbol: WebIcon.symbol("key"), screen: "Keys",
                 context: "ENCLOSURE / KEYS & CRYPTO", statusFooter: true),
            Item(id: "compute", label: "Compute", symbol: WebIcon.symbol("server"), screen: "Compute",
                 context: "ENCLOSURE / COMPUTE", statusFooter: true),
            Item(id: "sdk", label: "SDK & APIs", symbol: WebIcon.symbol("code"), screen: "SDK",
                 context: "ENCLOSURE / SDK & APIs", statusFooter: false),
            Item(id: "obs", label: "Observability", symbol: WebIcon.symbol("activity"), screen: "Observability",
                 context: "ENCLOSURE / OBSERVABILITY", statusFooter: false),
            Item(id: "resilience", label: "Resilience", symbol: WebIcon.symbol("refresh-cw"), screen: "Resilience",
                 context: "ENCLOSURE / RESILIENCE", statusFooter: false),
        ]),
    ]

    /// The navigation item that opens `screen` (a `ConsoleCatalog` name).
    public static func item(for screen: String) -> Item? {
        sections.flatMap(\.items).first { $0.screen == screen }
    }
}

/// The console header under the classification banner (`ConsoleShell`'s top bar): the
/// product mark, the context line, the seals and whatever the app puts at the end
/// (the toolbar). Compact (the iPhone): the mark and the first seal only, as the web's
/// narrow layout.
public struct WebConsoleHeader<Trailing: View>: View {
    @Environment(\.dynamicTypeSize) private var typeSize
    let product: String
    let accent: MGWebAccent
    let context: String
    let seals: [(WebStatusPill.Status, String)]
    let compact: Bool
    let tile: WebProductMark.Tile?
    let trailing: Trailing

    public init(product: String, accent: MGWebAccent, context: String, seals: [(WebStatusPill.Status, String)],
                compact: Bool = false, tile: WebProductMark.Tile? = nil, @ViewBuilder trailing: () -> Trailing) {
        self.product = product
        self.accent = accent
        self.context = context
        self.seals = seals
        self.compact = compact
        self.tile = tile
        self.trailing = trailing()
    }

    private var shownSeals: [(WebStatusPill.Status, String)] { compact ? Array(seals.prefix(1)) : seals }

    public var body: some View {
        Group {
            if compact && !typeSize.isAccessibilitySize {
                // The web's narrow header wraps (`flex-wrap`): the toolbar goes under the
                // mark and seal when the three do not fit on one line.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: 8) {
                        WebProductMark(name: product, accent: accent, tile: tile)
                        Spacer(minLength: 8)
                        seal
                        trailing
                    }
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 8) {
                            WebProductMark(name: product, accent: accent, tile: tile)
                            Spacer(minLength: 8)
                            seal
                        }
                        trailing
                    }
                }
            } else {
                full
            }
        }
        .padding(.horizontal, compact ? 10 : 16)
        .padding(.vertical, 6)
        .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
        .background(MGWeb.bgSunken)
        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.border).frame(height: 1) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("\(product) header"))
    }

    private var seal: some View {
        ForEach(Array(shownSeals.enumerated()), id: \.offset) { _, seal in
            WebStatusPill(seal.0, seal.1, live: accent.text)
        }
    }

    private var full: some View {
        (typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
            : AnyLayout(HStackLayout(spacing: compact ? 8 : 14))) {
            WebProductMark(name: product, accent: accent, tile: tile)
            if !compact {
                Text(context)
                    .font(MGWebFont.mono(11))
                    .mgTracking(0.14, size: MGWebFont.monoSize(11))
                    .foregroundStyle(MGWeb.textFaint)
                    .lineLimit(1)
                    .layoutPriority(-1)
                    .accessibilityIdentifier("mg.header.context")
            }
            if !typeSize.isAccessibilitySize { Spacer(minLength: 8) }
            seal
            trailing
        }
    }
}

/// A console page's body: on the Mac the main column (padded 20 × 24, scrolling) and
/// the inspector rail (340 pt on the sunken surface, padded 18, its last group at the
/// foot, as the web's `marginTop: auto`); on the iPhone the main column alone (padded
/// 12 × 14), with the inspector in a sheet. [DD-7]
public struct WebConsolePage<Main: View, Aside: View, AsideFoot: View>: View {
    let compact: Bool
    let identifier: String
    let asideIdentifier: String
    @Binding var inspecting: Bool
    let top: AnyView?
    let asideWidth: CGFloat
    let main: Main
    let aside: Aside
    let asideFoot: AsideFoot

    /// `top` heads the scroll on the iPhone (the console's banner and header);
    /// `asideWidth` is the screen's inspector rail on the Mac (the web's `asideWidth`).
    public init(compact: Bool, identifier: String, asideIdentifier: String, inspecting: Binding<Bool>, top: AnyView? = nil,
                asideWidth: CGFloat = 340, @ViewBuilder main: () -> Main, @ViewBuilder aside: () -> Aside,
                @ViewBuilder asideFoot: () -> AsideFoot) {
        self.compact = compact
        self.identifier = identifier
        self.asideIdentifier = asideIdentifier
        self._inspecting = inspecting
        self.top = top
        self.asideWidth = asideWidth
        self.main = main()
        self.aside = aside()
        self.asideFoot = asideFoot()
    }

    public var body: some View {
        if compact {
            mainColumn(padding: EdgeInsets(top: 14, leading: 12, bottom: 20, trailing: 12))
                .sheet(isPresented: $inspecting) {
                    NavigationStack {
                        asideColumn
                            .toolbar {
                                ToolbarItem(placement: .confirmationAction) {
                                    Button("Done") { inspecting = false }
                                        .accessibilityIdentifier("mg.inspector.done")
                                }
                            }
                    }
                    .presentationDetents([.medium, .large])
                    .presentationDragIndicator(.visible)
                }
        } else {
            HStack(spacing: 0) {
                mainColumn(padding: EdgeInsets(top: 20, leading: 24, bottom: 24, trailing: 24))
                Rectangle().fill(MGWeb.border).frame(width: 1).accessibilityHidden(true)
                asideColumn.frame(width: asideWidth)
            }
        }
    }

    private func mainColumn(padding: EdgeInsets) -> some View {
        ScrollView {
            VStack(spacing: 0) {
                if let top { top }
                VStack(alignment: .leading, spacing: 16) { main }
                    .padding(padding)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .background(MGWeb.bgPage)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(identifier)
    }

    private var asideColumn: some View {
        GeometryReader { geo in
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    aside
                    Spacer(minLength: 0)
                    asideFoot
                }
                .padding(18)
                .frame(maxWidth: .infinity, minHeight: max(0, geo.size.height), alignment: .topLeading)
            }
        }
        .background(MGWeb.bgSunken)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Inspector"))
        .accessibilityIdentifier(asideIdentifier)
    }
}

/// The inspector's heading row: an accent icon, the title (mono 14 semibold, or the
/// sans h4) and whatever sits at the end (a marking chip, a status pill).
public struct WebInspectorTitle<Trailing: View>: View {
    let symbol: String
    let title: String
    let mono: Bool
    let accent: MGWebAccent
    let identifier: String?
    let trailing: Trailing

    public init(symbol: String, title: String, mono: Bool = true, accent: MGWebAccent = .midgard,
                identifier: String? = nil, @ViewBuilder trailing: () -> Trailing) {
        self.symbol = symbol
        self.title = title
        self.mono = mono
        self.accent = accent
        self.identifier = identifier
        self.trailing = trailing()
    }

    public var body: some View {
        FlowRow(spacing: 10) {
            HStack(spacing: 10) {
                Image(systemName: symbol).font(MGWebFont.sans(16)).foregroundStyle(accent.text).accessibilityHidden(true)
                Text(title)
                    .font(mono ? MGWebFont.mono(14, .headline).weight(.semibold) : MGWebFont.display(20, .title3).weight(.bold))
                    .foregroundStyle(MGWeb.textHeading)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                    .modifier(OptionalIdentifier(identifier))
            }
            trailing
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// The two-up stat card at the top of an inspector (`Card size="sm"` holding two
/// `StatBlock`s).
public struct WebStatPair: View {
    let first: (value: String, label: String, accent: Bool)
    let second: (value: String, label: String, accent: Bool)
    let accent: MGWebAccent

    public init(_ first: (value: String, label: String, accent: Bool), _ second: (value: String, label: String, accent: Bool),
                accent: MGWebAccent = .midgard) {
        self.first = first
        self.second = second
        self.accent = accent
    }

    public var body: some View {
        WebPanel {
            HStack(alignment: .top, spacing: 12) {
                WebStatBlock(value: first.value, label: first.label, accent: first.accent ? accent : nil)
                    .frame(maxWidth: .infinity, alignment: .leading)
                WebStatBlock(value: second.value, label: second.label, accent: second.accent ? accent : nil)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// A row of provenance tags, wrapping.
public struct WebProvenance: View {
    let tags: [(String, String)]
    let titled: Bool

    public init(_ tags: [(String, String)], titled: Bool = false) {
        self.tags = tags
        self.titled = titled
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if titled { WebLabel("PROVENANCE") }
            FlowRow(spacing: 6) {
                ForEach(Array(tags.enumerated()), id: \.offset) { _, tag in WebProvenanceTag(tag.0, title: tag.1) }
            }
        }
    }
}
