// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "Arsenal",
    platforms: [.macOS(.v26)],
    products: [.executable(name: "Arsenal", targets: ["Arsenal"])],
    targets: [
        .executableTarget(
            name: "Arsenal",
            path: "Sources/Arsenal",
            swiftSettings: [.swiftLanguageMode(.v5)]
        )
    ]
)
