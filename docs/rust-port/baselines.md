# Resource baselines of the Swift engine

What the Rust engine must meet or beat (design section 6.4, phase 3 gate). Measured on the Swift engine library of
`timetug-shared` `v2` at commit `d7b4aa3` (Swift sources identical to the `1.8.1` baseline tag apart from the
`VectorExporter` target and the Rust workspace, neither of which is in the engine library). Every number below was
measured; the one row that was not (the Parallels VM) is marked TODO with the reason.

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

TODO (not measured): the Parallels VM `Windows 11` on the development Mac. It was suspended when this task ran, and
the `timetug-windows` repository's VM scripts are not checked out on this machine; per the plan's rule the VM was
not resumed or reconfigured. The hosted ARM64 runner above is a native Windows ARM64 machine and stands in for it;
add a VM row if a like-for-like reading on the development hardware is wanted.

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

`scripts/benchmarks/run-dedup.sh` (`swift run -c release DedupBenchmark`), development Mac (Apple M1 Max, 64 GB,
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
