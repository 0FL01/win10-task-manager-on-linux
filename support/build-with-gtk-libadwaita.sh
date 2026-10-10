#!/bin/bash

# shellcheck disable=SC2164

set -eux

export SRC_PATH=${SRC_PATH:-"/hostfs"}
export OUT_PATH=${OUT_PATH:-"/hostfs/_build/portable"}
export DEPS_PATH="$OUT_PATH/dependencies"

export HOME=/root
export TERM=xterm
export PATH="$HOME/llvm-tooling/bin:$HOME/.cargo/bin:/usr/local/bin:/usr/local/sbin:/usr/bin:/usr/sbin:/bin:/sbin:"

SCRIPT_RUST_VESION=1.96.1
SCRIPT_CLANG_VERSION=22.1.8

apt-get update

ln -sf /usr/share/zoneinfo/Etc/UTC /etc/localtime
DEBIAN_FRONTEND=noninteractive apt-get install -y tzdata
dpkg-reconfigure --frontend noninteractive tzdata

apt-get install -y curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --profile=minimal --default-toolchain="$SCRIPT_RUST_VESION" -y

apt install -y build-essential curl desktop-file-utils bison flex glslc gettext git libadwaita-1-dev libdbus-1-dev libdrm-dev libgbm-dev libgraphviz-dev libssl-dev libudev-dev libxml2-dev pkg-config python3-gi python3-pip zstd

pip3 install --break-system-packages cmake meson ninja

mkdir -p "$OUT_PATH" && cd "$OUT_PATH"

# https://www.linuxfromscratch.org/blfs/view/stable/general/glib2.html
# --------------------------------------------------------------------
GLIB_VER=2.88.2
GLIB_VER_MM=$(echo $GLIB_VER | cut -f1-2 -d'.')
# --------------------------------------------------------------------
rm -rf /usr/include/glib-2.0/
curl -LO https://download.gnome.org/sources/glib/$GLIB_VER_MM/glib-$GLIB_VER.tar.xz
tar xvf glib-$GLIB_VER.tar.xz
cd glib-$GLIB_VER
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release                 \
    -Dselinux=disabled                  \
    -Dglib_debug=disabled               \
    -Dglib_assert=false                 \
    -Dglib_checks=false                 \
    -Dtests=false                       \
    -Dman-pages=disabled
ninja && ninja install
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/general/gobject-introspection.html
# ------------------------------------------------------------------------------------
GOBJ_INTRSPEC_VER=1.86.0
GOBJ_INTRSPEC_VER_MM=$(echo $GOBJ_INTRSPEC_VER | cut -f1-2 -d'.')
# ------------------------------------------------------------------------------------
curl -LO https://download.gnome.org/sources/gobject-introspection/$GOBJ_INTRSPEC_VER_MM/gobject-introspection-$GOBJ_INTRSPEC_VER.tar.xz
tar xvf gobject-introspection-$GOBJ_INTRSPEC_VER.tar.xz
cd gobject-introspection-$GOBJ_INTRSPEC_VER
mkdir build && cd build
meson setup --prefix=/usr --libdir=/usr/lib/$(arch)-linux-gnu --buildtype=release
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../../ && rm -rf gobject-introspection-$GOBJ_INTRSPEC_VER*
cd $OUT_PATH

# Yes, compile it again because there is a circular dependency with `gobject-introspection`
# https://www.linuxfromscratch.org/blfs/view/stable/general/glib2.html
# --------------------------------------------------------------------
GLIB_VER=$GLIB_VER
GLIB_VER_MM=$GLIB_VER_MM
# --------------------------------------------------------------------
rm -rf /usr/include/glib-2.0/
cd glib-$GLIB_VER
cd build
ninja reconfigure
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../../ && rm -rf glib-$GLIB_VER*
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/general/freetype2.html
# -------------------------------------------------------------------
FREETYPE_VER=2.14.3
# -------------------------------------------------------------------
curl -LO https://downloads.sourceforge.net/freetype/freetype-$FREETYPE_VER.tar.xz
tar xvf freetype-$FREETYPE_VER.tar.xz
cd freetype-$FREETYPE_VER
sed -ri "s:.*(AUX_MODULES.*valid):\1:" modules.cfg
sed -r "s:.*(#.*SUBPIXEL_RENDERING) .*:\1:" -i include/freetype/config/ftoption.h
CFLAGS=-O2 ./configure                    \
    --prefix=/usr                         \
    --libdir=/usr/lib/$(arch)-linux-gnu   \
    --enable-freetype-config              \
    --without-harfbuzz                    \
    --disable-static
