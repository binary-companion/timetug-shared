import json, subprocess, sys, tempfile, unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'bump-resolved.py'
REPO = Path(__file__).resolve().parents[3]

OLD_REVISION = 'a' * 40
NEW_REVISION = 'b' * 40
OTHER_REVISION = 'c' * 40


def pin(identity, version, revision):
    # Written the way SwiftPM writes a version 3 file: two-space indent, a space on both sides of the colon.
    return (
        '    {\n'
        f'      "identity" : "{identity}",\n'
        '      "kind" : "remoteSourceControl",\n'
        f'      "location" : "https://github.com/example/{identity}",\n'
        '      "state" : {\n'
        f'        "revision" : "{revision}",\n'
        f'        "version" : "{version}"\n'
        '      }\n'
        '    }'
    )


def resolved(*pins, origin_hash='7b913900edd27dc4959e494a82a89ca0808a78a64f7a5ad3d7d648947c16e04b'):
    return ('{\n'
            f'  "originHash" : "{origin_hash}",\n'
            '  "pins" : [\n' + ',\n'.join(pins) + '\n  ],\n'
            '  "version" : 3\n'
            '}\n')


def manifest(floor='1.0.0', slug='calendar-connectors'):
    return ('// swift-tools-version: 6.0\nimport PackageDescription\n\n'
            'let package = Package(\n    name: "x",\n'
            f'    dependencies: [.package(url: "https://github.com/binary-companion/{slug}", from: "{floor}")]\n)\n')


FIXTURE = resolved(
    pin('another-package', '2.3.4', OTHER_REVISION),
    pin('calendar-connectors', '1.0.1', OLD_REVISION),
    pin('zebra', '0.9.0', OTHER_REVISION),
)


def run(*args):
    return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True)


class Workspace:
    """A temp directory with Package.resolved and Package.swift; run() bumps it and reports the outcome."""

    def __init__(self, test, resolved_text=FIXTURE, manifest_text=manifest()):
        self.directory = tempfile.TemporaryDirectory()
        test.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / 'Package.resolved'
        self.path.write_bytes(resolved_text.encode() if isinstance(resolved_text, str) else resolved_text)
        if manifest_text is not None:
            (Path(self.directory.name) / 'Package.swift').write_text(manifest_text)
        self.original = self.path.read_bytes()

    def bump(self, slug='calendar-connectors', version='1.0.4', revision=NEW_REVISION, *extra):
        return run(slug, version, revision, str(self.path), *extra)

    def unchanged(self):
        return self.path.read_bytes() == self.original


