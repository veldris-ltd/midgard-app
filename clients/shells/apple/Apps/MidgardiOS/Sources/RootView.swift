import MidgardCore
import MidgardKit
import SwiftUI

/// The app's root: the session's phases (enrol, sign in, locked, the consoles) under the
/// source banner, from MidgardKit's `SessionRootView`, over a status line that carries
/// the audit's hooks. It starts the model off the main thread, locks when the app goes
/// to the background, and covers the screen (`PrivacyCover`) while the app is in the
/// background, or inactive with a persona signed in, so the app switcher shows nothing.
/// The accent is MidgardKit's `MGStyle.accent`, design tokens that meet AA in both modes
/// (the system blue does not, on the sign-in screen's persona button).
/// [FR-C-1] [FR-C-9] [AC-C-11] [RC-49]
struct RootView: View {
    @ObservedObject var model: AppModel
    /// ⌘1…⌘6, from the app delegate's key commands.
    @ObservedObject var shortcuts: ConsoleShortcuts
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion
    @State private var started = false
    /// The console shown once signed in (1…6): kept here so the status line can say it.
    @State private var console = 1

    private var reducedMotion: Bool { MotionPolicy.reduced(system: systemReduceMotion) }

    /// Not while merely inactive and locked: Face ID's prompt makes the scene inactive,
    /// and covering the sign-in or lock screen then only made it flash. Nothing is
    /// shown there that the cover would hide.
    private var covered: Bool {
        switch scenePhase {
        case .background: return true
        case .inactive: return model.isUnlocked
        default: return false
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            SessionRootView(model: model) { persona in
                ConsoleShell(model: model, persona: persona, selection: $console)
            }
            // The banner at the top of the screen, whatever the phase's content.
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            StatusLine(reduced: reducedMotion, console: model.isUnlocked ? console : nil)
        }
        .tint(MGStyle.accent)
        .overlay {
            if covered { PrivacyCover() }
        }
        .transaction { transaction in
            if reducedMotion {
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        }
        .task {
            guard !started else { return }
            started = true
            await model.start(making: { try LaunchEnvironment.make() })
        }
        .onChange(of: shortcuts.request) { _, request in
            if let request, model.isUnlocked { console = request.number }
        }
        // VoiceOver moving its focus is activity too: it is not a touch the window sees.
        .onReceive(NotificationCenter.default.publisher(for: UIAccessibility.elementFocusedNotification)) { _ in
            model.touch()
        }
        .onChange(of: scenePhase) { _, phase in
            // Lock on leaving the app; only a signed-in session has anything to lock.
            guard phase == .background, model.isUnlocked else { return }
            // Saving and locking runs off the main thread: ask for the time to finish
            // it before the app is suspended.
            let task = BackgroundTask(name: "Lock Midgard")
            Task {
                await model.lock()
                task.end()
            }
        }
    }
}

/// Background execution time for one piece of work: begun on creation, ended by `end()`
/// or, if the system runs out of time first, by its expiration handler. Ends once.
@MainActor
final class BackgroundTask {
    private var id = UIBackgroundTaskIdentifier.invalid

    init(name: String) {
        id = UIApplication.shared.beginBackgroundTask(withName: name) { [self] in
            MainActor.assumeIsolated { end() }
        }
    }

    func end() {
        guard id != .invalid else { return }
        UIApplication.shared.endBackgroundTask(id)
        id = .invalid
    }
}

/// The status line at the bottom of the screen: which console is shown ("Console 1 of
/// 6: Midgard", `mg.console.selected` = 1…6, once signed in) and whether animations run
/// ("Animations off (Reduce Motion)", `mg.motion` = reduced/full, in every phase).
/// Visible text, as on the Mac, so VoiceOver and the contrast audit see what the UI
/// tests read (a hidden one-point marker failed the contrast audit). [RC-49] [AC-C-11]
struct StatusLine: View {
    let reduced: Bool
    let console: Int?
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        // One line, or, at accessibility text sizes, one statement under the other.
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: MGSpace.s1))
            : AnyLayout(HStackLayout(alignment: .firstTextBaseline, spacing: MGSpace.s3))
        layout {
            if let console {
                Text("Console \(console) of \(ConsoleCatalog.consoles.count): \(ConsoleCatalog.console(number: console).name)")
                    .accessibilityValue(Text("\(console)"))
                    .accessibilityIdentifier("mg.console.selected")
            }
            if !typeSize.isAccessibilitySize { Spacer(minLength: MGSpace.s2) }
            Text(reduced ? "Animations off (Reduce Motion)" : "Animations on")
                .accessibilityValue(Text(reduced ? "reduced" : "full"))
                .accessibilityIdentifier("mg.motion")
        }
        .font(MGFonts.sans(.caption))
        .foregroundStyle(Color.primary)
        .padding(.horizontal, MGSpace.s4)
        .padding(.vertical, MGSpace.s1)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(uiColor: .systemBackground))
        .overlay(alignment: .top) { Divider() }
    }
}

