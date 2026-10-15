import CalendarCore
import Foundation
import TimeTugCore
import TimeTugEngine

// Engine and Core types to wire DTOs. This is the one file where the protocol module names Core and engine types
// in its public API (the architecture check allow-lists it). Nothing here composes a display string.

extension WireEvent {
    public init(_ event: TimeTugCalendarEvent) {
        id = event.id
        sourceID = event.sourceID
        sourceEventID = event.sourceEventID
        calendarKey = event.calendarKey
        title = event.title
        isAllDay = event.isAllDay
        timeZone = event.timeZone.identifier
        location = event.location
        url = event.url?.absoluteString
        status = event.event.status.rawValue
        conferences = event.conferences.map { WireConference(url: $0.url.absoluteString, provider: $0.provider.rawValue) }
        otherAttendeeCount = event.otherAttendeeCount
        responseStatus = event.responseStatus?.rawValue
        additionalCalendarKeys = event.additionalCalendarKeys.sorted()
        members = event.mergedMembers.map {
            WireMember(
                memberID: $0.contentKey, calendarKey: $0.calendarKey, start: WireDates.string($0.start),
                end: WireDates.string($0.end), details: $0.details)
        }
        switch event.mergeProvenance {
        case nil: mergeProvenance = nil
        case .rule?: mergeProvenance = WireProvenance(kind: "rule")
        case .userConfirmed?: mergeProvenance = WireProvenance(kind: "userConfirmed")
        case .inference(let engineID, let engineName)?:
            mergeProvenance = WireProvenance(kind: "inference", engineID: engineID, engineName: engineName)
        }
        if let dates = event.allDayDates {
            startDate = WireDates.dateString(year: dates.first.year, month: dates.first.month, day: dates.first.day)
            endDate = WireDates.dateString(
                year: dates.endExclusive.year, month: dates.endExclusive.month, day: dates.endExclusive.day)
            start = nil
            end = nil
            displayStart = nil
        } else {
            start = WireDates.string(event.start)
            end = WireDates.string(event.end)
            displayStart = event.displayStart.map(WireDates.string)
            startDate = nil
            endDate = nil
        }
    }
}

extension WireCalendar {
    init(_ info: CalendarInfo) {
        key = info.key
        sourceID = info.sourceID
        calendarID = info.calendarID
        title = info.title
        accountName = info.accountName
        colorHex = info.colorHex
        kind = info.kind.rawValue
        service = info.service?.rawValue
        provider = info.provider?.rawValue
    }
}

extension WireSourceStatus {
    init(_ status: SourceStatus) {
        switch status {
        case .ok: self.init(kind: "ok", message: nil)
        case .needsPermission: self.init(kind: "needsPermission", message: nil)
        case .authExpired: self.init(kind: "authExpired", message: nil)
        case .failing(let message): self.init(kind: "failing", message: message)
        }
    }
}

extension WireInferenceStatus {
    init(_ status: InferenceStatus) {
        switch status {
        case .disabled: self.init(kind: "disabled")
        case .noEngine: self.init(kind: "noEngine")
        case .notOnDevice: self.init(kind: "notOnDevice")
        case .unavailable(let reason): self.init(kind: "unavailable", reason: reason)
        case .active(let info):
            self.init(kind: "active", engineID: info.id, engineName: info.displayName, isOnDevice: info.isOnDevice)
        }
    }
}

extension WireSettings {
    public init(_ settings: EngineSettings) {
        let t = settings.takeover
        takeover = WireTakeoverSettings(
            leadTime: t.leadTime, takeoverCalendarKeys: t.takeoverCalendarKeys.sorted(),
            hiddenCalendarKeys: t.hiddenCalendarKeys.sorted(), requireConferenceLink: t.requireConferenceLink,
            requireOtherAttendees: t.requireOtherAttendees, skipAllDayEvents: t.skipAllDayEvents, enabled: t.enabled)
        inferenceEnabled = settings.inferenceEnabled
        systemCalendarEnabled = settings.systemCalendarEnabled
    }
}

extension WireSettingsPatch {
    /// Applies every present field; absent fields leave `settings` alone. Hiding a calendar also removes its
    /// takeover opt-in (a hidden calendar never tugs), the same invariant `TakeoverSettings` decodes with.
    public func apply(to settings: inout EngineSettings) {
        if let v = leadTime { settings.takeover.leadTime = v }
        if let v = takeoverCalendarKeys { settings.takeover.takeoverCalendarKeys = Set(v) }
        if let v = hiddenCalendarKeys { settings.takeover.hiddenCalendarKeys = Set(v) }
        if let v = requireConferenceLink { settings.takeover.requireConferenceLink = v }
        if let v = requireOtherAttendees { settings.takeover.requireOtherAttendees = v }
        if let v = skipAllDayEvents { settings.takeover.skipAllDayEvents = v }
        if let v = enabled { settings.takeover.enabled = v }
        if let v = inferenceEnabled { settings.inferenceEnabled = v }
        if let v = systemCalendarEnabled { settings.systemCalendarEnabled = v }
        settings.takeover.takeoverCalendarKeys.subtract(settings.takeover.hiddenCalendarKeys)
    }
}

