import CalendarCore
import Foundation
@testable import TimeTugCore

// The event shape shared by the dedup, takeover, agenda and widget areas ("EventSpec"). An event vector input is
// built from these fields, and the exporter builds the Swift value from the same object, so the input alone
// determines the answer.
//
//   required: eventID, sourceID, calendarID, title (strings); start, end (Unix ms)
//   optional: timeZone (IANA id, default "UTC"), isAllDay (default false; all-day start/end are the canonical
//             midnights of the event's own zone), uid, uidScope ("global" | "provider"), notes, location, url,
//             attendees [{name?, email?, isSelf?, response?, role?}], organizer {name?, email?, isSelf?},
//             conferences [{url, provider, origin}] (provider and origin are the raw values of ConferenceProvider and
//             ConferenceOrigin), myResponse, calendarService, calendarProvider
//   Core state, overriding what `TimeTugCalendarEvent.init` derives from the fields above:
//             otherAttendeeCount (Int), responseStatus (string, or null for "the provider does not say"),
//             additionalCalendarKeys [String], displayStart (ms),
//             mergedMembers [{title, calendarKey, contentKey, details, start, end}],
//             mergeProvenance ("rule" | "userConfirmed" | {"inference": {"engineID", "engineName"}})

/// Any JSON number as Int64.
func i64(_ value: Any) -> Int64 { (value as! NSNumber).int64Value }

enum EventSpec {
    static func attendee(_ j: JSON) -> CalendarCore.Attendee {
        CalendarCore.Attendee(
            name: j["name"] as? String, email: j["email"] as? String,
            role: (j["role"] as? String).flatMap(AttendeeRole.init(rawValue:)) ?? .required,
            response: (j["response"] as? String).flatMap(ResponseStatus.init(rawValue:)) ?? .needsAction,
            isSelf: j["isSelf"] as? Bool ?? false, isOrganizer: false)
    }

    static func event(_ j: JSON) -> TimeTugCalendarEvent {
        let zone = TimeZone(identifier: j["timeZone"] as? String ?? "UTC")!
        let conferences = (j["conferences"] as? [JSON] ?? []).map {
            ConferenceInfo(
                url: URL(string: $0["url"] as! String)!, provider: ConferenceProvider(rawValue: $0["provider"] as! String)!,
                origin: ConferenceOrigin(rawValue: $0["origin"] as? String ?? "structured")!)
        }
        let base = CalendarCore.CalendarEvent(
            eventID: j["eventID"] as! String, uid: j["uid"] as? String,
            uidScope: (j["uidScope"] as? String).flatMap(UIDScope.init(rawValue:)),
            calendarID: j["calendarID"] as! String, title: j["title"] as! String,
            notes: j["notes"] as? String, location: j["location"] as? String,
            start: date(ms: i64(j["start"]!)), end: date(ms: i64(j["end"]!)), timeZone: zone,
            isAllDay: j["isAllDay"] as? Bool ?? false, attendees: (j["attendees"] as? [JSON] ?? []).map(attendee),
            organizer: (j["organizer"] as? JSON).map(attendee), conferences: conferences,
            url: (j["url"] as? String).flatMap { URL(string: $0) },
            participation: (j["myResponse"] as? String).flatMap(ResponseStatus.init(rawValue:)).map { .invited($0) })
        var event = TimeTugCalendarEvent(event: base, sourceID: j["sourceID"] as! String)
        event.calendarService = j["calendarService"] as? String
        event.calendarProvider = j["calendarProvider"] as? String
        if let count = j["otherAttendeeCount"] as? Int { event.otherAttendeeCount = count }
        if j.keys.contains("responseStatus") { event.responseStatus = (j["responseStatus"] as? String).flatMap(ResponseStatus.init(rawValue:)) }
        if let keys = j["additionalCalendarKeys"] as? [String] { event.additionalCalendarKeys = Set(keys) }
        if let shown = j["displayStart"] as? NSNumber { event.displayStart = date(ms: shown.int64Value) }
        event.mergedMembers = (j["mergedMembers"] as? [JSON] ?? []).map {
            MergedMember(
                title: $0["title"] as! String, calendarKey: $0["calendarKey"] as! String, contentKey: $0["contentKey"] as! String,
                details: $0["details"] as! String, start: date(ms: i64($0["start"]!)), end: date(ms: i64($0["end"]!)))
        }
        if let provenance = j["mergeProvenance"] {
            switch provenance {
            case let name as String: event.mergeProvenance = name == "rule" ? .rule : .userConfirmed
            case let object as JSON:
                let inner = object["inference"] as! JSON
                event.mergeProvenance = .inference(engineID: inner["engineID"] as! String, engineName: inner["engineName"] as! String)
            default: break
            }
        }
        return event
    }

