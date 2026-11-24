import MidgardCore
import SwiftUI

/// One of Bifrost's six screens, matched to the web console's
/// (`products/bifrost/ui/src/screens/`), over the release's sample data: Release Console,
/// Artefacts, Channels, Offline bundles, Fleet & drift and Rollback centre. On the Mac
/// (`compact` false) the main column and the inspector rail side by side inside the
/// console frame; on the iPhone the main column under the classification banner and
/// header (with the screen's toolbar item), the inspector in a sheet. Bifrost is not
/// served by a gateway: connected to one, the screens keep the sample data and say so.
/// [FR-C-1] [FR-B-2] [DD-7]
public struct BifrostScreen: View {
    @ObservedObject var app: AppModel
    let persona: String
    let screen: String
    let compact: Bool
    let toolbar: BifrostToolbar?
    /// The screen's own toolbar state, when the app passes none (the iPhone).
    @StateObject private var ownToolbar = BifrostToolbar()

    /// `toolbar` carries what the screen's toolbar item shows to the Mac's console
    /// header (`BifrostToolbarItem`); the iPhone draws the item in the page.
    public init(app: AppModel, persona: String, screen: String, compact: Bool, toolbar: BifrostToolbar? = nil) {
        self.app = app
        self.persona = persona
        self.screen = screen
        self.compact = compact
        self.toolbar = toolbar
    }

    public var body: some View {
        let ctx = BifrostScreenContext(app: app, persona: persona, compact: compact, bar: toolbar ?? ownToolbar)
        Group {
            switch screen {
            case "Artefacts": BifrostArtefactsScreen(ctx: ctx)
            case "Channels": BifrostChannelsScreen(ctx: ctx)
            case "Bundles": BifrostBundlesScreen(ctx: ctx)
            case "Fleet": BifrostFleetScreen(ctx: ctx)
            case "Rollback": BifrostRollbackScreen(ctx: ctx)
            default: BifrostReleaseScreen(ctx: ctx)
            }
        }
        .id("\(screen)|\(persona)")
        .safeAreaInset(edge: .top, spacing: 0) {
            if let live = app.userData.live {
                WebNotice("NOT SERVED BY \(live.host.uppercased()) · BIFROST SHOWS THE SAMPLE DATA, NOT THIS GATEWAY'S.",
                          tone: .warn, identifier: "mg.bifrost.notserved")
                    .padding(8)
                    .background(MGWeb.bgPage)
            }
        }
    }
}

/// What every Bifrost screen is given.
struct BifrostScreenContext {
    let app: AppModel
    let persona: String
    let compact: Bool
    let bar: BifrostToolbar
    let accent = MGWebAccent.bifrost

    /// `mg.content.bifrost.<slug>`, the screen's main column.
    func content(_ screen: String) -> String { "mg.content.bifrost.\(ConsoleCatalog.slug(screen))" }
    /// `mg.detail.<slug>`, the screen's inspector.
    func detail(_ screen: String) -> String { "mg.detail.\(ConsoleCatalog.slug(screen))" }
    /// The screen's inspector width on the Mac.
    func asideWidth(_ screen: String) -> CGFloat { BifrostChrome.item(for: screen)?.asideWidth ?? 340 }