extension Console {
    /// The console's symbol in the console bar and on its screens. (The catalogue is
    /// MidgardKit's.)
    var symbol: String {
        switch id {
        case "midgard": return "square.stack.3d.up"
        case "admin": return "gearshape.2"
        case "heimdall": return "scope"
        case "forge": return "hammer"
        case "mimir": return "brain"
        case "bifrost": return "shippingbox"
        default: return "square"
        }
    }
}

/// What the app switcher shows instead of the screen.
struct PrivacyCover: View {
    var body: some View {
        ZStack {
            Color(uiColor: .systemBackground)
            VStack(spacing: MGSpace.s3) {
                Image(systemName: "lock.fill").font(.largeTitle).accessibilityHidden(true)
                Text("Midgard").font(MGFonts.display(.title))
            }
        }
        .ignoresSafeArea()
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("mg.privacy")
    }
}

/// Signed in: the six consoles, in the order Midgard, Admin, Heimdall, Forge, Mimir,
/// Bifrost, switched by a bar of six at the bottom (⌘1…⌘6 on a hardware keyboard: the
/// app delegate's key commands).
/// Not a system `TabView`: an iPhone tab bar shows five, and the sixth console would go
/// under UIKit's More list, which wraps the console's own navigation stack in its own
/// (and whose rows did not respond in the simulator). Only the selected console is in
/// the view tree, so VoiceOver and the audit see one; each console's open screen and
/// search are kept here, so switching back returns to where it was.
struct ConsoleShell: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var selection: Int
    @State private var paths: [Int: [String]] = [:]
    @State private var queries: [Int: String] = [:]

    private var console: Console { ConsoleCatalog.console(number: selection) }

    var body: some View {
        ConsoleTab(model: model, console: console, persona: persona,
                   path: Binding(get: { paths[console.number] ?? [] }, set: { paths[console.number] = $0 }),
                   query: Binding(get: { queries[console.number] ?? "" }, set: { queries[console.number] = $0 }))
            .id(console.number)
            .safeAreaInset(edge: .bottom, spacing: 0) { ConsoleBar(selection: $selection) }
    }
}

/// The six consoles' buttons. At accessibility text sizes the
/// names give way to the symbols (a long press shows the large-content viewer).
struct ConsoleBar: View {
    @Binding var selection: Int
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        HStack(spacing: 0) {
            ForEach(ConsoleCatalog.consoles) { console in
                let selected = console.number == selection
                Button {
                    selection = console.number
                } label: {
                    VStack(spacing: 2) {
                        // Capped like a system tab bar's: six symbols at accessibility
                        // sizes would not fit the screen's width.
                        Image(systemName: console.symbol)
                            .font(.title3)
                            .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                            .accessibilityHidden(true)
                        if !typeSize.isAccessibilitySize {
                            Text(console.name)
                                .font(MGFonts.sans(.caption2).weight(selected ? .semibold : .regular))
                                .lineLimit(1)
                                .minimumScaleFactor(0.6)
                        }
                    }
                    .foregroundStyle(Color.primary)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .background {
                        if selected {
                            RoundedRectangle(cornerRadius: 10).fill(Color(uiColor: .tertiarySystemFill))
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text(console.name))
                .accessibilityAddTraits(selected ? [.isSelected] : [])
                .accessibilityShowsLargeContentViewer {
                    Image(systemName: console.symbol)
                    Text(console.name)
                }
                .accessibilityIdentifier("mg.console.tab.\(console.slug)")
            }
        }
        .padding(.horizontal, MGSpace.s2)
        .padding(.vertical, MGSpace.s1)
        .background(Color(uiColor: .systemBackground))
        .overlay(alignment: .top) { Divider() }
    }
}

/// One console: its screens, searchable, pull to refresh (sync now), each opening its
/// placeholder.
struct ConsoleTab: View {
    @ObservedObject var model: AppModel
    let console: Console
    let persona: PersonaInfo
    @Binding var path: [String]
    @Binding var query: String

    private var screens: [String] {
        let q = query.trimmingCharacters(in: .whitespaces)
        return q.isEmpty ? console.screens : console.screens.filter { $0.localizedCaseInsensitiveContains(q) }
    }

