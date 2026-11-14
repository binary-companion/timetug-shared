#!/usr/bin/env python3
"""Lists the public types of the 1.x Swift libraries that the Mac app uses (writes docs/bindings-surface.txt).
Usage: list-app-types.py --libraries DIR [DIR ...] --app DIR [--output FILE]

--libraries: the `Sources` directories of checkouts of `calendar-connectors` and `timetug-shared` on their `1.x` branches.
--app: the root of the `timetug` repository (its `Apps` and `Packages` directories are scanned, test sources excluded).
A name counts when a public `struct`, `enum`, `class`, `actor`, `protocol` or `typealias` of that name is declared in a
library (CalendarTestSupport and DedupBenchmark excluded) and the word appears in a non-test source of the app that does
not declare a type of the same name itself."""
import argparse
import os
import re
import sys

DECLARATION = re.compile(
    r'^\s*(?:@\w+(?:\([^)]*\))?\s+)*public\s+(?:final\s+|indirect\s+)?(?:struct|enum|class|actor|protocol|typealias)\s+([A-Za-z_]\w*)', re.M)
EXCLUDED_MODULES = {'CalendarTestSupport', 'DedupBenchmark'}


def swift_files(root):
    for directory, _, files in os.walk(root):
        for name in files:
            if name.endswith('.swift'):
                yield os.path.join(directory, name)


def library_types(directories):
    names = set()
    for directory in directories:
        for path in swift_files(directory):
            relative = os.path.relpath(path, directory)
            if relative.split(os.sep)[0] in EXCLUDED_MODULES:
                continue
            names.update(DECLARATION.findall(open(path, encoding='utf-8', errors='ignore').read()))
    return names


def app_text(app):
    pieces = []
    for sub in ('Apps', 'Packages'):
        for path in swift_files(os.path.join(app, sub)):
            parts = path.split(os.sep)
            if 'Tests' in parts or '.build' in parts or 'DerivedData' in parts:
                continue
            pieces.append(open(path, encoding='utf-8', errors='ignore').read())
    return '\n'.join(pieces)


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--libraries', nargs='+', required=True)
    parser.add_argument('--app', required=True)
    parser.add_argument('--output')
    args = parser.parse_args(argv)
    text = app_text(args.app)
    declared = set(re.findall(r'\b(?:struct|enum|class|actor|protocol|typealias)\s+([A-Za-z_]\w*)', text))
    used = sorted(name for name in library_types(args.libraries)
                  if name not in declared and re.search(r'\b' + re.escape(name) + r'\b', text))
    header = ('# The Swift types the Mac app uses today: public types of the 1.x Swift libraries named in the non-test '
              'sources of Apps/ and Packages/ (regenerate with scripts/dev/list-app-types.py)\n')
    body = header + '\n'.join(used) + '\n'
    if args.output:
        with open(args.output, 'w', encoding='utf-8') as handle:
            handle.write(body)
    else:
        sys.stdout.write(body)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
