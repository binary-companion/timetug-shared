## What and why

## Version

Add exactly one label before merging; a merge to `main` releases automatically.

- `semver:major`: a breaking change: anything removed, retyped or changed in meaning in a public API, the C interface,
  the wire contract or a persisted format; a non-additive edit of an existing fixture or scenario; any edit of a
  persisted-data fixture. Without it the `compat`
  and `compat-dotnet` checks fail such a change (AGENTS.md, "Backward compatibility").
- `semver:minor`: new API
- `semver:patch`: fixes and documentation (the default when no label is set)