    var body: some View {
        NavigationStack(path: $path) {
            // A scroll view of plain buttons, not a List of NavigationLinks. (A List's rows
            // once ignored taps under a tap gesture on the signed-in screen; that gesture
            // is gone, see MidgardKit's IdleTouchWatcher.) The rows stay buttons because
            // the accessibility audits were measured on them: their identifiers, 44 pt
            // height and token colours are set here, where a List's are the system's.
            ScrollView {
                LazyVStack(spacing: 0) {
                    ScreenSearchField(console: console, query: $query)
                        .padding(.horizontal, MGSpace.s4)
                        .padding(.vertical, MGSpace.s2)
                    // mg.0 is the session menu above the list; the rows follow in order.
                    ForEach(Array(screens.enumerated()), id: \.element) { index, screen in
                        Button {
                            path.append(screen)
                        } label: {
                            HStack {
                                Text(screen).font(MGFonts.sans(.body)).multilineTextAlignment(.leading)
                                Spacer(minLength: MGSpace.s2)
                                Image(systemName: "chevron.right").font(.footnote.weight(.semibold))
                                    .accessibilityHidden(true)
                            }
                            .foregroundStyle(Color.primary)
                            .padding(.horizontal, MGSpace.s4)
                            .padding(.vertical, MGSpace.s3)
                            .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(Text(screen))
                        .accessibilityHint(Text("Opens the screen"))
                        .accessibilityIdentifier("mg.\(index + 1).\(console.slug).\(ConsoleCatalog.slug(screen))")
                        Divider().padding(.leading, MGSpace.s4)
                    }
                }
            }
            .overlay {
                if screens.isEmpty {
                    ContentUnavailableView.search(text: query)
                }
            }
            .navigationTitle(console.name)
            // Inline: a large title leaves Midgard's eleventh screen cut by the console bar
            // on an iPhone 17, and a row cut by an edge fails the contrast audit.
            .navigationBarTitleDisplayMode(.inline)
            .navigationDestination(for: String.self) { screen in
                ScreenPlaceholder(model: model, console: console, screen: screen, persona: persona)
            }
            .refreshable { await model.syncNow() }
            // Typing is activity: a hardware keyboard's keys are not touches.
            .onChange(of: query) { model.touch() }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { SessionMenu(model: model, persona: persona) }
            }
        }
    }
}

/// The search over a console's screens, at the top of its list. Not `.searchable`: the
/// system search bar's grey placeholder fails the contrast audit and its field clips at
/// accessibility text sizes. This one uses the app's fonts, a short prompt lettered in
/// the secondary text token, and a field at least 44 pt tall (its own frame is the hit
/// region the audit measures, not the capsule around it).
struct ScreenSearchField: View {
    let console: Console
    @Binding var query: String
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: MGSpace.s2) {
            Image(systemName: "magnifyingglass").accessibilityHidden(true)
            TextField(text: $query, prompt: Text("Search").foregroundStyle(MGStyle.secondaryText)) {
                Text("Search \(console.name) screens")
            }
            .font(MGFonts.sans(.body))
            .frame(minHeight: 44)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .submitLabel(.search)
            .focused($focused)
            .accessibilityLabel(Text("Search \(console.name) screens"))
            .accessibilityIdentifier("mg.search.\(console.slug)")
            if !query.isEmpty {
                Button {
                    query = ""
                } label: {
                    Image(systemName: "xmark.circle.fill").frame(minWidth: 44, minHeight: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text("Clear search"))
                .accessibilityIdentifier("mg.search.clear")
            }
        }
        .foregroundStyle(Color.primary)
        .padding(.horizontal, MGSpace.s3)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color(uiColor: .secondarySystemBackground)))
        .contentShape(Rectangle())
        .onTapGesture { focused = true }
    }
}

/// A screen until its phase: what it is and when it arrives; Heimdall's Operating
/// Picture, matched to the web console (owner decision 22 Nov 2026), is built: the
/// picture first and the inspector as a sheet (`HeimdallOperatingPictureCompact`). In the
/// 3D and maps editions the Operating Picture also offers the edition library's "3D
/// view" entry (`AppModel.threeDEntry`, from the link-time edition provider); a release
/// app has no provider. [FR-C-1] [FR-H-4] [FR-H-5]
struct ScreenPlaceholder: View {
    @ObservedObject var model: AppModel
    let console: Console
    let screen: String
    let persona: PersonaInfo
    @State private var showingThreeD = false

    private var isOperatingPicture: Bool { console.id == "heimdall" && screen == console.screens.first }

