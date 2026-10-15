#!/usr/bin/env python3
"""Prints the resolved version of one pin of a Package.resolved file. Usage: resolved-version.py FILE IDENTITY"""
import json
import sys

if len(sys.argv) != 3:
    print(__doc__.strip().splitlines()[-1], file=sys.stderr)
    sys.exit(2)
path, identity = sys.argv[1], sys.argv[2]
try:
    with open(path) as handle:
        data = json.load(handle)
except (OSError, ValueError) as problem:
    print(f"cannot read {path}: {problem}", file=sys.stderr)
    sys.exit(1)
for pin in data.get("pins", []):
    if pin.get("identity") == identity:
        version = pin.get("state", {}).get("version")
        if version:
            print(version)
            sys.exit(0)
        print(f"pin {identity} in {path} has no version (a branch or revision pin)", file=sys.stderr)
        sys.exit(1)
print(f"no pin {identity} in {path}", file=sys.stderr)
sys.exit(1)
