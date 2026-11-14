// swift-tools-version: 6.0
import PackageDescription

// The package the Mac app (and any Swift host) depends on by URL: product TimeTugKit, the Swift front door of the Rust engine.
// The sources live in swift/TimeTugKit/Sources (the package there is the one the tests and `swift-test.sh` use).
//
// On a branch the binary target is a path: build it with `scripts/dev/build-xcframework.sh`. A release tag points at a release
// commit in which scripts/release/write-swift-package.py replaced the path by the URL and checksum of the released XCFramework
// (AGENTS.md, "Releases and release commits").
let package = Package(
    name: "TimeTugKit",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "TimeTugKit", targets: ["TimeTugKit"]),
    ],
    targets: [
        .binaryTarget(name: "TimeTugEngineFFI", url: "https://github.com/binary-companion/timetug-shared/releases/download/2.0.1/TimeTugEngineFFI-2.0.1.xcframework.zip", checksum: "98aa7c4a4ffda6a6cd2ea0efd79fe0c680e18e29552b66c2388b4ff5215492e3"),
        .target(
            name: "TimeTugKitGenerated",
            dependencies: ["TimeTugEngineFFI"],
            path: "swift/TimeTugKit/Sources/TimeTugKitGenerated",
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(
            name: "TimeTugKit",
            dependencies: ["TimeTugKitGenerated"],
            path: "swift/TimeTugKit/Sources/TimeTugKit"
        ),
    ]
)
