import json
import os
import shutil
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

    def test_deb_install_explicitly_includes_optional_hardware_prerequisites(self):
        for distro in ("debian13", "ubuntu26.04"):
            with self.subTest(distro=distro):
                result = self.install_arguments(distro, "apt-get")
                self.assertIn("polkitd", result["args"])
                self.assertIn("pkexec", result["args"])

    def test_arch_install_preserves_policy_and_whitelists_translations(self):
        result = self.install_arguments("arch", "pacman")
        self.assertEqual(
            result["config"],
            "[options]\nInclude = /etc/pacman.conf\n[options]\nNoExtract = !usr/share/locale/*\n",
        )

    def test_rpm_install_explicitly_includes_optional_hardware_prerequisites(self):
        for distro, manager in (("fedora44", "dnf"), ("opensuse-tumbleweed", "zypper")):
            with self.subTest(distro=distro):
                result = self.install_arguments(distro, manager)
                self.assertIn("polkit", result["args"])
                if distro == "opensuse-tumbleweed":
                    self.assertIn("pkexec", result["args"])

    def test_opensuse_helpers_include_xvfb_dependencies_and_session_daemon(self):
        result = self.install_arguments("opensuse-tumbleweed", "zypper", helpers=True)
        self.assertIn("gawk", result["args"])
        self.assertIn("dbus-1-daemon", result["args"])
        self.assertIn("xvfb-run", result["args"])
        self.assertNotIn("--allow-unsigned-rpm", result["args"])

    def test_opensuse_refresh_retries_once_and_requires_complete_metadata(self):
        for name in ("setup-native.sh", "verify-native.sh"):
            script = (ROOT / "support/ci" / name).read_text()
            start = script.index("    opensuse-tumbleweed)")
            branch = script[start:script.index("\n        ;;", start) + len("\n        ;;")]
            for failures in (1, 2):
                with self.subTest(script=name, refresh_failures=failures):
                    result = subprocess.run(
                        ["bash", "-c", """
set -euo pipefail
failures=$VERIFY_TEST_REFRESH_FAILURES
zypper() {
    "$VERIFY_TEST_PYTHON" -c 'import json, sys; print(json.dumps(sys.argv[1:]))' zypper "$@"
    if [[ $2 == refresh ]]; then
        if (( failures > 0 )); then
            failures=$((failures - 1))
            return 4
        fi
        return 0
    fi
    return 79
}
sleep() {
    "$VERIFY_TEST_PYTHON" -c 'import json, sys; print(json.dumps(sys.argv[1:]))' sleep "$@"
}
distro=opensuse-tumbleweed
package=taskmanager.rpm
version=1.2.7
case $distro in
""" + branch + "\nesac"],
                        env={**os.environ, "VERIFY_TEST_PYTHON": sys.executable,
                             "VERIFY_TEST_REFRESH_FAILURES": str(failures)},
                        capture_output=True, text=True,
                    )
                    self.assertEqual(result.returncode, 79 if failures == 1 else 4, result.stderr)
                    commands = [json.loads(line) for line in result.stdout.splitlines()]
                    refresh = ["zypper", "--non-interactive", "refresh", "--force"]
                    self.assertEqual(commands[:3], [refresh, ["sleep", "15"], refresh])
                    if failures == 1:
                        self.assertEqual(len(commands), 4)
                        self.assertEqual(commands[3][:4], ["zypper", "--non-interactive", "--no-refresh", "install"])
                    else:
                        self.assertEqual(len(commands), 3)

    def test_optional_hardware_prerequisites_are_checked_before_smoke_dependencies(self):
        script = (ROOT / "support/ci/verify-native.sh").read_text()
        probes = script[script.index("test -x /usr/bin/taskmanager"):script.index("test -s /usr/share/missioncenter/hw.db")]
        with tempfile.TemporaryDirectory() as temporary:
            result = subprocess.run(
                ["bash", "-c", """
set -e
test() { printf '%s\n' "$*" >> "$VERIFY_TEST_PROBES"; }
grep() { return 0; }
rpm() { printf '%s\n' "$*" >> "$VERIFY_TEST_PROBES"; }
distro=opensuse-tumbleweed
""" + probes],
                env={**os.environ, "VERIFY_TEST_PROBES": f"{temporary}/probes"},
                capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            checked = (Path(temporary) / "probes").read_text().splitlines()
            for binary in ("python3", "pkexec", "pkaction"):
                self.assertIn(f"-x /usr/bin/{binary}", checked)
            self.assertIn("-x /usr/libexec/taskmanager-hardware", checked)
            self.assertIn("-q pkexec", checked)
            self.assertIn("-s /usr/share/polkit-1/actions/io.github.taskmanager.hardware.policy", checked)

    @unittest.skipUnless(shutil.which("rpmspec"), "RPM metadata parser unavailable")
    def test_rpm_metadata_recommends_opensuse_split_pkexec_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            # The GPU closure is unrelated to this dependency query; provide an empty filter.
            filter_directory = Path(temporary) / "usr/lib/taskmanager/gpu"
            filter_directory.mkdir(parents=True)
            (filter_directory / "rpm-requires-filter.txt").touch()
            for suse, expected in ((0, False), (1600, True)):
                result = subprocess.run(
                    ["rpmspec", "--parse", "--define", f"stagedir {temporary}",
                     "--define", f"suse_version {suse}", str(ROOT / "packaging/rpm/taskmanager.spec")],
                    capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                recommends = [line.split(":", 1)[1].strip() for line in result.stdout.splitlines() if line.startswith("Recommends:")]
                self.assertIn("polkit", recommends)
                self.assertEqual("pkexec" in recommends, expected)
                self.assertIn("Requires:       python3", result.stdout)

    def test_gui_smoke_uses_truecolor_screen_instead_of_distribution_default(self):
        script = (ROOT / "support/ci/verify-native.sh").read_text()
        definition = script[script.index("run_gui() {"):script.index("\nrun_gui glow synthetic")]
        with tempfile.TemporaryDirectory() as temporary:
            result = subprocess.run(
                ["bash", "-c", """
timeout() {
    "$VERIFY_TEST_PYTHON" -c 'import json, sys; print(json.dumps(sys.argv[1:]))' "$@"
    return 79
}
tmp=$VERIFY_TEST_TMPDIR
""" + definition + "\nrun_gui glow synthetic"],
                env={**os.environ, "VERIFY_TEST_PYTHON": sys.executable, "VERIFY_TEST_TMPDIR": temporary},
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            arguments = json.loads((Path(temporary) / "glow-synthetic.log").read_text())
            self.assertIn("xvfb-run", arguments)
            self.assertIn("-s", arguments)
            self.assertEqual(arguments[arguments.index("-s") + 1], "-screen 0 1280x1024x24")
            self.assertIn("RUST_LOG=trace", arguments)
            self.assertIn("RUST_LOG_STYLE=never", arguments)

    def test_production_smoke_rejects_debug_trace_and_profile_reports(self):
        script = (ROOT / "support/ci/verify-native.sh").read_text()
        definition = script[script.index("run_gui() {"):script.index("\nrun_gui glow synthetic")]
        for message, expected in [
            ("[timestamp WARN component] retained warning", 0),
            ("[timestamp DEBUG component] debug leak", 1),
            ("[timestamp TRACE component] trace leak", 1),
            ("ui profile final elapsed_s=1", 1),
            ("daemon profile 10s cumulative", 1),
        ]:
            with self.subTest(message=message), tempfile.TemporaryDirectory() as temporary:
                result = subprocess.run(
                    ["bash", "-c", """
timeout() { printf '%s\n' "$VERIFY_TEST_LOG"; }
check_stats() { return 0; }
tmp=$VERIFY_TEST_TMPDIR
""" + definition + "\nrun_gui glow real"],
                    env={**os.environ, "VERIFY_TEST_TMPDIR": temporary, "VERIFY_TEST_LOG": message},
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(result.returncode, expected, result.stderr)


if __name__ == "__main__":
    unittest.main()
