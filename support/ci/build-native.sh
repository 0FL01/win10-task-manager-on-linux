#!/usr/bin/env bash
set -euo pipefail

usage() {
    printf 'Usage: %s DISTRO VERSION STAGE OUTPUT_DIR\n' "${0##*/}" >&2
    printf 'DISTRO: fedora44 | opensuse-tumbleweed | arch | debian13 | ubuntu26.04\n' >&2
}

if [[ $# -ne 4 ]]; then
    usage
    exit 2
fi

distro=$1
version=$2
stage_arg=$3
output_arg=$4
if [[ ! $version =~ ^[0-9][A-Za-z0-9.+_]*$ ]]; then
    printf 'Invalid package version: %s\n' "$version" >&2
    exit 2
fi
if [[ $(uname -m) != x86_64 ]]; then
    printf 'Native packages are only supported on x86_64 (found %s)\n' "$(uname -m)" >&2
    exit 2
fi

case $distro in
    fedora44|opensuse-tumbleweed|arch|debian13|ubuntu26.04) ;;
    *) usage; exit 2 ;;
esac

mkdir -p -- "$stage_arg" "$output_arg"
stage=$(cd -- "$stage_arg" && pwd -P)
output=$(cd -- "$output_arg" && pwd -P)
if [[ -n $(ls -A -- "$stage") ]]; then
    printf 'Stage directory must be empty: %s\n' "$stage" >&2
    exit 2
fi
if [[ $stage == "$output" || $stage == "$output"/* || $output == "$stage"/* ]]; then
    printf 'Stage and output directories must not overlap\n' >&2
    exit 2
fi
if [[ $distro == arch && $EUID -eq 0 ]]; then
    printf 'Arch packages must be built as a non-root user\n' >&2
    exit 2
fi

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
work=$(mktemp -d "${TMPDIR:-/tmp}/taskmanager-native.XXXXXX")
trap 'rm -rf -- "$work"' EXIT

# MC_GPU_LICENSES and MC_GPU_METADATA are intentionally inherited by xtask;
# native GPU-library staging requires matching distribution/source notices.
export PATCHELF="${PATCHELF:-${TASKMANAGER_BUILD_TOOLS:-/opt/taskmanager-build-tools}/bin/patchelf}"
if [[ ! -x $PATCHELF ]] && ! command -v "$PATCHELF" >/dev/null 2>&1; then
    printf 'patchelf not found at %s; run setup-native.sh first\n' "$PATCHELF" >&2
    exit 1
fi

printf 'Building %s native stage for version %s\n' "$distro" "$version"
(cd -- "$root" && cargo run --locked --quiet -p xtask -- install --prefix /usr --destdir "$stage")

case $distro in
    fedora44|opensuse-tumbleweed)
        topdir=$work/rpmbuild
        mkdir -p "$topdir"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS}
        if [[ $distro == fedora44 ]]; then
            dist_suffix=.fc44
        else
            dist_suffix=.tw
        fi
        rpmbuild -bb "$root/packaging/rpm/taskmanager.spec" \
            --define "_topdir $topdir" \
            --define "stagedir $stage" \
            --define "srcroot $root" \
            --define "mc_version $version" \
            --define 'mc_release 1' \
            --define "dist $dist_suffix"
        shopt -s nullglob
        rpms=("$topdir"/RPMS/x86_64/taskmanager-*.rpm)
        if [[ ${#rpms[@]} -eq 0 ]]; then
            printf 'rpmbuild produced no x86_64 Task Manager RPMs\n' >&2
            exit 1
        fi
        cp -p -- "${rpms[@]}" "$output/"
        ;;
    debian13|ubuntu26.04)
        package_root=$work/deb-root
        if [[ $distro == debian13 ]]; then
            package_version="${version}-1+deb13"
        else
            package_version="${version}-1+ubuntu26.04"
        fi
        mkdir -p "$package_root"
        cp -a "$stage/." "$package_root/"
        mkdir -p "$package_root/DEBIAN" "$package_root/usr/share/doc/taskmanager"
        sed "s/@VERSION@/$package_version/g" "$root/packaging/deb/control.in" \
            > "$package_root/DEBIAN/control"
        install -m644 "$root/packaging/deb/copyright" \
            "$package_root/usr/share/doc/taskmanager/copyright"
        install -m644 "$root/COPYING" "$package_root/usr/share/doc/taskmanager/COPYING"
        install -m644 "$root/crates/mc-ui/assets/fonts/OFL.txt" \
            "$package_root/usr/share/doc/taskmanager/OpenSans-OFL.txt"
        dpkg-deb --build --root-owner-group "$package_root" \
            "$output/taskmanager_${package_version}_amd64.deb"
        ;;
    arch)
        arch_work=$work/arch
        mkdir -p "$arch_work"
        sed "s/@VERSION@/$version/g" "$root/packaging/arch/PKGBUILD.in" \
            > "$arch_work/PKGBUILD"
        (
            cd -- "$arch_work"
            TASKMANAGER_STAGE=$stage TASKMANAGER_ROOT=$root PKGDEST=$output \
                makepkg --clean --force --noconfirm
        )
        ;;
esac

printf 'Native package output: %s\n' "$output"
