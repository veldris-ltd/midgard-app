import MidgardKit
import SwiftUI

/// Everything the 3D and maps editions show that the release edition does not: About and
/// Heimdall's MAP and 3D views. Linked only by the 3D and maps apps (MidgardMacMaps,
/// MidgardiOSMaps); a release app carries none of this code or wording
/// (tools/check-edition.sh). The edition banner was removed by the owner's decision of
/// 25 Nov 2026. [FR-C-1] [AC-C-13]
public final class EditionUIProvider: EditionProvider {
    public static let shared = EditionUIProvider()

    private init() {}

    @MainActor public func about(for edition: AppEdition) -> AnyView? {
        EditionAbout(edition).map { AnyView(AboutView($0)) }
    }

    public func pictureViews(for edition: AppEdition) -> HeimdallPictureViewsProvider? {
        EditionBanner(edition) == nil ? nil : GlobeRegistration.shared
    }
}

/// The link-time registration: the strong definition of MidgardHooks'
/// `midgard_edition_hook`, whose weak default returns nothing. Linking this library is
/// what registers it; nothing calls a register function at launch.
@_cdecl("midgard_edition_hook")
public func midgardEditionHook() -> UnsafeRawPointer? {
    UnsafeRawPointer(Unmanaged.passUnretained(EditionUIProvider.shared).toOpaque())
}

/// Which edition this is, for About and the picture views: the one mapping from an
/// edition to what it adds. An edition with none (release) has no About and no MAP or 3D
/// view. (The name is kept from when each edition also showed a banner.)
public enum EditionBanner: Equatable, Sendable {
    case threeD, maps

    public init?(_ edition: AppEdition) {
        switch edition {
        case .release: return nil
        case .threeD: self = .threeD
        case .maps: self = .maps
        }
    }

    /// The edition's name, as About shows it.
    public var name: String {
        switch self {
        case .threeD: return "Midgard 3D"
        case .maps: return "Midgard Maps"
        }
    }
}

/// What an edition's About screen says. nil for the release edition, which has none.
public struct EditionAbout: Equatable, Sendable {
    public let name: String
    public let editionLine: String
    public let releaseLine: String

    public init?(_ edition: AppEdition) {
        guard let banner = EditionBanner(edition) else { return nil }
        name = banner.name
        editionLine = "Edition: \(banner.name)"
        releaseLine = "Operations consoles with map and 3D views in Heimdall. The app starts on a "
            + "built-in sample workspace; connect your own data server or import JSON files from Your Data."
    }
}
