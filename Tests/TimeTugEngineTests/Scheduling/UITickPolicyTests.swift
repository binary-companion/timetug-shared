import CalendarCore
import Foundation
import Testing
import TimeTugCore
@testable import TimeTugEngine

@Suite struct UITickPolicyTests {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)
    private var calendar: Calendar {
        var value = Calendar(identifier: .gregorian)
        value.timeZone = TimeZone(secondsFromGMT: 0)!
        return value
    }

    private func event(startAfter seconds: TimeInterval, length: TimeInterval = 1800) -> TimeTugCalendarEvent {
        let start = now.addingTimeInterval(seconds)
        return TimeTugCalendarEvent(event: CalendarCore.CalendarEvent(
            eventID: "e", calendarID: "c", title: "Meeting", start: start, end: start.addingTimeInterval(length)), sourceID: "s")
    }

    @Test func countdownUsesMinuteTransitionsThenSeconds() throws {
        let far = event(startAfter: 61)
        let wake = try #require(UITickPolicy.next(now: now, countdownVisible: true, nextEvent: far,
                                                  events: [], leadTime: 60, calendar: calendar))
        #expect(abs(wake.date.timeIntervalSince(now) - 1.02) < 0.01)
        #expect(!wake.rebuildAgenda)
        let boundary = event(startAfter: 60)
        let boundaryWake = try #require(UITickPolicy.next(now: now, countdownVisible: true, nextEvent: boundary,
                                                          events: [], leadTime: 60, calendar: calendar))
        #expect(abs(boundaryWake.date.timeIntervalSince(now) - 0.02) < 0.01)
        let near = event(startAfter: 59)
        let nearWake = try #require(UITickPolicy.next(now: now, countdownVisible: true, nextEvent: near,
                                                      events: [], leadTime: 60, calendar: calendar))
        #expect(abs(nearWake.date.timeIntervalSince(now) - 0.02) < 0.01)
    }

    @Test func iconOnlyIdleWaitsForDayBoundaryAndEventStartRebuilds() throws {
        let idle = try #require(UITickPolicy.next(now: now, countdownVisible: false, nextEvent: nil,
                                                  events: [], leadTime: 60, calendar: calendar))
        #expect(idle.date.timeIntervalSince(now) > 3600)
        #expect(idle.rebuildAgenda)
        let meeting = event(startAfter: 30)
        let wake = try #require(UITickPolicy.next(now: now, countdownVisible: false, nextEvent: nil,
                                                  events: [meeting], leadTime: 60, calendar: calendar))
        #expect(wake.date == meeting.start)
        #expect(wake.rebuildAgenda)
    }

    @Test func anIdleHourHasNoTickTriggeredStructuralRebuilds() {
        var cursor = now
        var rebuilds = 0
        let hourEnd = now.addingTimeInterval(3600)
        while let wake = UITickPolicy.next(now: cursor, countdownVisible: false, nextEvent: nil,
                                           events: [], leadTime: 60, calendar: calendar), wake.date < hourEnd {
            if wake.rebuildAgenda { rebuilds += 1 }
            cursor = wake.date
        }
        #expect(rebuilds == 0)
    }
}
