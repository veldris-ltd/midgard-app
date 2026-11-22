import XCTest

/// The app MidgardMacMapsUITests runs against: Midgard Maps (MidgardEdition `maps`),
/// the 3D edition with the client network entitlement (for Phase I's map tiles). [FR-C-1]
enum Edition {
    static let isThreeD = true
    static let banner: (id: String, text: String)? = ("mg.banner.editionmaps", "Maps edition, not in the signed release")
    static let aboutName: String? = "Midgard Maps"
    /// Screens only this edition has, for AccessibilityAudit: Heimdall's 3D view.
    static let auditedScreens: [(XCUIApplication) -> Void] = [{ Nav.openThreeD($0) }]
}
