#!/usr/bin/env python3
"""Fails when a type the Mac app uses has no account in the bindings (phase 5, Task 3).
Usage: check-bindings-surface.py [--source DIR] [--surface FILE] [--docs FILE]

`docs/bindings-surface.txt` lists the public type names of the 1.x Swift libraries that the Mac app and its packages
use (`docs/bindings.md` says how to regenerate it). Each name must be one of

  * a type, host interface or custom type of `crates/timetug-engine-ffi/src` (a `pub struct`, `pub enum` or `pub trait`,
    a `mirror_record!`, `mirror_unit_enum!` or `field_update!` invocation, or a `uniffi::custom_type!` or
    `uniffi::custom_newtype!`) with the same name,
  * in the table "Names that differ" of `docs/bindings.md` (its first column), whose second column must itself be such a
    type,
  * in the table "Exposed as functions", whose second column lists `pub fn` names of the crate (comma separated),
  * in the table "Not exposed", with a reason (second column not empty), or
  * in the table "Planned" (listed in the output, not a failure; a later task removes the row).

Exits 1 with one line per problem."""
import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CRATE_SOURCE = ROOT / 'crates' / 'timetug-engine-ffi' / 'src'

DECLARATIONS = [
    re.compile(r'^\s*pub\s+(?:struct|enum|trait|type)\s+([A-Za-z_]\w*)', re.M),
    re.compile(r'^\s*mirror_record!\(\s*(?:forward_only\s+)?([A-Za-z_]\w*)', re.M),
    re.compile(r'^\s*mirror_unit_enum!\(\s*([A-Za-z_]\w*)', re.M),
    re.compile(r'^\s*field_update!\(\s*([A-Za-z_]\w*)', re.M),
    re.compile(r'uniffi::custom_(?:type|newtype)!\(\s*([A-Za-z_]\w*)'),
]


def strip_comments(text):
    text = re.sub(r'/\*.*?\*/', '', text, flags=re.S)
    return re.sub(r'//[^\n]*', '', text)


FUNCTION = re.compile(r'^\s*pub\s+(?:async\s+)?fn\s+([A-Za-z_]\w*)', re.M)


def exposed_functions(source):
    names = set()
    for path in sorted(Path(source).rglob('*.rs')):
        names.update(FUNCTION.findall(strip_comments(path.read_text(encoding='utf-8'))))
    return names


def exposed_names(source):
    names = set()
    for path in sorted(Path(source).rglob('*.rs')):
        text = strip_comments(path.read_text(encoding='utf-8'))
        for pattern in DECLARATIONS:
            names.update(pattern.findall(text))
    return names


def table_rows(docs_text, heading):
    """The rows (cells without backticks) of the first table under `## heading`."""
    section = re.search(r'^## ' + re.escape(heading) + r'\s*$(.*?)(?=^## |\Z)', docs_text, flags=re.M | re.S)
    rows = []
    if section is None:
        return rows
    for line in section.group(1).splitlines():
        line = line.strip()
        if not line.startswith('|') or set(line) <= set('|-: '):
            continue
        cells = [cell.strip().strip('`').strip() for cell in line.strip('|').split('|')]
        if cells and cells[0].lower() in ('swift name', ''):
            continue
        rows.append(cells)
    return rows


def swift_names(cell):
    """The Swift type names a first cell holds: comma separated, a nested type (`Outer.Inner`) by its last part."""
    return [part.strip().strip('`').strip().split('.')[-1] for part in cell.split(',') if part.strip()]


def expand(rows):
    """One (name, remaining cells) per Swift name of each row's first cell."""
    return [(name, row[1:]) for row in rows for name in swift_names(row[0])]


def surface_names(text):
    return [line.strip() for line in text.splitlines() if line.strip() and not line.strip().startswith('#')]


def problems(names, exposed, docs_text, functions=frozenset()):
    found, planned = [], []
    renamed = {name: rest[0] for name, rest in expand(table_rows(docs_text, 'Names that differ')) if rest}
    not_exposed = {name: (rest[0] if rest else '') for name, rest in expand(table_rows(docs_text, 'Not exposed'))}
    planned_names = {name for name, _ in expand(table_rows(docs_text, 'Planned'))}
    as_functions = {name: [f.strip().strip('`') for f in rest[0].split(',')] for name, rest in expand(table_rows(docs_text, 'Exposed as functions')) if rest}
    for swift, listed in sorted(as_functions.items()):
        for function in listed:
            if function not in functions:
                found.append(f'"Exposed as functions": {swift} -> {function}, but {function} is not a pub fn of the bindings')
    for swift, generated in sorted(renamed.items()):
        if generated not in exposed:
            found.append(f'"Names that differ": {swift} -> {generated}, but {generated} is not a type of the bindings')
    for swift, reason in sorted(not_exposed.items()):
        if not reason:
            found.append(f'"Not exposed": {swift} has no reason')
    for name in names:
        if name in exposed or name in renamed or name in not_exposed or name in as_functions:
            continue
        if name in planned_names:
            planned.append(name)
            continue
        found.append(f'{name}: used by the Mac app, but it is not a type of the bindings and is listed in no table of docs/bindings.md')
    return found, planned


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--source', default=str(CRATE_SOURCE))
    parser.add_argument('--surface', default=str(ROOT / 'docs' / 'bindings-surface.txt'))
    parser.add_argument('--docs', default=str(ROOT / 'docs' / 'bindings.md'))
    args = parser.parse_args(argv)
    try:
        names = surface_names(Path(args.surface).read_text(encoding='utf-8'))
        docs_text = Path(args.docs).read_text(encoding='utf-8')
    except OSError as error:
        print(f'error: {error}', file=sys.stderr)
        return 1
    found, planned = problems(names, exposed_names(args.source), docs_text, exposed_functions(args.source))
    for problem in found:
        print(f'error: {problem}', file=sys.stderr)
    if planned:
        print('planned (not yet in the bindings): ' + ', '.join(planned))
    if found:
        return 1
    print(f'{len(names)} names, all accounted for')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
