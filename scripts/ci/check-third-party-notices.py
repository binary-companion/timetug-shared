#!/usr/bin/env python3
"""Fails when THIRD-PARTY-NOTICES.md and the dependency tree of the engine library disagree.
Usage: check-third-party-notices.py [--notices FILE] [--tree-file FILE]

Phase 3 task 9: the Windows package ships the Rust library, so the licence of every crate linked into it travels
with it. THIRD-PARTY-NOTICES.md holds one `## <name> <version>` section per crate in the tree printed by

  cargo tree -e normal --prefix none -p timetug-engine-c --target all

(`--target all`, not the host default: the tree of the host leaves out the crates only another platform links, for
example the bundled zone database `jiff-tzdb`, which ships in the Windows library). The crates of this workspace
(path dependencies) are not listed: their licence is the LICENSE of the package. Each section names its licence
(`License: <SPDX expression>`) and holds the licence text in a fenced block.

Exits 1, one line per problem, when a crate of the tree has no section, a section names a crate or version no longer
in the tree (a bumped dependency leaves the old version behind), a section appears twice, or a section has no licence
line or no licence text. --tree-file reads the output of the cargo command from a file instead of running it (the
tests do). scripts/dev/generate-third-party-notices.py writes the file."""
import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TREE_COMMAND = ['cargo', 'tree', '-e', 'normal', '--prefix', 'none', '-p', 'timetug-engine-c', '--target', 'all']
# A line of `cargo tree --prefix none`: `name vX.Y.Z`, then optionally `(proc-macro)`, `(<path or url>)`, `(*)`.
TREE_LINE = re.compile(r'^(?P<name>[A-Za-z0-9_.\-]+) v(?P<version>\S+)(?P<rest>.*)$')
PATH_SOURCE = re.compile(r'\((?:/|[A-Za-z]:[\\/])')
HEADING = re.compile(r'^## (?P<name>\S+) (?P<version>\S+)\s*$')
MINIMUM_TEXT = 100  # characters; the shortest real licence text (a short BSD or ISC) is longer


def tree_crates(text):
    """The (name, version) pairs of a cargo tree listing, without the crates of this workspace."""
    crates = set()
    for line in text.splitlines():
        match = TREE_LINE.match(line.strip())
        if match is None:
            continue
        if PATH_SOURCE.search(match.group('rest')):
            continue
        crates.add((match.group('name'), match.group('version')))
    return crates


def notice_sections(text):
    """[(name, version, body)] for each `## name version` section; the title and the preamble are not sections."""
    sections = []
    current = None
    for line in text.splitlines():
        match = HEADING.match(line)
        if match:
            current = [match.group('name'), match.group('version'), []]
            sections.append(current)
        elif current is not None:
            current[2].append(line)
    return [(name, version, '\n'.join(body)) for name, version, body in sections]


def fenced_text(body):
    """The characters inside the fenced blocks of a section body."""
    blocks = re.findall(r'^(`{3,})[^\n]*\n(.*?)\n\1\s*$', body, flags=re.S | re.M)
    return ''.join(text for _, text in blocks)


def problems(tree, notices):
    """What is wrong (empty when nothing is). `tree` is the cargo tree output, `notices` the markdown."""
    wanted = tree_crates(tree)
    found = []
    if not wanted:
        found.append('the dependency tree lists no crates (is the cargo tree output empty?)')
    sections = notice_sections(notices)
    seen = {}
    for name, version, body in sections:
        key = (name, version)
        if key in seen:
            found.append(f'{name} {version} appears twice')
            continue
        seen[key] = body
        if key not in wanted:
            found.append(f'{name} {version} is listed but is not in the dependency tree any more')
        if not re.search(r'^License: \S', body, flags=re.M):
            found.append(f'{name} {version} has no "License:" line')
        if len(fenced_text(body).strip()) < MINIMUM_TEXT:
            found.append(f'{name} {version} has no licence text')
    for name, version in sorted(wanted - set(seen)):
        found.append(f'{name} {version} is in the dependency tree but has no section')
    return found


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--notices', default=str(ROOT / 'THIRD-PARTY-NOTICES.md'))
    parser.add_argument('--tree-file')
    args = parser.parse_args(argv)
    try:
        notices = Path(args.notices).read_text(encoding='utf-8')
    except OSError as error:
        print(f'error: cannot read {args.notices}: {error}', file=sys.stderr)
        return 1
    if args.tree_file:
        try:
            tree = Path(args.tree_file).read_text(encoding='utf-8')
        except OSError as error:
            print(f'error: cannot read {args.tree_file}: {error}', file=sys.stderr)
            return 1
    else:
        run = subprocess.run(TREE_COMMAND, cwd=ROOT, capture_output=True, text=True)
        if run.returncode != 0:
            print(f'error: {" ".join(TREE_COMMAND)} failed:\n{run.stderr}', file=sys.stderr)
            return 1
        tree = run.stdout
    found = problems(tree, notices)
    for problem in found:
        print(f'error: {Path(args.notices).name}: {problem}', file=sys.stderr)
    if found:
        print('regenerate it with scripts/dev/generate-third-party-notices.py', file=sys.stderr)
        return 1
    print(f'{Path(args.notices).name}: {len(tree_crates(tree))} crates, all with a licence and its text')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