make -j4
make install && make DESTDIR="$DEPS_PATH" install
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/x/cairo.html
# --------------------------------------------------------------
CAIRO_VER=1.18.4
# --------------------------------------------------------------
curl -LO https://gitlab.freedesktop.org/cairo/cairo/-/archive/$CAIRO_VER/cairo-$CAIRO_VER.tar.bz2
tar xvf cairo-$CAIRO_VER.tar.bz2
cd cairo-$CAIRO_VER
sed -e "/@prefix@/a exec_prefix=@exec_prefix@" -i util/cairo-script/cairo-script-interpreter.pc.in
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release                 \
    -Dtests=disabled                    \
    -Dtee=disabled                      \
    -Dxcb=disabled                      \
    -Dxlib-xcb=enabled                  \
    -Dpng=enabled                       \
    -Dzlib=enabled
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../.. && rm -rf cairo-$CAIRO_VER*
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/general/wayland.html
# ----------------------------------------------------------------------
WAYLAND_VER_REL=1.24.0-1
WAYLAND_VER=$(echo $WAYLAND_VER_REL | cut -f1 -d'-')
# ----------------------------------------------------------------------
curl -LO https://launchpad.net/ubuntu/+archive/primary/+sourcefiles/wayland/$WAYLAND_VER_REL/wayland_$WAYLAND_VER.orig.tar.gz
tar xvf wayland_$WAYLAND_VER.orig.tar.gz
cd wayland-$WAYLAND_VER
mkdir build && cd build
meson setup --prefix=/usr --libdir=/usr/lib/$(arch)-linux-gnu --buildtype=release -Ddocumentation=false ..
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../../ && rm -rf wayland*
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/general/wayland-protocols.html
# --------------------------------------------------------------------------------
WAYLAND_PROTO_VER_REL=1.47-1
WAYLAND_PROTO_VER=$(echo $WAYLAND_PROTO_VER_REL | cut -f1 -d'-')
# ----------------------------------------------------------------------
curl -LO https://launchpad.net/ubuntu/+archive/primary/+sourcefiles/wayland-protocols/$WAYLAND_PROTO_VER_REL/wayland-protocols_$WAYLAND_PROTO_VER.orig.tar.xz
tar xvf wayland-protocols_$WAYLAND_PROTO_VER.orig.tar.xz
cd wayland-protocols-$WAYLAND_PROTO_VER
mkdir build && cd build
meson setup --prefix=/usr --libdir=/usr/lib/$(arch)-linux-gnu --buildtype=release ..
ninja && ninja install
cd ../../ && rm -rf wayland-protocols*
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/x/adwaita-icon-theme.html
# ---------------------------------------------------------------------------
ADW_ICONS_VER=50.0
ADW_ICONS_VER_MM=$(echo $ADW_ICONS_VER | cut -f1 -d'.')
# ---------------------------------------------------------------------------
rm -rf /usr/share/icons/Adwaita
curl -LO https://download.gnome.org/sources/adwaita-icon-theme/$ADW_ICONS_VER_MM/adwaita-icon-theme-$ADW_ICONS_VER.tar.xz
tar xvf adwaita-icon-theme-$ADW_ICONS_VER.tar.xz
cd adwaita-icon-theme-$ADW_ICONS_VER
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../../ && rm -rf adwaita-icon-theme-$ADW_ICONS_VER*
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/general/harfbuzz.html
# -----------------------------------------------------------------------
HARFBUZZ_VER=14.1.0
# -----------------------------------------------------------------------
curl -LO https://github.com/harfbuzz/harfbuzz/releases/download/$HARFBUZZ_VER/harfbuzz-$HARFBUZZ_VER.tar.xz
tar xvf harfbuzz-$HARFBUZZ_VER.tar.xz
cd harfbuzz-$HARFBUZZ_VER
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release                 \
    -Dgraphite2=disabled                \
    -Dtests=disabled
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd $OUT_PATH && rm -rf harfbuzz-$HARFBUZZ_VER*
# FreeType and Harfbuzz depend on each other, so we need to compile FreeType again
cd freetype-$FREETYPE_VER
sed -ri "s:.*(AUX_MODULES.*valid):\1:" modules.cfg
sed -r "s:.*(#.*SUBPIXEL_RENDERING) .*:\1:" -i include/freetype/config/ftoption.h
CFLAGS=-O2 ./configure                    \
    --prefix=/usr                         \
    --libdir=/usr/lib/$(arch)-linux-gnu   \
    --enable-freetype-config              \
    --disable-static
