# UniFFI spike (throwaway, never merged)

Phase 5, Task 1 of the Rust engine program. Pinned `uniffi` 0.32.2. Findings: `docs/bindings-spike.md`.
Standalone Cargo workspace (not part of the repository workspace). Build and test:

    cd spike/uniffi && bash build-xcf.sh release && (cd swift && swift test -c release)
