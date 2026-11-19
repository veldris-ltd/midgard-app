import MidgardCore
import SwiftUI

/// Sign in as one of the demo personas. Signing in prompts for Face ID / Touch ID (or
/// the passcode) through the key root. [FR-C-9]
public struct PersonaPickerView: View {
    @ObservedObject var model: AppModel
    public init(model: AppModel) { self.model = model }

    public var body: some View {
        VStack(alignment: .leading, spacing: MGSpace.s4) {
            Text("Sign in").font(MGFonts.display(.title)).accessibilityAddTraits(.isHeader)
            Text("Demo personas").font(MGFonts.mono(.caption)).foregroundStyle(.secondary)
            ForEach(Array(model.personas.enumerated()), id: \.element.id) { index, persona in
                Button {
                    Task { await model.signIn(persona) }
                } label: {
                    HStack {
                        VStack(alignment: .leading) {
                            Text(persona.display).font(MGFonts.sans(.body))
                            Text(persona.role.uppercased()).font(MGFonts.mono(.caption))
                        }
                        Spacer()
                        Text(persona.clearanceLabel).font(MGFonts.mono(.caption2))
                    }
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
                }
                .accessibilityLabel(Text("\(persona.display), \(persona.role), clearance \(persona.clearanceLabel)"))
                .accessibilityIdentifier("mg.\(index).persona.\(persona.id)")
            }
            MessageView(message: model.message)
        }
        .padding(MGSpace.s5)
    }
}

/// The model's latest message, if any.
struct MessageView: View {
    let message: String?
    var body: some View {
        if let message {
            Text(message).font(MGFonts.sans(.footnote)).accessibilityIdentifier("mg.message")
        }
    }
}
