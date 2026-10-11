import importlib.util
import io
import os
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError, URLError


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("release_meta", ROOT / "support/ci/release-meta.py")
release_meta = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release_meta)


class ReleaseMetaTests(unittest.TestCase):
    def test_increases_patch_by_workflow_run_number(self):
        self.assertEqual(release_meta.release_version("1.2.0", 1), "1.2.1")
        self.assertEqual(release_meta.release_version("1.2.0", 13), "1.2.13")
        self.assertEqual(release_meta.release_version("1.3.0", 13), "1.3.13")

    def test_stamps_only_versioned_workspace_packages_and_appstream(self):
        names = sorted(release_meta.PACKAGES)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "data").mkdir()
            (root / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.0"\n')
            records = [f'[[package]]\nname = "{name}"\nversion = "1.2.0"\n' for name in names]
            records += [
                '[[package]]\nname = "mc-graph-core"\nversion = "0.1.0"\n',
                '[[package]]\nname = "magpie-types"\nversion = "1.2.0"\n',
                '[[package]]\nname = "serde"\nversion = "1.0.0"\n',
            ]
            (root / "Cargo.lock").write_text("version = 4\n\n" + "\n".join(records))
            metainfo = root / "data/io.github.taskmanager.TaskManager.metainfo.xml.in"
            metainfo.write_text('<component>\n    <releases>\n        <release version="1.2.0" date="2026-10-10" />\n    </releases>\n</component>\n')

            release_meta.stamp(root, "1.2.7", "2026-10-11")
            release_meta.stamp(root, "1.2.7", "2026-10-11")

            self.assertIn('version = "1.2.7"', (root / "Cargo.toml").read_text())
            lock = (root / "Cargo.lock").read_text()
            self.assertEqual(lock.count('version = "1.2.7"'), len(names))
            self.assertIn('name = "mc-graph-core"\nversion = "0.1.0"', lock)
            self.assertIn('name = "magpie-types"\nversion = "1.2.0"', lock)
            self.assertIn('name = "serde"\nversion = "1.0.0"', lock)
            self.assertEqual(metainfo.read_text().count('version="1.2.7"'), 1)
            self.assertIn('version="1.2.0"', metainfo.read_text())
            ET.parse(metainfo)


class ReleaseEligibilityTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "main.rs").write_text("fn main() {}\n")
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "1.2.0"\n')
        (self.root / "Cargo.lock").write_text("\n".join(
            f'[[package]]\nname = "{name}"\nversion = "1.2.0"\n'
            for name in sorted(release_meta.PACKAGES)
        ))
        (self.root / "data").mkdir()
        (self.root / "data/io.github.taskmanager.TaskManager.metainfo.xml.in").write_text(
            '<component>\n    <releases>\n    </releases>\n</component>\n'
        )
        self.git("init", "--quiet")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "published")
        self.git("tag", "v1.2.0")

    def git(self, *args):
        return subprocess.run(
            ["git", "-C", str(self.root), *args], check=True, capture_output=True, text=True,
            env={**os.environ, "GIT_AUTHOR_NAME": "CI test", "GIT_AUTHOR_EMAIL": "ci@example.invalid",
                 "GIT_COMMITTER_NAME": "CI test", "GIT_COMMITTER_EMAIL": "ci@example.invalid"},
        ).stdout.strip()

    def test_docs_ci_and_worktree_stamping_do_not_create_a_release(self):
        for path in ("README.md", "AGENTS.md", ".github/workflows/native-release.yml"):
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("documentation or CI change\n")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "docs and CI")
        release_meta.stamp(self.root, "1.2.99", "2026-10-11")
        self.assertFalse(release_meta.rust_changes(self.root, "refs/tags/v1.2.0", "HEAD"))

    def test_unpublished_rust_survives_later_ci_fix_and_revert_needs_no_release(self):
        (self.root / "main.rs").write_text("fn main() { println!(\"new\"); }\n")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Rust change")
        (self.root / "README.md").write_text("CI fix\n")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "later CI fix")
        self.assertTrue(release_meta.rust_changes(self.root, "refs/tags/v1.2.0", "HEAD"))
        self.git("tag", "v1.2.1")
        self.assertFalse(release_meta.rust_changes(self.root, "refs/tags/v1.2.1", "HEAD"))
        (self.root / "main.rs").write_text("fn main() {}\n")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "revert Rust")
        self.assertFalse(release_meta.rust_changes(self.root, "refs/tags/v1.2.0", "HEAD"))

    def test_cargo_inputs_and_submodule_revision_count(self):
        for path in ("Cargo.toml", "subprojects/magpie/Cargo.lock"):
            base = self.git("rev-parse", "HEAD")
            target = self.root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("changed Rust build input\n")
            self.git("add", ".")
            self.git("commit", "--quiet", "-m", "Cargo change")
            self.assertTrue(release_meta.rust_changes(self.root, base, "HEAD"))
        base = self.git("rev-parse", "HEAD")
        self.git("update-index", "--add", "--cacheinfo",
                 f"160000,{base},subprojects/magpie/platform-linux/crates/app-rummage")
        self.git("commit", "--quiet", "-m", "pinned Rust dependency")
        self.assertTrue(release_meta.rust_changes(self.root, base, "HEAD"))

    def test_removed_rust_path_counts_even_when_renamed_to_markdown(self):
        (self.root / "main.rs").rename(self.root / "README.md")
        self.git("add", "-A")
        self.git("commit", "--quiet", "-m", "rename Rust to docs")
        self.assertTrue(release_meta.rust_changes(self.root, "refs/tags/v1.2.0", "HEAD"))

    def test_first_release_and_missing_git_baseline(self):
        self.assertTrue(release_meta.rust_changes(self.root, None, "HEAD"))
        with self.assertRaises(subprocess.CalledProcessError):
            release_meta.rust_changes(self.root, "refs/tags/missing", "HEAD")

    def test_latest_release_lookup_distinguishes_absence_from_errors(self):
        with patch.object(release_meta, "urlopen", return_value=io.BytesIO(b'{"tag_name":"v1.2.0"}')):
            self.assertEqual(release_meta.latest_release_tag("test/repo", "https://api.github.com", "token"), "v1.2.0")
        with patch.object(release_meta, "urlopen", side_effect=HTTPError("url", 404, "missing", {}, None)):
            self.assertEqual(release_meta.latest_release_tag("test/repo", "https://api.github.com", "token"), "")
        with patch.object(release_meta, "urlopen", side_effect=HTTPError("url", 403, "denied", {}, None)):
            with self.assertRaises(HTTPError):
                release_meta.latest_release_tag("test/repo", "https://api.github.com", "token")
        with patch.object(release_meta, "urlopen", side_effect=URLError("network failed")):
            with self.assertRaises(URLError):
                release_meta.latest_release_tag("test/repo", "https://api.github.com", "token")

    def test_eligibility_uses_the_published_tag(self):
        with patch.dict(os.environ, {"GITHUB_REPOSITORY": "test/repo", "GITHUB_API_URL": "https://api.github.com", "GH_TOKEN": "token"}):
            with patch.object(release_meta, "latest_release_tag", return_value="v1.2.0"):
                self.assertEqual(release_meta.eligibility(self.root, "HEAD", "1.2.1"), (False, "v1.2.0"))
                output = self.root / "github-output"
                with patch.object(sys, "argv", ["release-meta.py", "eligibility", "--root", str(self.root),
                                              "--head", "HEAD", "--version", "1.2.1", "--output", str(output)]):
                    with patch.object(sys, "stderr", io.StringIO()):
                        release_meta.main()
                self.assertEqual(output.read_text(), "release_needed=false\nlatest_tag=v1.2.0\n")

    def test_older_run_does_not_release_against_a_newer_rust_baseline(self):
        old_head = self.git("rev-parse", "HEAD")
        (self.root / "main.rs").write_text("fn main() { println!(\"newer\"); }\n")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "newer Rust")
        self.git("tag", "v1.2.2")
        self.assertTrue(release_meta.rust_changes(self.root, "refs/tags/v1.2.2", old_head))
        with patch.dict(os.environ, {"GITHUB_REPOSITORY": "test/repo", "GITHUB_API_URL": "https://api.github.com", "GH_TOKEN": "token"}):
            with patch.object(release_meta, "latest_release_tag", return_value="v1.2.2"):
                self.assertEqual(release_meta.eligibility(self.root, old_head, "1.2.1"), (False, "v1.2.2"))


if __name__ == "__main__":
    unittest.main()
