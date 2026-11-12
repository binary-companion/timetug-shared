#!/usr/bin/env python3
"""Fails when the crates that must be deterministic read the clock or the environment.
Usage: check-no-clock.py [--root DIR]

Phase 2 of the Rust engine program: timetug-core and timetug-bridge never read the clock; every function that needs the
time takes `now: Timestamp`, and nothing reads the environment. Phase 3 adds timetug-engine to the scan with one
exception: the engine's only wall clock is `SystemClock` in crates/timetug-engine/src/clock.rs, so that one file may
use `tokio::time` (to sleep) and `Timestamp::now` (to read the time); everything else in the engine takes its time from
`EngineClock`. Scans every .rs file under the crates below, in DIR (default: the repository root), and exits 1, naming
file, line and token, when code contains one of:

  Timestamp::now  SystemTime  Instant::now  Zoned::now  tokio::time  std::env

Comments and the contents of string and character literals are ignored. Tests are excluded: files under a `tests`
directory, files named tests.rs, and items marked #[cfg(test)] (modules, functions, uses, struct fields, enum variants). A missing crate directory is skipped (a
crate may not exist yet)."""
import argparse
import re
import sys
from pathlib import Path

# (crate source directory, {file relative to the repository root: the names that file may use}, what to use instead).
CRATES = [
    ('crates/timetug-core/src', {}, 'take `now: Timestamp` as an argument'),
    ('crates/timetug-bridge/src', {}, 'take `now: Timestamp` as an argument'),
    ('crates/timetug-engine/src', {'crates/timetug-engine/src/clock.rs': {'tokio::time', 'Timestamp::now'}},
     'read the time and sleep through `EngineClock` (only `SystemClock` in clock.rs touches the wall clock and tokio::time)'),
]

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
    """Replaces every item marked #[cfg(test)] with spaces: an inline module, function or use (up to its closing brace or
    semicolon), or a struct field, enum variant or match arm (up to its comma, or up to the brace that closes the
    enclosing block, which is left alone)."""
    result = list(code)
    for match in re.finditer(r'#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]', code):
        j = match.end()
        depth = 0  # {} opened by the item itself
        nesting = 0  # () and [], where `;` and `,` do not end the item (`[u8; 4]`, `f(a, b)`)
        angle = 0  # <>, where `,` does not end the item (`HashMap<A, B>`)
        while j < len(code):
            c = code[j]
            if c in '([':
                nesting += 1
            elif c in ')]':
                nesting -= 1
            elif c == '<' and code[j - 1] not in '-=':
                angle += 1
            elif c == '>' and code[j - 1] not in '-=' and angle > 0:
                angle -= 1
            elif c == '{':
                depth += 1
            elif c == '}':
                if depth == 0:
                    break  # the block around the item ends here; the brace is not part of the item
                depth -= 1
                if depth == 0:
                    j += 1
                    break
            elif c == ';' and depth == 0 and nesting == 0:
                j += 1
                break
            elif c == ',' and depth == 0 and nesting == 0 and angle == 0:
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
    for crate_dir, allowed, advice in CRATES:
        base = root / crate_dir
        if not base.is_dir():
            continue
        for path in sorted(base.rglob('*.rs')):
            if is_test_file(path, base):
                continue
            relative = path.relative_to(root).as_posix()
            for line, name in problems_in(path):
                if name in allowed.get(relative, ()):
                    continue
                problems.append(f'{relative}:{line}: `{name}` is forbidden here; {advice}')
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
