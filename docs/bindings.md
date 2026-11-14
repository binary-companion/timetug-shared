# Generated bindings (Swift and Kotlin)

`timetug-engine-ffi` is the typed front door of the engine for Swift and Kotlin hosts, generated with UniFFI. What UniFFI
can and cannot express is in [`bindings-spike.md`](bindings-spike.md); the plan is phase 5 of the Rust engine program. This
page is the reference for what the bindings expose and how their names relate to the Swift names of the 1.x libraries. The
generated code is committed; CI fails when regenerating changes it.

## The surface list

`docs/bindings-surface.txt` is the list of public types of the 1.x Swift libraries that the Mac app and its packages use
(non-test sources). Regenerate it with

```sh
python3 scripts/dev/list-app-types.py \
  --libraries <calendar-connectors 1.x>/Sources <timetug-shared 1.x>/Sources --app <timetug> --output docs/bindings-surface.txt
```

`scripts/ci/check-bindings-surface.py` (job `architecture`) fails for a name that is none of: a type, host interface or
custom type of the crate with that name; a row of "Names that differ"; a row of "Exposed as functions"; a row of "Not
exposed" (with a reason); or a row of "Planned" (reported, not a failure; the task named in the row removes it).

## Names that differ

Two rules apply to every generated name and are not repeated in the table: a record field or argument whose Swift name ends in `ID`
or `URL` is generated as `...Id` or `...Url` (`eventID` is `eventId`, `joinURL` is `joinUrl`; UniFFI lower-camel-cases acronyms and no
rename can restore them), and a plain enum loses its Swift `rawValue`. The Swift package (Task 6) adds the old spellings as
computed properties and extensions.

| Swift name | Generated name | Shim |
|---|---|---|
| `UIDScope` | `UidScope` | typealias |
| `RecurrenceRule.End` | `RuleEnd` | typealias |
| `DayAgenda.Item` | `DayAgendaItem` | typealias |
| `DayAgenda.Item.State` | `DayAgendaItemState` | typealias |
| `AccountManager.Status` | `AccountStatus` | typealias |
| `EngineDiagnostic.Level` | `EngineDiagnosticLevel` | typealias |
| `ConnectionID` | `ConnectionId` | typealias |
| `EngineHost` | `EngineHostInterfaces` | the host objects are one record; the settings are `EngineConfig` |

## Exposed as functions

The Mac app (mainly its EventKit adapter) calls these pure helpers of the 1.x libraries; they are exported as functions of the
same behaviour.

| Swift name | Functions |
|---|---|
| `AllDay` | `all_day_dates`, `all_day_start_of_day`, `all_day_date_of`, `all_day_end_exclusive_after_last`, `calendar_date_adding_days` |
| `CalendarUserAddress` | `calendar_user_address_email` |
| `ConferenceDetector` | `detect_conferences` |
| `JoinURLPolicy` | `join_url_is_allowed` |
| `WriteValidation` | `require_writable` |
| `PatchMerge` | `patch_merge_conflicts`, `event_patch_diff` |

`PatchMerge.apply` (the retry loop around a host write, generic over two host closures) is not exported: the EventKit adapter
keeps the loop (about ten lines) and calls `patch_merge_conflicts` for the field comparison. A patch made with
`event_patch_diff` carries its base event across the boundary (`EventPatch.base`).

## Not exposed

