# Hardware monitoring

Task Manager uses available Linux telemetry without assuming that every CPU/GPU made since
2006 exposes every sensor. Missing counters are omitted; zero is displayed only when actually
reported. Extra Performance metadata remains accessible through **More details…** at narrow
window sizes. Left-click copies the complete value, including text shortened on screen.

## Implemented sources

| Field | Source and availability |
|---|---|
| Physical cores / logical processors | Online `/proc/stat` CPU IDs and sysfs package/core topology; hotplug preserves ID-to-graph mapping. |
| Intel P/E | Per-CPU CPUID 1A, with the collector's affinity restored afterwards. LP-E requires the kernel's separate `cpu_lowpower` PMU CPU list; E and LP-E sharing a PMU remain indistinguishable. |
| AMD compact cores | CPUID 80000026 with explicit heterogeneous-core flag and core type; family 19h/1Ah labels Zen 4/4c and Zen 5/5c. Older firmware that omits this leaf cannot be classified reliably. |
| CPU / RAM voltage | Explicit, unambiguous hwmon rail labels (Vcore, CPU Vcore, SVI2_Core, CPU Core Voltage; DRAM, VDIMM, VDDRAM, DDR, DRAM Voltage), mV converted to V. VID and configured targets are excluded. Multiple candidate rails are omitted. |
| GPU voltage | Device-associated amdgpu/radeon/nouveau `in0_input`, where supported by the driver. Proprietary NVIDIA voltage is not provided by this implementation. |
| BCLK | Labelled hwmon frequency or Arrow Lake-S/HX model C6h MCHBAR BCLK_FREQ firmware-reported SoC clock. No constant 100 MHz or effective-frequency-derived estimate. |
| FCLK / MCLK / UCLK | Labelled hwmon counters; authorized, version-gated ryzen_smu PM table for Matisse/Vermeer/Cezanne; Linux HSMP GET_FCLK_MCLK on a single-socket AMD system (UCLK unavailable through that command). |
| CPU multiplier | IA32_PERF_STATUS on allowlisted non-hybrid Intel family-6 models; HW_PSTATE_STATUS FID/DFSID on allowlisted family-17h/Zen-3 models. AMD value is a programmed ratio, not effective clock. Requires existing `/dev/cpu/*/msr`. No MSR writes or module loading. |
| Memory divider | UCLK:MCLK 1:1, 1:2 or 1:3 from a coherent clock sample. Intel Gear from active DDR channel scheduler registers on Tiger/Rocket/Alder/Raptor Lake, with no inference from RAM speed. |
| RAM hardware errors | EDAC controller CE/UE totals without counting DIMM totals twice. A counter is omitted if any exposed controller lacks it. `HardwareCorrupted` from meminfo is shown separately as bytes, not an error count. |
| SSD remaining endurance | NVMe `max(0, 100 − Percentage Used)`; ATA only interpreted `ssd-life-left`/`percent-lifetime-remain` normalized attributes. Not an overall health guarantee; available spare and arbitrary raw attribute IDs are not treated as life remaining. |
| System | Existing OS/kernel/desktop information, readable DMI motherboard/BIOS fields, CPU, memory modules, sorted GPUs and storage. Helper adds system/board serial and UUID. Empty firmware placeholders are filtered. |

SMART reads use UDisks2 with `auth.no_user_interaction`; background queries never open a dialog
or request authorization. Results are cached, at most one drive queried per normal collection
cycle, with five-minute refresh. A failed read removes its cached percentage.

## Memory slots and SPD

Memory shows firmware-reported DIMM/SO-DIMM locations, including empty slots. Unknown locations
are not assigned invented A1/B1 names. The diagram is symbolic, not a motherboard-position map.
Compact vertical slots sit alongside the Speed/Slots metadata, with a narrow-window fallback
below that column. Click an occupied slot for SMBIOS metadata and an automatic SPD read in the
same dialog; empty slots do not trigger EEPROM reads. Configured voltage is distinguished from
a live sensor. There is no separate SPD button.

