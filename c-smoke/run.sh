#!/usr/bin/env bash
# Builds the TimeTugEngineC library, checks that it exports the four tt_engine_* functions, compiles
# c-smoke/smoke.c and c-smoke/example.c (the example of docs/api.md) against include/timetug_engine.h with the system C compiler and runs them.
# Usage: c-smoke/run.sh [--rust]   (macOS and Linux; Windows uses scripts/ci/run-c-smoke.ps1)
# The library is the Rust one (cargo build --release -p timetug-engine-c), taken from target/release. --rust is
# accepted and changes nothing: it was the switch while the Swift library still existed.
set -euo pipefail
cd "$(dirname "$0")/.."
case "${1:-}" in
  ""|--rust) ;;
  *) echo "usage: c-smoke/run.sh [--rust]" >&2; exit 2 ;;
esac
cargo build --release --locked -p timetug-engine-c
bin="${CARGO_TARGET_DIR:-target}/release"
bin="$(cd "$bin" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

case "$(uname -s)" in
  Darwin)
    library="$bin/libTimeTugEngineC.dylib"
    exports="$(nm -gU "$library" | awk '{print $NF}' | sed 's/^_//')"
    ;;
  *)
    library="$bin/libTimeTugEngineC.so"
    exports="$(nm -D --defined-only "$library" | awk '{print $NF}')"
    ;;
esac
missing=0
for name in tt_engine_create tt_engine_send tt_engine_reply tt_engine_destroy; do
  if ! grep -qx "$name" <<<"$exports"; then
    echo "error: $library does not export $name" >&2
    missing=1
  fi
done
[ "$missing" = 0 ] || exit 1
echo "exports: tt_engine_create tt_engine_send tt_engine_reply tt_engine_destroy"

cc="${CC:-$(command -v clang || command -v cc)}"
flags=(-std=c11 -Wall -Wextra -Werror -I include c-smoke/smoke.c -L "$bin" -lTimeTugEngineC -Wl,-rpath,"$bin")
"$cc" "${flags[@]}" -o "$work/smoke"
"$work/smoke" "$work/data"

# The example shown in docs/api.md (C interface), proven the same way.
"$cc" -std=c11 -Wall -Wextra -Werror -I include c-smoke/example.c -L "$bin" -lTimeTugEngineC -Wl,-rpath,"$bin" -o "$work/example"
"$work/example" "$work/example-data"

# docs/api.md shows c-smoke/example.c as it is.
python3 - "$PWD" <<'PY'
import pathlib, sys
root = pathlib.Path(sys.argv[1])
example = (root / "c-smoke" / "example.c").read_text().rstrip("\n")
if example not in (root / "docs" / "api.md").read_text():
    sys.exit("docs/api.md no longer shows c-smoke/example.c as it is")
PY