| Swift name | Reason |
|---|---|
| `AccountManager` | Internal to the engine; the app reads `EngineState.accountStatus` and sends account commands. |
| `Attempt` | Only the argument type of `PatchMerge.apply`, which is not exported (above). |
| `CalDAVConnectorKind`, `GoogleConnectorKind`, `ICalSubscriptionKind`, `ICloudConnectorKind`, `MicrosoftConnectorKind` | Hosts do not construct connector kinds; `EngineConfig` (`EngineConfig`) enables them. |
| `GoogleOAuthConfig`, `MicrosoftOAuthConfig` | Replaced by the client fields of `EngineConfig` (`EngineConfig`). |
| `RetentionWindow` | Replaced by `EngineConfig.icalRetention` (`EngineConfig`). |
| `ConnectorKind`, `ConnectorRegistry` | Connector registration is configuration (`EngineConfig`, Task 4). |
| `CalendarSnapshot` | The engine's `EngineState` carries the same data. |
| `CalendarSource`, `SeriesSource`, `WritableCalendarSource`, `ConnectedSource`, `SyncStateStore` | The source seam stays inside the engine; the host implements `SystemCalendarSource`. |
| `SHA256Hashing` | Removed: the engine hashes with `sha2`. |
| `Scheduler` | The engine computes the next takeover: the app reads `EngineState.nextTakeover`. Phase 6 verifies that `MenuBarIconState` can use it (it is `nil` while a takeover is on screen). |
| `WidgetTimeline`, `NextUp`, `TodayRow` | App code: widgets do not link the engine (design 6.6); `WidgetSnapshot` is the contract. |
| `DiagnosticLog`, `FanOutDiagnosticLog`, `NullDiagnosticLog`, `RingBufferDiagnosticLog`, `EngineDiagnosticSink` | The engine reports diagnostics through the `DiagnosticSink` host interface; the app keeps its own log collection in Swift. |
| `Value` | The generic parameter of `FieldUpdate<Value>`, not a type; `FieldUpdate` has one mirror per instantiation (`FieldUpdateString`, `FieldUpdateReminders`, `FieldUpdateRecurrenceRule`). |

## Planned

| Swift name | Task |
|---|---|

## Mirrors that differ from the engine type

- `TakeoverLedger` is a read-only view (`keyCount`): the engine owns the ledger and its file.
- `EventPatch` has one concrete `FieldUpdate` mirror per field type, and a `base`.
- `Platform` is one platform (`macos`, `ios`, `linux`, `windows`), not the engine's option set; `android` arrives with the
  connector library change that adds the flag (`EngineConfig`).
- `BTreeSet` and `BTreeMap` fields cross as `Set` and `Dictionary`; `Date` fields cross as milliseconds since 1970.

## Host interfaces

The app implements these (foreign UniFFI traits). Every method may be called from the engine thread concurrently with the
others, so an implementor is thread-safe (`Sendable`), does not block for long, and hops to its own actor or queue inside the
method when it touches main-actor or EventKit state. A call that never returns is cancelled when the engine drops it (the
Swift `Task` is cancelled; a cooperative implementation stops).

| Interface | Methods | Notes |
|---|---|---|
| `CredentialStore` | `secrets`, `setSecrets`, `removeSecrets`, `credentialSnapshot`, `updateRefreshToken` | async; arguments are labelled `connectionId:` (Swift today: `for:`) |
| `AuthorizationInteraction` | `beginOauthRedirect`, `promptCredentials` | async; `beginOauthRedirect` returns `OAuthRedirectHandle` (a record holding the session: UniFFI cannot return a foreign object from an async foreign method) |
| `OAuthRedirectSession` | `redirectUri` (sync), `authorize`, `close` | async except the address |
| `SignInPrompting` | `prepare`, `lastNonSecretValues` | async |
| `SettingsStorage` | `load`, `save` | **synchronous**, like the engine's seam; return quickly |
| `HttpTransport` | `send` | async; do not follow redirects; throw `HostError.Network` for a transport failure |
| `TextGenerator` | `availability`, `generate` | async |
| `ExternalTokenSource` | `accessToken`, `signIn` | async |
| `SystemCalendarSource` | `id`, `displayName`, `capabilities` (sync); `requestAccess`, `calendars`, `eventsIn(start:end:)`, `series`, `create`, `update`, `delete`, `respond`, `setChangeHandler` | async; errors are `FfiError` (`.Source(.needsPermission)`, `.Write(.forbidden(nil))`); the engine calls `setChangeHandler` once, before the first read, and the host calls `eventsChanged`, `calendarsChanged` or `sourceFailed` on the handler it was given |
| `EngineObserver` | `stateChanged`, `event` | **synchronous**, called from the delivery thread one call at a time, in order; states are coalesced newest-wins, events are never dropped; nothing after `shutdown()` returns |
| `DiagnosticSink` | `log` | synchronous |

