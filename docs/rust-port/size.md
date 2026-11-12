# Size of the Rust engine library

The budget (design section 6.4, phase 3 gate): the native payload of the `TimeTug.Engine` package is at most
**8,388,608 bytes (8 MB) uncompressed per Windows architecture**. The payload is exactly one file,
`runtimes/<rid>/native/TimeTugEngineC.dll`. The Swift engine's payload was 74,626,048 bytes (x64) and 73,146,368 bytes
(ARM64), see [baselines.md](baselines.md).

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
