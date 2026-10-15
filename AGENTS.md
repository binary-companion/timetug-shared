# timetug-shared agent guide

## What this is

The portable logic shared by every TimeTug front end: `TimeTugCore` and `CalendarBridge` (and, in later phases, the engine and its C interface). It imports only Foundation, `CalendarCore` (from `calendar-connectors`) and its own modules, and builds on macOS, Linux and Windows. UI, Apple frameworks and platform adapters never live here.

## Repositories

- `calendar-connectors`: the connector library. This repository depends on it by version range and commits `Package.resolved`.
- `timetug-shared` (this repository): released as `X.Y.Z` tags.
- `timetug`: the Mac app; pins this repository exactly in `Apps/macOS/project.yml` (key `TimeTugShared`), and receives a bump pull request from every release here.
- `timetug-windows` (later): the WinUI app.

Dependency direction: `calendar-connectors` <- `timetug-shared` <- the apps.

## Rules

- Core never reads the clock (`now: Date` is passed in); the architecture check enforces it. Test each behaviour with Swift Testing, failing test first.
- Allowed imports: Core `Foundation CalendarCore TimeTugCore Testing`; Bridge adds nothing else. `scripts/ci/check-architecture.sh` enforces it.
- `Package.resolved` is changed only by `scripts/ci/bump-resolved.py` (the bump workflow) or by `swift package update` in a reviewed pull request.

## Verify

`swift test` (macOS, Linux and Windows run in CI), `scripts/ci/check-architecture.sh`, `scripts/ci/check-repository-contracts.py`, the tests under `scripts/ci/tests` and `scripts/release/tests`.

## Release flow

Do not tag by hand. A merge to `main` runs `release.yml`: the `semver:*` labels pick the bump, the workflow tags and publishes, appends "Built and tested with calendar-connectors X.Y.Z" to the release notes, then its `notify` job (environment `bump`, the `timetug-bot` token) dispatches `library-released` (`library` `timetug-shared`, `version` `X.Y.Z`) to `timetug`. `timetug` opens a "Bump timetug-shared to X.Y.Z" pull request that moves the `TimeTugShared` pin and raises its `CalendarConnectors` pin to at least the version this repository's `Package.resolved` records, so the app and the engine run the same connector version. The agent never merges or publishes; the owner does.

## Dependents and the bump chain

The chain is live and was proven by a drill on 2026-10-15: a `calendar-connectors` release dispatched to `timetug` and to this repository; this repository's bump pull request ("Bump calendar-connectors to X.Y.Z", from `.github/workflows/bump-dependencies.yml`) was merged and released a new `timetug-shared`; `timetug` then opened its own "Bump timetug-shared to X.Y.Z", which moved both pins and closed the two older bump pull requests as superseded.

- `calendar-connectors` release: the dispatch arrives here, and `bump-dependencies.yml` opens the connector bump (see CI workflows below).
- `timetug-shared` release: `release.yml` notifies `timetug` (above). Nothing else depends on this repository yet.
- Recover a missed dispatch for `timetug`: `gh workflow run "Bump dependencies" --repo binary-companion/timetug -f library=timetug-shared -f version=X.Y.Z`. Recover one for this repository (connector versions only): `gh workflow run "Bump dependencies" --repo binary-companion/timetug-shared -f version=X.Y.Z`. A failed `notify` job can also be re-run alone from this repository's Actions page; the workflows are idempotent, so a repeat opens nothing new.

## Finding code

- If your agent has a semantic code-index skill (in Claude Code and Codex, the `index` skill), use it in this repo without asking first; this is standing permission. At the start of a task check its status. If this checkout is not indexed, or files changed a lot since the last index, run the (incremental) index. A new checkout or worktree is seeded from the index that the `Code index` workflow publishes from `main` (artifact `qindex`, fetched with `gh`), so the first run only embeds what differs from `main` and takes seconds. Without a compatible published index a full build takes many minutes: start it in the background and keep working with grep until it finishes. Then search the index before broad grepping or reading many files, and read the hit ranges rather than trusting snippets. Use grep for exact identifiers and strings. `.qindexignore` lists paths the index leaves out.
- Without such a skill, start from `Sources/` (one directory per product) and grep.
- Exact references ("who uses or calls this?", definitions, implementations) come from Serena (symbol tools over the sourcekit-lsp language server) when your agent has it; use it before grep. Swift; run `swift build --build-tests` first so sourcekit-lsp has an index.

## CI workflows

- `.github/workflows/index.yml` (`Code index`): on pushes to `main` builds the semantic code index incrementally from the previous `qindex` artifact against a throwaway Qdrant and publishes it as the `qindex` artifact (90 days) for agents' index skill. `scripts/dev/qindex.py` is a copy of the index skill's script (`~/.agents/skills/index/scripts/qindex.py`): keep the two identical (`diff` them) and keep the library versions in the workflow equal to the skill's. The export records the model and chunker version, and the skill refuses an index that does not match and builds locally instead.
- `.github/workflows/bump-dependencies.yml` (`Bump dependencies`): when `calendar-connectors` releases it sends `library-released`, and this workflow (environment `bump`, the `timetug-bot` token, repository `timetug-shared` only) opens a "Bump calendar-connectors to X.Y.Z" pull request that moves the connector pin in `Package.resolved` through `scripts/ci/bump-resolved.py`. It applies no label (merging releases a patch version), never merges, and closes only same-repository bump pull requests for older versions. A missed dispatch is recovered with `gh workflow run "Bump dependencies" -f version=X.Y.Z`. A major version change needs a manual pull request (the script refuses it). Its tests are `scripts/ci/tests/test_bump_resolved.py` and `scripts/ci/tests/test-bump-workflow-step.sh`.
