#!/usr/bin/env python3
"""Writes THIRD-PARTY-NOTICES.md: the licence of every crate linked into the engine library.
Usage: generate-third-party-notices.py [--output FILE]

Takes the crate list from the same cargo tree that scripts/ci/check-third-party-notices.py checks against, the licence
expression and the crate directory from `cargo metadata`, and the licence text from the LICENSE, COPYING, UNLICENSE
and NOTICE files that the crate ships (for a crate of a git workspace, those of the workspace root as well). Run it
after any change to Cargo.lock, with the dependencies fetched (`cargo fetch --locked`), and commit the result.
A crate that ships no licence file stops the run: add its text by hand to LICENSE_TEXT_OVERRIDES below, with the
reason, after checking the licence at the crate's repository."""
import argparse
import importlib.util
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('check_third_party_notices', ROOT / 'scripts/ci/check-third-party-notices.py')
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)

LICENSE_FILE = re.compile(r'^(licen[cs]e|copying|unlicen[cs]e|notice)([-_.].*)?$', re.IGNORECASE)
# {(name, version): text} for crates that ship no licence file; none needed so far.
LICENSE_TEXT_OVERRIDES = {}
# {(name, version): [file names]} for crates that keep their licence texts in a file with another name. r-efi (UEFI
# only, linked on no platform the engine ships on) keeps its MIT, Apache-2.0 and LGPL-2.1 texts in AUTHORS.
EXTRA_LICENSE_FILES = {('r-efi', '6.0.0'): ['AUTHORS']}

HEADER = """# Third-party notices

The TimeTug engine library (`TimeTugEngineC`) is built from the crates below. Each section gives the crate, its
version, its licence and the licence text that the crate ships. This file lists the crates of
`cargo tree -e normal --prefix none -p timetug-engine-c --target all`, that is every crate that can be linked into the
library on any platform, including those that only the Windows build links (the bundled time zone database,
`jiff-tzdb`); crates that only run while building (proc macros, build scripts) are in that list when `cargo tree`
shows them and are kept so the list is stable. The crates of this repository are under its own `LICENSE`.

scripts/ci/check-third-party-notices.py fails when this file and the tree disagree; scripts/dev/generate-third-party-notices.py
writes it. Do not edit the sections by hand.

"""


def metadata():
    run = subprocess.run(['cargo', 'metadata', '--format-version', '1', '--locked', '-q'], cwd=ROOT,
                         capture_output=True, text=True)
    if run.returncode != 0:
        raise SystemExit(f'cargo metadata failed:\n{run.stderr}')
    return json.loads(run.stdout)


def tree():
    run = subprocess.run(checker.TREE_COMMAND, cwd=ROOT, capture_output=True, text=True)
    if run.returncode != 0:
        raise SystemExit(f'cargo tree failed:\n{run.stderr}')
    return run.stdout


def license_files(directory, stop):
    """The licence files in `directory`, then in each parent up to and including `stop` (a workspace root)."""
    seen, found = set(), []
    current = directory
    while True:
        for path in sorted(current.iterdir()):
            if path.is_file() and LICENSE_FILE.match(path.name) and path.name not in seen:
                seen.add(path.name)
                found.append(path)
        if current == stop or current.parent == current:
            return found
        current = current.parent


def checkout_root(package):
    """The directory above which licence files do not belong to the crate."""
    manifest = Path(package['manifest_path']).parent
    source = package.get('source') or ''
    if source.startswith('git+'):
        for parent in [manifest, *manifest.parents]:
            if (parent / '.git').exists() or parent.name.startswith('calendar-connectors'):
                return parent
        return manifest
    return manifest


def fence(text):
    ticks = '```'
    while ticks in text:
        ticks += '`'
    return f'{ticks}\n{text.strip(chr(10))}\n{ticks}'


def render(package):
    key = (package['name'], package['version'])
    files = license_files(Path(package['manifest_path']).parent, checkout_root(package))
    files += [Path(package['manifest_path']).parent / name for name in EXTRA_LICENSE_FILES.get(key, [])]
    blocks = []
    for path in files:
        text = path.read_text(encoding='utf-8', errors='replace').replace('\r\n', '\n')
        if text.strip():
            blocks.append((path.name, text))
    if not blocks and key in LICENSE_TEXT_OVERRIDES:
        blocks.append(('LICENSE', LICENSE_TEXT_OVERRIDES[key]))
    if not blocks:
        raise SystemExit(f'{key[0]} {key[1]} ships no licence file ({package["manifest_path"]}); see LICENSE_TEXT_OVERRIDES')
    expression = package.get('license') or 'see the text below'
    out = [f'## {key[0]} {key[1]}', '', f'License: {expression}', '']
    if package.get('repository'):
        out += [f'Source: {package["repository"]}', '']
    for name, text in blocks:
        out += [f'{name}:', '', fence(text), '']
    return '\n'.join(out)


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--output', default=str(ROOT / 'THIRD-PARTY-NOTICES.md'))
    args = parser.parse_args(argv)
    wanted = checker.tree_crates(tree())
    packages = {(p['name'], p['version']): p for p in metadata()['packages']}
    missing = sorted(wanted - set(packages))
    if missing:
        raise SystemExit(f'cargo metadata does not know: {missing}')
    body = HEADER + '\n'.join(render(packages[key]) for key in sorted(wanted))
    Path(args.output).write_text(body.rstrip('\n') + '\n', encoding='utf-8')
    print(f'wrote {args.output}: {len(wanted)} crates')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
