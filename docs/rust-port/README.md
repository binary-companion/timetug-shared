# Rust port

This directory holds the working documents of the Rust port of `timetug-shared` on the `v2` branch.
The Swift package this library started as is gone from `v2` (phase 3 task 10); baseline tag `1.8.1` is the last Swift release `v2` descends from.
`parity-ledger.md` lists behaviour changes merged to `main` (Swift) after the baseline that Rust must still receive.
The program tracker is in the `timetug` repository: `docs/superpowers/plans/2026-11-10-rust-engine-program-tracker.md`.
The porting conventions are in the same place: `docs/superpowers/plans/2026-11-10-rust-port-conventions.md`.

## Last dual-language commit

`268d3cddf07316dd2854d1fa2601a2db25d3b04e` (tip of `v2` before phase 3 task 10 removed the Swift package): the last commit that has both the Swift sources and the Rust workspace, so the Swift engine library can still be built and compared with the Rust one. Check it out to run either side against the other (`git checkout 268d3cd`, then `swift test` or `c-harness/run.sh --swift`). No git tag marks it (agents do not tag); this line is the record.
