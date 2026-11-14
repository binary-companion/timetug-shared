#!/usr/bin/env python3
"""Rewrite the root Package.swift for a release: the path binary target becomes the released XCFramework.

Usage: write-swift-package.py VERSION CHECKSUM [PACKAGE_SWIFT]   (default ./Package.swift, rewritten in place)

VERSION is X.Y.Z, X.Y.Z-alpha.N or (the release dry run) 0.0.0-dryrun.N; CHECKSUM is the 64 hex characters of `swift package compute-checksum`. The binary target
line `.binaryTarget(name: "TimeTugEngineFFI", path: ...)` becomes
`.binaryTarget(name: "TimeTugEngineFFI", url: "<release asset URL>", checksum: "<checksum>")`. Running it again with the same
arguments changes nothing; with other arguments it updates the URL and checksum."""
import re
import sys

REPOSITORY = 'binary-companion/timetug-shared'
VERSION = re.compile(r'^\d+\.\d+\.\d+(-(alpha|dryrun)\.\d+)?$')
CHECKSUM = re.compile(r'^[0-9a-f]{64}$')
TARGET = re.compile(r'\.binaryTarget\(\s*name:\s*"TimeTugEngineFFI",\s*(?:path:\s*"[^"]*"|url:\s*"[^"]*",\s*checksum:\s*"[^"]*")\s*\)')


def rewrite(text, version, checksum):
    if not VERSION.match(version):
        raise ValueError(f'not a release version (X.Y.Z, X.Y.Z-alpha.N or X.Y.Z-dryrun.N): {version!r}')
    if not CHECKSUM.match(checksum):
        raise ValueError(f'not a SwiftPM checksum (64 lowercase hex characters): {checksum!r}')
    url = f'https://github.com/{REPOSITORY}/releases/download/{version}/TimeTugEngineFFI-{version}.xcframework.zip'
    replacement = f'.binaryTarget(name: "TimeTugEngineFFI", url: "{url}", checksum: "{checksum}")'
    found = TARGET.findall(text)
    if len(found) != 1:
        raise ValueError(f'expected exactly one TimeTugEngineFFI binary target, found {len(found)}')
    return TARGET.sub(lambda _: replacement, text)


def main(argv):
    if len(argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    path = argv[2] if len(argv) == 3 else 'Package.swift'
    with open(path, encoding='utf-8') as handle:
        text = handle.read()
    try:
        result = rewrite(text, argv[0], argv[1])
    except ValueError as problem:
        print(f'write-swift-package: {problem}', file=sys.stderr)
        return 1
    if result != text:
        with open(path, 'w', encoding='utf-8') as handle:
            handle.write(result)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
