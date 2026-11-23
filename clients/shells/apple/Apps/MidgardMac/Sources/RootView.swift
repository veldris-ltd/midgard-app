import AppKit
import UniformTypeIdentifiers
@preconcurrency import MidgardCore
import MidgardKit
import SwiftUI

/// A window's root: the session's screens by phase (MidgardKit's `SessionRootView`:
/// the source banner, enrolment, sign-in, lock, the status chips) and, signed in, the
/// console shell, over a status line that carries the audit's hooks: `mg.motion`
/// (reduced/full) in every phase, with every animation disabled when motion is
/// reduced, and `mg.console.selected` (1…6) once signed in. [FR-C-1] [RC-49] [AC-C-11]
struct RootView: View {
    @ObservedObject var model: AppModel
    @Binding var route: ConsoleRoute
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion

    var body: some View {
        let reduced = MotionPolicy.reduced(system: systemReduceMotion)
        VStack(spacing: 0) {
            SessionRootView(model: model) { persona in
                ConsoleShell(model: model, persona: persona, route: $route)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            Divider()
            StatusLine(reduced: reduced, console: model.isUnlocked ? route.clamped : nil)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Midgard"))
        // The one window set-up, run again once signed in (the split view it names
        // exists only then) and on each route change (the columns are rebuilt).
        .background(WindowSetup(signedIn: model.isUnlocked, route: route.clamped))
        .transaction { transaction in
            if reduced {
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        }
        .focusedSceneValue(\.consoleRoute, $route)
        // Files dropped on the window are imported (Your Data), and Your Data opens to
        // say what came of it.
        .onDrop(of: [.fileURL], isTargeted: nil) { providers in
            Self.urls(from: providers) { urls in
                guard !urls.isEmpty else { return }
                openWindow(id: YourDataWindow.id)
                Task { await model.userData.importFiles(urls) }
            }
            return true
        }
    }

    @Environment(\.openWindow) private var openWindow

    /// The file URLs of a drop, on the main actor once all have loaded.
    static func urls(from providers: [NSItemProvider], _ done: @escaping @MainActor ([URL]) -> Void) {
        let group = DispatchGroup()
        let lock = NSLock()
        var urls: [URL] = []
        for p in providers where p.canLoadObject(ofClass: URL.self) {
            group.enter()
            _ = p.loadObject(ofClass: URL.self) { url, _ in
                if let url, url.isFileURL {
                    lock.lock()
                    urls.append(url)
                    lock.unlock()
                }
                group.leave()
            }
        }
        group.notify(queue: .main) { MainActor.assumeIsolated { done(urls) } }
    }
}

/// The window's status line: which console is shown ("Console 1 of 6",
/// `mg.console.selected` = 1…6) and whether animations run ("Animations off (Reduce
/// Motion)", `mg.motion` = reduced/full). Visible text, so VoiceOver and the contrast
/// audit see what the UI tests read. [RC-49] [AC-C-11]
struct StatusLine: View {
    let reduced: Bool
    let console: ConsoleRoute?

    var body: some View {
        HStack(spacing: MGSpace.s4) {
            if let console {
                Text("Console \(console.selectedValue) of \(ConsoleCatalog.consoles.count): \(ConsoleCatalog.console(console).name)")
                    .accessibilityValue(Text(console.selectedValue))
                    .accessibilityIdentifier("mg.console.selected")
            }
            Spacer(minLength: MGSpace.s2)
            Text(reduced ? "Animations off (Reduce Motion)" : "Animations on")
                .accessibilityValue(Text(reduced ? "reduced" : "full"))
                .accessibilityIdentifier("mg.motion")
        }
        // The heading token on the console's own sunken surface: over the window's
        // background, which macOS tints with the desktop picture, the house sans failed
        // the contrast audit at random once the bundled fonts were registered.
        .font(MGFonts.sans(.footnote).weight(.medium))
        .foregroundStyle(MGStyle.text)
        .padding(.horizontal, MGSpace.s3)
        .padding(.vertical, MGSpace.s1)
        .background(MGWeb.bgSunken)
    }
}

/// Window set-up SwiftUI on macOS 14 has no modifier for: hide the title text (the
/// title itself stays; `.toolbar(removing: .title)` is macOS 15), and name the AppKit
/// containers NavigationSplitView makes, which VoiceOver otherwise announces as bare
/// groups (the screen-reader audit's "Element has no description"): the split view
/// and each column's own group (its hosting view) only, never the groups SwiftUI makes
/// inside a column, which carry the screen's own labels.
struct WindowSetup: NSViewRepresentable {
    /// Whether the console shell (and its split view) is shown: set-up runs again when
    /// it changes.
    let signedIn: Bool
    /// The window's route: set-up runs again when it changes.
    let route: ConsoleRoute

    struct State: Equatable {
        let signedIn: Bool
        let route: ConsoleRoute
    }

    final class Coordinator { var state: State? }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        context.coordinator.state = State(signedIn: signedIn, route: route)
        Self.schedule(view)
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {
        let state = State(signedIn: signedIn, route: route)
        guard context.coordinator.state != state else { return }
        context.coordinator.state = state
        Self.schedule(view)
    }

    /// Once the view is in its window, and again once the split view has laid out its
    /// columns.
    private static func schedule(_ view: NSView) {
        DispatchQueue.main.async { setUp(view.window) }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { setUp(view.window) }
    }

    static let columnNames = ["Consoles", "Screen", "Details"]

    static func setUp(_ window: NSWindow?) {
        guard let window, let content = window.contentView else { return }
        window.titleVisibility = .hidden
        name(content, "Midgard window")
        for split in descendants(of: content).compactMap({ $0 as? NSSplitView }) {
            name(split, "Console columns")
            for (index, column) in split.arrangedSubviews.enumerated() {
                // The column's wrapper has no role; the group VoiceOver meets is the
                // column's hosting view, the first group inside it. Nothing deeper:
                // SwiftUI's own groups carry the screen's labels.
                if let group = firstGroup(in: column) {
                    name(group, index < columnNames.count ? columnNames[index] : "Column")
                }
            }
        }
    }

    /// The shallowest group at or under `view` (breadth first).
    private static func firstGroup(in view: NSView) -> NSView? {
        var level = [view]
        while !level.isEmpty {
            if let group = level.first(where: { $0.accessibilityRole() == .group }) { return group }
            level = level.flatMap(\.subviews)
        }
        return nil
    }

    /// Label an unlabelled group; leave everything else as it is.
    private static func name(_ view: NSView, _ label: String) {
        guard view.accessibilityRole() == .group, (view.accessibilityLabel() ?? "").isEmpty else { return }
        view.setAccessibilityLabel(label)
    }

    private static func descendants(of view: NSView) -> [NSView] {
        view.subviews + view.subviews.flatMap { descendants(of: $0) }
    }
}

/// Signed in: Heimdall, Midgard and Admin in their web consoles' own frames
/// (`HeimdallConsole`, `MidgardConsole`, `AdminConsole`, matched to the web suite, owner
/// decision 22 Nov 2026); every other
/// console, until its phase
/// matches it, the persona menu above a three-column split view. The sidebar lists the
/// six consoles (⌘1…⌘6) and, under the selected one, its screens; the content and
/// detail columns show the screen's placeholder until its phase builds it. [FR-C-1]
struct ConsoleShell: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var route: ConsoleRoute

    var body: some View {
        if ConsoleCatalog.console(route).id == "heimdall" {
            HeimdallConsole(model: model, persona: persona, route: $route)
                .navigationTitle(ConsoleCatalog.console(route).name)
        } else if ConsoleCatalog.console(route).id == "midgard" {
            MidgardConsole(model: model, persona: persona, route: $route)
                .navigationTitle(ConsoleCatalog.console(route).name)
        } else if ConsoleCatalog.console(route).id == "admin" {
            AdminConsole(model: model, persona: persona, route: $route)
                .navigationTitle(ConsoleCatalog.console(route).name)
        } else {
            SplitConsoleShell(model: model, persona: persona, route: $route)
        }
    }
}

/// Heimdall as the web console draws it (`ConsoleShell` with `OperatingPictureScreen`):
/// the classification banner, the header (mark, context, seals, the persona menu), the
/// navigation (the six consoles, then Heimdall's PICTURE and DECIDE sections with their
/// counts, and on the Operating Picture its LAYERS switches), the picture and the
/// inspector. Heimdall's other screens keep their placeholder inside the same frame. In
/// the Maps edition the header also carries the picture's view switch, 2D / MAP / 3D
/// (the web's `ViewSwitch`): MAP and 3D come from the edition library
/// (`AppModel.pictureViews`) and replace the picture while chosen, sharing its layers,
/// selection and inspector; a release app has no provider. [FR-C-1] [FR-H-1] [FR-H-4]
/// [FR-H-5] [DD-7]
struct HeimdallConsole: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var route: ConsoleRoute
    @StateObject private var picture: HeimdallPictureModel
    /// The picture's view, kept for the window as the web keeps it for the browser.
    @SceneStorage("mg-heimdall-view") private var pictureView = HeimdallPictureView.reference.id

    init(model: AppModel, persona: PersonaInfo, route: Binding<ConsoleRoute>) {
        self.model = model
        self.persona = persona
        self._route = route
        self._picture = StateObject(wrappedValue: HeimdallPictureModel(app: model))
    }

    private var isPicture: Bool { route.clamped.screen == 0 }
    private var views: HeimdallPictureViewsProvider? { isPicture ? model.pictureViews : nil }

    /// The edition's view chosen in the switch, when it is not the reference layout.
    private var chosenView: (HeimdallPictureViewsProvider, HeimdallPictureView)? {
        guard pictureView != HeimdallPictureView.reference.id, let views,
              let v = views.views.first(where: { $0.id == pictureView }) else { return nil }
        return (views, v)
    }
    private var heimdall: Console { ConsoleCatalog.console(route) }

    var body: some View {
        HeimdallConsoleFrame(nav: { nav }, main: { main }, aside: { aside }, trailing: { trailing })
            .task(id: persona.id) { await picture.load() }
            // The user's feeds are read only while the picture is open.
            .task(id: isPicture) { if isPicture { await picture.followUserData() } }
    }

    /// The navigation: the consoles first, as a row of tiles (numbered from 1; the
    /// persona menu is 0), then Heimdall's sections in the web's order.
    private var sections: (WebSideNavSection, [WebSideNavSection]) {
        var number = 0
        func next() -> Int { number += 1; return number }
        let consoles = ConsoleCatalog.consoles.enumerated().map { index, console in
            WebSideNavItem(id: "console.\(console.slug)", label: "\(console.name) console", symbol: console.symbol, badge: nil,
                           identifier: "mg.\(next()).console.\(console.slug)", selected: console.id == "heimdall",
                           action: { route.select(console: index) })
        }
        let consoleIndex = route.clamped.console
        let screens = HeimdallChrome.sections.map { section in
            WebSideNavSection(title: section.title, items: section.items.map { item in
                let index = heimdall.screens.firstIndex(of: item.screen) ?? 0
                return WebSideNavItem(id: item.id, label: item.label, symbol: item.symbol, badge: item.badge,
                                      identifier: "mg.\(next()).screen.\(ConsoleCatalog.slug(item.screen))",
                                      selected: route.clamped.screen == index,
                                      action: { route = ConsoleRoute(console: consoleIndex, screen: index).clamped })
            })
        }
        return (WebSideNavSection(title: "CONSOLES", items: consoles), screens)
    }

    private var nav: some View {
        let (rail, sections) = self.sections
        return WebSideNav(rail: rail, sections: sections, accent: .heimdall) {
            if isPicture { HeimdallLayersList(model: picture) }
        }
    }

    @ViewBuilder private var main: some View {
        if let pair = chosenView {
            let (provider, v) = pair
            ZStack(alignment: .bottomLeading) {
                provider.makeView(v, picture: picture, context: HeimdallPictureViewContext(offline: model.isOffline, compact: false))
                HeimdallStatsFooter(honesty: HeimdallStatsFooter.notRealPositions)
                    .padding(.horizontal, 12)
                    .padding(.bottom, 10)
            }
        } else if isPicture {
            HeimdallPictureCanvas(model: picture)
        } else {
            ScreenContent(route: route.clamped)
        }
    }

    @ViewBuilder private var aside: some View {
        if isPicture {
            HeimdallInspector(model: picture)
                .accessibilityIdentifier("mg.detail.operating-picture")
        } else {
            ScreenDetail(route: route.clamped)
        }
    }

    @ViewBuilder private var trailing: some View {
        if let views { HeimdallViewSwitch(views: views.views, selection: $pictureView) }
        PersonaMenu(model: model, persona: persona)
    }
}

/// Midgard as the web console draws it (`ConsoleShell` with each of its eleven screens):
/// the classification banner, the header (mark, the screen's context, seals, the search
/// box that opens Data search for its text across every type, the persona menu), the
/// navigation (the six consoles as a row of tiles, then Midgard's FABRIC, GOVERNANCE and
/// PLATFORM sections in the web's order and words, and the `midgard status` footer where
/// the web screen has it), and the screen with its inspector. [FR-C-1] [DD-1] [DD-7]
struct MidgardConsole: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var route: ConsoleRoute
    /// What the screen was opened with ("Explore instances", "Open in Search-around",
    /// the header's search); cleared when the navigation opens a screen.
    @State private var link: MidgardLink?

