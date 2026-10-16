#!/usr/bin/env python3
"""Counts the tests that a `swift test` log says were executed and fails when too few ran or any failed.
Usage: count-swift-tests.py LOG --minimum N [--minimum-runs R] [--minimum-per-run P]
       count-swift-tests.py LOG --dotnet-floor-file FLOOR

The Windows jobs use it because a green job must mean "the tests ran": a skipped target or an
unexpected zero would otherwise pass. It counts

  * Swift Testing: every `Test run with N tests ... passed|failed` summary line (one per test target);
  * XCTest: the `Executed N tests, with M failures` line under `Test Suite 'All tests'`, once per
    bundle (the same number is repeated under the .xctest suite, which is not counted again).

--minimum-runs R also requires R Swift Testing summaries (one per test target), so a target that
vanishes is noticed even when the remaining ones keep the total above the minimum.

--minimum-per-run P (default 1) requires every Swift Testing summary to report at least P tests, so a
test target that still builds but holds no tests (`Test run with 0 tests ... passed`) is red even when
the total stays above the floor. P 0 turns the check off.

It exits 1 when the total is zero or below the minimum, when a summary reports a failure, or when
the log cannot be read.

With --dotnet-floor-file the log is a `dotnet test` log instead: it sums the passed tests of every
`Passed!|Failed!  - Failed: F, Passed: P, Skipped: S, Total: T` summary (one per test assembly) and
fails when there is no summary, when any test failed, or when fewer tests passed than the floor, the
one number (comments start with #) in FLOOR (scripts/ci/dotnet-test-floor.txt in CI)."""
import argparse
import re
import sys

SWIFT_TESTING = re.compile(r'Test run with (\d+) tests?\b[^\r\n]*?\b(passed|failed)\b')
DOTNET = re.compile(r'(Passed|Failed)!\s+-\s+Failed:\s+(\d+),\s+Passed:\s+(\d+),\s+Skipped:\s+(\d+),\s+Total:\s+(\d+)')
XCTEST = re.compile(r"Test Suite 'All tests' (passed|failed) at [^\r\n]*\s+"
                    r"Executed (\d+) tests?, with (\d+) failures?")

parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
parser.add_argument('log')
parser.add_argument('--minimum', type=int)
parser.add_argument('--dotnet-floor-file')
parser.add_argument('--minimum-runs', type=int, default=0)
parser.add_argument('--minimum-per-run', type=int, default=1)
args = parser.parse_args()
if args.dotnet_floor_file is None and args.minimum is None:
    parser.error('--minimum is required (or --dotnet-floor-file for a dotnet test log)')

try:
    with open(args.log, 'rb') as handle:
        text = handle.read().decode('utf-8-sig', errors='replace')
except OSError as problem:
    print(f'cannot read {args.log}: {problem}', file=sys.stderr)
    sys.exit(1)

def dotnet_floor(path):
    try:
        with open(path, encoding='utf-8') as handle:
            numbers = [line.strip() for line in handle
                       if line.strip() and not line.strip().startswith('#')]
    except OSError as problem:
        print(f'cannot read the floor file {path}: {problem}', file=sys.stderr)
        sys.exit(1)
    if len(numbers) != 1 or not numbers[0].isdigit():
        print(f'the floor file {path} must hold exactly one number', file=sys.stderr)
        sys.exit(1)
    return int(numbers[0])


if args.dotnet_floor_file is not None:
    floor = dotnet_floor(args.dotnet_floor_file)
    problems = []
    passed_total = 0
    summaries = DOTNET.findall(text)
    for _status, failed, passed, skipped, total in summaries:
        passed_total += int(passed)
        if int(failed) > 0:
            problems.append(f'a dotnet test run had {failed} failed of {total} tests')
    print(f'Executed C# tests: {passed_total} passed in {len(summaries)} runs; floor {floor}')
    if not summaries:
        problems.append('no dotnet test summary found (the tests did not run)')
    elif passed_total < floor:
        problems.append(f'fewer C# tests passed than the floor: {passed_total} < {floor}')
    for problem in problems:
        print(f'error: {problem}', file=sys.stderr)
    sys.exit(1 if problems else 0)

swift_testing = xctest = runs = 0
problems = []
summaries = list(SWIFT_TESTING.finditer(text))
for number, summary in enumerate(summaries, start=1):
    count, status = summary.groups()
    swift_testing += int(count)
    runs += 1
    if status == 'failed':
        problems.append(f'a Swift Testing run of {count} tests failed')
    if int(count) < args.minimum_per_run:
        problems.append(f'Swift Testing run {number} of {len(summaries)} ran too few tests '
                        f'({int(count)} < {args.minimum_per_run}): "{summary.group(0)}" '
                        '(a test target holds no tests)')
for status, count, failures in XCTEST.findall(text):
    xctest += int(count)
    if status == 'failed' or int(failures) > 0:
        problems.append(f'an XCTest run of {count} tests reported {failures} failures')

total = swift_testing + xctest
print(f'Executed tests: {total} (Swift Testing {swift_testing} in {runs} runs, XCTest {xctest}); '
      f'floor {args.minimum}')
if total == 0:
    problems.append('no test was executed (no summary line found, or all of them say 0)')
elif total < args.minimum:
    problems.append(f'fewer tests ran than the floor: {total} < {args.minimum}')
if runs < args.minimum_runs:
    problems.append(f'only {runs} Swift Testing run(s) reported, expected at least {args.minimum_runs} '
                    '(a test target did not run)')
for problem in problems:
    print(f'error: {problem}', file=sys.stderr)
sys.exit(1 if problems else 0)
