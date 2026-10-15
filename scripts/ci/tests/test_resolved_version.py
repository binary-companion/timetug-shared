import json, subprocess, sys, tempfile, unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'resolved-version.py'

def pin(identity, version, revision='0' * 40):
    return {'identity': identity, 'kind': 'remoteSourceControl',
            'location': f'https://github.com/example/{identity}',
            'state': {'revision': revision, 'version': version}}

# v2 files wrap the pins the same way v3 does; v1 nested them under "object" and is not produced any more.
def resolved(version, *pins):
    return {'originHash': 'f' * 64, 'pins': list(pins), 'version': version}

def run(data, identity, raw=None):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'Package.resolved'
        path.write_text(raw if raw is not None else json.dumps(data))
        return subprocess.run([sys.executable, str(SCRIPT), str(path), identity],
                              capture_output=True, text=True)

class ResolvedVersionTests(unittest.TestCase):
    def test_prints_the_version_of_the_pin(self):
        result = run(resolved(3, pin('calendar-connectors', '1.0.4')), 'calendar-connectors')
        self.assertEqual((result.returncode, result.stdout), (0, '1.0.4\n'), result.stderr)

    def test_picks_the_requested_identity_among_several(self):
        data = resolved(3, pin('swift-argument-parser', '1.5.0'), pin('calendar-connectors', '1.2.3'))
        self.assertEqual(run(data, 'calendar-connectors').stdout, '1.2.3\n')
        self.assertEqual(run(data, 'swift-argument-parser').stdout, '1.5.0\n')

    def test_missing_pin_exits_1_and_names_it(self):
        result = run(resolved(3, pin('other', '1.0.0')), 'calendar-connectors')
        self.assertEqual((result.returncode, result.stdout), (1, ''))
        self.assertIn('no pin calendar-connectors', result.stderr)

    def test_file_without_pins_exits_1(self):
        self.assertEqual(run({'version': 3}, 'calendar-connectors').returncode, 1)

    def test_v2_file_shape(self):
        result = run(resolved(2, pin('calendar-connectors', '1.0.0')), 'calendar-connectors')
        self.assertEqual((result.returncode, result.stdout), (0, '1.0.0\n'))

    def test_identity_match_is_exact(self):
        result = run(resolved(3, pin('calendar-connectors-extra', '9.9.9')), 'calendar-connectors')
        self.assertEqual(result.returncode, 1)

    def test_pin_without_a_version_is_not_reported_as_one(self):
        # A branch or revision pin has no "version": the release note must not claim one.
        broken = {'identity': 'calendar-connectors', 'kind': 'remoteSourceControl',
                  'location': 'x', 'state': {'branch': 'main', 'revision': '0' * 40}}
        result = run(resolved(3, broken), 'calendar-connectors')
        self.assertEqual((result.returncode, result.stdout), (1, ''))

    def test_unreadable_file_exits_non_zero_with_no_output(self):
        result = run(None, 'calendar-connectors', raw='not json')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '')

if __name__ == '__main__':
    unittest.main()
