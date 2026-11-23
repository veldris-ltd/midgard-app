import MidgardKit
import SwiftUI

/// Everything the 3D and maps editions show that the release edition does not: the
/// edition banner, About and Heimdall's MAP and 3D views. Linked only by the 3D and maps apps
/// (MidgardMac3D, MidgardiOS3D, MidgardMacMaps, MidgardiOSMaps); a release app carries
/// none of this code or wording (tools/check-edition.sh). [FR-C-1] [AC-C-13]
public final class EditionUIProvider: EditionProvider {
    public static let shared = EditionUIProvider()

    private init() {}

    @MainActor public func banner(for edition: AppEdition) -> AnyView? {
        EditionBanner(edition).map { AnyView(EditionBannerView($0)) }
    }

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

/// The signed official release every edition is built from.
public enum SignedRelease {
    public static let tag = "v0.1.0-rc.3"
    public static let commit = "ea5899ab"
}

/// The edition banner, under the source banner. The one mapping from an edition to what
/// it shows: an edition with no banner (release) has no About and no MAP or 3D view either.
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

    public var text: String {
        switch self {
        case .threeD: return "3D edition, not in the signed release"
        case .maps: return "Maps edition, not in the signed release"
        }
    }

    /// Shown at accessibility text sizes, where the full text took several lines and left
    /// the iPhone's screen list no room. The full text stays the accessibility label.
    public var compactText: String {
        switch self {
        case .threeD: return "3D edition · not the signed release"
        case .maps: return "Maps edition · not the signed release"
        }
    }

    public var identifier: String {
        switch self {
        case .threeD: return "mg.banner.edition3d"
        case .maps: return "mg.banner.editionmaps"
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
        releaseLine = "\(banner.text). It is \(SignedRelease.tag) (\(SignedRelease.commit)) "
            + "with map and 3D views added to Heimdall."
    }
}
