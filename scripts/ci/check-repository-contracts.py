#!/usr/bin/env python3
"""Repository invariants that the compiler does not enforce. Usage: check-repository-contracts.py [root]"""
import re
import sys
from pathlib import Path

import yaml

root = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parents[2]
errors = []

def require(condition, message):
    if not condition:
        errors.append(message)

# The portable sources stay free of Apple-only and UI frameworks: TimeTugCore builds on Linux and
# Windows, and CalendarBridge is minimal glue between it and the connector library.
FORBIDDEN = {'AppKit', 'SwiftUI', 'EventKit', 'WidgetKit', 'FoundationModels',
             'UIKit', 'Security', 'CryptoKit', 'Network', 'Combine'}
# "import X", "@testable import X", "public import X", "@_exported public import X", "import struct X.Y".
IMPORT = re.compile(
    r'^[ \t]*(?:(?:@\w+(?:\([^)]*\))?|public|package|internal|fileprivate|private)[ \t]+)*'
    r'import[ \t]+(?:(?:struct|class|enum|protocol|typealias|let|var|func)[ \t]+)?(\w+)', re.M)
for target in ('TimeTugCore', 'CalendarBridge'):
    directory = root / 'Sources' / target
    require(directory.is_dir(), f'missing documented path: Sources/{target}')
    for file in sorted(directory.rglob('*.swift')):
        relative = file.relative_to(root)
        text = file.read_text()
        for match in IMPORT.finditer(text):
            require(match.group(1) not in FORBIDDEN,
                    f'Apple/UI import in portable source ({target}): '
                    f'{relative}:{text.count(chr(10), 0, match.start()) + 1}: import {match.group(1)}')

# Workflows: untrusted code never meets secrets (program design, section 8).
ALLOWED_ANYWHERE = {'GITHUB_TOKEN', 'DOCKERHUB_USERNAME', 'DOCKERHUB_TOKEN'}
def expressions_in(text):
    """The bodies of the ${{ ... }} expressions in text. The scan is quote-aware: a "}}" inside a
    single-quoted literal (with '' as its escape) does not end the expression. It fails closed: an
    expression with no closing "}}" outside quotes runs to the end of the string."""
    bodies, i = [], text.find('${{')
    while i >= 0:
        start = j = i + 3
        quoted = False
        end = len(text)
        while j < len(text):
            if text[j] == "'":
                if quoted and text.startswith("''", j):
                    j += 2
                    continue
                quoted = not quoted
            elif not quoted and text.startswith('}}', j):
                end = j
                break
            j += 1
        bodies.append(text[start:end])
        i = text.find('${{', min(end + 2, len(text)))
    return bodies
# Context and secret names are case-insensitive in GitHub expressions: SECRETS.X and Secrets.x are the same.
SECRET_NAME = re.compile(r"(?<![\w.])secrets\s*(?:\.\s*([A-Za-z0-9_]+)|\[\s*['\"]([^'\"]+)['\"]\s*\])", re.I)
# The secrets context used whole (toJSON(secrets), secrets[expr], format('{0}', secrets)): every secret at once.
WHOLE_CONTEXT = re.compile(
    r"(?<![\w.'\"])secrets\b(?!\s*\.\s*\w|\s*\[\s*['\"][^'\"]+['\"]\s*\])", re.I)

def strings(node, key=None):
    """Every string in a parsed YAML document, paired with the mapping key it sits under."""
    if isinstance(node, str):
        yield key, node
    elif isinstance(node, dict):
        for k, v in node.items():
            yield from strings(k)
            yield from strings(v, k)
    elif isinstance(node, list):
        for item in node:
            yield from strings(item, key)

def secret_use(node):
    """(names of secrets referenced, whether the whole secrets context is used) anywhere in node."""
    names, whole = set(), False
    for key, text in strings(node):
        # Only expression code can read the secrets context: the bodies of ${{ ... }} and an `if:`
        # value (an expression with or without the braces). Prose such as "Never echo secrets. Mask
        # them" in a name or a script is not a reference.
        expressions = expressions_in(text) + ([text] if key == 'if' else [])
        for expression in expressions:
            for match in SECRET_NAME.finditer(expression):
                names.add((match.group(1) or match.group(2)).upper())
        whole = whole or any(WHOLE_CONTEXT.search(expression) for expression in expressions)
    return names, whole

def named_environment(job):
    environment = job.get('environment')
    if isinstance(environment, dict):
        environment = environment.get('name')
    return isinstance(environment, str) and bool(environment.strip())

