#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ( $1 != fedora44 && $1 != opensuse-tumbleweed ) ]]; then
    printf 'Usage: %s fedora44|opensuse-tumbleweed\n' "${0##*/}" >&2
    exit 2
fi

command -v rpm >/dev/null
command -v curl >/dev/null
command -v gpg >/dev/null
command -v xz >/dev/null
command -v tar >/dev/null

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
libdrm=/usr/lib64/libdrm.so.2
if [[ ! -e $libdrm ]]; then
    libdrm=/usr/lib/libdrm.so.2
fi
if [[ ! -e $libdrm ]]; then
    printf 'Could not locate the installed libdrm.so.2\n' >&2
    exit 1
fi
version=$(rpm -qf --qf '%{VERSION}' "$libdrm")
if [[ ! $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    printf 'Unexpected libdrm upstream version: %s\n' "$version" >&2
    exit 1
fi

fingerprint=34FF9526CFEF0E97A340E2E40FDE7BE0E88F5E48
url="https://dri.freedesktop.org/libdrm/libdrm-${version}.tar.xz"
workspace="$root/target/ci-source-inputs/libdrm"
mkdir -p "$workspace"
archive="$workspace/libdrm-${version}-${1}.tar.xz"
signature="$archive.sig"
curl --fail --location --retry 3 --retry-all-errors --output "$archive" "$url"
curl --fail --location --retry 3 --retry-all-errors --output "$signature" "${url}.sig"

gnupg_home=$(mktemp -d "${TMPDIR:-/tmp}/taskmanager-gpg.XXXXXX")
notices_home=$(mktemp -d "${TMPDIR:-/tmp}/taskmanager-libdrm.XXXXXX")
trap 'rm -rf -- "$gnupg_home" "$notices_home"' EXIT
chmod 0700 "$gnupg_home"
gpg --batch --homedir "$gnupg_home" \
    --keyserver hkps://keys.openpgp.org --recv-keys "$fingerprint"
actual=$(gpg --batch --homedir "$gnupg_home" --with-colons --fingerprint "$fingerprint" |
    awk -F: '$1 == "fpr" { print $10; exit }')
if [[ $actual != "$fingerprint" ]]; then
    printf 'Unexpected libdrm signing key fingerprint: %s\n' "$actual" >&2
    exit 1
fi
gpg --batch --homedir "$gnupg_home" --verify "$signature" "$archive"

mkdir "$notices_home/source"
tar -xJf "$archive" --strip-components=1 -C "$notices_home/source"
sha256=$(sha256sum "$archive" | awk '{print $1}')
notice="$notices_home/libdrm-notices.txt"
python3 "$root/support/bundle-gpu-libs.py" \
    --source-notices "$notices_home/source" \
    --source-archive "$archive" \
    --source-url "$url" \
    --source-sha256 "$sha256" \
    --dest "$notice"

export MC_GPU_LICENSES
MC_GPU_LICENSES=$(python3 - "$notice" <<'PY'
import json
import sys

notice = sys.argv[1]
print(json.dumps({"libdrm.so.2": [notice], "libdrm_amdgpu.so.1": [notice]}))
PY
)
metadata="$notices_home/metadata.json"
python3 - "$metadata" "$url" "$sha256" <<'PY'
import json
import os
import sys

path, url, checksum = sys.argv[1:]
metadata = {}
current = os.environ.get("MC_GPU_METADATA")
if current:
    with open(current, encoding="utf-8") as source:
        metadata.update(json.load(source))
archive = {"url": url, "sha256": checksum}
for soname in ("libdrm.so.2", "libdrm_amdgpu.so.1"):
    metadata.setdefault(soname, {})["source_archive"] = archive
with open(path, "w", encoding="utf-8") as output:
    json.dump(metadata, output, indent=2)
PY
export MC_GPU_METADATA="$metadata"
printf 'Prepared signed libdrm %s notices and source provenance\n' "$version"
