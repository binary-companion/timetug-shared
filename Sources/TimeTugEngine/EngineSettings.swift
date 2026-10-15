import Foundation
import TimeTugCore

/// Everything the host lets the engine read about the user's preferences.
public struct EngineSettings: Equatable, Sendable {
    public var takeover: TakeoverSettings
    public var inferenceEnabled: Bool
    public var systemCalendarEnabled: Bool

    public init(takeover: TakeoverSettings = .init(), inferenceEnabled: Bool = false, systemCalendarEnabled: Bool = true) {
        self.takeover = takeover
        self.inferenceEnabled = inferenceEnabled
        self.systemCalendarEnabled = systemCalendarEnabled
    }
}

/// Where the host keeps settings; the engine reads and writes through this seam.
public protocol SettingsStorage: Sendable {
    func load() -> EngineSettings
    func save(_ settings: EngineSettings)
}

/// A settings store held in memory, for tests and hosts without persistence.
public final class InMemorySettingsStorage: SettingsStorage, @unchecked Sendable {
    private let lock = NSLock()
    private var current: EngineSettings
    private var history: [EngineSettings] = []

    public init(_ initial: EngineSettings = EngineSettings()) { current = initial }

    /// Every value passed to `save`, oldest first.
    public var saved: [EngineSettings] { lock.withLock { history } }

    public func load() -> EngineSettings { lock.withLock { current } }

    public func save(_ settings: EngineSettings) {
        lock.withLock {
            current = settings
            history.append(settings)
        }
    }
}