def load(path):
    try:
        data = yaml.safe_load(path.read_text())
    except yaml.YAMLError as problem:
        require(False, f'{path.relative_to(root)}: not valid YAML: {problem}')
        return {}
    return data if isinstance(data, dict) else {}

# The documented workflows must exist: a deleted release or CI workflow must not pass unnoticed.
require((root / '.github' / 'workflows').is_dir(), 'missing documented path: .github/workflows')
if (root / '.github' / 'workflows').is_dir():
    for documented in ('ci.yml', 'release.yml', 'bump-dependencies.yml'):
        require((root / '.github' / 'workflows' / documented).is_file(),
                f'missing documented path: .github/workflows/{documented}')

# The backward-compatibility jobs (AGENTS.md, "Backward compatibility") are documented by id and become required
# checks; a renamed or deleted one would silently stop enforcing the rule.
ci_workflow = root / '.github' / 'workflows' / 'ci.yml'
if ci_workflow.is_file():
    ci_jobs = load(ci_workflow).get('jobs')
    for documented in ('compat', 'compat-dotnet'):
        require(isinstance(ci_jobs, dict) and documented in ci_jobs,
                f'.github/workflows/ci.yml: missing documented job {documented}')

for workflow in sorted((root / '.github' / 'workflows').glob('*.y*ml')):
    data = load(workflow)
    name = workflow.name
    triggers = data.get(True, data.get('on')) or {}
    names = triggers if isinstance(triggers, (list, dict)) else [triggers]
    require('pull_request_target' not in names, f'{name}: pull_request_target is banned')
    jobs = data.get('jobs') if isinstance(data.get('jobs'), dict) else {}
    # Workflow-level keys (env, defaults, ...) apply to every job, so a secret there reaches all of them.
    used, whole = secret_use({key: value for key, value in data.items() if key != 'jobs'})
    for secret in sorted(used - ALLOWED_ANYWHERE):
        require(False, f'{name}: workflow-level use of secret {secret} (only {sorted(ALLOWED_ANYWHERE)} may be)')
    require(not whole, f'{name}: workflow-level use of the whole secrets context')
    for job_id, job in jobs.items():
        job = job if isinstance(job, dict) else {}
        require(job.get('secrets') != 'inherit', f'{name}: job {job_id} passes secrets: inherit')
        used, whole = secret_use(job)
        require(not whole, f'{name}: job {job_id} uses the whole secrets context (secrets[...], toJSON(secrets))')
        for secret in sorted(used - ALLOWED_ANYWHERE):
            require(named_environment(job),
                    f'{name}: job {job_id} uses secret {secret} outside an environment')

# Composite actions cannot read the secrets context directly: any use there is a mistake or a leak.
for action in sorted((root / '.github' / 'actions').glob('**/action.y*ml')):
    used, whole = secret_use(load(action))
    for secret in sorted(used):
        require(False, f'{action.relative_to(root)}: composite action uses secret {secret}')
    require(not whole, f'{action.relative_to(root)}: composite action uses the whole secrets context')

# Protocol: one golden fixture per message type, no orphan fixtures. The type lists are string literals in
# MessageCatalog.swift and ServiceMessages.swift; fixtures named edge-*.json are extra cases, not types.
catalog = root / 'Sources' / 'TimeTugEngineProtocol' / 'MessageCatalog.swift'
services = root / 'Sources' / 'TimeTugEngineProtocol' / 'ServiceMessages.swift'
golden = root / 'Conformance' / 'Fixtures' / 'golden'
if catalog.is_file():
    def literals(path, marker):
        text = path.read_text()
        start = text.find(marker)
        require(start >= 0, f'{path.relative_to(root)}: missing {marker}')
        if start < 0:
            return set()
        end = text.find(']', start + len(marker))
        return set(re.findall(r'"([A-Za-z0-9.]+)"', text[start:end]))
    types = (literals(catalog, 'hostToEngine: [String] = [') | literals(catalog, 'engineToHost: [String] = [')
             | (literals(services, 'types: [String] = [') if services.is_file() else set()))
    require(golden.is_dir(), 'missing documented path: Conformance/Fixtures/golden')
    fixtures = {f.stem for f in golden.glob('*.json') if not f.name.startswith('edge-')} if golden.is_dir() else set()
    for missing in sorted(types - fixtures):
        require(False, f'Conformance/Fixtures/golden: no fixture for message type {missing}')
    for orphan in sorted(fixtures - types):
        require(False, f'Conformance/Fixtures/golden/{orphan}.json: fixture for a type not in the message catalog')

if errors:
    for error in errors:
        print(f'FAIL: {error}', file=sys.stderr)
    sys.exit(1)
print('PASS: repository contracts')
