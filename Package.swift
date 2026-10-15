// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "timetug-shared",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "TimeTugCore", targets: ["TimeTugCore"]),
        .library(name: "CalendarBridge", targets: ["CalendarBridge"]),
        .library(name: "TimeTugEngine", targets: ["TimeTugEngine"]),
    ],
    dependencies: [.package(url: "https://github.com/binary-companion/calendar-connectors", from: "1.0.0")],
    targets: [
        .target(name: "TimeTugCore", dependencies: [.product(name: "CalendarCore", package: "calendar-connectors")]),
        .target(name: "CalendarBridge", dependencies: [
            "TimeTugCore",
            .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
        .target(name: "TimeTugEngine", dependencies: [
            "TimeTugCore", "CalendarBridge", .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
        .executableTarget(name: "DedupBenchmark", dependencies: [
            "TimeTugCore", .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
        .testTarget(name: "TimeTugCoreTests", dependencies: [
            "TimeTugCore", .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
        .testTarget(name: "CalendarBridgeTests", dependencies: [
            "CalendarBridge", "TimeTugCore", .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
        .testTarget(name: "TimeTugEngineTests", dependencies: [
            "TimeTugEngine", "TimeTugCore", "CalendarBridge", .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
    ]
)
