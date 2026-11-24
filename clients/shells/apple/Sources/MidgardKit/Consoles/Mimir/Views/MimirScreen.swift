import MidgardCore
import SwiftUI

/// One of Mimir's seven screens, matched to the web console's
/// (`products/mimir/ui/src/screens/`), over the release's sample data: Agent Studio,
/// Logic Builder, Session Traces, Evals, Guardrails, Feedback and Model Plane. On the Mac
/// (`compact` false) the main column and the inspector rail side by side inside the
/// console frame; on the iPhone the main column under the classification banner and
/// header (with the screen's toolbar item), the inspector in a sheet. Mimir is not
/// served by a gateway: connected to one, the screens keep the sample data and say so.
/// [FR-C-1] [FR-MI-2] [DD-7]
public struct MimirScreen: View {
    @ObservedObject var app: AppModel
    let persona: String
    let screen: String
    let compact: Bool
    let toolbar: MimirToolbar?
    /// The screen's own toolbar state, when the app passes none (the iPhone).
    @StateObject private var ownToolbar = MimirToolbar()

    /// `toolbar` carries what the screen's toolbar item shows to the Mac's console
    /// header (`MimirToolbarItem`); the iPhone draws the item in the page.
    public init(app: AppModel, persona: String, screen: String, compact: Bool, toolbar: MimirToolbar? = nil) {
        self.app = app
        self.persona = persona
        self.screen = screen
        self.compact = compact
        self.toolbar = toolbar
    }

    public var body: some View {
        let ctx = MimirScreenContext(app: app, persona: persona, compact: compact, bar: toolbar ?? ownToolbar)
        Group {
            switch screen {
            case "Agent Studio": MimirStudioScreen(ctx: ctx)
            case "Logic Builder": MimirLogicScreen(ctx: ctx)
            case "Session Traces": MimirSessionsScreen(ctx: ctx)
            case "Evals": MimirEvalsScreen(ctx: ctx)
            case "Guardrails": MimirGuardrailsScreen(ctx: ctx)
            case "Feedback": MimirFeedbackScreen(ctx: ctx)
            default: MimirModelPlaneScreen(ctx: ctx)
            }
        }
        .id("\(screen)|\(persona)")
        .safeAreaInset(edge: .top, spacing: 0) {
            if let live = app.userData.live {
                WebNotice("NOT SERVED BY \(live.host.uppercased()) · MIMIR SHOWS THE SAMPLE DATA, NOT THIS GATEWAY'S.",
                          tone: .warn, identifier: "mg.mimir.notserved")
                    .padding(8)
                    .background(MGWeb.bgPage)
            }
        }
    }
}

/// What every Mimir screen is given.
struct MimirScreenContext {
    let app: AppModel
    let persona: String
    let compact: Bool
    let bar: MimirToolbar
    let accent = MGWebAccent.mimir

    /// `mg.content.mimir.<slug>`, the screen's main column.
    func content(_ screen: String) -> String { "mg.content.mimir.\(ConsoleCatalog.slug(screen))" }
    /// `mg.detail.<slug>`, the screen's inspector.
    func detail(_ screen: String) -> String { "mg.detail.\(ConsoleCatalog.slug(screen))" }
    /// The screen's inspector width on the Mac.
    func asideWidth(_ screen: String) -> CGFloat { MimirChrome.item(for: screen)?.asideWidth ?? 330 }

