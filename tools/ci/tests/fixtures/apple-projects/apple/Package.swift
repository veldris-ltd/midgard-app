// swift-tools-version:5.10
// Fixture for tools/ci/tests/apple-projects.test.sh: stands for the MidgardKit package,
// which the app projects reference as a local package (path ../..). Never built.
import PackageDescription

let package = Package(
    name: "FixtureKit",
    products: [.library(name: "FixtureKit", targets: ["FixtureKit"])],
    targets: [.target(name: "FixtureKit", path: "Sources/FixtureKit")]
)
