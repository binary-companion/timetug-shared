import Foundation
import Testing
@testable import TimeTugCore

private let now = date("2026-09-18T12:00:00Z")

private func event(_ id: String, _ start: String, minutes: Int = 30, allDay: Bool = false) -> WidgetEvent {
    let s = date(start)
    return WidgetEvent(id: id, title: id, start: s, end: s.addingTimeInterval(TimeInterval(minutes * 60)), isAllDay: allDay)
}

private func snapshot(_ events: [WidgetEvent]) -> WidgetSnapshot { WidgetSnapshot(generatedAt: now, events: events) }

@Test func changeDatesIncludeFutureStartsEndsAndMidnights() {
    let s = snapshot([event("a", "2026-09-18T13:00:00Z"), event("b", "2026-09-18T11:00:00Z")])
    let dates = WidgetTimeline.changeDates(snapshot: s, now: now, calendar: utcCalendar, limit: 20)
    #expect(dates.first == date("2026-09-18T13:00:00Z"))
    #expect(dates.contains(date("2026-09-18T13:30:00Z")))
    #expect(dates.contains(date("2026-09-19T00:00:00Z")))
    #expect(!dates.contains(date("2026-09-18T11:00:00Z")))
    #expect(!dates.contains(date("2026-09-18T11:30:00Z")))
    #expect(dates == dates.sorted())
    #expect(Set(dates).count == dates.count)
}

@Test func changeDatesRespectLimit() {
    let s = snapshot([event("a", "2026-09-18T13:00:00Z"), event("b", "2026-09-18T14:00:00Z")])
    #expect(WidgetTimeline.changeDates(snapshot: s, now: now, calendar: utcCalendar, limit: 2).count == 2)
}

@Test func changeDatesForEmptySnapshotAreJustMidnights() {
    let dates = WidgetTimeline.changeDates(snapshot: snapshot([]), now: now, calendar: utcCalendar, limit: 10)
    #expect(dates == [date("2026-09-19T00:00:00Z"), date("2026-09-20T00:00:00Z")])
}

@Test func changeDatesIgnoreAllDayEventBoundariesButKeepMidnights() {
    let s = snapshot([event("d", "2026-09-18T00:00:00Z", minutes: 24 * 60, allDay: true)])
    let dates = WidgetTimeline.changeDates(snapshot: s, now: now, calendar: utcCalendar, limit: 10)
    #expect(dates == [date("2026-09-19T00:00:00Z"), date("2026-09-20T00:00:00Z")])
}

@Test func nextUpFindsCurrentAndUpcoming() {
    let s = snapshot([event("now", "2026-09-18T11:45:00Z"), event("n1", "2026-09-18T13:00:00Z"),
                      event("n2", "2026-09-18T14:00:00Z"), event("n3", "2026-09-19T09:00:00Z"),
                      event("past", "2026-09-18T09:00:00Z")])
    let result = WidgetTimeline.nextUp(snapshot: s, now: now, upcomingLimit: 2)
    #expect(result.current?.id == "now")
    #expect(result.upcoming.map(\.id) == ["n1", "n2"])
}

@Test func nextUpIgnoresAllDayEvents() {
    let s = snapshot([event("d", "2026-09-18T00:00:00Z", minutes: 24 * 60, allDay: true)])
    #expect(WidgetTimeline.nextUp(snapshot: s, now: now, upcomingLimit: 3) == .init(current: nil, upcoming: []))
}

@Test func nextUpPicksEarliestStartedWhenMeetingsOverlap() {
    let s = snapshot([event("late", "2026-09-18T11:50:00Z"), event("early", "2026-09-18T11:40:00Z")])
    #expect(WidgetTimeline.nextUp(snapshot: s, now: now, upcomingLimit: 3).current?.id == "early")
}

@Test func todayCoversOnlyTodayAndAssignsStates() {
    let s = snapshot([event("past", "2026-09-18T09:00:00Z"), event("cur", "2026-09-18T11:45:00Z"),
                      event("up", "2026-09-18T15:00:00Z"), event("tomorrow", "2026-09-19T09:00:00Z")])
    let rows = WidgetTimeline.today(snapshot: s, now: now, calendar: utcCalendar)
    #expect(rows.map(\.event.id) == ["past", "cur", "up"])
    #expect(rows.map(\.state) == [.past, .current, .upcoming])
}

@Test func todayKeepsAllDayEventsAsCurrent() {
    let s = snapshot([event("d", "2026-09-18T00:00:00Z", minutes: 24 * 60, allDay: true)])
    let states = WidgetTimeline.today(snapshot: s, now: now, calendar: utcCalendar).map { $0.state }
    #expect(states == [.current])
}
