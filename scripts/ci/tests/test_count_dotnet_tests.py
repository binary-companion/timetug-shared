import subprocess, sys, tempfile, unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'count-dotnet-tests.py'

# `dotnet test` summaries (the shape printed by the .NET 8 SDK's VSTest console logger).
DOTNET_PASSED = ("Passed!  - Failed:     0, Passed:    49, Skipped:     0, Total:    49, Duration: 1 s - "
                 "TimeTug.Engine.Tests.dll (net8.0)\n")
DOTNET_FAILED = ("Failed!  - Failed:    13, Passed:    36, Skipped:     0, Total:    49, Duration: 95 ms - "
                 "TimeTug.Engine.Tests.dll (net8.0)\n")

# The same with `--logger "console;verbosity=normal"` (what the CI jobs use).
DOTNET_NORMAL_PASSED = ("  Passed TimeTug.Engine.Tests.ScenarioTests.SubsetMatching [1 ms]\n\n"
                        "Test Run Successful.\nTotal tests: 49\n     Passed: 49\n Total time: 2.8157 Seconds\n")
DOTNET_NORMAL_FAILED = ("Test Run Failed.\nTotal tests: 49\n     Passed: 48\n     Failed: 1\n Total time: 3.1 Seconds\n")


def run_dotnet(text, floor_text='# C# floor\n45\n', raw=None):
    with tempfile.TemporaryDirectory() as directory:
        log = Path(directory) / 'dotnet.log'
        if raw is not None:
            log.write_bytes(raw)
        else:
            log.write_text(text, encoding='utf-8')
        floor = Path(directory) / 'floor.txt'
        floor.write_text(floor_text, encoding='utf-8')
        return subprocess.run([sys.executable, str(SCRIPT), str(log), '--floor-file', str(floor)],
                              capture_output=True, text=True)


class CountDotnetTestsTests(unittest.TestCase):
    def test_a_passing_run_above_the_floor_passes(self):
        result = run_dotnet('Starting test execution, please wait...\n' + DOTNET_PASSED)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Executed C# tests: 49', result.stdout)
        self.assertIn('floor 45', result.stdout)

    def test_the_normal_verbosity_summary_counts(self):
        result = run_dotnet(DOTNET_NORMAL_PASSED)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Executed C# tests: 49', result.stdout)

    def test_a_failed_normal_verbosity_run_fails(self):
        result = run_dotnet(DOTNET_NORMAL_FAILED, '1\n')
        self.assertEqual(result.returncode, 1)
        self.assertIn('1 failed', result.stderr)

    def test_both_summary_shapes_are_not_counted_twice(self):
        result = run_dotnet(DOTNET_PASSED + DOTNET_NORMAL_PASSED)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Executed C# tests: 49 passed in 1 runs', result.stdout)

    def test_at_the_floor_passes_and_one_below_fails(self):
        self.assertEqual(run_dotnet(DOTNET_PASSED, '49\n').returncode, 0)
        result = run_dotnet(DOTNET_PASSED, '50\n')
        self.assertEqual(result.returncode, 1)
        self.assertIn('49 < 50', result.stderr)

    def test_a_failed_run_fails(self):
        result = run_dotnet(DOTNET_FAILED, '1\n')
        self.assertEqual(result.returncode, 1)
        self.assertIn('13 failed', result.stderr)

    def test_no_summary_fails(self):
        result = run_dotnet('Build FAILED.\n')
        self.assertEqual(result.returncode, 1)
        self.assertIn('no dotnet test summary', result.stderr)

    def test_a_floor_file_without_a_number_fails(self):
        result = run_dotnet(DOTNET_PASSED, '# nothing here\n')
        self.assertEqual(result.returncode, 1)
        self.assertIn('floor', result.stderr)

    def test_crlf_and_utf8_bom_logs(self):
        raw = b'\xef\xbb\xbf' + DOTNET_PASSED.replace('\n', '\r\n').encode('utf-8')
        self.assertEqual(run_dotnet(None, raw=raw).returncode, 0)

    def test_missing_log_is_red(self):
        result = subprocess.run([sys.executable, str(SCRIPT), '/nonexistent/dotnet.log', '--floor-file', '/nonexistent/floor'],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn('cannot read', result.stderr)

    def test_the_repository_floor_file_holds_a_number(self):
        floor = Path(__file__).resolve().parents[1] / 'dotnet-test-floor.txt'
        numbers = [line.strip() for line in floor.read_text(encoding='utf-8').splitlines()
                   if line.strip() and not line.strip().startswith('#')]
        self.assertEqual(len(numbers), 1)
        self.assertGreater(int(numbers[0]), 0)


if __name__ == '__main__':
    unittest.main()
