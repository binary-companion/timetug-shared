"""Tests of scripts/ci/compat (the backward-compatibility checks) and of the CI jobs that run them.

The git-based checks run against throwaway repositories with a baseline tag, so the real commands are what is
tested: a file of the baseline changed or deleted fails, an added one passes; a changed C declaration fails, a changed
comment does not; and the semver:major label from the event payload skips every check."""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[3]
COMPAT = ROOT / 'scripts' / 'ci' / 'compat' / 'compat.py'
EXPORT = ROOT / 'scripts' / 'ci' / 'compat' / 'export-baseline.sh'
sys.path.insert(0, str(COMPAT.parent))
import compat  # noqa: E402

HEADER = '''/* the C interface */
#ifndef H
#define H
#ifdef __cplusplus
extern "C" {
#endif
#include <stdint.h>
typedef struct tt_engine tt_engine;
typedef void (*tt_message_fn)(void *context, const char *message_json);
TT_EXPORT tt_engine *tt_engine_create(const char *config_json, tt_message_fn on_message, void *context);
TT_EXPORT void tt_engine_send(tt_engine *engine, const char *command_json); // sends
TT_EXPORT void tt_engine_reply(tt_engine *engine, uint64_t request_id, const char *reply_json);
TT_EXPORT void tt_engine_destroy(tt_engine *engine);
#ifdef __cplusplus
}
#endif
#endif
'''
GIT_ENV = {'GIT_AUTHOR_NAME': 't', 'GIT_AUTHOR_EMAIL': 't@example.com', 'GIT_COMMITTER_NAME': 't',
           'GIT_COMMITTER_EMAIL': 't@example.com', 'GIT_CONFIG_GLOBAL': os.devnull, 'GIT_CONFIG_NOSYSTEM': '1'}


class Repo:
    """A throwaway repository with golden fixtures, a scenario, a persisted fixture and the header, tagged 1.0.0."""

    def __init__(self, directory):
        self.root = Path(directory)
        self.git('init', '-q', '-b', 'main')
        self.write('Conformance/Fixtures/golden/ready.json', '{"v":1,"type":"ready"}\n')
        self.write('Conformance/Scenarios/start.json', SCENARIO)
        self.write('Conformance/Scenarios/data/token.json', '{"access_token":"x"}\n')
        self.write('Tests/TimeTugEngineTests/Fixtures/settings-legacy.json', '{"leadTime":60}\n')
        self.write('include/timetug_engine.h', HEADER)
        self.commit('baseline')
        self.git('tag', '1.0.0')

    def git(self, *args):
        return subprocess.run(['git', *args], cwd=self.root, check=True, capture_output=True, text=True,
                              env={**os.environ, **GIT_ENV}).stdout

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def commit(self, message):
        self.git('add', '-A')
        self.git('commit', '-q', '--allow-empty', '-m', message)

    def compat(self, *args, env=None):
        return subprocess.run([sys.executable, str(COMPAT), *args], cwd=self.root, capture_output=True, text=True,
                              env={**os.environ, **GIT_ENV, **(env or {})})


FROZEN = ['--additive', 'Conformance/Fixtures/golden', '--appendable', 'Conformance/Scenarios',
          '--strict', 'Tests/TimeTugEngineTests/Fixtures']
FROZEN_PATHS = [path for path in FROZEN if not path.startswith('--')]
SCENARIO = '{"steps":[{"send":{"type":"start"}},{"expect":{"state":{"takeoverVisible":false}}}]}\n'



