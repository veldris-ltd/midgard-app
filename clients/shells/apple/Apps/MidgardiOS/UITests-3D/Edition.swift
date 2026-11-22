import XCTest

/// The app MidgardiOS3DUITests runs against: Midgard 3D (MidgardEdition `3d`), the
/// release plus Heimdall's 3D view. [FR-C-1]
enum Edition {
    static let isThreeD = true
    static let banner: (id: String, text: String)? = ("mg.banner.edition3d", "3D edition, not in the signed release")
    static let aboutName: String? = "Midgard 3D"
    /// Screens only this edition has, for AccessibilityAudit: Heimdall's 3D view.
    static let auditedScreens: [(XCUIApplication) -> Void] = [AppNavigation.openThreeD]
}
