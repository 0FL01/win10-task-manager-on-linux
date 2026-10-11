import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]


class CollectReleaseTests(unittest.TestCase):
    def test_publishes_only_five_packages_and_project_sources(self):
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
            source = root / "taskmanager-1.2.7-source.tar.gz"
            source.write_bytes(b"stamped project sources")
            for distro, suffix in packages.items():
                directory = downloads / f"native-{distro}" / "ci-packages"
                directory.mkdir(parents=True)
                (directory / f"taskmanager-{distro}{suffix}").write_bytes(distro.encode())
                (directory / "build-manifest.json").write_text(json.dumps({"distro": distro}))
                (directory / "BUILD-MANIFEST.json").write_text("{}")
                (directory / "SHA256SUMS").write_text("internal checksum")
                if suffix == ".rpm":
                    (directory / f"libdrm-2.4.134-{distro}.tar.xz").write_bytes(b"libdrm sources")
                    (directory / f"libdrm-2.4.134-{distro}.tar.xz.sig").write_bytes(b"signature")

            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "support/ci/collect-release.py"),
                    "--downloads",
                    str(downloads),
                    "--output",
                    str(output),
                    "--source-archive",
                    str(source),
                ],
                check=True,
                capture_output=True,
                text=True,
            )

            self.assertEqual(
                {path.name for path in output.iterdir()},
                {f"taskmanager-{distro}{suffix}" for distro, suffix in packages.items()} | {source.name},
            )
            self.assertEqual((output / source.name).read_bytes(), source.read_bytes())


if __name__ == "__main__":
    unittest.main()
