"""Pins the shape of the NuGet jobs of .github/workflows/release.yml and runs their shell steps.

The steps are taken from the workflow and executed, so the real scripts are what is tested: the version choice
(release tag, dry-run input, run number) and the notice when trusted publishing is not configured."""
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[3]
WORKFLOW = ROOT / '.github' / 'workflows' / 'release.yml'
PINNED = re.compile(r'^[\w.-]+/[\w.-]+@[0-9a-f]{40}$')


def load():
    data = yaml.safe_load(WORKFLOW.read_text(encoding='utf-8'))
    # PyYAML reads the key `on` as True.
    return data, data.get('on', data.get(True))


def step(job, name):
    found = [s for s in job['steps'] if s.get('name') == name]
    assert len(found) == 1, f'expected one step named {name!r}'
    return found[0]


def run_step(script, env):
    with tempfile.TemporaryDirectory() as directory:
        output = Path(directory) / 'output'
        output.write_text('')
        environment = {'PATH': os.environ['PATH'], 'GITHUB_OUTPUT': str(output), **env}
        result = subprocess.run(['bash', '-c', script], capture_output=True, text=True, env=environment)
        return result, dict(line.split('=', 1) for line in output.read_text().splitlines() if '=' in line)


class ReleaseNugetJobsTests(unittest.TestCase):
    def setUp(self):
        self.workflow, self.on = load()
        self.jobs = self.workflow['jobs']

    def test_dispatch_has_a_publish_switch_that_defaults_to_true(self):
        publish = self.on['workflow_dispatch']['inputs']['publish']
        self.assertEqual(publish['type'], 'boolean')
        self.assertIs(publish['default'], True)

    def test_every_action_is_pinned_to_a_commit(self):
        for job_id in ('nuget-native', 'nuget-pack', 'publish-nuget'):
            for s in self.jobs[job_id]['steps']:
                if 'uses' in s:
                    self.assertRegex(s['uses'], PINNED, f'{job_id}: {s["uses"]}')

    def test_native_sets_are_built_on_both_windows_architectures(self):
        job = self.jobs['nuget-native']
        include = job['strategy']['matrix']['include']
        self.assertEqual({(i['runner'], i['rid']) for i in include},
                         {('windows-latest', 'win-x64'), ('windows-11-arm', 'win-arm64')})
        names = [s.get('name', s.get('uses', '')) for s in job['steps']]
        stamp = names.index('Stamp the version into the engine')
        build = names.index('Build the engine library')
        self.assertLess(stamp, build, 'the version must be stamped before the build')
        self.assertIn('stamp-engine-version.py', step(job, 'Stamp the version into the engine')['run'])
        self.assertIn('assemble-native.ps1', step(job, 'Assemble the native set and check its exports')['run'])
        self.assertIn('run-dotnet-tests.ps1', step(job, 'C# tests against the native set')['run'])

    def test_the_package_is_checked_before_it_is_uploaded(self):
        job = self.jobs['nuget-pack']
        self.assertNotIn('environment', job)
        self.assertEqual(job['permissions'], {'contents': 'read'})
        names = [s.get('name') or s.get('uses', '').split('@')[0] for s in job['steps']]
        check = names.index('Check the package')
        upload = names.index('actions/upload-artifact')
        self.assertLess(names.index('C# tests on this architecture'), names.index('Pack'))
        self.assertLess(check, upload)
        self.assertIn('test_pack_inventory.py', step(job, 'Check the package')['run'])
        self.assertIn('-p:RequireNativeAssets=true', step(job, 'Pack')['run'])
        upload_step = job['steps'][upload]
        self.assertEqual(upload_step['with']['if-no-files-found'], 'error')
        self.assertNotIn('if', upload_step, 'the package is uploaded whether or not it is pushed')

    def test_publish_nuget_uses_trusted_publishing_from_the_publish_environment(self):
        job = self.jobs['publish-nuget']
        self.assertEqual(job['environment'], 'publish')
        self.assertEqual(job['permissions'], {'contents': 'read', 'id-token': 'write'})
        self.assertIn("needs.publish.outputs.version != ''", job['if'], 'pushes only for a released version')
        text = yaml.safe_dump(job)
        self.assertNotIn('secrets.', text, 'no stored key')
        login = step(job, 'Log in to nuget.org (OIDC)')
        self.assertTrue(login['uses'].startswith('NuGet/login@'))
        self.assertEqual(login['with'], {'user': '${{ vars.NUGET_USER }}'})
        push = step(job, 'Push')
        self.assertIn('--skip-duplicate', push['run'])
        self.assertIn('https://api.nuget.org/v3/index.json', push['run'])
        self.assertNotIn('${{', push['run'])
        for s in job['steps'][1:]:
            self.assertEqual(s.get('if'), "steps.setup.outputs.configured == 'true'")

    def test_publish_nuget_skips_with_a_notice_when_not_configured(self):
        script = step(self.jobs['publish-nuget'], 'Check the trusted-publishing setup')['run']
        result, outputs = run_step(script, {'NUGET_USER': ''})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('::notice::nuget.org trusted publishing is not configured; package built and tested, not pushed',
                      result.stdout)
        self.assertEqual(outputs, {'configured': 'false'})
        result, outputs = run_step(script, {'NUGET_USER': 'binary-companion'})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(outputs, {'configured': 'true'})

    def test_the_version_is_the_release_or_the_dry_run_choice(self):
        script = step(self.jobs['nuget-version'], 'Choose the package version')['run']
        cases = [
            ({'RELEASED': '1.4.0', 'REQUESTED': '', 'RUN_NUMBER': '7'}, '1.4.0'),
            ({'RELEASED': '', 'REQUESTED': '1.5.0-rc.1', 'RUN_NUMBER': '7'}, '1.5.0-rc.1'),
            ({'RELEASED': '', 'REQUESTED': '', 'RUN_NUMBER': '7'}, '0.0.0-dryrun.7'),
        ]
        for env, expected in cases:
            with self.subTest(env=env):
                result, outputs = run_step(script, env)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(outputs, {'version': expected})
        result, _ = run_step(script, {'RELEASED': '', 'REQUESTED': 'v1;rm', 'RUN_NUMBER': '7'})
        self.assertEqual(result.returncode, 1)

    def test_a_dry_run_never_tags(self):
        self.assertIn("inputs.publish", self.jobs['publish']['if'])
        self.assertIn("!inputs.publish", self.jobs['nuget-version']['if'])


if __name__ == '__main__':
    unittest.main()
