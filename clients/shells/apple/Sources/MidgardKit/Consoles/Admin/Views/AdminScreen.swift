import MidgardCore
import SwiftUI

/// One of Admin's eight screens, matched to the web console's
/// (`products/admin/ui/src/screens/`), over the release's sample data: on the Mac
/// (`compact` false) the main column and the inspector rail side by side inside the
/// console frame; on the iPhone the main column under the classification banner and
/// header (with the screen's toolbar item), the inspector in a sheet. `open` goes to
/// another screen; `link` carries the header's search to the Ledger. [FR-C-1] [FR-AD-1]
/// [DD-7]
public struct AdminScreen: View {
    @ObservedObject var app: AppModel
    let persona: String
    let screen: String
    let compact: Bool
    let link: MidgardLink?
    let toolbar: AdminToolbar?
    /// The screen's own toolbar state, when the app passes none (the iPhone).
    @StateObject private var ownToolbar = AdminToolbar()
    let open: (String, MidgardLink?) -> Void

    /// `toolbar` carries what the screen's toolbar item shows to the Mac's console
    /// header (`AdminToolbarItem`); the iPhone draws the item in the page.
    public init(app: AppModel, persona: String, screen: String, compact: Bool, link: MidgardLink? = nil,
                toolbar: AdminToolbar? = nil, open: @escaping (String, MidgardLink?) -> Void) {
        self.app = app
        self.persona = persona
        self.screen = screen
        self.compact = compact
        self.link = link
        self.toolbar = toolbar
        self.open = open
    }

    public var body: some View {
        let ctx = AdminScreenContext(app: app, persona: persona, compact: compact, bar: toolbar ?? ownToolbar, open: open)
        Group {
            switch screen {
            case "Overview": AdminOverviewScreen(ctx: ctx)
            case "Approvals": AdminApprovalsScreen(ctx: ctx)
            case "IAM": AdminIamScreen(ctx: ctx)
            case "Config": AdminConfigScreen(ctx: ctx)
            case "Ledger": AdminLedgerScreen(ctx: ctx, link: link)
            case "Registry": AdminRegistryScreen(ctx: ctx)
            case "Break-Glass": AdminBreakGlassScreen(ctx: ctx)
            default: AdminCopilotScreen(ctx: ctx)
            }
        }
        // A new link (or persona) is a new page, as a new URL is on the web.
        .id("\(screen)|\(persona)|\(String(describing: link))\(app.userData.live?.host ?? "")")
        // Connected to a gateway: it serves none of Admin's read models, so the screen
        // is empty and says so, rather than show the sample data under a LIVE chip.
        .safeAreaInset(edge: .top, spacing: 0) {
            if let live = app.userData.live {
                WebNotice("NOT SERVED BY \(live.host.uppercased()) · ADMIN'S SCREENS HAVE NO LIVE DATA FROM THIS GATEWAY. DISCONNECT IN YOUR DATA TO SEE THE SAMPLE DATA.",
                          tone: .warn, identifier: "mg.admin.notserved")
                    .padding(8)
                    .background(MGWeb.bgPage)
            }
        }
    }
}

/// What every Admin screen is given.
struct AdminScreenContext {
    let app: AppModel
    let persona: String
    let compact: Bool
    let bar: AdminToolbar
    let open: (String, MidgardLink?) -> Void

    /// `mg.content.admin.<slug>`, the screen's main column.
    func content(_ screen: String) -> String { "mg.content.admin.\(ConsoleCatalog.slug(screen))" }
    /// `mg.detail.<slug>`, the screen's inspector.
    func detail(_ screen: String) -> String { "mg.detail.\(ConsoleCatalog.slug(screen))" }
    /// The screen's inspector width on the Mac.
    func asideWidth(_ screen: String) -> CGFloat { AdminChrome.item(for: screen)?.asideWidth ?? 330 }

    /// On the iPhone the console's banner and header open the page, with the screen's
    /// toolbar item at the header's end (the web's narrow layout), in the scroll.
    @ViewBuilder func header<Toolbar: View>(_ screen: String, @ViewBuilder toolbar: () -> Toolbar) -> some View {
        if compact {
            VStack(spacing: 0) {
                WebClassificationBanner(level: AdminChrome.banner.level, text: AdminChrome.banner.text, compact: true)
                WebConsoleHeader(product: "Admin", accent: AdminChrome.accent,
                                 context: AdminChrome.item(for: screen)?.context ?? "", seals: AdminChrome.seals,
                                 compact: true, tile: AdminChrome.tile) { toolbar() }
            }
        }
    }

