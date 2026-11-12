#!/usr/bin/env python3
"""Counts the C# tests that a `dotnet test` log says passed and fails when too few ran or any failed.
Usage: count-dotnet-tests.py LOG --floor-file FLOOR

A green job must mean "the tests ran": a vanished test class or an unexpected zero would otherwise pass. The log is
a `dotnet test` log: the script sums the passed tests of every
`Passed!|Failed!  - Failed: F, Passed: P, Skipped: S, Total: T` summary (one per test assembly; with the console
logger at normal verbosity, the `Test Run Successful|Failed. Total tests: T Passed: P ...` block) and exits 1 when
there is no summary, when any test failed, when fewer tests passed than the floor (the one number in FLOOR,
scripts/ci/dotnet-test-floor.txt in CI; comments start with #), or when the log or the floor file cannot be read."""
import argparse
import re
import sys

DOTNET_NORMAL = re.compile(r'Test Run (Successful|Failed)\.\s+Total tests: (\d+)((?:\s+(?:Passed|Failed|Skipped): \d+)*)')
DOTNET = re.compile(r'(Passed|Failed)!\s+-\s+Failed:\s+(\d+),\s+Passed:\s+(\d+),\s+Skipped:\s+(\d+),\s+Total:\s+(\d+)')


def read_floor(path):
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


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('log')
    parser.add_argument('--floor-file', required=True)
    args = parser.parse_args()
    try:
        with open(args.log, 'rb') as handle:
            text = handle.read().decode('utf-8-sig', errors='replace')
    except OSError as problem:
        print(f'cannot read {args.log}: {problem}', file=sys.stderr)
        return 1
    floor = read_floor(args.floor_file)
    problems = []
    passed_total = 0
    summaries = DOTNET.findall(text)
    # `--logger "console;verbosity=normal"` prints a multi-line summary instead; read it into the same shape. Only
    # when there is no one-line summary, so a log that had both would not count its tests twice.
    for status, total, counts in ([] if summaries else DOTNET_NORMAL.findall(text)):
        found = dict(re.findall(r'(Passed|Failed|Skipped): (\d+)', counts))
        summaries.append((status, found.get('Failed', '0'), found.get('Passed', '0'), found.get('Skipped', '0'), total))
    for _status, failed, passed, _skipped, total in summaries:
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
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
