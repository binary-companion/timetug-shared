#!/usr/bin/env python3
"""Moves one pin of Package.resolved to a released version. Usage: bump-resolved.py SLUG X.Y.Z REVISION [FILE]

Edits the text of the pin's `revision` and `version` values only (SwiftPM's own formatting and `originHash`
stay as they are; CI proves the result with `swift test --force-resolved-versions`). The edited text is parsed
again and compared with the original before anything is written, so a result that changed anything but those
two values is never saved.

Refuses (exit 2, file untouched): a malformed version (X.Y.Z, each number at most 9 digits so a hostile tag
cannot overflow anything downstream) or revision (40 lowercase hex characters), an unknown or ambiguous pin,
a downgrade, the same version at another revision (a moved tag), a version below the `from:` floor, and a change
of major relative to the `from:` floor in the Package.swift next to FILE. When Package.swift or the floor
cannot be found the major cannot be judged, so that refuses too. The same version at the same revision is a
no-op (exit 0).
"""
import copy
import json
import re
import sys
from pathlib import Path

NUMBER = r'(0|[1-9][0-9]{0,8})'
SEMVER = re.compile(rf'{NUMBER}\.{NUMBER}\.{NUMBER}')  # used with fullmatch: no trailing newline, ASCII digits
REVISION = re.compile(r'[0-9a-f]{40}')
SLUG = re.compile(r'[a-z0-9][a-z0-9._-]*')


def fail(message):
    print(message, file=sys.stderr)
    sys.exit(2)


def as_tuple(text):
    return tuple(int(part) for part in text.split('.'))


def floor_of(manifest, slug):
    """The (major, minor, patch) of the `from:` requirement of the package whose URL ends in /SLUG."""
    if not manifest.is_file():
        fail(f'{manifest} not found; the major version cannot be checked')
    pattern = r'url:\s*"[^"]*/%s(?:\.git)?"\s*,\s*from:\s*"([^"]*)"' % re.escape(slug)
    try:
        text = manifest.read_text()
    except (OSError, UnicodeDecodeError) as problem:
        fail(f'cannot read {manifest}: {problem}')
    floors = re.findall(pattern, text)
    if len(floors) != 1:
        fail(f'cannot find exactly one `from:` floor for {slug} in {manifest}; the major version cannot be checked')
    if not SEMVER.fullmatch(floors[0]):
        fail(f'the from: floor of {slug} in {manifest} is not X.Y.Z: {floors[0]}')
    return as_tuple(floors[0])


def main(argv):
    if len(argv) not in (4, 5):
        fail('usage: bump-resolved.py SLUG X.Y.Z REVISION [FILE]')
    slug, version, revision = argv[1:4]
    path = Path(argv[4] if len(argv) == 5 else 'Package.resolved')
    if not SLUG.fullmatch(slug):
        fail(f'not a package identity: {slug!r}')
    if not SEMVER.fullmatch(version):
        fail(f'not X.Y.Z (at most 9 digits per number): {version!r}')
    if not REVISION.fullmatch(revision):
        fail(f'not a 40-character lowercase hex revision: {revision!r}')
    if not path.is_file():
        fail(f'{path} not found')
    try:
        text = path.read_bytes().decode('utf-8')  # bytes in, bytes out: line endings are not translated
    except (OSError, UnicodeDecodeError) as problem:
        fail(f'cannot read {path}: {problem}')
    try:
        before = json.loads(text)
    except ValueError as problem:
        fail(f'{path} is not valid JSON: {problem}')
    pins = before.get('pins') if isinstance(before, dict) else None
    if not isinstance(pins, list):
        fail(f'{path} has no pins list')
    matching = [p for p in pins if isinstance(p, dict) and p.get('identity') == slug]
    if not matching:
        fail(f'no pin {slug} in {path}')
    if len(matching) > 1:
        fail(f'more than one pin {slug} in {path}')
    state = matching[0].get('state')
    current = state.get('version') if isinstance(state, dict) else None
    if not isinstance(current, str) or not SEMVER.fullmatch(current):
        fail(f'pin {slug} has no X.Y.Z version (at most 9 digits per number)')
    if as_tuple(version) < as_tuple(current):
        fail(f'refusing to move {slug} back from {current} to {version}')
    if version == current:
        if state.get('revision') == revision:
            print(f'{slug} is already resolved at {version}; nothing to do')
            return
        fail(f'{slug} is resolved at {version} with revision {state.get("revision")} but {version} now points at '
             f'{revision}; a moved tag needs a manual change')
    floor = floor_of(path.parent / 'Package.swift', slug)
    if as_tuple(version)[0] != floor[0]:
        fail(f'{slug} {version} is in another major than from: {floor[0]}.x; this needs a manual change')
    if as_tuple(version) < floor:
        fail(f'{slug} {version} is below the from: floor {".".join(map(str, floor))} of Package.swift')

    # The text edit: the pin's own object, found by its identity, then its two values inside that object.
    block_pattern = re.compile(r'\{[^{}]*"identity"\s*:\s*"%s"[^{}]*\{[^{}]*\}[^{}]*\}' % re.escape(slug))
    blocks = list(block_pattern.finditer(text))
    if len(blocks) != 1:
        fail(f'cannot locate the text of pin {slug} in {path}')
    block = blocks[0].group(0)
    new_block, revisions = re.subn(r'("revision"\s*:\s*")[0-9a-f]+(")', rf'\g<1>{revision}\g<2>', block)
    new_block, versions = re.subn(r'("version"\s*:\s*")[^"]+(")', rf'\g<1>{version}\g<2>', new_block)
    if (revisions, versions) != (1, 1):
        fail(f'cannot locate the revision and version of pin {slug} in {path}')
    new_text = text[:blocks[0].start()] + new_block + text[blocks[0].end():]

    expected = copy.deepcopy(before)
    for pin in expected['pins']:
        if isinstance(pin, dict) and pin.get('identity') == slug:
            pin['state']['revision'] = revision
            pin['state']['version'] = version
    try:
        parsed = json.loads(new_text)
    except ValueError as problem:
        fail(f'the edited {path} is not valid JSON ({problem}); nothing was written')
    if parsed != expected:
        fail(f'the edit would change more than the revision and version of {slug}; nothing was written')
    try:
        path.write_bytes(new_text.encode('utf-8'))
    except OSError as problem:
        fail(f'cannot write {path}: {problem}')
    print(f'{slug} resolved at {version} (was {current})')


main(sys.argv)
