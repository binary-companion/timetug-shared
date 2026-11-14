import os
import stat
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'bump-cargo-dependency.py'
ROOT = Path(__file__).resolve().parents[3]
URL = 'https://github.com/binary-companion/calendar-connectors'
SHA = 'ea8a9e2fdb29749a3a041ef381eaa1c9aed7f027'
CRATES = ['calendar-core', 'calendar-oauth', 'calendar-ical']


def manifest(pin, extra=''):
    lines = '\n'.join(f'{name} = {{ git = "{URL}", {pin} }}' for name in CRATES)
    return textwrap.dedent('''\
        [workspace]
        members = ["crates/*"]

        [workspace.dependencies]
        timetug-core = { path = "crates/timetug-core" }
        # the connector library
        %s
        serde = { version = "=1.0.229", default-features = false }
        other = { git = "https://github.com/someone/else", rev = "abc" }
        %s
        [workspace.lints.rust]
        unsafe_code = "forbid"
        ''') % (lines, extra)


def lock(pin):
    blocks = ''.join(f'[[package]]\nname = "{name}"\nversion = "0.0.0"\nsource = "git+{URL}?{pin}#{SHA}"\n\n' for name in CRATES)
    return 'version = 4\n\n' + blocks


# A stand-in for cargo: records its arguments and rewrites the lock from the manifest, as `cargo update -p` would.
FAKE_CARGO = '''#!/usr/bin/env python3
import os, re, sys
log = os.environ["CARGO_LOG"]
open(log, "a").write(" ".join(sys.argv[1:]) + "\\n")
if os.environ.get("CARGO_FAIL"):
    print("error: failed to fetch the tag", file=sys.stderr)
    sys.exit(101)
manifest = sys.argv[sys.argv.index("--manifest-path") + 1]
text = open(manifest).read()
tag = re.search(r'tag = "([^"]+)"', text).group(1)
lock = os.path.join(os.path.dirname(manifest), "Cargo.lock")
data = open(lock).read()
data = re.sub(r"\\?(rev|tag)=[^#\\"]+#[0-9a-f]+", "?tag=" + os.environ.get("CARGO_LOCK_TAG", tag) + "#" + "1" * 40, data)
open(lock, "w").write(data)
'''


class Workspace:
    def __init__(self, directory, pin, lock_pin=None):
        self.root = Path(directory)
        self.manifest = self.root / 'Cargo.toml'
        self.lock = self.root / 'Cargo.lock'
        self.manifest.write_text(manifest(pin))
        self.lock.write_text(lock(lock_pin or pin.replace(' = ', '=').replace('"', '')))
        self.log = self.root / 'cargo.log'
        cargo = self.root / 'fake-cargo'
        cargo.write_text(FAKE_CARGO)
        cargo.chmod(cargo.stat().st_mode | stat.S_IEXEC)
        self.cargo = cargo
        self.original = (self.manifest.read_bytes(), self.lock.read_bytes())

    def run(self, *args, **env):
        environment = {**os.environ, 'CARGO': str(self.cargo), 'CARGO_LOG': str(self.log), **env}
        return subprocess.run([sys.executable, str(SCRIPT), *args, '--manifest', str(self.manifest)],
                              capture_output=True, text=True, env=environment)

    def untouched(self):
        return (self.manifest.read_bytes(), self.lock.read_bytes()) == self.original

    def cargo_calls(self):
        return self.log.read_text().splitlines() if self.log.exists() else []