    /// On the iPhone the console's banner and header open the page, with the screen's
    /// toolbar item at the header's end (the web's narrow layout), in the scroll.
    func header(_ screen: String) -> AnyView? {
        guard compact else { return nil }
        return AnyView(VStack(spacing: 0) {
            WebClassificationBanner(level: MimirChrome.banner.level, text: MimirChrome.banner.text, compact: true)
            WebConsoleHeader(product: "Mimir", accent: MimirChrome.accent,
                             context: MimirChrome.item(for: screen)?.context ?? "", seals: MimirChrome.seals,
                             compact: true) {
                MimirToolbarItem(screen: screen, toolbar: bar)
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

/// What a screen's toolbar item shows (the web's `toolbar` prop): Evals publishes its
/// Run suite, and the Mac's console header draws it (`MimirToolbarItem`).
@MainActor
public final class MimirToolbar: ObservableObject {
    /// Evals: whether a run is in flight, and why Run suite cannot be pressed now.
    @Published public var running = false
    @Published public var runDisabled: String?
    var runSuite: (() -> Void)?

    public init() {}
}

/// A screen's toolbar item (the web's `toolbar` prop). The controls the web leaves
/// without a handler (the model and route selects, Settings, Test run, Publish, the
/// exports) are disabled with the reason; at accessibility text sizes on the iPhone
/// they give way, as Forge's do.
public struct MimirToolbarItem: View {
    let screen: String
    @ObservedObject var toolbar: MimirToolbar
    @Environment(\.dynamicTypeSize) private var typeSize

    public init(screen: String, toolbar: MimirToolbar) {
        self.screen = screen
        self.toolbar = toolbar
    }

    private var large: Bool { typeSize.isAccessibilitySize }

    public var body: some View {
        switch screen {
        case "Agent Studio":
            if !large {
                WebButton("Mistral Small 3.2", trailingSymbol: "chevron.down", variant: .secondary, size: .sm,
                          disabledReason: "Not available in this app: the model is routed in the enclosure",
                          accent: .mimir, identifier: "mg.mimir.studio.model")
            }
        case "Logic Builder":
            if !large {
                HStack(spacing: 8) {
                    WebButton("Test run", variant: .secondary, size: .sm,
                              disabledReason: "Not available in this app: a test run is made in the enclosure",
                              accent: .mimir, identifier: "mg.mimir.logic.test")
                    WebButton("Publish v7", symbol: WebIcon.symbol("check"), size: .sm,
                              disabledReason: "Not available in this app: logic is published in the enclosure",
                              accent: .mimir, identifier: "mg.mimir.logic.publish")
                }
            }
        case "Session Traces":
            HStack(spacing: 8) {
                WebBadge("Mistral Small 3.2 · LOCAL", tone: .accent, accent: .mimir)
                if !large {
                    WebButton("Export trace", symbol: WebIcon.symbol("download"), variant: .secondary, size: .sm,
                              disabledReason: "Not available in this app: traces are exported in the enclosure",
                              accent: .mimir, identifier: "mg.mimir.sessions.export")
                }
            }
        case "Evals":
            WebButton(toolbar.running ? "Running…" : "Run suite", symbol: WebIcon.symbol("play"), size: .sm,
                      disabledReason: toolbar.runDisabled, accent: .mimir, identifier: "mg.mimir.evals.run-suite") {
                toolbar.runSuite?()
            }
        case "Guardrails":
            if !large {
                WebButton("Export incident log", symbol: WebIcon.symbol("download"), variant: .secondary, size: .sm,
                          disabledReason: "Not available in this app: the incident log is exported in the enclosure",
                          accent: .mimir, identifier: "mg.mimir.guardrails.export")
            }
        case "Feedback":
            WebBadge("SELF-LEARNING · EVALS-GATED", tone: .accent, accent: .mimir)
        case "Model Plane":
            if !large {
                WebButton("route: capability", trailingSymbol: "chevron.down", variant: .secondary, size: .sm,
                          disabledReason: "Not available in this app: routing is set in the enclosure",
                          accent: .mimir, identifier: "mg.mimir.models.route")
            }
        default:
            EmptyView()
        }
    }
}

/// A row's icon and text, the icon in the accent, as the web's first cells.
struct MimirIconCell: View {
    let symbol: String
    let text: String
    var mono = false
    var colour: Color? = nil

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: symbol).font(MGWebFont.sans(13)).foregroundStyle(MGWebAccent.mimir.text)
                .accessibilityHidden(true)
            Text(text)
                .font(mono ? MGWebFont.mono(12, .caption) : MGWebFont.sans(14))
                .foregroundStyle(colour ?? MGWeb.grey100)
                .lineLimit(1)
        }
    }
}

/// An inspector's small label and its rows in mono: the key faint and upper case in a
/// fixed column, the value brighter, a divider under each (the web's WEIGHTS
/// PROVENANCE, VERSIONS, MEMORY RECORDS).
struct MimirRules: View {
    let title: String
    let rows: [(String, String)]
    var keyWidth: CGFloat = 76
    var upperKeys = true
    var firstHighlighted = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            WebLabel(title).padding(.bottom, 8)
            ForEach(Array(rows.enumerated()), id: \.offset) { i, row in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(upperKeys ? row.0.uppercased() : row.0)
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(firstHighlighted && i == 0 ? MGWebAccent.mimir.text
                                         : upperKeys ? MGWeb.textFaint : MGWeb.grey100)
                        .frame(width: keyWidth, alignment: .leading)
                    Text(row.1)
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(upperKeys ? MGWeb.grey100 : MGWeb.textFaint)
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
/// MANDATORY STEP, POISONING DEFENCE).
struct MimirNoteCard: View {
    let heading: String
    let headingColour: Color
    let border: Color
    let text: String
    var identifier: String? = nil

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
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(border))
        .accessibilityElement(children: .combine)
        .modifier(OptionalIdentifier(identifier))
    }
}

/// A list of check or clock rows (the web's PATH TO POLICY, the Evals run's CASES).
struct MimirCheckRow: View {
    let done: Bool
    let failed: Bool
    let text: String
    var detail: String? = nil

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 9) {
            Image(systemName: failed ? "xmark" : done ? "checkmark" : "clock")
                .font(MGWebFont.sans(12))
                .foregroundStyle(failed ? MGWeb.dangerText : done ? MGWeb.secureText : MGWeb.textFaint)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text(text)
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(done || failed ? MGWeb.textBody : MGWeb.textFaint)
                    .fixedSize(horizontal: false, vertical: true)
                if let detail {
                    Text(detail).font(MGWebFont.mono(9)).foregroundStyle(MGWeb.textFaint)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 5)
        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("\(failed ? "Failed" : done ? "Passed" : "Pending"): \(text)\(detail.map { ", \($0)" } ?? "")"))
    }
}
