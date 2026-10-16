#!/usr/bin/env python3
"""Stamps the release version into the engine before the package is built: replaces the development value of
`EngineBuild.version` ("0.0.0-dev") in Sources/TimeTugEngineC/EngineSession.swift, so `ready.engineVersion` is the
timetug-shared tag the package carries.

Usage: stamp-engine-version.py VERSION [FILE]

Exits 1, changing nothing, when VERSION is not X.Y.Z (with an optional -prerelease) or the file does not hold the
development value exactly once."""
import re
import sys
from pathlib import Path

DEVELOPMENT = 'static let version = "0.0.0-dev"'
VERSION = re.compile(r'^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$')

def main(argv):
    if len(argv) not in (1, 2):
        print('usage: stamp-engine-version.py VERSION [FILE]', file=sys.stderr)
        return 2
    version = argv[0]
    path = Path(argv[1]) if len(argv) == 2 else Path(__file__).resolve().parents[2] / 'Sources' / 'TimeTugEngineC' / 'EngineSession.swift'
    if not VERSION.match(version):
        print(f'error: not a package version: {version!r}', file=sys.stderr)
        return 1
    # newline='' keeps the file's line endings on Windows too.
    with open(path, encoding='utf-8', newline='') as handle:
        text = handle.read()
    if text.count(DEVELOPMENT) != 1:
        print(f'error: {path} must hold {DEVELOPMENT} exactly once', file=sys.stderr)
        return 1
    with open(path, 'w', encoding='utf-8', newline='') as handle:
        handle.write(text.replace(DEVELOPMENT, f'static let version = "{version}"'))
    print(f'EngineBuild.version = {version} ({path})')
    return 0

if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
