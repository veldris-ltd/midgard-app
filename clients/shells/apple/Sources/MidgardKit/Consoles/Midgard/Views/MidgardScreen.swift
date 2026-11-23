import MidgardCore
import SwiftUI

/// How one Midgard screen opens another with something to show, as the web's links
/// carry a query string: "Explore instances" (`/search?type=`), "Open in
/// Search-around" (`/search?seed=`) and the header's search (`/data?q=`). [DD-1]
public enum MidgardLink: Hashable, Sendable {
    case exploreType(String)
    case seed(String)
    case text(String)

    /// As a navigation path element on the iPhone: `<screen>\n<kind>:<value>`.
    public func encoded(for screen: String) -> String {
        switch self {
        case let .exploreType(t): return "\(screen)\ntype:\(t)"
        case let .seed(s): return "\(screen)\nseed:\(s)"
        case let .text(q): return "\(screen)\nq:\(q)"
        }
    }

    /// A path element back as its screen and link.
    public static func decode(_ element: String) -> (screen: String, link: MidgardLink?) {
        let parts = element.split(separator: "\n", maxSplits: 1).map(String.init)
        guard parts.count == 2, let colon = parts[1].firstIndex(of: ":") else { return (element, nil) }
        let kind = parts[1][..<colon]
        let value = String(parts[1][parts[1].index(after: colon)...])
        switch kind {
        case "type": return (parts[0], .exploreType(value))
        case "seed": return (parts[0], .seed(value))
        case "q": return (parts[0], .text(value))
        default: return (parts[0], nil)
        }
    }
}

/// One of Midgard's eleven screens, matched to the web console's
/// (`products/midgard/ui/src/screens/`), over the release's sample data: on the Mac
/// (`compact` false) the main column and the inspector rail side by side inside the
/// console frame; on the iPhone the main column under the classification banner and
/// header, the inspector in a sheet. `open` goes to another screen (with a link).
/// [FR-C-1] [DD-1] [DD-7]
public struct MidgardScreen: View {
    @ObservedObject var app: AppModel
    let persona: String
    let screen: String
    let compact: Bool
    let link: MidgardLink?
    let open: (String, MidgardLink?) -> Void

    public init(app: AppModel, persona: String, screen: String, compact: Bool, link: MidgardLink? = nil,
                open: @escaping (String, MidgardLink?) -> Void) {
        self.app = app
        self.persona = persona
        self.screen = screen
        self.compact = compact
        self.link = link
        self.open = open
    }

    public var body: some View {
        let ctx = MidgardScreenContext(app: app, persona: persona, compact: compact, open: open)
        Group {
            switch screen {
            case "Object Fabric": ObjectFabricScreen(ctx: ctx)
            case "Data Search": DataSearchScreen(ctx: ctx, link: link)
            case "Search Around": SearchAroundScreen(ctx: ctx, link: link)
            case "Policies": PoliciesScreen(ctx: ctx)
            case "Identity": IdentityScreen(ctx: ctx)
            case "Keys": KeysScreen(ctx: ctx)
            case "Audit": AuditScreen(ctx: ctx)
            case "Compute": ComputeScreen(ctx: ctx)
            case "Observability": ObservabilityScreen(ctx: ctx)
            case "Resilience": ResilienceScreen(ctx: ctx)
            default: SdkScreen(ctx: ctx)
            }
        }
        // A new link (or persona, or a gateway connected) is a new page, as a new URL is
        // on the web.
        .id("\(screen)|\(persona)|\(String(describing: link))|\(app.userData.live?.host ?? "")")
    }
}

/// What every Midgard screen is given.
struct MidgardScreenContext {
    let app: AppModel
    let persona: String
    let compact: Bool
    let open: (String, MidgardLink?) -> Void

    /// `mg.content.midgard.<slug>`, the screen's main column.
    func content(_ screen: String) -> String { "mg.content.midgard.\(ConsoleCatalog.slug(screen))" }
    /// `mg.detail.<slug>`, the screen's inspector.
    func detail(_ screen: String) -> String { "mg.detail.\(ConsoleCatalog.slug(screen))" }

    /// On the iPhone the console's banner and header open the page (the web's narrow
    /// layout), in the scroll.
    @MainActor @ViewBuilder func header(_ screen: String) -> some View {
        if compact {
            VStack(spacing: 0) {
                WebClassificationBanner(level: MidgardChrome.banner.level, text: MidgardChrome.banner.text, compact: true)
                WebConsoleHeader(product: "Midgard", accent: .midgard,
                                 context: MidgardChrome.item(for: screen)?.context ?? "", seals: MidgardChrome.seals(live: app.userData.live),
                                 compact: true) { EmptyView() }
            }
        }
    }
}

