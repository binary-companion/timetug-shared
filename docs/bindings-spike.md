# UniFFI spike: what the bindings can express

Phase 5, Task 1 of the Rust engine program. The throwaway code is on the branch `spike/uniffi` (directory `spike/uniffi/`, never merged); this page is the deliverable and the later tasks follow it. Measured on an Apple Silicon Mac (macOS 27, Xcode 27.2, Swift 6.4.2), Rust 1.99.1, 2026-11-14.

## 1. Version and sources

- `uniffi` **0.32.2** (released 2026-09-22; the newest on crates.io today), pinned with `=0.32.2`. Every statement below comes from the 0.32.2 manual in the tagged source (`docs/manual`), its changelog and the experiments in the spike, not from memory.
- Changelog items that matter for us: 0.32.0 made `#[uniffi::export(foreign)]` and `#[uniffi::export(rust, foreign)]` the spelling for foreign-implemented traits (`with_foreign` is deprecated); recursive enums are supported (`indirect` in Swift); record fields can be renamed with an attribute; items can be excluded from the bindings in `uniffi.toml`; the global `--config` file replaced the old flat override. Foreign trait methods cannot take references; every parameter is by value.
- Dependencies of the spike crate: `uniffi` (no default features), `thiserror`, `async-trait`, `tokio` (`rt`, `time`, `sync`, `macros` only). All are on the conventions list.

## 2. Commands that worked

Crate: `crate-type = ["staticlib", "cdylib", "lib"]`, `uniffi::setup_scaffolding!();` in `lib.rs` (proc-macro mode only, no UDL), a `uniffi.toml` next to `Cargo.toml`.

Bindgen binaries live in the crate behind a cargo feature, so a normal build does not compile them:

```toml
[features]
bindgen = ["uniffi/cli", "uniffi/cargo-metadata"]
[[bin]] name = "uniffi-bindgen-swift"  path = "src/bin/uniffi-bindgen-swift.rs"  required-features = ["bindgen"]
[[bin]] name = "uniffi-bindgen"        path = "src/bin/uniffi-bindgen.rs"        required-features = ["bindgen"]
```

`fn main() { uniffi::uniffi_bindgen_swift() }` and `fn main() { uniffi::uniffi_bindgen_main() }`. **`uniffi/cargo-metadata` is required**: without it the generator silently ignores the crate's `uniffi.toml` (custom types come out as plain `Int64`/`String`). With `uniffi` at `default-features = false` the feature is not on by default.

Swift, from a static library (library mode, no UDL):

```sh
cargo build --release --lib --target aarch64-apple-darwin
cargo build --features bindgen --bin uniffi-bindgen-swift
B=target/debug/uniffi-bindgen-swift; L=target/aarch64-apple-darwin/release/libtimetug_spike.a
$B $L out/Sources --swift-sources                       # TimeTugSpike.swift (module name from uniffi.toml)
$B $L out/Headers --headers                             # TimeTugSpikeFFI.h
$B $L out/Headers --modulemap --module-name TimeTugSpikeFFI --modulemap-filename module.modulemap
xcodebuild -create-xcframework -library $L -headers out/Headers -output build/TimeTugSpikeFFI.xcframework
```

- The header is named `<ffi_module_name>.h` (`uniffi.toml`: `ffi_module_name`, `module_name`). The plain `--modulemap` emits `module TimeTugSpikeFFI { header "TimeTugSpikeFFI.h" ... }`, which is what a static-library XCFramework wants. **Do not use `--xcframework`**: it emits `framework module ...`, which is for framework slices.
- SwiftPM: a `.binaryTarget(name: "TimeTugSpikeFFI", path: ...)` plus a Swift target holding the generated file and depending on it. `swift build` and `swift test` work.
- Universal macOS slice: `lipo -create` of the `aarch64-apple-darwin` and `x86_64-apple-darwin` archives, then `xcodebuild -create-xcframework` with three `-library ... -headers ...` groups (macOS universal, `aarch64-apple-ios`, `aarch64-apple-ios-sim`) worked.
- Generated output is deterministic (two runs identical), identical whether read from the debug or the release archive, and identical for the macOS and iOS archives. So CI can regenerate from the host build and compare with the committed file.
- Kotlin (`kotlin.sh` in the spike): `target/debug/uniffi-bindgen generate --library <cdylib> --language kotlin --out-dir out-kt` works (one 7,751-line file for the spike; it imports JNA and `kotlinx.coroutines`; `ktlint` is optional and only formats).

## 3. Probe results

"Works" means it ran in the Swift test target of the spike, unless noted.

