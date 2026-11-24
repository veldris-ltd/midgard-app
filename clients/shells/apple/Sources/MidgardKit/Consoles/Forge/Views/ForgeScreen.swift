import MidgardCore
import SwiftUI

/// One of Forge's seven screens, matched to the web console's
/// (`products/forge/ui/src/screens/`), over the release's sample data: Pipeline Builder,
/// Connectors, Datasets, Data Health, Schedules, Transform IDE and Object Projection. On
/// the Mac (`compact` false) the main column and the inspector rail side by side inside
/// the console frame; on the iPhone the main column under the classification banner
/// and header (with the screen's toolbar item), the inspector in a sheet. `open` goes to
/// another Forge screen ("Open in builder", "Open projection mapping"). Forge is not
/// served by a gateway: connected to one, the screens keep the sample data and say so.
/// [FR-C-1] [FR-F-1] [DD-7]
public struct ForgeScreen: View {
    @ObservedObject var app: AppModel
    let persona: String
    let screen: String
    let compact: Bool
    let toolbar: ForgeToolbar?
    /// The screen's own toolbar state, when the app passes none (the iPhone).
    @StateObject private var ownToolbar = ForgeToolbar()
    let open: (String) -> Void

    /// `toolbar` carries what the screen's toolbar item shows to the Mac's console
    /// header (`ForgeToolbarItem`); the iPhone draws the item in the page.
    public init(app: AppModel, persona: String, screen: String, compact: Bool, toolbar: ForgeToolbar? = nil,
                open: @escaping (String) -> Void) {
        self.app = app
        self.persona = persona
        self.screen = screen
        self.compact = compact
        self.toolbar = toolbar
        self.open = open
    }

    public var body: some View {
        let ctx = ForgeScreenContext(app: app, persona: persona, compact: compact, bar: toolbar ?? ownToolbar, open: open)
        Group {
            switch screen {
            case "Pipeline Builder": ForgePipelineScreen(ctx: ctx)
            case "Connectors": ForgeConnectorsScreen(ctx: ctx)
            case "Datasets": ForgeDatasetsScreen(ctx: ctx)
            case "Data Health": ForgeHealthScreen(ctx: ctx)
            case "Schedules": ForgeSchedulesScreen(ctx: ctx)
            case "Transform IDE": ForgeTransformScreen(ctx: ctx)
            default: ForgeProjectionScreen(ctx: ctx)
            }
        }
        .id("\(screen)|\(persona)")
        .safeAreaInset(edge: .top, spacing: 0) {
            if let live = app.userData.live {
                WebNotice("NOT SERVED BY \(live.host.uppercased()) · FORGE SHOWS THE SAMPLE DATA, NOT THIS GATEWAY'S.",
                          tone: .warn, identifier: "mg.forge.notserved")
                    .padding(8)
                    .background(MGWeb.bgPage)
            }
        }
    }
}

/// What every Forge screen is given.
struct ForgeScreenContext {
    let app: AppModel
    let persona: String
    let compact: Bool
    let bar: ForgeToolbar
    let open: (String) -> Void
    let accent = MGWebAccent.forge

    /// `mg.content.forge.<slug>`, the screen's main column.
    func content(_ screen: String) -> String { "mg.content.forge.\(ConsoleCatalog.slug(screen))" }
    /// `mg.detail.<slug>`, the screen's inspector.
    func detail(_ screen: String) -> String { "mg.detail.\(ConsoleCatalog.slug(screen))" }
    /// The screen's inspector width on the Mac.
    func asideWidth(_ screen: String) -> CGFloat { ForgeChrome.item(for: screen)?.asideWidth ?? 330 }