make -j4
make install && make DESTDIR="$DEPS_PATH" install
cd ../ && rm -rf freetype-$FREETYPE_VER*
cd $OUT_PATH

# Fontconfig
# ------------------------------------------------------------------
FONTCONFIG_VER=2.17.1
# ------------------------------------------------------------------
curl -LO https://gitlab.freedesktop.org/fontconfig/fontconfig/-/archive/$FONTCONFIG_VER/fontconfig-$FONTCONFIG_VER.tar.bz2
tar xvf fontconfig-$FONTCONFIG_VER.tar.bz2
cd fontconfig-$FONTCONFIG_VER
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release                 \
    -Ddoc=disabled                      \
    -Dtests=disabled                    \
    -Ddefault-sub-pixel-rendering='rgb'
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../../ && rm -rf fontconfig-$FONTCONFIG_VER*
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/x/pango.html
# ------------------------------------------------------------------
PANGO_VER=1.57.1
PANGO_VER_MM=$(echo $PANGO_VER | cut -f1-2 -d'.')
# ------------------------------------------------------------------
curl -LO https://download.gnome.org/sources/pango/$PANGO_VER_MM/pango-$PANGO_VER.tar.xz
tar xvf pango-$PANGO_VER.tar.xz
cd pango-$PANGO_VER
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release                 \
    -Dintrospection=enabled             \
    -Dbuild-testsuite=false             \
    -Dbuild-examples=false
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../../ && rm -rf pango-$PANGO_VER*
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/x/gtk4.html
# -------------------------------------------------------------
GTK_VER=4.22.4
GTK_VER_MM=$(echo $GTK_VER | cut -f1-2 -d'.')
# -------------------------------------------------------------
curl -LO https://download.gnome.org/sources/gtk/$GTK_VER_MM/gtk-$GTK_VER.tar.xz
tar xvf gtk-$GTK_VER.tar.xz
cd gtk-$GTK_VER
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release                 \
    -Dvulkan=enabled                    \
    -Dintrospection=enabled             \
    -Dbuild-examples=false              \
    -Dbuild-tests=false                 \
    -Dbuild-demos=false                 \
    -Dbuild-testsuite=false             \
    -Dbroadway-backend=false            \
    -Dmedia-gstreamer=disabled          \
    -Dprint-cups=disabled
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../../ && rm -rf gtk-$GTK_VER*
cd "$OUT_PATH"

# https://www.linuxfromscratch.org/blfs/view/stable/general/vala.html
# -------------------------------------------------------------------
VALA_VER=0.56.18
VALA_VER_MM=$(echo $VALA_VER | cut -f1-2 -d'.')
# -------------------------------------------------------------------
curl -LO https://download.gnome.org/sources/vala/$VALA_VER_MM/vala-$VALA_VER.tar.xz
tar xvf vala-$VALA_VER.tar.xz
cd vala-$VALA_VER
CFLAGS=-O2 ./configure --prefix=/usr --libdir=/usr/lib/$(arch)-linux-gnu
make -j4 && make install
cd ../ && rm -rf vala-$VALA_VER*
cd $OUT_PATH

