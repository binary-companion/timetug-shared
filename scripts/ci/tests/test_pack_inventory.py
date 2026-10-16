#!/usr/bin/env python3
"""Checks a TimeTug.Engine .nupkg before it is published, and tests that check.

Usage: test_pack_inventory.py PACKAGE.nupkg [--version X.Y.Z]

The package must hold the managed wrapper, the engine and its Swift runtime for both Windows architectures,
the C header and the golden fixtures; with --version its nuspec must carry exactly that version. Exits 1,
listing what is wrong, otherwise 0. release.yml (job nuget-pack) runs it on the produced package before the
package is uploaded or pushed.

Run without arguments by unittest discovery (the architecture job), the TestCase classes below test the check
itself against packages built in a temporary directory."""
import re
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

REQUIRED = [
    'lib/net8.0/TimeTug.Engine.dll',
    'runtimes/win-x64/native/TimeTugEngineC.dll',
    'runtimes/win-x64/native/swiftCore.dll',
    'runtimes/win-arm64/native/TimeTugEngineC.dll',
    'runtimes/win-arm64/native/swiftCore.dll',
    'contentFiles/any/any/include/timetug_engine.h',
    'LICENSE',
]
FIXTURES = 'contentFiles/any/any/fixtures/'


def problems(package, version=None):
    """What is wrong with the package (empty when nothing is)."""
    try:
        with zipfile.ZipFile(package) as archive:
            names = set(archive.namelist())
            nuspecs = [n for n in names if n.endswith('.nuspec') and '/' not in n]
            nuspec = archive.read(nuspecs[0]).decode('utf-8') if len(nuspecs) == 1 else None
    except (OSError, zipfile.BadZipFile) as error:
        return [f'cannot read {package}: {error}']
    found = []
    for entry in REQUIRED:
        if entry not in names:
            found.append(f'missing {entry}')
    if not any(n.startswith(FIXTURES) and n.endswith('.json') for n in names):
        found.append(f'missing the golden fixtures ({FIXTURES}*.json)')
    if nuspec is None:
        found.append('no single .nuspec at the package root')
    elif version is not None:
        match = re.search(r'<version>([^<]+)</version>', nuspec)
        if match is None or match.group(1).strip() != version:
            found.append(f'nuspec version is {match.group(1).strip() if match else "missing"}, expected {version}')
    return found


def main(argv):
    if not argv or argv[0].startswith('-'):
        print(__doc__.split('\n\n')[0], file=sys.stderr)
        return 2
    package = argv[0]
    version = None
    if len(argv) == 3 and argv[1] == '--version':
        version = argv[2]
    elif len(argv) != 1:
        print('usage: test_pack_inventory.py PACKAGE.nupkg [--version X.Y.Z]', file=sys.stderr)
        return 2
    found = problems(package, version)
    for problem in found:
        print(f'error: {Path(package).name}: {problem}', file=sys.stderr)
    if not found:
        with zipfile.ZipFile(package) as archive:
            print(f'{Path(package).name}: {len(archive.namelist())} entries, all required ones present')
    return 1 if found else 0


def build(directory, entries, version='1.2.3'):
    path = Path(directory) / 'TimeTug.Engine.nupkg'
    with zipfile.ZipFile(path, 'w') as archive:
        archive.writestr('TimeTug.Engine.nuspec',
                         f'<package><metadata><id>TimeTug.Engine</id><version>{version}</version></metadata></package>')
        for entry in entries:
            archive.writestr(entry, b'x')
    return path


COMPLETE = REQUIRED + [FIXTURES + 'state.json']


class PackInventoryTests(unittest.TestCase):
    def test_a_complete_package_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(problems(build(directory, COMPLETE), '1.2.3'), [])

    def test_each_missing_entry_is_reported(self):
        for entry in REQUIRED:
            with self.subTest(entry=entry), tempfile.TemporaryDirectory() as directory:
                found = problems(build(directory, [e for e in COMPLETE if e != entry]))
                self.assertEqual(found, [f'missing {entry}'])

    def test_missing_fixtures_are_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            found = problems(build(directory, REQUIRED))
            self.assertEqual(len(found), 1)
            self.assertIn('fixtures', found[0])

    def test_a_wrong_version_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            found = problems(build(directory, COMPLETE, version='0.0.0-dev'), '1.2.3')
            self.assertEqual(found, ['nuspec version is 0.0.0-dev, expected 1.2.3'])

    def test_a_file_that_is_not_a_package_is_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'broken.nupkg'
            path.write_text('not a zip')
            self.assertIn('cannot read', problems(path)[0])

    def test_main_exit_codes(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(main([str(build(directory, COMPLETE)), '--version', '1.2.3']), 0)
            self.assertEqual(main([str(build(directory, REQUIRED))]), 1)
            self.assertEqual(main([]), 2)


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
