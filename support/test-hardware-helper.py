# Copyright 2026 Mission Center Developers
# SPDX-License-Identifier: GPL-3.0-or-later
"""Helper contract tests; never requires root or reads the host's telemetry."""
import importlib.machinery
import importlib.util
from contextlib import contextmanager
import io
import pathlib
import json
import os
import socket
import struct
import tempfile
import unittest
import threading
import time
from unittest.mock import Mock, patch

loader = importlib.machinery.SourceFileLoader("hardware", str(pathlib.Path(__file__).with_name("taskmanager-hardware")))
spec = importlib.util.spec_from_loader(loader.name, loader)
hardware = importlib.util.module_from_spec(spec)
loader.exec_module(hardware)


class HelperTests(unittest.TestCase):
    def setUp(self):
        owner = patch.object(hardware, "OWNER_UID", os.getuid())
        owner.start()
        self.addCleanup(owner.stop)

    @contextmanager
    def layout(self, prefix="/usr/local"):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            repo = pathlib.Path(__file__).resolve().parents[1]
            def command(arguments, lifetime=None):
                if arguments[1:] == ["unit-paths"]:
                    return "/etc/systemd/system\n/run/systemd/system\n/usr/local/lib/systemd/system\n/usr/lib/systemd/system\n"
                if arguments[1:2] == ["show"]:
                    return f"{prefix}/lib/systemd/system/{arguments[-1]}\n"
                if arguments[0] == "/usr/bin/pkaction":
                    return "io.github.taskmanager.hardware.enable\n"
                if lifetime is not None:
                    lifetime.check()
                return ""
            with patch.object(hardware, "INSTALL_ROOT", directory), patch.object(hardware, "GRANTS", str(root / "var/lib/taskmanager-hardware")), patch.object(hardware, "INSTALL_SOURCE", (repo / "support/taskmanager-hardware").read_text(), create=True), patch.object(hardware, "SERVICE_TEXT", (repo / "data/taskmanager-hardware.service").read_text(), create=True), patch.object(hardware, "SOCKET_TEXT", (repo / "data/taskmanager-hardware.socket").read_text(), create=True), patch.object(hardware, "POLICY_TEXT", (repo / "data/io.github.taskmanager.hardware.policy").read_text(), create=True), patch.object(hardware, "run_fixed", side_effect=command) as commands:
                yield root, commands

    @contextmanager
    def lifetime(self):
        reader, writer = os.pipe()
        lifetime = hardware.Lifetime(os.getpid(), os.getuid(), reader)
        try:
            yield lifetime, writer
        finally:
            lifetime.close()
            os.close(reader)
            try:
                os.close(writer)
            except OSError:
                pass

    def test_service_install_uses_embedded_payloads_and_fixed_units(self):
        with self.layout() as (root, commands), self.lifetime() as (lifetime, _):
            hardware.install_service(1000, lifetime)
            helper = root / "usr/local/libexec/taskmanager-hardware"
            self.assertEqual(helper.read_text(), hardware.INSTALL_SOURCE)
            self.assertEqual(helper.stat().st_mode & 0o777, 0o755)
            self.assertIn("ExecStart=/usr/local/libexec/taskmanager-hardware serve", (root / "usr/local/lib/systemd/system/taskmanager-hardware.service").read_text())
            self.assertIn(">/usr/local/libexec/taskmanager-hardware</annotate>", (root / "usr/share/polkit-1/actions/io.github.taskmanager.hardware.policy").read_text())
            self.assertFalse((root / "usr/local/share/polkit-1/actions").exists())
            self.assertTrue(hardware.authorized(1000))
            self.assertEqual((root / "var/lib/taskmanager-hardware/.operation.lock").stat().st_mode & 0o777, 0o600)
            commands.assert_any_call(["/usr/bin/systemctl", "daemon-reload"], lifetime)
            commands.assert_any_call(["/usr/bin/pkaction"], lifetime)

    def test_bootstrap_refuses_assets_links_and_unit_shadowing_without_replacement(self):
        paths = [hardware.LOCAL_HELPER, hardware.POLICY,
                 "/usr/local/lib/systemd/system/taskmanager-hardware.service",
                 "/etc/systemd/system/taskmanager-hardware.service",
                 "/run/systemd/system/taskmanager-hardware.socket",
                 "/usr/lib/systemd/system/taskmanager-hardware.service.d",
                 "/etc/systemd/system/taskmanager-.service.d"]
        for path in paths:
            for symlink in (False, True):
                with self.subTest(path=path, symlink=symlink), self.layout() as (root, commands), self.lifetime() as (lifetime, _):
                    conflict = root / path.lstrip("/")
                    conflict.parent.mkdir(parents=True)
                    if symlink:
                        conflict.symlink_to("missing-conflict-target")
                    else:
                        conflict.write_text("existing administrator asset")
                    with self.assertRaisesRegex(RuntimeError, "conflicts"):
                        hardware.install_service(1000, lifetime)
                    self.assertTrue(conflict.is_symlink() if symlink else conflict.read_text() == "existing administrator asset")
                    self.assertFalse(hardware.authorized(1000))
                    self.assertEqual(commands.call_count, 1)

    def test_bootstrap_refuses_symlinked_parent_and_unsupported_unit_directory(self):
        with self.layout() as (root, _), self.lifetime() as (lifetime, _):
            (root / "usr").mkdir()
            (root / "usr/local").symlink_to(root / "elsewhere")
            with self.assertRaises(RuntimeError):
                hardware.install_service(1000, lifetime)
        with self.layout(), self.lifetime() as (lifetime, _), patch.object(hardware, "run_fixed", return_value="/usr/lib/systemd/system\n"):
            with self.assertRaisesRegex(RuntimeError, "not supported"):
                hardware.install_service(1000, lifetime)

    def test_atomic_publication_never_replaces_a_destination_created_after_preflight(self):
        with self.layout() as (root, _):
            target = root / "usr/local/libexec/taskmanager-hardware"
            target.parent.mkdir(parents=True)
            target.symlink_to("administrator-owned-target")
            with self.assertRaises(FileExistsError):
                hardware.publish_new(hardware.LOCAL_HELPER, "new helper", 0o755)
            self.assertEqual(os.readlink(target), "administrator-owned-target")
            self.assertEqual(list(target.parent.iterdir()), [target])

    def test_installed_enable_uses_policy_without_updating_assets(self):
        with self.layout() as (root, commands), self.lifetime() as (lifetime, _), patch.object(hardware.sys, "argv", [hardware.LOCAL_HELPER, "enable"]):
            hardware.install_service(1000, lifetime)
            before = {str(path): path.read_bytes() for path in root.rglob("*") if path.is_file() and "/usr/" in str(path)}
            commands.reset_mock()
            hardware.enable(1001, lifetime)
            after = {str(path): path.read_bytes() for path in root.rglob("*") if path.is_file() and "/usr/" in str(path)}
            self.assertEqual(before, after)
            self.assertTrue(hardware.authorized(1001))
            self.assertFalse(any(call.args[0][1:] == ["daemon-reload"] for call in commands.call_args_list))

    def test_installed_enable_rejects_wrong_or_undiscovered_action_and_shadowed_unit(self):
        for failure in ("policy", "unit", "discovery"):
            with self.subTest(failure=failure), self.layout() as (root, commands), self.lifetime() as (lifetime, _), patch.object(hardware.sys, "argv", [hardware.LOCAL_HELPER, "enable"]):
                hardware.install_service(1000, lifetime)
                if failure == "policy":
                    policy = root / hardware.POLICY.lstrip("/")
                    policy.write_text(policy.read_text().replace(hardware.LOCAL_HELPER, "/usr/libexec/taskmanager-hardware"))
                else:
                    prior = commands.side_effect
                    commands.side_effect = lambda args, life: "" if (failure == "discovery" and args[0] == "/usr/bin/pkaction") or (failure == "unit" and args[1:2] == ["show"]) else prior(args, life)
                with self.assertRaises(RuntimeError):
                    hardware.enable(1001, lifetime)
                self.assertFalse(hardware.authorized(1001))

    def test_cancelled_waiting_installer_cannot_regrant_after_confirmed_revoke(self):
        with self.layout() as (root, commands), self.lifetime() as (lifetime, writer):
            errors = []
            blocked = threading.Event()
            resume = threading.Event()
            flock = hardware.fcntl.flock
            def acquire(fd, flags):
                try:
                    return flock(fd, flags)
                except BlockingIOError:
                    blocked.set()
                    if not resume.wait(2):
                        raise RuntimeError("Test installer did not resume")
                    # Let the old operation acquire only after revoke was acknowledged.
                    return flock(fd, flags)
            def installer():
                try:
                    hardware.install_service(1000, lifetime)
                except Exception as error:
                    errors.append(error)
            with patch.object(hardware.fcntl, "flock", side_effect=acquire):
                thread = threading.Thread(target=installer)
                try:
                    with hardware.operation_lock():
                        thread.start()
                        self.assertTrue(blocked.wait(1))
                        os.close(writer)
                    self.assertTrue(hardware.revoke(1000)["revoked"])
                finally:
                    resume.set()
                    thread.join(2)
            self.assertFalse(thread.is_alive())
            self.assertTrue(errors)
            self.assertFalse(hardware.authorized(1000))
            commands.assert_not_called()
            self.assertFalse((root / hardware.LOCAL_HELPER.lstrip("/")).exists())

    def test_cancellation_during_system_command_does_not_create_grant(self):
        with self.layout() as (_, commands), self.lifetime() as (lifetime, writer):
            original = commands.side_effect
            def command(args, life=None):
                result = original(args, life)
                if args[1:] == ["enable", "--now", "taskmanager-hardware.socket"]:
                    os.close(writer)
                return result
            commands.side_effect = command
            with self.assertRaisesRegex(RuntimeError, "cancelled"):
                hardware.install_service(1000, lifetime)
            self.assertFalse(hardware.authorized(1000))

    def test_lifetime_requires_matching_pid_and_an_open_pipe(self):
        with self.lifetime() as (lifetime, writer):
            lifetime.check()
            os.close(writer)
            with self.assertRaisesRegex(RuntimeError, "cancelled"):
                lifetime.check()
        reader, writer = os.pipe()
        try:
            with self.assertRaisesRegex(RuntimeError, "identity"):
                hardware.Lifetime(os.getpid(), os.getuid() + 1, reader)
        finally:
            os.close(reader)
            os.close(writer)

    def test_busy_revoke_and_denial_are_unconfirmed_but_ack_is_confirmed(self):
        with self.layout():
            with hardware.operation_lock():
                pathlib.Path(hardware.GRANTS, "1000").touch(mode=0o600)
                reply = hardware.revoke(1000)
                self.assertTrue(reply["busy"])
                self.assertFalse(reply["revoked"])
                self.assertTrue(hardware.authorized(1000))
                self.assertFalse(hardware.denial(1001)["revoked"])
                self.assertTrue(hardware.denial(1001)["busy"])
            self.assertTrue(hardware.revoke(1000)["revoked"])
            self.assertTrue(hardware.denial(1000)["revoked"])

    def test_silent_unauthorized_client_is_denied_before_recv_and_next_client_runs(self):
        if os.getuid() == 0:
            self.skipTest("Root is intentionally not a telemetry client")
        with self.layout(), patch.object(hardware, "snapshot", return_value={}) as snapshot:
            client, server = socket.socketpair()
            with client, server:
                start = time.monotonic()
                hardware.handle_connection(server)
                self.assertLess(time.monotonic() - start, 0.5)
                self.assertTrue(json.loads(client.recv(8192))["revoked"])
                snapshot.assert_not_called()
            with hardware.operation_lock():
                pathlib.Path(hardware.GRANTS, str(os.getuid())).touch(mode=0o600)
                for request in (b"read", b"spd", b"disable"):
                    client, server = socket.socketpair()
                    with client, server, patch.object(hardware, "spd_eeproms", return_value=[]):
                        client.sendall(request + b"\n")
                        hardware.handle_connection(server)
                        result = json.loads(client.recv(hardware.REPLY_LIMIT))
                        self.assertTrue(result["busy"] if request == b"disable" else result["enabled"])

    def test_denial_json_remains_readable_when_a_client_has_already_sent_a_request(self):
        with self.layout():
            client, server = socket.socketpair()
            with client:
                client.sendall(b"read\n")
                with server:
                    hardware.handle_connection(server)
                # Read the complete bounded newline reply as a real socket client does.
                with client.makefile("rb") as stream:
                    reply = json.loads(stream.read(hardware.REPLY_LIMIT))
                self.assertTrue(reply["revoked"])

    def test_operation_lock_refuses_symlink_and_multilink_files(self):
        for symlink in (True, False):
            with self.subTest(symlink=symlink), self.layout() as (root, _):
                directory = pathlib.Path(hardware.GRANTS)
                directory.mkdir(parents=True, mode=0o700)
                target = root / "lock-target"
                target.touch(mode=0o600)
                lock = directory / ".operation.lock"
                if symlink:
                    lock.symlink_to(target)
                else:
                    os.link(target, lock)
                with self.assertRaises((OSError, RuntimeError)):
                    with hardware.operation_lock():
                        self.fail("Untrusted lock acquired")

    def test_fixed_subprocess_waits_are_bounded_and_root_child_reaps(self):
        # A disposable Python child stands in for a fixed system command, never pkexec/systemctl.
        spawn = hardware.subprocess.Popen
        for cancelled in (False, True):
            with self.subTest(cancelled=cancelled):
                children = []
                def child(*args, **kwargs):
                    process = spawn(*args, **kwargs)
                    children.append(process)
                    return process
                lifetime = Mock() if cancelled else None
                if lifetime is not None:
                    lifetime.check.side_effect = RuntimeError("cancelled")
                with patch.object(hardware, "COMMAND_TIMEOUT", 0.1), patch.object(hardware.subprocess, "Popen", side_effect=child):
                    start = time.monotonic()
                    with self.assertRaisesRegex(RuntimeError, "cancelled" if cancelled else "timed out"):
                        hardware.run_fixed([os.sys.executable, "-c", "import time; time.sleep(30)"], lifetime)
                    self.assertLess(time.monotonic() - start, 3)
                self.assertIsNotNone(children[0].returncode)
                with self.assertRaises(ChildProcessError):
                    os.waitpid(children[0].pid, os.WNOHANG)

    def test_socket_rejects_unknown_commands_and_caps_json_without_telemetry_io(self):
        uid = os.getuid()
        if uid == 0:
            self.skipTest("Root is intentionally not a telemetry client")
        with self.layout(), patch.object(hardware, "snapshot") as snapshot, patch.object(hardware, "spd_eeproms") as spd:
            with hardware.operation_lock():
                pathlib.Path(hardware.GRANTS, str(uid)).touch(mode=0o600)
            for request in (b"read /etc/shadow", b"write 0x198", b"spd /dev/i2c-0", b"x" * 64):
                client, server = socket.socketpair()
                with client, server:
                    client.sendall(request)
                    hardware.handle_connection(server)
                    self.assertFalse(json.loads(client.recv(8192))["enabled"])
            snapshot.assert_not_called()
            spd.assert_not_called()
        with self.assertRaisesRegex(RuntimeError, "fixed limit"):
            hardware.encoded_reply({"enabled": True, "data": "x" * hardware.REPLY_LIMIT})

    def test_temporary_session_exits_and_removes_socket_when_lifetime_pipe_closes(self):
        uid = os.getuid()
        if uid == 0:
            self.skipTest("Requires a non-root session user")
        with tempfile.TemporaryDirectory() as directory, patch.object(hardware, "authenticated_uid", return_value=uid), patch.object(hardware.os, "chown"), patch.object(hardware, "snapshot", return_value={"cpu_voltage_v": 1.25}):
            # Sandbox TMPDIR can exceed AF_UNIX's path limit; this names the same disposable root.
            directory_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
            short_directory = f"/proc/self/fd/{directory_fd}"
            directory_patch = patch.object(hardware, "SESSION_DIRECTORY", short_directory)
            directory_patch.start()
            reader, writer = os.pipe()
            token = "a" * 32
            path = f"{short_directory}/taskmanager-hardware-{os.getpid()}-{token}.sock"
            errors = []
            def serve():
                try:
                    hardware.session(os.getpid(), token, reader)
                except Exception as error:
                    errors.append(error)
            thread = threading.Thread(target=serve)
            thread.start()
            try:
                deadline = time.monotonic() + 2
                while not os.path.exists(path) and not errors and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertFalse(errors)
                with socket.socket(socket.AF_UNIX) as client:
                    client.connect(path)
                    client.sendall(b"read\n")
                    self.assertTrue(json.loads(client.recv(8192))["enabled"])
                self.assertEqual(pathlib.Path(path).stat().st_mode & 0o777, 0o600)
            finally:
                os.close(writer)
                thread.join(3)
                os.close(reader)
                directory_patch.stop()
                os.close(directory_fd)
            self.assertFalse(thread.is_alive())
            self.assertFalse(errors)
            self.assertFalse(os.path.exists(path))

    def test_session_rejects_other_users_even_with_persistent_grant(self):
        uid = os.getuid()
        client, server = socket.socketpair()
        with client, server, patch.object(hardware, "authorized", return_value=True), patch.object(hardware, "snapshot") as snapshot:
            client.sendall(b"read\n")
            hardware.handle_connection(server, uid + 1)
            self.assertFalse(json.loads(client.recv(8192))["enabled"])
            snapshot.assert_not_called()

    def test_pm_table_requires_matching_cpu_version_size_and_finite_counters(self):
        layouts = [
            (4, 0x240803, (0x17, 0x71), 0x7E4, (48, 50, 51), 41),
            (4, 0x240903, (0x17, 0x71), 0x518, (48, 50, 51), 40),
            (12, 0x380804, (0x19, 0x21), 0x8A4, (48, 50, 51), 41),
            (12, 0x380805, (0x19, 0x21), 0x8F0, (48, 50, 51), 40),
            (12, 0x380904, (0x19, 0x21), 0x5A4, (48, 50, 51), 41),
            (14, 0x400005, (0x19, 0x50), 0x944, (409, 410, 411), 99),
        ]
        for codename, version, cpu, size, indices, voltage in layouts:
            table = bytearray(size)
            for index, value in zip(indices, [1900., 950., 1900.]):
                struct.pack_into("<f", table, index * 4, value)
            struct.pack_into("<f", table, voltage * 4, 1.25)
            values = {"pm_table_version": struct.pack("<I", version), "pm_table_size": struct.pack("<Q", size), "pm_table": table}
            def blob(path, expected):
                data = values[path.rsplit("/", 1)[-1]]
                return data if len(data) == expected else None
            with self.subTest(version=hex(version)), patch.object(hardware, "read", return_value=str(codename)), patch.object(hardware, "binary", side_effect=blob):
                result = hardware.ryzen_pm(("AuthenticAMD", *cpu))
                self.assertEqual([result[k] for k in ("fclk_mhz", "uclk_mhz", "mclk_mhz")], [1900., 950., 1900.])
                self.assertEqual(result["cpu_voltage_v"], 1.25)
                self.assertEqual(hardware.ryzen_pm(("AuthenticAMD", 0x19, 0x61)), {})
                struct.pack_into("<f", table, indices[0] * 4, float("nan"))
                self.assertEqual(hardware.ryzen_pm(("AuthenticAMD", *cpu)), {})
                values["pm_table_version"] = struct.pack("<I", 0x380905)
                self.assertEqual(hardware.ryzen_pm(("AuthenticAMD", *cpu)), {})
                values["pm_table_version"] = struct.pack("<I", version)
                values["pm_table_size"] = struct.pack("<Q", size + 4)
                self.assertEqual(hardware.ryzen_pm(("AuthenticAMD", *cpu)), {})

    def test_amd_ratio_gates_encoding_and_does_not_use_vid(self):
        with patch.object(hardware.glob, "glob", return_value=["/dev/cpu/2/msr"]), patch.object(hardware.os, "open", return_value=9) as opened, patch.object(hardware.os, "close"), patch.object(hardware.os, "pread", return_value=struct.pack("<Q", 152 | (8 << 8) | (255 << 14))) as pread:
            self.assertEqual(hardware.amd_multiplier(("AuthenticAMD", 0x17, 0x71))["cpu_multiplier"], 38)
            pread.assert_called_once_with(9, 8, 0xC0010293)
            self.assertEqual(hardware.amd_multiplier(("AuthenticAMD", 0x19, 0x61)), {})
            self.assertEqual(opened.call_count, 1)
            pread.return_value = struct.pack("<Q", 152)
            self.assertEqual(hardware.amd_multiplier(("AuthenticAMD", 0x17, 0x71)), {})

    def test_hsmp_fixed_get_only_single_socket(self):
        def ioctl(fd, command, message, mutate):
            self.assertEqual((fd, command, len(message)), (9, 0xC02CF800, 44))
            self.assertEqual(struct.unpack_from("<IHH", message), (0x0F, 0, 2))
            struct.pack_into("<II", message, 8, 1800, 1600)
        with patch.object(hardware.glob, "glob", return_value=["/cpu/package"]), patch.object(hardware, "read", return_value="0"), patch.object(hardware.os, "open", return_value=9) as opened, patch.object(hardware.os, "close"), patch.object(hardware.fcntl, "ioctl", side_effect=ioctl):
            self.assertEqual(hardware.amd_hsmp(("AuthenticAMD", 0x19, 1))["fclk_mhz"], 1800)
            opened.assert_called_once_with("/dev/hsmp", os.O_RDONLY | os.O_CLOEXEC)
        with patch.object(hardware.glob, "glob", return_value=["/cpu/0", "/cpu/1"]), patch.object(hardware, "read", side_effect=["0", "1"]), patch.object(hardware.os, "open") as opened:
            self.assertEqual(hardware.amd_hsmp(("AuthenticAMD", 0x19, 1)), {})
            opened.assert_not_called()

    def test_intel_gear_uses_active_channels_and_arrow_lake_bclk(self):
        base = 0xFEDC0000
        registers = {0xD800: 1, 0xD80C: 16, 0xD810: 0, 0xE088: 1 << 31,
                     0x1D800: 1, 0x1D80C: 16, 0x1D810: 0, 0x1E088: 1 << 31,
                     0x5F60: 101250}
        def pread(fd, size, offset):
            if fd == 8:
                self.assertEqual((size, offset), (8, 0x48))
                return struct.pack("<Q", base | 1)
            self.assertEqual((fd, size), (9, 4))
            return struct.pack("<I", registers[offset - base])
        def opened(path, flags):
            self.assertEqual(flags, os.O_RDONLY | os.O_CLOEXEC)
            return 8 if path.endswith("/config") else 9
        with patch.object(hardware, "read", side_effect=lambda path: "0x8086" if path.endswith("/vendor") else "0x060000"), patch.object(hardware.os, "open", side_effect=opened), patch.object(hardware.os, "close"), patch.object(hardware.os, "pread", side_effect=pread):
            self.assertEqual(hardware.intel_memory(("GenuineIntel", 6, 0x97))["intel_gear"], 2)
            registers[0x1E088] = 0
            self.assertEqual(hardware.intel_memory(("GenuineIntel", 6, 0x97)), {})
            registers[0x1E088] = registers[0xE088] = 1 << 15
            self.assertEqual(hardware.intel_memory(("GenuineIntel", 6, 0xB7))["intel_gear"], 4)
            registers[0xD800] = 2
            self.assertEqual(hardware.intel_memory(("GenuineIntel", 6, 0x97)), {})
            self.assertEqual(hardware.intel_memory(("GenuineIntel", 6, 0xC6))["bclk_mhz"], 101.25)
        with patch.object(hardware.os, "open") as opened:
            self.assertEqual(hardware.intel_memory(("GenuineIntel", 6, 0x55)), {})
            opened.assert_not_called()

    def test_explicit_enable_activates_only_fixed_socket_and_keeps_grant(self):
        with self.layout() as (root, commands), self.lifetime() as (lifetime, _), patch.object(hardware.os, "geteuid", return_value=0), patch.dict(hardware.os.environ, {"PKEXEC_UID": "1000"}), patch.object(hardware.sys, "argv", [hardware.LOCAL_HELPER, "enable"]), patch.object(hardware.sys, "stdout", Mock(buffer=io.BytesIO())):
            hardware.install_service(1000, lifetime)
            hardware.main()
            hardware.main()
            self.assertTrue(hardware.authorized(1000))
            self.assertEqual((root / "var/lib/taskmanager-hardware/1000").stat().st_mode & 0o777, 0o600)
            commands.assert_called_with(["/usr/bin/systemctl", "enable", "--now", "taskmanager-hardware.socket"], None)

    def test_installed_enable_pid_cli_uses_pkexec_uid_and_stdin_lifetime(self):
        if os.getuid() == 0:
            self.skipTest("PKEXEC_UID must be a non-root authenticated user")
        with self.layout(), self.lifetime() as (lifetime, _):
            hardware.install_service(1000, lifetime)
            output = Mock(buffer=io.BytesIO())
            with patch.object(hardware.os, "geteuid", return_value=0), patch.dict(hardware.os.environ, {"PKEXEC_UID": str(os.getuid())}), patch.object(hardware.sys, "argv", [hardware.LOCAL_HELPER, "enable", str(os.getpid())]), patch.object(hardware.sys, "stdout", output), patch.object(hardware, "Lifetime", return_value=lifetime) as constructed, patch.object(lifetime, "close"):
                hardware.main()
            constructed.assert_called_once_with(os.getpid(), os.getuid())
            self.assertEqual(json.loads(output.buffer.getvalue()), {"enabled": True})

    def test_socket_uses_peer_credentials_and_revokes_persistent_grant(self):
        uid = os.getuid()
        if uid == 0:
            self.skipTest("Root is intentionally not a telemetry client")
        with tempfile.TemporaryDirectory() as directory, patch.object(hardware, "INSTALL_ROOT", directory), patch.object(hardware, "GRANTS", directory), patch.object(hardware, "snapshot", return_value={"cpu_voltage_v": 1.25}) as snapshot:
            def request(command):
                client, server = socket.socketpair()
                with client, server:
                    client.sendall(command + b"\n")
                    hardware.handle_connection(server)
                    return json.loads(client.recv(8192))
            self.assertFalse(request(b"read")["enabled"])
            snapshot.assert_not_called()
            with patch.object(hardware, "spd_eeproms") as spd:
                self.assertFalse(request(b"spd")["enabled"])
                spd.assert_not_called()
            pathlib.Path(directory, str(uid)).touch()
            with patch.object(hardware, "spd_eeproms", return_value=[{"source": "0-0050", "driver": "ee1004", "hex": "00"}]) as spd:
                self.assertEqual(request(b"spd")["spd"][0]["source"], "0-0050")
                spd.assert_called_once()
            self.assertEqual(request(b"read")["data"]["cpu_voltage_v"], 1.25)
            self.assertFalse(request(b"disable")["enabled"])
            self.assertFalse(request(b"read")["enabled"])
            self.assertFalse(pathlib.Path(directory, str(uid)).exists())

    def test_only_explicit_known_voltage_and_single_rail(self):
        with patch.object(hardware.glob, "glob", return_value=["/sensor/in0_label"]), patch.object(hardware, "read", side_effect=["VID", "Vcore", "1250"]):
            self.assertIsNone(hardware.voltage({"vcore"}))
            self.assertEqual(hardware.voltage({"vcore"}), 1.25)
        with patch.object(hardware.glob, "glob", return_value=["/sensor/in0_label", "/sensor/in1_label"]), patch.object(hardware, "read", side_effect=["Vcore", "1250", "Vcore", "1300"]):
            self.assertIsNone(hardware.voltage({"vcore"}))

    def test_unknown_and_virtual_models_never_open_msr(self):
        for info in ["vendor_id: AuthenticAMD\ncpu family: 25\nmodel: 1", "vendor_id: GenuineIntel\ncpu family: 6\nmodel: 65535", "vendor_id: GenuineIntel\ncpu family: 6\nmodel: 158\nflags: hypervisor"]:
            with patch.object(hardware, "read", return_value=info), patch.object(hardware.os, "open") as opened:
                self.assertIsNone(hardware.intel_multiplier())
                opened.assert_not_called()

    def test_permission_is_uid_scoped_and_persistent(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(hardware, "GRANTS", directory):
            pathlib.Path(directory, "1000").touch()
            self.assertTrue(hardware.authorized(1000))
            self.assertFalse(hardware.authorized(1001))
            self.assertFalse(hardware.authorized(0))
            pathlib.Path(directory, "1000").unlink()
            self.assertFalse(hardware.authorized(1000))


if __name__ == "__main__":
    unittest.main()
