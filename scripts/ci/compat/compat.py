#!/usr/bin/env python3
"""Backward-compatibility checks of the shared library against the latest release (AGENTS.md, "Backward
compatibility"). Within a major version a release must drop into an existing front end with no change there, so a
pull request may add but not remove, retype or change; that needs the label semver:major.

Usage (from the repository root; the CI jobs `compat` and `compat-dotnet` run these):
  compat.py gate [--base-ref REF]            decide whether the checks run: writes run=true|false and baseline=TAG
                                             to $GITHUB_OUTPUT (pull requests only; skipped with a notice for
                                             semver:major, other events and when no X.Y.Z tag exists)
  compat.py baseline [--ref REF]             print the highest X.Y.Z tag reachable from REF (default origin/main)
  compat.py frozen BASELINE PATH...          fail if a file under PATH that exists in BASELINE was changed or
                                             deleted since (only additions are allowed)
  compat.py header BASELINE HEADER           fail if a declaration of HEADER at BASELINE (the tt_engine_* functions
                                             and the typedefs) differs, comments and spacing aside
  compat.py exports BASELINE HEADER LIBRARY  fail if LIBRARY does not export every function HEADER declared at BASELINE
  compat.py nuget-baseline [--index URL]     write version=<latest X.Y.Z of TimeTug.Engine on nuget.org> (empty:
                                             none or unreachable) to $GITHUB_OUTPUT

The pull request's labels come from the event payload at $GITHUB_EVENT_PATH; the event name from $GITHUB_EVENT_NAME.
Exit status: 0 pass or skip, 1 a compatibility break, 2 a usage or environment error."""
import json
import os
import re
import subprocess
import sys
import urllib.request

MAJOR_LABEL = 'semver:major'
RELEASE_TAG = re.compile(r'^(\d+)\.(\d+)\.(\d+)$')
NUGET_INDEX = 'https://api.nuget.org/v3-flatcontainer/timetug.engine/index.json'


def notice(message):
    print(f'::notice::{message}')


def error(message):
    print(f'::error::{message}')


def git(*args):
    return subprocess.run(['git', *args], check=True, capture_output=True, text=True).stdout


def highest_release(names):
    """The highest strict X.Y.Z among names, or None."""
    releases = [tuple(int(part) for part in match.groups()) for match in map(RELEASE_TAG.match, names) if match]
    return '.'.join(map(str, max(releases))) if releases else None


def baseline_tag(ref):
    return highest_release(git('tag', '--merged', ref, '--sort=-v:refname').split())


def labels(event):
    pull = event.get('pull_request') if isinstance(event, dict) else None
    found = pull.get('labels') if isinstance(pull, dict) else None
    return [label.get('name') for label in found or [] if isinstance(label, dict)]


def decide(event_name, event, baseline):
    """(run, reason): whether the compatibility checks apply to this run."""
    if event_name != 'pull_request':
        return False, f'compatibility checks run on pull requests only (event: {event_name or "none"})'
    if MAJOR_LABEL in labels(event):
        return False, f'the pull request is labelled {MAJOR_LABEL}: breaking changes are allowed, checks skipped'
    if not baseline:
        return False, 'no X.Y.Z release tag is reachable from the base branch (first release): nothing to compare with'
    return True, f'comparing with release {baseline}'


def load_event(path):
    if not path:
        return {}
    try:
        with open(path, encoding='utf-8') as handle:
            return json.load(handle)
    except (OSError, ValueError) as problem:
        raise SystemExit(f'compat: cannot read the event payload {path}: {problem}')


def write_outputs(**values):
    target = os.environ.get('GITHUB_OUTPUT')
    lines = ''.join(f'{key}={value}\n' for key, value in values.items())
    if target:
        with open(target, 'a', encoding='utf-8') as handle:
            handle.write(lines)
    else:
        sys.stdout.write(lines)


def changed_baseline_files(name_status):
    """The lines of `git diff --name-status --no-renames` that are not additions."""
    return [line for line in name_status.splitlines() if line.strip() and not line.startswith('A\t')]


def strip_comments(text):
    text = re.sub(r'/\*.*?\*/', ' ', text, flags=re.S)
    return re.sub(r'//[^\n]*', ' ', text)


