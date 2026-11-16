import MidgardCore
import SwiftUI

/// The locked screen: the persona last used, Unlock (one prompt), and Reset demo behind
/// a confirmation. [FR-C-9]
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
            Button("Unlock") { Task { await model.signIn(persona) } }
                .frame(minWidth: 44, minHeight: 44)
                .accessibilityIdentifier("mg.0.unlock")
            Button("Reset demo", role: .destructive) { confirmingReset = true }
                .frame(minWidth: 44, minHeight: 44)
                .accessibilityIdentifier("mg.1.reset")
                .confirmationDialog("Reset the demo?", isPresented: $confirmingReset) {
                    Button("Reset demo", role: .destructive) { Task { await model.resetDemo() } }
                } message: {
                    Text("Deletes this device's saved data and key. Signing in again starts the demo from the beginning.")
                }
            MessageView(message: model.message)
        }
        .padding(MGSpace.s5)
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
            Button("Set up") { Task { await model.enrol() } }
                .frame(minWidth: 44, minHeight: 44)
                .accessibilityIdentifier("mg.0.enrol")
            MessageView(message: model.message)
        }
        .padding(MGSpace.s5)
    }
}

/// The session's screens by phase, under the demo banner; `content` is the signed-in
/// app (a console). Any touch postpones the idle lock.
public struct SessionRootView<Content: View>: View {
    @ObservedObject var model: AppModel
    let content: (PersonaInfo) -> Content

    public init(model: AppModel, @ViewBuilder content: @escaping (PersonaInfo) -> Content) {
        self.model = model
        self.content = content
    }

    public var body: some View {
        VStack(spacing: 0) {
            if let banner = model.demoBanner { DemoBannerView(banner) }
            switch model.phase {
            case .loading:
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
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
                .simultaneousGesture(TapGesture().onEnded { model.touch() })
            case let .failed(text):
                Text(text).font(MGFonts.sans(.body)).padding(MGSpace.s5).accessibilityIdentifier("mg.failed")
            }
        }
    }
}
