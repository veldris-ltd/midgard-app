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

/// Signed in: each of the six consoles in its web console's own frame (`MidgardConsole`,
/// `AdminConsole`, `HeimdallConsole`, `ForgeConsole`, `MimirConsole`, `BifrostConsole`),
/// matched to the web suite (owner decision 22 Nov 2026): every screen is built, none is
/// a placeholder. [FR-C-1]
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
        } else if ConsoleCatalog.console(route).id == "forge" {
            ForgeConsole(model: model, persona: persona, route: $route)
                .navigationTitle(ConsoleCatalog.console(route).name)
        } else if ConsoleCatalog.console(route).id == "mimir" {
            MimirConsole(model: model, persona: persona, route: $route)
                .navigationTitle(ConsoleCatalog.console(route).name)
        } else {
            BifrostConsole(model: model, persona: persona, route: $route)
                .navigationTitle(ConsoleCatalog.console(route).name)
        }
    }
}

/// Heimdall as the web console draws it (`ConsoleShell` with `OperatingPictureScreen`):
/// the classification banner, the header (mark, context, seals, the persona menu), the
/// navigation (the six consoles, then Heimdall's PICTURE and DECIDE sections with their
/// counts, and on the Operating Picture its LAYERS switches), the picture and the
/// inspector, with the Alerts list in the header. Heimdall's other five screens
/// (`HeimdallScreen`: Entities, Feeds, Picture Sync, Workflows, Decision Lineage) fill
/// the main area with their own inspector, and their toolbar item sits in the header. In
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
    @StateObject private var toolbar = HeimdallToolbar()
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
    private var screen: String { ConsoleCatalog.screenName(route) }

    private func open(_ screen: String) {
        let index = heimdall.screens.firstIndex(of: screen) ?? 0
        route = ConsoleRoute(console: route.clamped.console, screen: index).clamped
    }

    var body: some View {
        HeimdallConsoleFrame(asideWidth: isPicture ? 360 : nil, context: HeimdallChrome.item(for: screen)?.context ?? "",
                             nav: { nav }, main: { main }, aside: { aside }, trailing: { trailing })
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
            HeimdallScreen(app: model, persona: persona.id, screen: screen, compact: false, toolbar: toolbar) { open($0) }
        }
    }

    @ViewBuilder private var aside: some View {
        if isPicture {
            HeimdallInspector(model: picture)
                .accessibilityIdentifier("mg.detail.operating-picture")
        }
    }

    @ViewBuilder private var trailing: some View {
        if isPicture {
            HeimdallAlertsButton(model: picture)
        } else {
            HeimdallToolbarItem(screen: screen, toolbar: toolbar)
        }
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
        MidgardConsoleFrame(context: item?.context ?? "", seals: MidgardChrome.seals(live: model.userData.live)) {
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

/// Forge as the web console draws it (`ConsoleShell product="forge"` with each of its
/// seven screens): the RESTRICTED · DATA PLANE banner, the header (mark, the screen's
/// context, the seal, the screen's toolbar item, the persona menu), the navigation (the
/// six consoles as a row of tiles, then Forge's DATA OPS and FABRIC sections in the web's
/// order, words and badges), and the screen with its inspector. [FR-C-1] [FR-F-1] [DD-7]
struct ForgeConsole: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var route: ConsoleRoute
    @StateObject private var toolbar = ForgeToolbar()

    private var forge: Console { ConsoleCatalog.console(route) }
    private var screen: String { ConsoleCatalog.screenName(route) }

    var body: some View {
        let item = ForgeChrome.item(for: screen)
        let context = screen == "Pipeline Builder" ? toolbar.context ?? item?.context ?? "" : item?.context ?? ""
        WebConsoleFrame(banner: ForgeChrome.banner, product: "Forge", accent: ForgeChrome.accent,
                        context: context, seals: ForgeChrome.seals) {
            nav
        } content: {
            ForgeScreen(app: model, persona: persona.id, screen: screen, compact: false, toolbar: toolbar) { open($0) }
        } trailing: {
            ForgeToolbarItem(screen: screen, toolbar: toolbar)
            PersonaMenu(model: model, persona: persona)
        }
    }

    private func open(_ screen: String) {
        let index = forge.screens.firstIndex(of: screen) ?? 0
        route = ConsoleRoute(console: route.clamped.console, screen: index).clamped
    }

    /// The consoles first, as a row of tiles (numbered from 1; the persona menu is 0),
    /// then Forge's sections in the web's order.
    private var nav: some View {
        var number = 0
        func next() -> Int { number += 1; return number }
        let consoles = ConsoleCatalog.consoles.enumerated().map { index, console in
            WebSideNavItem(id: "console.\(console.slug)", label: "\(console.name) console", symbol: console.symbol, badge: nil,
                           identifier: "mg.\(next()).console.\(console.slug)", selected: console.id == "forge",
                           action: { route.select(console: index) })
        }
        let sections = ForgeChrome.sections.map { section in
            WebSideNavSection(title: section.title, items: section.items.map { item in
                WebSideNavItem(id: item.id, label: item.label, symbol: item.symbol, badge: item.badge,
                               identifier: "mg.\(next()).screen.\(ConsoleCatalog.slug(item.screen))",
                               selected: item.screen == screen,
                               action: { open(item.screen) })
            })
        }
        return WebSideNav(rail: WebSideNavSection(title: "CONSOLES", items: consoles), sections: sections,
                          accent: ForgeChrome.accent) {
            EmptyView()
        }
    }
}

/// Mimir as the web console draws it (`ConsoleShell product="mimir"` with each of its
/// seven screens): the CONFIDENTIAL banner, the header (mark, the screen's context, the
/// two seals, the screen's toolbar item, the persona menu), the navigation (the six
/// consoles as a row of tiles, then Mimir's BUILD, GOVERN and SERVE sections in the web's
/// order, words and badges), and the screen with its inspector. [FR-C-1] [FR-MI-2] [DD-7]
struct MimirConsole: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var route: ConsoleRoute
    @StateObject private var toolbar = MimirToolbar()

    private var mimir: Console { ConsoleCatalog.console(route) }
    private var screen: String { ConsoleCatalog.screenName(route) }

    var body: some View {
        WebConsoleFrame(banner: MimirChrome.banner, product: "Mimir", accent: MimirChrome.accent,
                        context: MimirChrome.item(for: screen)?.context ?? "", seals: MimirChrome.seals) {
            nav
        } content: {
            MimirScreen(app: model, persona: persona.id, screen: screen, compact: false, toolbar: toolbar)
        } trailing: {
            MimirToolbarItem(screen: screen, toolbar: toolbar)
            PersonaMenu(model: model, persona: persona)
        }
    }

    private func open(_ screen: String) {
        let index = mimir.screens.firstIndex(of: screen) ?? 0
        route = ConsoleRoute(console: route.clamped.console, screen: index).clamped
    }

    /// The consoles first, as a row of tiles (numbered from 1; the persona menu is 0),
    /// then Mimir's sections in the web's order.
    private var nav: some View {
        var number = 0
        func next() -> Int { number += 1; return number }
        let consoles = ConsoleCatalog.consoles.enumerated().map { index, console in
            WebSideNavItem(id: "console.\(console.slug)", label: "\(console.name) console", symbol: console.symbol, badge: nil,
                           identifier: "mg.\(next()).console.\(console.slug)", selected: console.id == "mimir",
                           action: { route.select(console: index) })
        }
        let sections = MimirChrome.sections.map { section in
            WebSideNavSection(title: section.title, items: section.items.map { item in
                WebSideNavItem(id: item.id, label: item.label, symbol: item.symbol, badge: item.badge,
                               identifier: "mg.\(next()).screen.\(ConsoleCatalog.slug(item.screen))",
                               selected: item.screen == screen,
                               action: { open(item.screen) })
            })
        }
        return WebSideNav(rail: WebSideNavSection(title: "CONSOLES", items: consoles), sections: sections,
                          accent: MimirChrome.accent) {
            EmptyView()
        }
    }
}

