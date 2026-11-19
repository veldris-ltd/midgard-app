import XCTest

/// The console shell's catalogue: the six consoles in the design's order with the
/// screens of the plan's Phase C–H tables, and the routes the windows keep. [FR-C-1]
final class ConsoleCatalogTests: XCTestCase {
    func testSixConsolesInSidebarOrder() {
        XCTAssertEqual(ConsoleCatalog.consoles.map(\.name), ["Midgard", "Admin", "Heimdall", "Forge", "Mimir", "Bifrost"])
        XCTAssertEqual(ConsoleCatalog.consoles.map(\.phase), ["C", "D", "E", "F", "G", "H"])
    }

    func testScreenCountsMatchThePlan() {
        XCTAssertEqual(ConsoleCatalog.consoles.map(\.screens.count), [11, 8, 6, 7, 7, 6])
        XCTAssertEqual(ConsoleCatalog.consoles.reduce(0) { $0 + $1.screens.count }, 45)
    }

    func testFirstScreens() {
        XCTAssertEqual(ConsoleCatalog.consoles.map { $0.screens.first },
                       ["Object Fabric", "Overview", "Operating Picture", "Connectors", "Model Plane", "Release Console"])
    }

    func testIdsAreUniqueAndIdentifierSafe() {
        let consoleIds = ConsoleCatalog.consoles.map(\.id)
        XCTAssertEqual(Set(consoleIds).count, 6)
        for console in ConsoleCatalog.consoles {
            let ids = console.screens.map(ConsoleCatalog.slug)
            XCTAssertEqual(Set(ids).count, ids.count, console.name)
            for id in ids + [console.id] {
                XCTAssertNotNil(id.range(of: "^[a-z0-9-]+$", options: .regularExpression), id)
            }
        }
        XCTAssertEqual(ConsoleCatalog.slug("Break-Glass"), "break-glass")
        XCTAssertEqual(ConsoleCatalog.slug("Transform IDE"), "transform-ide")
    }

    func testPlaceholderNamesThePhase() {
        let route = ConsoleRoute(console: 4, screen: 2)
        XCTAssertEqual(ConsoleCatalog.placeholder(for: route), "Agent Studio arrives in Phase G (Mimir).")
    }

    func testRouteClampsOutOfRangeValues() {
        // A restored window may carry a route from another build; it never traps.
        XCTAssertEqual(ConsoleRoute(console: 9, screen: 40).clamped, ConsoleRoute(console: 5, screen: 5))
        XCTAssertEqual(ConsoleRoute(console: -1, screen: -3).clamped, ConsoleRoute(console: 0, screen: 0))
        XCTAssertEqual(ConsoleRoute(console: 2, screen: 3).clamped, ConsoleRoute(console: 2, screen: 3))
    }

    func testSelectingAConsoleOpensItsFirstScreen() {
        var route = ConsoleRoute(console: 0, screen: 7)
        route.select(console: 3)
        XCTAssertEqual(route, ConsoleRoute(console: 3, screen: 0))
        route.select(console: 3)  // the same console again keeps nothing stale
        XCTAssertEqual(route, ConsoleRoute(console: 3, screen: 0))
    }

    func testRouteRoundTripsAsJSON() throws {
        let route = ConsoleRoute(console: 1, screen: 6)
        let data = try JSONEncoder().encode(route)
        XCTAssertEqual(try JSONDecoder().decode(ConsoleRoute.self, from: data), route)
    }

    func testSelectedValueIsOneBased() {
        XCTAssertEqual(ConsoleRoute(console: 0, screen: 0).selectedValue, "1")
        XCTAssertEqual(ConsoleRoute(console: 5, screen: 0).selectedValue, "6")
    }
}
