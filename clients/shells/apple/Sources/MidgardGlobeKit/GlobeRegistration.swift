import MidgardKit
import SwiftUI

/// Heimdall's 3D view in the 3D and maps editions (Task B7; the globe itself is Phase I).
/// [FR-C-1]
public final class GlobeRegistration: HeimdallThreeDProvider {
    public static let shared = GlobeRegistration()

    public let title = "3D view"

    private init() {}

    @MainActor public func makeEntry(showing: Binding<Bool>) -> AnyView {
        AnyView(ThreeDEntryButton(title: title, showing: showing))
    }

    @MainActor public func makeView() -> AnyView { AnyView(GlobePlaceholderView()) }
}

/// Heimdall's "3D view" entry: opens the 3D view (the Mac's detail column, a pushed
/// screen on the iPhone), and on the Mac closes it again.
struct ThreeDEntryButton: View {
    let title: String
    @Binding var showing: Bool

    var body: some View {
        #if os(iOS)
        // A navigation-bar button on the iPhone (the app places it there): a symbol, with
        // "Open 3D view" as its label. A full-width row above the picture took the room
        // the picture's lowest track label needs above the console bar.
        Button {
            showing = true
        } label: {
            Label("Open \(title)", systemImage: "cube")
                .labelStyle(.iconOnly)
                .frame(minWidth: 44, minHeight: 44)
                .contentShape(Rectangle())
        }
        .accessibilityIdentifier("mg.heimdall.3d")
        #else
        Button(showing ? "Close \(title)" : "Open \(title)") { showing.toggle() }
            .font(MGFonts.sans(.body))
            .frame(minHeight: 44)
            .accessibilityIdentifier("mg.heimdall.3d")
        #endif
    }
}

/// Until Phase I: a placeholder that says what will be there.
public struct GlobePlaceholderView: View {
    public static let text = "3D view"
    public static let identifier = "mg.heimdall.3d.view"

    public init() {}

    public var body: some View {
        VStack(spacing: MGSpace.s3) {
            Image(systemName: "globe").font(.largeTitle).accessibilityHidden(true)
            Text(Self.text).font(MGFonts.display(.title2)).accessibilityAddTraits(.isHeader)
        }
        .padding(MGSpace.s5)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier(Self.identifier)
    }
}
