// The records of protocol v1, written to match the golden fixtures in Conformance/Fixtures/golden (the round-trip
// tests in TimeTug.Engine.Tests fail when a record and a fixture disagree). Member names are the wire field names
// in PascalCase; the camel-case naming policy of WireJson maps them back. Instants are UTC DateTimeOffset values
// (millisecond precision on the wire), all-day dates are DateOnly, durations are seconds. Enumerations stay strings
// so that a value a later engine adds does not break an older host; the documented values are listed per member.
// Per-member XML docs would only repeat the wire field names, so CS1591 is off in this file alone.
#pragma warning disable CS1591

using System;
using System.Collections.Generic;
using System.Text.Json.Serialization;

namespace TimeTug.Engine;

// MARK: - State

/// <summary>The whole engine state, field for field (message <c>state</c>). Newer states replace older ones.</summary>
public sealed record EngineState
{
    public ulong Revision { get; init; }
    public Agenda Agenda { get; init; } = new();
    public IReadOnlyList<CalendarEvent> Events { get; init; } = [];
    public IReadOnlyList<CalendarInfo> Calendars { get; init; } = [];
    /// <summary>Source id to its status.</summary>
    public IReadOnlyDictionary<string, SourceStatus> Statuses { get; init; } = new Dictionary<string, SourceStatus>();
    /// <summary>Source id to its notices.</summary>
    public IReadOnlyDictionary<string, IReadOnlyList<SourceNotice>> Notices { get; init; } =
        new Dictionary<string, IReadOnlyList<SourceNotice>>();
    /// <summary>Source id to its display name (an account name, not an engine-composed string).</summary>
    public IReadOnlyDictionary<string, string> SourceNames { get; init; } = new Dictionary<string, string>();
    /// <summary>Pending duplicate pairs: an event id to the ids of the events it may duplicate (all in
    /// <see cref="Events"/>).</summary>
    public IReadOnlyDictionary<string, IReadOnlyList<string>> Candidates { get; init; } =
        new Dictionary<string, IReadOnlyList<string>>();
    public IReadOnlyList<Account> Accounts { get; init; } = [];
    public AccountStatus AccountStatus { get; init; } = new();
    public EngineSettings Settings { get; init; } = new();
    public InferenceStatus InferenceStatus { get; init; } = new();
    public TakeoverLedger Ledger { get; init; } = new();
    public NextTakeover? NextTakeover { get; init; }
    public bool TakeoverVisible { get; init; }
    public IReadOnlyList<AccountKind> AvailableAccountKinds { get; init; } = [];
}

/// <summary>A merged event's original copy. <see cref="MemberID"/> is what <see cref="EngineCommand.Separate"/>
/// takes.</summary>
public sealed record EventMember
{
    public string MemberID { get; init; } = "";
    public string CalendarKey { get; init; } = "";
    public DateTimeOffset Start { get; init; }
    public DateTimeOffset End { get; init; }
    public string Details { get; init; } = "";
}

public sealed record Conference
{
    public string Url { get; init; } = "";
    /// <summary><c>meet</c>, <c>teams</c>, <c>zoom</c>, <c>webex</c>, <c>goToMeeting</c>, <c>whereby</c>,
    /// <c>jitsi</c>, <c>slack</c> or <c>other</c>.</summary>
    public string Provider { get; init; } = "";
}

public sealed record MergeProvenance
{
    /// <summary><c>rule</c>, <c>inference</c> or <c>userConfirmed</c>.</summary>
    public string Kind { get; init; } = "";
    public string? EngineID { get; init; }
    public string? EngineName { get; init; }
}

