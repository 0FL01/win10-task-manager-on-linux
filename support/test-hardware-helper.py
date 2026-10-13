# Copyright 2026 Mission Center Developers
# SPDX-License-Identifier: GPL-3.0-or-later
"""Helper contract tests; never requires root or reads the host's telemetry."""
import importlib.machinery
import importlib.util
import pathlib
import json
import os
import socket
import struct
import tempfile
import unittest
import threading
import time
from unittest.mock import patch

loader = importlib.machinery.SourceFileLoader("hardware", str(pathlib.Path(__file__).with_name("taskmanager-hardware")))
spec = importlib.util.spec_from_loader(loader.name, loader)
hardware = importlib.util.module_from_spec(spec)
loader.exec_module(hardware)


class HelperTests(unittest.TestCase):
    def test_service_install_uses_embedded_payloads_and_fixed_units(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(hardware, "INSTALL_ROOT", directory), patch.object(hardware, "authenticated_uid", return_value=1000), patch.object(hardware, "INSTALL_SOURCE", "#!/usr/bin/python3 -I\n", create=True), patch.object(hardware, "SERVICE_TEXT", "service", create=True), patch.object(hardware, "SOCKET_TEXT", "socket", create=True), patch.object(hardware, "POLICY_TEXT", "policy", create=True), patch.object(hardware.subprocess, "run") as command, patch.object(hardware, "enable") as enable:
            hardware.install_service()
            helper = pathlib.Path(directory, "usr/libexec/taskmanager-hardware")
            self.assertEqual(helper.read_text(), "#!/usr/bin/python3 -I\n")
            self.assertEqual(helper.stat().st_mode & 0o777, 0o755)
            self.assertEqual(pathlib.Path(directory, "usr/lib/systemd/system/taskmanager-hardware.socket").read_text(), "socket")
            command.assert_called_once_with(["/usr/bin/systemctl", "daemon-reload"], check=True)
            enable.assert_called_once_with(1000)

    def test_temporary_session_exits_and_removes_socket_when_lifetime_pipe_closes(self):
        uid = os.getuid()
        if uid == 0:
            self.skipTest("Requires a non-root session user")
        with tempfile.TemporaryDirectory() as directory, patch.object(hardware, "SESSION_DIRECTORY", directory), patch.object(hardware, "authenticated_uid", return_value=uid), patch.object(hardware.os, "chown"), patch.object(hardware, "snapshot", return_value={"cpu_voltage_v": 1.25}):
            reader, writer = os.pipe()
            token = "a" * 32
            path = f"{directory}/taskmanager-hardware-{os.getpid()}-{token}.sock"
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
        with tempfile.TemporaryDirectory() as directory, patch.object(hardware, "GRANTS", directory), patch.object(hardware.os, "geteuid", return_value=0), patch.dict(hardware.os.environ, {"PKEXEC_UID": "1000"}), patch.object(hardware.sys, "argv", ["taskmanager-hardware", "enable"]), patch.object(hardware.subprocess, "run") as activated:
            hardware.main()
            hardware.main()
            self.assertTrue(hardware.authorized(1000))
            self.assertEqual(pathlib.Path(directory, "1000").stat().st_mode & 0o777, 0o600)
            activated.assert_called_with(["/usr/bin/systemctl", "enable", "--now", "taskmanager-hardware.socket"], check=True)

    def test_socket_uses_peer_credentials_and_revokes_persistent_grant(self):
        uid = os.getuid()
        if uid == 0:
            self.skipTest("Root is intentionally not a telemetry client")
        with tempfile.TemporaryDirectory() as directory, patch.object(hardware, "GRANTS", directory), patch.object(hardware, "snapshot", return_value={"cpu_voltage_v": 1.25}) as snapshot:
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
