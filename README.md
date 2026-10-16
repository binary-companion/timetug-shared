# timetug-shared

The platform-neutral logic of [TimeTug](https://github.com/binary-companion/timetug): `TimeTugCore` (the calendar store, duplicate resolution, takeover scheduling, agenda and widget snapshot) and `CalendarBridge` (the adapter between the connector library and Core). Pure Swift on top of [calendar-connectors](https://github.com/binary-companion/calendar-connectors); it builds and is tested on macOS, Linux, Windows x64 and Windows ARM64.

## Use

```swift
.package(url: "https://github.com/binary-companion/timetug-shared", from: "1.0.0")
```

Products: `TimeTugCore`, `CalendarBridge`. Core never reads the clock: callers pass `now: Date`.

## Releases

Every merge to `main` publishes a version tag (`X.Y.Z`) and a GitHub release. The pull request's `semver:major`, `semver:minor` or `semver:patch` label chooses the bump; patch is the default. Within a major version a release must work in an existing front end unchanged: the `compat` CI checks fail a removal, a retyping or a changed fixture unless the pull request is labelled `semver:major` (see `AGENTS.md`). Release notes record the `calendar-connectors` version the code was built and tested with. Published tags are immutable. A release notifies `timetug`, which opens the pull request that takes the new version, and a `calendar-connectors` release opens a bump pull request here (`.github/workflows/bump-dependencies.yml`); see `AGENTS.md` for the chain and how to recover a missed dispatch.

## License

MIT with the Commons Clause (no selling the software). See [LICENSE](LICENSE).
