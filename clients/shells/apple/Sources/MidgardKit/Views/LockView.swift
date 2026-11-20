import MidgardCore
import SwiftUI

/// The locked screen: the persona last used, Unlock (one prompt), and Reset demo behind
/// a confirmation. Buttons in the token accent (`MGStyle`), whatever the app's accent.
/// [FR-C-9] [RC-49]
public struct LockView: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @State private var confirmingReset = false

    public init(model: AppModel, persona: PersonaInfo) {
        self.model = model
        self.persona = persona
    }

    public var body: some View {
        VStack(spacing: MGSpace.s4) {
            Text("Locked").font(MGFonts.display(.title)).accessibilityAddTraits(.isHeader)
            Text(persona.display).font(MGFonts.sans(.body))
            Button { Task { await model.signIn(persona) } } label: { HitTarget("Unlock") }
                .accessibilityIdentifier("mg.0.unlock")
            Button(role: .destructive) { confirmingReset = true } label: { HitTarget("Reset demo") }
                .accessibilityIdentifier("mg.1.reset")
                .confirmationDialog("Reset the demo?", isPresented: $confirmingReset) {
                    Button("Reset demo", role: .destructive) { Task { await model.resetDemo() } }
                } message: {
                    Text("Deletes this device's saved data and key. Signing in again starts the demo from the beginning.")
                }
            MessageView(message: model.message)
        }
        .padding(MGSpace.s5)
        .tint(MGStyle.accent)
    }
}

/// First launch: set up this device's key (no prompt).
public struct EnrolView: View {
    @ObservedObject var model: AppModel
    public init(model: AppModel) { self.model = model }

    public var body: some View {
        VStack(spacing: MGSpace.s4) {
            Text("Set up this device").font(MGFonts.display(.title)).accessibilityAddTraits(.isHeader)
            Text(model.custodyLabel.map { "Keys are protected by: \($0)" } ?? "")
                .font(MGFonts.sans(.footnote))
            Button { Task { await model.enrol() } } label: { HitTarget("Set up") }
                .accessibilityIdentifier("mg.0.enrol")
            MessageView(message: model.message)
        }
        .padding(MGSpace.s5)
        .tint(MGStyle.accent)
    }
}

/// The session's screens by phase, under the demo banner; `content` is the signed-in
/// app (a console). Any touch postpones the idle lock: on iOS a touch anywhere in the
/// window (`IdleTouchWatcher`); on macOS the app watches mouse and key events itself.
/// Not a tap gesture over the screen: SwiftUI then reports every element under it as
/// tappable, and the hit-region audit failed the status chips. [RC-49]
public struct SessionRootView<Content: View>: View {
    @ObservedObject var model: AppModel
    let content: (PersonaInfo) -> Content

    public init(model: AppModel, @ViewBuilder content: @escaping (PersonaInfo) -> Content) {
        self.model = model
        self.content = content
    }

    public var body: some View {
        VStack(spacing: 0) {
            if let banner = model.banner { DemoBannerView(banner) }
            switch model.phase {
            case .loading:
                LoadingView().frame(maxWidth: .infinity, maxHeight: .infinity)
            case .needsEnrolment:
                EnrolView(model: model)
            case .locked:
                if let persona = model.lastPersona {
                    LockView(model: model, persona: persona)
                } else {
                    PersonaPickerView(model: model)
                }
            case let .unlocked(persona):
                VStack(spacing: 0) {
                    if let chips = model.chipsPresentation { StatusChipsView(chips).padding(MGSpace.s2) }
                    MessageView(message: model.message)
                    content(persona)
                }
                .background { IdleTouchWatcher { model.touch() } }
            case let .failed(text):
                Text(text).font(MGFonts.sans(.body)).padding(MGSpace.s5).accessibilityIdentifier("mg.failed")
            }
        }
    }
}

/// A button label at least 44 × 44 pt where it can be hit: the frame sits inside the
/// button, so it is the button's hit region (a `.frame` outside a button only pads it).
/// [RC-49] [AC-C-11]
struct HitTarget: View {
    let title: String
    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
    }
}
