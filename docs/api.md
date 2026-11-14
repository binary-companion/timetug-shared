# timetug-shared: API reference

`timetug-shared` is the platform-neutral logic of [TimeTug](https://github.com/binary-companion/timetug), published so
other programs can use it: a calendar store with duplicate resolution, takeover scheduling, an agenda and a widget
snapshot, and an **engine** that wraps them with account management, persistence and a message protocol. It builds on
the connector library [calendar-connectors](https://github.com/binary-companion/calendar-connectors)
([its API reference](https://github.com/binary-companion/calendar-connectors/blob/v2/docs/api.md)).

This page documents the `v2` branch (the `2.x` line, Rust). Read section 1 first: it says honestly what exists for
each language today.

Contents: 1 What exists for which language. 2 Versions. 3 Rust API. 4 The engine and its protocol. 5 C reference.
6 C# on Windows. 7 Swift on macOS. 8 Staying current.

## 1. What exists for which language

| You write | Use | Status on 2026-11-13 |
|---|---|---|
| Rust | the crates `timetug-core`, `timetug-bridge`, `timetug-engine`, `timetug-engine-protocol`, `timetug-engine-c` by git tag | Available. Prerelease tag `2.0.0-alpha.1`; no crates.io release. Section 3. |
| C, C++ or anything with a C FFI | the library `TimeTugEngineC` and `include/timetug_engine.h` (four functions, JSON messages) | Available. Section 5. |
| C# on Windows (x64, ARM64) | NuGet package `TimeTug.Engine` | Available. `2.0.0-alpha.1` is a prerelease on nuget.org; `1.8.x` are the earlier Swift-engine releases of the same C# API. Section 6. |
| Swift on macOS | the SwiftPM product `TimeTugKit` of the 2.x tags (UniFFI bindings to the Rust engine), or the `1.x` SwiftPM libraries (`TimeTugCore`, `TimeTugEngine`, ...) | `TimeTugKit` is available from `2.0.0` (macOS 14 and iOS 17); the 1.x Swift line is the earlier engine. Section 7. |

Nothing in this document is implemented that is not listed as available above.

## 2. Versions

- **1.x** (branch `1.x`, tags `1.0.x` to `1.8.4`): the Swift package. The Mac app uses it today. It keeps receiving
  fixes until the Mac app moves to the Rust engine.
- **2.x** (branch `v2`, becoming `main` at the `2.0.0` cutover): the Rust workspace. Prereleases are tags
  `2.0.0-alpha.N`, cut by hand; the only one so far is `2.0.0-alpha.1`. A prerelease makes no stability promise.
- **A version is a git tag.** The crate `version` fields read `0.0.0` and every crate has `publish = false`.
- **The wire protocol stays `v: 1`** across the 1.x to 2.x change: the implementation changed, the JSON contract did
  not. Section 4.5.
- **Compatibility rule (from `2.0.0` on).** Within a major version a release must drop into an existing front end
  without changing it. Additions are allowed. Removing, renaming or retyping anything, or changing a value or a meaning,
  needs a new major version. CI enforces this for the wire contract (golden fixtures must stay a superset of the
  baseline), the C header and its four exports, the persisted data files, and the public API of the C# wrapper
  (`dotnet pack` package validation against the newest published package). There is **no automatic Rust API diff
  yet**: for the Rust crates the rule is enforced by review. See `AGENTS.md`, "Backward compatibility".
- **Dependency pairing.** Each release was built and tested with one version of `calendar-connectors` (the release
  notes name it). In Rust, use the same `calendar-connectors` tag as the `timetug-shared` tag you depend on;
  otherwise cargo builds two copies of `calendar-core` and their types do not match.

## 3. Rust API

### 3.1 Install

Depend on a git tag. There is no crates.io release and none is promised. Requirements: edition 2024, Rust 1.99.1 or
newer (the repository pins 1.99.1 in `rust-toolchain.toml`).

```toml
[dependencies]
timetug-core = { git = "https://github.com/binary-companion/timetug-shared", tag = "2.0.0-alpha.1" }
calendar-core = { git = "https://github.com/binary-companion/calendar-connectors", tag = "2.0.0-alpha.1" }
```

(`cargo` resolved and compiled exactly this pair when this page was written; it builds one `calendar-core`.)
`calendar-core` is needed because Core's event type wraps the connector library's `CalendarEvent`.

### 3.2 The crates

| Crate | For | Main public items | Runtime needs |
|---|---|---|---|
| `timetug-core` | The pure rules: model, agenda, takeover scheduling, duplicate resolution, the store, the widget snapshot | `TimeTugCalendarEvent`, `CalendarInfo`, `TakeoverSettings`, `CalendarSource` (Core's own source trait), `CalendarStore`, `CalendarSnapshot`; modules `agenda::day_agenda` (`DayAgenda`), `takeover` (`Scheduler`, `TakeoverPolicy`, `TakeoverLedger`, `TakeoverGuard`, `TakeoverRequest`), `dedup` (`DuplicateResolver`, `DuplicateRules`, `LessonBook`, adjudication), `widget` (`WidgetSnapshot`, `WidgetTimeline`) | None for the pure functions; `CalendarStore` is `async` and uses channels (`tokio::sync`) |
| `timetug-bridge` | Adapts a connector-library source to Core's `CalendarSource` | `ConnectedSource`, `EventMapper` | Spawns one task per source to forward its changes: call inside a Tokio runtime |
| `timetug-engine` | The engine: accounts, settings, persistence, takeover timing, the command and event model | `Engine`, `EngineHost`, `EngineCommand`, `EngineEvent`, `EngineState`, `EngineSettings`, `EngineClock`, `SystemClock`, `SettingsStorage`, `TextGenerator` | A Tokio runtime provided by the caller (it owns no thread) |
| `timetug-engine-protocol` | The JSON contract v1 as types: envelopes, configuration, wire DTOs, command decoding, service messages | `Envelope`, `MessageCatalog`, `ProtocolVersion`, `WireConfig`, `CommandDecoder`, `ServiceMessages`, `wire_types` | None |
| `timetug-engine-c` | The C interface (section 5): builds the `TimeTugEngineC` library | the four `tt_engine_*` exports | Creates its own threads |
| `timetug-engine-ffi` | The UniFFI crate behind the Swift package `TimeTugKit` and the generated Kotlin (`docs/bindings.md`); not published as a Rust API | the typed `Engine` object, host interfaces, value mirrors and pure functions of the bindings | Creates its own runtime and threads |

Rules shared by all crates: `unsafe` is forbidden except in `timetug-engine-c` (the FFI layer, two files, each block
has a `SAFETY` comment); parsers return errors and never panic; no crate reads the clock except `timetug-engine`'s
`SystemClock` (everything else takes `now` as a parameter); the full item-level reference is the rustdoc
(`cargo doc --no-deps --workspace --open`), in which every public item is documented.

### 3.3 Example: agenda and next takeover

`timetug-core` is the part most worth reusing on its own. The example builds two meetings, asks for today's agenda and
for the next takeover, and shows the ledger that stops a takeover from firing twice. It is
`crates/timetug-core/examples/next_takeover.rs`; `cargo test` runs it (`tests/examples.rs`) and fails if this page
no longer shows it as it is. It reads no clock and needs no network.

```rust
//! Builds an agenda and picks the next takeover from a list of events. `timetug-core` is pure: it never reads the clock,
//! so the example passes `now` itself, and it needs no runtime and no network.
//!
//! Run it with `cargo run -p timetug-core --example next_takeover`.

#![allow(clippy::print_stdout, clippy::expect_used)]

use calendar_core::model::CalendarEvent;
use calendar_core::time::Zone;
use jiff::Timestamp;
use timetug_core::TimeTugCalendarEvent;
use timetug_core::agenda::day_agenda::{DayAgenda, State};
use timetug_core::model::takeover_settings::TakeoverSettings;
use timetug_core::takeover::ledger::TakeoverLedger;
use timetug_core::takeover::request::TakeoverRequest;
use timetug_core::takeover::scheduler::Scheduler;

fn at(text: &str) -> Timestamp {
    text.parse().expect("a valid instant")
}

/// A meeting with two attendees, as a connector would report it, wrapped for Core with the id of its source.
fn meeting(id: &str, title: &str, start: &str, end: &str) -> TimeTugCalendarEvent {
    let mut event = CalendarEvent::new(id, "work", title, at(start), at(end));
    event.url = calendar_core::url_string::UrlString::parse("https://meet.example.test/room");
    TimeTugCalendarEvent::new(event, "demo-account")
}

/// Runs the example (public so that the crate's test can call it).
pub fn main() {
    let zone = Zone::named("Europe/Berlin").expect("a zone every platform knows");
    let events = vec![
        meeting(
            "a",
            "Standup",
            "2026-09-18T07:00:00Z",
            "2026-09-18T07:15:00Z",
        ),
        meeting(
            "b",
            "Design review",
            "2026-09-18T12:00:00Z",
            "2026-09-18T13:00:00Z",
        ),
    ];
    // The defaults: takeover on, one minute of lead time, all-day events skipped. No calendar takes over until the
    // user turns it on; the key of a calendar is "<source id>/<calendar id>".
    let mut settings = TakeoverSettings::default();
    settings.set_takeover(true, "demo-account/work");
    let now = at("2026-09-18T07:05:00Z");

    // Today's agenda: each event is Past, Current or Upcoming at `now`.
    let agenda = DayAgenda::make(&events, &settings, now, &zone);
    assert_eq!(agenda.items.len(), 2);
    assert_eq!(agenda.items[0].state, State::Current);
    assert_eq!(agenda.items[1].state, State::Upcoming);

    // The next takeover to arm. The first event is already running, so it fires at once; after it has fired the
    // ledger skips it and the review is next, one lead time before it starts.
    let mut ledger = TakeoverLedger::new();
    let first = Scheduler::next(&events, &settings, &ledger, now).expect("something to arm");
    assert_eq!(first.event.title(), "Standup");
    assert_eq!(first.fire_at, now);
    ledger.mark_fired(&first.event, now);

    let second = Scheduler::next(&events, &settings, &ledger, now).expect("something to arm");
    assert_eq!(second.event.title(), "Design review");
    assert_eq!(second.fire_at, at("2026-09-18T11:59:00Z"));

    // What a front end needs to draw the takeover: plain data, no display strings.
    let request = TakeoverRequest::make(&second.event, second.fire_at);
    assert!(!request.has_started);
    println!("{} at {}", request.event.title(), second.fire_at);
}
```

A takeover only arms for calendars the user switched on (`settings.set_takeover(true, "<source id>/<calendar id>")`),
which is why the example does that first.

### 3.4 Errors, threading and async

- **Errors.** `timetug-core` reuses the connector library's `Failure` and `Outcome` (see its reference): a source
  failure is `Failure::Source`, cancellation is `Failure::Cancelled`. Core's `SourceError` separates the two actionable
  provider errors, `AuthExpired` and `Network`, from everything else. A failing source never discards the last good
  events: `CalendarStore` keeps them and records the source's `SourceStatus`.
- **Threading.** Every trait is `Send + Sync`. `CalendarStore` is shared by `&self`; its state sits behind one mutex that
  an operation takes for a short step (the code calls each step an "actor turn"), and overlapping refreshes are
  supported. A refresh whose sources,
  zone or window changed while it was running discards its results instead of publishing them.
- **Async.** Async methods are plain futures; a source hands out its changes as a Tokio unbounded-channel receiver.
  `ConnectedSource` and `Engine::new` spawn tasks and need a Tokio runtime to be current.
- **Time.** `now: jiff::Timestamp` is always an argument. A time zone is a `calendar_core::time::Zone`; the engine uses
  only the Gregorian calendar and a zone identifier from the host.

### 3.5 Embedding the engine from Rust

`timetug_engine::Engine::new(EngineHost)` returns an `Arc<Engine>`; `start().await`, `send(command).await`,
`states()` (a `watch` receiver, newest state wins), `events()` (an unbounded receiver) and `current_state()` drive it.
`EngineHost` holds the host services: settings storage, credential store, sign-in interaction, connector registry,
optional text generator, diagnostics sink, clock and zone. The C interface (`timetug-engine-c`) is the one embedding
this repository ships and tests; it wraps `Engine` with a current-thread runtime, a command intake and the delivery
queue of section 5.4. There is no separate runnable Rust embedding example: to see one, read
`crates/timetug-engine-c/src/engine_session.rs`.

## 4. The engine and its protocol

The engine does the work: it owns the accounts, reads calendars through the connector library, resolves duplicates,
decides when a takeover is due, and publishes state. The **host** (the app) owns everything with a user interface or
a secret: HTTP, credential storage, sign-in windows, optional on-device text generation, and the display of
takeovers. They talk in JSON messages.

### 4.1 Message envelope

Every message, in both directions, is one JSON object:

```json
{"v":1,"type":"settings.apply","id":"cmd-7","payload":{"leadTime":120}}
```

`v` is the protocol version, `type` names the message, `id` is optional on commands (a number or a string; it comes back
on any diagnostic the command causes) and is the **request id** on `service.*` requests, `payload` depends on `type`.
Decoders ignore unknown members, so additions are not breaking. Timestamps are ISO 8601 UTC with milliseconds
(`2025-10-09T09:38:20.000Z`). One golden example of every message is under `Conformance/Fixtures/golden/` (and in the
NuGet package); the end-to-end scripts are `Conformance/Scenarios/`.

### 4.2 Configuration (`tt_engine_create`)

```json
{"v":1,"dataDirectory":"C:\\Users\\me\\AppData\\Local\\MyApp\\engine","platform":"windows",
 "connectors":{"google":{"clientID":"...","clientSecret":"..."},"microsoft":{"clientID":"..."}},
 "textGeneration":false,"externalTokens":false,"timeZone":"Europe/Berlin"}
```

| Member | Meaning |
|---|---|
| `v` | Required. Must be a supported protocol version (`1`). |
| `dataDirectory` | Required, not empty. The engine keeps its settings, accounts, ledger and duplicate state here (one engine per directory) and creates it if needed. |
| `platform` | Required: `windows`, `linux` or `macos`. Selects the account kinds on offer. |
| `connectors` | OAuth client registrations by family (`google` with optional `clientSecret`; `microsoft` without). A kind without a client is listed as not configured. Default: none. |
| `textGeneration` | `true` when the host answers `service.text.*`; lets the engine use the host's text model to judge duplicates. Default `false`. |
| `externalTokens` | `true` when the host answers `service.token.*` (access tokens from the operating system's account service). Enables the `microsoft-brokered` account kind. Default `false`. |
| `timeZone` | An IANA zone id; default: the process zone. Later changes are sent as `system.event`. |
| `testing`, `testClockStart` | Test mode: a manual clock moved by `test.advanceClock`. Not for production. |

### 4.3 Messages from the host (commands)

| `type` | Payload | Effect |
|---|---|---|
| `start` | none | Starts the engine. The first message back is `ready`. Nothing is delivered before it. |
| `refresh` | none | Re-reads all sources. |
| `account.add` | `{"kindID": "google"}` | Starts adding an account of a kind listed in `availableAccountKinds`. May cause `service.auth.*` or `service.signIn.*` requests. |
| `account.reconnect` / `account.remove` | `{"connectionID": "..."}` | Repairs sign-in / removes the account and its secrets. |
| `account.cancel` | none | Cancels a sign-in in progress. Acts at once. |
| `settings.apply` | a patch: `leadTime` (seconds), `takeoverCalendarKeys`, `hiddenCalendarKeys`, `requireConferenceLink`, `requireOtherAttendees`, `skipAllDayEvents`, `enabled`, `inferenceEnabled`, `systemCalendarEnabled`; a member that is absent leaves that setting unchanged, a key list replaces the stored list | Applies and persists settings. |
| `countdown.visible` | `{"visible": true}` | Tells the engine the countdown UI is showing. |
| `takeover.presented` | `{"eventID": "...", "onScreen": true}` | The host showed the takeover (`onScreen: false`: it fell back to a toast). |
| `takeover.closed` / `takeover.join` / `takeover.dismiss` | `{"eventID": "..."}` | The user closed it / chose Join / dismissed it. |
| `takeover.snooze` | `{"eventID": "...", "seconds": 300}` | Snoozes. |
| `takeover.test` | `{"title": "...", "calendarTitle": "..."}` | Fires a test takeover. |
| `dedup.merge` | `{"eventIDs": ["a", "b"]}` (exactly two) | Merges two events by hand. |
| `dedup.unmerge` | `{"eventID": "..."}` | Undoes a merge. |
| `dedup.separate` | `{"eventID": "...", "memberIDs": ["..."]}` | Splits members off a merged event. |
| `dedup.forget` | none | Forgets learned corrections. |
| `system.event` | `{"kind": "wake" \| "clockChanged" \| "timeZoneChanged" \| "dayChanged" \| "networkReturned" \| "displayChanged", "timeZone": "..."}` | Tells the engine about the system; `timeZone` goes with `timeZoneChanged` only. |
| `test.advanceClock` | `{"seconds": 60}` | Moves the manual clock (only when created with `testing`). |

Commands in the same group (accounts, settings, takeover, dedup, countdown, general) reach the engine one after another
in the order sent; groups do not wait for each other, so a sign-in waiting for a person never delays a takeover.

### 4.4 Messages from the engine

| `type` | Payload | Delivery |
|---|---|---|
| `ready` | `engineVersion`, `protocolVersions` (`[1]`), `platform`, `capabilities` (`systemCalendar`, `textGeneration`, `externalTokens` when true) | First, after `start`. |
| `state` | The whole picture: `revision`, `agenda`, `events`, `calendars`, `statuses`, `notices`, `sourceNames`, `candidates`, `accounts`, `accountStatus`, `settings`, `inferenceStatus`, `ledger`, `nextTakeover`, `takeoverVisible`, `availableAccountKinds` | **Coalesced**: if the host is slow, an unread state is replaced by the newer one. `revision` only grows. |
| `event.takeoverDue` | The event, `hasStarted`, `joinURL`, `snoozeOptions`, `calendarTitle` | Never dropped. Show a takeover. |
| `event.takeoverWithdrawn` | `{"eventID"}` | Never dropped. The meeting changed or ended: hide it. |
| `event.widgetSnapshot` | Events for the next days for widgets | Never dropped. |
| `event.joinRequested` | `{"url"}` | Never dropped. Open the link. |
| `event.diagnostic` | `level`, `category`, `message`; the command's `id` is echoed in the envelope | Never dropped. For logs and for errors in your commands (4.6). Messages are written to hold no calendar content, tokens or addresses. |
| `service.*` | See 4.5 | Never dropped. Must be answered. |

### 4.5 Host services (`service.*`)

The engine asks the host for everything it cannot do itself, with a request carrying a fresh numeric `id`. The host
answers once with `tt_engine_reply(engine, id, reply)`, where the reply is `{"v":1,"result":{...}}` or
`{"v":1,"error":{"kind":"network|cancelled|notFound|denied|failed","message":"..."}}`. Requests may be answered in any
order, from any thread.

| Request | Payload | `result` |
|---|---|---|
| `service.credentials.get` | `connectionID` | `{"secrets":{...}}`, or `{}` when none |
| `service.credentials.set` | `connectionID`, `secrets` | `{}` |
| `service.credentials.remove` | `connectionID` | `{}` |
| `service.credentials.snapshot` | `connectionID` | `{"snapshot":{"secrets":{...},"revision":"..."}}` |
| `service.credentials.updateRefreshToken` | `connectionID`, `token`, `expectedRevision` | `{"updated": true\|false}` (write only if the revision still matches) |
| `service.http.send` | `method`, `url`, `headers`, `body` (base64) | `{"status":200,"headers":{...},"body":"<base64>"}`; 4xx and 5xx are results, not errors; a transport failure is error kind `network`. The engine times a request out after 60 seconds. |
| `service.auth.beginRedirect` | none | `{"sessionID":"...","redirectURI":"..."}` (start listening for the OAuth redirect) |
| `service.auth.authorize` | `sessionID`, `url` | `{"url":"<the redirect URL received>"}` (open the browser; no timeout, a person is involved) |
| `service.auth.closeRedirect` | `sessionID` | `{}` |
| `service.auth.promptCredentials` | `fields` (`key`, `label`, `isSecret`) | `{"values":{...}}` |
| `service.signIn.prepare` | `title`, `help`, `values`, `error` | `{}` |
| `service.signIn.lastNonSecretValues` | none | `{"values":{...}}` or `{}` |
| `service.text.availability` | none | `{"kind":"available\|unavailable\|temporarilyUnavailable", ...}` |
| `service.text.generate` | `instructions`, `prompt` | `{"text":"..."}` |
| `service.token.get` | `connectionID`, `accountID`, `email`, `scopes`, `forceRefresh` | `{"status":"ok\|interactionRequired\|unavailable","accessToken":"...","expiresAt":"...","reason":"..."}`; engine gives up after 60 s |
| `service.token.signIn` / `service.token.cancelSignIn` | `signInID`, `scopes`, ... / `signInID` | sign-in result / `{}` |

Only the services the configuration enables are used: with `textGeneration` false the engine never sends
`service.text.*`; with `externalTokens` false never `service.token.*`. A reply to an unknown or already answered id is
ignored. Never put tokens or passwords in an `error.message`: the engine treats it as text it may log.

### 4.6 Errors

There are no numeric error codes. A failure reaches the host in one of four ways:

1. `tt_engine_create` returns `NULL` and makes no callback: the configuration is not JSON of the expected shape, `v` is
   not supported, `dataDirectory` is empty or cannot be created, `platform` or `timeZone` is unknown, or the
   callback is `NULL`.
2. A command the engine cannot use produces an `event.diagnostic` with `category: "command"` and the command's `id`:
   `invalid JSON`, `unknown command type`, `unsupported protocol version`, `invalid payload`, `unknown event id`,
   `unknown member id`, `unknown system event kind`, `unknown time zone`. The engine carries on.
3. A source that cannot be read appears in `state.statuses` (per source) and `state.accountStatus.buildFailures`; the
   last good events stay visible.
4. A host service the host cannot perform: reply with `error.kind` (`network`, `cancelled`, `notFound`, `denied`,
   `failed`). `cancelled` means the person or the host abandoned it.

### 4.7 Protocol versioning

`v` is `1` in 1.x and 2.x. `ready.protocolVersions` lists the versions the engine speaks. A command with an unsupported
`v` is answered with a diagnostic (`unsupported protocol version`) and ignored. Within a version, adding members to
objects and adding message types is allowed and decoders must ignore what they do not know; removing, renaming or
retyping a member is not (CI checks the golden fixtures as a superset of the previous release's).

## 5. C reference

Header: `include/timetug_engine.h` (also in the NuGet package). Library: `TimeTugEngineC.dll`,
`libTimeTugEngineC.dylib` or `libTimeTugEngineC.so`, built with `cargo build --release -p timetug-engine-c`
(`target/release`; the crate type is `cdylib` and `staticlib`). The library exports exactly four functions
(`c-smoke/run.sh` checks this on every build).

### 5.1 Functions

```c
typedef struct tt_engine tt_engine;
typedef void (*tt_message_fn)(void *context, const char *message_json);

tt_engine *tt_engine_create(const char *config_json, tt_message_fn on_message, void *context);
void tt_engine_send(tt_engine *engine, const char *command_json);
void tt_engine_reply(tt_engine *engine, uint64_t request_id, const char *reply_json);
void tt_engine_destroy(tt_engine *engine);
```

| Function | Does | Returns |
|---|---|---|
| `tt_engine_create` | Validates the configuration (4.2), creates the data directory, builds an engine that is **not started**. | A handle, or `NULL` (no callback is made) for any invalid configuration or a `NULL` callback. |
| `tt_engine_send` | Queues one command (4.3). Returns at once. | nothing |
| `tt_engine_reply` | Answers the service request `request_id` (4.5). Returns at once. | nothing |
| `tt_engine_destroy` | Stops and releases the engine; pending service requests fail as `cancelled`. | nothing |

### 5.2 Ownership and lifetimes

- All strings are NUL-terminated UTF-8 JSON. Invalid UTF-8 sent to the engine is read with replacement characters.
- The engine **copies** what it keeps from `config_json`, `command_json` and `reply_json` before the function returns:
  you may free or reuse them at once.
- The `message_json` passed to your callback is valid **only during that call**. Copy what you keep.
- The engine never frees, keeps or returns host memory, and the host never frees engine memory. That is what makes a
  statically linked C runtime safe (the Windows DLL needs no redistributable, 6.5).
- A handle is an opaque token, not a pointer into engine memory. `NULL`, destroyed and unknown handles are ignored by
  every function; so are replies for unknown or already answered request ids. A second `tt_engine_destroy` of the same
  handle is harmless.
- `context` is yours and is passed back unchanged. It must stay valid until `tt_engine_destroy` has returned (or, when
  `destroy` was called inside a callback, until that callback returns).

### 5.3 Threading

- `tt_engine_send` and `tt_engine_reply` never block and may be called from **any thread, including from inside
  `on_message`**.
- The engine runs on its own thread (a Tokio current-thread runtime) and delivers on a second dedicated thread.
  `on_message` is therefore called on an engine thread, never on yours, and your `context` must be safe to use from it.
- Callbacks are **serialized and ordered**: one at a time, in the order the engine produced the messages.
- Because they never overlap, a slow callback delays later messages but never loses them (next section).

### 5.4 Delivery rules

- **States coalesce:** a new `state` replaces one still waiting, so you always get the newest, never one older than an
  event queued before it.
- **Events and service requests are never dropped or reordered** (the queue is unbounded).
- Nothing is delivered until you send `start`; the first message is then `ready`.
- Do slow work (UI, disk) off the callback thread, or accept that delivery waits.

### 5.5 Shutdown

- `tt_engine_destroy` called **outside** a callback returns only when no callback is running, and none runs after it
  returns. It waits up to ten seconds for the engine thread to end.
- Called **inside** a callback it returns at once and no further callback starts after the current one returns.
- Release builds are built with `panic = "abort"`: a bug that panics ends the process rather than unwinding into your
  code. No code relies on unwinding at the boundary.

### 5.6 Example

`c-smoke/example.c` creates an engine, reads `ready` and `state`, shows how to answer a service request, sends a command,
triggers a diagnostic with a bad command, and shuts down. `c-smoke/run.sh` (macOS and Linux) compiles it with `-Wall
-Wextra -Werror` against the header and runs it on every CI build, and fails if this page no longer shows the file as it
is.

```c
typedef struct tt_engine tt_engine;
typedef void (*tt_message_fn)(void *context, const char *message_json);

tt_engine *tt_engine_create(const char *config_json, tt_message_fn on_message, void *context);
void tt_engine_send(tt_engine *engine, const char *command_json);
void tt_engine_reply(tt_engine *engine, uint64_t request_id, const char *reply_json);
void tt_engine_destroy(tt_engine *engine);
```

| Function | Does | Returns |
|---|---|---|
| `tt_engine_create` | Validates the configuration (4.2), creates the data directory, builds an engine that is **not started**. | A handle, or `NULL` (no callback is made) for any invalid configuration or a `NULL` callback. |
| `tt_engine_send` | Queues one command (4.3). Returns at once. | nothing |
| `tt_engine_reply` | Answers the service request `request_id` (4.5). Returns at once. | nothing |
| `tt_engine_destroy` | Stops and releases the engine; pending service requests fail as `cancelled`. | nothing |

### 5.2 Ownership and lifetimes

- All strings are NUL-terminated UTF-8 JSON. Invalid UTF-8 sent to the engine is read with replacement characters.
- The engine **copies** what it keeps from `config_json`, `command_json` and `reply_json` before the function returns:
  you may free or reuse them at once.
- The `message_json` passed to your callback is valid **only during that call**. Copy what you keep.
- The engine never frees, keeps or returns host memory, and the host never frees engine memory. That is what makes a
  statically linked C runtime safe (the Windows DLL needs no redistributable, 6.5).
- A handle is an opaque token, not a pointer into engine memory. `NULL`, destroyed and unknown handles are ignored by
  every function; so are replies for unknown or already answered request ids. A second `tt_engine_destroy` of the same
  handle is harmless.
- `context` is yours and is passed back unchanged. It must stay valid until `tt_engine_destroy` has returned (or, when
  `destroy` was called inside a callback, until that callback returns).

### 5.3 Threading

- `tt_engine_send` and `tt_engine_reply` never block and may be called from **any thread, including from inside
  `on_message`**.
- The engine runs on its own thread (a Tokio current-thread runtime) and delivers on a second dedicated thread.
  `on_message` is therefore called on an engine thread, never on yours, and your `context` must be safe to use from it.
- Callbacks are **serialized and ordered**: one at a time, in the order the engine produced the messages.
- Because they never overlap, a slow callback delays later messages but never loses them (next section).

### 5.4 Delivery rules

- **States coalesce:** a new `state` replaces one still waiting, so you always get the newest, never one older than an
  event queued before it.
- **Events and service requests are never dropped or reordered** (the queue is unbounded).
- Nothing is delivered until you send `start`; the first message is then `ready`.
- Do slow work (UI, disk) off the callback thread, or accept that delivery waits.

### 5.5 Shutdown

- `tt_engine_destroy` called **outside** a callback returns only when no callback is running, and none runs after it
  returns. It waits up to ten seconds for the engine thread to end.
- Called **inside** a callback it returns at once and no further callback starts after the current one returns.
- Release builds are built with `panic = "abort"`: a bug that panics ends the process rather than unwinding into your
  code. No code relies on unwinding at the boundary.

### 5.6 Example

`c-smoke/example.c` creates an engine, reads `ready` and `state`, answers a service request, sends a command, triggers a
diagnostic with a bad command, and shuts down. `c-smoke/run.sh` (macOS and Linux) compiles it with `-Wall -Wextra
-Werror` against the header and runs it on every CI build; this page shows the file as it is (a test-free check:
`scripts/ci/check-doc-examples.py` is not needed because `c-smoke/run.sh` and the C# test fail when the text differs? no,
see below).

```c
/*
 * The example in docs/api.md, part "C interface": create an engine, start it, read `ready` and `state`, answer a
 * host-service request, send a command, and shut down. Built and run by c-smoke/run.sh (macOS and Linux) against
 * include/timetug_engine.h and the TimeTugEngineC library only.
 * Usage: example <data-directory>
 *
 * The callback runs on an engine thread, so everything it touches is atomic. A real host parses the JSON with a JSON
 * library; this example only looks for substrings so that it needs nothing else.
 */
#define _POSIX_C_SOURCE 200809L

#include <inttypes.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "timetug_engine.h"

typedef struct {
    tt_engine *volatile engine; /* set after tt_engine_create returns; the callback may run before that */
    atomic_int ready;
    atomic_int states;
    atomic_int diagnostics;
} host;

static void on_message(void *context, const char *json) {
    host *h = context;
    /* `json` is valid only during this call: copy anything you keep. */
    if (strstr(json, "\"type\":\"ready\"")) atomic_store(&h->ready, 1);
    if (strstr(json, "\"type\":\"state\"")) atomic_fetch_add(&h->states, 1);
    if (strstr(json, "\"type\":\"event.diagnostic\"")) atomic_fetch_add(&h->diagnostics, 1);

    /* A service request: {"v":1,"type":"service.xxx","id":<n>,"payload":{...}}. `id` precedes `payload`, so the
     * first "id": is the request id. Every request must be answered exactly once; this host has no services. */
    if (strstr(json, "\"type\":\"service.")) {
        const char *id = strstr(json, "\"id\":");
        if (id && h->engine) {
            uint64_t request_id = strtoull(id + 5, NULL, 10);
            /* Allowed from inside the callback: reply never blocks. */
            tt_engine_reply(h->engine, request_id,
                            "{\"v\":1,\"error\":{\"kind\":\"notFound\",\"message\":\"this host has no services\"}}");
        }
    }
}

static int wait_for(atomic_int *flag, int minimum) {
    struct timespec pause = {0, 25 * 1000 * 1000};
    for (int i = 0; i < 400; i++) { /* at most ten seconds */
        if (atomic_load(flag) >= minimum) return 1;
        nanosleep(&pause, NULL);
    }
    return 0;
}

int main(int argc, char **argv) {
    const char *directory = argc > 1 ? argv[1] : ".";
#if defined(__APPLE__)
    const char *platform = "macos";
#else
    const char *platform = "linux";
#endif
    char config[1400];
    /* Escape the directory properly in real code (backslashes and quotes); the example's path has neither. */
    snprintf(config, sizeof config, "{\"v\":1,\"dataDirectory\":\"%s\",\"platform\":\"%s\"}", directory, platform);

    host h = {0};
    tt_engine *engine = tt_engine_create(config, on_message, &h); /* NULL: the config was rejected */
    if (!engine) { fprintf(stderr, "the engine rejected the configuration\n"); return 1; }
    h.engine = engine;

    tt_engine_send(engine, "{\"v\":1,\"type\":\"start\"}"); /* nothing is delivered before this */
    if (!wait_for(&h.ready, 1) || !wait_for(&h.states, 1)) { fprintf(stderr, "no ready/state\n"); tt_engine_destroy(engine); return 2; }

    int seen = atomic_load(&h.states);
    tt_engine_send(engine, "{\"v\":1,\"type\":\"refresh\",\"id\":\"r1\"}");
    tt_engine_send(engine, "{\"v\":1,\"type\":\"frobnicate\",\"id\":\"bad\"}"); /* an event.diagnostic comes back */
    if (!wait_for(&h.states, seen + 1) || !wait_for(&h.diagnostics, 1)) { fprintf(stderr, "no answer\n"); tt_engine_destroy(engine); return 3; }

    tt_engine_destroy(engine); /* returns when no callback is running; none runs after it */
    printf("example: ok (%d states, %d diagnostics)\n", atomic_load(&h.states), atomic_load(&h.diagnostics));
    return 0;
}
```

Build and run it: `c-smoke/run.sh` (it builds the library first).

## 6. C# on Windows

### 6.1 Install

```powershell
dotnet add package TimeTug.Engine --version 2.0.0-alpha.1
```

`2.0.0-alpha.1` is a prerelease: nuget.org hides it unless you ask (`--prerelease`, or the exact version as above).
Versions `1.6.2` to `1.8.4` are earlier releases of the Swift engine, with a much larger native payload (6.5); the C# API only grew additively. Prerelease
packages are for evaluation; do not ship a product on one.

Prerequisite, as the package states it: the package targets **.NET 8** (`net8.0`) and has no other package
dependencies. It contains `lib/net8.0/TimeTug.Engine.dll` and its XML documentation (IntelliSense), the native engine
for `win-x64` and `win-arm64`, the C header, the third-party notices and the golden fixtures. The package is signed.

### 6.2 Public API

Namespace `TimeTug.Engine`.

| Type | Role |
|---|---|
| `EngineFactory.Create(EngineOptions)` | Creates an engine (not started). Throws `EngineConfigurationException` when the engine rejects the configuration. |
| `EngineOptions` | `DataDirectory` (required), `Host` (required), `Platform` (default `windows`), `OAuthClients` (by family `google`, `microsoft`), `TimeZone`, `Dispatcher`, `Testing`, `TestClockStart`. |
| `IEngine : IAsyncDisposable` | `State` (`IAsyncEnumerable<EngineState>`), `Events` (`IAsyncEnumerable<EngineEvent>`), `SendAsync(EngineCommand, string? id = null)`. |
| `EngineCommand` | Records: `Start`, `Refresh`, `AddAccount`, `ReconnectAccount`, `RemoveAccount`, `CancelAuthorization`, `ApplySettings`, `SetCountdownVisible`, `TakeoverPresented`, `TakeoverClosed`, `Join`, `Dismiss`, `Snooze`, `TestTakeover`, `Merge`, `Unmerge`, `Separate`, `ForgetCorrections`, `SystemEvent`, `AdvanceTestClock`. |
| `EngineEvent` | `Ready`, `TakeoverDue`, `TakeoverWithdrawn`, `WidgetSnapshotUpdated`, `JoinRequested`, `Diagnostic`. |
| `EngineState` and its parts | `Revision`, `Agenda`, `Events`, `Calendars`, `Statuses`, `Notices`, `SourceNames`, `Candidates`, `Accounts`, `AccountStatus`, `Settings`, `InferenceStatus`, `Ledger`, `NextTakeover`, `TakeoverVisible`, `AvailableAccountKinds`. |
| `IEngineHost` | The services you supply: `Credentials` (`ICredentialStore`), `Http` (`IHttpTransport`), `Authorization` (`IAuthorizationInteraction`), `TextGenerator` (`ITextGenerator?`, may be `null`), `Diagnostic(EngineDiagnostic)`. |
| `IExternalTokenHost` | Optional: implement it on your host to supply Microsoft access tokens from Windows' account service; the engine then offers the `microsoft-brokered` account kind. |
| `EngineServiceException` + `ServiceErrorKind` | Throw from a host service to fail a request with `Network`, `Cancelled`, `NotFound`, `Denied` or `Failed`. |
| `SystemEventKind` | Constants for `EngineCommand.SystemEvent.Kind`: `Wake`, `ClockChanged`, `TimeZoneChanged`, `DayChanged`, `NetworkReturned`, `DisplayChanged`. |

Behaviour of the wrapper:

- `State` keeps only the newest state (a bounded channel that drops the oldest); `Events` is unbounded and ordered. Each
  can have **one reader at a time** and ends when the engine is disposed.
- `SendAsync` never blocks. `id` comes back as `EngineDiagnostic.CommandId` on any diagnostic the command causes.
- Host services are called on the engine's delivery thread, or through `EngineOptions.Dispatcher` when given (for
  example to marshal to a UI thread). A method may complete synchronously or later; the cancellation token you receive
  is cancelled when the engine is disposed.
- An exception from a host service becomes an error reply: `EngineServiceException` keeps its kind,
  `OperationCanceledException` is `Cancelled`, `HttpRequestException` is `Network`, anything else is `Failed`. For
  `service.token.*` the text of an exception is never passed on, because it could carry account data.
- `DisposeAsync` stops the engine once (later calls return the same task); outside a callback it completes when
  `tt_engine_destroy` has returned, after which no callback runs. It is safe to call from inside a host callback.
- Problems in the wrapper itself (a malformed message) are reported through `IEngineHost.Diagnostic` with category
  `wrapper`; a host sink that throws does not stop delivery.
- The native library is found, in order, through the environment variable `TIMETUG_ENGINE_LIBRARY` (a file or the
  directory holding it), the application directory, `runtimes/<rid>/native` under it, then the runtime's default
  probing, which covers the package's native assets in an application's `deps.json`.

### 6.3 Complete example

A console program that creates the engine, supplies configuration and host services (an in-memory credential store, an
`HttpClient` transport, and a host with no sign-in window), starts it, reads `ready` and the account kinds from the
first useful state, changes a setting, reads the resulting state and shuts down through `await using`. It is
`dotnet/TimeTug.Engine.Example/Program.cs`; the test `ExampleTests` runs it against the real native library and fails
if this page no longer shows the file as it is.

```csharp
using System.Net;
using TimeTug.Engine;

namespace TimeTug.Engine.Example;

/// <summary>Starts the TimeTug engine, changes a setting, reads the state it publishes and shuts the engine down.</summary>
public static class EngineExample
{
    /// <summary>Runs the example against <paramref name="dataDirectory"/> and returns the lead time the engine reports
    /// after the settings change (120 seconds).</summary>
    public static async Task<double> RunAsync(string dataDirectory, TimeSpan timeout)
    {
        using var timeoutSource = new CancellationTokenSource(timeout);
        var cancellation = timeoutSource.Token;

        // 1. Configuration. The engine keeps its files in DataDirectory (one engine per directory). OAuth clients are
        //    your own registrations with Google and Microsoft; without one, that account kind is listed as not
        //    configured. Host supplies the services the engine asks for.
        var options = new EngineOptions
        {
            DataDirectory = dataDirectory,
            Platform = "windows",
            TimeZone = "Europe/Berlin", // omit to use the process time zone
            OAuthClients = new Dictionary<string, OAuthClient>
            {
                // ["google"] = new OAuthClient("<your client id>", "<your client secret>"),
                // ["microsoft"] = new OAuthClient("<your client id>"),
            },
            Host = new ExampleHost(),
        };

        // 2. Create, then start. Create throws EngineConfigurationException for a configuration the engine rejects.
        //    DisposeAsync (here through "await using") stops the engine; no callback runs after it completes.
        await using var engine = EngineFactory.Create(options);
        await engine.SendAsync(new EngineCommand.Start());

        // 3. The first event is Ready. Events arrive in order and are never dropped.
        await foreach (var engineEvent in engine.Events.WithCancellation(cancellation))
        {
            if (engineEvent is EngineEvent.Ready ready)
            {
                Console.WriteLine($"engine {ready.Info.EngineVersion} on {ready.Info.Platform}, protocol {string.Join(",", ready.Info.ProtocolVersions)}");
                break;
            }
        }

        // 4. States are the whole picture each time; a slow reader skips the intermediate ones (the newest wins).
        //    Wait for the one that lists the account kinds this platform offers.
        EngineState state = await FirstStateAsync(engine, s => s.AvailableAccountKinds.Count > 0, cancellation);
        foreach (var kind in state.AvailableAccountKinds)
        {
            Console.WriteLine($"account kind {kind.Id}: {kind.DisplayName} (configured: {kind.IsConfigured})");
        }

        // 5. Commands never block. A settings patch changes only the members you set. The id comes back on any
        //    diagnostic the command causes.
        await engine.SendAsync(new EngineCommand.ApplySettings { LeadTime = 120 }, id: "lead-time");
        state = await FirstStateAsync(engine, s => s.Settings.Takeover.LeadTime == 120, cancellation);
        Console.WriteLine($"lead time is now {state.Settings.Takeover.LeadTime} seconds");

        return state.Settings.Takeover.LeadTime;
    }

    private static async Task<EngineState> FirstStateAsync(IEngine engine, Func<EngineState, bool> wanted, CancellationToken cancellation)
    {
        await foreach (var state in engine.State.WithCancellation(cancellation))
        {
            if (wanted(state)) return state;
        }
        throw new InvalidOperationException("the engine stopped before the state arrived");
    }
}

/// <summary>The host services. This host keeps secrets in memory (a real one uses the Windows Credential Locker), sends
/// HTTP with <see cref="HttpClient"/>, and has no sign-in window, so it refuses sign-in requests.</summary>
internal sealed class ExampleHost : IEngineHost
{
    public ICredentialStore Credentials { get; } = new MemoryCredentials();
    public IHttpTransport Http { get; } = new HttpClientTransport();
    public IAuthorizationInteraction Authorization { get; } = new NoSignIn();
    public ITextGenerator? TextGenerator => null; // no on-device text model: the engine will not ask

    public void Diagnostic(EngineDiagnostic diagnostic) =>
        Console.WriteLine($"[{diagnostic.Level}] {diagnostic.Category}: {diagnostic.Message}");
}

internal sealed class MemoryCredentials : ICredentialStore
{
    private readonly Dictionary<string, (IReadOnlyDictionary<string, string> Secrets, string Revision)> _items = new();
    private readonly object _gate = new();

    public ValueTask<IReadOnlyDictionary<string, string>?> GetAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_gate) return new(_items.TryGetValue(connectionID, out var item) ? item.Secrets : null);
    }

    public ValueTask SetAsync(string connectionID, IReadOnlyDictionary<string, string> secrets, CancellationToken cancellationToken)
    {
        lock (_gate) _items[connectionID] = (secrets, Guid.NewGuid().ToString("N"));
        return ValueTask.CompletedTask;
    }

    public ValueTask RemoveAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_gate) _items.Remove(connectionID);
        return ValueTask.CompletedTask;
    }

    public ValueTask<CredentialSnapshot?> SnapshotAsync(string connectionID, CancellationToken cancellationToken)
    {
        lock (_gate)
        {
            return new(_items.TryGetValue(connectionID, out var item) ? new CredentialSnapshot(item.Secrets, item.Revision) : null);
        }
    }

    public ValueTask<bool> UpdateRefreshTokenAsync(string connectionID, string token, string expectedRevision, CancellationToken cancellationToken)
    {
        lock (_gate)
        {
            if (!_items.TryGetValue(connectionID, out var item) || item.Revision != expectedRevision) return new(false);
            var secrets = new Dictionary<string, string>(item.Secrets) { ["refresh_token"] = token };
            _items[connectionID] = (secrets, Guid.NewGuid().ToString("N"));
            return new(true);
        }
    }
}

/// <summary>HTTP for the connectors. 4xx and 5xx are results, not errors. Redirects are not followed (the engine checks
/// each hop for CalDAV and iCal links before it sends credentials), and nothing is cached or kept in cookies.</summary>
internal sealed class HttpClientTransport : IHttpTransport
{
    private static readonly HttpClient Client = new(new SocketsHttpHandler
    {
        AllowAutoRedirect = false,
        UseCookies = false,
        AutomaticDecompression = DecompressionMethods.All,
    })
    {
        Timeout = TimeSpan.FromSeconds(60),
    };

    public async ValueTask<EngineHttpResponse> SendAsync(EngineHttpRequest request, CancellationToken cancellationToken)
    {
        try
        {
            using var message = new HttpRequestMessage(new HttpMethod(request.Method), request.Url);
            if (request.Body is not null) message.Content = new ByteArrayContent(request.Body);
            foreach (var (name, value) in request.Headers)
            {
                // Content headers (Content-Type, ...) belong to the content, the rest to the message.
                if (!message.Headers.TryAddWithoutValidation(name, value)) message.Content?.Headers.TryAddWithoutValidation(name, value);
            }
            using var response = await Client.SendAsync(message, cancellationToken).ConfigureAwait(false);
            var headers = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            foreach (var header in response.Headers) headers[header.Key] = string.Join(", ", header.Value);
            foreach (var header in response.Content.Headers) headers[header.Key] = string.Join(", ", header.Value);
            var body = await response.Content.ReadAsByteArrayAsync(cancellationToken).ConfigureAwait(false);
            return new EngineHttpResponse((int)response.StatusCode, headers, body);
        }
        catch (HttpRequestException exception)
        {
            // A transport failure is Network. Keep addresses and tokens out of the message.
            throw new EngineServiceException(ServiceErrorKind.Network, "the request did not complete", exception);
        }
    }
}

/// <summary>A host without sign-in windows: every sign-in request is refused as <see cref="ServiceErrorKind.Denied"/>.</summary>
internal sealed class NoSignIn : IAuthorizationInteraction
{
    private static EngineServiceException Refuse() => new(ServiceErrorKind.Denied, "this host cannot sign in");

    public ValueTask<RedirectSession> BeginRedirectAsync(CancellationToken cancellationToken) => throw Refuse();
    public ValueTask<string> AuthorizeAsync(string sessionID, string url, CancellationToken cancellationToken) => throw Refuse();
    public ValueTask CloseRedirectAsync(string sessionID, CancellationToken cancellationToken) => ValueTask.CompletedTask;
    public ValueTask<IReadOnlyDictionary<string, string>> PromptCredentialsAsync(IReadOnlyList<CredentialField> fields, CancellationToken cancellationToken) => throw Refuse();
    public ValueTask PrepareSignInAsync(SignInForm form, CancellationToken cancellationToken) => throw Refuse();
    public ValueTask<IReadOnlyDictionary<string, string>?> LastNonSecretValuesAsync(CancellationToken cancellationToken) => new((IReadOnlyDictionary<string, string>?)null);
}
```

Run it (with the engine library built or unpacked): `dotnet run --project dotnet/TimeTug.Engine.Example`. In your own
project you reference the NuGet package instead of the project.

### 6.4 Where the data goes

The engine keeps its files (settings, account list, takeover ledger, duplicate decisions) in `DataDirectory`. Secrets
never go there: the engine asks your `ICredentialStore`, which should use the Windows Credential Locker or similar.

### 6.5 Packaging

- **One native DLL per architecture**: `runtimes/win-x64/native/TimeTugEngineC.dll` and
  `runtimes/win-arm64/native/TimeTugEngineC.dll`, nothing else. NuGet copies the right one next to your application for
  a `win-x64` or `win-arm64` publish.
- **No Visual C++ redistributable.** The C runtime is linked statically (`.cargo/config.toml`), and CI checks that the
  DLL imports no `VCRUNTIME*` or `MSVCP*` library and only Windows system DLLs.
- **Size** (measured in CI by `dotnet/pack/measure-size.ps1`, recorded in `docs/rust-port/size.md`): the native payload
  is 3,534,336 bytes for x64 and 3,351,040 bytes for ARM64 uncompressed, against a budget of 8,388,608 bytes (CI fails
  above it). The published `2.0.0-alpha.1` package file is 3,331,141 bytes. The Swift engine's payload in `1.8.x` was 74,626,048
  (x64) and 73,146,368 (ARM64) bytes uncompressed, in 17 files.
- **Platforms**: Windows x64 and Windows ARM64. The package contains no macOS or Linux library (the C interface itself
  builds on both; build `timetug-engine-c` yourself, section 5).
- The Windows bundle has a zone database inside it, because Windows has none.

## 7. Swift on macOS

Two Swift front doors exist: `TimeTugKit` (2.x, the Rust engine through generated UniFFI bindings, 7.2) and the `1.x` Swift
libraries (the earlier Swift engine, 7.1). New work uses `TimeTugKit`.

### 7.1 What exists today: the 1.x Swift libraries

The `1.x` branch (latest tag `1.8.4`) is a SwiftPM package, the one the Mac app uses. It supports macOS 14 and later
(swift-tools-version 6.0) and builds on the connector library `calendar-connectors` 1.2.0 or a newer 1.x. It is not a stable
third-party API: its design non-goals say so, the Mac app is its reference host, and the Apple adapters (Keychain,
the loopback OAuth listener, EventKit, Apple Intelligence) live in the Mac app's repository, not here, so an adopter
supplies their own.

```swift
// Package.swift
dependencies: [
    .package(url: "https://github.com/binary-companion/timetug-shared", exact: "1.8.4"),
],
targets: [
    .target(name: "MyApp", dependencies: [
        .product(name: "TimeTugCore", package: "timetug-shared"),
        .product(name: "TimeTugEngine", package: "timetug-shared"),
    ]),
]
```

Pin the `1.x` line (`exact:` or `.upToNextMinor(from: "1.8.0")`); `2.x` tags are the Rust workspace and are not Swift
packages. This snippet was built against tag `1.8.4` with Swift 6.4 on macOS when this page was written (not in CI).

Products: `TimeTugCore`, `CalendarBridge`, `TimeTugEngine`, `TimeTugEngineProtocol`, and `TimeTugEngineC` (a dynamic
library exporting the four C functions of section 5, the same header).

The 1.8.x public API, from the sources at the tag:

| Module | Public types |
|---|---|
| `TimeTugCore` | `actor CalendarStore` (`init(sources:calendar:...)`, `setSources`, `setCalendar`, `fetchWindow(now:leadTime:)`, `refresh(now:leadTime:sourceIDs:...)`, `inferenceStatus()`, `setInferenceEnabled`, `resolvePending`, `merge`, `unmerge`, `separate`, `forgetLessons`, `state()`, `load`); `protocol CalendarSource`; `CalendarSnapshot`, `TimeTugCalendarEvent`, `CalendarInfo`, `TakeoverSettings`, `DayAgenda`, `Scheduler`, `TakeoverPolicy`, `TakeoverGuard`, `TakeoverLedger`, `TakeoverRequest`, `DuplicateResolver`, `DuplicateRules`, `WidgetSnapshot`, `WidgetTimeline` and the dedup and adjudication types |
| `CalendarBridge` | `ConnectedSource` (adapts a `calendar-connectors` source to Core's `CalendarSource`), `EventMapper` |
| `TimeTugEngine` | `actor Engine` (`init(host:)`, `states` and `events` as `AsyncStream`, `currentState()`, `start()`, `send(_:)`); `EngineHost` (data directory, `SettingsStorage`, `CredentialStore`, `AuthorizationInteraction`, `SignInPrompting`, `ConnectorRegistry`, optional system source, `TextGenerator`, diagnostics sink, `EngineClock`, `Calendar`); `EngineCommand` (the commands of section 4.3 as enum cases, `applySettings` taking a closure that mutates `EngineSettings`); `EngineEvent` (`takeoverDue`, `takeoverWithdrawn`, `widgetSnapshot`, `joinRequested`, `diagnostic`); `EngineState`, `EngineSettings`, `SystemEvent`, `AccountManager`, `FileSettingsStorage`, `SystemClock` |
| `TimeTugEngineProtocol` | The wire types and `CommandDecoder` of section 4 |

A minimal snippet that was compiled against `1.8.4`:

```swift
import Foundation
import TimeTugCore
import TimeTugEngine

let settings = TakeoverSettings()                       // lead time 60 s, takeover on
let empty = EngineState.empty                           // revision 0, no events
let command = EngineCommand.applySettings { $0.takeover.leadTime = 120 }
```

To run the 1.x engine you build an `EngineHost` with your implementations of the host protocols and call
`Engine(host:)`, `start()` and `send(_:)`; the Mac app's `Packages/CalendarApple` and `Apps/macOS` in the `timetug`
repository are the reference. This page does not provide a runnable end-to-end Swift example for 1.x; none is
maintained here.

### 7.2 `TimeTugKit` (2.x)

`TimeTugKit` is a SwiftPM product of this repository's `2.x` tags (macOS 14 and iOS 17, Swift 6). Its package is a binary target
(the XCFramework attached to the release: macOS arm64 and x86_64, iOS, iOS simulator) under the generated Swift
(`TimeTugKitGenerated`) and a hand-written layer (`TimeTugKit`) that an app imports:

```swift
// Package.swift
dependencies: [
    .package(url: "https://github.com/binary-companion/timetug-shared", from: "2.0.0"),
],
targets: [
    .target(name: "MyApp", dependencies: [.product(name: "TimeTugKit", package: "timetug-shared")]),
]
```

(The additions below need `2.1.0` or newer. The install line is checked at release time, not in CI.) The reference for names, host interfaces,
configuration, delivery rules and threading is [`bindings.md`](bindings.md); what UniFFI could not express is in
[`bindings-spike.md`](bindings-spike.md). The shape: an `Engine` object built from an `EngineConfig`, the host objects the app
implements in Swift (`EngineHostInterfaces`: credentials, sign-in, HTTP, settings storage, and optionally text generation, external
tokens, a system calendar source such as EventKit and a diagnostics sink), and an observer that receives state summaries and events.
`ObservedEngine` offers the full state and the events as `AsyncStream`s. The `swift-bindings` CI job builds the package and runs
its Swift tests against the real engine on a manual clock.

Added in `2.1.0` for the Mac app (all additive; the C interface, the wire contract and the persisted-data fixtures are unchanged):

- Swift shims that restore the 1.x spelling of the model: `TimeTugCalendarEvent` accessors, `id`, `calendarKey`, `contentKey` and
  `init(event:sourceID:)`; `CalendarInfo.key`; `TakeoverSettings` with its 1.x defaults, `Codable` form and mutators;
  `Connection.sourceID`; the `CalendarService` and `CalendarProvider` values; the nested `RecurrenceRule` names; convenience
  initialisers with 1.x defaults (`bindings.md`, "The Swift package").
- Functions for the write checks of an EventKit-style adapter: `eventDraftUsedFields(draft:)`, `eventPatchTouchedFields(patch:)`,
  `validateEventDraft(draft:)`, `validateEventTiming(timing:)` and `validateRecurrenceRule(rule:)` (the last three throw
  `FfiError.Write`).
- `EngineStateSummary.nextPendingStart` (and `EngineState.nextPendingStart`): the start of the next meeting that still qualifies for a
  takeover, also while one is on screen.
- `DiagnosticSink.record(event:)`: the connectors' structured diagnostic events, with their private flags (a Swift sink that only
  implements `log` keeps compiling; a Kotlin one adds `record`).

## 8. Staying current

This page is part of the release checklist: the C and C# sections are proven by tests (the C example is built and run by
`c-smoke/run.sh`; the C# example is run by `ExampleTests`; the Rust example by `cargo test`), and the Swift section is
summarised from `bindings.md` (the Swift package is built and tested by the `swift-bindings` job). The numbers in section 6.5 come from `docs/rust-port/size.md`; update them when it
changes.