/// <summary>One calendar event. A timed event has <see cref="Start"/> and <see cref="End"/>; an all-day event has
/// <see cref="StartDate"/> and <see cref="EndDate"/> (exclusive), read in <see cref="TimeZone"/>.</summary>
public sealed record CalendarEvent
{
    public string Id { get; init; } = "";
    public string SourceID { get; init; } = "";
    public string SourceEventID { get; init; } = "";
    public string CalendarKey { get; init; } = "";
    public string Title { get; init; } = "";
    public bool IsAllDay { get; init; }
    public DateTimeOffset? Start { get; init; }
    public DateTimeOffset? End { get; init; }
    public DateOnly? StartDate { get; init; }
    public DateOnly? EndDate { get; init; }
    /// <summary>Where the shown range starts when it differs from <see cref="Start"/> (a merged meeting).</summary>
    public DateTimeOffset? DisplayStart { get; init; }
    /// <summary>An IANA zone id.</summary>
    public string TimeZone { get; init; } = "";
    public string? Location { get; init; }
    public string? Url { get; init; }
    /// <summary><c>confirmed</c>, <c>tentative</c> or <c>cancelled</c>.</summary>
    public string Status { get; init; } = "";
    public IReadOnlyList<Conference> Conferences { get; init; } = [];
    public int OtherAttendeeCount { get; init; }
    /// <summary><c>accepted</c>, <c>tentative</c>, <c>declined</c> or <c>needsAction</c>; null when the provider
    /// does not say.</summary>
    public string? ResponseStatus { get; init; }
    public IReadOnlyList<string> AdditionalCalendarKeys { get; init; } = [];
    public IReadOnlyList<EventMember> Members { get; init; } = [];
    public MergeProvenance? MergeProvenance { get; init; }
}

public sealed record AgendaItem
{
    public string EventID { get; init; } = "";
    /// <summary><c>past</c>, <c>current</c> or <c>upcoming</c>.</summary>
    public string State { get; init; } = "";
}

public sealed record Agenda
{
    public IReadOnlyList<AgendaItem> Items { get; init; } = [];
    public string? NextEventID { get; init; }
}

public sealed record CalendarInfo
{
    public string Key { get; init; } = "";
    public string SourceID { get; init; } = "";
    public string CalendarID { get; init; } = "";
    public string Title { get; init; } = "";
    public string? AccountName { get; init; }
    public string? ColorHex { get; init; }
    /// <summary><c>standard</c>, <c>subscribed</c> or <c>birthdays</c>.</summary>
    public string Kind { get; init; } = "";
    public string? Service { get; init; }
    public string? Provider { get; init; }
}

public sealed record SourceStatus
{
    /// <summary><c>ok</c>, <c>needsPermission</c>, <c>authExpired</c> or <c>failing</c>.</summary>
    public string Kind { get; init; } = "";
    public string? Message { get; init; }
}

public sealed record SourceNotice
{
    public string Kind { get; init; } = "";
    public int Count { get; init; }
}

public sealed record Account
{
    public string KindID { get; init; } = "";
    public string ConnectionID { get; init; } = "";
    public string DisplayName { get; init; } = "";
    public IReadOnlyDictionary<string, string> Config { get; init; } = new Dictionary<string, string>();
}

public sealed record AccountStatus
{
    public bool IsWorking { get; init; }
    public IReadOnlyDictionary<string, string> BuildFailures { get; init; } = new Dictionary<string, string>();
    public string? ErrorMessage { get; init; }
    /// <summary><c>browser</c> or <c>inApp</c>.</summary>
    public string WaitingStyle { get; init; } = "";
}

public sealed record AccountKind
{
    public string Id { get; init; } = "";
    /// <summary>The provider's product name ("Google"); not localized. Key localized labels on <see cref="Id"/>.</summary>
    public string DisplayName { get; init; } = "";
    /// <summary><c>oauth</c>, <c>password</c> or <c>system</c>.</summary>
    public string Authorization { get; init; } = "";
    public bool IsConfigured { get; init; }
}

public sealed record InferenceStatus
{
    /// <summary><c>disabled</c>, <c>noEngine</c>, <c>unavailable</c>, <c>notOnDevice</c> or <c>active</c>.</summary>
    public string Kind { get; init; } = "";
    public string? Reason { get; init; }
    public string? EngineID { get; init; }
    public string? EngineName { get; init; }
    public bool? IsOnDevice { get; init; }
}

public sealed record TakeoverLedger
{
    public int Count { get; init; }
}

public sealed record NextTakeover
{
    public string EventID { get; init; } = "";
    public DateTimeOffset FireAt { get; init; }
}

