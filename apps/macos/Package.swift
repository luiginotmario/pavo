// swift-tools-version: 6.2
import PackageDescription

let settings: [SwiftSetting] = [
    .defaultIsolation(MainActor.self),
    .enableUpcomingFeature("NonisolatedNonsendingByDefault"),
    .enableUpcomingFeature("InferIsolatedConformances"),
]

let package = Package(
    name: "Cambio",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(name: "Cambio", path: "Sources/Cambio", swiftSettings: settings),
        .testTarget(name: "CambioTests", dependencies: ["Cambio"], path: "Tests/CambioTests", swiftSettings: settings),
    ]
)
