import XCTest

/// The app MidgardiOSMapsUITests runs against: Midgard Maps (MidgardEdition `maps`),
/// Heimdall's MAP and 3D views with their map tiles. [FR-C-1]
enum Edition {
    static let isThreeD = true
    /// About's description begins so (no edition banner since 25 Nov 2026).
    static let aboutRelease: String? = "Operations consoles with map and 3D views in Heimdall."
    static let aboutName: String? = "Midgard Maps"
    /// Screens only this edition has, for AccessibilityAudit: Heimdall's MAP and 3D views.
    static let auditedScreens: [(XCUIApplication) -> Void] = [AppNavigation.openMap, AppNavigation.openThreeD]
}
