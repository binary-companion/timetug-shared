#!/usr/bin/env python3
"""Prints the tag a git dependency is pinned at in a Cargo.lock. Usage: cargo-dependency-version.py FILE REPOSITORY

REPOSITORY is the repository name at the end of the git URL (`calendar-connectors`). Every package of the lock whose
source is that repository must be pinned at the same tag (`?tag=X.Y.Z` in its source). Exits 1, printing why on
stderr, when the lock cannot be read, no package comes from the repository, a package is pinned by revision or
branch instead of by tag (the pin has no version then), or the packages disagree.

The release workflow uses it for the line "Built and tested with calendar-connectors <tag>"; the bump workflow of
`timetug` reads the same pin (phase 6)."""
import re
import sys

PACKAGE = re.compile(r'^\[\[package\]\]\s*$', re.M)
SOURCE = re.compile(r'^source = "git\+(?P<url>[^"?#]+)(?:\?(?P<query>[^"#]*))?(?:#(?P<commit>[0-9a-f]+))?"\s*$', re.M)
NAME = re.compile(r'^name = "([^"]+)"\s*$', re.M)


def pins(lock_text, repository):
    """{package name: ('tag'|'rev'|'branch'|'none', value)} for the packages whose git source is the repository."""
    found = {}
    for block in PACKAGE.split(lock_text)[1:]:
        source = SOURCE.search(block)
        name = NAME.search(block)
        if not source or not name:
            continue
        url = source.group('url')
        if url.rstrip('/').removesuffix('.git').rsplit('/', 1)[-1] != repository:
            continue
        query = dict(part.split('=', 1) for part in (source.group('query') or '').split('&') if '=' in part)
        for kind in ('tag', 'rev', 'branch'):
            if kind in query:
                found[name.group(1)] = (kind, query[kind])
                break
        else:
            found[name.group(1)] = ('none', source.group('commit') or '')
    return found


def pinned_tag(lock_text, repository):
    """(tag, None) or (None, why not)."""
    found = pins(lock_text, repository)
    if not found:
        return None, f'no package of {repository} in the lock'
    untagged = sorted(name for name, (kind, _) in found.items() if kind != 'tag')
    if untagged:
        return None, f'{", ".join(untagged)} of {repository} is not pinned by tag (a revision or branch has no version)'
    tags = {value for _, value in found.values()}
    if len(tags) != 1:
        return None, f'the packages of {repository} are pinned at different tags: {", ".join(sorted(tags))}'
    return tags.pop(), None


def main(argv):
    if len(argv) != 2:
        print('usage: cargo-dependency-version.py FILE REPOSITORY', file=sys.stderr)
        return 2
    try:
        with open(argv[0], encoding='utf-8') as handle:
            text = handle.read()
    except (OSError, UnicodeDecodeError) as problem:
        print(f'cannot read {argv[0]}: {problem}', file=sys.stderr)
        return 1
    tag, why = pinned_tag(text, argv[1])
    if why:
        print(f'{argv[0]}: {why}', file=sys.stderr)
        return 1
    print(tag)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
