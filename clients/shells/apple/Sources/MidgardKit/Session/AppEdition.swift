import Foundation

/// Which of the three apps this is (owner decision 22 Nov 2026), from the Info.plist key
/// `MidgardEdition`:
/// - `release`: Midgard, the signed official release only, with no visible change;
/// - `3d`: Midgard 3D, the same release plus a 3D view in Heimdall, with no network;
/// - `maps`: Midgard Maps, the 3D edition plus the client network entitlement, the
///   only edition with one (for Phase I's map tiles).
/// Neither 3D edition is in the signed release. Any other value, or none, is the "app
/// misconfigured" failed state. [FR-C-1] [AC-C-13]
public enum AppEdition: String, Equatable, Sendable, CaseIterable {
    case release = "release"
    case threeD = "3d"
    case maps = "maps"

    /// Whether this edition has Heimdall's 3D view (and links the 3D library).
    public var hasThreeD: Bool { self != .release }
}

/// The signed official release every edition is built from (named only in the 3D
/// editions' About screen).
public enum SignedRelease {
    public static let tag = "v0.1.0-rc.3"
    public static let commit = "ea5899ab"
}

/// The second banner, under the source banner: only the 3D editions have one.
public enum EditionBanner: Equatable, Sendable {
    case threeD, maps

    /// The banner `edition` shows, if any (none for the release edition).
    public init?(_ edition: AppEdition) {
        switch edition {
        case .release: return nil
        case .threeD: self = .threeD
        case .maps: self = .maps
        }
    }

    public var text: String {
        switch self {
        case .threeD: return "3D edition, not in the signed release"
        case .maps: return "3D maps edition, not in the signed release"
        }
    }

    public var identifier: String {
        switch self {
        case .threeD: return "mg.banner.edition3d"
        case .maps: return "mg.banner.editionmaps"
        }
    }
}

/// What a 3D edition's About screen says. The release edition has no About screen and no
/// visible change at all (owner decision 22 Nov 2026): there is no value for it, so
/// nothing can show one.
public struct EditionAbout: Equatable, Sendable {
    public let name: String
    public let editionLine: String
    public let releaseLine: String

    /// A 3D edition's About; nil for the release edition.
    public init?(_ edition: AppEdition) {
        guard let banner = EditionBanner(edition) else { return nil }
        name = edition == .maps ? "Midgard Maps" : "Midgard 3D"
        editionLine = "Edition: \(name)"
        releaseLine = "\(banner.text). It is \(SignedRelease.tag) (\(SignedRelease.commit)) "
            + "with a 3D view added to Heimdall."
    }
}
