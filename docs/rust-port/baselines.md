# Resource baselines of the Swift engine

What the Rust engine must meet or beat (design section 6.4, phase 3 gate). Measured on the Swift engine library of
`timetug-shared` `v2` at commit `d7b4aa3` (Swift sources identical to the `1.8.1` baseline tag apart from the
`VectorExporter` target and the Rust workspace, neither of which is in the engine library). Every number below was
measured, including the Parallels VM row on the development Mac.

## Acceptance rule

The Rust engine must be at or below each number: with 10% tolerance for run-to-run noise on time (`readyMs`) and CPU
(`cpuMsIdle`, dedup timings), and none on size (`nativeModuleBytes`, package sizes). Memory (`workingSetBytes`,
`privateBytes`) is compared at the median of three runs, with the same 10% tolerance as time.

## How it is measured

`TimeTug.Engine.Bench --idle-seconds <n> --out <file>` (`dotnet/TimeTug.Engine.Bench`) creates an engine through the
managed wrapper in a fresh temporary data directory (`platform` of the host, `testing: false`, no connectors), sends
`start` and times `tt_engine_create` to the `ready` event (`readyMs`); every `service.*` request is answered with
error kind `failed`. It then idles for `idle-seconds`, reading `Process.TotalProcessorTime` at the start and the end
(`cpuMsIdle`), and reads memory at the end. The runs below idle 600 s. `nativeModuleBytes` is the sum of the sizes of
the native (non-managed) `.dll` / `.dylib` / `.so` files loaded from the application directory.

- `workingSetBytes`: `Process.WorkingSet64`. `privateBytes`: `Process.PrivateMemorySize64` on Windows; on macOS, where
  .NET reports none, the process's physical footprint (`ri_phys_footprint`, `proc_pid_rusage`). The two are therefore
  comparable between runs on one platform, not between platforms.
- The process includes the .NET runtime and the managed wrapper, as an app hosting the engine does; the Rust engine
  is compared in the same harness.
- The Windows working set after 600 s is small because Windows trims the working set of an idle process; the
  Rust engine is compared on the same terms.
