import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]


class VerifyNativeTests(unittest.TestCase):
    def install_arguments(self, distro, manager, *, helpers=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "taskmanager.pkg"
            package.touch()
            command = root / manager
            command.write_text(
                f"#!{sys.executable}\n"
                "import json, pathlib, sys\n"
                "args = sys.argv[1:]\n"
                f"if {helpers!r} and {str(package)!r} in args:\n"
                "    raise SystemExit(0)\n"
                "if 'install' not in args and '-U' not in args:\n"
                "    raise SystemExit(0)\n"
                "result = {'args': args}\n"
                "if '--config' in args:\n"
                "    result['config'] = pathlib.Path(args[args.index('--config') + 1]).read_text()\n"
                "print(json.dumps(result))\n"
                "raise SystemExit(79)\n"
            )
            command.chmod(0o755)
            arguments = ["bash", str(ROOT / "support/ci/verify-native.sh"), distro, str(package), "1.2.7"]
            if helpers:
                # Skip installed-host probes; stop at the helper install, before private ABI/GUI checks.
                arguments = [
                    "bash", "-c",
                    "rpm() { printf '%s\\n' '1.2.7-1.tw'; }; "
                    "test() { return 0; }; grep() { return 0; }; ldconfig() { return 0; }; "
                    'export -f rpm test grep ldconfig; exec "$@"',
                    "verify-helpers", *arguments,
                ]
            result = subprocess.run(
                arguments,
                env={**os.environ, "PATH": f"{root}:{os.environ['PATH']}"},
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 79, result.stderr)
            self.assertIn("Native verification failed (exit 79)", result.stderr)
            return json.loads(result.stdout)

    def test_ubuntu_install_overrides_minimal_image_translation_exclusions(self):
        result = self.install_arguments("ubuntu26.04", "apt-get")
        self.assertIn("Dpkg::Options::=--path-include=/usr/share/locale/*", result["args"])

    def test_arch_install_preserves_policy_and_whitelists_translations(self):
        result = self.install_arguments("arch", "pacman")
        self.assertEqual(
            result["config"],
            "[options]\nInclude = /etc/pacman.conf\n[options]\nNoExtract = !usr/share/locale/*\n",
        )

    def test_opensuse_helpers_include_xvfb_dependencies_and_session_daemon(self):
        result = self.install_arguments("opensuse-tumbleweed", "zypper", helpers=True)
        self.assertIn("gawk", result["args"])
        self.assertIn("dbus-1-daemon", result["args"])
        self.assertIn("xvfb-run", result["args"])
        self.assertNotIn("--allow-unsigned-rpm", result["args"])


if __name__ == "__main__":
    unittest.main()