| Probe | Result | Detail and consequence |
|---|---|---|
| Record with 40 fields: nested records, optionals, `Vec`, `HashMap<String,_>`, a set | **Works**, with two changes | `BTreeMap` and `BTreeSet` are **not supported** (only `HashMap` and `HashSet`). Mirrors use `HashMap`/`HashSet` (Swift `Dictionary`/`Set` are unordered anyway); anything whose order is observable stays a `Vec`. Fields become lower-camel (`f00_x` is `f00X`). Records are `Equatable, Hashable` and `Sendable` for free; one containing an object or foreign trait is not Equatable/Hashable (`HostBundle`). Round trip equal; `Set<BigEvent>` works. |
| Enum with associated values, labeled and unlabeled; enum inside a record; recursive enum | **Works** | Labeled fields keep their labels (`.time(start:end:)`), unlabeled stay positional (`.tentative(0.5, "x")`), `Option` payloads work, a recursive enum becomes `indirect`. Plain enums are `Equatable, Hashable`. Case names are lower-camel (`.authExpired`). |
| Custom types: `Timestamp` as `Date`, `Zone` as `TimeZone`, `UrlString` as `URL`, UUID as `UUID`, `Vec<u8>` as `Data`, duration as `TimeInterval` | **Works** | `Vec<u8>` is `Data` and `Duration` is `TimeInterval` with no configuration. The others use `[bindings.swift.custom_types.X]` in `uniffi.toml` (`type_name`, `lift`, `lower`, `imports`; the lift expression may not throw). `Date` to `Int64` milliseconds rounds to the nearest millisecond as the plan wants (0.4 ms rounds down, 0.6 ms up: tested). Custom types must be local newtypes (we mirror anyway). **Danger found:** a `try_lift` that fails (an unknown zone identifier) becomes a typed thrown error only when the receiving function returns `Result` with an error type that the custom error converts into (`echoZoneThrowing` threw `.InvalidConfig`); in a function that does not return `Result` it is a **Swift `fatalError`** ("Failed to convert arg"). So: every host-to-Rust function with a fallible custom type must be `throws`, or take a `String` and validate in Rust. A Swift fixed-offset `TimeZone` has the identifier `GMT+0100`, which `jiff` does not read; Task 2 needs an explicit rule (offset-form identifiers are converted at the boundary, with a test). |
| Error enum thrown from Rust, caught as typed Swift error; error thrown by a Swift callback and received in Rust | **Works** | `#[derive(uniffi::Error)]` enum becomes a Swift `Error`, `Equatable`, `Hashable`, `LocalizedError`; payload cases work. **Case names of an error enum are always UpperCamel** (`.Source`, `.Cancelled`, `.Host(message:)`); `uniffi.toml` rename of an error variant is applied but then normalised back to UpperCamel. So today's lower-camel `SourceError`/`WriteError` cases (`.authExpired`) cannot be generated as error enums. **Workaround, recommended:** make `SourceError`, `WriteError` and the other mirrors **plain `uniffi::Enum`s** (lower-camel cases, as in the spike's `SourceErrorKind`) and make the one thrown type `FfiError` (five cases) carry them (`.Source(SourceError)`), with a shim `extension SourceError: Error {}` so existing `throw`/`catch` code reads the same. A Swift error that is not the declared host error type reaches Rust through `From<UnexpectedUniFFICallbackError>` (carries its description), so `HostError` needs that impl (spike: `Boom.x` arrived as `Failed("x")`). A host error maps to `Failure::Host(message)`; the `Display` text is the message, with no `host:` prefix added by UniFFI. |
| Async Rust method called with Swift `await`, driven by a `tokio` current-thread runtime on the engine thread | **Works with the oneshot pattern, not with `async_runtime = "tokio"`** | The `uniffi` `tokio` feature wraps the future in `async-compat`, which starts its own global multi-thread `tokio` runtime; that conflicts with the allowed `tokio` features (design 6.2). Do not enable it. Instead the exported `async fn` does `handle.spawn(work)` on the engine runtime's `Handle` and `await`s a `tokio::sync::oneshot` receiver; UniFFI polls that future from the Swift executor, which only needs a waker. Spike: `SpikeEngine::send` (engine thread built with `new_current_thread().enable_time()`; the thread blocks on `block_on(stop_rx)`), including a runtime handle moved to other threads. 500 construct, send and shutdown loops ran without an unexpected error (0.032 s). |
| Async foreign-implemented trait (Swift implements, Rust awaits), incl. a `@MainActor` implementor and one that never returns, then Rust drops the future | **Works** | Declaration: `#[uniffi::export(foreign)]` outside `#[async_trait::async_trait]` (order matters). Swift sees `protocol X: AnyObject, Sendable` with `async throws` methods. A `@MainActor` method satisfies an `async` requirement, and calling it from a `@MainActor` test that is itself suspended in `await engine.callMainActor()` did not deadlock. For the never-returning method, Rust wrapped the call in `tokio::time::timeout` on the engine runtime and dropped it after 100 ms: the Swift side saw its `Task` **cancelled** (`Task.isCancelled` became true; a cooperative implementation stops, an uncooperative one keeps running and its result is discarded). A thrown `HostError` arrives as the matching Rust variant. |
| Foreign trait inside a record, or returned from another foreign trait method | **Works with one workaround** | A record field `Arc<dyn Trait>` and `Option<Arc<dyn Trait>>` work (`HostBundle`; the record is not Equatable). A **sync** foreign method returning `Result<Arc<dyn Other>, E>` compiles. An **async** foreign method returning `Result<Arc<dyn Other>, E>` does **not compile** (`implementation of FfiConverterArc is not general enough`, also with `+ 'static`). Workaround: return a record that holds it (`RedirectBegun { session }`); verified end to end. For `beginOAuthRedirect()` the Swift spelling becomes `beginRedirect() async throws -> RedirectBegun`; the shim can hide the wrapper. |
| Foreign trait with 12 methods, some returning optional records | **Works** | `SystemSource`: access, calendars, events in a range (record list), series (optional record), capabilities, create, update, delete, move, rsvp, set a change handler, refresh; all 12 called from one Rust async fn. A Rust-implemented object (`ChangeHandler`) passed to the foreign side as an argument works: Swift stored it, called `changed()` later, the Rust counter read 3. |
| Non-async foreign trait called from the delivery thread at high rate | **Works** | 100,000 empty states from a Rust thread to a Swift observer: 0.112 s (1.1 microseconds each), all 100,000 delivered in order. The cost is the payload, see section 4. |
| Kotlin generation of all of the above; `kotlinc` | **Generation works; compile not run** | All probes generate. No `kotlinc` is installed and the only Kotlin compiler on the machine (1.6.10) lives in Gradle's instrumented cache and does not start. Installing a Kotlin compiler is a download, so Task 7 is an owner item (section 6). |
| Default argument values; `Equatable`/`Hashable`/`Sendable` on records and enums | **Works** | `#[uniffi(default = false)]` makes the Swift initializer parameter defaulted (`Attendee(name:email:response:)` without `isSelf`). Records and plain enums are `Equatable, Hashable`; `Sendable` conformances are generated so values cross actors in Swift 6 mode (tested in a Swift 6 test target). `Comparable`, `Identifiable` and `Codable` are not generated (`generate_codable_conformance = true` exists; not used) and come from shims. |
| Two generated types whose Swift names equal Foundation or app types | **Does not work without a rename** | A generated `Calendar` record makes unqualified `Calendar` **ambiguous** in any file importing Foundation and the module (`'Calendar' is ambiguous for type lookup`). `[bindings.swift.rename] Calendar = "CalendarRecord"` fixes it. `Connection` has no Foundation twin and was fine. Task 3 must list every generated name that Foundation, Swift or the Mac app also declares (`Calendar`, `Event`, `Date`-like names) and rename it, recording the pair in `docs/bindings.md`. |

