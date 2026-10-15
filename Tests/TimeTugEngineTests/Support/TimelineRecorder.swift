import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

/// Records the engine's `takeoverDue`, `takeoverWithdrawn` and `widgetSnapshot` events in arrival order. A takeover
/// entry is stamped with the manual clock's time (seconds after the clock's start) when it arrived; a widget entry
/// with its snapshot's `generatedAt`, so a lagging consumer never mis-stamps it. Conformance tests assert the
/// takeover entries exactly and the widget entries only structurally: how many partial publications a refresh
/// makes depends on how the sources' answers interleave. Subscribe before sending `.start`.
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

    private let received = LockedBox(0)

    init(_ h: EngineHarness) async {
        let stream = await h.engine.events
        // Events emitted from now on reach this recorder; `settle()` waits until it received all of them.
        let activity = h.engine.activity, offset = activity.eventsEmitted, received = received
        h.drainChecks.mutate { $0.append { received.get() == activity.eventsEmitted - offset } }
        let clock = h.clock, origin = h.clock.now(), box = box, widgetBox = widgetBox
        Task {
            for await event in stream {
                defer { received.mutate { $0 += 1 } }
                let at = clock.now().timeIntervalSince(origin)
                switch event {
                case .takeoverDue(let request, _):
                    box.mutate { $0.append(.due(request.event.sourceEventID, at: at, late: request.hasStarted)) }
                case .takeoverWithdrawn(let id):
                    box.mutate { $0.append(.withdrawn(Self.sourceEventID(of: id), at: at)) }
                case .widgetSnapshot(let snapshot):
                    widgetBox.mutate { $0.append(snapshot) }
                    box.mutate { $0.append(.widget(at: snapshot.generatedAt.timeIntervalSince(origin))) }
                case .joinRequested, .diagnostic:
                    break
                }
            }
        }
    }

    var entries: [Entry] { box.get() }
    /// The `takeoverDue` and `takeoverWithdrawn` entries, in order.
    var takeovers: [Entry] { entries.filter { if case .widget = $0 { false } else { true } } }
    /// The times at which at least one widget snapshot was generated.
    var widgetTimes: Set<TimeInterval> {
        Set(entries.compactMap { if case .widget(let at) = $0 { at } else { nil } })
    }
    /// Every widget snapshot, in arrival order.
    var widgets: [WidgetSnapshot] { widgetBox.get() }

    /// `TimeTugCalendarEvent.id` is "source/sourceEventID/startEpoch".
    private static func sourceEventID(of id: String) -> String {
        let parts = id.split(separator: "/")
        return parts.count >= 3 ? parts.dropFirst().dropLast().joined(separator: "/") : id
    }

    /// Waits until `count` takeover entries arrived, then (with `settle`) until the engine is quiescent, so an extra
    /// one shows up in the assertion that follows. Pass `settle: false` while the test holds a refresh at a gate.
    func waitForTakeovers(_ count: Int, _ h: EngineHarness, settle: Bool = true,
                          sourceLocation: SourceLocation = #_sourceLocation) async {
        await h.waitUntil("\(count) takeover entries (have \(takeovers))", sourceLocation: sourceLocation) {
            takeovers.count >= count
        }
        if settle { await h.settle(sourceLocation: sourceLocation) }
    }

    /// Waits until a widget snapshot generated at `seconds` arrived (a refresh at that time published), then
    /// lets the rest of that refresh land.
    func waitForWidget(at seconds: TimeInterval, _ h: EngineHarness, sourceLocation: SourceLocation = #_sourceLocation) async {
        await h.waitUntil("a widget snapshot at \(Int(seconds)) (have \(widgetTimes.sorted()))", sourceLocation: sourceLocation) {
            widgetTimes.contains(seconds)
        }
        await h.settle(sourceLocation: sourceLocation)
    }
}
