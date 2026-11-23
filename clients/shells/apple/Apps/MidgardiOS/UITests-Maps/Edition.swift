import XCTest

/// The app MidgardiOSMapsUITests runs against: Midgard Maps (MidgardEdition `maps`),
/// Heimdall's MAP and 3D views with their map tiles. [FR-C-1]
enum Edition {
    static let isThreeD = true
    static let banner: (id: String, text: String)? = ("mg.banner.editionmaps", "Maps edition, not in the signed release")
    static let aboutName: String? = "Midgard Maps"
    /// Screens only this edition has, for AccessibilityAudit: Heimdall's MAP and 3D views.
    static let auditedScreens: [(XCUIApplication) -> Void] = [AppNavigation.openMap, AppNavigation.openThreeD]
}