- On macOS the native module is the engine library alone (the Swift runtime is the system's, in `/usr/lib/swift`);
  on Windows it is the engine DLL plus the Swift runtime DLLs shipped beside it, which equals the payload of the
  published package.
- The engine reports its own version as `0.0.0-dev` in `ready` for these builds (the version is stamped by the
  release workflow).

Commands (three runs per platform, each on a fresh runner; the median is recorded):

```
# Windows (PowerShell; Swift toolchain, Visual Studio, .NET 8 SDK)
pwsh scripts/benchmarks/run-engine-bench.ps1 -IdleSeconds 600 -Out build/bench/engine-bench.json
# macOS / Linux
scripts/benchmarks/run-engine-bench.sh --idle-seconds 600 --out build/bench/engine-bench.json
# CI: workflow "Engine benchmark" (bench.yml), workflow_dispatch: platform and idle-seconds inputs
# Dedup benchmark (macOS)
scripts/benchmarks/run-dedup.sh
```

## Windows x64

GitHub-hosted `windows-latest` (image `win25-vs2026` 20261103.298.1, Windows 10.0.26100, 4 logical CPUs), workflow run
42923800836, three runs.

| Run | readyMs | workingSetBytes | privateBytes | cpuMsIdle per 600 s | nativeModuleBytes |
|---|---|---|---|---|---|
| 1 | 193.3 | 18,870,272 | 12,972,032 | 31.2 | 74,626,048 |
| 2 | 412.3 | 18,882,560 | 13,455,360 | 109.4 | 74,626,048 |
| 3 | 196.3 | 18,878,464 | 13,959,168 | 62.5 | 74,626,048 |
| **Median** | **196.3** | **18,878,464** | **13,455,360** | **62.5** | **74,626,048** |

Package (Task 1, `timetug.engine.1.8.1.nupkg`): native payload uncompressed 74,626,048 bytes, compressed in the
package 28,299,296 bytes. `nativeModuleBytes` equals the uncompressed payload.

## Windows ARM64

GitHub-hosted `windows-11-arm` (image `win11-vs2026-arm64` 20261101.203.1, Windows 10.0.26200, 4 logical CPUs),
workflow run 42923800836, three runs (run 3 is a re-run of the same job after its Swift install step hung).

| Run | readyMs | workingSetBytes | privateBytes | cpuMsIdle per 600 s | nativeModuleBytes |
|---|---|---|---|---|---|
| 1 | 614.4 | 9,625,600 | 14,057,472 | 93.8 | 73,146,368 |
| 2 | 550.7 | 9,740,288 | 13,979,648 | 93.8 | 73,146,368 |
| 3 | 571.0 | 9,535,488 | 14,045,184 | 125.0 | 73,146,368 |
| **Median** | **571.0** | **9,625,600** | **14,045,184** | **93.8** | **73,146,368** |

Package (Task 1): native payload uncompressed 73,146,368 bytes, compressed 27,643,266 bytes.

### Parallels VM on the development Mac

Parallels VM `Windows 11` (Windows 11 Pro 10.0.26200, ARM64, 6 vCPUs) on the Apple M1 Max, 64 GB development Mac.
Swift 6.4.0 (the toolchain `install-swift-windows.ps1` pins; the build reports `+assertions`, as on the hosted
runner), .NET 8.0.428 runtime and SDK (the build itself picked SDK 10.0.404, the harness targets net8.0), Visual Studio
2022 17.7 MSVC tools. Measured 2026-11-11 with `scripts/benchmarks/run-engine-bench.ps1 -IdleSeconds 600`, three
runs one after another, each with its own (incremental) build and a fresh data directory.

| Run | readyMs | workingSetBytes | privateBytes | cpuMsIdle per 600 s | nativeModuleBytes |
|---|---|---|---|---|---|
| 1 | 100.6 | 53,710,848 | 15,876,096 | 156.2 | 73,130,496 |
| 2 | 105.6 | 53,665,792 | 15,802,368 | 265.6 | 73,130,496 |
| 3 | 96.6 | 53,104,640 | 15,233,024 | 109.4 | 73,130,496 |
| **Median** | **100.6** | **53,665,792** | **15,802,368** | **156.2** | **73,130,496** |

Conditions: the Parallels setting "Pause idle" was switched off for the runs (with it on, Parallels pauses an idle VM
about every 33 s, which breaks a 600 s idle measurement) and restored afterwards; no pause occurred during the runs.
The EA Desktop app in the VM user session was spinning at about 300% CPU, so its processes were stopped before the
runs. No user was signed in to the VM desktop (Windows Update had restarted it), so the harness ran as SYSTEM instead
of as the logged-in user. The runs started with 1-minute / 5-minute / 15-minute host load averages of 7.44 / 14.47 /
11.49 (run 1, still settling after the VM's Windows Update restart; 84.6% CPU idle), 2.75 / 5.54 / 7.96 (run 2; 83.7%
idle) and 4.33 / 4.96 / 6.35 (run 3; 74.9% idle), and ended at 2.75, 4.33 and 3.69 (1 minute). The `cpuMsIdle`
spread (109 to 266 ms) is wider than on the other hosts and run 2 is the high one; the median is reported as for the
other rows. The VM numbers are higher in `workingSetBytes` than the hosted runner (53.7 MB against 9.6 MB) and lower in
`readyMs` (100.6 against 571.0 ms); the VM was not otherwise comparable (6 vCPUs, virtualised, Windows 11 Pro).

Package (Task 1) sizes are the hosted-runner figures above (the native payload here is 73,130,496 bytes against
73,146,368 there, a different Swift build of the same sources).

## macOS arm64

### GitHub-hosted `macos-26`

Image `macos26` 20261102.0473.1, macOS 26.7.2, Apple silicon, 3 logical CPUs, workflow run 42923800836, three runs.

| Run | readyMs | workingSetBytes | privateBytes (physical footprint) | cpuMsIdle per 600 s | nativeModuleBytes |
|---|---|---|---|---|---|
| 1 | 226.9 | 39,976,960 | 31,590,272 | 81.1 | 7,610,248 |
| 2 | 233.3 | 37,650,432 | 32,376,704 | 72.8 | 7,610,248 |
| 3 | 314.8 | 35,471,360 | 32,802,688 | 92.0 | 7,610,248 |
| **Median** | **233.3** | **37,650,432** | **32,376,704** | **81.1** | **7,610,248** |

### The development Mac

Apple M1 Max, 64 GB, 10 logical CPUs, macOS 27.0.1, Swift 6.4.2, .NET SDK 8.0.133 (Homebrew `dotnet@8`), measured
2026-11-11 with `scripts/benchmarks/run-engine-bench.sh --idle-seconds 600`, three runs one after another, each with
its own build and a fresh data directory. The owner's usual apps (Chrome, the Claude app, the menu bar tools) stayed
open and no builds, tests or agents ran during a measurement, but the machine never settled below a 1-minute load
average of 2 in the 30 minutes it was watched (it idled at about 3.5 to 6 from those apps); each run started after up
to 3 minutes of waiting for the load to drop, with 1-minute / 5-minute load averages of 5.10 / 4.79 (run 1),
4.44 / 5.48 (run 2) and 4.59 / 4.92 (run 3), about 78 to 80% CPU idle.

| Run | readyMs | workingSetBytes | privateBytes (physical footprint) | cpuMsIdle per 600 s | nativeModuleBytes |
|---|---|---|---|---|---|
| 1 | 353.0 | 58,966,016 | 33,538,888 | 86.2 | 7,936,248 |
| 2 | 483.2 | 58,687,488 | 33,522,504 | 82.2 | 7,936,248 |
| 3 | 388.4 | 58,884,096 | 33,325,896 | 80.7 | 7,936,248 |
| **Median** | **388.4** | **58,884,096** | **33,522,504** | **82.2** | **7,936,248** |

The development Mac numbers are the macOS baseline for the dev hardware; the hosted `macos-26` numbers above are the
baseline for the CI runner (a smaller machine and an older macOS, so its working set is lower and its native module
is smaller). The Rust engine is compared on the same machine as the row it is checked against.

## Dedup benchmark (macOS)

`scripts/benchmarks/run-dedup.sh` (`swift run -c release DedupBenchmark`; `--rust` runs the Rust port), development Mac (Apple M1 Max, 64 GB,
macOS 27.0.1, Swift 6.4.2), `calendar-connectors` 1.2.0, measured 2026-11-11: the release build first, then three
runs one after another, the median of the three runs per row recorded. Nothing but the owner's usual open apps
(Chrome, the Claude app, the menu bar tools) ran besides it; the 1-minute / 5-minute load averages when the runs
started were 3.22 / 4.48 (run 1), 7.03 / 5.27 (run 2, a short spike just before it began) and 3.64 / 4.46 (run 3),
at about 62 to 82% CPU idle, because the machine never settled below a load of 2 in 30 minutes of watching. An
earlier single run on a machine busy with builds and agents gave 102,717.787 ms for dense 5000, so the busy-machine
penalty was small; the three-run medians below are the baseline. Output lines (medians):

```
shape,count,median_ms,output_events
sparse,100,0.255,100
sparse,500,1.355,500
sparse,1000,2.905,1000
sparse,5000,14.880,5000
dense,100,35.506,100
dense,500,911.095,500
dense,1000,3660.757,1000
dense,5000,102016.760,5000
duplicates,100,46.232,25
duplicates,500,490.346,125
duplicates,1000,1063.666,250
duplicates,5000,5743.033,1250
```

`output_events` is deterministic and must match exactly in the Rust port.

### Rust port against Swift (phase 2, Task 7 gate)

`scripts/benchmarks/run-dedup.sh --rust` (`cargo run --release -p dedup-benchmark`; the workspace release profile,
`opt-level = "s"`, fat LTO) against `scripts/benchmarks/run-dedup.sh`, same Mac as above (Apple M1 Max, macOS 27.0.1,
Swift 6.4.2, Rust 1.99.1), measured 2026-11-11 in one session: both release builds first, then six runs one after
another and nothing else of the agent's running, alternating Swift and Rust (S1 R1 S2 R2 S3 R3). Each run waited up
to 3 minutes for a 1-minute load average below 3 and was then measured anyway. Medians of the three runs per row.
The Swift figures were re-measured in this session so the gate compares like with like; they are 0 to 5% lower
than the earlier figures above (a calmer machine) and are the ones the gate uses.

1-minute load average when each run started: S1 2.83, R1 2.98, S2 2.76, R2 5.44 (the load did not drop below 3 within
3 minutes, so it was measured anyway), S3 2.87, R3 2.11.

| Row | Swift median (ms) | Rust median (ms) | Rust / Swift | Gate (at most 1.10) | `output_events` |
|---|---|---|---|---|---|
| sparse 100 | 0.261 | 0.116 | 0.44 | pass | 100 |
| sparse 500 | 1.411 | 0.608 | 0.43 | pass | 500 |
| sparse 1000 | 2.757 | 1.268 | 0.46 | pass | 1000 |
| sparse 5000 | 14.703 | 6.941 | 0.47 | pass | 5000 |
| dense 100 | 33.832 | 18.474 | 0.55 | pass | 100 |
| dense 500 | 872.693 | 485.971 | 0.56 | pass | 500 |
| dense 1000 | 3505.369 | 1956.640 | 0.56 | pass | 1000 |
| dense 5000 | 98,070.544 | 57,294.890 | 0.58 | pass | 5000 |
| duplicates 100 | 44.255 | 22.668 | 0.51 | pass | 25 |
| duplicates 500 | 473.066 | 238.346 | 0.50 | pass | 125 |
| duplicates 1000 | 1030.131 | 512.012 | 0.50 | pass | 250 |
| duplicates 5000 | 5577.906 | 2731.589 | 0.49 | pass | 1250 |

Individual runs (ms), Swift then Rust: sparse 5000 14.250 to 15.073 and 6.531 to 7.456; dense 5000 94,701.829 to
100,215.287 and 53,723.511 to 57,463.668. All twelve rows pass with a wide margin, so no hot-spot fix was needed.
`output_events` is identical to the Swift rows in all three Rust runs.

Run it: `scripts/benchmarks/run-dedup.sh --rust` (optionally `dense 1000`). A dense 5000 row takes about a minute in
Rust and over a minute and a half in Swift per repetition set (six repetitions each), so a full run is several minutes.

## Rust engine against these baselines (phase 3, Task 9 gate)

`TimeTug.Engine.Bench` against the Rust library (`scripts/benchmarks/run-engine-bench.sh --rust`,
`scripts/benchmarks/run-engine-bench.ps1 -Rust`, or the workflow "Engine benchmark" with `engine=rust`), the same
harness, the same 600 s idle, three runs per machine, the median recorded, measured 2026-11-12. The engine library
is the workspace release build (`opt-level = "s"`, fat LTO, one codegen unit, strip, abort; on Windows a static C
runtime). Gate: each of `readyMs`, `workingSetBytes`, `privateBytes`, `cpuMsIdle` at or below the Swift median of the
same machine (10% tolerance on time and CPU, which was not needed), and the native payload at most 8 MB per Windows
architecture ([size.md](size.md)).

| Machine | Metric | Swift median | Rust median | Rust / Swift | Gate |
|---|---|---:|---:|---:|---|
| Windows x64, hosted `windows-latest` (run 43174649487) | readyMs | 196.3 | 147.4 | 0.75 | pass |
| | workingSetBytes | 18,878,464 | 17,248,256 | 0.91 | pass |
| | privateBytes | 13,455,360 | 9,994,240 | 0.74 | pass |
| | cpuMsIdle | 62.5 | 31.2 | 0.50 | pass |
| | nativeModuleBytes | 74,626,048 | 3,534,336 | 0.05 | pass (at most 8,388,608) |
| Windows ARM64, hosted `windows-11-arm` | readyMs | 571.0 | 162.7 | 0.28 | pass |
| | workingSetBytes | 9,625,600 | 9,199,616 | 0.96 | pass |
| | privateBytes | 14,045,184 | 10,874,880 | 0.77 | pass |
| | cpuMsIdle | 93.8 | 31.2 | 0.33 | pass |
| | nativeModuleBytes | 73,146,368 | 3,351,040 | 0.05 | pass (at most 8,388,608) |
| macOS arm64, hosted `macos-26` | readyMs | 233.3 | 208.7 | 0.89 | pass |
| | workingSetBytes | 37,650,432 | 37,453,824 | 0.99 | pass |
| | privateBytes (physical footprint) | 32,376,704 | 31,999,808 | 0.99 | pass |
| | cpuMsIdle | 81.1 | 69.0 | 0.85 | pass |
| | nativeModuleBytes | 7,610,248 | 3,193,904 | 0.42 | pass |
| macOS arm64, development Mac (Apple M1 Max) | readyMs | 388.4 | 254.9 | 0.66 | pass |
| | workingSetBytes | 58,884,096 | 57,196,544 | 0.97 | pass |
| | privateBytes (physical footprint) | 33,522,504 | 32,621,336 | 0.97 | pass |
| | cpuMsIdle | 82.2 | 25.2 | 0.31 | pass |
| | nativeModuleBytes | 7,936,248 | 5,713,544 | 0.72 | pass |

Individual runs (readyMs / workingSetBytes / privateBytes / cpuMsIdle):

- Windows x64: 147.4 / 17,264,640 / 9,994,240 / 46.9; 111.8 / 17,248,256 / 9,957,376 / 15.6; 152.6 / 17,182,720 / 10,878,976 / 31.2.
- Windows ARM64: 161.5 / 9,224,192 / 10,936,320 / 15.6; 167.3 / 9,113,600 / 10,874,880 / 31.2; 162.7 / 9,199,616 / 10,809,344 / 62.5.
- macOS hosted: 219.6 / 37,044,224 / 32,032,640 / 89.2; 145.6 / 37,453,824 / 31,999,808 / 69.0; 208.7 / 37,732,352 / 30,590,784 / 57.3.
- Development Mac: 191.6 / 57,196,544 / 32,621,336 / 25.2; 254.9 / 56,885,248 / 32,408,392 / 24.7; 355.0 / 58,048,512 / 33,424,248 / 71.9.

Conditions. The hosted rows ran on the same images as the Swift rows (`win25-vs2026` 20261103.298.1, `win11-vs2026-arm64`
20261101.203.1, `macos26` 20261102.0473.1), three fresh runners each. The development Mac (macOS 27.0.1, .NET SDK
8.0.133, Rust 1.99.1, 10 logical CPUs) ran three runs one after another with only the owner's usual apps open and no
other work of the agent's in the Mac's foreground; the machine was not quiet, and the 1-minute / 5-minute / 15-minute
load averages at the start of each run were 3.27 / 2.63 / 2.76 (run 1), 1.92 / 1.98 / 2.34 (run 2) and 6.51 / 6.39 /
4.81 (run 3), ending at 1.92, 6.51 and 9.53 (1 minute): other sessions began building during runs 2 and 3. Run 3 (the
slowest `readyMs` and the highest `cpuMsIdle`) therefore ran under load; the median is run 2 for `readyMs` and run 1
for the others, and the Rust figures are below the Swift baseline in every run. **Not measured: the Parallels VM row.**
The VM was suspended when this work started and its "Pause idle" setting is on, which breaks a 600 s idle
measurement; changing it needs the owner. The ARM64 comparison is therefore against the hosted `windows-11-arm`
row only. The .NET suite ran against the Rust library on all of these platforms (66 tests, floor 59): see the pull
request.

## Windows app, Swift engine versus Rust engine

The whole Windows app (`timetug-windows`) with each engine, on the Parallels VM `Windows 11` (Windows 11 Pro build
26200, ARM64, 6 vCPUs, 16 GB; .NET 10.0.15 desktop runtime; no Visual C++ redistributable installed) on the
development Mac, measured 2026-11-12 (phase 4, Task 4). Both builds are the CI development package
(`msix-arm64`, package `BinaryCompanion.TimeTug.Dev`, version 0.1.0.0):

- **Swift engine:** `main` of `timetug-windows` at `3fb1f90` (`TimeTug.Engine` 1.8.x).
- **Rust engine:** the trial branch `rust-engine-trial` at `cc65e6b` (`TimeTug.Engine` 2.0.0-alpha.1; draft pull request
  `binary-companion/timetug-windows#54`, CI run 43301880174).

Measured with `scripts/vm/vm-measure.ps1` (`timetug-windows`), in the signed-in user's session. Each run stops the
app, starts it fresh from the same account-free state (no calendar account; the welcome window is open, as on any first
start), and records: installed size (summed file lengths of the package's install folder, once per package); time from
process start to the tray icon (the first time the tray library's `H.NotifyIcon_*` window exists in the process, polled
every 20 ms); working set and private bytes of the app's processes 300 s after process start; CPU time used over the
next 600 s of idle. Three runs per build, the median recorded, each build installed fresh before its runs. The Parallels
setting "Pause idle" was off for the runs and has been restored to on. The script holds the VM awake while it runs.

The dev package carries both the x64 and ARM64 engine libraries (and the x64/ARM64/x86 MSAL runtime), so installed
size here is larger than a single-architecture release package; the comparison is like with like.

| Metric | Swift engine (median) | Rust engine (median) | Rust / Swift | Gate |
|---|---:|---:|---:|---|
| Installed size (bytes) | 386,735,028 | 245,672,288 | 0.64 | pass (lower) |
| Files in the package | 181 | 147 | 0.81 | |
| MSIX file, ARM64 (bytes) | 143,480,981 | 91,738,516 | 0.64 | |
| MSIX file, x64 (bytes, CI artifact) | 143,459,224 | 91,716,806 | 0.64 | |
| Engine native files per architecture | 21 (`TimeTugEngineC.dll` 13,896,192 + Swift runtime and Foundation DLLs) | 1 (`TimeTugEngineC.dll` 3,351,040) | | |
| Working set at 5 min (bytes) | 211,439,616 (batch 1), 204,705,792 (batch 2) | 194,736,128 | 0.92 to 0.96 | pass |
| Private bytes at 5 min (bytes) | 38,387,712 (batch 1), 40,488,960 (batch 2) | 38,146,048 | 0.94 to 0.99 | pass |
| Start to tray icon (ms) | 792 (batch 1), 682 (batch 2) | 722 | 0.91 to 1.06 | pass |
| Idle CPU over 600 s (s) | 0.625 (batch 1), 2.563 (batch 2) | 2.422 | 3.9 (batch 1), 0.95 (batch 2) | see below |

Individual runs. Swift batch 1 and Rust were run back to back on the same VM session; Swift batch 2 is a repeat of the
Swift build (reinstalled) after the Rust runs, made because of the idle CPU spread.

| Build | Run | Start to tray (ms) | Working set (bytes) | Private bytes (bytes) | Idle CPU 600 s (s) |
|---|---|---:|---:|---:|---:|
| Swift, batch 1 | 1 | 1,379 | 212,881,408 | 40,222,720 | 2.422 |
| | 2 | 673 | 211,353,600 | 38,277,120 | 0.406 |
| | 3 | 792 | 211,439,616 | 38,387,712 | 0.625 |
| | **Median** | **792** | **211,439,616** | **38,387,712** | **0.625** |
| Rust | 1 | 1,134 | 194,842,624 | 38,391,808 | 2.422 |
| | 2 | 568 | 194,736,128 | 38,146,048 | 2.484 |
| | 3 | 722 | 194,326,528 | 37,732,352 | 2.329 |
| | **Median** | **722** | **194,736,128** | **38,146,048** | **2.422** |
| Swift, batch 2 | 1 | 1,362 | 206,618,624 | 40,767,488 | 2.047 |
| | 2 | 598 | 204,091,392 | 39,956,480 | 2.812 |
| | 3 | 682 | 204,705,792 | 40,488,960 | 2.563 |
| | **Median** | **682** | **204,705,792** | **40,488,960** | **2.563** |

Pooled over all six Swift runs: start to tray 737 ms, working set 208,986,112, private bytes 40,089,600, idle CPU
2.23 s (Rust medians as above).

**Reading the table.**

- **Size:** the Rust package is 36% smaller installed and 36% smaller as an MSIX. The engine's own native payload fell
  from 13.9 MB plus the Swift runtime and Foundation DLLs to one 3.4 MB DLL per architecture.
- **Memory:** the Rust build is 4% to 8% lower in working set and equal or slightly lower in private bytes. The app is
  dominated by .NET and WinUI (about 195 to 211 MB working set), so the engine change is a small share.
- **Start to tray:** the first start after a fresh install is slower for both builds (about 1.1 to 1.4 s); later starts are
  0.57 to 0.79 s. Rust is within noise of Swift.
- **Idle CPU (anomaly):** Swift batch 1 measured 0.41 to 0.63 s in two of its three runs, which the Rust build did not
  reproduce (2.33 to 2.48 s in all three runs); against that batch alone the Rust build is 3.9 times higher and fails
  the 10% tolerance. The Swift repeat (batch 2) measured 2.05 to 2.81 s, the same as Rust, so the pooled Swift median
  (2.23 s) is 8% below Rust's (2.42 s), inside the tolerance, and one run of batch 1 (run 1) also sat at 2.42 s. The
  samples show the same pattern in the high runs for both builds: a steady 30 to 150 ms of CPU in every 15 s, with no
  spike, from the app as a whole (the process includes the .NET runtime, WinUI and the tray). The low runs of batch 1
  were 0 to 60 ms per 15 s with long zero stretches. No instrumented comparison of the engine's own threads was made, so
  the data cannot say whether the difference between the two batches is the machine (Defender, a Windows background
  task, the host) or the build. The absolute figures are about 0.4% of one core. This metric should be re-measured
  before the gate is called passed: ideally interleaved runs (Swift, Rust, Swift, Rust) and, in addition, the engine
  harness numbers above, in which Rust used half the idle CPU of Swift on every machine.
- **Gate (installed size lower; memory, idle CPU and start time not worse, 10% tolerance on time and CPU):** met for
  size, memory and start time; met for idle CPU against the pooled Swift median and the second Swift batch, not met
  against the first Swift batch alone (see above).

### Sleep and resume (Parallels suspend)

For each build: start the app fresh (60 s settle), `scripts/vm/vm-suspend.sh`, 180 s suspended, `vm-up.sh` (resume),
then a probe 5 s, 65 s and 125 s after the resume (process list, `Responding`, working set, private bytes, CPU time,
a search with UI Automation for a window titled "TimeTug couldn’t start", a second launch of the package alias, and the
number of error and warning lines in `timetug.log`).

| Build | Same process after resume | Responding | "couldn’t start" window | Second launch exits (ms) before / +5 s / +65 s / +125 s | Error or warning lines in the log | CPU after resume (s, +5 s to +125 s) |
|---|---|---|---|---|---|---|
| Swift engine | yes (one process, same id) | yes at all three probes | none | 300 / 313 / 296 / 292 (exit code 0) | 0 | 1.97 to 2.38 |
| Rust engine | yes | yes at all three probes | none | 1,439 / 768 / 484 / 279 (exit code 0) | 0 | 1.80 to 1.97 |

Both builds survive a suspend and resume of the VM with the app and its engine intact and answering (the second launch
reaches the running instance and exits 0). The first second launch of the Rust run was slower (1.4 s) and settled to the
Swift level by the third probe. **Not covered:** one late takeover across a meeting start. It needs a calendar account
with a meeting, which the VM does not have (the existing Windows account is not connected to TimeTug), and a test tug is
immediate, not scheduled. That stays an owner-attended item (checklist 8.7).

### Not measured

- **x64.** No x64 Windows machine or runner was available without new sign-ins or setup (the VM runs only the ARM64
  .NET desktop runtime, the hosted x64 runner cannot take these measurements). The x64 MSIX sizes from CI are in the
  table (143,459,224 bytes Swift, 91,716,806 bytes Rust).
- **A single-architecture release package.** The numbers are for the development package, which holds both engine
  architectures.
- **The release (Store or Direct) flavour,** and a machine with real accounts (a connected account adds sync work and
  memory).
