// swift-tools-version: 6.2
import PackageDescription

let settings: [SwiftSetting] = [
    .defaultIsolation(MainActor.self),
    .enableUpcomingFeature("NonisolatedNonsendingByDefault"),
    .enableUpcomingFeature("InferIsolatedConformances"),
]

let package = Package(
    name: "Pavo",
    platforms: [.macOS(.v14)],
    targets: [
        .executableTarget(name: "Pavo", path: "Sources/Pavo", swiftSettings: settings),
        // the engine's way into apple's vision framework; ships as Contents/Helpers/pavo-vision
        .executableTarget(name: "PavoVision", path: "Sources/PavoVision", swiftSettings: settings),
        .testTarget(name: "PavoTests", dependencies: ["Pavo"], path: "Tests/PavoTests", swiftSettings: settings),
    ]
)
