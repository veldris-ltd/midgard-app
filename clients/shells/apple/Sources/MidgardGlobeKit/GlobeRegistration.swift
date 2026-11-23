import MidgardKit
import SwiftUI

/// Heimdall's MAP and 3D views in the Maps edition (Phase I), after the web console's
/// `ViewSwitch`: 2D (the app's own reference layout), MAP (the tracks on a vector map)
/// and 3D (the globe). Both share the picture's model, so the layer switches, the
/// selected track and the inspector are the same in every view. [FR-C-1] [FR-H-1]
public final class GlobeRegistration: HeimdallPictureViewsProvider {
    public static let shared = GlobeRegistration()

    public static let map = HeimdallPictureView(id: "map", label: "MAP")
    public static let globe = HeimdallPictureView(id: "3d", label: "3D")

    public let views = [GlobeRegistration.map, GlobeRegistration.globe]

    private init() {}

    @MainActor public func makeView(_ view: HeimdallPictureView, picture: HeimdallPictureModel,
                                    context: HeimdallPictureViewContext) -> AnyView {
        if view.id == Self.map.id {
            return AnyView(HeimdallMapView(picture: picture, context: context))
        }
        return AnyView(HeimdallGlobeView(picture: picture, context: context))
    }
}
