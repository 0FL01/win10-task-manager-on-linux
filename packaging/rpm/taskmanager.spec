# RPM for Fedora built from a staged install tree (`cargo xtask rpm` creates the tree with
# `cargo xtask install` and passes it as the "stagedir" macro).

%global debug_package %{nil}
%global __strip /bin/true
# Exact SONAMEs (and symbol-version suffixes) from this artifact's closure.
# Keep every other native dependency, including OS ABI and installed drivers.
%global __requires_exclude %(cat "%{stagedir}/usr/lib/taskmanager/gpu/rpm-requires-filter.txt")
%global __provides_exclude_from ^%{_prefix}/lib/taskmanager/gpu/.*$

Name:           taskmanager
Version:        %{?mc_version}%{!?mc_version:1.2.0}
Release:        %{?mc_release}%{!?mc_release:13}%{?dist}
Summary:        Task Manager: processes, performance and services
# Selawik, libdrm, libudev and the Fedora libgcc runtime closure.
License:        GPL-3.0-or-later AND OFL-1.1 AND MIT AND LGPL-2.1-or-later AND (GPL-3.0-or-later WITH GCC-exception-3.1)
URL:            https://github.com/0FL01/win10-task-manager-on-linux

# Replaces the earlier build of this project, packaged as "missioncenter"
Obsoletes:      missioncenter < 1.2.0-8
# Installs the same statistics backend binary as Fedora's Mission Center package
Conflicts:      mission-center

BuildRequires:  python3
BuildRequires:  binutils

# Loaded at run time (dlopen) by the egui/eframe front-end
Requires:       libwayland-client
Requires:       libxkbcommon
Requires:       vulkan-loader
Requires:       mesa-vulkan-drivers
Requires:       mesa-libEGL
Requires:       hicolor-icon-theme
# Per-process network usage (enabled from Options → Enable per-process network usage…)
Recommends:     nethogs

%description
Task Manager shows processes, performance, details and services: per-process CPU, memory, disk
and network usage, live graphs for CPU, memory, disks, network, GPU, fans and batteries, and
systemd services. Its pure-Rust user interface (egui/eframe) is written from scratch.

Task Manager is based on Mission Center: it uses the Mission Center statistics backend
(magpie, installed as missioncenter-magpie) and code from the Mission Center project by the
Mission Center Developers (https://missioncenter.io).

%prep
test -d "%{stagedir}/usr/bin" || { echo "stagedir is not set or empty: %{stagedir}"; exit 1; }
python3 "%{srcroot}/support/bundle-gpu-libs.py" --verify --dest "%{stagedir}/usr/lib/taskmanager/gpu" --binary "%{stagedir}/usr/bin/missioncenter-magpie"
"%{stagedir}/usr/bin/missioncenter-magpie" --test
cp -p "%{srcroot}/COPYING" .
cp -p "%{srcroot}/crates/mc-ui/assets/fonts/OFL.txt" Selawik-OFL.txt

%build

%install
mkdir -p %{buildroot}
cp -a %{stagedir}/. %{buildroot}/

%files
%license COPYING Selawik-OFL.txt
%{_bindir}/taskmanager
%{_bindir}/missioncenter-magpie
%dir %{_prefix}/lib/taskmanager
%dir %{_prefix}/lib/taskmanager/gpu
%{_prefix}/lib/taskmanager/gpu/*.so.*
%{_prefix}/lib/taskmanager/gpu/manifest.json
%{_prefix}/lib/taskmanager/gpu/rpm-requires-filter.txt
%license %{_prefix}/lib/taskmanager/gpu/licenses/
%{_datadir}/missioncenter/hw.db
%{_datadir}/applications/io.github.taskmanager.TaskManager.desktop
%{_datadir}/metainfo/io.github.taskmanager.TaskManager.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/io.github.taskmanager.TaskManager.png
%{_datadir}/locale/*/LC_MESSAGES/taskmanager.mo

%changelog
* Sun Oct 11 2026 Task Manager Developers - 1.2.0-13
- Windows names for amdgpu engines, hardware engines listed before first use, unified VCN
  video shown as Video Encode/Decode.

* Sun Oct 11 2026 Task Manager Developers - 1.2.0-12
- Size menus to their content so submenus open beside their parent instead of over it.

* Sun Oct 11 2026 Task Manager Developers - 1.2.0-11
- Hide virtual network adapters (Docker, bridges, veth, Multipass) by default; View menu
  toggle "Show virtual network adapters".

* Sun Oct 11 2026 Task Manager Developers - 1.2.0-10
- Show partial GPU utilization as a plain percentage; the reason stays in the tooltip.

* Sun Oct 11 2026 Task Manager Developers - 1.2.0-9
- Bundle the open GPU userspace ABI privately, with relative RUNPATHs, notices,
  source/checksum manifests and a device-independent daemon bootstrap check.

* Sat Oct 10 2026 Task Manager Developers - 1.2.0-8
- Renamed to Task Manager (binary taskmanager, app id io.github.taskmanager.TaskManager,
  settings in ~/.config/taskmanager, copied once from ~/.config/missioncenter); new icon.
  The statistics backend remains Mission Center's magpie.

* Sat Oct 10 2026 Mission Center Developers - 1.2.0-7
- Window size is kept across restarts on Wayland

* Sat Oct 10 2026 Mission Center Developers - 1.2.0-6
- Interface state is kept across restarts: column widths and order (drag headers), sorting,
  collapsed groups, Always on top, Fewer details, Paused

* Sat Oct 10 2026 Mission Center Developers - 1.2.0-5
- Per-process network usage without nethogs or privileges (TCP socket counters)

* Sat Oct 10 2026 Mission Center Developers - 1.2.0-4
- Network column: total from the network interfaces; "—" and a hint while per-process data
  is unavailable; Options → Enable per-process network usage…; recommends nethogs

* Sat Oct 10 2026 Mission Center Developers - 1.2.0-3
- Performance tab: the device list separator can be dragged to widen the list

* Sat Oct 10 2026 Mission Center Developers - 1.2.0-2
- Interface name "Task Manager"; older settings get the Win10 window size and graph defaults
- Minimum window size; long texts are shortened with an ellipsis

* Sat Oct 10 2026 Mission Center Developers - 1.2.0-1
- egui/eframe user interface instead of GTK4/libadwaita
- Windows 10 Task Manager style interface (clean room), bundled Selawik font