    private var midgard: Console { ConsoleCatalog.console(route) }
    private var screen: String { ConsoleCatalog.screenName(route) }

    var body: some View {
        let item = MidgardChrome.item(for: screen)
        MidgardConsoleFrame(context: item?.context ?? "") {
            nav(footer: item?.statusFooter ?? false)
        } content: {
            MidgardScreen(app: model, persona: persona.id, screen: screen, compact: false, link: link) { screen, link in
                open(screen, link)
            }
        } trailing: {
            MidgardHeaderSearch { open("Data Search", .text($0)) }
            PersonaMenu(model: model, persona: persona)
        }
    }

    private func open(_ screen: String, _ link: MidgardLink?) {
        self.link = link
        let index = midgard.screens.firstIndex(of: screen) ?? 0
        route = ConsoleRoute(console: route.clamped.console, screen: index).clamped
    }

    /// The consoles first, as a row of tiles (numbered from 1; the persona menu is 0),
    /// then Midgard's sections in the web's order.
    private func nav(footer: Bool) -> some View {
        var number = 0
        func next() -> Int { number += 1; return number }
        let consoles = ConsoleCatalog.consoles.enumerated().map { index, console in
            WebSideNavItem(id: "console.\(console.slug)", label: "\(console.name) console", symbol: console.symbol, badge: nil,
                           identifier: "mg.\(next()).console.\(console.slug)", selected: console.id == "midgard",
                           action: { route.select(console: index) })
        }
        let sections = MidgardChrome.sections.map { section in
            WebSideNavSection(title: section.title, items: section.items.map { item in
                WebSideNavItem(id: item.id, label: item.label, symbol: item.symbol, badge: nil,
                               identifier: "mg.\(next()).screen.\(ConsoleCatalog.slug(item.screen))",
                               selected: item.screen == screen,
                               action: { open(item.screen, nil) })
            })
        }
        return WebSideNav(rail: WebSideNavSection(title: "CONSOLES", items: consoles), sections: sections, accent: .midgard) {
            if footer { MidgardStatusFooter() }
        }
    }
}

