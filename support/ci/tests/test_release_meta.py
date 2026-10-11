import importlib.util
import xml.etree.ElementTree as ET
from pathlib import Path
import tempfile
import unittest


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


if __name__ == "__main__":
    unittest.main()
