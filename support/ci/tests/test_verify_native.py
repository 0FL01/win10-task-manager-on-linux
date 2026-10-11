import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]


class VerifyNativeTests(unittest.TestCase):
    def install_arguments(self, distro, manager):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = root / "taskmanager.pkg"
            package.touch()
            command = root / manager
            command.write_text(
                f"#!{sys.executable}\n"
                "import json, pathlib, sys\n"
                "args = sys.argv[1:]\n"
                "if 'install' not in args and '-U' not in args:\n"
                "    raise SystemExit(0)\n"
                "result = {'args': args}\n"
                "if '--config' in args:\n"
                "    result['config'] = pathlib.Path(args[args.index('--config') + 1]).read_text()\n"
                "print(json.dumps(result))\n"
                "raise SystemExit(79)\n"
            )
            command.chmod(0o755)
            result = subprocess.run(
                ["bash", str(ROOT / "support/ci/verify-native.sh"), distro, str(package), "1.2.7"],
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


if __name__ == "__main__":
    unittest.main()