class Baseline(unittest.TestCase):
    def test_highest_strict_release(self):
        self.assertEqual(compat.highest_release(['1.2.0', '1.10.0', '1.9.9', 'v9.0.0', '2.0.0-rc.1', 'x']), '1.10.0')
        self.assertIsNone(compat.highest_release(['v1.0.0', 'latest']))

    def test_tag_reachable_from_the_base_branch(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            repo.commit('second')
            repo.git('tag', '1.1.0')
            repo.git('tag', 'v7.0.0')
            repo.git('checkout', '-q', '-b', 'side')
            repo.commit('side')
            repo.git('tag', '9.0.0')  # on a side branch: not reachable from main
            repo.git('checkout', '-q', 'main')
            self.assertEqual(repo.compat('baseline', '--ref', 'main').stdout.strip(), '1.1.0')

    def test_no_tag_means_no_baseline(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            repo.git('tag', '-d', '1.0.0')
            self.assertEqual(repo.compat('baseline', '--ref', 'main').stdout.strip(), '')


class Gate(unittest.TestCase):
    def pull_request(self, *names):
        return {'pull_request': {'labels': [{'name': name} for name in names]}}

    def test_a_pull_request_with_a_baseline_runs(self):
        self.assertEqual(compat.decide('pull_request', self.pull_request('semver:minor'), '1.6.3'),
                         (True, 'comparing with release 1.6.3'))

    def test_the_major_label_skips(self):
        run, reason = compat.decide('pull_request', self.pull_request('semver:minor', 'semver:major'), '1.6.3')
        self.assertFalse(run)
        self.assertIn('semver:major', reason)

    def test_other_events_and_the_first_release_skip(self):
        self.assertFalse(compat.decide('push', {}, '1.6.3')[0])
        self.assertFalse(compat.decide('pull_request', self.pull_request(), None)[0])

    def test_a_label_that_only_resembles_major_does_not_skip(self):
        self.assertTrue(compat.decide('pull_request', self.pull_request('semver:major?', 'major'), '1.0.0')[0])

    def run_gate(self, repo, event, event_name='pull_request'):
        event_file = repo.root / 'event.json'
        event_file.write_text(json.dumps(event))
        output = repo.root / 'output'
        output.write_text('')
        result = repo.compat('gate', '--base-ref', 'main', env={
            'GITHUB_EVENT_PATH': str(event_file), 'GITHUB_EVENT_NAME': event_name, 'GITHUB_OUTPUT': str(output)})
        self.assertEqual(result.returncode, 0, result.stderr)
        return result, dict(line.split('=', 1) for line in output.read_text().splitlines())

    def test_the_gate_reads_the_event_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            _, outputs = self.run_gate(repo, self.pull_request('semver:minor'))
            self.assertEqual(outputs, {'run': 'true', 'baseline': '1.0.0'})
            result, outputs = self.run_gate(repo, self.pull_request('semver:major'))
            self.assertEqual(outputs['run'], 'false')
            self.assertIn('::notice::', result.stdout)
            _, outputs = self.run_gate(repo, {}, event_name='push')
            self.assertEqual(outputs['run'], 'false')

    def test_a_major_labelled_breaking_change_passes_the_job(self):
        # Every category broken at once; with semver:major the gate turns every check step off, so the job passes.
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            repo.write('Conformance/Fixtures/golden/ready.json', '{"v":1,"kind":"ready"}\n')
            repo.write('Tests/TimeTugEngineTests/Fixtures/settings-legacy.json', '{"leadTime":60,"added":1}\n')
            (repo.root / 'Conformance/Scenarios/start.json').unlink()
            repo.write('include/timetug_engine.h', HEADER.replace('const char *command_json', 'const char *json, int n'))
            repo.commit('break everything')
            self.assertEqual(repo.compat('frozen', '1.0.0', *FROZEN).returncode, 1)
            self.assertEqual(repo.compat('header', '1.0.0', 'include/timetug_engine.h').returncode, 1)
            _, outputs = self.run_gate(repo, self.pull_request('semver:major'))
            self.assertEqual(outputs['run'], 'false')
            for job in ('compat', 'compat-dotnet'):
                checks = [step for step in workflow()['jobs'][job]['steps'] if 'gate' not in str(step.get('id', ''))
                          and 'uses' not in step]
                for step in checks:
                    self.assertTrue(skipped_when_gate_is_off(step), f'{job}: {step.get("name")} runs for semver:major')


class Frozen(unittest.TestCase):
    def frozen(self, change):
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            change(repo)
            repo.commit('change')
            return repo.compat('frozen', '1.0.0', *FROZEN)

    def assert_passes(self, change):
        result = self.frozen(change)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def assert_breaks(self, change, needle):
        result = self.frozen(change)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(needle, result.stdout)
        self.assertIn('semver:major', result.stdout)

    def test_unchanged_and_added_files_pass(self):
        def change(repo):
            repo.write('Conformance/Fixtures/golden/edge-new.json', '{}\n')
            repo.write('Conformance/Scenarios/new.json', '{}\n')
            repo.write('Conformance/Scenarios/data/new.json', '{}\n')
            repo.write('Tests/TimeTugEngineTests/Fixtures/new-format.json', '{}\n')
            repo.write('Sources/Unrelated.swift', '// changed elsewhere\n')
        self.assert_passes(change)

    # --- golden fixtures: additive changes only ---

    def test_a_field_added_to_a_golden_fixture_passes(self):
        result = self.assert_passes(lambda repo: repo.write(
            'Conformance/Fixtures/golden/ready.json', '{\n  "v": 1,\n  "type": "ready",\n  "payload": {"new": true}\n}\n'))
        self.assertIn('ready.json: additive change', result.stdout)

    def test_a_renamed_field_in_a_golden_fixture_fails(self):
        self.assert_breaks(lambda repo: repo.write('Conformance/Fixtures/golden/ready.json', '{"v":1,"kind":"ready"}\n'),
                           'Conformance/Fixtures/golden/ready.json: modified')

    def test_a_removed_field_in_a_golden_fixture_fails(self):
        self.assert_breaks(lambda repo: repo.write('Conformance/Fixtures/golden/ready.json', '{"type":"ready"}\n'),
                           'Conformance/Fixtures/golden/ready.json: modified')

    def test_a_retyped_or_changed_value_in_a_golden_fixture_fails(self):
        for text in ('{"v":"1","type":"ready"}', '{"v":true,"type":"ready"}', '{"v":2,"type":"ready"}'):
            with self.subTest(text=text):
                self.assert_breaks(lambda repo: repo.write('Conformance/Fixtures/golden/ready.json', text + '\n'),
                                   'Conformance/Fixtures/golden/ready.json: modified')

    def test_an_entry_appended_to_a_golden_array_fails(self):
        def change(repo):
            repo.write('Conformance/Fixtures/golden/ready.json', '{"v":1,"type":"ready","list":[1]}\n')
            repo.commit('baseline list')
            repo.git('tag', '-f', '1.0.0')
            repo.write('Conformance/Fixtures/golden/ready.json', '{"v":1,"type":"ready","list":[1,2]}\n')
        self.assert_breaks(change, 'Conformance/Fixtures/golden/ready.json: modified')

    # --- scenarios and their data: additive changes and appended entries ---

    def test_an_appended_step_or_added_expectation_field_passes(self):
        self.assert_passes(lambda repo: repo.write('Conformance/Scenarios/start.json', (
            '{"name":"start","steps":[{"send":{"type":"start"}},'
            '{"expect":{"state":{"takeoverVisible":false,"revision":1}}},{"expect":{"noMoreMessagesFor":0.2}}]}\n')))

    def test_a_changed_scenario_expectation_fails(self):
        self.assert_breaks(lambda repo: repo.write('Conformance/Scenarios/start.json',
                                                   SCENARIO.replace('"takeoverVisible":false', '"takeoverVisible":true')),
                           'Conformance/Scenarios/start.json: modified')

    def test_a_removed_scenario_step_fails(self):
        self.assert_breaks(lambda repo: repo.write('Conformance/Scenarios/start.json',
                                                   '{"steps":[{"send":{"type":"start"}}]}\n'),
                           'Conformance/Scenarios/start.json: modified')

    def test_an_inserted_scenario_step_fails(self):
        self.assert_breaks(lambda repo: repo.write('Conformance/Scenarios/start.json', (
            '{"steps":[{"send":{"type":"refresh"}},{"send":{"type":"start"}},'
            '{"expect":{"state":{"takeoverVisible":false}}}]}\n')), 'Conformance/Scenarios/start.json: modified')

    def test_changed_scenario_data_fails(self):
        self.assert_breaks(lambda repo: repo.write('Conformance/Scenarios/data/token.json', '{}\n'),
                           'Conformance/Scenarios/data/token.json: modified')

    def test_a_deleted_scenario_fails(self):
        self.assert_breaks(lambda repo: (repo.root / 'Conformance/Scenarios/start.json').unlink(),
                           'Conformance/Scenarios/start.json: deleted')

    def test_a_scenario_that_is_no_longer_json_fails(self):
        self.assert_breaks(lambda repo: repo.write('Conformance/Scenarios/start.json', 'not json\n'),
                           'Conformance/Scenarios/start.json: modified (not JSON')

    # --- persisted-data fixtures: never changed ---

    def test_a_field_added_to_a_persisted_fixture_fails(self):
        self.assert_breaks(
            lambda repo: repo.write('Tests/TimeTugEngineTests/Fixtures/settings-legacy.json', '{"leadTime":60,"x":1}\n'),
            'Tests/TimeTugEngineTests/Fixtures/settings-legacy.json: modified')

    def test_a_changed_persisted_fixture_fails(self):
        self.assert_breaks(
            lambda repo: repo.write('Tests/TimeTugEngineTests/Fixtures/settings-legacy.json', '{"leadTime":30}\n'),
            'Tests/TimeTugEngineTests/Fixtures/settings-legacy.json: modified')

    def test_a_renamed_fixture_fails_as_a_deletion(self):
        self.assert_breaks(lambda repo: repo.git('mv', 'Conformance/Fixtures/golden/ready.json',
                                                 'Conformance/Fixtures/golden/ready2.json'),
                           'Conformance/Fixtures/golden/ready.json: deleted')

    def test_usage_errors(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            self.assertEqual(repo.compat('frozen', '1.0.0').returncode, 2)
            self.assertEqual(repo.compat('frozen', '1.0.0', '--loose', 'x').returncode, 2)

    def test_only_additions_are_listed_as_unchanged(self):
        self.assertEqual(compat.changed_baseline_files('A\ta.json\nM\tb.json\nD\tc.json\n'), ['M\tb.json', 'D\tc.json'])


class Covers(unittest.TestCase):
    BASE = {'a': 1, 'b': 'x', 'c': True, 'd': None, 'list': [{'e': 1.5}], 'o': {'f': [1, 2]}}

    def test_added_fields_pass(self):
        current = {**self.BASE, 'new': 1, 'list': [{'e': 1.5, 'g': 'y'}], 'o': {'f': [1, 2], 'h': {}}}
        self.assertTrue(compat.covers(current, self.BASE))
        self.assertTrue(compat.covers({**self.BASE, 'a': 1.0}, self.BASE))  # the same JSON number

    def test_removed_retyped_or_changed_fields_fail(self):
        for key, value in (('a', '1'), ('a', True), ('a', 2), ('b', 'y'), ('c', 1), ('d', 0), ('d', False),
                           ('list', {'e': 1.5}), ('o', {'f': [2, 1]}), ('o', {'f': [1]})):
            with self.subTest(key=key, value=value):
                self.assertFalse(compat.covers({**self.BASE, key: value}, self.BASE))
        without = dict(self.BASE)
        del without['b']
        self.assertFalse(compat.covers(without, self.BASE))

    def test_arrays_grow_only_when_appending_is_allowed(self):
        self.assertFalse(compat.covers([1, 2, 3], [1, 2]))
        self.assertTrue(compat.covers([1, 2, 3], [1, 2], append=True))
        self.assertFalse(compat.covers([0, 1, 2], [1, 2], append=True))
        self.assertFalse(compat.covers([1], [1, 2], append=True))


class Header(unittest.TestCase):
    def test_declarations(self):
        found = compat.declarations(HEADER)
        self.assertEqual(sorted(found), ['tt_engine', 'tt_engine_create', 'tt_engine_destroy', 'tt_engine_reply',
                                         'tt_engine_send', 'tt_message_fn'])
        self.assertEqual(found['tt_engine_send'], 'TT_EXPORT void tt_engine_send(tt_engine*engine,const char*command_json)')
        self.assertEqual(compat.exported_functions(HEADER),
                         ['tt_engine_create', 'tt_engine_destroy', 'tt_engine_reply', 'tt_engine_send'])

    def test_comments_spacing_and_additions_pass(self):
        current = (HEADER.replace('/* the C interface */', '/* reworded */')
                         .replace('void tt_engine_destroy(tt_engine *engine);', 'void  tt_engine_destroy( tt_engine* engine );')
                         .replace('#endif\n#endif', '#endif\n#endif\nTT_EXPORT int tt_engine_new(void);'))
        self.assertEqual(compat.header_breaks(HEADER, current), [])

    def test_a_changed_signature_fails(self):
        problems = compat.header_breaks(HEADER, HEADER.replace('uint64_t request_id', 'uint32_t request_id'))
        self.assertEqual(len(problems), 1)
        self.assertIn('tt_engine_reply changed', problems[0])

    def test_a_removed_function_or_retyped_callback_fails(self):
        removed = HEADER.replace('TT_EXPORT void tt_engine_destroy(tt_engine *engine);\n', '')
        self.assertIn('tt_engine_destroy was removed', compat.header_breaks(HEADER, removed)[0])
        retyped = HEADER.replace('typedef void (*tt_message_fn)(void *context,', 'typedef int (*tt_message_fn)(void *context,')
        self.assertIn('tt_message_fn changed', compat.header_breaks(HEADER, retyped)[0])

    def test_the_command_compares_with_the_baseline_tag(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            self.assertEqual(repo.compat('header', '1.0.0', 'include/timetug_engine.h').returncode, 0)
            repo.write('include/timetug_engine.h', HEADER.replace('const char *command_json', 'const char *json'))
            result = repo.compat('header', '1.0.0', 'include/timetug_engine.h')
            self.assertEqual(result.returncode, 1)
            self.assertIn('tt_engine_send changed', result.stdout)

    def test_the_real_header_has_four_exported_functions(self):
        self.assertEqual(compat.exported_functions((ROOT / 'include' / 'timetug_engine.h').read_text()),
                         ['tt_engine_create', 'tt_engine_destroy', 'tt_engine_reply', 'tt_engine_send'])


class Exports(unittest.TestCase):
    def test_nm_output(self):
        linux = '0000000000012340 T tt_engine_create\n                 U malloc\n0000000000012350 T tt_engine_send\n'
        self.assertEqual(compat.symbol_names(linux, darwin=False), {'tt_engine_create', 'malloc', 'tt_engine_send'})
        darwin = '0000000000001000 T _tt_engine_create\n'
        self.assertEqual(compat.symbol_names(darwin, darwin=True), {'tt_engine_create'})

    def test_a_library_missing_a_baseline_function_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            bin_dir = repo.root / 'bin'
            bin_dir.mkdir()
            nm = bin_dir / 'nm'
            nm.write_text('#!/usr/bin/env bash\ncat "$NM_OUTPUT"\n')
            nm.chmod(0o755)
            symbols = repo.root / 'symbols.txt'
            env = {'PATH': f'{bin_dir}{os.pathsep}{os.environ["PATH"]}', 'NM_OUTPUT': str(symbols)}
            names = ['tt_engine_create', 'tt_engine_send', 'tt_engine_reply', 'tt_engine_destroy']
            symbols.write_text(''.join(f'0000 T {name}\n' for name in names))
            self.assertEqual(repo.compat('exports', '1.0.0', 'include/timetug_engine.h', 'lib.so', env=env).returncode, 0)
            symbols.write_text(''.join(f'0000 T {name}\n' for name in names if name != 'tt_engine_reply'))
            result = repo.compat('exports', '1.0.0', 'include/timetug_engine.h', 'lib.so', env=env)
            self.assertEqual(result.returncode, 1)
            self.assertIn('no longer exports tt_engine_reply', result.stdout)


class NuGet(unittest.TestCase):
    def test_latest_published_release(self):
        with tempfile.TemporaryDirectory() as directory:
            index = Path(directory) / 'index.json'
            index.write_text(json.dumps({'versions': ['1.5.0', '1.6.2', '0.0.0-dryrun.4', '1.6.10-beta']}))
            self.assertEqual(compat.nuget_baseline(index.as_uri()), '1.6.2')
            output = Path(directory) / 'output'
            output.write_text('')
            result = subprocess.run([sys.executable, str(COMPAT), 'nuget-baseline', '--index', index.as_uri()],
                                    capture_output=True, text=True, env={**os.environ, 'GITHUB_OUTPUT': str(output)})
            self.assertEqual(result.returncode, 0)
            self.assertEqual(output.read_text(), 'version=1.6.2\n')

    def test_an_unreachable_index_or_none_published_skips(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertIsNone(compat.nuget_baseline((Path(directory) / 'missing.json').as_uri()))
            empty = Path(directory) / 'empty.json'
            empty.write_text('{"versions": []}')
            self.assertIsNone(compat.nuget_baseline(empty.as_uri()))


class ExportBaseline(unittest.TestCase):
    def test_exports_the_baseline_conformance_files(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Repo(directory)
            # The script exports from the repository it lives in; copy it into the throwaway one.
            target = repo.root / 'scripts' / 'ci' / 'compat' / 'export-baseline.sh'
            target.parent.mkdir(parents=True)
            target.write_text(EXPORT.read_text())
            target.chmod(0o755)
            repo.write('Conformance/Scenarios/start.json', '{"steps":["changed"]}\n')
            repo.commit('change after the baseline')
            out = repo.root / 'out'
            subprocess.run([str(target), '1.0.0', str(out)], check=True, capture_output=True, text=True)
            self.assertEqual((out / 'golden' / 'ready.json').read_text(), '{"v":1,"type":"ready"}\n')
            self.assertEqual((out / 'scenarios' / 'start.json').read_text(), SCENARIO)
            self.assertTrue((out / 'scenarios' / 'data' / 'token.json').is_file())


def workflow():
    return yaml.safe_load((ROOT / '.github' / 'workflows' / 'ci.yml').read_text())


def skipped_when_gate_is_off(step):
    condition = str(step.get('if', ''))
    return "steps.gate.outputs.run == 'true'" in condition or "steps.nuget.outputs.version != ''" in condition


class Workflow(unittest.TestCase):
    def test_label_changes_rerun_ci(self):
        triggers = workflow().get('on', workflow().get(True))
        self.assertTrue({'labeled', 'unlabeled', 'opened', 'synchronize', 'reopened'}
                        <= set(triggers['pull_request']['types']))

    def test_the_compat_jobs_check_out_the_history_and_gate_first(self):
        for job in ('compat', 'compat-dotnet'):
            steps = workflow()['jobs'][job]['steps']
            self.assertEqual(steps[0]['with']['fetch-depth'], 0, job)
            self.assertEqual(steps[1]['id'], 'gate', job)
            self.assertIn('compat.py gate', steps[1]['run'])
            for step in steps[2:]:
                if 'uses' not in step:
                    self.assertTrue(skipped_when_gate_is_off(step), f'{job}: {step.get("name")}')

    def test_the_compat_job_runs_every_check(self):
        runs = '\n'.join(step.get('run', '') for step in workflow()['jobs']['compat']['steps'])
        self.assertIn('compat.py frozen "$BASELINE"', runs)
        self.assertIn('--additive Conformance/Fixtures/golden --appendable Conformance/Scenarios', runs)
        self.assertIn('--strict Tests/TimeTugEngineTests/Fixtures', runs)
        self.assertIn('compat.py header "$BASELINE" include/timetug_engine.h', runs)
        self.assertIn('diagnose-api-breaking-changes "$BASELINE"', runs)
        for product in ('TimeTugCore', 'CalendarBridge', 'TimeTugEngine', 'TimeTugEngineProtocol', 'TimeTugEngineC'):
            self.assertIn(f' {product}', runs)
        self.assertIn('TT_BASELINE_FIXTURES=', runs)
        self.assertIn('TT_BASELINE_SCENARIOS=', runs)
        self.assertIn('compat.py exports "$BASELINE"', runs)

    def test_every_library_product_is_compared(self):
        package = (ROOT / 'Package.swift').read_text()
        runs = '\n'.join(step.get('run', '') for step in workflow()['jobs']['compat']['steps'])
        import re
        for product in re.findall(r'\.library\(name: "(\w+)"', package):
            self.assertIn(f' {product}', runs.split('diagnose-api-breaking-changes', 1)[1], product)

    def test_the_dotnet_job_validates_against_the_published_package(self):
        runs = '\n'.join(step.get('run', '') for step in workflow()['jobs']['compat-dotnet']['steps'])
        self.assertIn('compat.py nuget-baseline', runs)
        self.assertIn('-p:TimeTugPackageValidationBaseline=$VERSION', runs)
        project = (ROOT / 'dotnet' / 'TimeTug.Engine' / 'TimeTug.Engine.csproj').read_text()
        self.assertIn('<EnablePackageValidation>true</EnablePackageValidation>', project)
        self.assertIn('<PackageValidationBaselineVersion>$(TimeTugPackageValidationBaseline)</PackageValidationBaselineVersion>',
                      project)

    def test_the_rule_is_documented(self):
        agents = (ROOT / 'AGENTS.md').read_text()
        self.assertIn('## Backward compatibility', agents)
        self.assertIn('semver:major', agents)


if __name__ == '__main__':
    unittest.main()