/// Admin as the web console draws it (`ConsoleShell product="admin"` with each of its
/// eight screens): the management-zone banner, the header (the grey ADMIN mark, the
/// screen's context, seals, the screen's toolbar item, the search box that opens the
/// audit ledger searched for its text, the persona menu), the navigation (the six
/// consoles as a row of tiles, then Admin's OVERSEE, ADMINISTER, EMERGENCY and ASSIST
/// sections in the web's order, words and badges, and the `admin status` footer on
/// Overview), and the screen with its inspector. [FR-C-1] [FR-AD-1] [DD-7]
struct AdminConsole: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var route: ConsoleRoute
    /// What the screen was opened with (the header's search); cleared when the
    /// navigation opens a screen.
    @State private var link: MidgardLink?
    @StateObject private var toolbar = AdminToolbar()

    private var admin: Console { ConsoleCatalog.console(route) }
    private var screen: String { ConsoleCatalog.screenName(route) }

    var body: some View {
        WebConsoleFrame(banner: AdminChrome.banner, product: "Admin", accent: AdminChrome.accent, tile: AdminChrome.tile,
                        context: AdminChrome.item(for: screen)?.context ?? "", seals: AdminChrome.seals) {
            nav
        } content: {
            AdminScreen(app: model, persona: persona.id, screen: screen, compact: false, link: link, toolbar: toolbar) {
                open($0, $1)
            }
        } trailing: {
            AdminToolbarItem(screen: screen, toolbar: toolbar)
            AdminHeaderSearch { open("Ledger", .text($0)) }
            PersonaMenu(model: model, persona: persona)
        }
    }

    private func open(_ screen: String, _ link: MidgardLink?) {
        self.link = link
        let index = admin.screens.firstIndex(of: screen) ?? 0
        route = ConsoleRoute(console: route.clamped.console, screen: index).clamped
    }

    /// The consoles first, as a row of tiles (numbered from 1; the persona menu is 0),
    /// then Admin's sections in the web's order.
    private var nav: some View {
        var number = 0
        func next() -> Int { number += 1; return number }
        let consoles = ConsoleCatalog.consoles.enumerated().map { index, console in
            WebSideNavItem(id: "console.\(console.slug)", label: "\(console.name) console", symbol: console.symbol, badge: nil,
                           identifier: "mg.\(next()).console.\(console.slug)", selected: console.id == "admin",
                           action: { route.select(console: index) })
        }
        let sections = AdminChrome.sections.map { section in
            WebSideNavSection(title: section.title, items: section.items.map { item in
                WebSideNavItem(id: item.id, label: item.label, symbol: item.symbol, badge: item.badge,
                               identifier: "mg.\(next()).screen.\(ConsoleCatalog.slug(item.screen))",
                               selected: item.screen == screen,
                               action: { open(item.screen, nil) })
            })
        }
        return WebSideNav(rail: WebSideNavSection(title: "CONSOLES", items: consoles), sections: sections,
                          accent: AdminChrome.accent) {
            if screen == "Overview" { AdminStatusFooter() }
        }
    }
}