/// Bifrost as the web console draws it (`ConsoleShell product="bifrost"` with each of its
/// six screens): the INTERNAL · RELEASE CONTROL PLANE banner, the header (mark, the
/// screen's context, the ARTEFACTS ML-DSA seal, the screen's toolbar item, the persona
/// menu), the navigation (the six consoles as a row of tiles, then Bifrost's DELIVERY and
/// FLEET sections in the web's order, words and badges), and the screen with its
/// inspector. [FR-C-1] [FR-B-2] [DD-7]
struct BifrostConsole: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var route: ConsoleRoute
    @StateObject private var toolbar = BifrostToolbar()

    private var bifrost: Console { ConsoleCatalog.console(route) }
    private var screen: String { ConsoleCatalog.screenName(route) }

    var body: some View {
        WebConsoleFrame(banner: BifrostChrome.banner, product: "Bifrost", accent: BifrostChrome.accent,
                        context: BifrostChrome.item(for: screen)?.context ?? "", seals: BifrostChrome.seals) {
            nav
        } content: {
            BifrostScreen(app: model, persona: persona.id, screen: screen, compact: false, toolbar: toolbar)
        } trailing: {
            BifrostToolbarItem(screen: screen, toolbar: toolbar)
            PersonaMenu(model: model, persona: persona)
        }
    }

    private func open(_ screen: String) {
        let index = bifrost.screens.firstIndex(of: screen) ?? 0
        route = ConsoleRoute(console: route.clamped.console, screen: index).clamped
    }

    /// The consoles first, as a row of tiles (numbered from 1; the persona menu is 0),
    /// then Bifrost's sections in the web's order.
    private var nav: some View {
        var number = 0
        func next() -> Int { number += 1; return number }
        let consoles = ConsoleCatalog.consoles.enumerated().map { index, console in
            WebSideNavItem(id: "console.\(console.slug)", label: "\(console.name) console", symbol: console.symbol, badge: nil,
                           identifier: "mg.\(next()).console.\(console.slug)", selected: console.id == "bifrost",
                           action: { route.select(console: index) })
        }
        let sections = BifrostChrome.sections.map { section in
            WebSideNavSection(title: section.title, items: section.items.map { item in
                WebSideNavItem(id: item.id, label: item.label, symbol: item.symbol, badge: item.badge,
                               identifier: "mg.\(next()).screen.\(ConsoleCatalog.slug(item.screen))",
                               selected: item.screen == screen,
                               action: { open(item.screen) })
            })
        }
        return WebSideNav(rail: WebSideNavSection(title: "CONSOLES", items: consoles), sections: sections,
                          accent: BifrostChrome.accent) {
            EmptyView()
        }
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
            if let live = model.userData.live {
                Section("Signed in to \(live.host) as \(live.subject)") {}
            }
            Section(model.userData.live == nil ? "Switch persona" : "Sample persona (the screens not served live)") {
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
            Label(model.signedInName(persona), systemImage: "person.crop.circle")
                .font(MGFonts.sans(.body))
                .frame(minHeight: 44)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityLabel(Text("Signed in as \(model.signedInName(persona))"))
        .accessibilityIdentifier("mg.0.persona-menu")
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

/// Names a window's unlabelled root group for VoiceOver: the Your Data window's had no
/// description (the Mac audit's screen-reader check), as the console window's had before
/// `WindowSetup` named it.
struct WindowName: NSViewRepresentable {
    let label: String

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        let label = label
        DispatchQueue.main.async { Self.name(view.window, label) }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { Self.name(view.window, label) }
        return view
    }

    func updateNSView(_ view: NSView, context: Context) {}

    /// The shallowest group in the window's content (breadth first), if unlabelled.
    private static func name(_ window: NSWindow?, _ label: String) {
        guard let content = window?.contentView else { return }
        var level = [content]
        while !level.isEmpty {
            if let group = level.first(where: { $0.accessibilityRole() == .group }) {
                if (group.accessibilityLabel() ?? "").isEmpty { group.setAccessibilityLabel(label) }
                return
            }
            level = level.flatMap(\.subviews)
        }
    }
}
