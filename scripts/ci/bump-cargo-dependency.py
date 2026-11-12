#!/usr/bin/env python3
"""Moves the calendar-connectors git dependency of the Cargo workspace to a released tag.
Usage: bump-cargo-dependency.py VERSION [--manifest Cargo.toml]    (Cargo.lock is the file next to it)
       bump-cargo-dependency.py --is-older OLD NEW      (exit 0 when OLD sorts below NEW, else 1)

Rewrites the `rev = "..."` or `tag = "..."` of every `[workspace.dependencies]` line whose git URL is the
calendar-connectors repository to `tag = "VERSION"`, then runs `cargo update -p <crate>` for each of those crates so
Cargo.lock follows (the command is `cargo`, or $CARGO). Nothing but those values changes in the manifest. The lock
is read again afterwards and must pin every connector crate at VERSION; if cargo fails or the lock does not say so,
both files are restored.

Refuses (exit 2, files untouched): a VERSION that is not X.Y.Z or X.Y.Z-alpha.N (each number at most 9 digits so a
hostile tag cannot overflow anything downstream), a manifest without connector dependencies, a dependency that is
pinned by branch, a move back to a lower version (prereleases sort below their release: 2.0.0-alpha.2 < 2.0.0), a
change of major version (it needs a manual change), and connector lines that disagree about their current tag. The same version is a no-op (exit 0). A move from a
revision to a tag cannot be compared and is accepted.
"""
import importlib.util
import os
import re
import subprocess
import sys
from pathlib import Path

NUMBER = r'(0|[1-9][0-9]{0,8})'
VERSION = re.compile(rf'{NUMBER}\.{NUMBER}\.{NUMBER}(?:-alpha\.{NUMBER})?')  # used with fullmatch
REPOSITORY = 'https://github.com/binary-companion/calendar-connectors'
LINE = re.compile(r'^(?P<name>[A-Za-z0-9_-]+)(?P<space>\s*=\s*)\{(?P<body>[^{}\n]*)\}(?P<tail>\s*(?:#.*)?)$')
GIT = re.compile(r'\bgit\s*=\s*"(?P<url>[^"]*)"')
PIN = re.compile(r'\b(?P<key>rev|tag|branch)\s*=\s*"(?P<value>[^"]*)"')


def fail(message):
    print(message, file=sys.stderr)
    sys.exit(2)


def key(version):
    """Sort key of X.Y.Z[-alpha.N]: a prerelease sorts below its release."""
    match = VERSION.fullmatch(version)
    major, minor, patch, alpha = match.groups()
    return (int(major), int(minor), int(patch), 1 if alpha is None else 0, int(alpha or 0))


def connector_lines(lines):
    """[(index, match)] for the connector dependency lines inside [workspace.dependencies]."""
    found, section = [], None
    for index, line in enumerate(lines):
        header = re.match(r'^\s*\[([^\]]+)\]\s*(?:#.*)?$', line)
        if header:
            section = header.group(1).strip()
            continue
        if section != 'workspace.dependencies':
            continue
        match = LINE.match(line.rstrip('\n').rstrip('\r'))
        git = GIT.search(match.group('body')) if match else None
        if git and git.group('url').rstrip('/').removesuffix('.git') == REPOSITORY:
            found.append((index, match))
    return found