def declarations(header):
    """{name: normalized declaration} for the exported functions and the typedefs of a C header."""
    lines = [line for line in strip_comments(header).splitlines() if not line.lstrip().startswith('#')]
    text = re.sub(r'extern\s+"C"\s*\{', ' ', '\n'.join(lines))
    found = {}
    for statement in text.split(';'):
        statement = ' '.join(statement.replace('}', ' ').split())
        statement = re.sub(r'\s*([*(),])\s*', r'\1', statement)
        if statement.startswith('TT_EXPORT '):
            match = re.search(r'(\w+)\(', statement)
        elif statement.startswith('typedef '):
            match = re.search(r'\(\*(\w+)\)', statement) or re.search(r'(\w+)$', statement)
        else:
            continue
        if match:
            found[match.group(1)] = statement
    return found


def exported_functions(header):
    return sorted(name for name, statement in declarations(header).items() if statement.startswith('TT_EXPORT '))


def header_breaks(baseline_header, current_header):
    old, new = declarations(baseline_header), declarations(current_header)
    problems = []
    for name, statement in sorted(old.items()):
        if name not in new:
            problems.append(f'{name} was removed (baseline: {statement})')
        elif new[name] != statement:
            problems.append(f'{name} changed: baseline "{statement}", now "{new[name]}"')
    return problems


def symbol_names(nm_output, darwin):
    names = set()
    for line in nm_output.splitlines():
        parts = line.split()
        if not parts:
            continue
        name = parts[-1]
        names.add(name[1:] if darwin and name.startswith('_') else name)
    return names


def library_symbols(library):
    darwin = library.endswith('.dylib')
    command = ['nm', '-gU', library] if darwin else ['nm', '-D', '--defined-only', library]
    return symbol_names(subprocess.run(command, check=True, capture_output=True, text=True).stdout, darwin)


def nuget_baseline(index_url):
    try:
        with urllib.request.urlopen(index_url, timeout=30) as response:
            versions = json.load(response).get('versions', [])
    except Exception as problem:  # noqa: BLE001 - any failure means "no baseline", reported as a notice
        notice(f'cannot read the published TimeTug.Engine versions ({problem}); package validation skipped')
        return None
    return highest_release(versions)


def main(argv):
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    command, args = argv[0], argv[1:]
    if command == 'gate':
        ref = args[args.index('--base-ref') + 1] if '--base-ref' in args else 'origin/main'
        event_name = os.environ.get('GITHUB_EVENT_NAME', '')
        event = load_event(os.environ.get('GITHUB_EVENT_PATH'))
        baseline = baseline_tag(ref) if event_name == 'pull_request' else None
        run, reason = decide(event_name, event, baseline)
        if run:
            print(reason)
        else:
            notice(reason)
        write_outputs(run='true' if run else 'false', baseline=baseline or '')
        return 0
    if command == 'baseline':
        ref = args[args.index('--ref') + 1] if '--ref' in args else 'origin/main'
        print(baseline_tag(ref) or '')
        return 0
    if command == 'frozen' and len(args) >= 2:
        baseline, paths = args[0], args[1:]
        changed = changed_baseline_files(git('diff', '--name-status', '--no-renames', baseline, 'HEAD', '--', *paths))
        for line in changed:
            status, _, path = line.partition('\t')
            error(f'{path}: {"deleted" if status == "D" else "modified"} since {baseline}; files that exist in the '
                  f'baseline may only be added to (label the pull request {MAJOR_LABEL} for a breaking change)')
        if not changed:
            print(f'frozen files unchanged since {baseline}: {" ".join(paths)}')
        return 1 if changed else 0
    if command == 'header' and len(args) == 2:
        baseline, header = args
        with open(header, encoding='utf-8') as handle:
            problems = header_breaks(git('show', f'{baseline}:{header}'), handle.read())
        for problem in problems:
            error(f'{header}: {problem} since {baseline} (C ABI; needs {MAJOR_LABEL})')
        if not problems:
            print(f'{header}: declarations unchanged since {baseline}')
        return 1 if problems else 0
    if command == 'exports' and len(args) == 3:
        baseline, header, library = args
        wanted = exported_functions(git('show', f'{baseline}:{header}'))
        if not wanted:
            error(f'{header} at {baseline} declares no exported function')
            return 2
        missing = [name for name in wanted if name not in library_symbols(library)]
        for name in missing:
            error(f'{library} no longer exports {name} (exported by {baseline}; needs {MAJOR_LABEL})')
        if not missing:
            print(f'{library} exports {" ".join(wanted)}')
        return 1 if missing else 0
    if command == 'nuget-baseline':
        index = args[args.index('--index') + 1] if '--index' in args else NUGET_INDEX
        version = nuget_baseline(index)
        if version:
            print(f'published baseline: TimeTug.Engine {version}')
        else:
            notice('no TimeTug.Engine release is published on nuget.org; package validation skipped')
        write_outputs(version=version or '')
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
