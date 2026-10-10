<img align="left"  src="https://gitlab.com/mission-center-devs/mission-center/-/raw/main/data/icons/hicolor/scalable/apps/io.missioncenter.MissionCenter.svg" alt="drawing" width="64"/> 

# Mission Center

Monitor your CPU, Memory, Disk, Network and GPU usage with [Mission Center](https://missioncenter.io/)

![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0001-cpu.png)

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
* Supports a minified summary view for simple monitoring
* Use hardware accelerated rendering for all the graphs in an effort to reduce CPU and overall resource usage
* Uses GTK4 and Libadwaita
* Written in Rust

## Limitations

Please note there is ongoing work to overcome all of these.

* Per-process network monitoring requires manual setup,
  see [this page](https://gitlab.com/mission-center-devs/mission-center/-/wikis/Home/Nethogs) for more information.
* Intel GPU monitoring is only supported for Broadwell and later GPUs; and does not support VRAM, power, or temperature
  monitoring.
* When using Linux Mint/Cinnamon, launched applications may not show up in the "Applications" section. (Upstream
  issue: https://github.com/linuxmint/cinnamon/issues/12015)

Please also note that as Mission Center is a libadwaita application, it will not follow system-defined stylesheets
(themes).

## Installing

[AppImage (x86_64)](https://gitlab.com/mission-center-devs/mission-center/-/jobs/15536631699/artifacts/raw/MissionCenter-1.2.0-x86_64.AppImage)  
[AppImage (ARM64)](https://gitlab.com/mission-center-devs/mission-center/-/jobs/15536631700/artifacts/raw/MissionCenter-1.2.0-aarch64.AppImage)  
[Flatpak](https://flathub.org/apps/io.missioncenter.MissionCenter)  
[Snap](https://snapcraft.io/mission-center)

Also available from https://portable-linux-apps.github.io/apps/mission-center.html

Might also be available in your distribution's repository:  
[![](https://repology.org/badge/vertical-allrepos/mission-center.svg)](https://repology.org/project/mission-center/versions)

Installed by default in:

* [Aurora](https://getaurora.dev/)
* [Bazzite](https://bazzite.gg)
* [Bluefin](https://projectbluefin.io/)
* [DeLinuxCo](https://www.delinuxco.com/)

Source code is available at [GitLab](https://gitlab.com/mission-center-devs/mission-center)

## Screenshots

<details>
  <summary><b>Show</b></summary>

  <br/>

*CPU view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0001-cpu.png)

*Memory view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0002-memory.png)

*Disk view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0003-disk.png)

*Ethernet and Wi-Fi view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0004-ethernet.png)
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0005-wifi.png)

*GPU view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0006-gpu.png)

*Fan view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0007-fan.png)

*Apps page*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0008-apps.png)

*Services page*
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0008-services.png)

*Dark mode*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0009-cpu-dark.png)
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0010-disk-dark.png)

  </details>

## Building and running

### Building - Native

**Requirements:**

| Dependency                   | Comment                    | Minimum Version |
|------------------------------|----------------------------|----------------:|
| Meson                        |                            |           1.0.2 |
| Rust                         |                            |            1.90 |
| CMake                        |                            |            3.15 |
| Python3                      |                            |            3.10 |
| Python GObject Introspection | Used by Blueprint Compiler |             N/A |
| DRM development libraries    |                            |             N/A |
| GBM development libraries    |                            |             N/A |
| udev development libraries   |                            |             N/A |
| GTK 4                        |                            |            4.22 |
| libadwaita                   |                            |             1.9 |

**Build instructions**

Note: A native build requires, at least, GTK 4.22 and libadwaita 1.9. That means ArchLinux >= 20260501, Fedora >= 44,
Ubuntu >= 26.04.

```bash
# Avoid using "--depth=1" flag as it will not include the submodules which will result in failed build
git clone https://gitlab.com/mission-center-devs/mission-center --recursive
cd mission-center

# On Ubuntu 26.04 all dependencies, except for the Rust toolchain, can be installed with:
sudo apt install build-essential cmake curl desktop-file-utils gettext git libadwaita-1-dev libdbus-1-dev libdrm-dev libgbm-dev libudev-dev meson pkg-config protobuf-compiler python3-gi python3-pip

BUILD_ROOT="$(pwd)/build-meson-debug"

meson setup "$BUILD_ROOT" -Dbuildtype=debug # Alternatively pass `-Dbuildtype=release` for a release build
ninja -C "$BUILD_ROOT"
```

If you want to run the application from the build directory (for development or debugging) some set up is required:

```bash
export PATH="$BUILD_ROOT/subprojects/magpie/src:$PATH"
export GSETTINGS_SCHEMA_DIR="$BUILD_ROOT/data"
export MC_MAGPIE_HW_DB="$BUILD_ROOT/subprojects/magpie/platform-linux/hwdb/hw.db"
export MC_RESOURCE_DIR="$BUILD_ROOT/resources"

glib-compile-schemas --strict "$(pwd)/data" && mv "$(pwd)/data/gschemas.compiled" "$BUILD_ROOT/data/"
```

And then to run the app:

```bash
"$BUILD_ROOT/src/missioncenter"
```

If you want to install the app just run:

```bash
ninja -C $BUILD_ROOT install
```

And run the app from your launcher or from the command-line:

```bash
missioncenter
```

### Building - AppImage

**Note:** Creating a fully distro-agnostic AppImage requires an running the `support/create-appimage.sh` script in an
ArchLinux container. The process described here is just informative.

```bash
# On Ubuntu 26.04 all dependencies, except for the Rust toolchain, can be installed with:
sudo apt install build-essential cmake curl desktop-file-utils gettext git libadwaita-1-dev libdbus-1-dev libdrm-dev libgbm-dev libudev-dev meson pkg-config protobuf-compiler python3-gi python3-pip

meson setup _build -Dbuildtype=debug # Alternatively pass `-Dbuildtype=release` for a release build
ninja -C _build
```

And then build the AppImage:

```bash
meson install -C _build --no-rebuild --destdir "AppDir"

appimage-builder --appdir _build/AppDir/ 
```

And run the app from the command-line:

```bash
./"Mission Center-${version}-${arch}.AppImage"
```

### Building - Flatpak

**Requirements:**

* Flatpak
* Flatpak-Builder

Add the `flathub` repo is not already present:

```bash
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

Install the required Flatpak runtimes and SDKs:

```bash
flatpak install -y \
    org.freedesktop.Platform//25.08 \
    org.freedesktop.Sdk//25.08 \
    org.gnome.Platform//50 \
    org.gnome.Sdk//50
```

Finally build a Flatpak package:

```bash
cd flatpak
flatpak-builder --repo=repo --ccache --force-clean build io.missioncenter.MissionCenter.json
flatpak build-bundle repo missioncenter.flatpak io.missioncenter.MissionCenter
```

Install the package:

```bash
flatpak uninstall -y io.missioncenter.MissionCenter
flatpak install -y missioncenter.flatpak
```

Run the app from your launcher or from the command-line:

```bash
flatpak run io.missioncenter.MissionCenter
```

## Contributing

### Issues

Report issues to GitLab [issue tracking system](https://gitlab.com/mission-center-devs/mission-center/-/issues).

### Discord

Join [the Discord server](https://discord.gg/RG7QTeB9yk) and let's talk about what you think is missing or can be
improved.

### Translations

If you'd like to help translating Mission Center into your language, please head over
to [Weblate](https://hosted.weblate.org/engage/mission-center/).

<a href="https://hosted.weblate.org/engage/mission-center/">
  <img src="https://hosted.weblate.org/widgets/mission-center/-/mission-center/multi-auto.svg" alt="Translation status" />
</a>

### Monetary Contributions

Instead of donating to Mission Center directly, consider supporting the projects that Mission Center depends on:

* [GNOME](https://donate.gnome.org/)
* [NNG](https://github.com/gdamore)
* [NVTOP](https://github.com/Syllo/nvtop)
* [Rust Foundation](https://rustfoundation.org/get-involved/)

If you'd, still, like to support the development of Mission Center financially, please visit
our [Open Collective page](https://opencollective.com/mission-center).

Comments, suggestions, bug reports and contributions are welcome.

## License

This program is free software; you can redistribute it and/or modify it under the terms of the GNU General Public
License as published by the Free Software Foundation; either version 3 of the License, or (at your option) any later
version.

Please see COPYING file in the root of this repository for the complete license text. Alternatively see
[the official license](https://www.gnu.org/licenses/gpl-3.0.html) as written by the Free Software Foundation.

## Code of Conduct

Mission Center follows the GNOME Code of Conduct. All communications in project spaces are expected to follow it.