What a callback throws: `HostError` (`Network(message)`, `Cancelled`, `NotFound`, `Denied`, `Failed(message)`); any other
Swift error arrives as `Failed` with its description. The engine converts it as the C interface's reply errors are converted
(general services: `Failure.Host`; HTTP: `SourceError.Network`; external tokens: fixed texts; a cancelled reply is a
cancellation everywhere).

## Configuration

`EngineConfig` (a record): `dataDirectory`, `platform` (`macos`, `ios`, `android`, `linux`, `windows`), `googleClient`
(`id` and `secret`), `microsoftClient` (`id`), `textGeneration`, `externalTokens`, `systemCalendar`, `timeZone`
(identifier), `icalDaysBack` (3) and `icalDaysAhead` (7). The plan's single `icalRetention` field is two flat fields
here, so each has a default in Swift. `platform` decides the account kinds offered; `android` offers none until the
connector library gains the flag (`Platform::ANDROID`, a `calendar-connectors` change). An invalid configuration is
`FfiError.InvalidConfig(reason:)` from `Engine(config:...)` (empty data directory, unknown time zone, a client with an empty
id or secret, a retention above 3650 days).

## The `Engine` object

```swift
public final class Engine {
    public init(config: EngineConfig, host: EngineHostInterfaces, observer: EngineObserver) throws   // FfiError.InvalidConfig
    public func start() async
    public func send(_ command: EngineCommand) async          // returns when the engine finished the command
    public func currentSummary() -> EngineStateSummary
    public func currentState() -> EngineState                 // full: summary plus events, agenda, candidates
    public func currentEvents() -> [TimeTugCalendarEvent]
    public func currentAgenda() -> DayAgenda
    public func currentCandidates() -> [String: [TimeTugCalendarEvent]]
    public func setTimeZone(_ identifier: String) throws      // then send(.system(.timeZoneChanged))
    public func shutdown()                                    // idempotent
}
```

- **State arrives as a summary.** `EngineObserver.stateChanged` gets an `EngineStateSummary`: the state without its three large
  parts (events, agenda, merge candidates), which carry a revision each (`eventsRevision`, `agendaRevision`,
  `candidatesRevision`: the revision of the state in which that part last changed). A host that renders events fetches
  `currentEvents()` only when `eventsRevision` changed. The reason is measured: delivering 500 full events to a Swift observer
  took 24 ms (budget 5 ms; `bindings-spike.md`). The Swift package (Task 6) offers `engine.states` as a stream of full
  `EngineState`s built the same way.
- **Delivery**: the first observer call after `start()` is a state; summaries coalesce (newest wins), events are never
  dropped, calls are serialised and ordered, nothing is delivered after `shutdown()` returns; `shutdown()` inside an observer
  call returns at once.
- **Commands**: those of one kind of work (accounts, settings, takeovers, ...) run one after another in send order; kinds do
  not wait for each other, so a sign-in waiting for a person never holds up a takeover. `send` returns when the engine
  finished the command, and at once after `shutdown()`.
- **Diagnostics** go to `EngineHostInterfaces.diagnostics` when the host gave one, otherwise they arrive as
  `EngineEvent.diagnostic`.
- `EngineConfig` flags and interfaces must agree: `textGeneration`, `externalTokens` and `systemCalendar` are on exactly when
  the matching interface is passed, else `InvalidConfig`.
- Tests of the host can build the library with the cargo feature `test-clock`, which adds `Engine.newForTesting(config:host:observer:clockStart:)`
  and `advanceClock(seconds:)`; it is off in every release build.

## How to regenerate and build

```sh
scripts/dev/generate-bindings.sh      # Swift into swift/TimeTugKit/Sources/TimeTugKitGenerated, Kotlin into kotlin/generated
scripts/dev/build-xcframework.sh --release   # build/TimeTugEngineFFI.xcframework: macOS (arm64 + x86_64), iOS, iOS simulator (arm64 only)
swift build --package-path swift/TimeTugKit
scripts/dev/swift-test.sh --release   # the Swift tests, on a copy of the package built with the `test-clock` feature
scripts/dev/swift-test.sh --ios-sim-link   # the same tests built for the iOS simulator, which links the library
```

