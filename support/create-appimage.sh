#!/bin/sh

set -eux

export SRC_PATH=${SRC_PATH:-"/hostfs"}

unset DISPLAY
unset WAYLAND_DISPLAY

ARCH="$(uname -m)"
SHARUN="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools/quick-sharun.sh"
EXTRA_PACKAGES="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools/get-debloated-pkgs.sh"

pacman -Syu --noconfirm base-devel zsync wget python patchelf libxkbcommon wayland libglvnd vulkan-icd-loader mesa

wget "$SHARUN" -O ./quick-sharun
chmod +x ./quick-sharun

wget "$EXTRA_PACKAGES" -O ./get-debloated-pkgs
chmod +x ./get-debloated-pkgs

export VERSION=$(grep -oP 'version = \K.*' "$SRC_PATH/Cargo.toml" | head -n1 | tr -d '"')
export ICON=/usr/share/icons/hicolor/256x256/apps/io.github.taskmanager.TaskManager.png
export DESKTOP=/usr/share/applications/io.github.taskmanager.TaskManager.desktop
export OUTPATH="$SRC_PATH/"
export OUTNAME=TaskManager-"$VERSION"-"$ARCH".AppImage

./get-debloated-pkgs --add-common --prefer-nano

# The egui front-end loads Wayland/X11, xkbcommon and the GPU drivers at run time (dlopen);
# ask quick-sharun to bundle the OpenGL and Vulkan stacks.
export DEPLOY_OPENGL=1
export DEPLOY_VULKAN=1

export APPDIR=${APPDIR:-"$PWD/AppDir"}
GPU_LIBS=/usr/lib/taskmanager/gpu
python3 "$SRC_PATH/support/bundle-gpu-libs.py" --verify --dest "$GPU_LIBS" \
    --binary /usr/bin/missioncenter-magpie
# Explicit library arguments include the dlopened AMD ABI and its full closure.
# Passing the directory also carries license notices and the source manifest.
./quick-sharun /usr/bin/taskmanager /usr/bin/missioncenter-magpie "$GPU_LIBS"/*.so.* "$GPU_LIBS"
# sharun relocates the real binaries into shared/bin (shared/lib -> ../lib).
# Reapply private RUNPATHs and verify build IDs, ABI and notices after relocation.
python3 "$SRC_PATH/support/bundle-gpu-libs.py" --relocate-manifest "$GPU_LIBS/manifest.json" \
    --dest "$APPDIR/lib/taskmanager/gpu" \
    --binary "$APPDIR/shared/bin/missioncenter-magpie"
"$APPDIR/bin/missioncenter-magpie" --test
./quick-sharun --make-appimage
