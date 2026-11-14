// swift-tools-version: 6.0
import PackageDescription
let package = Package(
    name: "TimeTugSpike",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [.library(name: "TimeTugSpike", targets: ["TimeTugSpike"])],
    targets: [
        .binaryTarget(name: "TimeTugSpikeFFI", path: "../build/TimeTugSpikeFFI.xcframework"),
        .target(name: "TimeTugSpike", dependencies: ["TimeTugSpikeFFI"], swiftSettings: [.swiftLanguageMode(.v5)]),
        .testTarget(name: "TimeTugSpikeTests", dependencies: ["TimeTugSpike"]),
    ]
)