public sealed record TakeoverSettings
{
    /// <summary>Seconds.</summary>
    public double LeadTime { get; init; }
    public IReadOnlyList<string> TakeoverCalendarKeys { get; init; } = [];
    public IReadOnlyList<string> HiddenCalendarKeys { get; init; } = [];
    public bool RequireConferenceLink { get; init; }
    public bool RequireOtherAttendees { get; init; }
    public bool SkipAllDayEvents { get; init; }
    public bool Enabled { get; init; }
}

public sealed record EngineSettings
{
    public TakeoverSettings Takeover { get; init; } = new();
    public bool InferenceEnabled { get; init; }
    public bool SystemCalendarEnabled { get; init; }
}

// MARK: - Event payloads

/// <summary>Payload of <c>ready</c>, the first message after <c>start</c>.</summary>
public sealed record ReadyInfo
{
    public string EngineVersion { get; init; } = "";
    public IReadOnlyList<int> ProtocolVersions { get; init; } = [];
    public string Platform { get; init; } = "";
    public EngineCapabilities Capabilities { get; init; } = new();
}

public sealed record EngineCapabilities
{
    public bool SystemCalendar { get; init; }
    public bool TextGeneration { get; init; }
}

/// <summary>Payload of <c>event.takeoverDue</c>.</summary>
public sealed record TakeoverRequest
{
    public CalendarEvent Event { get; init; } = new();
    public bool HasStarted { get; init; }
    public string? JoinURL { get; init; }
    /// <summary>Seconds.</summary>
    public IReadOnlyList<double> SnoozeOptions { get; init; } = [];
    /// <summary>The calendar's own title (data, not an engine-composed string).</summary>
    public string? CalendarTitle { get; init; }
}

public sealed record WidgetEvent
{
    public string Id { get; init; } = "";
    public string Title { get; init; } = "";
    public DateTimeOffset Start { get; init; }
    public DateTimeOffset End { get; init; }
    public bool IsAllDay { get; init; }
    public string? ColorHex { get; init; }
    public string? JoinURL { get; init; }
}

/// <summary>Payload of <c>event.widgetSnapshot</c>: all-day events carry the local midnights of their dates.</summary>
public sealed record WidgetSnapshot
{
    public DateTimeOffset GeneratedAt { get; init; }
    public IReadOnlyList<WidgetEvent> Events { get; init; } = [];
}

/// <summary>A diagnostic from the engine (<c>event.diagnostic</c>) or from this wrapper. Never calendar
/// content.</summary>
public sealed record EngineDiagnostic
{
    /// <summary><c>info</c>, <c>warning</c> or <c>error</c>.</summary>
    public string Level { get; init; } = "";
    public string Category { get; init; } = "";
    public string Message { get; init; } = "";
    /// <summary>The envelope id of the command that caused it (numbers as their decimal text); not part of the
    /// payload.</summary>
    [JsonIgnore]
    public string? CommandId { get; init; }
}

/// <summary>Something the engine tells the host. Delivered in order through <see cref="IEngine.Events"/>; never
/// dropped.</summary>
public abstract record EngineEvent
{
    private protected EngineEvent() { }

    /// <summary><c>ready</c>: the engine started; the version and capabilities it has.</summary>
    public sealed record Ready(ReadyInfo Info) : EngineEvent;

    /// <summary><c>event.takeoverDue</c>: present a takeover now.</summary>
    public sealed record TakeoverDue(TakeoverRequest Request) : EngineEvent;

    /// <summary><c>event.takeoverWithdrawn</c>: the takeover for this event no longer applies.</summary>
    public sealed record TakeoverWithdrawn(string EventID) : EngineEvent;

    /// <summary><c>event.widgetSnapshot</c>.</summary>
    public sealed record WidgetSnapshotUpdated(WidgetSnapshot Snapshot) : EngineEvent;

    /// <summary><c>event.joinRequested</c>: open this URL with the host's own safe-open policy.</summary>
    public sealed record JoinRequested(string Url) : EngineEvent;

    /// <summary><c>event.diagnostic</c>.</summary>
    public sealed record Diagnostic(EngineDiagnostic Info) : EngineEvent;
}

// MARK: - Commands

/// <summary>A command from the host to the engine. Each nested record's properties are its wire payload.</summary>
public abstract record EngineCommand
{
    private protected EngineCommand() { }

    /// <summary>The envelope <c>type</c>.</summary>
    internal abstract string WireType { get; }

