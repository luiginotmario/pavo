// swift-tools-version: 6.2
import PackageDescription

let settings: [SwiftSetting] = [
    .defaultIsolation(MainActor.self),
    .enableUpcomingFeature("NonisolatedNonsendingByDefault"),
    .enableUpcomingFeature("InferIsolatedConformances"),
]

let package = Package(
    name: "Pavo",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(name: "Pavo", path: "Sources/Pavo", swiftSettings: settings),
        .testTarget(name: "PavoTests", dependencies: ["Pavo"], path: "Tests/PavoTests", swiftSettings: settings),
    ]
)
