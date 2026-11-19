@preconcurrency import MidgardCore
import MidgardKit
import SwiftUI

/// A window's root: the session's screens by phase (MidgardKit's `SessionRootView`:
/// the source banner, enrolment, sign-in, lock, the status chips) and, signed in, the
/// console shell. Carries the audit's hooks: `mg.motion` (reduced/full) in every phase,
/// with every animation disabled when motion is reduced. [FR-C-1] [RC-49] [AC-C-11]
struct RootView: View {
    @ObservedObject var model: AppModel
    @Binding var route: ConsoleRoute
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion

    var body: some View {
        let reduced = MotionPolicy.reduced(system: systemReduceMotion)
        SessionRootView(model: model) { persona in
            ConsoleShell(model: model, persona: persona, route: $route)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        .background(alignment: .topLeading) {
            HiddenMarker(id: "mg.motion", label: "Motion", value: reduced ? "reduced" : "full")
        }
        .transaction { transaction in
            if reduced {
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        }
        .focusedSceneValue(\.consoleRoute, $route)
    }
}

/// An element the UI tests read (`mg.motion`, `mg.console.selected`): one point, no
/// drawing, a label and a value.
struct HiddenMarker: View {
    let id: String
    let label: String
    let value: String

    var body: some View {
        Color.clear
            .frame(width: 1, height: 1)
            .accessibilityElement()
            .accessibilityLabel(Text(label))
            .accessibilityValue(Text(value))
            .accessibilityIdentifier(id)
    }
}

/// Signed in: the persona menu above a three-column split view. The sidebar lists the
/// six consoles (⌘1…⌘6) and, under the selected one, its screens; the content and
/// detail columns show the screen's placeholder until its phase builds it. [FR-C-1]
struct ConsoleShell: View {
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
            } content: {
                ScreenContent(route: route.clamped)
                    .navigationSplitViewColumnWidth(min: 280, ideal: 360)
            } detail: {
                ScreenDetail(route: route.clamped)
            }
        }
        .background(alignment: .topLeading) {
            HiddenMarker(id: "mg.console.selected", label: "Selected console", value: route.selectedValue)
        }
        .navigationTitle(ConsoleCatalog.console(route).name)
        .navigationSubtitle(ConsoleCatalog.screenName(route))
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
        case let .console(c): return "mg.\(number).console.\(ConsoleCatalog.consoles[c].id)"
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
    }

    @ViewBuilder private func row(_ entry: SidebarEntry) -> some View {
        switch entry.item {
        case let .console(c):
            Label(entry.title, systemImage: symbol(c))
                .font(MGFonts.sans(.body).weight(.semibold))
                .accessibilityLabel(Text("\(entry.title) console"))
        case .screen:
            Text(entry.title)
                .font(MGFonts.sans(.body))
                .padding(.leading, MGSpace.s4)
        }
    }

    private func symbol(_ console: Int) -> String {
        ["square.grid.3x3", "person.badge.key", "map", "hammer", "brain", "shippingbox"][console]
    }
}

/// The content column: the screen's name and what will appear there.
struct ScreenContent: View {
    let route: ConsoleRoute

    var body: some View {
        let console = ConsoleCatalog.console(route)
        let name = ConsoleCatalog.screenName(route)
        VStack(alignment: .leading, spacing: MGSpace.s3) {
            Text(name).font(MGFonts.display(.title2)).accessibilityAddTraits(.isHeader)
            Text(ConsoleCatalog.placeholder(for: route)).font(MGFonts.sans(.body))
            Text("Until then this screen is empty: no data source is connected.")
                .font(MGFonts.sans(.body))
        }
        .padding(MGSpace.s5)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("mg.content.\(ConsoleCatalog.slug(name))")
        .navigationTitle(console.name)
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

extension AppModel {
    /// Whether the session is offline, from the sync chip.
    var isOffline: Bool { chips?.sync.phase == .offline }
}