extension WireState {
    public init(_ state: EngineState) {
        revision = state.revision
        agenda = WireAgenda(
            items: state.agenda.items.map {
                WireAgendaItem(eventID: $0.event.id, state: Self.agendaState($0.state))
            },
            nextEventID: state.agenda.next?.id)
        events = state.events.map(WireEvent.init)
        calendars = state.calendars.map(WireCalendar.init)
        statuses = state.statuses.mapValues(WireSourceStatus.init)
        notices = state.notices.mapValues { $0.map { WireNotice(kind: Self.noticeKind($0.kind), count: $0.count) } }
        sourceNames = state.sourceNames
        candidates = state.candidates.mapValues { $0.map(\.id) }
        accounts = state.accounts.map {
            WireAccount(kindID: $0.kindID, connectionID: $0.connectionID, displayName: $0.displayName, config: $0.config)
        }
        let status = state.accountStatus
        accountStatus = WireAccountStatus(
            isWorking: status.isWorking, buildFailures: status.buildFailures, errorMessage: status.errorMessage,
            waitingStyle: status.waitingStyle == .inApp ? "inApp" : "browser")
        settings = WireSettings(state.settings)
        inferenceStatus = WireInferenceStatus(state.inferenceStatus)
        ledger = WireLedger(count: state.ledger.keyCount)
        nextTakeover = state.nextTakeover.map { WireNextTakeover(eventID: $0.event.id, fireAt: WireDates.string($0.fireAt)) }
        takeoverVisible = state.takeoverVisible
        availableAccountKinds = state.availableAccountKinds.map {
            WireAccountKind(
                id: $0.id, displayName: $0.displayName, authorization: Self.authorization($0.authorization),
                isConfigured: $0.isConfigured)
        }
    }

    private static func agendaState(_ state: DayAgenda.State) -> String {
        switch state {
        case .past: "past"
        case .current: "current"
        case .upcoming: "upcoming"
        }
    }

    private static func noticeKind(_ kind: CalendarCore.SourceNotice.Kind) -> String {
        switch kind {
        case .unreadableRecurrence: "unreadableRecurrence"
        }
    }

    private static func authorization(_ a: AccountKindInfo.Authorization) -> String {
        switch a {
        case .oauth: "oauth"
        case .password: "password"
        case .system: "system"
        }
    }
}

extension WireTakeoverRequest {
    public init(_ request: TakeoverRequest, calendarTitle: String?) {
        event = WireEvent(request.event)
        hasStarted = request.hasStarted
        joinURL = request.joinURL?.absoluteString
        snoozeOptions = request.snoozeOptions
        self.calendarTitle = calendarTitle
    }
}

extension WireWidgetSnapshot {
    public init(_ snapshot: WidgetSnapshot) {
        generatedAt = WireDates.string(snapshot.generatedAt)
        events = snapshot.events.map {
            WireWidgetEvent(
                id: $0.id, title: $0.title, start: WireDates.string($0.start), end: WireDates.string($0.end),
                isAllDay: $0.isAllDay, colorHex: $0.colorHex, joinURL: $0.joinURL?.absoluteString)
        }
    }
}

extension WireDiagnostic {
    public init(_ diagnostic: EngineDiagnostic) {
        level = diagnostic.level.rawValue
        category = diagnostic.category
        message = diagnostic.message
    }
}

extension JSONValue {
    /// The JSON value of any Encodable DTO. Encoding a value the module's own DTOs produce cannot fail.
    public init<T: Encodable>(encoding value: T) {
        guard let data = try? JSONEncoder().encode(value),
              let decoded = try? JSONDecoder().decode(JSONValue.self, from: data) else {
            self = .null
            return
        }
        self = decoded
    }
}

/// An engine event as a host-bound message. Events carry no envelope id; the session adds one only to echo a
/// rejected command.
public func wireMessage(for event: EngineEvent) -> Envelope {
    switch event {
    case .takeoverDue(let request, let calendarTitle):
        return Envelope(
            type: "event.takeoverDue", payload: JSONValue(encoding: WireTakeoverRequest(request, calendarTitle: calendarTitle)))
    case .takeoverWithdrawn(let eventID):
        return Envelope(type: "event.takeoverWithdrawn", payload: .object(["eventID": .string(eventID)]))
    case .widgetSnapshot(let snapshot):
        return Envelope(type: "event.widgetSnapshot", payload: JSONValue(encoding: WireWidgetSnapshot(snapshot)))
    case .joinRequested(let url):
        return Envelope(type: "event.joinRequested", payload: .object(["url": .string(url.absoluteString)]))
    case .diagnostic(let diagnostic):
        return wireDiagnostic(diagnostic)
    }
}

public func wireDiagnostic(_ diagnostic: EngineDiagnostic, id: MessageID? = nil) -> Envelope {
    Envelope(type: "event.diagnostic", id: id, payload: JSONValue(encoding: WireDiagnostic(diagnostic)))
}

public func wireMessage(for state: EngineState) -> Envelope {
    Envelope(type: "state", payload: JSONValue(encoding: WireState(state)))
}
