"""Tests of scripts/ci/check-third-party-notices.py (run by unittest discovery in the rust-lint job)."""
import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'check-third-party-notices.py'
spec = importlib.util.spec_from_file_location('check_third_party_notices', SCRIPT)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)

TEXT = 'Permission is hereby granted, free of charge, to any person obtaining a copy of this software.\n' * 2
TREE = '''timetug-engine-c v0.0.0 (/work/crates/timetug-engine-c)
serde v1.0.229
serde_core v1.0.229
serde_derive v1.0.229 (proc-macro)
serde v1.0.229 (*)
calendar-core v0.0.0 (https://github.com/binary-companion/calendar-connectors?rev=ea8a9e2f#ea8a9e2f)
timetug-core v0.0.0 (C:\\work\\crates\\timetug-core)
'''


def section(name, version, license_line='License: MIT OR Apache-2.0', text=TEXT):
    return f'## {name} {version}\n\n{license_line}\n\n```\n{text}```\n\n'


def notices(*sections):
    return '# Third-party notices\n\nPreamble.\n\n' + ''.join(sections)


COMPLETE = notices(section('serde', '1.0.229'), section('serde_core', '1.0.229'),
                   section('serde_derive', '1.0.229'), section('calendar-core', '0.0.0'))


class TreeTests(unittest.TestCase):
    def test_workspace_crates_and_repeats_are_not_listed(self):
        self.assertEqual(checker.tree_crates(TREE), {('serde', '1.0.229'), ('serde_core', '1.0.229'),
                                                     ('serde_derive', '1.0.229'), ('calendar-core', '0.0.0')})


class ProblemsTests(unittest.TestCase):
    def test_a_complete_file_passes(self):
        self.assertEqual(checker.problems(TREE, COMPLETE), [])

    def test_a_missing_crate_is_reported(self):
        text = notices(section('serde', '1.0.229'), section('serde_core', '1.0.229'), section('calendar-core', '0.0.0'))
        self.assertEqual(checker.problems(TREE, text), ['serde_derive 1.0.229 is in the dependency tree but has no section'])

    def test_a_crate_that_left_the_tree_is_reported(self):
        text = COMPLETE + section('left-pad', '1.0.0')
        self.assertEqual(checker.problems(TREE, text), ['left-pad 1.0.0 is listed but is not in the dependency tree any more'])

    def test_a_bumped_version_is_reported_both_ways(self):
        text = COMPLETE.replace('## serde 1.0.229', '## serde 1.0.228')
        self.assertEqual(sorted(checker.problems(TREE, text)), [
            'serde 1.0.228 is listed but is not in the dependency tree any more',
            'serde 1.0.229 is in the dependency tree but has no section'])

    def test_a_duplicate_section_is_reported(self):
        self.assertEqual(checker.problems(TREE, COMPLETE + section('serde', '1.0.229')), ['serde 1.0.229 appears twice'])

    def test_a_missing_licence_line_is_reported(self):
        text = COMPLETE.replace('License: MIT OR Apache-2.0', 'Licence text follows', 1)
        self.assertEqual(checker.problems(TREE, text), ['serde 1.0.229 has no "License:" line'])

    def test_a_missing_or_empty_licence_text_is_reported(self):
        for body in ('', 'short\n'):
            with self.subTest(body=body):
                text = notices(section('serde', '1.0.229', text=body), section('serde_core', '1.0.229'),
                               section('serde_derive', '1.0.229'), section('calendar-core', '0.0.0'))
                self.assertEqual(checker.problems(TREE, text), ['serde 1.0.229 has no licence text'])

    def test_text_outside_a_fence_is_not_licence_text(self):
        unfenced = '## serde 1.0.229\n\nLicense: MIT\n\n' + TEXT + '\n'
        text = notices(unfenced, section('serde_core', '1.0.229'), section('serde_derive', '1.0.229'),
                       section('calendar-core', '0.0.0'))
        self.assertEqual(checker.problems(TREE, text), ['serde 1.0.229 has no licence text'])

    def test_an_empty_tree_is_reported(self):
        self.assertIn('the dependency tree lists no crates (is the cargo tree output empty?)',
                      checker.problems('', notices()))


class MainTests(unittest.TestCase):
    def run_main(self, tree, text):
        with tempfile.TemporaryDirectory() as directory:
            tree_file = Path(directory) / 'tree.txt'
            notices_file = Path(directory) / 'N.md'
            tree_file.write_text(tree)
            notices_file.write_text(text)
            return checker.main(['--notices', str(notices_file), '--tree-file', str(tree_file)])

    def test_exit_codes(self):
        self.assertEqual(self.run_main(TREE, COMPLETE), 0)
        self.assertEqual(self.run_main(TREE, notices()), 1)

    def test_an_unreadable_file_is_an_error(self):
        self.assertEqual(checker.main(['--notices', '/nonexistent/N.md', '--tree-file', '/nonexistent/t.txt']), 1)


if __name__ == '__main__':
    unittest.main()
