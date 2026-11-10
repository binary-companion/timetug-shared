import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'check-crate-architecture.py'


CONNECTORS = ['calendar-core', 'calendar-oauth', 'calendar-ical', 'calendar-google', 'calendar-microsoft',
              'calendar-caldav', 'calendar-ical-subscription']
CRATES_IO = 'registry+https://github.com/rust-lang/crates.io-index'
CONNECTOR_GIT = 'git+https://github.com/binary-companion/calendar-connectors?rev=5ce4366#5ce43666af6a29e175addcd23e88774e3e8ccd47'


def package(name, deps=()):
    """A `cargo metadata` package entry. deps are (name, kind) pairs; kind None is a normal dependency.
    Connector crates come from the git dependency, workspace crates from paths, the rest from crates.io."""
    def source(dep):
        if dep.startswith('calendar-'):
            return CONNECTOR_GIT
        return None if dep.startswith('timetug-') or dep == 'dedup-benchmark' else CRATES_IO
    return {
        'name': name,
        'id': f'path+file:///work/crates/{name}#0.0.0',
        'source': None,
        'dependencies': [{'name': dep, 'kind': kind, 'source': source(dep)} for dep, kind in deps],
    }


# The workspace of design section 6.1 with every internal edge the table allows, plus external crates on the list.
CLEAN = [
    package('timetug-core', [('calendar-core', None), ('serde', None), ('jiff', None), ('thiserror', None)]),
    package('timetug-bridge', [('timetug-core', None), ('calendar-core', None), ('serde_json', 'dev')]),
    package('timetug-engine', [('timetug-core', None), ('timetug-bridge', None), ('calendar-core', None),
                               ('tokio', None), ('calendar-test-support', 'dev')]),
    package('timetug-engine-protocol', [('timetug-engine', None), ('timetug-core', None), ('timetug-bridge', None),
                                        ('calendar-core', None), ('serde_json', None)]),
    package('timetug-engine-c', [('timetug-engine-protocol', None), ('timetug-engine', None), ('timetug-core', None)]
            + [(name, None) for name in CONNECTORS]),
    package('timetug-engine-ffi', [('timetug-engine-protocol', None), ('timetug-engine', None), ('timetug-core', None)]
            + [(name, None) for name in CONNECTORS]),
    package('dedup-benchmark', [('timetug-core', None), ('calendar-core', None), ('jiff', None)]),
]


def run(packages):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'metadata.json'
        path.write_text(json.dumps({'packages': packages, 'workspace_members': [p['id'] for p in packages]}))
        result = subprocess.run([sys.executable, str(SCRIPT), '--metadata', str(path)], capture_output=True, text=True)
    return result.returncode, result.stdout + result.stderr


def replaced(name, **changes):
    return [dict(p, **changes) if p['name'] == name else p for p in CLEAN]


class CrateArchitectureTests(unittest.TestCase):
    def test_clean_workspace_passes(self):
        code, output = run(CLEAN)
        self.assertEqual(code, 0, output)

    def test_core_depending_on_the_bridge_fails_naming_the_edge(self):
        packages = replaced('timetug-core', dependencies=package('x', [('calendar-core', None), ('timetug-bridge', None)])['dependencies'])
        code, output = run(packages)
        self.assertEqual(code, 1, output)
        self.assertIn('timetug-core -> timetug-bridge', output)

    def test_core_depending_on_a_connector_other_than_calendar_core_fails(self):
        packages = replaced('timetug-core', dependencies=package('x', [('calendar-core', None), ('calendar-google', None)])['dependencies'])
        code, output = run(packages)
        self.assertEqual(code, 1, output)
        self.assertIn('timetug-core -> calendar-google', output)

    def test_bridge_depending_on_the_engine_fails(self):
        packages = replaced('timetug-bridge', dependencies=package('x', [('timetug-core', None), ('timetug-engine', None)])['dependencies'])
        code, output = run(packages)
        self.assertEqual(code, 1, output)
        self.assertIn('timetug-bridge -> timetug-engine', output)

    def test_engine_depending_on_the_protocol_fails(self):
        packages = replaced('timetug-engine', dependencies=package('x', [('timetug-engine-protocol', None)])['dependencies'])
        code, output = run(packages)
        self.assertEqual(code, 1, output)
        self.assertIn('timetug-engine -> timetug-engine-protocol', output)

    def test_dedup_benchmark_depending_on_the_engine_fails(self):
        packages = replaced('dedup-benchmark', dependencies=package('x', [('timetug-engine', None)])['dependencies'])
        code, output = run(packages)
        self.assertEqual(code, 1, output)
        self.assertIn('dedup-benchmark -> timetug-engine', output)

    def test_external_crate_off_the_list_fails(self):
        packages = replaced('timetug-engine-c', dependencies=package('x', [('timetug-engine', None), ('reqwest', None)])['dependencies'])
        code, output = run(packages)
        self.assertEqual(code, 1, output)
        self.assertIn('reqwest', output)

    def test_uniffi_is_allowed_only_in_the_bindings_crate(self):
        ffi = replaced('timetug-engine-ffi', dependencies=package('x', [('timetug-engine', None), ('uniffi', None)])['dependencies'])
        self.assertEqual(run(ffi)[0], 0, run(ffi)[1])
        other = replaced('timetug-engine-c', dependencies=package('x', [('timetug-engine', None), ('uniffi', None)])['dependencies'])
        code, output = run(other)
        self.assertEqual(code, 1, output)
        self.assertIn('timetug-engine-c -> uniffi', output)

    def test_test_support_is_allowed_only_as_a_dev_dependency(self):
        packages = replaced('timetug-core', dependencies=package('x', [('calendar-core', None), ('calendar-test-support', None)])['dependencies'])
        code, output = run(packages)
        self.assertEqual(code, 1, output)
        self.assertIn('timetug-core -> calendar-test-support', output)

    def test_workspace_crate_missing_from_the_table_fails(self):
        code, output = run(CLEAN + [package('timetug-extra', [('timetug-core', None)])])
        self.assertEqual(code, 1, output)
        self.assertIn('timetug-extra', output)

    def test_unreadable_metadata_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'metadata.json'
            path.write_text('not json')
            result = subprocess.run([sys.executable, str(SCRIPT), '--metadata', str(path)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)


if __name__ == '__main__':
    unittest.main()
