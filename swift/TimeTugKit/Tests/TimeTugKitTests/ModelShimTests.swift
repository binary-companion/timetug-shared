import Foundation
import Testing
import TimeTugKit

/// The shims that give the generated model its 1.x spellings, defaults and formulas (phase 6, items M1 to M4 and M6 to M8).
@Suite struct ModelShimTests {
    private let utc = TimeZone(identifier: "UTC")!

    private func event(
        id: String = "e1", calendar: String = "c1", title: String = "Planning", start: TimeInterval = 1_794_654_000,
        attendees: [Attendee] = [], participation: Participation? = nil
    ) -> CalendarEvent {
        CalendarEvent(
            eventID: id, calendarID: calendar, title: title, start: Date(timeIntervalSince1970: start),
            end: Date(timeIntervalSince1970: start + 1800), attendees: attendees, participation: participation)
    }

    // MARK: M1 TimeTugCalendarEvent

    @Test func aTimeTugEventStartsWithTheDerivedFieldsOf1x() {
        let me = Attendee(email: "Me@Example.com", response: .tentative, isSelf: true)
        let other = Attendee(email: "you@example.com")
        let link = ConferenceInfo(url: URL(string: "https://meet.example.com/a")!, provider: .meet)
        var base = event(attendees: [me, other, Attendee(email: "x@example.com")])
        base.conferences = [link]
        let value = TimeTugCalendarEvent(event: base, sourceID: "src")
        #expect(value.sourceId == "src" && value.sourceID == "src")
        #expect(value.conferences == [link])
        #expect(value.conferenceURL == link.url)
        #expect(value.otherAttendeeCount == 2)
        #expect(value.responseStatus == .tentative)   // the attendee marked as self
        #expect(value.additionalCalendarKeys.isEmpty && value.mergedMembers.isEmpty)
        #expect(value.mergeProvenance == nil && value.displayStart == nil)
        #expect(value.calendarService == nil && value.calendarProvider == nil)

        base.participation = .invited(.accepted)
        #expect(TimeTugCalendarEvent(event: base, sourceID: "src").responseStatus == .accepted)   // participation wins
        #expect(TimeTugCalendarEvent(event: event(), sourceID: "src").responseStatus == nil)
        #expect(TimeTugCalendarEvent(event: event(), sourceID: "src").conferenceURL == nil)
    }

    @Test func aTimeTugEventForwardsTheFieldsOfItsEvent() {
        var value = TimeTugCalendarEvent(event: event(), sourceID: "src")
        #expect(value.title == "Planning" && value.calendarID == "c1")
        #expect(value.start == Date(timeIntervalSince1970: 1_794_654_000) && value.end == value.start.addingTimeInterval(1800))
        #expect(!value.isAllDay && value.timeZone == utc)
        #expect(value.location == nil && value.notes == nil && value.url == nil)
        value.title = "Renamed"
        value.location = "Room 4"
        value.notes = "n"
        value.url = URL(string: "https://example.com")
        value.isAllDay = true
        value.timeZone = TimeZone(identifier: "Europe/Berlin")!
        value.calendarID = "c2"
        value.start = Date(timeIntervalSince1970: 10)
        value.end = Date(timeIntervalSince1970: 20)
        #expect(value.event.title == "Renamed" && value.event.location == "Room 4" && value.event.notes == "n")
        #expect(value.event.url == URL(string: "https://example.com") && value.event.isAllDay)
        #expect(value.event.timeZone.identifier == "Europe/Berlin" && value.event.calendarId == "c2")
        #expect(value.event.start == Date(timeIntervalSince1970: 10) && value.event.end == Date(timeIntervalSince1970: 20))
    }

