"""Tests of release-tags.py: each test builds a throwaway git repository."""
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / 'release-tags.py'
ENV = {'GIT_AUTHOR_NAME': 't', 'GIT_AUTHOR_EMAIL': 't@example.com', 'GIT_COMMITTER_NAME': 't',
       'GIT_COMMITTER_EMAIL': 't@example.com', 'HOME': '/nonexistent', 'PATH': '/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin'}


class Repo:
    def __init__(self, directory):
        self.dir = directory
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'commit.gpgsign', 'false')
        self.n = 0

    def git(self, *args):
        return subprocess.run(['git', '-C', self.dir, *args], check=True, capture_output=True, text=True, env=ENV).stdout.strip()

    def commit(self, path=None, message=None):
        self.n += 1
        name = path or f'file{self.n}.txt'
        (Path(self.dir) / name).write_text(f'{self.n}\n')
        self.git('add', name)
        self.git('commit', '-q', '-m', message or f'c{self.n}')
        return self.git('rev-parse', 'HEAD')

    def run(self, *args):
        return subprocess.run(['python3', str(SCRIPT), *args], cwd=self.dir, capture_output=True, text=True, env=ENV)

    def out(self, *args):
        result = self.run(*args)
        assert result.returncode == 0, result.stderr
        return result.stdout.strip()


class ReleaseTagsTests(unittest.TestCase):
    def repo(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        return Repo(directory.name)

    def test_tags_on_the_branch_match_git_tag_merged(self):
        r = self.repo()
        c1 = r.commit(); r.git('tag', '1.0.0')
        c2 = r.commit(); r.git('tag', '1.10.0')
        r.commit(); r.git('tag', '1.2.0')
        head = r.commit()
        merged = r.git('tag', '--merged', head, '--sort=-v:refname').split()
        listed = [line.split()[0] for line in r.out('list', '--ref', head).splitlines()]
        self.assertEqual(listed, merged)
        self.assertEqual(listed[0], '1.10.0')
        self.assertEqual(r.out('latest', '--ref', head), '1.10.0')
        self.assertEqual(r.out('at', '--sha', c2), '1.10.0')
        self.assertEqual(r.out('at', '--sha', c1), '1.0.0')
        self.assertEqual(r.out('at', '--sha', head), '')

    def test_a_release_commit_belongs_through_its_parent(self):
        r = self.repo()
        r.commit()
        c1 = r.commit()
        r.git('checkout', '-q', '--detach', c1)
        rel = r.commit('Package.swift', 'release')
        r.git('tag', '2.1.0')
        r.git('checkout', '-q', 'main')
        c2 = r.commit()
        self.assertEqual(r.out('latest', '--ref', c2), '2.1.0')
        self.assertEqual(r.out('list', '--ref', c2), f'2.1.0 {rel} {c1}')
        self.assertEqual(r.out('at', '--sha', c1), '2.1.0')
        self.assertEqual(r.out('at', '--sha', c2), '')
        self.assertEqual(r.out('source', '--tag', '2.1.0'), c1)

    def test_a_side_branch_commit_or_a_merge_commit_does_not_belong(self):
        r = self.repo()
        base = r.commit()
        r.git('checkout', '-q', '-b', 'side')
        r.commit('other.txt'); r.git('tag', '3.0.0')
        r.git('checkout', '-q', 'main')
        c2 = r.commit()
        self.assertEqual(r.out('latest', '--ref', c2), '')
        # two parents, only Package.swift differs from the first parent: not a release commit
        r.git('checkout', '-q', '--detach', base)
        (Path(r.dir) / 'Package.swift').write_text('x\n')
        r.git('add', 'Package.swift')
        r.git('commit', '-q', '-m', 'p')
        r.git('merge', '-q', '--no-ff', '-m', 'merge', 'side')
        r.git('tag', '3.1.0')
        r.git('checkout', '-q', 'main')
        self.assertEqual(r.out('latest', '--ref', c2), '')

    def test_a_release_commit_belongs_to_its_own_branch_only(self):
        r = self.repo()
        r.commit()
        r.git('checkout', '-q', '-b', 'one-x')
        on_one = r.commit()
        r.git('checkout', '-q', '--detach', on_one)
        r.commit('Package.swift', 'release')
        r.git('tag', '1.5.0')
        r.git('checkout', '-q', 'main')
        on_main = r.commit()
        self.assertEqual(r.out('latest', '--ref', 'main'), '')
        self.assertEqual(r.out('latest', '--ref', 'one-x'), '1.5.0')
        self.assertEqual(r.out('latest', '--ref', on_main), '')

    def test_the_highest_version_wins_between_release_commits_and_branch_tags(self):
        r = self.repo()
        c1 = r.commit(); r.git('tag', '2.0.0')
        r.git('checkout', '-q', '--detach', c1)
        r.commit('Package.swift', 'release')
        r.git('tag', '2.1.0')
        r.git('checkout', '-q', 'main')
        c2 = r.commit()
        self.assertEqual([l.split()[0] for l in r.out('list', '--ref', c2).splitlines()], ['2.1.0', '2.0.0'])
        self.assertEqual(r.out('latest', '--ref', c2), '2.1.0')

    def test_prerelease_tags_are_ignored(self):
        r = self.repo()
        c1 = r.commit(); r.git('tag', '2.0.0-alpha.1')
        r.git('checkout', '-q', '--detach', c1)
        r.commit('Package.swift', 'release'); r.git('tag', '2.1.0-alpha.1')
        r.git('checkout', '-q', 'main')
        c2 = r.commit()
        self.assertEqual(r.out('latest', '--ref', c2), '')
        self.assertEqual(r.out('at', '--sha', c1), '')

    def test_a_missing_option_is_a_usage_error(self):
        r = self.repo(); r.commit()
        self.assertEqual(r.run('latest').returncode, 2)
        self.assertEqual(r.run().returncode, 2)


if __name__ == '__main__':
    unittest.main()