    public sealed record Start : EngineCommand { internal override string WireType => "start"; }

    public sealed record Refresh : EngineCommand { internal override string WireType => "refresh"; }

    public sealed record AddAccount(string KindID) : EngineCommand { internal override string WireType => "account.add"; }

    public sealed record ReconnectAccount(string ConnectionID) : EngineCommand
    {
        internal override string WireType => "account.reconnect";
    }

    public sealed record RemoveAccount(string ConnectionID) : EngineCommand
    {
        internal override string WireType => "account.remove";
    }

    public sealed record CancelAuthorization : EngineCommand { internal override string WireType => "account.cancel"; }

    /// <summary>A settings patch: a null member leaves that setting unchanged; a key list replaces the stored
    /// list.</summary>
    public sealed record ApplySettings : EngineCommand
    {
        internal override string WireType => "settings.apply";
        /// <summary>Seconds.</summary>
        public double? LeadTime { get; init; }
        public IReadOnlyList<string>? TakeoverCalendarKeys { get; init; }
        public IReadOnlyList<string>? HiddenCalendarKeys { get; init; }
        public bool? RequireConferenceLink { get; init; }
        public bool? RequireOtherAttendees { get; init; }
        public bool? SkipAllDayEvents { get; init; }
        public bool? Enabled { get; init; }
        public bool? InferenceEnabled { get; init; }
        public bool? SystemCalendarEnabled { get; init; }
    }

    public sealed record SetCountdownVisible(bool Visible) : EngineCommand
    {
        internal override string WireType => "countdown.visible";
    }

    /// <summary><paramref name="OnScreen"/> false means the front end fell back to a toast.</summary>
    public sealed record TakeoverPresented(string EventID, bool OnScreen) : EngineCommand
    {
        internal override string WireType => "takeover.presented";
    }

    public sealed record TakeoverClosed(string EventID) : EngineCommand
    {
        internal override string WireType => "takeover.closed";
    }

    public sealed record Join(string EventID) : EngineCommand { internal override string WireType => "takeover.join"; }

    public sealed record Dismiss(string EventID) : EngineCommand { internal override string WireType => "takeover.dismiss"; }

    /// <summary><paramref name="Seconds"/>: how long to snooze.</summary>
    public sealed record Snooze(string EventID, double Seconds) : EngineCommand
    {
        internal override string WireType => "takeover.snooze";
    }

    public sealed record TestTakeover(string Title, string CalendarTitle) : EngineCommand
    {
        internal override string WireType => "takeover.test";
    }

    /// <summary>Exactly two event ids.</summary>
    public sealed record Merge(IReadOnlyList<string> EventIDs) : EngineCommand
    {
        internal override string WireType => "dedup.merge";
    }

    public sealed record Unmerge(string EventID) : EngineCommand { internal override string WireType => "dedup.unmerge"; }

    public sealed record Separate(string EventID, IReadOnlyList<string> MemberIDs) : EngineCommand
    {
        internal override string WireType => "dedup.separate";
    }

    public sealed record ForgetCorrections : EngineCommand { internal override string WireType => "dedup.forget"; }

    /// <summary><paramref name="Kind"/>: one of <see cref="SystemEventKind"/>. <paramref name="TimeZone"/>: an IANA
    /// zone id, with <see cref="SystemEventKind.TimeZoneChanged"/> only.</summary>
    public sealed record SystemEvent(string Kind, string? TimeZone = null) : EngineCommand
    {
        internal override string WireType => "system.event";
    }

    /// <summary>Moves the manual clock (only with <see cref="EngineOptions.Testing"/>).</summary>
    public sealed record AdvanceTestClock(double Seconds) : EngineCommand
    {
        internal override string WireType => "test.advanceClock";
    }
}

/// <summary>The values of <see cref="EngineCommand.SystemEvent.Kind"/>.</summary>
public static class SystemEventKind
{
    public const string Wake = "wake";
    public const string ClockChanged = "clockChanged";
    public const string TimeZoneChanged = "timeZoneChanged";
    public const string DayChanged = "dayChanged";
    public const string NetworkReturned = "networkReturned";
    public const string DisplayChanged = "displayChanged";
}
