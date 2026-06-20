// swift-tools-version: 6.2
import PackageDescription

// The runnable app lives in the Xcode project (KaraokeMic.xcodeproj, generated
// from project.yml). This package provides the testable core library; the app
// entry point in App/ is compiled by the Xcode app target, which links this
// library as a package product.
let package = Package(
    name: "KaraokeMic",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "KaraokeMicCore", targets: ["KaraokeMicCore"]),
    ],
    targets: [
        .target(
            name: "KaraokeMicCore",
            path: "Sources/KaraokeMicCore"
        ),
        .testTarget(
            name: "KaraokeMicTests",
            dependencies: ["KaraokeMicCore"],
            path: "Tests/KaraokeMicTests"
        ),
    ]
)