    /// On the iPhone the console's banner and header open the page, with the screen's
    /// toolbar item at the header's end (the web's narrow layout), in the scroll.
    func header(_ screen: String) -> AnyView? {
        guard compact else { return nil }
        return AnyView(VStack(spacing: 0) {
            WebClassificationBanner(level: BifrostChrome.banner.level, text: BifrostChrome.banner.text, compact: true)
            WebConsoleHeader(product: "Bifrost", accent: BifrostChrome.accent,
                             context: BifrostChrome.item(for: screen)?.context ?? "", seals: BifrostChrome.seals,
                             compact: true) {
                BifrostToolbarItem(screen: screen, toolbar: bar, compact: true)
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

/// What a screen's toolbar item shows (the web's `toolbar` prop): the Release Console
/// publishes its release, gate and Promote; the Rollback centre its Run drill. The Mac's
/// console header draws it (`BifrostToolbarItem`).
@MainActor
public final class BifrostToolbar: ObservableObject {
    /// Release Console: the release's version and channel, the gate, and Promote.
    @Published public var version = "2.5.0"
    @Published public var channel = "stable"
    @Published public var gate = "watch"
    @Published public var promoting = false
    @Published public var promoteDisabled: String?
    var promote: (() -> Void)?
    /// Rollback centre: whether the drill is playing, and Run drill.
    @Published public var drilling = false
    var runDrill: (() -> Void)?

    public init() {}
}

/// A screen's toolbar item (the web's `toolbar` prop). The controls the web leaves
/// without a handler (the channel select) are disabled with the reason; the search box,
/// colour-mode toggle, notifications and account icons of `ConsoleToolbar` are left
/// out, as on the other consoles.
public struct BifrostToolbarItem: View {
    let screen: String
    @ObservedObject var toolbar: BifrostToolbar
    /// On the iPhone the channel select (disabled: the web's has no handler) gives way,
    /// so Promote to Ring 2 is not cut on the header's line.
    let compact: Bool
    @Environment(\.dynamicTypeSize) private var typeSize

    public init(screen: String, toolbar: BifrostToolbar, compact: Bool = false) {
        self.screen = screen
        self.toolbar = toolbar
        self.compact = compact
    }

    public var body: some View {
        switch screen {
        case "Release Console":
            HStack(spacing: 8) {
                WebBadge("v\(toolbar.version)", tone: .accent, accent: .bifrost)
                if !compact && !typeSize.isAccessibilitySize {
                    WebButton(toolbar.channel, trailingSymbol: "chevron.down", variant: .secondary, size: .sm,
                              disabledReason: "Not available in this app: a release's channel is set in the enclosure",
                              accent: .bifrost, identifier: "mg.bifrost.release.channel")
                }
                bifrostGatePill(toolbar.gate)
                    .accessibilityIdentifier("mg.bifrost.release.gate")
                WebButton(toolbar.promoting ? "Promoting…" : "Promote to Ring 2", symbol: WebIcon.symbol("rocket"),
                          size: .sm, disabledReason: toolbar.promoteDisabled, accent: .bifrost,
                          identifier: "mg.bifrost.release.promote") {
                    toolbar.promote?()
                }
            }
        case "Rollback":
            WebButton(toolbar.drilling ? "Drilling…" : "Run drill", symbol: WebIcon.symbol("play"), variant: .secondary,
                      size: .sm, disabledReason: toolbar.drilling ? "The drill is running" : nil, accent: .bifrost,
                      identifier: "mg.bifrost.rollback.drill") {
                toolbar.runDrill?()
            }
        default:
            EmptyView()
        }
    }
}

/// A row's icon and text, the icon in the accent, as the web's first cells.
struct BifrostIconCell: View {
    let symbol: String
    let text: String
    var mono = false

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: symbol).font(MGWebFont.sans(13)).foregroundStyle(MGWebAccent.bifrost.text)
                .accessibilityHidden(true)
            Text(text)
                .font(mono ? MGWebFont.mono(12, .caption) : MGWebFont.sans(14))
                .foregroundStyle(MGWeb.grey100)
                .lineLimit(1)
        }
    }
}

/// An inspector's small label and its rows: the key faint and upper case in a fixed
/// column, the value brighter, a divider under each (the web's PROMOTION POLICY,
/// ON-HOST AGENT, CONTENTS).
struct BifrostRules: View {
    let title: String
    let rows: [(String, String)]
    var keyWidth: CGFloat = 90
    /// CONTENTS writes its values in sans, the others in mono.
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
/// BLOCKED · NOTHING WAS PROMOTED, CONFIG DRIFT).
struct BifrostNoteCard<Extra: View>: View {
    let heading: String
    let colour: Color
    let border: Color
    let text: String?
    var identifier: String? = nil
    @ViewBuilder var extra: Extra

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(heading)
                .font(MGWebFont.mono(10))
                .mgTracking(0.12, size: MGWebFont.monoSize(10))
                .foregroundStyle(colour)
            if let text {
                Text(text)
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.textBody)
                    .lineSpacing(3)
                    .fixedSize(horizontal: false, vertical: true)
            }
            extra
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(border))
        .accessibilityElement(children: .contain)
        .modifier(OptionalIdentifier(identifier))
    }
}

/// A line in the inspector naming an audit id (the web's LAST STEP · AUDIT and EXECUTED ·
/// AUDIT), the id in the accent.
struct BifrostAuditLine: View {
    let label: String
    let auditId: String
    let identifier: String

    var body: some View {
        (Text("\(label) ").foregroundStyle(MGWeb.textFaint) + Text(auditId).foregroundStyle(MGWebAccent.bifrost.text))
            .font(MGWebFont.mono(11))
            .mgTracking(0.14, size: MGWebFont.monoSize(11))
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text("\(label) \(auditId)"))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier(identifier)
    }
}

/// A refusal or a queued note the core gave an action.
struct BifrostMessage: View {
    let text: String
    let identifier: String

    var body: some View {
        WebNotice(text, tone: .warn, identifier: identifier)
    }
}

/// The empty inspector, when nothing can be selected.
struct BifrostNothingSelected: View {
    let withheld: Int
    let noun: String

    var body: some View {
        Text(withheld > 0 ? "Every \(noun) is above your clearance." : "No \(noun) is selected.")
            .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
    }
}