    /// What an exporter reports about an event it computed (a merge result, say). The fields every consumer reads.
    static func summary(_ e: TimeTugCalendarEvent) -> JSON {
        var out: JSON = [
            "id": e.id, "sourceID": e.sourceID, "eventID": e.sourceEventID, "calendarKey": e.calendarKey, "title": e.title,
            "start": ms(e.start), "end": ms(e.end), "isAllDay": e.isAllDay, "contentKey": e.contentKey,
            "conferenceURLs": e.conferences.map { $0.url.absoluteString },
            "otherAttendeeCount": e.otherAttendeeCount,
            "additionalCalendarKeys": e.additionalCalendarKeys.sorted(),
            "mergedMembers": e.mergedMembers.map { m -> JSON in
                ["title": m.title, "calendarKey": m.calendarKey, "contentKey": m.contentKey, "details": m.details,
                 "start": ms(m.start), "end": ms(m.end)]
            },
            "responseStatus": e.responseStatus?.rawValue ?? NSNull(),
            "displayStart": e.displayStart.map { ms($0) } ?? NSNull(),
            "location": e.location ?? NSNull(), "notes": e.notes ?? NSNull(), "url": e.url?.absoluteString ?? NSNull(),
        ]
        switch e.mergeProvenance {
        case nil: out["mergeProvenance"] = NSNull()
        case .rule?: out["mergeProvenance"] = "rule"
        case .userConfirmed?: out["mergeProvenance"] = "userConfirmed"
        case .inference(let id, let name)?: out["mergeProvenance"] = ["inference": ["engineID": id, "engineName": name]]
        }
        return out
    }

    static func settings(_ j: JSON) -> TakeoverSettings {
        var s = TakeoverSettings()
        s.leadTime = (j["leadTime"] as? NSNumber)?.doubleValue ?? s.leadTime
        s.takeoverCalendarKeys = Set(j["takeoverCalendarKeys"] as? [String] ?? [])
        s.hiddenCalendarKeys = Set(j["hiddenCalendarKeys"] as? [String] ?? [])
        s.requireConferenceLink = j["requireConferenceLink"] as? Bool ?? false
        s.requireOtherAttendees = j["requireOtherAttendees"] as? Bool ?? false
        s.skipAllDayEvents = j["skipAllDayEvents"] as? Bool ?? true
        s.enabled = j["enabled"] as? Bool ?? true
        return s
    }

    static func calendar(zone id: String) -> Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: id)!
        return calendar
    }
}

// Shared pools for the generated areas.
enum Pools {
    static let titles = [
        "Standup", "Weekly sync", "Piano lesson", "Doctor", "Scott: Doctor", "Intermountain Health", "Team offsite",
        "Lunch", "1:1", "Planning - Q4", "Dentist appt.", "Board meeting", "Parent night", "Design review",
        "Caf\u{E9} R\u{F6}ndevoo", "R\u{E9}union \u{E9}quipe", "Dance class", "Yoga", "Quarterly business review",
    ]
    static let locations = ["Room 101 East", "Conference Room B", "Dentist Office", "Riverside Hall", "Main St", "HQ", "Cafe Roma"]
    static let emails = ["kristin@x.com", "scott@x.com", "dana@y.org", "lee@z.net", "pat@x.com"]
    static let zones = [
        "UTC", "America/New_York", "America/Los_Angeles", "America/Denver", "Europe/London", "Europe/Berlin",
        "Asia/Tokyo", "Australia/Sydney", "Asia/Kolkata", "Pacific/Auckland", "America/St_Johns", "Pacific/Kiritimati",
    ]
    /// A conference link and its provider; `variants` are other spellings of the same meeting.
    static let conferences: [(url: String, provider: String, variants: [String])] = [
        ("https://meet.google.com/abc-defg-hij", "meet", ["https://MEET.google.com/abc-defg-hij/", "https://meet.google.com/abc-defg-hij?authuser=0"]),
        ("https://teams.microsoft.com/l/meetup-join/19%3ameeting_Xyz%40thread.v2/0", "teams", ["https://TEAMS.microsoft.com/l/meetup-join/19%3AMeeting_xyz%40thread.v2/0/"]),
        ("https://acme.zoom.us/j/123456789?pwd=abc", "zoom", ["https://acme.zoom.us/j/123456789/", "https://ACME.zoom.us/j/123456789?pwd=zzz"]),
        ("https://acme.zoom.us/j/987654321", "zoom", ["https://acme.zoom.us/j/987654321?pwd=q"]),
        ("https://acme.webex.com/meet/jdoe", "webex", ["https://acme.webex.com/meet/JDoe/"]),
        ("https://acme.webex.com/acme/j.php?MTID=m11&other=1", "webex", ["https://acme.webex.com/acme/j.php?other=2&mtid=M11"]),
        ("https://acme.webex.com/acme/j.php?MTID=m22", "webex", []),
        ("https://chime.aws/1234567890", "other", ["https://chime.aws/1234567890/"]),
        ("https://whereby.com/some-room", "whereby", []),
    ]
}
