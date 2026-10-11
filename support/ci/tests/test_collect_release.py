import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("collect_release", ROOT / "support/ci/collect-release.py")
collect_release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(collect_release)


class CollectReleaseTests(unittest.TestCase):
    def test_flattens_five_packages_and_records_builds_and_checksums(self):
        packages = {
            "fedora44": ".rpm",
            "opensuse-tumbleweed": ".rpm",
            "arch": ".pkg.tar.zst",
            "debian13": ".deb",
            "ubuntu26.04": ".deb",
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            downloads = root / "downloads"
            output = root / "release"
            for distro, suffix in packages.items():
                directory = downloads / f"native-{distro}" / "ci-packages"
                directory.mkdir(parents=True)
                (directory / f"taskmanager-{distro}{suffix}").write_bytes(distro.encode())
                (directory / "build-manifest.json").write_text(json.dumps({"distro": distro}))

            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "support/ci/collect-release.py"),
                    "--downloads",
                    str(downloads),
                    "--output",
                    str(output),
                    "--version",
                    "1.2.7",
                    "--commit",
                    "abc123",
                    "--run-number",
                    "7",
                ],
                check=True,
                capture_output=True,
                text=True,
            )

            manifest = json.loads((output / "BUILD-MANIFEST.json").read_text())
            self.assertEqual(manifest["version"], "1.2.7")
            self.assertEqual({build["distro"] for build in manifest["builds"]}, set(packages))
            self.assertEqual(len([path for path in output.iterdir() if path.name.endswith(collect_release.PACKAGE_SUFFIXES)]), 5)
            checksum_names = {line.split("  ", 1)[1] for line in (output / "SHA256SUMS").read_text().splitlines()}
            self.assertEqual(
                checksum_names,
                {path.name for path in output.iterdir() if path.is_file() and path.name != "SHA256SUMS"},
            )


if __name__ == "__main__":
    unittest.main()
