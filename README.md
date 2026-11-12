# timetug-shared

The platform-neutral logic of [TimeTug](https://github.com/binary-companion/timetug), as a Rust workspace on top of [calendar-connectors](https://github.com/binary-companion/calendar-connectors): the calendar store, duplicate resolution, takeover scheduling, agenda and widget snapshot (`timetug-core`, `timetug-bridge`), and the engine (`timetug-engine`, `timetug-engine-protocol`) behind a small C interface (`timetug-engine-c`, `include/timetug_engine.h`). It builds and is tested on macOS, Linux, Windows x64 and Windows ARM64.

This is the `v2` branch, the Rust rewrite. The Swift package it replaces is gone from it; `main` still holds that package and releases `1.x` until `v2` replaces it (see `docs/rust-port/README.md`).

## Use

The C interface library is `TimeTugEngineC.dll`, `libTimeTugEngineC.dylib` or `libTimeTugEngineC.so`, built with `cargo build --release -p timetug-engine-c` (`target/release`); four functions, `tt_engine_create`, `tt_engine_send`, `tt_engine_reply` and `tt_engine_destroy`, declared in `include/timetug_engine.h` and speaking the JSON contract v1 (one golden example per message in `Conformance/Fixtures/golden`, end-to-end scripts in `Conformance/Scenarios`). The .NET wrapper `dotnet/TimeTug.Engine` ships as the NuGet package `TimeTug.Engine` with one native DLL per Windows architecture (at most 8 MB each, `docs/rust-port/size.md`). Core never reads the clock: callers pass `now`.

```sh
cargo test --workspace            # the whole suite
c-smoke/run.sh                    # the C header against the built library
c-harness/run.sh                  # the C concurrency harness
```

## Releases

On `main`, every merge publishes a version tag (`X.Y.Z`) and a GitHub release; on `v2`, the owner dispatches the `Release` workflow with a `prerelease` such as `alpha.1` to publish `2.0.0-alpha.1` (a tag, a GitHub prerelease and a prerelease package; nobody is notified). The pull request's `semver:major`, `semver:minor` or `semver:patch` label chooses the bump; patch is the default. Within a major version a release must work in an existing front end unchanged: the `compat` CI checks fail a removal, a retyping or a changed value (adding fields is fine) unless the pull request is labelled `semver:major` (see `AGENTS.md`). Release notes record the `calendar-connectors` version the code was built and tested with. Published tags are immutable. A release notifies `timetug`, which opens the pull request that takes the new version, and a `calendar-connectors` release opens a bump pull request here (`.github/workflows/bump-dependencies.yml`); see `AGENTS.md` for the chain and how to recover a missed dispatch.

## License

MIT with the Commons Clause (no selling the software). See [LICENSE](LICENSE).