The slot click reads already-bound Linux `eeprom`, `ee1004` and `spd5118` EEPROM devices at addresses
0x50–0x57. It performs no bus scan, driver binding or user-space bus writes. Reads are on demand,
not every collection cycle; unreadable EEPROMs can be accessed through the authorized helper.
DDR2/DDR3/DDR4/DDR5 base checksum/CRC is required before decoding JEDEC speed, supported CAS,
CL-tRCD-tRP-tRAS, timings in ns, part and serial. SPD capabilities are not live applied timings.
XMP/EXPO profiles are not decoded in this initial implementation.

Unique, non-placeholder serials link SPD to SMBIOS slots. When firmware returns duplicate/zero
serials, EEPROMs remain separately identified by their bus address rather than being guessed
into slots. This is common on desktop boards.

## Optional privileged access

Native `cargo xtask install` and native packages install:

- `/usr/libexec/taskmanager-hardware` (default local prefix: `/usr/local/libexec`);
- `taskmanager-hardware.socket` and `.service`;
- `io.github.taskmanager.hardware.enable` polkit action.

Select **Options → Extended hardware monitoring**. If no authorized permanent service is
available, the application uses pkexec to run its embedded helper temporarily. No terminal
installation is required. The helper is held by a lifetime pipe and a kernel pidfd for the
requesting application. Closing Task Manager, a crash or disabling access ends the helper and
removes its random per-instance socket under `/run` (0600, authenticated user's ownership and
SO_PEERCRED check). No persistent grant or service is created in this mode. A new application
session may require another password.

For permanent access, select **Options → Install hardware monitoring service…** (also available
in the status dialog). After authorization it installs the embedded helper and fixed unit/policy
files, reloads systemd, enables the socket, and creates a root-owned UID marker under
`/var/lib/taskmanager-hardware` (directory 0700, marker 0600). Authorization survives application
restart, logout and reboot. The socket checks kernel `SO_PEERCRED`, not a client-supplied UID.
Only fixed read/disable operations are available; clients cannot supply paths, registers or
commands. Disable removes the current account's grant. No password is stored.

If the helper is unavailable or authorization fails, the option reports the error in its
status dialog and privileged fields disappear on the refreshed snapshot. No retrying password
dialogs are generated at startup or while collecting readings. Existing readable sensors remain
available without the helper. A sandbox needs host installation and socket access; native
installation is the currently integrated path.

The helper accepts these exact ryzen_smu layouts, checking CPU family/model, driver codename,
table version, exact size and finite clock values before publishing a snapshot:

| Processor | Version | Size (bytes) |
|---|---|---:|
| Matisse (17h/71h) | 240803 / 240903 | 2020 / 1304 |
| Vermeer (19h/21h) | 380804 / 380805 / 380904 | 2212 / 2288 / 1444 |
| Cezanne (19h/50h) | 400005 | 2372 |

These are firmware-specific, reverse-engineered layouts, with the version shown in the UI.
Unknown layouts (including the guessed 380905 layout) are rejected. The optional `ryzen_smu`
driver must already be installed; Task Manager neither installs it nor sends raw SMU/SMN writes.
Zen 4/5 multiplier encodings and PM tables are not yet supported.

Intel Gear reads fixed MCHBAR offsets through read-only `/dev/mem` after checking CPU model,
Intel host bridge and enabled MCHBAR. All active DDR channels must agree. LPDDR, invalid mode
bits, inaccessible MMIO or kernel lockdown yield no field. Arrow Lake BCLK is firmware-reported,
not an independent physical oscillator measurement. Other Intel/AMD register-based BCLK sources
remain unsupported. Field sources are shown alongside the values.

