import Foundation

/// Which of the three apps this is (owner decision 22 Nov 2026), from the Info.plist key
/// `MidgardEdition`: `release`, `3d` or `maps`. Any other value, or none, is the "app
/// misconfigured" failed state. The release apps read it silently: everything an edition
/// shows (its banner, About, Heimdall's 3D view) comes from the edition library through
/// `EditionHook`, and only the 3D and maps apps link that library. [FR-C-1] [AC-C-13]
public enum AppEdition: String, Equatable, Sendable, CaseIterable {
    case release = "release"
    case threeD = "3d"
    case maps = "maps"
}
