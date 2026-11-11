#!/usr/bin/env python3
"""Fails when the pure crates read the clock or the environment.
Usage: check-no-clock.py [--root DIR]

Phase 2 of the Rust engine program: timetug-core and timetug-bridge never read the clock; every function that needs the
time takes `now: Timestamp`, and nothing reads the environment. Scans every .rs file under crates/timetug-core/src and
crates/timetug-bridge/src of DIR (default: the repository root) and exits 1, naming file, line and token, when code
contains one of:

  Timestamp::now  SystemTime  Instant::now  Zoned::now  tokio::time  std::env

Comments and the contents of string and character literals are ignored. Tests are excluded: files under a `tests`
directory, files named tests.rs, and items marked #[cfg(test)]. A missing crate directory is skipped (the bridge may not
exist yet)."""
import argparse
import re
import sys
from pathlib import Path

CRATE_DIRS = ['crates/timetug-core/src', 'crates/timetug-bridge/src']

# (name for the message, pattern). `::` may have spaces around it. The braced forms catch `use std::{env, ...}`.
FORBIDDEN = [
    ('Timestamp::now', re.compile(r'\bTimestamp\s*::\s*now\b')),
    ('SystemTime', re.compile(r'\bSystemTime\b')),
    ('Instant::now', re.compile(r'\bInstant\s*::\s*now\b')),
    ('Zoned::now', re.compile(r'\bZoned\s*::\s*now\b')),
    ('tokio::time', re.compile(r'\btokio\s*::\s*time\b')),
    ('tokio::time', re.compile(r'\btokio\s*::\s*\{[^{}]*\btime\b')),
    ('std::env', re.compile(r'\bstd\s*::\s*env\b')),
    ('std::env', re.compile(r'\bstd\s*::\s*\{[^{}]*\benv\b')),
]


def blank(source):
    """The source with comments and the insides of string and character literals replaced by spaces (newlines kept), so
    positions and line numbers stay the same."""
    out = []
    i, n = 0, len(source)

    def spaces(text):
        return ''.join(c if c == '\n' else ' ' for c in text)

    while i < n:
        c = source[i]
        two = source[i:i + 2]
        if two == '//':
            j = source.find('\n', i)
            j = n if j == -1 else j
            out.append(spaces(source[i:j]))
            i = j
        elif two == '/*':
            depth, j = 1, i + 2
            while j < n and depth:
                if source[j:j + 2] == '/*':
                    depth, j = depth + 1, j + 2
                elif source[j:j + 2] == '*/':
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append(spaces(source[i:j]))
            i = j
        elif c == 'r' and re.match(r'r#*"', source[i:i + 40]) and (i == 0 or not (source[i - 1].isalnum() or source[i - 1] == '_')):
            hashes = len(re.match(r'r(#*)"', source[i:]).group(1))
            end = source.find('"' + '#' * hashes, i + 2 + hashes)
            j = n if end == -1 else end + 1 + hashes
            out.append(spaces(source[i:j]))
            i = j
        elif c == '"':
            j = i + 1
            while j < n and source[j] != '"':
                j += 2 if source[j] == '\\' else 1
            j = min(j + 1, n)
            out.append(spaces(source[i:j]))
            i = j
        elif c == "'":
            # A character literal ('x', '\n', '\u{..}'), not a lifetime ('a).
            match = re.match(r"'(?:\\u\{[0-9a-fA-F_]+\}|\\.|[^\\'\n])'", source[i:i + 14])
            if match:
                out.append(spaces(match.group(0)))
                i += len(match.group(0))
            else:
                out.append(c)
                i += 1
        else:
            out.append(c)
            i += 1
    return ''.join(out)


def drop_cfg_test_items(code):
    """Replaces every item marked #[cfg(test)] (an inline module, function or use) with spaces."""
    result = list(code)
    for match in re.finditer(r'#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]', code):
        j = match.end()
        depth = 0
        nesting = 0  # () and [], where a `;` does not end the item (`[u8; 4]`)
        while j < len(code):
            if code[j] in '([':
                nesting += 1
            elif code[j] in ')]':
                nesting -= 1
            elif code[j] == '{':
                depth += 1
            elif code[j] == '}':
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            elif code[j] == ';' and depth == 0 and nesting == 0:
                j += 1
                break
            j += 1
        for k in range(match.start(), min(j, len(code))):
            if result[k] != '\n':
                result[k] = ' '
    return ''.join(result)


def is_test_file(path, base):
    relative = path.relative_to(base)
    return 'tests' in relative.parts[:-1] or relative.name == 'tests.rs'


def problems_in(path):
    code = drop_cfg_test_items(blank(path.read_text(encoding='utf-8', errors='replace')))
    found = []
    for name, pattern in FORBIDDEN:
        for match in pattern.finditer(code):
            line = code.count('\n', 0, match.start()) + 1
            found.append((line, name))
    return sorted(set(found))


def check(root):
    problems = []
    for crate_dir in CRATE_DIRS:
        base = root / crate_dir
        if not base.is_dir():
            continue
        for path in sorted(base.rglob('*.rs')):
            if is_test_file(path, base):
                continue
            for line, name in problems_in(path):
                problems.append(f'{path.relative_to(root)}:{line}: `{name}` is forbidden in the pure crates; take `now: Timestamp` as an argument')
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--root', default=str(Path(__file__).resolve().parents[2]), help='the repository root')
    args = parser.parse_args()
    try:
        problems = check(Path(args.root))
    except OSError as error:
        print(f'check-no-clock: cannot read the sources: {error}', file=sys.stderr)
        return 1
    for problem in problems:
        print(f'check-no-clock: {problem}', file=sys.stderr)
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
