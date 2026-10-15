import Foundation

public struct DiagnosticEvent: Sendable, Equatable {
    public enum Level: String, Sendable { case info, warning, error }
    public var level: Level
    public var category: String
    /// Never contains event titles or other calendar content.
    public var message: String
    public init(level: Level, category: String, message: String) {
        self.level = level; self.category = category; self.message = message
    }
}

public typealias DiagnosticSink = @Sendable (DiagnosticEvent) -> Void
