import MidgardCore
import SwiftUI

/// One of Heimdall's screens after the Operating Picture, matched to the web console's
/// (`products/heimdall/ui/src/screens/`), over the release's sample data: Entities,
/// Feeds, Picture Sync, Workflows and Decision Lineage. On the Mac (`compact` false) the
/// main column and the inspector rail side by side inside the console frame; on the
/// iPhone the main column under the classification banner and header (with the
/// screen's toolbar item), the inspector in a sheet. `open` goes to another Heimdall
/// screen ("Open picture"). Heimdall is not served by a gateway: connected to one, the
/// screens keep the sample data and say so. [FR-C-1] [FR-H-1] [DD-7]
public struct HeimdallScreen: View {
    @ObservedObject var app: AppModel
    let persona: String
    let screen: String
    let compact: Bool
    let toolbar: HeimdallToolbar?
    /// The screen's own toolbar state, when the app passes none (the iPhone).
    @StateObject private var ownToolbar = HeimdallToolbar()
    let open: (String) -> Void

    /// `toolbar` carries what the screen's toolbar item shows to the Mac's console
    /// header (`HeimdallToolbarItem`); the iPhone draws the item in the page.
    public init(app: AppModel, persona: String, screen: String, compact: Bool, toolbar: HeimdallToolbar? = nil,
                open: @escaping (String) -> Void) {
        self.app = app
        self.persona = persona
        self.screen = screen
        self.compact = compact
        self.toolbar = toolbar
        self.open = open
    }

    public var body: some View {
        let ctx = HeimdallScreenContext(app: app, persona: persona, compact: compact, bar: toolbar ?? ownToolbar, open: open)
        Group {
            switch screen {
            case "Entities": HeimdallEntitiesScreen(ctx: ctx)
            case "Feeds": HeimdallFeedsScreen(ctx: ctx)
            case "Picture Sync": HeimdallSyncScreen(ctx: ctx)
            case "Workflows": HeimdallWorkflowsScreen(ctx: ctx)
            default: HeimdallLineageScreen(ctx: ctx)
            }
        }
        .id("\(screen)|\(persona)")
        .safeAreaInset(edge: .top, spacing: 0) {
            if let live = app.userData.live {
                WebNotice("NOT SERVED BY \(live.host.uppercased()) · HEIMDALL SHOWS THE SAMPLE DATA, NOT THIS GATEWAY'S.",
                          tone: .warn, identifier: "mg.heimdall.notserved")
                    .padding(8)
                    .background(MGWeb.bgPage)
            }
        }
    }
}

/// What every Heimdall screen is given.
struct HeimdallScreenContext {
    let app: AppModel
    let persona: String
    let compact: Bool
    let bar: HeimdallToolbar
    let open: (String) -> Void
    let accent = MGWebAccent.heimdall

    /// `mg.content.heimdall.<slug>`, the screen's main column.
    func content(_ screen: String) -> String { "mg.content.heimdall.\(ConsoleCatalog.slug(screen))" }
    /// `mg.detail.<slug>`, the screen's inspector.
    func detail(_ screen: String) -> String { "mg.detail.\(ConsoleCatalog.slug(screen))" }
    /// The screen's inspector width on the Mac.
    func asideWidth(_ screen: String) -> CGFloat { HeimdallChrome.item(for: screen)?.asideWidth ?? 330 }

    /// On the iPhone the console's banner and header open the page, with the screen's
    /// toolbar item at the header's end (the web's narrow layout), in the scroll.
    func header<Toolbar: View>(_ screen: String, @ViewBuilder toolbar: () -> Toolbar) -> AnyView? {
        guard compact else { return nil }
        let bar = toolbar()
        return AnyView(VStack(spacing: 0) {
            WebClassificationBanner(level: HeimdallChrome.banner.level, text: HeimdallChrome.banner.text, compact: true)
            HeimdallHeader(compact: true, context: HeimdallChrome.item(for: screen)?.context ?? "") { bar }
        })
    }

    /// The selection rule every list shares: select the row; on the iPhone also open
    /// the inspector.
    func select(_ index: Int, _ set: (Int) -> Void, _ inspecting: Binding<Bool>) {
        set(index)
        if compact { inspecting.wrappedValue = true }
    }
}

/// What a screen's toolbar item shows (the web's `toolbar` prop): the screens publish
/// it, and the Mac's console header draws it (`HeimdallToolbarItem`).
@MainActor
public final class HeimdallToolbar: ObservableObject {
    /// Workflows' "N AWAITING AUTHORISATION": the human-gated workflows shown.
    @Published public var awaiting = 0

    public init() {}
}

/// A screen's toolbar item (the web's `toolbar` prop): a badge or a button. The buttons
/// the web leaves without a handler are disabled with the reason.
public struct HeimdallToolbarItem: View {
    let screen: String
    @ObservedObject var toolbar: HeimdallToolbar

    public init(screen: String, toolbar: HeimdallToolbar) {
        self.screen = screen
        self.toolbar = toolbar
    }

