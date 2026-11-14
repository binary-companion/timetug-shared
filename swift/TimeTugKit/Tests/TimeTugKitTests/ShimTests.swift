import Foundation
import Testing
import TimeTugKit

@Suite struct ShimTests {
    private func event() -> CalendarEvent {
        CalendarEvent(
            eventId: "e1", uid: nil, uidScope: nil, calendarId: "c1", title: "T", notes: nil, location: nil, start: clockStart,
            end: clockStart.addingTimeInterval(60), timeZone: TimeZone(identifier: "UTC")!, isAllDay: false, status: .confirmed,
            availability: nil, visibility: nil, kind: nil, series: .occurrence(seriesId: "s1", originalStart: clockStart),
            attendees: [], organizer: nil, conferences: [], reminders: nil, url: nil, version: nil, lastModified: nil,
            created: nil, participation: .invited(.accepted), sourceId: "src")
    }

    @Test func identifiableAndTheComputedPropertiesOfTheOldModel() {
        let event = event()
        #expect(event.id == "c1/e1")
        #expect(event.eventID == "e1" && event.calendarID == "c1" && event.sourceID == "src")
        #expect(event.seriesID == "s1")
        #expect(event.originalStart == clockStart)
        #expect(event.myResponse == .accepted)
        #expect(event.conference == nil)
        var changed = event
        changed.eventID = "e2"
        #expect(changed.eventId == "e2")
    }

    @Test func calendarDatesAreComparable() {
        let a = CalendarDate(year: 2026, month: 11, day: 14)
        #expect(a < CalendarDate(year: 2026, month: 11, day: 15))
        #expect(a < CalendarDate(year: 2027, month: 1, day: 1))
        #expect(!(a < a))
    }

    @Test func remindersKeepTheirMinutesBefore() {
        #expect(Reminder(minutesBefore: 15).minutesBefore == 15)
        let after = Reminder(trigger: .relative(offset: 60, to: .start), kind: .display, repeatCount: 0, repeatInterval: nil, isCalendarDefault: nil)
        #expect(after.minutesBefore == nil)
    }

    @Test func accessRoleFollowsThePermissions() {
        #expect(CalendarPermissions(canViewDetails: false, canEdit: false, canShare: nil, canViewPrivate: nil).accessRole == .freeBusyReader)
        #expect(CalendarPermissions(canViewDetails: true, canEdit: false, canShare: nil, canViewPrivate: nil).accessRole == .reader)
        #expect(CalendarPermissions(canViewDetails: true, canEdit: true, canShare: true, canViewPrivate: nil).accessRole == .owner)
        #expect(CalendarPermissions(canViewDetails: true, canEdit: true, canShare: false, canViewPrivate: nil).accessRole == .writer)
        #expect(CalendarPermissions(canViewDetails: true, canEdit: true, canShare: nil, canViewPrivate: nil).accessRole == nil)
    }

    @Test func fieldUpdateConvertsToTheConcreteMirrors() {
        #expect(FieldUpdate<String>.set("x").mirror == FieldUpdateString.set("x"))
        #expect(FieldUpdate(FieldUpdateString.clear) == .clear)
        #expect(FieldUpdate<[Reminder]>.keep.mirror == FieldUpdateReminders.keep)
        #expect(FieldUpdate<RecurrenceRule>.clear.mirror == FieldUpdateRecurrenceRule.clear)
    }

    @Test func applySettingsDiffsIntoAPatch() {
        let old = FakeSettings().load()
        let command = EngineCommand.applySettings(from: old) { $0.takeover.leadTime = 120; $0.inferenceEnabled = true }
        guard case .applySettings(let patch) = command else { Issue.record("not applySettings"); return }
        #expect(patch.leadTime == 120)
        #expect(patch.inferenceEnabled == true)
        #expect(patch.enabled == nil && patch.takeoverCalendarKeys == nil)
        #expect(SettingsPatch(from: old, to: old).isEmpty)
    }

    @Test func theNameDifferencesAreAliased() {
        #expect(UIDScope.global == UidScope.global)
        let rule: RecurrenceRule.End = .never
        #expect(rule == RuleEnd.never)
    }

    @Test func errorsAreCatchableWithTheirOldSpelling() {
        func fail() throws { throw WriteError.notFound }
        #expect(throws: WriteError.self) { try fail() }
        let error: Error = SourceError.authExpired
        #expect(error is SourceError)
    }

    @Test func aZoneTheEngineDoesNotKnowIsATypedErrorNotACrash() {
        let engineless = Host()
        var config = engineless.config(directory: temporaryDirectory("badzone"))
        config.timeZone = "Mars/Base"
        #expect(throws: FfiError.self) {
            _ = try Engine(config: config, host: engineless.interfaces, observer: engineless.observer)
        }
    }
}
