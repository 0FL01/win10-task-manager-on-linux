#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || $EUID -ne 0 ]]; then
    printf 'Usage: sudo %s DISTRO\n' "${0##*/}" >&2
    printf 'DISTRO: fedora44 | opensuse-tumbleweed | arch | debian13 | ubuntu26.04\n' >&2
    exit 2
fi

distro=$1
case $distro in
    fedora44)
        dnf install -y \
            binutils cmake curl dbus-devel desktop-file-utils dirmngr gettext git gcc gcc-c++ gnupg2 \
            hicolor-icon-theme libX11-devel libXcursor-devel libXi-devel libdrm-devel mesa-libgbm-devel libxcb-devel \
            libxkbcommon-devel libxkbcommon-x11-devel libglvnd-devel patch \
            pkgconf-pkg-config python3 python3-pip rpm-build shadow-utils systemd-devel \
            vulkan-loader-devel wayland-devel
        ;;
    opensuse-tumbleweed)
        zypper --non-interactive install --no-recommends \
            binutils cmake curl dbus-1-devel desktop-file-utils dirmngr gcc gcc-c++ gettext-tools git gpg2 \
            hicolor-icon-theme libX11-devel libXcursor-devel libXi-devel libdrm-devel libgbm-devel libxcb-devel \
            libxkbcommon-devel libxkbcommon-x11-devel Mesa-libEGL-devel \
            Mesa-libGL-devel libvulkan_intel libvulkan_lvp libvulkan_radeon \
            patch pkg-config python3 python3-pip \
            rpm-build shadow systemd-devel vulkan-devel wayland-devel
        ;;
    arch)
        pacman -Syu --noconfirm
        pacman -S --needed --noconfirm \
            base-devel binutils cmake curl dbus desktop-file-utils gettext git hicolor-icon-theme \
            gnupg libdrm libglvnd libx11 libxcb libxcursor libxi libxkbcommon libxkbcommon-x11 licenses mesa \
            patch pkgconf python python-pip shadow systemd-libs \
            vulkan-icd-loader wayland
        ;;
    debian13|ubuntu26.04)
        apt-get update
        DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
            binutils build-essential cmake curl desktop-file-utils gettext git libdbus-1-dev \
            dirmngr gnupg hicolor-icon-theme libdrm-dev libegl-dev libgbm-dev libgl-dev libxcursor-dev libxi-dev \
            libudev-dev libvulkan-dev libwayland-dev libx11-dev libxcb1-dev \
            libxkbcommon-dev libxkbcommon-x11-dev mesa-vulkan-drivers patch \
            passwd pkg-config python3 python3-pip python3-venv
        ;;
    *)
        printf 'Unsupported native build target: %s\n' "$distro" >&2
        exit 2
        ;;
esac

tools_venv=${TASKMANAGER_BUILD_TOOLS:-/opt/taskmanager-build-tools}
python3 -m venv "$tools_venv"
PIP_DISABLE_PIP_VERSION_CHECK=1 "$tools_venv/bin/python" -m pip install \
    --no-cache-dir 'patchelf==0.19.1.0'
printf 'Build tools installed; set PATCHELF=%s/bin/patchelf\n' "$tools_venv"
