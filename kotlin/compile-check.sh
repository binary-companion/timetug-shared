#!/usr/bin/env bash
# Compile the generated Kotlin bindings (kotlin/generated) with a pinned kotlinc, JNA and kotlinx-coroutines.
# Downloads go to a cache directory (default: build/kotlin-tools) and are verified against the checksums below
# before use. Needs: bash, curl, unzip, a JDK, and sha256sum or shasum.
set -euo pipefail

KOTLIN_VERSION=2.4.21
KOTLIN_SHA256=7cc140e76daf416a0424a5557f99afd7ca89ebd463ea1101261e3e01c33e75cd
JNA_VERSION=5.19.1
JNA_SHA1=ca303052cd617c1af2e2c8d344c98a706fb63143
COROUTINES_VERSION=1.11.0
COROUTINES_SHA256=d1d75aa01dffbb4d1c520e67e4c4e7f5f6174718e7cb4632412503f2f0e604fa

root="$(cd "$(dirname "$0")/.." && pwd)"
cache="${KOTLIN_TOOLS_CACHE:-$root/build/kotlin-tools}"
mkdir -p "$cache"

sha256_of() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi; }
sha1_of() { if command -v sha1sum >/dev/null; then sha1sum "$1" | cut -d' ' -f1; else shasum -a 1 "$1" | cut -d' ' -f1; fi; }

# fetch URL FILE ALGORITHM EXPECTED: download when missing, always verify, delete and fail on a mismatch.
fetch() {
  local url="$1" file="$2" algo="$3" expected="$4"
  [ -f "$file" ] || curl --fail --location --silent --show-error --output "$file" "$url"
  local actual
  actual="$("${algo}_of" "$file")"
  if [ "$actual" != "$expected" ]; then
    rm -f "$file"
    echo "checksum mismatch for $url: expected $expected, got $actual" >&2
    exit 1
  fi
}

fetch "https://github.com/JetBrains/kotlin/releases/download/v$KOTLIN_VERSION/kotlin-compiler-$KOTLIN_VERSION.zip" \
  "$cache/kotlin-compiler-$KOTLIN_VERSION.zip" sha256 "$KOTLIN_SHA256"
fetch "https://repo1.maven.org/maven2/net/java/dev/jna/jna/$JNA_VERSION/jna-$JNA_VERSION.jar" \
  "$cache/jna-$JNA_VERSION.jar" sha1 "$JNA_SHA1"
fetch "https://repo1.maven.org/maven2/org/jetbrains/kotlinx/kotlinx-coroutines-core-jvm/$COROUTINES_VERSION/kotlinx-coroutines-core-jvm-$COROUTINES_VERSION.jar" \
  "$cache/kotlinx-coroutines-core-jvm-$COROUTINES_VERSION.jar" sha256 "$COROUTINES_SHA256"

kotlinc_dir="$cache/kotlinc-$KOTLIN_VERSION"
if [ ! -x "$kotlinc_dir/kotlinc/bin/kotlinc" ]; then
  rm -rf "$kotlinc_dir"
  mkdir -p "$kotlinc_dir"
  unzip -q "$cache/kotlin-compiler-$KOTLIN_VERSION.zip" -d "$kotlinc_dir"
fi

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
classpath="$cache/jna-$JNA_VERSION.jar:$cache/kotlinx-coroutines-core-jvm-$COROUTINES_VERSION.jar"

find "$root/kotlin/generated" -name '*.kt' -print0 | xargs -0 \
  "$kotlinc_dir/kotlinc/bin/kotlinc" -classpath "$classpath" -d "$out" -Werror -nowarn
echo "kotlin: generated bindings compile ($(find "$root/kotlin/generated" -name '*.kt' | wc -l | tr -d ' ') file(s))"
