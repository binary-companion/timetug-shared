"""Tests of write-swift-package.py."""
import importlib.util
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'write-swift-package.py'
ROOT_PACKAGE = Path(__file__).resolve().parents[3] / 'Package.swift'
SUM = 'ab' * 32
spec = importlib.util.spec_from_file_location('write_swift_package', SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
URL = 'https://github.com/binary-companion/timetug-shared/releases/download/2.1.0/TimeTugEngineFFI-2.1.0.xcframework.zip'


class WriteSwiftPackageTests(unittest.TestCase):
    def text(self):
        return ROOT_PACKAGE.read_text(encoding='utf-8')

    def test_the_path_target_becomes_the_url_target(self):
        out = module.rewrite(self.text(), '2.1.0', SUM)
        self.assertIn(f'.binaryTarget(name: "TimeTugEngineFFI", url: "{URL}", checksum: "{SUM}")', out)
        self.assertNotIn('build/TimeTugEngineFFI.xcframework', out.split('targets:')[1])
        self.assertEqual(out.count('.target('), self.text().count('.target('), 'nothing else changes')

    def test_a_prerelease_version_is_accepted(self):
        out = module.rewrite(self.text(), '2.0.0-alpha.3', SUM)
        self.assertIn('/download/2.0.0-alpha.3/TimeTugEngineFFI-2.0.0-alpha.3.xcframework.zip', out)

    def test_it_is_idempotent_and_updates_in_place(self):
        once = module.rewrite(self.text(), '2.1.0', SUM)
        self.assertEqual(module.rewrite(once, '2.1.0', SUM), once)
        again = module.rewrite(once, '2.1.1', 'cd' * 32)
        self.assertIn('/download/2.1.1/', again)
        self.assertNotIn('/download/2.1.0/', again)

    def test_bad_versions_and_checksums_are_refused(self):
        for version in ('2.1', 'v2.1.0', '2.1.0-beta.1', '2.1.0-alpha', '2.1.0 ', ''):
            with self.subTest(version=version), self.assertRaises(ValueError):
                module.rewrite(self.text(), version, SUM)
        for checksum in ('', 'AB' * 32, 'ab' * 31, 'zz' * 32, SUM + '0'):
            with self.subTest(checksum=checksum), self.assertRaises(ValueError):
                module.rewrite(self.text(), '2.1.0', checksum)

    def test_a_package_without_the_target_is_refused(self):
        with self.assertRaises(ValueError):
            module.rewrite('let package = Package(name: "x")', '2.1.0', SUM)

    def test_the_command_line_rewrites_a_file_and_exits_nonzero_on_bad_input(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'Package.swift'
            path.write_text(self.text(), encoding='utf-8')
            ok = subprocess.run(['python3', str(SCRIPT), '2.1.0', SUM, str(path)], capture_output=True, text=True)
            self.assertEqual(ok.returncode, 0, ok.stderr)
            self.assertIn(URL, path.read_text(encoding='utf-8'))
            bad = subprocess.run(['python3', str(SCRIPT), '2.1', SUM, str(path)], capture_output=True, text=True)
            self.assertEqual(bad.returncode, 1)
            self.assertIn(URL, path.read_text(encoding='utf-8'), 'a refused call leaves the file alone')


if __name__ == '__main__':
    unittest.main()
