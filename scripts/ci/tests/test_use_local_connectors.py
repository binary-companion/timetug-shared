import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[2] / 'dev' / 'use-local-connectors.sh'
CRATES = ['calendar-core', 'calendar-oauth', 'calendar-ical', 'calendar-google', 'calendar-microsoft',
          'calendar-caldav', 'calendar-ical-subscription', 'calendar-test-support']


class UseLocalConnectorsTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        base = Path(self.directory.name)
        # A repository layout with the script in place, and a connectors checkout beside it.
        self.repo = base / 'repo'
        (self.repo / 'scripts' / 'dev').mkdir(parents=True)
        (self.repo / '.cargo').mkdir()
        shutil.copy(SCRIPT, self.repo / 'scripts' / 'dev' / 'use-local-connectors.sh')
        self.connectors = base / 'connectors'
        for name in CRATES:
            (self.connectors / 'crates' / name).mkdir(parents=True)
        self.config = self.repo / '.cargo' / 'config.local.toml'

    def tearDown(self):
        self.directory.cleanup()

    def run_script(self, *args):
        return subprocess.run(['bash', str(self.repo / 'scripts' / 'dev' / 'use-local-connectors.sh'), *args],
                              capture_output=True, text=True)

    def test_writes_a_patch_block_for_every_connector_crate(self):
        result = self.run_script(str(self.connectors))
        self.assertEqual(result.returncode, 0, result.stderr)
        text = self.config.read_text()
        self.assertIn('[patch."https://github.com/binary-companion/calendar-connectors"]', text)
        for name in CRATES:
            self.assertIn(f'{name} = {{ path = "{self.connectors.resolve()}/crates/{name}" }}', text)

    def test_restore_deletes_the_file(self):
        self.run_script(str(self.connectors))
        self.assertTrue(self.config.exists())
        result = self.run_script('--restore')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.config.exists())

    def test_restore_without_a_file_succeeds(self):
        self.assertEqual(self.run_script('--restore').returncode, 0)

    def test_a_path_that_is_not_a_connectors_checkout_fails_and_writes_nothing(self):
        result = self.run_script(str(self.connectors / 'missing'))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.config.exists())

    def test_no_argument_fails(self):
        self.assertNotEqual(self.run_script().returncode, 0)

    def test_relative_path_is_stored_absolute(self):
        result = subprocess.run(['bash', str(self.repo / 'scripts' / 'dev' / 'use-local-connectors.sh'), '../connectors'],
                                capture_output=True, text=True, cwd=self.repo)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f'path = "{self.connectors.resolve()}/crates/calendar-core"', self.config.read_text())


if __name__ == '__main__':
    unittest.main()
