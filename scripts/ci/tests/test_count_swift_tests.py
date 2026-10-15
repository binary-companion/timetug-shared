import subprocess, sys, tempfile, unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'count-swift-tests.py'

# The shapes below are copied from the `swift test` output of the windows-latest and windows-11-arm
# runners (Swift 6.4.0, workflow run 38698368287 of binary-companion/timetug-shared): blank lines
# between the lines, a tab before the XCTest "Executed" line, and the Swift Testing summary with a
# status symbol in front.
XCTEST_EMPTY = (
    "Test Suite 'All tests' started at 2026-10-15 07:26:49.905\n\n"
    "Test Suite 'Debug-windows-x86_64.xctest' started at 2026-10-15 07:26:49.909\n\n"
    "Test Suite 'Debug-windows-x86_64.xctest' passed at 2026-10-15 07:26:49.909\n"
    "\t Executed 0 tests, with 0 failures (0 unexpected) in 0.0 (0.0) seconds\n\n"
    "Test Suite 'All tests' passed at 2026-10-15 07:26:49.909\n"
    "\t Executed 0 tests, with 0 failures (0 unexpected) in 0.0 (0.0) seconds\n\n"
)
CORE_PASSED = "◊ Test run started.\n\n√ Test run with 301 tests in 0 suites passed after 0.927 seconds.\n\n"
BRIDGE_PASSED = "√ Test run with 13 tests in 0 suites passed after 0.059 seconds.\n"
CORE_FAILED = ("× Test disablingInferenceDiscardsAnInFlightVerdict() failed after 0.434 seconds with 3 issues.\n\n"
               "× Test run with 301 tests in 0 suites failed after 0.905 seconds with 3 issues.\n\n")
REAL_PASSING = XCTEST_EMPTY + CORE_PASSED + XCTEST_EMPTY + BRIDGE_PASSED
REAL_FAILING = XCTEST_EMPTY + CORE_FAILED + XCTEST_EMPTY + BRIDGE_PASSED

def run(text, minimum='280', raw=None, *extra):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'test.log'
        if raw is not None:
            path.write_bytes(raw)
        elif text is not None:
            path.write_text(text, encoding='utf-8')
        return subprocess.run([sys.executable, str(SCRIPT), str(path), '--minimum', minimum, *extra],
                              capture_output=True, text=True)

class CountSwiftTestsTests(unittest.TestCase):
    def test_real_windows_log_counts_both_swift_testing_runs(self):
        result = run(REAL_PASSING)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Executed tests: 314', result.stdout)

    def test_at_the_floor_passes_and_one_below_fails(self):
        self.assertEqual(run(REAL_PASSING, '314').returncode, 0)
        below = run(REAL_PASSING, '315')
        self.assertEqual(below.returncode, 1)
        self.assertIn('314', below.stderr)

    def test_a_skipped_target_turns_the_count_red(self):
        # Only the Bridge target ran: the job must not pass on 13 tests.
        result = run(XCTEST_EMPTY + BRIDGE_PASSED)
        self.assertEqual(result.returncode, 1)
        self.assertIn('fewer', result.stderr)

    def test_minimum_runs_catches_a_skipped_target_that_the_floor_misses(self):
        # Core alone has 301 tests, above a floor of 280: only the run count notices Bridge is gone.
        core_only = XCTEST_EMPTY + CORE_PASSED
        self.assertEqual(run(core_only).returncode, 0)
        result = run(core_only, '280', None, '--minimum-runs', '2')
        self.assertEqual(result.returncode, 1)
        self.assertIn('only 1 Swift Testing run', result.stderr)
        self.assertEqual(run(REAL_PASSING, '280', None, '--minimum-runs', '2').returncode, 0)
        self.assertEqual(run(REAL_PASSING, '280', None, '--minimum-runs', '3').returncode, 1)

    def test_no_summary_at_all_is_red(self):
        for text in ('', 'Build complete! (22.73 secs)\n', XCTEST_EMPTY):
            with self.subTest(text=text[:20]):
                self.assertEqual(run(text, '1').returncode, 1)
                self.assertEqual(run(text, '0').returncode, 1)

    def test_a_failed_swift_testing_run_is_red_even_above_the_floor(self):
        result = run(REAL_FAILING)
        self.assertEqual(result.returncode, 1)
        self.assertIn('failed', result.stderr)

    def test_singular_summary_line(self):
        text = "× Test run with 1 test in 0 suites failed after 0.001 seconds with 1 issue.\n"
        self.assertEqual(run(text, '0').returncode, 1)
        passing = "√ Test run with 1 test in 0 suites passed after 0.001 seconds.\n"
        result = run(passing, '1')
        self.assertEqual((result.returncode, 'Executed tests: 1' in result.stdout), (0, True))

    def test_xctest_is_counted_once_from_the_all_tests_summary(self):
        # A mixed run: the .xctest bundle line and the "All tests" line both carry the number.
        xctest = (
            "Test Suite 'All tests' started at 2026-10-15 07:26:49.905\n"
            "Test Suite 'Debug-windows-x86_64.xctest' passed at 2026-10-15 07:26:49.909\n"
            "\t Executed 5 tests, with 0 failures (0 unexpected) in 0.1 (0.1) seconds\n"
            "Test Suite 'All tests' passed at 2026-10-15 07:26:49.909\n"
            "\t Executed 5 tests, with 0 failures (0 unexpected) in 0.1 (0.1) seconds\n")
        text = xctest + "√ Test run with 100 tests in 0 suites passed after 1 seconds.\n"
        self.assertEqual(run(text, '105').returncode, 0)
        self.assertEqual(run(text, '106').returncode, 1)
        self.assertIn('XCTest 5', run(text, '105').stdout)

    def test_a_failing_xctest_summary_is_red(self):
        text = ("Test Suite 'All tests' failed at 2026-10-15 07:26:49.909\n"
                "\t Executed 3 tests, with 1 failures (1 unexpected) in 0.1 (0.1) seconds\n")
        self.assertEqual(run(text, '0').returncode, 1)

    def test_crlf_and_utf8_bom_logs(self):
        raw = b'\xef\xbb\xbf' + REAL_PASSING.replace('\n', '\r\n').encode('utf-8')
        result = run(None, raw=raw)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('Executed tests: 314', result.stdout)

    def test_missing_log_is_red(self):
        result = subprocess.run([sys.executable, str(SCRIPT), '/nonexistent/test.log', '--minimum', '1'],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)

    def test_minimum_must_be_given(self):
        result = subprocess.run([sys.executable, str(SCRIPT), 'x.log'], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)

if __name__ == '__main__':
    unittest.main()