    /// On the iPhone the console's banner and header open the page, with the screen's
    /// toolbar item at the header's end (the web's narrow layout), in the scroll.
    func header(_ screen: String) -> AnyView? {
        guard compact else { return nil }
        return AnyView(VStack(spacing: 0) {
            WebClassificationBanner(level: ForgeChrome.banner.level, text: ForgeChrome.banner.text, compact: true)
            WebConsoleHeader(product: "Forge", accent: ForgeChrome.accent,
                             context: ForgeChrome.item(for: screen)?.context ?? "", seals: ForgeChrome.seals,
                             compact: true) {
                ForgeToolbarItem(screen: screen, toolbar: bar)
            }
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
/// it, and the Mac's console header draws it (`ForgeToolbarItem`).
@MainActor
public final class ForgeToolbar: ObservableObject {
    /// Pipeline Builder's context line (`PIPELINES / <context>`).
    @Published public var context: String?
    /// The branch badge (Pipelines: the pipeline's; Transform IDE: the transform's).
    @Published public var branch = "MAIN"
    /// Run build: whether a build runs, and why it cannot be pressed now.
    @Published public var building = false
    @Published public var runDisabled: String?
    /// Transform IDE: the next version, and whether a preview or commit is in flight.
    @Published public var nextVersion = ""
    @Published public var busy = false
    var run: (() -> Void)?
    var preview: (() -> Void)?

    public init() {}
}

/// A screen's toolbar item (the web's `toolbar` prop): Pipelines' and Transform IDE's
/// branch badge and buttons; the other screens' search box, mode toggle and icons (the
/// shared `ConsoleToolbar`) have no handler on the web and are left out, as on
/// Heimdall's. The buttons the web leaves without a handler are disabled with the
/// reason.
public struct ForgeToolbarItem: View {
    let screen: String
    @ObservedObject var toolbar: ForgeToolbar
    @Environment(\.dynamicTypeSize) private var typeSize

    public init(screen: String, toolbar: ForgeToolbar) {
        self.screen = screen
        self.toolbar = toolbar
    }

    /// At accessibility text sizes the iPhone's header has the room of one line: the
    /// disabled Branch button gives way.
    private var large: Bool { typeSize.isAccessibilitySize }

    public var body: some View {
        switch screen {
        case "Pipeline Builder":
            HStack(spacing: 8) {
                if !large {
                    WebBadge("BRANCH · \(toolbar.branch.uppercased())", tone: .accent, accent: .forge)
                        .accessibilityIdentifier("mg.forge.toolbar.branch")
                    WebButton("Branch", symbol: WebIcon.symbol("git-branch"), variant: .secondary, size: .sm,
                              disabledReason: "Not available in this app: branches are cut in the enclosure",
                              identifier: "mg.forge.pipeline.branch")
                }
                WebButton(toolbar.building ? "Building…" : "Run build", symbol: WebIcon.symbol("play"), size: .sm,
                          disabledReason: toolbar.runDisabled, accent: .forge, identifier: "mg.forge.pipeline.run") {
                    toolbar.run?()
                }
            }
        case "Transform IDE":
            HStack(spacing: 8) {
                if !large {
                    WebBadge("BRANCH · \(toolbar.branch.uppercased())", tone: .accent, accent: .forge)
                        .accessibilityIdentifier("mg.forge.toolbar.branch")
                }
                WebButton("Preview run", symbol: WebIcon.symbol("sparkles"), variant: .secondary, size: .sm,
                          disabledReason: toolbar.busy ? "A preview or commit is in flight" : nil, accent: .forge,
                          identifier: "mg.forge.ide.preview") { toolbar.preview?() }
                WebButton("Commit \(toolbar.nextVersion)", symbol: WebIcon.symbol("git-commit"), size: .sm,
                          disabledReason: toolbar.busy ? "A preview or commit is in flight" : nil, accent: .forge,
                          identifier: "mg.forge.ide.commit") { toolbar.preview?() }
            }
        default:
            EmptyView()
        }
    }
}

/// A row's icon and text in the accent, as the web's first cells.
struct ForgeIconCell: View {
    let symbol: String
    let text: String
    var mono = false

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: symbol).font(MGWebFont.sans(13)).foregroundStyle(MGWebAccent.forge.text)
                .accessibilityHidden(true)
            Text(text)
                .font(mono ? MGWebFont.mono(12, .caption) : MGWebFont.sans(14))
                .foregroundStyle(MGWeb.grey100)
                .lineLimit(1)
        }
    }
}

/// An inspector's small label and its key-value rows in mono: the key faint and upper
/// case in a fixed column, the value in the brighter grey, a divider under each (the
/// web's TRUST BOUNDARY, TRIGGER RULES, CONTRACT CHECK).
struct ForgeRules: View {
    let title: String
    let rows: [(String, String)]
    var keyWidth: CGFloat = 80
    var monoValues = true

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            WebLabel(title).padding(.bottom, 8)
            ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(row.0.uppercased())
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(MGWeb.textFaint)
                        .frame(width: keyWidth, alignment: .leading)
                    Text(row.1)
                        .font(monoValues ? MGWebFont.mono(11) : MGWebFont.sans(12, .caption))
                        .foregroundStyle(monoValues ? MGWeb.grey100 : MGWeb.textBody)
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
/// ACTION REQUIRED, MIMIR DIAGNOSIS, RETRY 2/3), and what follows it.
struct ForgeNoteCard<Footer: View>: View {
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

extension ForgeNoteCard where Footer == EmptyView {
    init(heading: String, headingColour: Color, border: Color, text: String, identifier: String? = nil) {
        self.init(heading: heading, headingColour: headingColour, border: border, text: text, identifier: identifier) {
            EmptyView()
        }
    }
}

/// The static filter tags the web draws without a handler (Connectors' and Schedules'):
/// shown as the web shows them, the first active, none acting.
struct ForgeStaticTags: View {
    let tags: [String]

    var body: some View {
        FlowRow(spacing: 10) {
            ForEach(Array(tags.enumerated()), id: \.offset) { i, tag in
                WebTag(tag, active: i == 0, accent: .forge)
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Filters: \(tags.joined(separator: ", ")); shown as on the web, not applied"))
        .accessibilityAddTraits(.isStaticText)
    }
}