    @Test func theKeysFollowTheFormulasOfTheEngine() {
        var value = TimeTugCalendarEvent(event: event(id: "ev", calendar: "cal", title: "Weekly SYNC"), sourceID: "google-1")
        #expect(value.id == "google-1/ev/1794654000")
        #expect(value.calendarKey == "google-1/cal")
        #expect(value.contentKey == "weekly sync|1794654000|1794655800")
        #expect(value.shownStart == value.start)
        value.displayStart = Date(timeIntervalSince1970: 1_794_650_000)
        #expect(value.shownStart == Date(timeIntervalSince1970: 1_794_650_000))
        #expect(value.allCalendarKeys == ["google-1/cal"])
        value.additionalCalendarKeys = ["eventkit/other"]
        #expect(value.allCalendarKeys == ["google-1/cal", "eventkit/other"])
        let identifiable: any Identifiable = value
        #expect(identifiable.id as? String == value.id)
        // Whole seconds, truncated toward zero, as the engine does.
        value.start = Date(timeIntervalSince1970: 1_794_654_000.9)
        #expect(value.id == "google-1/ev/1794654000")
    }

    // MARK: M2 CalendarInfo

    @Test func aCalendarInfoHasItsKeyAndNormalisesTheColour() {
        let info = CalendarInfo(sourceID: "eventkit", calendarID: "c1", title: "Work", colorHex: " #abc ")
        #expect(info.key == "eventkit/c1" && info.id == "eventkit/c1")
        #expect(CalendarInfo.key(sourceID: "s", calendarID: "c") == "s/c")
        #expect(info.colorHex == "#AABBCC")
        #expect(info.kind == .standard && info.accountName == nil && info.service == nil && info.provider == nil)
        #expect(info.sourceID == "eventkit" && info.calendarID == "c1")
        let full = CalendarInfo(
            sourceID: "g", calendarID: "c", title: "T", accountName: "me@example.com", colorHex: "nonsense", kind: .birthdays,
            service: .google, provider: .google)
        #expect(full.colorHex == nil && full.kind == .birthdays && full.service == .google && full.provider == .google)
        #expect(CalendarInfo.normalizedHex("123456") == "#123456" && CalendarInfo.normalizedHex("#12345") == nil)
    }

    // MARK: M3 TakeoverSettings

    @Test func theDefaultsAreThoseOf1x() {
        let settings = TakeoverSettings()
        #expect(settings.leadTime == 60 && settings.enabled && settings.skipAllDayEvents)
        #expect(!settings.requireConferenceLink && !settings.requireOtherAttendees)
        #expect(settings.takeoverCalendarKeys.isEmpty && settings.hiddenCalendarKeys.isEmpty)
    }

    /// What `JSONEncoder` (sorted keys) wrote for the 1.x `TakeoverSettings` of TimeTugCore, produced by compiling that file.
    private let writtenBy1x = """
        {"enabled":false,"hiddenCalendarKeys":["google-1\\/holidays"],"leadTime":180,"requireConferenceLink":true,\
        "requireOtherAttendees":true,"skipAllDayEvents":false,"takeoverCalendarKeys":["eventkit\\/work"]}
        """

    @Test func theJSONTheLastSwiftReleaseWroteDecodesToTheSameValues() throws {
        let decoded = try JSONDecoder().decode(TakeoverSettings.self, from: Data(writtenBy1x.utf8))
        var expected = TakeoverSettings()
        expected.leadTime = 180
        expected.takeoverCalendarKeys = ["eventkit/work"]
        expected.hiddenCalendarKeys = ["google-1/holidays"]
        expected.requireConferenceLink = true
        expected.requireOtherAttendees = true
        expected.skipAllDayEvents = false
        expected.enabled = false
        #expect(decoded == expected)
        let defaults = #"{"enabled":true,"hiddenCalendarKeys":[],"leadTime":60,"requireConferenceLink":false,"requireOtherAttendees":false,"skipAllDayEvents":true,"takeoverCalendarKeys":[]}"#
        #expect(try JSONDecoder().decode(TakeoverSettings.self, from: Data(defaults.utf8)) == TakeoverSettings())
    }

    @Test func theEncodedFormIsTheOneThe1xDecoderReads() throws {
        var settings = TakeoverSettings()
        settings.leadTime = 180
        settings.takeoverCalendarKeys = ["eventkit/work"]
        settings.hiddenCalendarKeys = ["google-1/holidays"]
        settings.requireConferenceLink = true
        settings.requireOtherAttendees = true
        settings.skipAllDayEvents = false
        settings.enabled = false
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        // The same bytes the 1.x release wrote, so a downgrade keeps reading what an upgrade wrote.
        #expect(String(decoding: try encoder.encode(settings), as: UTF8.self) == writtenBy1x)
        #expect(try JSONDecoder().decode(TakeoverSettings.self, from: try JSONEncoder().encode(settings)) == settings)
    }