/// The split-view shell of the consoles not yet matched to the web suite: the persona
/// menu above the sidebar, the screen's content and its detail. [FR-C-1]
struct SplitConsoleShell: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var route: ConsoleRoute

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                PersonaMenu(model: model, persona: persona)
                Spacer()
            }
            .padding(.horizontal, MGSpace.s3)
            .padding(.vertical, MGSpace.s1)
            Divider()
            NavigationSplitView {
                ConsoleSidebar(route: $route)
                    .navigationSplitViewColumnWidth(min: 200, ideal: 230)
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel(Text("Consoles"))
            } content: {
                ScreenContent(route: route.clamped)
                    .navigationSplitViewColumnWidth(min: 280, ideal: 360)
            } detail: {
                ScreenDetail(route: route.clamped)
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel(Text("Details"))
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel(Text("Console"))
        }
        // The window keeps its title (Window menu, Mission Control) but does not draw
        // it (RootView's WindowSetup hides it): the titlebar's grey title text fails the
        // contrast audit, and the sidebar and the screen heading already say where the
        // window is.
        .navigationTitle(ConsoleCatalog.console(route).name)
    }
}

/// The signed-in persona, and what can be done as them: switch persona, go offline or
/// online, sync now, lock. The same actions are in the Session menu with shortcuts.
struct PersonaMenu: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo

    var body: some View {
        let offline = model.isOffline
        Menu {
            Section("Switch persona") {
                ForEach(model.personas, id: \.id) { other in
                    Button(other.display) { Task { await model.switchPersona(other) } }
                        .disabled(other.id == persona.id)
                }
            }
            Divider()
            Button(offline ? "Go online" : "Go offline") { Task { await model.setOffline(!offline) } }
            Button("Sync now") { Task { await model.syncNow() } }
            Divider()
            Button("Lock") { Task { await model.lock() } }
        } label: {
            Label(persona.display, systemImage: "person.crop.circle")
                .font(MGFonts.sans(.body))
                .frame(minHeight: 44)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityLabel(Text("Signed in as \(persona.display)"))
        .accessibilityIdentifier("mg.0.persona-menu")
    }
}