    private var awaiting: Int { toolbar.awaiting }

    public var body: some View {
        switch screen {
        case "Entities":
            EmptyView()
        case "Feeds":
            WebButton("Register feed", symbol: WebIcon.symbol("plus"), size: .sm,
                      disabledReason: "Not available in this app: feeds are registered with the enclosure. Your own feeds are added in Your Data, unverified.",
                      accent: .heimdall, identifier: "mg.heimdall.feeds.register")
        case "Picture Sync":
            WebButton("Cut picture snapshot", symbol: WebIcon.symbol("download"), variant: .secondary, size: .sm,
                      disabledReason: "Not available in this app: snapshots are cut by the sync service",
                      identifier: "mg.heimdall.sync.snapshot")
        case "Workflows":
            WebBadge("\(awaiting) AWAITING AUTHORISATION", tone: .warn).accessibilityIdentifier("mg.heimdall.toolbar.awaiting")
        case "Decision Lineage":
            WebButton("Export for review", symbol: WebIcon.symbol("download"), variant: .secondary, size: .sm,
                      disabledReason: "Not available in this app: the lineage is exported by the enclosure",
                      identifier: "mg.heimdall.lineage.export")
        default:
            EmptyView()
        }
    }
}

/// An inspector's small label and its rows: the key faint in a fixed column, the value
/// in the body colour, a divider under each (the web's `TRACK HISTORY`, `REPLAY`).
struct HeimdallHistory: View {
    let title: String
    let rows: [(String, String)]
    var keyWidth: CGFloat = 44
    var upperKeys = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            WebLabel(title).padding(.bottom, 8)
            ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(upperKeys ? row.0.uppercased() : row.0)
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(MGWeb.textFaint)
                        .frame(width: keyWidth, alignment: .leading)
                    Text(row.1)
                        .font(MGWebFont.sans(12, .caption))
                        .foregroundStyle(MGWeb.textBody)
                        .lineSpacing(3)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 7)
                .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                .accessibilityElement(children: .combine)
            }
        }
    }
}

/// A bordered card with a small mono heading in a tone and a paragraph (the web's
/// `OPTIONS AVAILABLE`, `EXCLUDED FROM FUSION`, `MARKING FILTER ACTIVE`, `OUTCOME`).
struct HeimdallNoteCard<Footer: View>: View {
    let heading: String
    let headingColour: Color
    let border: Color
    let text: String
    let identifier: String?
    let footer: Footer

    init(heading: String, headingColour: Color, border: Color, text: String, identifier: String? = nil,
         @ViewBuilder footer: () -> Footer) {
        self.heading = heading
        self.headingColour = headingColour
        self.border = border
        self.text = text
        self.identifier = identifier
        self.footer = footer()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(heading)
                .font(MGWebFont.mono(10))
                .mgTracking(0.12, size: MGWebFont.monoSize(10))
                .foregroundStyle(headingColour)
            Text(text)
                .font(MGWebFont.sans(12, .caption))
                .foregroundStyle(MGWeb.textBody)
                .lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
            footer
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(border))
        .accessibilityElement(children: .contain)
        .modifier(OptionalIdentifier(identifier))
    }
}

extension HeimdallNoteCard where Footer == EmptyView {
    init(heading: String, headingColour: Color, border: Color, text: String, identifier: String? = nil) {
        self.init(heading: heading, headingColour: headingColour, border: border, text: text, identifier: identifier) {
            EmptyView()
        }
    }
}

/// A row's icon and text in the accent (or a tone), as the web's first cells.
struct HeimdallIconCell: View {
    let symbol: String
    let text: String
    var colour: Color = MGWebAccent.heimdall.text
    var mono = false

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: symbol).font(MGWebFont.sans(13)).foregroundStyle(colour).accessibilityHidden(true)
            Text(text)
                .font(mono ? MGWebFont.mono(12, .caption) : MGWebFont.sans(14))
                .foregroundStyle(MGWeb.grey100)
                .lineLimit(1)
        }
    }
}

/// An id cell in the accent's text colour (mono), or the warn colour for an anomaly.
struct HeimdallIdCell: View {
    let text: String
    var warn = false

    var body: some View {
        Text(text)
            .font(MGWebFont.mono(12, .caption))
            .foregroundStyle(warn ? MGWeb.warnText : MGWebAccent.heimdall.text)
            .lineLimit(1)
    }
}

/// The Entities table's confidence (`Conf`): a 60 pt bar, warn below 70 %, secure from
/// it, and the figure in mono.
struct HeimdallConfBar: View {
    let value: Double

    var body: some View {
        let v = min(max(value, 0), 100)
        HStack(spacing: 8) {
            ZStack(alignment: .leading) {
                Capsule().fill(MGWeb.surfaceInset)
                Capsule().fill(v < 70 ? MGWeb.warn : MGWeb.secure).frame(width: 60 * v / 100)
            }
            .frame(width: 60, height: 5)
            .accessibilityHidden(true)
            Text("\(Int(v.rounded()))%").font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("\(Int(v.rounded()))%"))
    }
}
