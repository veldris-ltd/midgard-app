import XCTest

/// The app MidgardMacMapsUITests runs against: Midgard Maps (MidgardEdition `maps`),
/// Heimdall's MAP and 3D views, with the client network entitlement for their map
/// tiles. [FR-C-1]
enum Edition {
    static let isThreeD = true
    /// About's description begins so (no edition banner since 25 Nov 2026).
    static let aboutRelease: String? = "Operations consoles with map and 3D views in Heimdall."
    static let aboutName: String? = "Midgard Maps"
    /// Screens only this edition has, for AccessibilityAudit: Heimdall's MAP and 3D views.
    static let auditedScreens: [(XCUIApplication) -> Void] = [{ Nav.openMap($0) }, { Nav.openThreeD($0) }]
}
