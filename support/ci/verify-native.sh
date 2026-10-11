#!/usr/bin/env bash
set -euo pipefail

usage() {
    printf 'Usage: %s DISTRO PACKAGE UPSTREAM_VERSION\n' "${0##*/}" >&2
}
if [[ $# -ne 3 ]]; then
    usage
    exit 2
fi
distro=$1
package=$(realpath "$2")
version=$3

case $distro in
    fedora44)
        dnf install -y "$package"
        package_version=$(rpm -q --qf '%{VERSION}-%{RELEASE}' taskmanager)
        expected_version="${version}-1.fc44"
        ;;
    opensuse-tumbleweed)
        zypper --non-interactive install "$package"
        package_version=$(rpm -q --qf '%{VERSION}-%{RELEASE}' taskmanager)
        expected_version="${version}-1.tw"
        ;;
    debian13|ubuntu26.04)
        apt-get update
        DEBIAN_FRONTEND=noninteractive apt-get install -y "$package"
        package_version=$(dpkg-query -W -f='${Version}' taskmanager)
        if [[ $distro == debian13 ]]; then
            expected_version="${version}-1+deb13"
        else
            expected_version="${version}-1+ubuntu26.04"
        fi
        ;;
    arch)
        pacman -Syu --noconfirm
        pacman -U --noconfirm "$package"
        package_version=$(pacman -Q taskmanager | awk '{print $2}')
        expected_version="${version}-1"
        ;;
    *) usage; exit 2 ;;
esac
if [[ $package_version != "$expected_version" ]]; then
    printf 'Installed package version %s differs from expected %s\n' "$package_version" "$expected_version" >&2
    exit 1
fi

test -x /usr/bin/taskmanager
test -x /usr/bin/missioncenter-magpie
test -s /usr/share/missioncenter/hw.db
test -s /usr/share/applications/io.github.taskmanager.TaskManager.desktop
grep -Fq "<release version=\"$version\"" \
    /usr/share/metainfo/io.github.taskmanager.TaskManager.metainfo.xml
test -d /usr/share/locale
test -n "$(find /usr/share/locale -type f -name taskmanager.mo -print -quit)"
test -d /usr/share/licenses/taskmanager || test -d /usr/share/doc/taskmanager

# Check the app's dlopen dependencies before installing extra smoke-test libraries.
for soname in libX11.so.6 libXcursor.so.1 libX11-xcb.so.1 libXi.so.6 libxcb.so.1 \
    libxkbcommon.so.0 libxkbcommon-x11.so.0 libwayland-client.so.0 libwayland-egl.so.1 \
    libGL.so.1 libEGL.so.1 libvulkan.so.1; do
    if ! ldconfig -p | grep -Fq "$soname"; then
        printf 'Missing runtime dlopen dependency: %s\n' "$soname" >&2
        exit 1
    fi
done

case $distro in
    fedora44)
        dnf install -y binutils dbus-x11 python3 xorg-x11-server-Xvfb xorg-x11-xauth mesa-dri-drivers mesa-vulkan-drivers
        ;;
    opensuse-tumbleweed)
        zypper --non-interactive install binutils dbus-1 python3 xorg-x11-server xauth Mesa-dri libvulkan_lvp
        ;;
    arch)
        pacman -S --needed --noconfirm binutils dbus python xorg-server-xvfb xorg-xauth mesa vulkan-swrast
        ;;
    debian13|ubuntu26.04)
        apt-get update
        DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
            binutils dbus-x11 libgl1-mesa-dri mesa-vulkan-drivers python3 xauth xvfb
        ;;
esac

python3 /workspace/support/bundle-gpu-libs.py \
    --verify --dest /usr/lib/taskmanager/gpu --binary /usr/bin/missioncenter-magpie
/usr/bin/missioncenter-magpie --test

tmp=$(mktemp -d)
trap 'rm -rf -- "$tmp"' EXIT
mkdir -p "$tmp/config" "$tmp/cache" "$tmp/data"

check_stats() {
    local file=$1
    test -s "$file"
    awk -F= '
        $1 == "frames" && $2 > 0 { frames = 1 }
        $1 == "passes" && $2 > 0 { passes = 1 }
        $1 == "readings_applied" && $2 > 0 { readings = 1 }
        END { exit !(frames && passes && readings) }
    ' "$file"
}

run_gui() {
    local renderer=$1
    local mode=$2
    local label="$renderer-$mode"
    local -a args=(--renderer "$renderer" --no-config --bench-seconds 8 --stats-file "$tmp/$label.stats")
    if [[ $mode == synthetic ]]; then
        args+=(--synthetic 300)
    fi
    if ! timeout --signal=TERM --kill-after=5s 45s \
        xvfb-run -a dbus-run-session -- env \
            HOME="$tmp" XDG_CONFIG_HOME="$tmp/config" XDG_CACHE_HOME="$tmp/cache" \
            XDG_DATA_HOME="$tmp/data" MC_MAGPIE_HW_DB=/usr/share/missioncenter/hw.db \
            LIBGL_ALWAYS_SOFTWARE=1 RUST_LOG=debug \
            /usr/bin/taskmanager "${args[@]}" >"$tmp/$label.log" 2>&1; then
        printf 'GUI smoke failed: %s\n' "$label" >&2
        cat "$tmp/$label.log" >&2
        return 1
    fi
    check_stats "$tmp/$label.stats" || {
        printf 'GUI stats are missing or incomplete: %s\n' "$label" >&2
        cat "$tmp/$label.log" >&2
        return 1
    }
    if [[ $mode == real ]]; then
        grep -Fq 'PERF: CPU information loaded and serialized' "$tmp/$label.log"
        grep -Fq 'PERF: ProcessMap (processes) loaded and serialized' "$tmp/$label.log"
        if grep -Eq 'Error while decoding response|Failed to send request:|Failed to read response:' "$tmp/$label.log"; then
            printf 'Installed app/daemon IPC error in %s\n' "$label" >&2
            cat "$tmp/$label.log" >&2
            return 1
        fi
    fi
}

run_gui glow synthetic
run_gui wgpu synthetic
run_gui glow real
printf 'Verified installed %s package %s (private ABI, synthetic renderers, real IPC)\n' \
    "$distro" "$package_version"
