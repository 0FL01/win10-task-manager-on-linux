#!/bin/bash

# Builds a portable install tree of Task Manager (egui UI + magpie daemon) for the Snap and
# AppImage packages. Unlike the former GTK build, no UI toolkit has to be built from source:
# the egui front-end only needs the system's Wayland/X11 client libraries, xkbcommon and a
# Vulkan or OpenGL driver at run time.

set -eux

export SRC_PATH=${SRC_PATH:-"/hostfs"}
export OUT_PATH=${OUT_PATH:-"${SRC_PATH}/target/portable"}
export DEPS_PATH="$OUT_PATH/dependencies"

export HOME=${HOME:-/root}
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin"

SCRIPT_RUST_VERSION=1.96.1

apt-get update
DEBIAN_FRONTEND=noninteractive apt-get install -y tzdata curl build-essential cmake gettext git \
    python3 python3-venv pkg-config libdbus-1-dev libdrm-dev libdrm-amdgpu1 libgbm-dev libudev-dev desktop-file-utils patch

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --profile=minimal --default-toolchain="$SCRIPT_RUST_VERSION" -y

mkdir -p "$OUT_PATH" "$DEPS_PATH"
# The snap job copies `dependencies/*`; keep the directory non-empty
echo "No bundled UI toolkit is required by the egui front-end." > "$DEPS_PATH/README"

cd "$SRC_PATH"
# A pinned builder-only tool supports modern ELF layouts even on older builders.
# Keep the venv outside the portable tree and its staged dependency directory.
python3 -m venv --clear target/gpu-patchelf-venv
target/gpu-patchelf-venv/bin/pip install patchelf==0.19.1.0
export PATCHELF="$SRC_PATH/target/gpu-patchelf-venv/bin/patchelf"
cargo build --release -p taskmanager-app
cargo xtask install --prefix /usr --destdir "$OUT_PATH"
python3 support/bundle-gpu-libs.py --verify --dest "$OUT_PATH/usr/lib/taskmanager/gpu" \
    --binary "$OUT_PATH/usr/bin/missioncenter-magpie"
"$OUT_PATH/usr/bin/missioncenter-magpie" --test