class BumpCargoDependencyTests(unittest.TestCase):
    def workspace(self, pin=f'rev = "{SHA}"'):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        return Workspace(directory.name, pin)

    def test_a_revision_moves_to_a_tag_and_only_those_lines_change(self):
        w = self.workspace()
        result = w.run('2.0.0-alpha.1')
        self.assertEqual(result.returncode, 0, result.stderr)
        old, new = w.original[0].decode().splitlines(), w.manifest.read_text().splitlines()
        self.assertEqual(len(old), len(new))
        changed = [n for n, o in zip(new, old) if n != o]
        self.assertEqual(changed, [f'{name} = {{ git = "{URL}", tag = "2.0.0-alpha.1" }}' for name in CRATES])
        self.assertIn('other = { git = "https://github.com/someone/else", rev = "abc" }', new)
        self.assertIn('the connector library', w.manifest.read_text())

    def test_cargo_updates_exactly_the_connector_crates_and_the_lock_follows(self):
        w = self.workspace()
        self.assertEqual(w.run('2.0.0-alpha.1').returncode, 0)
        self.assertEqual(w.cargo_calls(), [f'update --manifest-path {w.manifest} ' + ' '.join(f'-p {n}' for n in CRATES)])
        self.assertIn('?tag=2.0.0-alpha.1#', w.lock.read_text())
        self.assertNotIn('rev=', w.lock.read_text())

    def test_a_tag_moves_to_a_higher_tag(self):
        w = self.workspace('tag = "2.0.0-alpha.1"')
        result = w.run('2.0.0-alpha.2')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('tag = "2.0.0-alpha.2"', w.manifest.read_text())
        self.assertNotIn('alpha.1', w.manifest.read_text())
        self.assertIn('(was 2.0.0-alpha.1)', result.stdout)

    def test_a_release_follows_its_prereleases(self):
        w = self.workspace('tag = "2.0.0-alpha.9"')
        self.assertEqual(w.run('2.0.0').returncode, 0)
        self.assertIn('tag = "2.0.0"', w.manifest.read_text())

    def test_a_downgrade_is_refused_and_nothing_is_written(self):
        for current, lower in (('2.0.0-alpha.2', '2.0.0-alpha.1'), ('2.0.0', '2.0.0-alpha.9'), ('2.1.0', '2.0.9'),
                               ('2.0.10', '2.0.9'), ('10.0.0', '9.9.9')):
            with self.subTest(current=current, lower=lower):
                w = self.workspace(f'tag = "{current}"')
                result = w.run(lower)
                self.assertEqual(result.returncode, 2)
                self.assertIn('refusing to move', result.stderr)
                self.assertTrue(w.untouched())
                self.assertEqual(w.cargo_calls(), [])

    def test_a_change_of_major_needs_a_manual_change(self):
        for current, other in (('2.0.0', '3.0.0'), ('2.4.1', '3.0.0-alpha.1'), ('3.0.0', '2.9.9')):
            with self.subTest(current=current, other=other):
                w = self.workspace(f'tag = "{current}"')
                result = w.run(other)
                self.assertEqual(result.returncode, 2)
                self.assertTrue(w.untouched())
                self.assertEqual(w.cargo_calls(), [])
        self.assertIn('manual change', self.workspace('tag = "2.0.0"').run('3.0.0').stderr)

    def test_the_same_version_is_a_no_op(self):
        w = self.workspace('tag = "2.0.0-alpha.1"')
        result = w.run('2.0.0-alpha.1')
        self.assertEqual(result.returncode, 0)
        self.assertIn('nothing to do', result.stdout)
        self.assertTrue(w.untouched())
        self.assertEqual(w.cargo_calls(), [])

    def test_malformed_versions_are_refused_before_anything_is_touched(self):
        w = self.workspace()
        for bad in ('1.0', 'v1.0.5', '2.0.0-beta.1', '2.0.0-alpha', '2.0.0-alpha.01', '2.0.0-alpha.1.2', '2.0.0+build',
                    '1.0.1234567890', '99999999999999999999.0.0', '1.0.5 --jq .', '$(touch /tmp/x)', '../x', '', '01.0.0',
                    '2.0.0\n1.0.0', '-1.0.0'):
            with self.subTest(version=bad):
                result = w.run(bad)
                self.assertEqual(result.returncode, 2, result.stdout)
                self.assertTrue(w.untouched())
        self.assertEqual(w.cargo_calls(), [])

    def test_a_cargo_failure_restores_both_files(self):
        w = self.workspace()
        result = w.run('2.0.0-alpha.1', CARGO_FAIL='1')
        self.assertEqual(result.returncode, 2)
        self.assertIn('were restored', result.stderr)
        self.assertTrue(w.untouched())

    def test_a_lock_that_does_not_follow_restores_both_files(self):
        w = self.workspace()
        result = w.run('2.0.0-alpha.1', CARGO_LOCK_TAG='2.0.0-alpha.0')
        self.assertEqual(result.returncode, 2)
        self.assertIn('not 2.0.0-alpha.1', result.stderr)
        self.assertTrue(w.untouched())

    def test_a_branch_pin_needs_a_manual_change(self):
        w = self.workspace('branch = "v2"')
        result = w.run('2.0.0')
        self.assertEqual(result.returncode, 2)
        self.assertIn('branch', result.stderr)
        self.assertTrue(w.untouched())

    def test_mixed_tags_need_a_manual_change(self):
        w = self.workspace('tag = "2.0.0-alpha.1"')
        text = w.manifest.read_text().replace('calendar-oauth = { git = "%s", tag = "2.0.0-alpha.1" }' % URL,
                                              'calendar-oauth = { git = "%s", tag = "2.0.0-alpha.2" }' % URL)
        w.manifest.write_text(text)
        w.original = (w.manifest.read_bytes(), w.lock.read_bytes())
        result = w.run('2.0.0')
        self.assertEqual(result.returncode, 2)
        self.assertIn('different tags', result.stderr)
        self.assertTrue(w.untouched())

    def test_a_manifest_without_connectors_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.toml').write_text('[workspace.dependencies]\nserde = { version = "=1.0.229" }\n')
            (root / 'Cargo.lock').write_text('version = 4\n')
            result = subprocess.run([sys.executable, str(SCRIPT), '2.0.0', '--manifest', str(root / 'Cargo.toml')],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('no calendar-connectors git dependency', result.stderr)

    def test_a_dependency_outside_workspace_dependencies_is_left_alone(self):
        w = self.workspace()
        w.manifest.write_text(w.manifest.read_text() + f'\n[dependencies]\nstray = {{ git = "{URL}", rev = "abc" }}\n')
        w.original = (w.manifest.read_bytes(), w.lock.read_bytes())
        self.assertEqual(w.run('2.0.0').returncode, 0)
        self.assertIn(f'stray = {{ git = "{URL}", rev = "abc" }}', w.manifest.read_text())

    def test_crlf_line_endings_are_kept(self):
        w = self.workspace()
        w.manifest.write_bytes(w.manifest.read_bytes().replace(b'\n', b'\r\n'))
        w.original = (w.manifest.read_bytes(), w.lock.read_bytes())
        self.assertEqual(w.run('2.0.0').returncode, 0)
        data = w.manifest.read_bytes()
        self.assertEqual(data.count(b'\r\n'), data.count(b'\n'))
        self.assertIn(b'tag = "2.0.0" }\r\n', data)

    def test_usage(self):
        result = subprocess.run([sys.executable, str(SCRIPT)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)

    def test_is_older_orders_prereleases_below_their_release(self):
        def older(a, b):
            return subprocess.run([sys.executable, str(SCRIPT), '--is-older', a, b], capture_output=True).returncode
        self.assertEqual(older('2.0.0-alpha.1', '2.0.0-alpha.2'), 0)
        self.assertEqual(older('2.0.0-alpha.9', '2.0.0-alpha.10'), 0)
        self.assertEqual(older('2.0.0-alpha.2', '2.0.0'), 0)
        self.assertEqual(older('2.0.0', '2.0.0-alpha.2'), 1)
        self.assertEqual(older('1.0.9', '1.0.10'), 0)
        self.assertEqual(older('1.0.10', '1.0.10'), 1)
        self.assertEqual(older('x', '1.0.0'), 2)

    def test_the_real_manifest_names_only_connector_crates_of_the_workspace(self):
        # The script finds every connector dependency of the real manifest (the 8 crates of phase 1) and edits none of
        # the others; a dry check that does not write: the same function the script uses.
        sys.path.insert(0, str(SCRIPT.parent))
        import importlib.util
        spec = importlib.util.spec_from_file_location('bump_cargo_dependency', SCRIPT)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        lines = (ROOT / 'Cargo.toml').read_text().splitlines(keepends=True)
        names = sorted(match.group('name') for _, match in module.connector_lines(lines))
        self.assertEqual(names, ['calendar-caldav', 'calendar-core', 'calendar-google', 'calendar-ical',
                                 'calendar-ical-subscription', 'calendar-microsoft', 'calendar-oauth',
                                 'calendar-test-support'])


if __name__ == '__main__':
    unittest.main()
