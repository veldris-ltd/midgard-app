import MidgardHooks
import SwiftUI

/// Heimdall's 3D view, as the 3D edition's library provides it. [FR-C-1]
public protocol HeimdallThreeDView: AnyObject {
    /// The entry's title ("3D view").
    var title: String { get }
    @MainActor func makeView() -> AnyView
}

/// Whether Heimdall offers its "3D" entry: only in a 3D edition (`3d`, `maps`), and only when the
/// 3D library registered itself. The registration is a link-time hook
/// (`midgard_heimdall_3d_hook` in MidgardHooks): a weak default that returns nothing,
/// replaced by the library's strong definition in the one target that links it. There
/// is no call at launch, no lookup by name and no runtime switch. [FR-C-1]
public enum HeimdallThreeD {
    /// The registered view, if the 3D library is linked into this binary.
    public static var registered: HeimdallThreeDView? {
        guard let pointer = midgard_heimdall_3d_hook() else { return nil }
        return Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue() as? HeimdallThreeDView
    }

    /// The entry to show: `registered` in a 3D edition, nothing otherwise.
    public static func entry(edition: AppEdition?, registered: HeimdallThreeDView?) -> HeimdallThreeDView? {
        edition?.hasThreeD == true ? registered : nil
    }
}
