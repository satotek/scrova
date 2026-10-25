// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "ScrovaKit",
    // Keep in sync with the deployment targets in scripts/build-apple.sh.
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "ScrovaKit", targets: ["ScrovaKit"]),
    ],
    targets: [
        // Built by scripts/build-apple.sh; not checked in.
        .binaryTarget(name: "ScrovaFFI", path: "ScrovaFFI.xcframework"),
        .target(name: "ScrovaKit", dependencies: ["ScrovaFFI"]),
        .testTarget(name: "ScrovaKitTests", dependencies: ["ScrovaKit"]),
    ]
)
