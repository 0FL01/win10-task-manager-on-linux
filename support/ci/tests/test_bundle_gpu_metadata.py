import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[2] / "bundle-gpu-libs.py"
SPEC = importlib.util.spec_from_file_location("bundle_gpu_libs", SCRIPT)
bundle_gpu_libs = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bundle_gpu_libs)


class PackageMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def write(self, virtual, content=""):
        path = self.root / virtual.lstrip("/")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def add_arch_package(self, *, licenses=("GPL-3.0-or-later WITH Linux-syscall-note",)):
        local = self.root / "var/lib/pacman/local/libdemo-runtime-2.4-1"
        local.mkdir(parents=True)
        license_fields = "".join(f"{license_name}\n" for license_name in licenses)
        (local / "desc").write_text(
            "%NAME%\nlibdemo-runtime\n\n"
            "%VERSION%\n2.4-1\n\n"
            "%BASE%\nlibdemo\n\n"
            "%URL%\nhttps://example.test/libdemo\n\n"
            f"%LICENSE%\n{license_fields}\n"
        )
        (local / "files").write_text(
            "%FILES%\n"
            "usr/\n"
            "usr/lib/\n"
            "usr/lib/libdemo.so\n"
            "usr/share/licenses/libdemo-runtime/\n"
            "usr/share/licenses/libdemo-runtime/NOTICE\n"
        )
        library = self.write("/usr/lib/libdemo.so.1", "library")
        alias = self.root / "usr/lib/libdemo.so"
        alias.symlink_to("libdemo.so.1")
        self.write("/usr/share/licenses/libdemo-runtime/NOTICE", "runtime notice")
        self.write("/usr/share/licenses/libdemo/BASE-LICENSE", "base source notice")
        self.write("/usr/share/licenses/spdx/GPL-3.0-or-later.txt", "GPL text")
        self.write(
            "/usr/share/licenses/spdx/exceptions/Linux-syscall-note.txt",
            "exception text",
        )
        return library

    def test_pacman_metadata_includes_package_base_and_spdx_notices(self):
        library = self.add_arch_package()

        metadata = bundle_gpu_libs.package_metadata(
            self.root, library, Path("/usr/lib/libdemo.so")
        )

        self.assertEqual(metadata["package"], "libdemo-runtime")
        self.assertEqual(metadata["version"], "2.4-1")
        self.assertEqual(
            metadata["source"],
            "pacman-source:libdemo@2.4-1 https://example.test/libdemo",
        )
        self.assertEqual(
            metadata["license_expression"],
            "GPL-3.0-or-later WITH Linux-syscall-note",
        )
        self.assertEqual(
            set(metadata["licenses"]),
            {
                "/usr/share/licenses/libdemo-runtime/NOTICE",
                "/usr/share/licenses/libdemo/BASE-LICENSE",
                "/usr/share/licenses/spdx/GPL-3.0-or-later.txt",
                "/usr/share/licenses/spdx/exceptions/Linux-syscall-note.txt",
            },
        )

    def test_arch_database_prevents_host_rpm_fallback_for_root_sysroot(self):
        self.add_arch_package()
        # Simulate the ordinary host root while redirecting its filesystem
        # lookups into this temporary Arch sysroot. A host rpm executable may
        # exist, but no RPM database is selected and rpm must not be run.
        self.write("/usr/bin/rpm", "host rpm executable")
        real_in_root = bundle_gpu_libs.in_root

        def temporary_sysroot(_root, virtual):
            return real_in_root(self.root, virtual)

        with patch.object(bundle_gpu_libs, "in_root", side_effect=temporary_sysroot):
            with patch.object(bundle_gpu_libs, "run") as run:
                metadata = bundle_gpu_libs.package_metadata(
                    Path("/"), Path("/usr/lib/libdemo.so.1"), Path("/usr/lib/libdemo.so")
                )

        self.assertEqual(metadata["package"], "libdemo-runtime")
        run.assert_not_called()

    def test_missing_package_database_does_not_query_rpm(self):
        library = self.write("/usr/lib/libdemo.so.1", "library")

        with patch.object(bundle_gpu_libs, "run") as run:
            with self.assertRaisesRegex(ValueError, "no source/version package metadata"):
                bundle_gpu_libs.package_metadata(self.root, library)

        run.assert_not_called()

    def test_rpm_is_queried_when_selected_sysroot_has_its_database(self):
        library = self.write("/usr/lib/libdemo.so.1", "library")
        self.write("/var/lib/rpm/rpmdb.sqlite", "database")
        with patch.object(
            bundle_gpu_libs,
            "run",
            side_effect=[
                "libdemo\t2.4-1\tlibdemo.src.rpm\thttps://example.test\tMIT\n",
                "/usr/share/licenses/libdemo/LICENSE\tl\n",
            ],
        ) as run:
            metadata = bundle_gpu_libs.package_metadata(self.root, library)

        self.assertEqual(metadata["package"], "libdemo")
        self.assertEqual(metadata["version"], "2.4-1")
        self.assertEqual(metadata["licenses"], ["/usr/share/licenses/libdemo/LICENSE"])
        self.assertEqual(run.call_args_list[0].args[:3], ("rpm", "--root", str(self.root)))

    def test_pacman_package_without_license_files_returns_no_notices(self):
        library = self.add_arch_package(licenses=("custom:unbundled",))
        for path in (self.root / "usr/share/licenses").rglob("*"):
            if path.is_file():
                path.unlink()

        metadata = bundle_gpu_libs.package_metadata(
            self.root, library, Path("/usr/lib/libdemo.so")
        )

        self.assertEqual(metadata["licenses"], [])


if __name__ == "__main__":
    unittest.main()
