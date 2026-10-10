# RPM for Fedora built from a staged install tree (`cargo xtask rpm` creates the tree with
# `cargo xtask install` and passes it as the "stagedir" macro).

%global debug_package %{nil}
%global __strip /bin/true

Name:           taskmanager
Version:        %{?mc_version}%{!?mc_version:1.2.0}
Release:        %{?mc_release}%{!?mc_release:8}%{?dist}
Summary:        Task Manager: processes, performance and services
# The binary embeds the Selawik font (OFL-1.1)
License:        GPL-3.0-or-later AND OFL-1.1
URL:            https://github.com/0FL01/win10-task-manager-on-linux

# Replaces the earlier build of this project, packaged as "missioncenter"
Obsoletes:      missioncenter < 1.2.0-8
# Installs the same statistics backend binary as Fedora's Mission Center package
Conflicts:      mission-center

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
%{_datadir}/missioncenter/hw.db
%{_datadir}/applications/io.github.taskmanager.TaskManager.desktop
%{_datadir}/metainfo/io.github.taskmanager.TaskManager.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/io.github.taskmanager.TaskManager.png
%{_datadir}/locale/*/LC_MESSAGES/taskmanager.mo

%changelog
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
