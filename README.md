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

## Limitations

Please note there is ongoing work to overcome all of these.

* Per-process network monitoring requires manual setup,
  see [this page](https://gitlab.com/mission-center-devs/mission-center/-/wikis/Home/Nethogs) for more information.
* Intel GPU monitoring is only supported for Broadwell and later GPUs; and does not support VRAM, power, or temperature
  monitoring.
* When using Linux Mint/Cinnamon, launched applications may not show up in the "Applications" section. (Upstream
  issue: https://github.com/linuxmint/cinnamon/issues/12015)

Please also note that Task Manager draws its own widgets (egui) and its own window decorations in a fixed light style;
it does not follow the system light/dark preference or system-defined stylesheets (themes).

## Installing

Task Manager is built from source (see below). On Fedora, `cargo xtask rpm` builds an RPM package
(`taskmanager`, which replaces earlier builds of this project packaged as `missioncenter`).

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
| Vulkan or OpenGL driver   | run time only (wgpu or glow renderer)           |             N/A |

No UI toolkit development packages are needed: the interface is written with egui and talks to Wayland/X11 directly.

**Build instructions**

```bash
git clone --recurse-submodules https://github.com/0FL01/win10-task-manager-on-linux.git
cd win10-task-manager-on-linux

# On Ubuntu 26.04 all dependencies, except for the Rust toolchain, can be installed with:
sudo apt install build-essential cmake curl desktop-file-utils gettext git libdbus-1-dev libdrm-dev libgbm-dev libudev-dev pkg-config python3

# Fedora alternative:
# sudo dnf install gcc gcc-c++ cmake curl desktop-file-utils gettext git libdrm-devel mesa-libgbm-devel systemd-devel pkgconf-pkg-config python3 patch

cargo build --release -p taskmanager-app
```

To run the application from the source tree (builds the magpie daemon and the translations on first use):

```bash
cargo xtask run-dev
```

Useful flags of the `taskmanager` binary:

* `--renderer wgpu|glow` — select the renderer; glow (OpenGL) is the default, both are compiled in by default builds
  (`cargo build -p taskmanager-app --no-default-features --features glow` produces an OpenGL-only build)
* `--synthetic N` — run on generated data with `N` processes, without the daemon
* `--perf-overlay` (or F12) — show frame and UI pass timings

Environment variables: `MC_LOCALEDIR` (translations directory), `MC_MAGPIE_HW_DB` (hardware database of the daemon),
`RUST_LOG` (logging).

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
`flatpak/cargo-sources-magpie.json` corresponds to the pinned magpie submodule.

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
