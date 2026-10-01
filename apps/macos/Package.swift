// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "Cambio",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(name: "Cambio", path: "Sources/Cambio")
    ]
)