## 4. Measurements

Release build of the spike (`opt-level = "s"`, fat LTO, `panic = "abort"`). The probe event has 40 fields including two attendees, two reminders, a map, a set and several strings, so it is heavier than most real events.

| What | Result |
|---|---|
| Static library, `aarch64-apple-darwin`, release, with all probes | 24.9 MB archive (it holds the object files; the linked size is what ships). A small executable that links it and calls one function: 3.4 MB (2.8 MB after `strip -x`), against 50 KB for hello world. This includes the generated Swift for the probe types; the real engine will add its own code. Phase 5's close-out records the real figures. |
| `strip = "symbols"` versus `"debuginfo"` for the static library | **Inconclusive here.** For a static library rustc strips by running `rust-objcopy`, which crashed on this machine (`Library not loaded: @rpath/libLLVM.dylib`, a warning, not an error) in both profiles, so the two archives are almost the same size (24,875,680 versus 24,884,880 bytes, 360 external symbols either way) and both link and run. Task 6 repeats the comparison in CI (the `macos-26` runner) before it decides whether a `release-apple` profile is needed; `sizes.sh` in the spike repeats it. |
| Swift to Rust, 500 events (`countEvents`) | 1.95 ms per call |
| Rust to Swift, 500 events (`makeAgenda`: builds the events in Rust, lowers, Swift lifts) | **23.8 ms per call** (about 48 microseconds per event); one event 49 microseconds |
| Round trip of 500 events (`echoEvents`, the plan's loop of 1,000 calls) | 25.0 ms per call (debug build: 51 ms) |
| One state delivery of 500 events to a Swift observer (200 deliveries from a Rust thread; each clones a prebuilt template, lowers, Swift lifts) | **23.8 ms** each |
| A small record call (`makeSummary`, four fields) | 1.8 microseconds |

**Review Focus 1 fails**: delivering a state that carries 500 full events takes about five times the 5 ms budget, and the cost is in crossing the boundary, not in building the events: delivering a prebuilt template (clone, lower, lift) and building the events in Rust each time both take 23.8 ms, while the same data going Swift to Rust takes 1.95 ms. The spike does not separate Rust encoding from Swift decoding. Task 6 step 5's split is therefore **required, not a fallback**: the observer receives a light `EngineStateSummary` and the host fetches events, agenda and candidates only when their sub-revision changed. Task 5 should build the summary and the sub-revisions from the start. Task 6 re-measures with the real types.

Evidence for each row is in the spike: `Probes.swift` (the numbers print as `ECHO500`, `RUST->SWIFT`, `BLAST`, `HIGHRATE`, `LIFECYCLE`), `Crash.swift` (run with `CRASH=1`, the `fatalError` below), `kotlin.sh`, `sizes.sh`, `src/compile_fail.rs.txt` and the comment at the end of `uniffi.toml`.

## 5. Other findings that change later tasks

1. **Swift 6 language mode does not compile the generated file** with Swift 6.4.2: two `Task { }` closures in the async foreign-trait glue are rejected (`passing closure as a 'sending' parameter risks causing data races`). The target holding the generated file needs `swiftSettings: [.swiftLanguageMode(.v5)]`. Recommended Task 6 layout: three targets, `TimeTugEngineFFI` (binary), `TimeTugKitGenerated` (the generated file, Swift 5 mode), `TimeTugKit` (shims, Swift 6 mode, `@_exported import TimeTugKitGenerated`), so the hand-written code keeps strict concurrency. Task 6 verifies this layout.
2. **iOS simulator**: `xcodebuild build -scheme X -destination 'generic/platform=iOS Simulator'` builds both arm64 and x86_64 and fails to link against an arm64-only simulator slice (`found architecture 'arm64', required architecture 'x86_64'`). Add `ARCHS=arm64`: `xcodebuild build-for-testing -scheme TimeTugSpike -destination 'generic/platform=iOS Simulator' ARCHS=arm64` links the test bundle (`TEST BUILD SUCCEEDED`). A plain `build` of a library target does not link anything, so the link check must be `build-for-testing`.
3. **The engine's `tokio` shape is unchanged**: no `async_runtime = "tokio"` anywhere, so the "no other tokio feature" CI script keeps passing.
4. **Bindgen dependencies**: the `bindgen` feature adds `clap`, `camino`, `cargo_metadata`, `goblin` and others to `Cargo.lock`. Task 2 either allows them in `deny.toml` and `check-crate-architecture.py` for the `bindgen` feature only, or moves the two binaries into a separate crate outside the workspace members (`tools/uniffi-bindgen`, run with the repository root as the working directory). Task 2 chooses and records it; the feature-gated route is the one verified here.
5. **Names**: the module name comes from `uniffi.toml` (`module_name`); types keep their Rust names; functions and fields become lower-camel; a Rust argument name becomes the Swift label (`events_in(start, end)` generates `eventsIn(start:end:)`, so a Swift spelling such as `events(in:)` is a shim); errors get a `LocalizedError` conformance (`omit_localized_error_conformance` turns it off).
6. **Value semantics**: every record or enum method call copies the whole value across the boundary; the bindings expose no methods on records (shims add them in Swift).
7. **Statements not covered by a probe** (so Task 6 must test them): a never-returning host call at the moment of `shutdown()` (only the timeout-drop and the 500-iteration send versus shutdown loop were run); thread-safety of one Swift host object called from the engine thread and the delivery thread at once; the sync `Arc<dyn>` return was compiled but not called.

## 6. Recommendations per "does not work"

| Problem | Recommendation |
|---|---|
| `BTreeMap`, `BTreeSet` | Mirror as `HashMap`/`HashSet`; keep `Vec` where order is observable. |
| Error cases are UpperCamel | Plain `uniffi::Enum` mirrors for `SourceError`, `WriteError` and other payload errors; one thrown `FfiError`; Swift shim conformances. |
| Failed custom-type lift outside a `Result` function | Fallible custom types only in `throws` functions, or `String` plus Rust validation; explicit rule for offset-form zone identifiers. |
| Async foreign method returning a foreign trait object | Return a record wrapping the object (`RedirectBegun`), hidden by a shim. |
| `Calendar` and other Foundation clashes | `uniffi.toml` rename; list in `docs/bindings.md`. |
| 500-event state takes 24 ms | Light `EngineStateSummary` plus fetch on sub-revision change (Task 5 and 6). |
| Swift 6 mode | Generated target in Swift 5 mode, shims in Swift 6 mode. |
| iOS simulator link | `ARCHS=arm64` and `build-for-testing`. |
| `kotlinc` | Owner item (download of a Kotlin compiler and JNA/coroutines jars for Task 7). |

None of these is a blocker: each "does not work" has an acceptable workaround, so Task 2 can start.
