import subprocess, sys, tempfile, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SCRIPT = ROOT / 'scripts' / 'release' / 'stamp-engine-version.py'
SESSION = ROOT / 'Sources' / 'TimeTugEngineC' / 'EngineSession.swift'

def stamp(version, text):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'EngineSession.swift'
        path.write_text(text, encoding='utf-8')
        result = subprocess.run([sys.executable, str(SCRIPT), version, str(path)], capture_output=True, text=True)
        return result, path.read_text(encoding='utf-8')

class StampEngineVersionTests(unittest.TestCase):
    def test_the_repository_holds_the_development_value_once(self):
        self.assertEqual(SESSION.read_text(encoding='utf-8').count('static let version = "0.0.0-dev"'), 1)

    def test_stamps_the_version(self):
        result, text = stamp('1.4.0', SESSION.read_text(encoding='utf-8'))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('static let version = "1.4.0"', text)
        self.assertNotIn('0.0.0-dev', text)

    def test_accepts_a_prerelease(self):
        result, text = stamp('0.0.0-dryrun.17', 'enum EngineBuild {\n    static let version = "0.0.0-dev"\n}\n')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('"0.0.0-dryrun.17"', text)

    def test_refuses_a_version_that_is_not_semver(self):
        original = 'static let version = "0.0.0-dev"\n'
        for bad in ('1.4', 'v1.4.0', '1.4.0"; evil()', ''):
            with self.subTest(version=bad):
                result, text = stamp(bad, original)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(text, original)

    def test_fails_when_the_development_value_is_missing(self):
        result, _ = stamp('1.4.0', 'static let version = "1.3.0"\n')
        self.assertEqual(result.returncode, 1)
        self.assertIn('0.0.0-dev', result.stderr)
