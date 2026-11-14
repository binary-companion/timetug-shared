import subprocess, sys, tempfile, unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'cargo-dependency-version.py'
ROOT = Path(__file__).resolve().parents[3]

SHA = 'ea8a9e2fdb29749a3a041ef381eaa1c9aed7f027'


def package(name, query, repository='binary-companion/calendar-connectors', suffix=''):
    return (f'[[package]]\nname = "{name}"\nversion = "0.0.0"\n'
            f'source = "git+https://github.com/{repository}{suffix}?{query}#{SHA}"\n\n')


def lock(*blocks):
    return 'version = 4\n\n[[package]]\nname = "serde"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n\n' + ''.join(blocks)


def run(text, repository='calendar-connectors'):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'Cargo.lock'
        path.write_text(text, encoding='utf-8')
        return subprocess.run([sys.executable, str(SCRIPT), str(path), repository], capture_output=True, text=True)


class CargoDependencyVersionTests(unittest.TestCase):
    def test_prints_the_pinned_tag(self):
        result = run(lock(package('calendar-core', 'tag=2.0.0-alpha.1'), package('calendar-oauth', 'tag=2.0.0-alpha.1')))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '2.0.0-alpha.1\n')

    def test_a_release_tag_and_a_dot_git_url(self):
        result = run(lock(package('calendar-core', 'tag=2.0.0', suffix='.git')))
        self.assertEqual(result.stdout, '2.0.0\n')

    def test_other_repositories_are_ignored(self):
        result = run(lock(package('calendar-core', 'tag=2.0.0'), package('other', 'rev=abc', 'someone/not-calendar-connectors')))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '2.0.0\n')

    def test_a_revision_pin_has_no_version(self):
        result = run(lock(package('calendar-core', f'rev={SHA}')))
        self.assertEqual(result.returncode, 1)
        self.assertIn('not pinned by tag', result.stderr)
        self.assertEqual(result.stdout, '')

    def test_a_branch_pin_has_no_version(self):
        result = run(lock(package('calendar-core', 'branch=v2')))
        self.assertEqual(result.returncode, 1)
        self.assertIn('not pinned by tag', result.stderr)

    def test_one_package_off_the_tag_fails(self):
        result = run(lock(package('calendar-core', 'tag=2.0.0'), package('calendar-oauth', f'rev={SHA}')))
        self.assertEqual(result.returncode, 1)
        self.assertIn('calendar-oauth', result.stderr)

    def test_different_tags_fail(self):
        result = run(lock(package('calendar-core', 'tag=2.0.0'), package('calendar-oauth', 'tag=2.0.1')))
        self.assertEqual(result.returncode, 1)
        self.assertIn('different tags', result.stderr)

    def test_no_package_of_the_repository_fails(self):
        result = run(lock())
        self.assertEqual(result.returncode, 1)
        self.assertIn('no package of calendar-connectors', result.stderr)

    def test_a_missing_file_fails(self):
        result = subprocess.run([sys.executable, str(SCRIPT), '/nonexistent/Cargo.lock', 'calendar-connectors'],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn('cannot read', result.stderr)

    def test_usage(self):
        result = subprocess.run([sys.executable, str(SCRIPT)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)

    def test_the_real_lock_names_the_connectors(self):
        # Until the connector bump (phase 3 task 11) the workspace pins a revision; afterwards a tag. Either way the
        # script must answer, with a tag or with the reason there is none.
        result = subprocess.run([sys.executable, str(SCRIPT), str(ROOT / 'Cargo.lock'), 'calendar-connectors'],
                                capture_output=True, text=True)
        self.assertIn(result.returncode, (0, 1))
        self.assertNotIn('no package of', result.stderr)


if __name__ == '__main__':
    unittest.main()