    @Test func missingKeysTakeTheirDefaultsAndLegacyKeysCarryTheChoiceForward() throws {
        // `fixtures/engine/settings-legacy.json` of the repository: only keys of earlier versions.
        let legacy = try JSONDecoder().decode(
            TakeoverSettings.self, from: Data(#"{"disabled":true,"skipSoloEvents":true,"leadTime":120}"#.utf8))
        #expect(!legacy.enabled && legacy.requireOtherAttendees && legacy.leadTime == 120 && legacy.skipAllDayEvents)
        #expect(try JSONDecoder().decode(TakeoverSettings.self, from: Data("{}".utf8)) == TakeoverSettings())
        // A current key beats the legacy one.
        let both = try JSONDecoder().decode(
            TakeoverSettings.self, from: Data(#"{"disabled":true,"enabled":true,"skipSoloEvents":true,"requireOtherAttendees":false}"#.utf8))
        #expect(both.enabled && !both.requireOtherAttendees)
        // A calendar that is hidden never tugs.
        let conflicting = try JSONDecoder().decode(
            TakeoverSettings.self, from: Data(#"{"takeoverCalendarKeys":["a/1","a/2"],"hiddenCalendarKeys":["a/2"]}"#.utf8))
        #expect(conflicting.takeoverCalendarKeys == ["a/1"] && conflicting.hiddenCalendarKeys == ["a/2"])
        #expect(throws: DecodingError.self) { try JSONDecoder().decode(TakeoverSettings.self, from: Data(#"{"leadTime":"soon"}"#.utf8)) }
    }

    @Test func theMutatorsKeepVisibilityAndTakeoverConsistent() {
        var settings = TakeoverSettings()
        settings.setShownInList(false, forCalendar: "a/1")
        #expect(!settings.isShownInList("a/1") && settings.isShownInList("a/2"))
        settings.setTakeover(true, forCalendar: "a/1")   // enabling shows it again
        #expect(settings.isShownInList("a/1") && settings.takeoverCalendarKeys == ["a/1"])
        settings.setTakeover(false, forCalendar: "a/1")  // disabling leaves visibility alone
        #expect(settings.isShownInList("a/1") && settings.takeoverCalendarKeys.isEmpty)
        settings.setTakeover(true, forCalendars: ["a/1", "b/1", "b/2"])
        #expect(settings.takeoverCalendarKeys == ["a/1", "b/1", "b/2"])
        settings.setShownInList(false, forCalendar: "b/1")   // hiding turns takeover off
        #expect(settings.takeoverCalendarKeys == ["a/1", "b/2"] && settings.hiddenCalendarKeys == ["b/1"])
        settings.setShownInList(true, forCalendar: "b/1")    // showing leaves takeover alone
        #expect(settings.takeoverCalendarKeys == ["a/1", "b/2"] && settings.hiddenCalendarKeys.isEmpty)
        settings.setShownInList(false, forCalendar: "b/3")
        settings.removeCalendars(forSourceID: "b")
        #expect(settings.takeoverCalendarKeys == ["a/1"] && settings.hiddenCalendarKeys.isEmpty)
    }

    // MARK: M4 Connection

    @Test func aConnectionHasItsSourceIdAndAnEmptyConfigByDefault() {
        let connection = Connection(kindID: "google", connectionID: "ABC", displayName: "Me")
        #expect(connection.sourceID == "google-ABC")
        #expect(connection.config.isEmpty && connection.id == "ABC")
        #expect(connection.kindId == "google" && connection.connectionId == "ABC")
        #expect(Connection(kindID: "k", connectionID: "c", displayName: "n", config: ["a": "b"]).config == ["a": "b"])
    }

    // MARK: M6 CalendarService and CalendarProvider

    @Test func theServiceAndProviderValuesHaveTheirRawStringsOf1x() {
        #expect(CalendarService.eventKit.rawValue == "eventkit")
        #expect(CalendarService.google.rawValue == "google")
        #expect(CalendarService.microsoft.rawValue == "microsoft")
        #expect(CalendarService.calDAV.rawValue == "caldav")
        #expect(CalendarService.iCalSubscription.rawValue == "icalsub")
        #expect(CalendarProvider.iCloud.rawValue == "icloud")
        #expect(CalendarProvider.google.rawValue == "google")
        #expect(CalendarProvider.microsoft.rawValue == "microsoft")
        #expect(CalendarProvider.local.rawValue == "local")
        #expect(CalendarProvider.subscription.rawValue == "subscription")
        #expect(CalendarProvider.calDAV.rawValue == "caldav")
        func raw<T: RawRepresentable>(_ value: T) -> T.RawValue { value.rawValue }
        #expect(raw(CalendarService.eventKit) == "eventkit")
        #expect(CalendarService(rawValue: "eventkit") == .eventKit)
        #expect(CalendarProvider(rawValue: "custom").rawValue == "custom")
    }

    // MARK: M7 recurrence names

    @Test func theNestedNamesOfRecurrenceRuleResolve() {
        let day: RecurrenceRule.Weekday = .tuesday
        let frequency: RecurrenceRule.Frequency = .monthly
        let occurrence = RecurrenceRule.WeekdayOccurrence(day, ordinal: 2)
        #expect(occurrence == WeekdayOccurrence(weekday: .tuesday, ordinal: 2))
        #expect(frequency == Frequency.monthly)
        #expect(RecurrenceRule.WeekdayOccurrence(.friday).ordinal == nil)
        #expect(WeekdayOccurrence(.friday, ordinal: -1).ordinal == -1)
        let end: RecurrenceRule.End = .count(3)
        #expect(end == RuleEnd.count(3))
    }

    // MARK: M8 initialisers

    @Test func anEventHasTheDefaultsOf1x() {
        let value = CalendarEvent(
            eventID: "e", calendarID: "c", title: "T", start: Date(timeIntervalSince1970: 0), end: Date(timeIntervalSince1970: 60))
        #expect(value.eventId == "e" && value.calendarId == "c" && value.sourceId == nil)
        #expect(value.timeZone == utc && !value.isAllDay && value.status == .confirmed)
        #expect(value.uid == nil && value.uidScope == nil && value.notes == nil && value.location == nil)
        #expect(value.availability == nil && value.visibility == nil && value.kind == nil && value.series == nil)
        #expect(value.attendees.isEmpty && value.organizer == nil && value.conferences.isEmpty && value.reminders == nil)
        #expect(value.url == nil && value.version == nil && value.lastModified == nil && value.created == nil)
        #expect(value.participation == nil)
        let stamped = CalendarEvent(
            eventID: "e", uid: "u", uidScope: .global, calendarID: "c", title: "T", start: Date(timeIntervalSince1970: 0),
            end: Date(timeIntervalSince1970: 60), isAllDay: true, series: .notRecurring, sourceID: "src")
        #expect(stamped.uid == "u" && stamped.uidScope == .global && stamped.isAllDay && stamped.sourceID == "src")
    }

    @Test func aDescriptorHasTheDefaultsOf1xAndNormalisesTheColour() {
        let descriptor = CalendarDescriptor(id: "c", title: "Work", service: .eventKit, colorHex: "ff8800")
        #expect(descriptor.colorHex == "#FF8800")
        #expect(descriptor.permissions == CalendarPermissions())
        #expect(descriptor.isDefault == nil && descriptor.timeZone == nil && descriptor.accountName == nil)
        #expect(descriptor.kind == .standard && descriptor.defaultReminders == nil)
        #expect(descriptor.provider == nil && descriptor.supportedAvailabilities == nil)
        #expect(descriptor.service == .eventKit)
        #expect(CalendarDescriptor(id: "c", title: "T", service: .google, colorHex: "zzz").colorHex == nil)
        let full = CalendarDescriptor(
            id: "c", title: "T", service: .google, permissions: CalendarPermissions(canEdit: true), isDefault: true,
            timeZone: utc, accountName: "me", kind: .subscribed, defaultReminders: [Reminder(minutesBefore: 5)],
            provider: .google, supportedAvailabilities: [.busy])
        #expect(full.isDefault == true && full.provider == .google && full.supportedAvailabilities == [.busy])
        #expect(full.accessRole == nil)   // can edit, and the source does not say whether it can share
    }

    @Test func permissionsDefaultToReadOnlyWithDetails() {
        #expect(CalendarPermissions() == CalendarPermissions(canViewDetails: true, canEdit: false, canShare: nil, canViewPrivate: nil))
        #expect(CalendarPermissions(canViewDetails: true, canEdit: true).canEdit)
        #expect(CalendarPermissions(canEdit: true, canShare: true).accessRole == .owner)
        // Naming every field still reaches the generated initialiser.
        let all = CalendarPermissions(canViewDetails: false, canEdit: true, canShare: false, canViewPrivate: true)
        #expect(!all.canViewDetails && all.canEdit && all.canShare == false && all.canViewPrivate == true)
    }

    @Test func capabilitiesDefaultToNothing() {
        let none = SourceCapabilities()
        #expect(!none.canWrite && !none.canEditAttendees && !none.canRespondToInvite && none.providedFields.isEmpty)
        #expect(none.syncKind == .none && !none.supportsPush && none.writableFields.isEmpty)
        #expect(!none.controlsNotifications && none.recurrenceScopes.isEmpty)
        let some = SourceCapabilities(canWrite: true, writableFields: [.title, .timing], recurrenceScopes: [.thisInstance])
        #expect(some.canWrite && some.writableFields == [.title, .timing] && some.recurrenceScopes == [.thisInstance])
        let all = SourceCapabilities(
            canWrite: true, canEditAttendees: true, canRespondToInvite: true, providedFields: [], syncKind: .none,
            supportsPush: true, writableFields: [], controlsNotifications: true, recurrenceScopes: [])
        #expect(all.supportsPush && all.controlsNotifications)
    }

    @Test func aReminderKeepsTheTypeSpelling() {
        let trigger = ReminderTrigger.relative(offset: -600, to: .start)
        let reminder = Reminder(trigger: trigger)
        #expect(reminder.type == .display && reminder.kind == .display)
        #expect(reminder.repeatCount == 0 && reminder.repeatInterval == nil && reminder.isCalendarDefault == nil)
        #expect(reminder.minutesBefore == 10)
        let email = Reminder(trigger: trigger, type: .email(address: "me@example.com"), repeatCount: 2, repeatInterval: 60, isCalendarDefault: true)
        #expect(email.type == .email(address: "me@example.com") && email.repeatCount == 2 && email.repeatInterval == 60)
        #expect(Reminder.before(minutes: 15) == Reminder(minutesBefore: 15))
        #expect(Reminder.before(minutes: 5, type: .audio(soundName: nil), isCalendarDefault: false).isCalendarDefault == false)
    }

    @Test func anAttendeeNormalisesItsAddressAndDefaultsToRequired() {
        let attendee = Attendee(name: "Ann", email: "  Ann@Example.COM ")
        #expect(attendee.email == "ann@example.com" && attendee.role == .required && attendee.response == .needsAction)
        #expect(!attendee.isSelf && !attendee.isOrganizer && attendee.name == "Ann")
        #expect(Attendee(email: "   ").email == nil)
        #expect(Attendee().email == nil)
        #expect(Attendee(isSelf: true).isSelf)
    }

    @Test func aConferenceDefaultsToAStructuredOrigin() {
        let url = URL(string: "https://zoom.us/j/1")!
        #expect(ConferenceInfo(url: url, provider: .zoom).origin == .structured)
        #expect(ConferenceInfo(url: url, provider: .zoom, origin: .notes).origin == .notes)
    }

    // MARK: Connector diagnostics

    @Test func aSinkThatOnlyReadsEngineLinesStillConforms() {
        final class LinesOnly: DiagnosticSink {
            let lines = Locked<[EngineDiagnostic]>([])
            func log(diagnostic: EngineDiagnostic) { lines.update { $0.append(diagnostic) } }
        }
        let sink = LinesOnly()
        let event = DiagnosticEvent(level: .info, category: "icalsub", name: "n", fields: [], date: Date(timeIntervalSince1970: 0))
        sink.record(event: event)   // the default drops it
        sink.log(diagnostic: EngineDiagnostic(level: .info, category: "c", message: "m"))
        #expect(sink.lines.get().count == 1)
    }
}