Source-tree runs use the same automatic temporary launch and GUI installation. The developer
`xtask install-hardware` command remains available for packaging/staging (`--destdir`); users
do not need to run it. Both runtime modes need Python 3, pkexec and a desktop polkit agent;
permanent installation additionally needs systemd.

## What requires privileged access?

The GUI and normal magpie backend run as the current user. The optional Python helper runs as
either a temporary root process or a root system service, returning bounded JSON over its Unix
socket. Only the permanent service uses `/run/taskmanager-hardware.sock`.

| Data | Normal access | When the helper is needed |
|---|---|---|
| CPU topology/types, graphs, slots and module SMBIOS metadata | `/proc`, public sysfs/udev properties | Not needed |
| CPU/GPU/RAM voltage, labelled clocks, EDAC errors | Readable hwmon/EDAC sysfs files | Only if those specific files restrict reading; missing sensors cannot be created by root |
| SPD JEDEC data | Registered EEPROM files, if readable by the user | EEPROM files are commonly root-only; the helper can read them on slot click |
| SSD endurance | Existing UDisks2 service/policy, without interactive authorization | This helper does not read SMART or grant UDisks permissions |
| Intel/AMD register multiplier | `/dev/cpu/*/msr` | Normally root plus kernel MSR access policy; the application implements these adapters in the helper |
| Intel Gear and Arrow Lake register BCLK | Host PCI configuration and `/dev/mem` | Normally privileged; strict-devmem/lockdown may still deny root |
| Ryzen version-specific PM table and HSMP | `ryzen_smu` sysfs table or `/dev/hsmp` | Usually protected by driver permissions; these adapters currently run in the helper |
| System/board serial and UUID | Protected DMI sysfs fields | Usually root-only; these optional reads run in the helper |

For SPD or a driver table, an administrator could grant a dedicated group read access to those
specific files (with a persistent rule supported by that driver), rather than run a userspace
reader as root. SPD already uses such unprivileged access automatically. The current PM-table
and HSMP adapters would additionally need an unprivileged collection path. Device capabilities
and broad permissions on `/dev/mem` or MSR do not make the access unprivileged in substance;
they delegate privileged hardware access to the process. A narrow kernel API exposing only
needed read-only counters would be the cleaner long-term alternative for those registers.

The helper has exactly `read`, `spd` and `disable` socket requests; callers cannot supply a path,
register address, UID or shell command. It validates model/table layouts, checks the kernel's
peer UID against its session owner or a persistent root-owned grant, reads fixed sensors/registers, and returns data.
Its one-time `enable` operation is invoked through polkit and enables its fixed systemd socket.
It does not install/load drivers, change voltages/timings, write MSRs/MMIO, bind I2C devices or
perform an I2C scan. Kernel drivers may internally perform transactions to obtain a snapshot.

## Verification

- `python3 -B support/test-hardware-helper.py`: rail semantics, model gating, persistent grants,
  real Unix socket peer credentials and revocation, PM-table version/size/NaN rejection,
  fixed HSMP GET, MSR encoding, active-channel Gear and BCLK units, without root or host sensor reads.
- Backend tests: hwmon units/ambiguity and EDAC absence versus zero/partial totals.
- Offscreen UI tests: original light pixel probes, dark renders, actual System metadata,
  full-value clipboard output, live System theme changes, occupied/empty Memory slots and SPD dialog.
- Staged helper installer test verifies executable mode, unit/polkit path agreement and no implicit grant.
- Temporary-session tests use a real socket/pipe and verify cleanup on lifetime EOF and rejection
  of another UID. Embedded-source parsing and fixed GUI installer payloads are tested without root.
- Native KDE tests: real KWin keepAbove state and instance isolation, conflict preservation;
  isolated session-bus test checks second-instance activation.

Polkit/systemd installation and authorization have not been exercised with root on the
development machine. Closed-application shortcut launch could not be exercised with the
requested key here because Mission Center already owns Ctrl+Shift+Esc.