/// One sidebar line: a console or one of the selected console's screens, numbered in
/// reading order for the audit's `mg.<n>.<name>` identifiers.
struct SidebarEntry: Identifiable, Hashable {
    enum Item: Hashable {
        case console(Int)
        case screen(Int, Int)
    }

    let item: Item
    let number: Int
    let title: String

    var id: Item { item }

    var identifier: String {
        switch item {
        case let .console(c): return "mg.\(number).console.\(ConsoleCatalog.consoles[c].slug)"
        case let .screen(c, s): return "mg.\(number).screen.\(ConsoleCatalog.slug(ConsoleCatalog.consoles[c].screens[s]))"
        }
    }

    /// The lines for `route`, numbered from `first` (the persona menu is 0).
    static func entries(for route: ConsoleRoute, first: Int = 1) -> [SidebarEntry] {
        let route = route.clamped
        var list: [SidebarEntry] = []
        for (c, console) in ConsoleCatalog.consoles.enumerated() {
            list.append(SidebarEntry(item: .console(c), number: first + list.count, title: console.name))
            if c == route.console {
                for (s, screen) in console.screens.enumerated() {
                    list.append(SidebarEntry(item: .screen(c, s), number: first + list.count, title: screen))
                }
            }
        }
        return list
    }
}

struct ConsoleSidebar: View {
    @Binding var route: ConsoleRoute

