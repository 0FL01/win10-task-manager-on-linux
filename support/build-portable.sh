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
    python3 pkg-config libdbus-1-dev libdrm-dev libgbm-dev libudev-dev desktop-file-utils patch

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --profile=minimal --default-toolchain="$SCRIPT_RUST_VERSION" -y

mkdir -p "$OUT_PATH" "$DEPS_PATH"
# The snap job copies `dependencies/*`; keep the directory non-empty
echo "No bundled UI toolkit is required by the egui front-end." > "$DEPS_PATH/README"

cd "$SRC_PATH"
cargo build --release -p taskmanager-app
cargo xtask install --prefix /usr --destdir "$OUT_PATH"
