import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("release_notes", ROOT / "support/ci/release-notes.py")
release_notes = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release_notes)


class ReleaseNotesTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "main.rs").write_text("fn main() {}\n")
        self.git("init", "--quiet")
        self.git("config", "user.name", "CI test")
        self.git("config", "user.email", "ci@example.invalid")

    def git(self, *args):
        return subprocess.run(
            ["git", "-C", str(self.root), *args], check=True, capture_output=True, text=True,
        ).stdout.strip()

    def commit(self, message):
        self.counter = getattr(self, "counter", 0) + 1
        (self.root / "main.rs").write_text(f"fn main() {{ // {self.counter}\n}}\n")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", message)
        return self.git("rev-parse", "HEAD")

    def test_lists_commits_since_previous_release_with_links(self):
        self.commit("published baseline")
        self.git("tag", "v1.2.14")
        first = self.commit("fix(research): publish addon-compatible categories")
        second = self.commit("chore(opencode): fall back to default subagent models")
        head = self.git("rev-parse", "HEAD")

        content = release_notes.notes(self.root, "v1.2.14", "HEAD", "https://github.com/test/repo")

        self.assertIn("**Previous release:** `v1.2.14`", content)
        self.assertIn("**New commits:** 2", content)
        self.assertLess(content.index("fix(research)"), content.index("chore(opencode)"))
        self.assertIn(f"- [`{first[:7]}`](https://github.com/test/repo/commit/{first}) fix(research)", content)
        self.assertIn(f"- [`{second[:7]}`](https://github.com/test/repo/commit/{second}) chore(opencode)", content)
        self.assertIn(f"[Full changelog](https://github.com/test/repo/compare/v1.2.14...{head})", content)

    def test_without_previous_tag_covers_entire_history(self):
        self.commit("first commit")
        second = self.commit("second commit")

        content = release_notes.notes(self.root, "", "HEAD", "https://github.com/test/repo")

        self.assertIn("**Range:** entire commit history", content)
        self.assertIn("**New commits:** 2", content)
        self.assertIn(f"- [`{second[:7]}`]", content)
        self.assertNotIn("Previous release", content)
        self.assertNotIn("Full changelog", content)

    def test_without_repo_url_renders_plain_short_shas(self):
        self.commit("published baseline")
        self.git("tag", "v1.2.14")
        head = self.commit("plain entry")

        content = release_notes.notes(self.root, "v1.2.14", "HEAD", "")

        self.assertIn(f"- `{head[:7]}` plain entry", content)
        self.assertNotIn("](http", content)
        self.assertNotIn("Full changelog", content)

    def test_missing_tag_fails_instead_of_dropping_commits(self):
        self.commit("only commit")
        with self.assertRaises(subprocess.CalledProcessError):
            release_notes.notes(self.root, "v9.9.9", "HEAD", "https://github.com/test/repo")


if __name__ == "__main__":
    unittest.main()
