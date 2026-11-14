#!/usr/bin/env python3
"""Which release tags belong to a branch (AGENTS.md, "Releases and release commits").

A release of timetug-shared with the Swift package is tagged on a *release commit*: a commit whose only parent is the
`main` (or `v2`) commit being released and whose only change is `Package.swift` (the binary target rewritten to the
released XCFramework URL). That commit is on no branch, so `git tag --merged <ref>` and `git tag --points-at <sha>`
cannot see it. This script owns the rule; the release workflow and compat.py call it and never use those commands.

A strict X.Y.Z tag belongs to <ref> when
  - its commit is an ancestor of <ref> (source commit = the tag commit; every tag made before the Swift package), or
  - its commit is a release commit (one parent, `git diff --name-only parent commit` is exactly `Package.swift`) and
    that parent is an ancestor of <ref> (source commit = the parent).
Prerelease tags (X.Y.Z-alpha.N) are ignored.

Usage (from inside the repository):
  release-tags.py list   --ref REF   one line per belonging tag, highest first: "<X.Y.Z> <tag commit> <source commit>"
  release-tags.py latest --ref REF   the highest belonging version (empty when none)
  release-tags.py at     --sha SHA   the highest strict version whose source commit is SHA (empty when none)
  release-tags.py source --tag TAG   the source commit of TAG (any tag name, prereleases included)
"""
import re
import subprocess
import sys

STRICT = re.compile(r'^(\d+)\.(\d+)\.(\d+)$')


def git(*args, check=True):
    return subprocess.run(['git', *args], check=check, capture_output=True, text=True)


def tag_commit(tag):
    result = git('rev-parse', '-q', '--verify', f'refs/tags/{tag}^{{commit}}', check=False)
    return result.stdout.strip() or None


def release_parent(commit):
    """The parent when `commit` is a release commit by structure, else None."""
    parents = git('rev-list', '--parents', '-n', '1', commit).stdout.split()[1:]
    if len(parents) != 1:
        return None
    changed = git('diff', '--name-only', parents[0], commit).stdout.split()
    return parents[0] if changed == ['Package.swift'] else None


def is_ancestor(commit, ref):
    return git('merge-base', '--is-ancestor', commit, ref, check=False).returncode == 0


def strict_tags():
    names = git('tag', '--list').stdout.split()
    found = [(tuple(int(part) for part in match.groups()), name) for name in names if (match := STRICT.match(name))]
    return [name for _, name in sorted(found, reverse=True)]


def belonging(ref):
    """[(version, tag commit, source commit)] highest first."""
    rows = []
    for tag in strict_tags():
        commit = tag_commit(tag)
        if commit is None:
            continue
        if is_ancestor(commit, ref):
            rows.append((tag, commit, commit))
            continue
        parent = release_parent(commit)
        if parent is not None and is_ancestor(parent, ref):
            rows.append((tag, commit, parent))
    return rows


def belonging_latest(ref):
    rows = belonging(ref)
    return rows[0][0] if rows else ''


def source_of(tag):
    commit = tag_commit(tag)
    if commit is None:
        return None
    return release_parent(commit) or commit


def at(sha):
    full = git('rev-parse', '--verify', f'{sha}^{{commit}}').stdout.strip()
    for tag in strict_tags():
        commit = tag_commit(tag)
        if commit == full or (commit and release_parent(commit) == full):
            return tag
    return ''


def option(args, name):
    if name not in args or args.index(name) + 1 >= len(args):
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    return args[args.index(name) + 1]


def main(argv):
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    command, args = argv[0], argv[1:]
    if command == 'list':
        for row in belonging(option(args, '--ref')):
            print(' '.join(row))
    elif command == 'latest':
        print(belonging_latest(option(args, '--ref')))
    elif command == 'at':
        print(at(option(args, '--sha')))
    elif command == 'source':
        print(source_of(option(args, '--tag')) or '')
    else:
        print(__doc__, file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
