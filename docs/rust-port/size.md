# Size of the Rust engine library

The budget (design section 6.4, phase 3 gate): the native payload of the `TimeTug.Engine` package is at most
**8,388,608 bytes (8 MB) uncompressed per Windows architecture**. The payload is exactly one file,
`runtimes/<rid>/native/TimeTugEngineC.dll`. The Swift engine's payload was 74,626,048 bytes (x64) and 73,146,368 bytes
(ARM64), see [baselines.md](baselines.md).

## Result

Measured by `dotnet/pack/measure-size.ps1` in the `dotnet-rust-package` job of pull request 56 (CI, hosted
`windows-latest` and `windows-11-arm`; the same bytes are in every run, the build is deterministic enough that all
three benchmark runs saw the same size):

| Architecture | Files | Uncompressed | Zip (Optimal) | Budget | Result |
|---|---:|---:|---:|---:|---|
| win-x64 | 1 | 3,534,336 bytes (3.4 MB) | 1,589,598 bytes | 8,388,608 bytes | pass, 42% of the budget |
| win-arm64 | 1 | 3,351,040 bytes (3.2 MB) | 1,514,038 bytes | 8,388,608 bytes | pass, 40% of the budget |

The package (`TimeTug.Engine`, both architectures, header, notices, fixtures) is 3,317,804 bytes. The Swift package
carried 74,626,048 bytes (x64) and 73,146,368 bytes (ARM64) uncompressed (28,299,296 and 27,643,266 compressed): the
Rust payload is 4.7% and 4.6% of that. Other platforms (not part of the gate): `libTimeTugEngineC.dylib` 5,713,544
bytes (development Mac, arm64; 3,193,904 on the hosted `macos-26` run, which is a different build).

`dumpbin /dependents` of `TimeTugEngineC.dll`, both architectures: `bcryptprimitives.dll`, `kernel32.dll`,
`api-ms-win-core-synch-l1-2-0.dll`, `KERNEL32.dll`, `ntdll.dll` (the list as the script prints it, every entry on the
allow-list; no `VCRUNTIME*`, no `MSVCP*`). The release profile (`opt-level = "s"`, fat LTO, one codegen unit,
`panic = "abort"`, `strip = "symbols"`) was in effect from the start and the bundled zone database is in the library,
so none of the "if the gate fails" steps was needed.

## The gate

`dotnet/pack/measure-size.ps1` measures the uncompressed native set of each architecture and exits 1 above
8,388,608 bytes. CI runs it in the `dotnet-rust-package` job on the two native sets that the `dotnet-rust-windows-x64`
and `dotnet-rust-windows-arm64` jobs assemble with `dotnet/pack/assemble-native.ps1 -Rust`. That script also proves
the library is self-contained: it exports the four `tt_engine_*` functions, depends only on Windows system DLLs
(`KERNEL32.dll`, `ntdll.dll`, `bcryptprimitives.dll`, `ADVAPI32.dll`, `USERENV.dll`, `WS2_32.dll`, `api-ms-win-*.dll`,
`bcrypt.dll`) and imports no `VCRUNTIME*.dll` or `MSVCP*.dll` (the C runtime is linked statically,
`.cargo/config.toml`).

## If the gate fails

In this order (phase 3 plan, task 9, step 9). Never raise the limit.

1. Confirm the release profile is in effect: `opt-level = "s"`, `lto = "fat"`, `codegen-units = 1`,
   `panic = "abort"`, `strip = "symbols"` (`cargo build --release -v -p timetug-engine-c`).
2. `cargo bloat --release -p timetug-engine-c --crates` (a local tool, not a dependency). Remove the largest avoidable
   contributor: unused `serde_json` features, `jiff` features such as the bundled database on platforms that have a
   system one, formatting machinery from `Debug` derives on wire types.
3. Try `opt-level = "z"` and compare the benchmark (`scripts/benchmarks/run-engine-bench.*`).
4. **Never drop the bundled zone database on Windows** (`jiff-tzdb`, feature `tzdb-bundle-platform`): Windows has no
   system database, and the engine's local-time behaviour depends on it.

## Bindings (phase 5)

Measured 2026-11-14 on Apple Silicon with `scripts/dev/build-xcframework.sh --release` (profile `release-apple`,
`strip = "debuginfo"`), crate `timetug-engine-ffi`:

| Artifact | Size |
|---|---|
| macOS slice `libtimetug_engine_ffi.a` (arm64 and x86_64 fat archive) | 153,196,424 bytes (arm64 part 76,398,720) |
| iOS device slice | 76,173,656 bytes |
| iOS simulator slice (arm64) | 76,139,144 bytes |

These are static archives (object files with symbol tables, not stripped of dead code); the size a host adds is what the linker
keeps. The size added to a linked executable and the Android `libtimetug_engine_ffi.so` (arm64-v8a, release) are not measured
here: the second is printed by the `kotlin-bindings` CI job in its step summary, the first is measured in phase 5 Task 9. No
size gate applies to the bindings.