class BumpResolvedTests(unittest.TestCase):
    def refused(self, workspace, result, needle=None):
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertTrue(workspace.unchanged(), 'the file was modified although the bump was refused')
        if needle:
            self.assertIn(needle, result.stderr)

    # --- the happy path: exactly the revision and the version of one pin change ---

    def test_moves_revision_and_version_of_the_pin_and_nothing_else(self):
        ws = Workspace(self)
        result = ws.bump()
        self.assertEqual(result.returncode, 0, result.stderr)
        expected = FIXTURE.replace(OLD_REVISION, NEW_REVISION).replace('"version" : "1.0.1"', '"version" : "1.0.4"')
        self.assertEqual(ws.path.read_text(), expected)
        self.assertIn('calendar-connectors resolved at 1.0.4 (was 1.0.1)', result.stdout)

    def test_origin_hash_and_other_pins_are_unchanged(self):
        ws = Workspace(self)
        self.assertEqual(ws.bump().returncode, 0)
        before, after = json.loads(FIXTURE), json.loads(ws.path.read_text())
        self.assertEqual(after['originHash'], before['originHash'])
        self.assertEqual(after['version'], 3)
        by_identity = {p['identity']: p for p in after['pins']}
        original = {p['identity']: p for p in before['pins']}
        self.assertEqual(by_identity['another-package'], original['another-package'])
        self.assertEqual(by_identity['zebra'], original['zebra'])
        self.assertEqual(by_identity['calendar-connectors']['state'],
                         {'revision': NEW_REVISION, 'version': '1.0.4'})
        self.assertEqual([p['identity'] for p in after['pins']], [p['identity'] for p in before['pins']])

    def test_the_first_and_the_last_pin_can_be_moved(self):
        for slug, version in (('another-package', '2.4.0'), ('zebra', '0.9.1')):
            with self.subTest(slug=slug):
                ws = Workspace(self, manifest_text=manifest('2.0.0' if slug == 'another-package' else '0.1.0', slug))
                result = ws.bump(slug, version)
                self.assertEqual(result.returncode, 0, result.stderr)
                pins = {p['identity']: p['state'] for p in json.loads(ws.path.read_text())['pins']}
                self.assertEqual(pins[slug], {'revision': NEW_REVISION, 'version': version})
                self.assertEqual(pins['calendar-connectors'], {'revision': OLD_REVISION, 'version': '1.0.1'})

    def test_a_longer_identity_with_the_same_prefix_is_not_touched(self):
        text = resolved(pin('calendar-connectors-extra', '1.0.1', OTHER_REVISION),
                        pin('calendar-connectors', '1.0.1', OLD_REVISION))
        ws = Workspace(self, text)
        self.assertEqual(ws.bump().returncode, 0)
        pins = {p['identity']: p['state'] for p in json.loads(ws.path.read_text())['pins']}
        self.assertEqual(pins['calendar-connectors-extra'], {'revision': OTHER_REVISION, 'version': '1.0.1'})
        self.assertEqual(pins['calendar-connectors'], {'revision': NEW_REVISION, 'version': '1.0.4'})

    def test_identity_is_matched_exactly_not_as_a_prefix_or_a_pattern(self):
        for slug in ('calendar', 'calendar.connectors', 'calendar-connector', 'calendar-connectors-extra', '.*'):
            with self.subTest(slug=slug):
                ws = Workspace(self)
                self.refused(ws, ws.bump(slug))

    def test_a_numeric_comparison_not_a_string_one(self):
        ws = Workspace(self, resolved(pin('calendar-connectors', '1.0.9', OLD_REVISION)))
        self.assertEqual(ws.bump(version='1.0.10').returncode, 0)
        self.assertEqual(json.loads(ws.path.read_text())['pins'][0]['state']['version'], '1.0.10')
        ws = Workspace(self, resolved(pin('calendar-connectors', '1.0.10', OLD_REVISION)))
        self.refused(ws, ws.bump(version='1.0.9'), 'refusing to move calendar-connectors back from 1.0.10 to 1.0.9')
        ws = Workspace(self, resolved(pin('calendar-connectors', '1.9.0', OLD_REVISION)))
        self.assertEqual(ws.bump(version='1.10.0').returncode, 0)

    def test_crlf_line_endings_survive(self):
        ws = Workspace(self, FIXTURE.replace('\n', '\r\n'))
        self.assertEqual(ws.bump().returncode, 0)
        data = ws.path.read_bytes()
        self.assertEqual(data.count(b'\r\n'), data.count(b'\n'))
        self.assertEqual(json.loads(data)['pins'][1]['state']['version'], '1.0.4')

    def test_default_file_is_package_resolved_in_the_working_directory(self):
        ws = Workspace(self)
        result = subprocess.run([sys.executable, str(SCRIPT), 'calendar-connectors', '1.0.4', NEW_REVISION],
                                cwd=ws.directory.name, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(NEW_REVISION, ws.path.read_text())

    # --- idempotence and downgrades ---

    def test_same_version_and_revision_is_a_no_op(self):
        ws = Workspace(self)
        self.assertEqual(ws.bump(version='1.0.1', revision=OLD_REVISION).returncode, 0)
        self.assertTrue(ws.unchanged())

    def test_running_the_same_bump_twice_changes_nothing_the_second_time(self):
        ws = Workspace(self)
        self.assertEqual(ws.bump().returncode, 0)
        once = ws.path.read_bytes()
        again = ws.bump()
        self.assertEqual(again.returncode, 0, again.stderr)
        self.assertEqual(ws.path.read_bytes(), once)

    def test_same_version_at_another_revision_is_refused(self):
        # A moved tag must not silently rewrite a pin; that needs a reviewed manual change.
        ws = Workspace(self)
        self.refused(ws, ws.bump(version='1.0.1', revision=NEW_REVISION), 'revision')

    def test_lower_version_is_refused(self):
        for version in ('1.0.0', '0.9.9'):
            with self.subTest(version=version):
                ws = Workspace(self)
                self.refused(ws, ws.bump(version=version), f'refusing to move calendar-connectors back from 1.0.1 to {version}')

    # --- the major rule ---

    def test_a_change_of_major_needs_a_manual_change(self):
        ws = Workspace(self)
        self.refused(ws, ws.bump(version='2.0.0'), 'needs a manual change')

    def test_major_is_judged_against_the_from_floor(self):
        ws = Workspace(self, resolved(pin('calendar-connectors', '2.0.1', OLD_REVISION)), manifest('2.0.0'))
        self.assertEqual(ws.bump(version='2.1.0').returncode, 0)
        ws = Workspace(self, resolved(pin('calendar-connectors', '2.0.1', OLD_REVISION)), manifest('2.0.0'))
        self.refused(ws, ws.bump(version='3.0.0'), 'needs a manual change')

    def test_a_version_below_the_floor_is_refused(self):
        ws = Workspace(self, manifest_text=manifest('1.2.0'))
        self.refused(ws, ws.bump(version='1.1.0'), 'floor')

    def test_missing_manifest_or_missing_floor_fails_closed(self):
        ws = Workspace(self, manifest_text=None)
        self.refused(ws, ws.bump(), 'Package.swift')
        ws = Workspace(self, manifest_text=manifest(slug='something-else'))
        self.refused(ws, ws.bump(), 'from:')
        ws = Workspace(self, manifest_text='let package = Package(name: "x")\n')
        self.refused(ws, ws.bump(), 'from:')

    def test_an_unreadable_manifest_is_a_refusal_not_a_traceback(self):
        ws = Workspace(self)
        (Path(ws.directory.name) / 'Package.swift').write_bytes(b'// caf\xe9\n' + manifest().encode())
        result = ws.bump()
        self.refused(ws, result, 'cannot read')
        self.assertNotIn('Traceback', result.stderr)

    # --- input validation: nothing is written on a refusal ---

    def test_malformed_versions_are_refused(self):
        for version in ('1.0', '1', '1.0.4.1', 'v1.0.4', '01.0.4', '1.0.04', '1.0.4-rc1', '1.0.4+build', '1.0.4\n',
                        ' 1.0.4', '1.0.4 ', '', 'latest', '1.0.x', '-1.0.4', '1.0.\u0664', '1.0.1\u0664', '1.\u0660.4', '../1.0.4'):
            with self.subTest(version=repr(version)):
                ws = Workspace(self)
                self.refused(ws, ws.bump(version=version), 'not X.Y.Z')

    def test_components_are_capped_at_nine_digits(self):
        ws = Workspace(self)
        self.assertEqual(ws.bump(version='1.999999999.999999999').returncode, 0)
        for version in ('1.0.1000000000', '1.1234567890.0', '1.0.99999999999999999999',
                        '999999999999999999999.0.0', '18446744073709551616.0.0'):
            with self.subTest(version=version):
                ws = Workspace(self)
                self.refused(ws, ws.bump(version=version), 'not X.Y.Z')

    def test_a_pin_with_an_oversized_version_is_refused(self):
        ws = Workspace(self, resolved(pin('calendar-connectors', '1.0.99999999999999999999', OLD_REVISION)))
        self.refused(ws, ws.bump(version='1.0.5'), 'X.Y.Z')

    def test_malformed_revisions_are_refused(self):
        for revision in ('B' * 40, 'a' * 39, 'a' * 41, 'g' * 40, '', NEW_REVISION + '\n', ' ' + NEW_REVISION,
                         NEW_REVISION[:-1] + '\u0661', 'HEAD', '1.0.4'):
            with self.subTest(revision=repr(revision)):
                ws = Workspace(self)
                self.refused(ws, ws.bump(revision=revision), '40-character lowercase hex')

    def test_unknown_slug_is_refused(self):
        ws = Workspace(self)
        self.refused(ws, ws.bump('no-such-package'), 'no pin no-such-package')

    def test_malformed_slugs_are_refused(self):
        for slug in ('', 'Calendar-Connectors', 'a b', 'a"b', '../x'):
            with self.subTest(slug=repr(slug)):
                ws = Workspace(self)
                self.refused(ws, ws.bump(slug))

    def test_missing_file_and_a_directory_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            result = run('calendar-connectors', '1.0.4', NEW_REVISION, str(Path(directory) / 'Package.resolved'))
            self.assertEqual(result.returncode, 2)
            self.assertIn('not found', result.stderr)
            result = run('calendar-connectors', '1.0.4', NEW_REVISION, directory)
            self.assertEqual(result.returncode, 2)

    def test_usage_errors_exit_2(self):
        self.assertEqual(run().returncode, 2)
        self.assertEqual(run('calendar-connectors', '1.0.4').returncode, 2)
        self.assertEqual(run('calendar-connectors', '1.0.4', NEW_REVISION, 'a', 'b').returncode, 2)

    def test_a_file_that_is_not_json_is_refused(self):
        for text in (FIXTURE[:-30], FIXTURE + 'trailing', '', 'not json'):
            with self.subTest(text=text[-20:]):
                ws = Workspace(self, text)
                self.refused(ws, ws.bump(), 'JSON')

    def test_a_file_that_is_not_utf8_is_refused(self):
        ws = Workspace(self, b'\xff\xfe{}')
        self.refused(ws, ws.bump())

    def test_a_pin_without_a_released_version_is_refused(self):
        text = FIXTURE.replace('"version" : "1.0.1"', '"branch" : "main"')
        ws = Workspace(self, text)
        self.refused(ws, ws.bump(), 'X.Y.Z')

    def test_two_pins_with_the_same_identity_are_refused(self):
        ws = Workspace(self, resolved(pin('calendar-connectors', '1.0.1', OLD_REVISION),
                                      pin('calendar-connectors', '1.0.2', OLD_REVISION)))
        self.refused(ws, ws.bump())

    # --- the repository's real, committed files ---

    def test_the_real_package_resolved_stays_valid_and_only_revision_and_version_change(self):
        real = (REPO / 'Package.resolved').read_text()
        before = json.loads(real)
        current = next(p for p in before['pins'] if p['identity'] == 'calendar-connectors')['state']['version']
        major, minor, patch = (int(part) for part in current.split('.'))
        target = f'{major}.{minor}.{patch + 1}'
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            (work / 'Package.resolved').write_text(real)
            (work / 'Package.swift').write_text((REPO / 'Package.swift').read_text())
            result = run('calendar-connectors', target, NEW_REVISION, str(work / 'Package.resolved'))
            self.assertEqual(result.returncode, 0, result.stderr)
            changed = (work / 'Package.resolved').read_text()
        after = json.loads(changed)  # still valid JSON
        self.assertEqual(set(after), set(before))
        self.assertEqual(after['originHash'], before['originHash'])
        self.assertEqual(after['version'], before['version'])
        self.assertEqual(len(after['pins']), len(before['pins']))
        for old, new in zip(before['pins'], after['pins']):
            self.assertEqual(set(new), set(old))
            self.assertEqual(set(new['state']), set(old['state']))
            if old['identity'] == 'calendar-connectors':
                self.assertEqual(new['state'], {**old['state'], 'revision': NEW_REVISION, 'version': target})
                self.assertEqual({k: v for k, v in new.items() if k != 'state'},
                                 {k: v for k, v in old.items() if k != 'state'})
            else:
                self.assertEqual(new, old)
        # Text level: the file differs from the original exactly in those two values.
        expected = real.replace(f'"version" : "{current}"', f'"version" : "{target}"').replace(
            next(p for p in before['pins'] if p['identity'] == 'calendar-connectors')['state']['revision'],
            NEW_REVISION)
        self.assertEqual(changed, expected)
        self.assertNotEqual(changed, real)


if __name__ == '__main__':
    unittest.main()
