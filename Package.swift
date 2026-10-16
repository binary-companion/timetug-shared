// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "timetug-shared",
    platforms: [.macOS(.v14)],
    products: [
        .library(name: "TimeTugCore", targets: ["TimeTugCore"]),
        .library(name: "CalendarBridge", targets: ["CalendarBridge"]),
        .library(name: "TimeTugEngine", targets: ["TimeTugEngine"]),
        .library(name: "TimeTugEngineProtocol", targets: ["TimeTugEngineProtocol"]),
        // The C interface: a native library exporting the four tt_engine_* functions of include/timetug_engine.h.
        .library(name: "TimeTugEngineC", type: .dynamic, targets: ["TimeTugEngineC"]),
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
        .target(name: "TimeTugEngineProtocol", dependencies: [
            "TimeTugEngine", "TimeTugCore", "CalendarBridge", .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
        .target(name: "TimeTugEngineC", dependencies: [
            "TimeTugEngineProtocol", "TimeTugEngine", "TimeTugCore",
            .product(name: "CalendarCore", package: "calendar-connectors"),
            .product(name: "GoogleCalendar", package: "calendar-connectors"),
            .product(name: "MicrosoftCalendar", package: "calendar-connectors"),
            .product(name: "CalDAVCalendar", package: "calendar-connectors"),
            .product(name: "ICalSubscription", package: "calendar-connectors"),
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
        ], resources: [.copy("Fixtures")]),
        // Conformance/ lives at the package root and is found through #filePath, not SwiftPM resources.
        .testTarget(name: "TimeTugEngineProtocolTests", dependencies: [
            "TimeTugEngineProtocol", "TimeTugEngine", "TimeTugCore", .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
        // Drives the C functions in-process; the scenario files under Conformance/ are found through #filePath.
        .testTarget(name: "TimeTugEngineCTests", dependencies: [
            "TimeTugEngineC", "TimeTugEngineProtocol", "TimeTugEngine", "TimeTugCore",
            .product(name: "CalendarCore", package: "calendar-connectors"),
        ]),
    ]
)