/// The iPhone's button that opens a screen's inspector sheet ("Inspect …"): one element
/// over its drawn text (`WholeElement`), so a button the page's edge cuts above the
/// console bar is not measured as a cut-off text.
struct MidgardInspectButton: View {
    let title: String
    var identifier = "mg.midgard.inspect"
    let action: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "sidebar.right")
            Text(title).font(MGWebFont.sans(14).weight(.semibold)).lineLimit(1)
        }
        .foregroundStyle(MGWeb.textHeading)
        .padding(.horizontal, 14)
        .frame(maxWidth: .infinity, minHeight: 44)
        .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.borderStrong))
        .contentShape(Rectangle())
        .modifier(WholeElement(label: title, traits: .isButton, identifier: identifier, action: action))
    }
}

/// "N ROWS WITHHELD · ABOVE YOUR CLEARANCE", for a persona below some of a screen's rows
/// (as the Operating Picture says it for tracks).
struct MidgardWithheld: View {
    let count: Int
    let noun: String

    var body: some View {
        if count > 0 {
            WebMicro("\(count) \(noun.uppercased())\(count == 1 ? "" : "S") WITHHELD · ABOVE YOUR CLEARANCE", tracking: 0.1)
                .accessibilityIdentifier("mg.midgard.withheld")
        }
    }
}

/// The Mac's console frame for Midgard (`ConsoleShell`): the classification banner, the
/// header (mark, the screen's context, the seals, then the toolbar the app passes), and
/// the navigation beside the screen, 1 pt rules between. [DD-7]
public struct MidgardConsoleFrame<Nav: View, Content: View, Trailing: View>: View {
    let context: String
    let seals: [(WebStatusPill.Status, String)]
    let nav: Nav
    let content: Content
    let trailing: Trailing

    /// `seals` are the header's (`MidgardChrome.seals(live:)`).
    public init(context: String, seals: [(WebStatusPill.Status, String)] = MidgardChrome.seals, @ViewBuilder nav: () -> Nav,
                @ViewBuilder content: () -> Content, @ViewBuilder trailing: () -> Trailing) {
        self.context = context
        self.seals = seals
        self.nav = nav()
        self.content = content()
        self.trailing = trailing()
    }

    public var body: some View {
        WebConsoleFrame(banner: MidgardChrome.banner, product: "Midgard", accent: .midgard, context: context,
                        seals: seals) {
            nav
        } content: {
            content
        } trailing: {
            trailing
        }
    }
}

/// A console's frame on the Mac (`ConsoleShell`): the classification banner, the header
/// (mark, the screen's context, the seals, then the toolbar the app passes), and the
/// navigation beside the screen, 1 pt rules between. Midgard's and Admin's. [DD-7]
public struct WebConsoleFrame<Nav: View, Content: View, Trailing: View>: View {
    let banner: (level: MarkingLevelChip, text: String)
    let product: String
    let accent: MGWebAccent
    let tile: WebProductMark.Tile?
    let context: String
    let seals: [(WebStatusPill.Status, String)]
    let nav: Nav
    let content: Content
    let trailing: Trailing

    public init(banner: (level: MarkingLevelChip, text: String), product: String, accent: MGWebAccent,
                tile: WebProductMark.Tile? = nil, context: String, seals: [(WebStatusPill.Status, String)],
                @ViewBuilder nav: () -> Nav, @ViewBuilder content: () -> Content, @ViewBuilder trailing: () -> Trailing) {
        self.banner = banner
        self.product = product
        self.accent = accent
        self.tile = tile
        self.context = context
        self.seals = seals
        self.nav = nav()
        self.content = content()
        self.trailing = trailing()
    }

    public var body: some View {
        VStack(spacing: 0) {
            WebClassificationBanner(level: banner.level, text: banner.text)
            WebConsoleHeader(product: product, accent: accent, context: context, seals: seals, tile: tile) { trailing }
            HStack(spacing: 0) {
                nav
                    .frame(width: 232)
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel(Text("Navigation"))
                Rectangle().fill(MGWeb.border).frame(width: 1).accessibilityHidden(true)
                content.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .frame(maxHeight: .infinity)
        }
        .background(MGWeb.bgPage)
    }
}

/// The header's search box (`ConsoleToolbar`): Return opens Data search for the text
/// across every object type.
public struct MidgardHeaderSearch: View {
    @State private var text = ""
    let onSearch: (String) -> Void

    public init(onSearch: @escaping (String) -> Void) {
        self.onSearch = onSearch
    }

    public var body: some View {
        WebTextField(nil, placeholder: "Search all object data…", text: $text, symbol: "magnifyingglass",
                     identifier: "mg.midgard.header.search", width: 300) {
            let q = text.trimmingCharacters(in: .whitespaces)
            if !q.isEmpty { onSearch(q) }
        }
        .help("Press Return to search every object type for this text")
    }
}

/// The `midgard status` terminal under the navigation (the screens' `navFooter`).
public struct MidgardStatusFooter: View {
    public init() {}

    public var body: some View {
        WebTerminal(lines: ["> midgard status", "# ALL SYSTEMS NOMINAL"], small: true)
            .padding(.horizontal, 0)
    }
}