    /// The 3D entry, on Heimdall's first screen only, when this app has one.
    private var threeD: HeimdallThreeDProvider? { isOperatingPicture ? model.threeDEntry : nil }

    var body: some View {
        content
            .navigationTitle(screen)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { SessionMenu(model: model, persona: persona) }
            }
            .modifier(ThreeDEntry(entry: threeD, showing: $showingThreeD))
    }

    @ViewBuilder private var content: some View {
        if isOperatingPicture {
            HeimdallOperatingPictureCompact(app: model, personaId: persona.id)
        } else {
            ScrollView {
                VStack(spacing: MGSpace.s3) {
                    Image(systemName: console.symbol).font(.largeTitle).accessibilityHidden(true)
                    Text(screen).font(MGFonts.display(.title2)).accessibilityAddTraits(.isHeader)
                    Text("This \(console.name) screen arrives in Phase \(console.phase), matched to its web console.")
                        .font(MGFonts.sans(.body))
                        .fixedSize(horizontal: false, vertical: true)
                }
                .multilineTextAlignment(.center)
                .foregroundStyle(Color.primary)
                .padding(MGSpace.s5)
                .frame(maxWidth: .infinity)
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("mg.placeholder.\(console.slug).\(ConsoleCatalog.slug(screen))")
            }
        }
    }
}

/// With an edition provider's 3D entry: the entry above the screen (a top inset), which
/// pushes the 3D view. Without one (every release app): the screen as it is.
struct ThreeDEntry: ViewModifier {
    let entry: HeimdallThreeDProvider?
    @Binding var showing: Bool

    func body(content: Content) -> some View {
        if let entry {
            content
                .safeAreaInset(edge: .top, spacing: 0) {
                    entry.makeEntry(showing: $showing)
                        .padding(.horizontal, MGSpace.s4)
                        .padding(.vertical, MGSpace.s2)
                        .background(Color(uiColor: .systemBackground))
                }
                .navigationDestination(isPresented: $showing) {
                    entry.makeView()
                        .navigationTitle(entry.title)
                        .navigationBarTitleDisplayMode(.inline)
                }
        } else {
            content
        }
    }
}

/// The session menu: who is signed in (and switching persona), sync now, offline, lock;
/// in the 3D and maps editions only, About, from the edition provider (`AppModel.about`).
/// The release app has no provider, so its menu is unchanged.
struct SessionMenu: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @State private var showingAbout = false

    var body: some View {
        let offline = model.isOffline
        Menu {
            Section("Signed in as \(persona.display)") {
                ForEach(model.personas, id: \.id) { other in
                    Button {
                        Task { await model.switchPersona(other) }
                    } label: {
                        if other.id == persona.id {
                            Label(other.display, systemImage: "checkmark")
                        } else {
                            Text(other.display)
                        }
                    }
                    .disabled(other.id == persona.id)
                }
            }
            Button {
                Task { await model.syncNow() }
            } label: {
                Label("Sync now", systemImage: "arrow.triangle.2.circlepath")
            }
            .accessibilityIdentifier("mg.session.sync")
            Button {
                Task { await model.setOffline(!offline) }
            } label: {
                Label(offline ? "Go online" : "Go offline", systemImage: offline ? "wifi" : "wifi.slash")
            }
            .accessibilityIdentifier("mg.session.offline")
            Button {
                Task { await model.lock() }
            } label: {
                Label("Lock", systemImage: "lock")
            }
            .accessibilityIdentifier("mg.session.lock")
            if model.about != nil {
                Button {
                    showingAbout = true
                } label: {
                    Label("About", systemImage: "info.circle")
                }
                .accessibilityIdentifier("mg.session.about")
            }
        } label: {
            Label("Session", systemImage: "person.crop.circle")
                .frame(minWidth: 44, minHeight: 44)
        }
        .accessibilityLabel(Text("Session: \(persona.display)"))
        .accessibilityIdentifier("mg.0.session-menu")
        .modifier(AboutSheet(about: model.about, showing: $showingAbout))
    }
}

/// With an edition provider's About: the sheet the session menu opens. Without one
/// (every release app): nothing.
struct AboutSheet: ViewModifier {
    let about: AnyView?
    @Binding var showing: Bool

    func body(content: Content) -> some View {
        if let about {
            content.sheet(isPresented: $showing) {
                NavigationStack {
                    about
                        .frame(maxHeight: .infinity, alignment: .top)
                        .toolbar {
                            ToolbarItem(placement: .confirmationAction) {
                                Button("Done") { showing = false }
                                    .accessibilityIdentifier("mg.about.done")
                            }
                        }
                }
            }
        } else {
            content
        }
    }
}
