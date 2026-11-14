#!/usr/bin/env python3
"""Fails when the Cargo workspace breaks the crate rules of the Rust engine program (design section 6.1 and 6.2).
Usage: check-crate-architecture.py [--metadata FILE]

Reads `cargo metadata --format-version 1` (run here, or FILE) and exits 1, listing every problem:

  * a workspace-internal dependency edge that is not in ALLOWED (the "Depends on" column of the design table);
  * an external crate a workspace crate depends on that is not in EXTERNAL (conventions section 3);
  * a workspace crate that is not in ALLOWED.

The connector crates are dependencies by git revision; they are recognised by name (CONNECTORS).

calendar-test-support (from calendar-connectors) may be a dev-dependency of every crate, never a normal or build dependency.
Only the direct dependencies of workspace crates are checked; cargo-deny covers the rest of the tree."""
import argparse
import json
import subprocess
import sys

# The connector crates of calendar-connectors, which this workspace depends on by git revision. They are not workspace
# members, so cargo metadata lists them as ordinary dependencies; they are recognised by name.
CONNECTORS = [
    'calendar-core', 'calendar-oauth', 'calendar-ical', 'calendar-google', 'calendar-microsoft', 'calendar-caldav',
    'calendar-ical-subscription',
]
# crate -> the workspace and connector crates it may depend on (any kind, except where TEST_SUPPORT says otherwise).
ALLOWED = {
    'timetug-core': {'calendar-core'},
    'timetug-bridge': {'timetug-core', 'calendar-core'},
    'timetug-engine': {'timetug-core', 'timetug-bridge', 'calendar-core', 'calendar-caldav', 'calendar-google', 'calendar-ical-subscription', 'calendar-microsoft'},
    'timetug-engine-protocol': {'timetug-engine', 'timetug-core', 'timetug-bridge', 'calendar-core'},
    'timetug-engine-c': {'timetug-engine-protocol', 'timetug-engine', 'timetug-core', *CONNECTORS},
    'timetug-engine-ffi': {'timetug-engine-protocol', 'timetug-engine', 'timetug-core', *CONNECTORS},
    'dedup-benchmark': {'timetug-core', 'calendar-core'},
}
# Allowed as a dev-dependency of every crate (from calendar-connectors).
TEST_SUPPORT = 'calendar-test-support'
# External crates (conventions section 3).
EXTERNAL = {
    'serde', 'serde_json', 'jiff', 'quick-xml', 'sha2', 'getrandom', 'uuid', 'url', 'thiserror', 'async-trait',
    'tokio', 'base64',
}
# External crates allowed in particular workspace crates only: uniffi is for the bindings crate (phase 5 adds it).
EXTERNAL_ONLY_IN = {'uniffi': {'timetug-engine-ffi'}}


def check(metadata):
    problems = []
    members = set(metadata.get('workspace_members', []))
    workspace = [p for p in metadata.get('packages', []) if p.get('id') in members]
    internal = {p['name'] for p in workspace} | set(CONNECTORS) | {TEST_SUPPORT}
    for pkg in sorted(workspace, key=lambda p: p['name']):
        name = pkg['name']
        if name not in ALLOWED:
            problems.append(f'{name}: workspace crate is not in the ALLOWED table of check-crate-architecture.py')
            continue
        for dep in pkg.get('dependencies', []):
            dep_name, kind = dep['name'], dep.get('kind')
            if dep_name in internal:
                if dep_name == TEST_SUPPORT and kind == 'dev':
                    continue
                if dep_name not in ALLOWED[name]:
                    problems.append(f'{name} -> {dep_name}: dependency edge is not allowed')
            elif dep_name in EXTERNAL:
                continue
            elif name in EXTERNAL_ONLY_IN.get(dep_name, ()):
                continue
            else:
                problems.append(f'{name} -> {dep_name}: external crate is not on the allowed list')
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--metadata', help='a saved `cargo metadata --format-version 1` output')
    args = parser.parse_args()
    try:
        if args.metadata:
            with open(args.metadata, encoding='utf-8') as handle:
                metadata = json.load(handle)
        else:
            output = subprocess.run(['cargo', 'metadata', '--format-version', '1', '--locked'],
                                    capture_output=True, text=True, check=True).stdout
            metadata = json.loads(output)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'check-crate-architecture: cannot read the workspace metadata: {error}', file=sys.stderr)
        return 1
    problems = check(metadata)
    for problem in problems:
        print(f'check-crate-architecture: {problem}', file=sys.stderr)
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