    private var selection: Binding<SidebarEntry.Item?> {
        Binding(get: {
            let r = route.clamped
            return .screen(r.console, r.screen)
        }, set: { item in
            switch item {
            case let .console(c)?: route.select(console: c)
            case let .screen(c, s)?: route = ConsoleRoute(console: c, screen: s).clamped
            case nil: break
            }
        })
    }

    var body: some View {
        List(selection: selection) {
            ForEach(SidebarEntry.entries(for: route)) { entry in
                row(entry)
                    .tag(entry.item)
                    .accessibilityIdentifier(entry.identifier)
            }
        }
        .listStyle(.sidebar)
        // Opaque: over the translucent sidebar material the rows' contrast depends on
        // whatever is behind the window.
        .scrollContentBackground(.hidden)
        .background(Color(nsColor: .windowBackgroundColor))
    }

    @ViewBuilder private func row(_ entry: SidebarEntry) -> some View {
        switch entry.item {
        case let .console(c):
            Label(entry.title, systemImage: ConsoleCatalog.consoles[c].symbol)
                .font(MGFonts.sans(.body).weight(.semibold))
                .accessibilityLabel(Text("\(entry.title) console"))
        case .screen:
            Text(entry.title)
                .font(MGFonts.sans(.body))
                .padding(.leading, MGSpace.s4)
        }
    }
}

/// The content column: the screen's name and what will appear there.
struct ScreenContent: View {
    let route: ConsoleRoute

    var body: some View {
        let name = ConsoleCatalog.screenName(route)
        VStack(alignment: .leading, spacing: MGSpace.s3) {
            Text(name).font(MGFonts.display(.title2)).accessibilityAddTraits(.isHeader)
            Text(ConsoleCatalog.placeholder(for: route)).font(MGFonts.sans(.body))
            Text("Until then it shows nothing of the sample data.")
                .font(MGFonts.sans(.body))
        }
        .padding(MGSpace.s5)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("mg.content.\(ConsoleCatalog.slug(name))")
    }
}

/// The detail column: the inspector a screen's rows will open.
struct ScreenDetail: View {
    let route: ConsoleRoute

    var body: some View {
        let console = ConsoleCatalog.console(route)
        let name = ConsoleCatalog.screenName(route)
        VStack(alignment: .leading, spacing: MGSpace.s3) {
            Text("Details").font(MGFonts.display(.title3)).accessibilityAddTraits(.isHeader)
            Text("\(name) details arrive in Phase \(console.phase) (\(console.name)).").font(MGFonts.sans(.body))
        }
        .padding(MGSpace.s5)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("mg.detail.\(ConsoleCatalog.slug(name))")
    }
}

extension Console {
    /// The console's sidebar symbol on the Mac. (The catalogue is MidgardKit's.)
    var symbol: String {
        switch id {
        case "midgard": return "square.grid.3x3"
        case "admin": return "person.badge.key"
        case "heimdall": return "map"
        case "forge": return "hammer"
        case "mimir": return "brain"
        case "bifrost": return "shippingbox"
        default: return "square"
        }
    }
}