# https://www.linuxfromscratch.org/blfs/view/stable/x/libadwaita.html
# -------------------------------------------------------------------
LIBADW_VER=1.9.1
LIBADW_VER_MM=$(echo $LIBADW_VER | cut -f1-2 -d'.')
# -------------------------------------------------------------------
curl -LO https://download.gnome.org/sources/libadwaita/$LIBADW_VER_MM/libadwaita-$LIBADW_VER.tar.xz
tar xvf libadwaita-$LIBADW_VER.tar.xz
cd libadwaita-$LIBADW_VER
# Patch for Yaru support
for f in $SRC_PATH/support/patches/libadwaita/*.patch; do
  patch -p1 < $f
done
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release                 \
    -Dtests=false                       \
    -Dexamples=false                    \
    -Dintrospection=enabled
ninja && ninja install && env DESTDIR="$DEPS_PATH" ninja install
cd ../../ && rm -rf libadwaita-$LIBADW_VER*
cd "$OUT_PATH"

# Blueprint Compiler
# -------------------------------------------------------------------
BP_CMP_VER=0.18.0
# -------------------------------------------------------------------
curl -L https://github.com/GNOME/blueprint-compiler/archive/refs/tags/$BP_CMP_VER.tar.gz --output blueprint-compiler-$BP_CMP_VER.tar.gz
tar xvf blueprint-compiler-*.tar.gz && rm blueprint-compiler-*.tar.gz
cd blueprint-compiler-$BP_CMP_VER
mkdir build && cd build
meson setup ..                          \
    --prefix=/usr                       \
    --libdir=/usr/lib/$(arch)-linux-gnu \
    --buildtype=release
ninja && ninja install
cd ../../ && rm -rf blueprint-compiler-*
cd "$OUT_PATH"

# LLVM tooling
# -------------------------------------------------------------------
LLVM_TOOLS_VERSION="$SCRIPT_CLANG_VERSION"
# -------------------------------------------------------------------
cd $HOME
curl -LO "https://missioncenter.io/build-tools/llvm-tooling-$(arch)-gnu-$LLVM_TOOLS_VERSION.tar.zst"
mkdir llvm-tooling && tar xvf llvm-tooling-$(arch)-gnu-$LLVM_TOOLS_VERSION.tar.zst -C llvm-tooling
rm "llvm-tooling-$(arch)-gnu-$LLVM_TOOLS_VERSION.tar.zst"

rm llvm-tooling/lib/$(arch)-unknown-linux-gnu/crtbeginS.o
rm llvm-tooling/lib/$(arch)-unknown-linux-gnu/crtendS.o
rm llvm-tooling/lib/$(arch)-unknown-linux-gnu/libgcc.a

ln -sf $HOME/llvm-tooling/bin/clang   /usr/bin/cc
ln -sf $HOME/llvm-tooling/bin/ld.lld  /usr/bin/ld
ln -sf $HOME/llvm-tooling/bin/llvm-ar /usr/bin/ar
ln -sf $HOME/llvm-tooling/bin/llvm-as /usr/bin/as
ln -sf $HOME/llvm-tooling/lib/$(arch)-unknown-linux-gnu/libc++.so.1    /usr/lib/$(arch)-linux-gnu/
ln -sf $HOME/llvm-tooling/lib/$(arch)-unknown-linux-gnu/libc++abi.so.1 /usr/lib/$(arch)-linux-gnu/
ln -sf $HOME/llvm-tooling/lib/$(arch)-unknown-linux-gnu/libunwind.so.1 /usr/lib/$(arch)-linux-gnu/

# Mission Center and Co.
# -------------------------------------------------------------------
export CC=clang
export CXX=clang++
export CC_LD=lld
export CXX_LD=lld
export LDFLAGS=-lgcc

cd "$SRC_PATH"
BUILD_DIR="_build-$(arch)"
rm -rf "$BUILD_DIR" && meson setup "$BUILD_DIR" -Dbuildtype=release -Db_lto=true -Dprefix=/usr -Dversion="$VERSION_STRING"
ninja -C "$BUILD_DIR" && env DESTDIR="$OUT_PATH" ninja -C "$BUILD_DIR" install

cp "$OUT_PATH/usr/share/glib-2.0/schemas/io.missioncenter.MissionCenter.gschema.xml" "$DEPS_PATH/usr/share/glib-2.0/schemas/"
glib-compile-schemas "$DEPS_PATH/usr/share/glib-2.0/schemas/"

cd "$DEPS_PATH"
rm -rfv etc var usr/bin/ usr/include/ usr/lib/{python3*,$(arch)-linux-gnu/{*.a,*.la,cairo/*.la,cmake,girepository-1.0,glib-2.0,gobject-introspection,graphene-1.0,pkgconfig,libvala*,vala-*,valadoc-*,libfontconfig.a,libgirepository*,libsass.so,libharfbuzz-{cairo*,gobject*,icu*},libwayland-cursor*,libwayland-server*}} usr/libexec/ usr/share/{aclocal,appstream,bash-completion,devhelp,gdb,gettext,glib-2.0/{codegen,dtds,gdb,gettext,valgrind},gobject-introspection-1.0,gtk-4.0/valgrind,gtk-doc,installed-tests,man,pkgconfig,thumbnailers,vala,vala-*,valadoc-*,wayland,wayland-protocols}

# Strip binaries
find $OUT_PATH -type f -executable -exec llvm-strip {} \;
find $OUT_PATH -type f -name "*.so*" -exec llvm-strip {} \;
