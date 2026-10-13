<img align="left" src="data/icons/hicolor/64x64/apps/io.github.taskmanager.TaskManager.png" alt="Task Manager icon" width="64"/>

# Task Manager

See your processes, performance, process details and services in a familiar tabbed task manager.

Task Manager is based on [Mission Center](https://missioncenter.io/): it uses the Mission Center statistics
backend (magpie, shipped as `missioncenter-magpie`) and code from the Mission Center project by the Mission Center
Developers. The user interface is written from scratch with egui; see [Based on Mission Center](#based-on-mission-center).

## Features

* Monitor overall or per-thread CPU usage
* See system process, thread, and handle count, uptime, clock speed (base and current), cache sizes
* Monitor RAM and Swap usage
* See a breakdown how the memory is being used by the system
* Monitor Disk utilization and transfer rates
* Monitor network utilization and transfer speeds
* See network interface information such as network card name, connection type (Wi-Fi or Ethernet), wireless speeds and
  frequency, hardware address, IP address
* Monitor overall GPU usage, video encoder and decoder usage, memory usage and power consumption, powered by the popular
  NVTOP project
* See a breakdown of resource usage by app and process
* Familiar interface in the style of the Windows 10 Task Manager (Processes, Performance, Details and Services tabs),
  re-created clean-room from measurements; compact "Fewer details" view for simple monitoring
* Use hardware accelerated rendering for all the graphs in an effort to reduce CPU and overall resource usage
* Pure-Rust user interface ([egui](https://github.com/emilk/egui)/eframe); repaints only when new data arrives
* Written in Rust
* Light, Dark and System themes in View → Theme, preserving the Win10 layout
* Performance → System groups software, motherboard, BIOS and hardware information
* Click metadata values to copy the full value, with a brief highlight
* Hardware-reported core types, supported voltage sensors, EDAC memory counters and SSD remaining endurance
* DIMM/SO-DIMM slot diagrams, module metadata and on-demand SPD JEDEC timings (DDR2–DDR5)

## Limitations

Please note there is ongoing work to overcome all of these.

* Per-process network usage counts TCP traffic of your own processes only (kernel socket
  statistics, no privileges): UDP such as QUIC and other users' processes are not visible.
* Intel GPU monitoring is only supported for Broadwell and later GPUs; and does not support VRAM, power, or temperature
  monitoring.
* When using Linux Mint/Cinnamon, launched applications may not show up in the "Applications" section. (Upstream
  issue: https://github.com/linuxmint/cinnamon/issues/12015)

Task Manager draws its own widgets and window decorations. View → Theme chooses Light, Dark or
System (follows desktop light/dark changes); desktop stylesheets do not change its Win10 geometry.
Hardware telemetry availability depends on the processor, board, driver and permissions, rather
than just the device's release year. See [hardware monitoring](docs/hardware-monitoring.md).

## Installing

Native packages are published in [GitHub Releases](https://github.com/0FL01/win10-task-manager-on-linux/releases)
for Fedora 44 (`.rpm`), openSUSE Tumbleweed (`.rpm`), Arch Linux (`.pkg.tar.zst`), Debian 13 (`.deb`)
and Ubuntu 26.04 (`.deb`, x86_64). Builds run on every branch push. Successful `main` builds are
released automatically only when Rust sources, Cargo manifests/lockfiles or the pinned app-rummage
revision differ from the published latest release. Documentation-only changes do not create a
release unless they include previously unpublished Rust changes; fixing CI can publish those changes
without a manual release. With no published release, the existing Rust code is released once.
Older workflow runs do not release after a newer version has been published.
The application patch version increases for each workflow run, so gaps are expected when
other branches, failed runs or unchanged Rust inputs consume version numbers. The packages replace/conflict with older
Mission Center package names where they install the same `missioncenter-magpie` daemon.

For local Fedora development, `cargo xtask rpm` still builds an RPM using the current workspace version.

Settings live in `~/.config/taskmanager/config.toml`; settings of earlier builds
(`~/.config/missioncenter/config.toml`) are copied on the first start.

## Building and running

### Building - Native

**Requirements:**

| Dependency                | Comment                                         | Minimum Version |
|---------------------------|-------------------------------------------------|----------------:|
| Rust                      |                                                 |            1.92 |
| CMake                     | builds the bundled NNG library                  |            3.15 |
| C/C++ compiler, pkg-config, patch | native backend libraries and upstream patches |          N/A |
| Python3                   | generates the hardware database for magpie      |            3.10 |
| gettext                   | `msgfmt`, compiles translations                 |             N/A |
| DRM development libraries | magpie                                          |             N/A |
| GBM development libraries | magpie                                          |             N/A |
| udev development libraries| magpie                                          |             N/A |
| patchelf, readelf          | builder-only private GPU library staging        |             N/A |
| Vulkan or OpenGL driver   | run time only (wgpu or glow renderer)           |             N/A |

No UI toolkit development packages are needed: the interface is written with egui and talks to Wayland/X11 directly.

**Build instructions**

```bash
git clone --recurse-submodules https://github.com/0FL01/win10-task-manager-on-linux.git
cd win10-task-manager-on-linux

# On Ubuntu 26.04 all dependencies, except for the Rust toolchain, can be installed with:
sudo apt install build-essential cmake curl desktop-file-utils gettext git libdbus-1-dev libdrm-dev libgbm-dev libudev-dev pkg-config python3 patchelf binutils

# Fedora alternative:
# sudo dnf install gcc gcc-c++ cmake curl desktop-file-utils gettext git libdrm-devel mesa-libgbm-devel systemd-devel pkgconf-pkg-config python3 patch patchelf binutils

cargo build --release -p taskmanager-app
```

To run the application from the source tree (builds the magpie daemon and the translations on first use):

```bash
cargo xtask run-dev
```

`run-dev` uses the host's installed GPU libraries; it does not redistribute them or require
`patchelf` or extra library license-notice files. `cargo xtask install`, `rpm` and `magpie`
build the relocatable private bundle and require the packaging tools and matching notices.

Useful flags of the `taskmanager` binary:

* `--renderer wgpu|glow` — select the renderer; glow (OpenGL) is the default, both are compiled in by default builds
  (`cargo build -p taskmanager-app --no-default-features --features glow` produces an OpenGL-only build)
* `--synthetic N` — run on generated data with `N` processes, without the daemon
* `--perf-overlay` (or F12) — show frame and UI pass timings

Environment variables: `MC_LOCALEDIR` (translations directory), `MC_MAGPIE_HW_DB` (hardware database of the daemon),
`RUST_LOG` (logging).

### Always on top

On native KDE Plasma 6 Wayland (source builds, native packages and AppImage),
Options → Always on top uses KWin's scripting API to change only this instance's window.
No separately installed script or utility is required. The check mark and saved preference
change after KWin confirms the requested state; forced KWin window rules may reject it.
The last confirmed state is shown, without continuously tracking changes made in KWin's
own window menu. Pending operations and failures have explanatory tooltips.

Other Wayland desktops and Flatpak/Snap Wayland launches currently show the option as
unavailable. The existing window-level request is unchanged on X11 and other platforms.

### Launch shortcuts and extended hardware monitoring

On native KDE, Options → Keyboard Shortcuts configures Ctrl+Shift+Esc and optional
Ctrl+Alt+Delete. KDE launches the application even when it is closed; a subsequent launch
restores the existing instance. Assignments persist in KDE across login/reboot. Conflicts are
reported in the option's tooltip without replacing another application's shortcut. On other
desktops, assign `taskmanager` in the desktop's keyboard settings.

Options → Extended hardware monitoring starts an embedded temporary helper after polkit
authorization, without terminal commands or manual installation. It exits and removes its socket
when Task Manager closes. Options → Install hardware monitoring service… instead installs and
authorizes a permanent systemd service from the GUI, for persistent access across restarts.
Disabling monitoring revokes a permanent grant or ends the temporary helper. Startup and background
sampling never ask for a password. The GUI and magpie stay unprivileged.
Available sources and installation details are in [hardware monitoring](docs/hardware-monitoring.md).

### GPU monitoring

Processes includes GPU and GPU engine by default; the header menu also exposes total,
dedicated and shared GPU memory. Details offers the four GPU columns through its header menu.
Performance shows each adapter with selectable engine graphs, separate memory graphs,
available sensors/driver metadata, copy and graph summary (Ctrl+M or double-click).

DRM fdinfo counters are collected inside the host-side magpie backend; NVIDIA uses the installed
driver's NVML library, not `nvidia-smi`. No GPU monitoring utility or extra monitoring-library
package is required on the runtime host. Open libraries and notices are staged privately by
`cargo xtask install`; the OS runtime and matching graphics driver are still required.
Missing permissions, unsupported counters, warmup and stale samples show `—` with a reason;
partial values are marked `~`. NVIDIA does not expose a complete per-process 3D/Compute/Copy
breakdown or portable shared-memory counter, so these are not fabricated.

Native builders must provide the actual libdrm/libdrm_amdgpu/libudev dependency closure and
matching license notices. If a distribution omits notices (for example Fedora's libdrm package),
set builder-only `MC_GPU_LICENSES` to a JSON map of SONAMEs to matching-source notice paths;
the bundle fails instead of silently omitting licenses. `MC_GPU_SYSROOT`, `MC_GPU_LIB_ROOTS`,
`MC_GPU_METADATA` and `PATCHELF` select controlled builder inputs, not runtime GPU utilities.

To install the app (binaries, daemon, translations, desktop entry, icons):

```bash
cargo xtask install --prefix /usr/local        # add `--destdir DIR` for staged installs
```

And run the app from your launcher or from the command-line:

```bash
taskmanager
```

### Building - AppImage

**Note:** Creating a fully distro-agnostic AppImage requires running the `support/create-appimage.sh` script in an
ArchLinux container after installing Task Manager into that container (`cargo xtask install --prefix /usr`).

### Building - Snap

The Snap package is built from the portable tree produced by `support/build-portable.sh`.

### Building - Flatpak

**Requirements:**

* Flatpak
* Flatpak-Builder

The frontend vendor list `flatpak/cargo-sources.json` is generated from the root `Cargo.lock`.
After changing that lockfile, regenerate it with the
[official Cargo generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo):
`python3 flatpak-cargo-generator.py Cargo.lock -o flatpak/cargo-sources.json`
(the generator requires `aiohttp` and `tomlkit`). The backend's vendor list is separate:
`flatpak/cargo-sources-magpie.json` corresponds to `subprojects/magpie/Cargo.lock`.
The backend sources are tracked here; only the pinned app-rummage dependency is a submodule.

Add the `flathub` repo is not already present:

```bash
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

Install the required Flatpak runtimes and SDKs:

```bash
flatpak install -y \
    org.freedesktop.Platform//25.08 \
    org.freedesktop.Sdk//25.08
```

Finally build a Flatpak package:

```bash
cd flatpak
flatpak-builder --repo=repo --ccache --force-clean build io.github.taskmanager.TaskManager.json
flatpak build-bundle repo taskmanager.flatpak io.github.taskmanager.TaskManager
```

Install the package:

```bash
flatpak uninstall -y io.github.taskmanager.TaskManager
flatpak install -y taskmanager.flatpak
```

Run the app from your launcher or from the command-line:

```bash
flatpak run io.github.taskmanager.TaskManager
```

## Based on Mission Center

Task Manager borrows, with thanks and under the GPL, from [Mission Center](https://missioncenter.io/)
(© 2023-2025 Mission Center Developers, GPL-3.0-or-later):

* the statistics backend **magpie** (`subprojects/magpie`, installed as `missioncenter-magpie`) and the graph data
  library (the upstream graph-widget core, now local at `crates/mc-graph-core`);
* the application logic ported from the Mission Center GTK application (data model, formatting, settings, translations
  in `po/`);
* the Mission Center GTK application as the historical source of the port. Its `src/`, `resources/` and Meson build
  have been removed from the active tree; the original sources remain in repository history.

See [provenance](docs/PROVENANCE.md) for original authors, source revisions and preserved historical notices.

The interface (`crates/mc-ui/src/win10/`) is new and was implemented clean-room from measurements
(`docs/win10-spec/`). Bugs of Task Manager are not Mission Center's; to support the backend, see the
[Mission Center project](https://gitlab.com/mission-center-devs/mission-center) and
[Weblate](https://hosted.weblate.org/engage/mission-center/) for translations.

## License

This program is free software; you can redistribute it and/or modify it under the terms of the GNU General Public
License as published by the Free Software Foundation; either version 3 of the License, or (at your option) any later
version.

Please see COPYING file in the root of this repository for the complete license text. Alternatively see
[the official license](https://www.gnu.org/licenses/gpl-3.0.html) as written by the Free Software Foundation.

The bundled Selawik font is licensed under the SIL Open Font License 1.1, see `crates/mc-ui/assets/fonts/OFL.txt`; if
Segoe UI is installed, it is used instead. Windows is a trademark of Microsoft Corporation; Task Manager is not
affiliated with or endorsed by Microsoft, and contains no Microsoft code, fonts or image files.

The application icon (`data/icons/hicolor/*/apps/io.github.taskmanager.TaskManager.png`) is a fan-made redraw
provided by the project owner; it is not part of the clean-room interface work.

## Code of Conduct

Task Manager follows the GNOME Code of Conduct, like Mission Center. All communications in project spaces are expected to
follow it.
