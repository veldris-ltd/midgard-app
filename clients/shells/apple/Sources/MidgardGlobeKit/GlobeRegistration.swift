import MidgardKit
import SwiftUI

/// The 3D editions' Heimdall view (Task B7; the globe itself is Phase I). Linked only by
/// the 3D and maps apps (MidgardMac3D, MidgardiOS3D, MidgardMacMaps, MidgardiOSMaps):
/// tools/check-edition.sh fails a release app whose binary or bundle carries any of this
/// library. [FR-C-1]
public final class GlobeRegistration: HeimdallThreeDView {
    /// Lives for the whole process: the hook hands out an unretained pointer to it.
    public static let shared = GlobeRegistration()

    public let title = "3D view"

    private init() {}

    @MainActor public func makeView() -> AnyView { AnyView(GlobePlaceholderView()) }
}

/// The link-time registration: the strong definition of MidgardHooks'
/// `midgard_heimdall_3d_hook`, whose weak default returns nothing. Linking this library
/// is what registers it; nothing calls a register function at launch.
@_cdecl("midgard_heimdall_3d_hook")
public func midgardHeimdall3DHook() -> UnsafeRawPointer? {
    UnsafeRawPointer(Unmanaged.passUnretained(GlobeRegistration.shared).toOpaque())
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