def load_lock_reader():
    path = Path(__file__).with_name('cargo-dependency-version.py')
    spec = importlib.util.spec_from_file_location('cargo_dependency_version', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main(argv):
    if len(argv) == 3 and argv[0] == '--is-older':
        for version in argv[1:]:
            if not VERSION.fullmatch(version):
                fail(f'not X.Y.Z or X.Y.Z-alpha.N: {version!r}')
        return 0 if key(argv[1]) < key(argv[2]) else 1
    usage = 'usage: bump-cargo-dependency.py VERSION [--manifest Cargo.toml]'
    manifest, version = Path('Cargo.toml'), None
    args = list(argv)
    while args:
        arg = args.pop(0)
        if arg == '--manifest' and args:
            manifest = Path(args.pop(0))
        elif version is None and not arg.startswith('--'):
            version = arg
        else:
            fail(usage)
    if version is None:
        fail(usage)
    lock = manifest.with_name('Cargo.lock')
    if not VERSION.fullmatch(version):
        fail(f'not X.Y.Z or X.Y.Z-alpha.N (each number at most 9 digits): {version!r}')
    for path in (manifest, lock):
        if not path.is_file():
            fail(f'{path} not found')
    try:
        original = manifest.read_bytes().decode('utf-8')  # bytes in, bytes out: line endings are not translated
        original_lock = lock.read_bytes()
    except (OSError, UnicodeDecodeError) as problem:
        fail(f'cannot read {manifest} or {lock}: {problem}')
    lines = original.splitlines(keepends=True)
    found = connector_lines(lines)
    if not found:
        fail(f'no calendar-connectors git dependency in [workspace.dependencies] of {manifest}')

    current_tags, names = set(), []
    for _, match in found:
        pin = PIN.search(match.group('body'))
        if pin is None:
            fail(f'the dependency {match.group("name")} has no rev or tag; this needs a manual change')
        if pin.group('key') == 'branch':
            fail(f'the dependency {match.group("name")} is pinned by branch; this needs a manual change')
        if pin.group('key') == 'tag':
            current_tags.add(pin.group('value'))
        names.append(match.group('name'))
    if len(current_tags) > 1:
        fail(f'the connector dependencies are pinned at different tags ({", ".join(sorted(current_tags))}); fix them by hand')
    if len({PIN.search(m.group('body')).group('key') for _, m in found}) > 1:
        fail('the connector dependencies mix rev and tag pins; fix them by hand')
    if current_tags:
        current = current_tags.pop()
        if not VERSION.fullmatch(current):
            fail(f'the current tag {current!r} is not X.Y.Z or X.Y.Z-alpha.N; this needs a manual change')
        if current == version:
            print(f'calendar-connectors is already pinned at {version}; nothing to do')
            return 0
        if key(version) < key(current):
            fail(f'refusing to move calendar-connectors back from {current} to {version}')
        if key(version)[0] != key(current)[0]:
            fail(f'calendar-connectors {version} is in another major than {current}; this needs a manual change')
    else:
        current = None

    edited = list(lines)
    for index, match in found:
        body = PIN.sub(f'tag = "{version}"', match.group('body'), count=1)
        newline = lines[index][len(lines[index].rstrip('\r\n')):]
        edited[index] = f'{match.group("name")}{match.group("space")}{{{body}}}{match.group("tail")}{newline}'
    new_text = ''.join(edited)
    changed = [i for i, (a, b) in enumerate(zip(lines, edited)) if a != b]
    if changed != [index for index, _ in found]:
        fail('the edit would change more than the connector dependency lines; nothing was written')

    cargo = os.environ.get('CARGO', 'cargo')
    command = [cargo, 'update', '--manifest-path', str(manifest)]
    for name in names:
        command += ['-p', name]
    manifest.write_bytes(new_text.encode('utf-8'))
    try:
        subprocess.run(command, check=True)
        reader = load_lock_reader()
        tag, why = reader.pinned_tag(lock.read_text(encoding='utf-8'), 'calendar-connectors')
        if tag != version:
            raise RuntimeError(why or f'Cargo.lock pins calendar-connectors at {tag}, not {version}')
    except (subprocess.CalledProcessError, OSError, RuntimeError) as problem:
        manifest.write_bytes(original.encode('utf-8'))
        lock.write_bytes(original_lock)
        fail(f'could not move calendar-connectors to {version}: {problem}; {manifest} and {lock} were restored')
    print(f'calendar-connectors pinned at {version}' + (f' (was {current})' if current else ' (was a revision)'))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