    /// The iPhone's banner and header, with the screen's toolbar item.
    func header(_ screen: String) -> AnyView {
        AnyView(header(screen) { AdminToolbarItem(screen: screen, toolbar: bar) })
    }

    /// The selection rule every list shares: select the row; on the iPhone also open
    /// the inspector.
    func select(_ index: Int, _ set: (Int) -> Void, _ inspecting: Binding<Bool>) {
        set(index)
        if compact { inspecting.wrappedValue = true }
    }
}

/// What a screen's toolbar item shows (the web's `toolbar` prop): the screens publish
/// it, and the Mac's console header draws it (`AdminToolbarItem`).
@MainActor
public final class AdminToolbar: ObservableObject {
    /// Approvals' "N AWAITING".
    @Published public var awaiting = 0
    /// Break-glass's phase: idle, active or review.
    @Published public var breakGlassPhase = "review"
    /// Config's Rollback…: why it cannot be pressed now, if it cannot.
    @Published public var rollbackDisabled: String?
    /// Config's Rollback… of the selected change.
    var rollback: (() -> Void)?

    public init() {}
}

/// A screen's toolbar item: a badge or a button, as each web screen puts in its header.
public struct AdminToolbarItem: View {
    let screen: String
    @ObservedObject var toolbar: AdminToolbar

    public init(screen: String, toolbar: AdminToolbar) {
        self.screen = screen
        self.toolbar = toolbar
    }

    public var body: some View {
        switch screen {
        case "Approvals":
            WebBadge("\(toolbar.awaiting) AWAITING", tone: .warn).accessibilityIdentifier("mg.admin.toolbar.awaiting")
        case "IAM":
            WebButton("New binding", symbol: "plus", size: .sm,
                      disabledReason: "Not available yet: bindings are made with mg-admin, under dual control")
        case "Config":
            WebButton("Rollback…", symbol: WebIcon.symbol("undo-2"), variant: .secondary, size: .sm,
                      disabledReason: toolbar.rollbackDisabled, identifier: "mg.admin.config.rollback") {
                toolbar.rollback?()
            }
        case "Ledger":
            WebButton("Export for auditors", symbol: WebIcon.symbol("download"), variant: .secondary, size: .sm,
                      disabledReason: "Not available yet: the ledger is exported by the governance service")
        case "Registry":
            WebBadge("1 CANDIDATE IN EVALS", tone: .warn)
        case "Break-Glass":
            let phase = toolbar.breakGlassPhase
            WebBadge(phase == "active" ? "1 ACTIVE" : phase == "review" ? "1 REVIEW DUE" : "SEALED",
                     tone: phase == "active" ? .danger : phase == "review" ? .warn : .neutral)
                .accessibilityIdentifier("mg.admin.toolbar.breakglass")
        case "Copilot":
            WebBadge("CANNOT EXECUTE", tone: .info)
        default:
            EmptyView()
        }
    }
}

/// The status a subsystem, role or binding carries (`AdminStatus`) as a pill.
func adminPill(_ status: String, _ label: String) -> WebStatusPill {
    switch status {
    case "secure": return WebStatusPill(.secure, label)
    case "online": return WebStatusPill(.online, label)
    case "warn": return WebStatusPill(.warn, label)
    case "danger": return WebStatusPill(.danger, label)
    default: return WebStatusPill(.healthy, label)
    }
}

/// A numbered step of an inspector's list (the guarded path, the break-glass
/// lifecycle): the numeral in Oxanium black, faint, then the step.
struct AdminStep<Content: View>: View {
    let number: Int
    @ViewBuilder let content: Content

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(String(format: "%02d", number))
                .font(MGWebFont.display(13, .footnote).weight(.black))
                .foregroundStyle(MGWeb.textFaint)
                .frame(width: 18, alignment: .leading)
                .accessibilityHidden(true)
            content
            Spacer(minLength: 0)
        }
        .padding(.vertical, 7)
        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
        .accessibilityElement(children: .combine)
    }
}

/// The header's search box (`ConsoleToolbar`): Return opens the Audit ledger searched
/// for the text.
public struct AdminHeaderSearch: View {
    @State private var text = ""
    let onSearch: (String) -> Void

    public init(onSearch: @escaping (String) -> Void) {
        self.onSearch = onSearch
    }

    public var body: some View {
        WebTextField(nil, placeholder: "Search admin actions, roles, ledger…", text: $text, symbol: "magnifyingglass",
                     identifier: "mg.admin.header.search", width: 300) {
            let q = text.trimmingCharacters(in: .whitespaces)
            if !q.isEmpty { onSearch(q) }
        }
        .help("Press Return to search the audit ledger for this text")
    }
}
