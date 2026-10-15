import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

/// Records the engine's `takeoverDue`, `takeoverWithdrawn` and `widgetSnapshot` events in arrival order, each
/// stamped with the manual clock's time (seconds after the clock's start) when it arrived. Conformance tests
/// assert the whole list, so a change in ordering or count fails. Subscribe before sending `.start`.
final class TimelineRecorder: @unchecked Sendable {
    enum Entry: Equatable, CustomStringConvertible {
        /// `id` is the request's `sourceEventID`; `late` is `TakeoverRequest.hasStarted`.
        case due(String, at: TimeInterval, late: Bool = false)
        /// `id` is the withdrawn event's `sourceEventID`.
        case withdrawn(String, at: TimeInterval)
        case widget(at: TimeInterval)

        var description: String {
            switch self {
            case .due(let id, let at, let late): "due(\(id)\(late ? ", late" : "")@\(Int(at)))"
            case .withdrawn(let id, let at): "withdrawn(\(id)@\(Int(at)))"
            case .widget(let at): "widget@\(Int(at))"
            }
        }
    }

    private let box = LockedBox<[Entry]>([])
    private let widgetBox = LockedBox<[WidgetSnapshot]>([])

    init(_ h: EngineHarness) async {
        let stream = await h.engine.events
        let clock = h.clock, origin = h.clock.now(), box = box, widgetBox = widgetBox
        Task {
            for await event in stream {
                let at = clock.now().timeIntervalSince(origin)
                switch event {
                case .takeoverDue(let request, _):
                    box.mutate { $0.append(.due(request.event.sourceEventID, at: at, late: request.hasStarted)) }
                case .takeoverWithdrawn(let id):
                    box.mutate { $0.append(.withdrawn(Self.sourceEventID(of: id), at: at)) }
                case .widgetSnapshot(let snapshot):
                    widgetBox.mutate { $0.append(snapshot) }
                    box.mutate { $0.append(.widget(at: at)) }
                case .joinRequested, .diagnostic:
                    break
                }
            }
        }
    }

    var entries: [Entry] { box.get() }
    /// Every widget snapshot, in arrival order.
    var widgets: [WidgetSnapshot] { widgetBox.get() }

    /// `TimeTugCalendarEvent.id` is "source/sourceEventID/startEpoch".
    private static func sourceEventID(of id: String) -> String {
        let parts = id.split(separator: "/")
        return parts.count >= 3 ? parts.dropFirst().dropLast().joined(separator: "/") : id
    }

    /// Waits (bounded, no wall-clock time) until `count` entries arrived, then lets stragglers land so an extra
    /// entry shows up in the assertion that follows.
    func waitFor(_ count: Int, _ h: EngineHarness, sourceLocation: SourceLocation = #_sourceLocation) async {
        await h.waitUntil("\(count) timeline entries (have \(entries))", sourceLocation: sourceLocation) {
            entries.count >= count
        }
        await h.settle()
    }
}