`generate-bindings.sh` needs the `bindgen` cargo feature (it builds the two generator binaries) and
`uniffi/cargo-metadata` (without it `uniffi.toml` is silently ignored). The output does not depend on the profile or the
target. The tests need `Engine.newForTesting` and `advanceClock`, which the shipped library does not have (`test-clock` is off
in every release build), so `swift-test.sh` regenerates the Swift into a temporary copy of the package with the feature on.

## The Swift package `TimeTugKit`

Three targets, because the generated file does not compile in Swift 6 language mode (two `Task {}` closures of the async
foreign-trait glue; `docs/bindings-spike.md`): `TimeTugEngineFFI` (the binary XCFramework), `TimeTugKitGenerated` (the
generated Swift, built in Swift 5 mode) and `TimeTugKit` (the hand-written layer below, Swift 6 mode, `@_exported import
TimeTugKitGenerated`). An app depends on the product `TimeTugKit`. Platforms: macOS 14, iOS 17.

Shims (`Sources/TimeTugKit`, each with a test in `Tests/TimeTugKitTests/ShimTests.swift`):

| Shim | What it restores |
|---|---|
| `Names.swift` | the name differences above as typealiases; `eventID`, `calendarID`, `sourceID`, `kindID`, `connectionID`, `seriesID`, `joinURL` as computed properties over the generated `...Id` and `...Url` fields (the `ID` and `URL` acronyms UniFFI cannot spell) |
| `Conformances.swift` | `Identifiable` on `CalendarEvent` (`calendarId/eventId`), `Comparable` on `CalendarDate`, `seriesID`, `originalStart`, `conference`, `myResponse`, `Reminder.minutesBefore`, `accessRole`, `Error` on `SourceError` and `WriteError` |
| `FieldUpdate.swift` | `FieldUpdate<Value>` as a generic enum with conversions to and from the concrete mirrors |
| `SettingsPatch.swift` | `SettingsPatch(from:to:)` and `EngineCommand.applySettings(from:_:)`, the closure spelling that diffs into a patch |
| `ObservedEngine.swift` | `states: AsyncStream<EngineState>` (full states assembled from the summaries, fetching a large part only when its revision changed) and `events: AsyncStream<EngineEvent>`, built on an internal observer, with `start`, `send(_:)`, `applySettings { }` and `shutdown` |

Not restored by a shim (the app changes with the cutover, phase 6): argument labels of the host interfaces (`secrets(connectionId:)`
instead of `secrets(for:)`), the labels of enum payloads that end in `ID` (`.occurrence(seriesId:originalStart:)`), the cases
of `FfiError`, `HostError` and the other UniFFI error enums, which are upper camel (`.Source`, `.Denied`).

## Threading rules for host implementors

Every host interface is called from the engine's own thread, concurrently with the others and with the observer.

- Implementations are thread-safe (`Sendable`). A class that needs main-actor or EventKit state hops to its actor or queue
  inside the method (`await MainActor.run { }`); a main-actor-isolated `async` method may implement an `async` requirement.
  A host interface implemented on the main actor does not deadlock while the main actor awaits `engine.send`: the engine
  never waits for the main thread, and `send` suspends instead of blocking (tested).
- Do not block the engine thread for long. `SettingsStorage`, `OAuthRedirectSession.redirectUri`, `DiagnosticSink`,
  `EngineObserver` and the capability getters of `SystemCalendarSource` are synchronous and run on the engine or delivery
  thread: return quickly. Every other method is `async`: suspend, do not block.
- A call the engine no longer needs is cancelled: the Swift `Task` running it is cancelled, so a cooperative implementation
  (`Task.sleep`, `URLSession`) stops.
- Throw `HostError` for failures the engine should treat specially (`Network`, `Cancelled`, `NotFound`, `Denied`, `Failed`);
  any other error becomes `Failed` with its description. `SystemCalendarSource` throws `FfiError`.
- After `Engine.shutdown()` returns, no host method starts. A call that was running may still finish.
