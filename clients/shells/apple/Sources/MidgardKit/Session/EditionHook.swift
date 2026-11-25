import MidgardHooks
import SwiftUI

/// One view of Heimdall's Operating Picture beside the built-in 2D reference layout, as
/// the picture's view switch names it. [FR-C-1] [FR-H-1]
public struct HeimdallPictureView: Hashable, Sendable, Identifiable {
    public let id: String
    public let label: String

    public init(id: String, label: String) {
        self.id = id
        self.label = label
    }

    /// The reference layout every app has (the web's `2d`, its default).
    public static let reference = HeimdallPictureView(id: "2d", label: "2D")
}

/// What a picture view is shown with: whether the session is offline (the OFFLINE chip),
/// whether it is the iPhone's compact layout, and what to do when a track is chosen in
/// it (the iPhone opens the inspector). [FR-H-1]
public struct HeimdallPictureViewContext {
    public let offline: Bool
    public let compact: Bool
    public let onInspect: (String) -> Void

    public init(offline: Bool, compact: Bool, onInspect: @escaping (String) -> Void = { _ in }) {
        self.offline = offline
        self.compact = compact
        self.onInspect = onInspect
    }
}

/// Heimdall's further picture views, as an edition library provides them: the switch
/// lists `views` after the reference layout, and the app shows `makeView` in the
/// picture's place while one is chosen. The views share the picture's model, so the
/// layer switches, the selected track and the inspector are the same in every view.
/// [FR-C-1] [FR-H-1]
public protocol HeimdallPictureViewsProvider: AnyObject, Sendable {
    var views: [HeimdallPictureView] { get }
    @MainActor func makeView(_ view: HeimdallPictureView, picture: HeimdallPictureModel,
                             context: HeimdallPictureViewContext) -> AnyView
}

/// What an edition library adds to an app for its edition: an About screen and
/// Heimdall's further picture views, or nothing (nil) for an edition that has none (no
/// edition banner since the owner's decision of 25 Nov 2026). MidgardKit holds no edition wording or view: it only asks.
/// [FR-C-1]
public protocol EditionProvider: AnyObject, Sendable {
    @MainActor func about(for edition: AppEdition) -> AnyView?
    func pictureViews(for edition: AppEdition) -> HeimdallPictureViewsProvider?
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
