// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "KaraokeMic",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "KaraokeMic", targets: ["KaraokeMic"]),
    ],
    targets: [
        .target(
            name: "KaraokeMicCore",
            path: "Sources/KaraokeMicCore"
        ),
        .executableTarget(
            name: "KaraokeMic",
            dependencies: ["KaraokeMicCore"],
            path: "Sources/KaraokeMic"
        ),
        .testTarget(
            name: "KaraokeMicTests",
            dependencies: ["KaraokeMicCore"],
            path: "Tests/KaraokeMicTests"
        ),
    ]
)
