// swift-tools-version: 6.0
import PackageDescription

// TimeTugKit: the Swift front door of the Rust engine (timetug-engine-ffi).
//
//   TimeTugEngineFFI   the XCFramework with the engine (scripts/dev/build-xcframework.sh)
//   TimeTugKitGenerated the UniFFI-generated Swift (committed; scripts/dev/generate-bindings.sh). It does not compile in
//                       Swift 6 language mode, so this one target is built in Swift 5 mode (docs/bindings-spike.md).
//   TimeTugKit          the hand-written layer (conformances, computed properties, streams) in Swift 6 mode; re-exports
//                       the generated module, so an app imports TimeTugKit only.
let package = Package(
    name: "TimeTugKit",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "TimeTugKit", targets: ["TimeTugKit"]),
    ],
    targets: [
        .binaryTarget(name: "TimeTugEngineFFI", path: "../../build/TimeTugEngineFFI.xcframework"),
        .target(
            name: "TimeTugKitGenerated",
            dependencies: ["TimeTugEngineFFI"],
            swiftSettings: [.swiftLanguageMode(.v5)]
        ),
        .target(name: "TimeTugKit", dependencies: ["TimeTugKitGenerated"]),
        .testTarget(name: "TimeTugKitTests", dependencies: ["TimeTugKit"]),
    ]
)
