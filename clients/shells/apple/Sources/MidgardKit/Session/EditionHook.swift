import MidgardHooks
import SwiftUI

/// Heimdall's 3D view, as an edition library provides it. [FR-C-1]
public protocol HeimdallThreeDProvider: AnyObject, Sendable {
    /// The entry's title.
    var title: String { get }
    /// The entry itself (a button that sets `showing`), which the app places on Heimdall's
    /// Operating Picture.
    @MainActor func makeEntry(showing: Binding<Bool>) -> AnyView
    /// The 3D view, which the app shows while `showing` is set.
    @MainActor func makeView() -> AnyView
}

/// What an edition library adds to an app for its edition: a banner under the source
/// banner, an About screen and Heimdall's 3D view, or nothing (nil) for an edition that
/// has none. MidgardKit holds no edition wording or view: it only asks. [FR-C-1]
public protocol EditionProvider: AnyObject, Sendable {
    @MainActor func banner(for edition: AppEdition) -> AnyView?
    @MainActor func about(for edition: AppEdition) -> AnyView?
    func threeD(for edition: AppEdition) -> HeimdallThreeDProvider?
}

/// The edition provider registered at link time (`midgard_edition_hook` in MidgardHooks):
/// a weak default that returns nothing, replaced by the edition library's strong
/// definition in the targets that link it (the 3D and maps apps). No call at launch, no
/// lookup by name, no runtime switch: a release app, which links no edition library, has
/// no provider. [FR-C-1]
public enum EditionHook {
    public static var registered: EditionProvider? {
        guard let pointer = midgard_edition_hook() else { return nil }
        return Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue() as? EditionProvider
    }
}
