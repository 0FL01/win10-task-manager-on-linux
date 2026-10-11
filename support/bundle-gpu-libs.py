#!/usr/bin/env python3
"""Stage the open GPU userspace ABI in a private, relocatable directory.

Only readelf inspects ELF inputs; neither ldd nor the input binaries are run.
Search roots are *inside* --sysroot, including absolute symlink targets. There
is no host fallback. Package metadata supplies versions, source package names
and notices (RPM, Debian or APK). For unpacked/custom builds --metadata accepts
{SONAME: {version, source, license_expression, licenses: [sysroot paths],
source_archive: {url, sha256}}}. Partial records supplement package metadata.
--license SONAME=/path adds a notice missing from the distribution package;
the builder must obtain it from that package's matching source release.
--source-notices preserves notices from a checksum-pinned source archive in a
single --dest file, before staging libraries (no ELF or build tools needed).

Native callers can set MC_GPU_SYSROOT, MC_GPU_LIB_ROOTS (colon separated),
MC_GPU_METADATA and MC_GPU_LICENSES (JSON object of SONAME -> path list).
READELF and PATCHELF select build tools, never runtime dependencies.
"""

# Copyright 2026 Mission Center Developers
# SPDX-License-Identifier: GPL-3.0-or-later

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

REQUIRED = ("libdrm.so.2", "libdrm_amdgpu.so.1", "libudev.so.1")
# OS ABI and driver stacks are supplied by the OS/installed driver. In
# particular, never collect the proprietary NVIDIA management/compute stack.
EXTERNAL = re.compile(
    r"^(?:ld(?:64)?[-.].*|lib(?:c|m|dl|pthread|rt|resolv|util|anl)\.so(?:\..*)?"
    r"|libc\.musl-.*|lib(?:GL.*|EGL.*|GLES.*|OpenGL|glapi|gbm|gallium.*)\.so.*"
    r"|lib(?:vulkan.*|VkLayer.*|nvidia.*|cuda.*|nvcuvid|nvidia-ml|amdhip64|hsa-runtime64)\.so.*"
    r"|.*_dri\.so)$"
)


def run(*args):
    return subprocess.check_output(args, text=True, env={**os.environ, "LC_ALL": "C"})


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def source_notices(source, archive, url, expected, destination):
    if not source.is_dir() or not url or not re.fullmatch(r"[0-9a-f]{64}", expected or ""):
        raise ValueError("source notices require a source directory, URL and pinned SHA256")
    if sha256(archive) != expected:
        raise ValueError(f"source archive checksum mismatch: {archive}")
    notices = {}
    for path in sorted(source.rglob("*")):
        if path.suffix not in (".c", ".h"):
            continue
        for block in re.findall(r"/\*.*?\*/", path.read_text(errors="surrogateescape"), re.S):
            if re.search(r"copyright|permission|licen[sc]e", block, re.I):
                notices.setdefault(block, []).append(str(path.relative_to(source)))
    if not notices:
        raise ValueError(f"no source copyright/license notices found in {source}")
    text = f"{source.name} source notices\n{url}\nArchive SHA256: {expected}\n\n"
    for path in sorted(source.iterdir()):
        if path.is_file() and path.name.startswith(("COPYING", "LICENSE", "LICENCE")):
            text += path.name + "\n" + path.read_text(errors="surrogateescape") + "\n\n"
    for block, paths in notices.items():
        text += ", ".join(paths) + "\n" + block + "\n\n"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(text, errors="surrogateescape")
    print(f"preserved {len(notices)} distinct source notices: {destination}")


def elf(path):
    text = run(os.environ.get("READELF", "readelf"), "-W", "-h", "-d", "-V", "-n", str(path))
    def field(name):
        match = re.search(r"^\s*" + name + r":\s*(.+)$", text, re.M)
        if not match:
            raise ValueError(f"not a valid ELF ({name} missing): {path}")
        return match[1]
    def dynamic(tag):
        return re.findall(r"\(" + tag + r"\).*?\[([^]]+)\]", text)
    build_id = re.search(r"Build ID: (\w+)", text)
    return {
        "architecture": [field("Class"), field("Data"), field("Machine")],
        "soname": dynamic("SONAME"),
        "needed": dynamic("NEEDED"),
        "runpath": dynamic("RUNPATH"),
        "rpath": dynamic("RPATH"),
        "build_id": build_id[1] if build_id else None,
        "symbol_versions": sorted(set(re.findall(r"Name: (\S+)", text))),
    }


def in_root(root, virtual):
    """Resolve symlinks as the target OS would, not against the builder's /."""
    pending = list(Path(os.path.normpath("/" + str(virtual).lstrip("/"))).parts[1:])
    parts = []
    links = 0
    while pending:
        part = pending.pop(0)
        if part in ("", "."):
            continue
        if part == "..":
            if parts:
                parts.pop()
            continue
        path = root.joinpath(*parts, part)
        if path.is_symlink():
            links += 1
            if links > 40:
                raise ValueError(f"symlink loop in sysroot: {virtual}")
            target = Path(os.readlink(path))
            if target.is_absolute():
                parts = []
            pending = [p for p in target.parts if p != "/"] + pending
        else:
            parts.append(part)
    return root.joinpath(*parts)


def paragraphs(text):
    for entry in text.split("\n\n"):
        fields = {}
        for line in entry.splitlines():
            if ":" in line and not line.startswith(" "):
                key, value = line.split(":", 1)
                fields[key] = value.strip()
        yield fields


def package_metadata(root, path):
    virtual = "/" + str(path.relative_to(root))
    status = root / "var/lib/dpkg/status"
    if status.is_file():
        packages = {p.get("Package"): p for p in paragraphs(status.read_text())}
        for listing in sorted((root / "var/lib/dpkg/info").glob("*.list")):
            if virtual not in listing.read_text().splitlines():
                continue
            package = listing.name[:-5].split(":")[0]
            info = packages.get(package, {})
            source = info.get("Source", package)
            return {
                "version": info.get("Version"), "source": f"deb-source:{source}@{info.get('Version')}",
                "package": package, "license_expression": "see Debian copyright",
                "licenses": [f"/usr/share/doc/{package}/copyright"],
            }
    apk = root / "lib/apk/db/installed"
    if apk.is_file():
        for entry in apk.read_text().split("\n\n"):
            info = next(paragraphs(entry))
            # APK's repeated R: fields need the complete file list, with F:
            # changing the containing directory for subsequent R: entries.
            directory = ""
            found = False
            for line in entry.splitlines():
                if line.startswith("F:"):
                    directory = line[2:]
                elif line.startswith("R:") and "/" + directory + "/" + line[2:] == virtual:
                    found = True
            if found:
                package = info["P"]
                notices = in_root(root, f"/usr/share/licenses/{package}")
                return {
                    "version": info.get("V"), "package": package,
                    "source": f"apk-source:{info.get('o', package)}@{info.get('V')} "
                              f"aports-commit:{info.get('c', 'unknown')} {info.get('U', '')}",
                    "license_expression": info.get("L"),
                    "licenses": ["/" + str(p.relative_to(root)) for p in sorted(notices.rglob("*"))
                                 if p.is_file()] if notices.is_dir() else [],
                }
    # Query only the selected sysroot's database, never the builder's database
    # for a foreign rootfs. RPM file flags identify notices, not arbitrary docs.
    if root == Path("/") or any((root / p).exists() for p in ("usr/lib/sysimage/rpm", "var/lib/rpm")):
        rpm = ["rpm", "--root", str(root)]
        values = run(*rpm, "-qf", virtual, "--qf",
                     "%{NAME}\t%{VERSION}-%{RELEASE}\t%{SOURCERPM}\t%{URL}\t%{LICENSE}\n").splitlines()[0].split("\t")
        files = run(*rpm, "-qf", virtual, "--qf", "[%{FILENAMES}\t%{FILEFLAGS:fflags}\n]")
        return {
            "package": values[0], "version": values[1],
            "source": f"rpm-source:{values[2]} {values[3]}", "license_expression": values[4],
            "licenses": [line.split("\t")[0] for line in files.splitlines()
                         if "l" in line.split("\t")[-1]],
        }
    raise ValueError(f"no source/version package metadata for {virtual}; supply --metadata")


def patch(path, runpath):
    run(os.environ.get("PATCHELF", "patchelf"), "--set-rpath", runpath, str(path))
    info = elf(path)
    if info["runpath"] != [runpath] or info["rpath"]:
        raise ValueError(f"failed to set relative RUNPATH on {path}")


def check_library(path, soname, architecture):
    info = elf(path)
    if info["architecture"] != architecture or info["soname"] != [soname]:
        raise ValueError(f"architecture/SONAME mismatch for {path}: {info}")
    return info


def binary_runpath(binary, destination):
    suffix = Path("lib/taskmanager/gpu")
    if destination.name in ("glibc", "musl"):
        suffix /= destination.name
    # sharun's shared/lib -> ../lib preserves the native prefix-relative path.
    if (binary.parent.parent / suffix).resolve() == destination.resolve():
        return "$ORIGIN/../" + str(suffix)
    relative = os.path.relpath(destination, binary.parent)
    return "$ORIGIN/" + relative


def rpm_filter(names):
    # The unmodified GUI also needs libgcc_s from the OS runtime. A private
    # daemon copy must not hide that pre-existing frontend requirement.
    names = set(names) - {"libgcc_s.so.1"}
    return "^(" + "|".join(re.escape(name) for name in sorted(names)) + r")(\([^)]*\))*$"


def verify(destination, binaries):
    manifest = json.loads((destination / "manifest.json").read_text())
    names = {record["soname"] for record in manifest["libraries"]}
    if manifest["schema"] != 1 or not set(REQUIRED) <= names or len(names) != len(manifest["libraries"]):
        raise ValueError("manifest is missing mandatory GPU ABI libraries")
    actual = {path.name for path in destination.iterdir() if ".so" in path.name}
    if actual != names:
        raise ValueError(f"unexpected/missing libraries in private directory: {actual ^ names}")
    if (destination / "rpm-requires-filter.txt").read_text().strip() != rpm_filter(names):
        raise ValueError("RPM filter differs from the exact bundled SONAME closure")
    for record in manifest["libraries"]:
        name = record["soname"]
        if EXTERNAL.fullmatch(name):
            raise ValueError(f"OS/driver library in private bundle: {name}")
        path = destination / name
        info = check_library(path, name, manifest["architecture"])
        if info["needed"] != record["needed"] or info["runpath"] != ["$ORIGIN"] or info["rpath"]:
            raise ValueError(f"dependency/RUNPATH mismatch: {path}")
        for needed in info["needed"]:
            if needed not in names and not EXTERNAL.fullmatch(needed):
                raise ValueError(f"incomplete private dependency closure: {name} -> {needed}")
        if not record["source"] or not record["version"] or sha256(path) != record["sha256"]:
            raise ValueError(f"source/version/checksum mismatch: {path}")
        archive = record.get("source_archive")
        if archive is not None and (not archive.get("url") or not re.fullmatch(r"[0-9a-f]{64}", archive.get("sha256", ""))):
            raise ValueError(f"invalid source archive provenance: {name}")
        if not record["licenses"]:
            raise ValueError(f"missing licenses: {name}")
        for notice in record["licenses"]:
            if Path(notice["file"]).is_absolute() or ".." in Path(notice["file"]).parts:
                raise ValueError(f"license path outside private bundle: {notice['file']}")
            if sha256(destination / notice["file"]) != notice["sha256"]:
                raise ValueError(f"license checksum mismatch: {notice['file']}")
    for binary in binaries:
        info = elf(binary)
        if info["architecture"] != manifest["architecture"] or info["runpath"] != [binary_runpath(binary, destination)] or info["rpath"]:
            raise ValueError(f"binary architecture/RUNPATH mismatch: {binary}")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", type=Path, action="append", default=[])
    parser.add_argument("--dest", type=Path, required=True)
    parser.add_argument("--sysroot", type=Path, default=Path(os.environ.get("MC_GPU_SYSROOT", "/")))
    parser.add_argument("--search-root", action="append")
    parser.add_argument("--metadata", type=Path, default=os.environ.get("MC_GPU_METADATA"))
    parser.add_argument("--license", action="append", default=[], metavar="SONAME=SYSROOT_PATH")
    parser.add_argument("--source-notices", type=Path, help="preserve source copyright/license notices in --dest")
    parser.add_argument("--source-archive", type=Path)
    parser.add_argument("--source-url")
    parser.add_argument("--source-sha256")
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--relocate-manifest", type=Path, help="refresh checksums after AppImage's ELF relocation; preserve build IDs and source provenance")
    args = parser.parse_args()
    destination = args.dest.absolute()
    binaries = [p.absolute() for p in args.binary]
    if args.source_notices:
        if not args.source_archive or args.verify or args.relocate_manifest or binaries:
            parser.error("--source-notices requires --source-archive and cannot be combined with ELF staging")
        source_notices(args.source_notices, args.source_archive, args.source_url, args.source_sha256, destination)
        return
    if args.verify:
        verify(destination, binaries)
        print(f"verified GPU bundle: {destination}")
        return
    if not binaries:
        parser.error("at least one --binary is required for architecture and relative RUNPATH")
    patchelf_version = run(os.environ.get("PATCHELF", "patchelf"), "--version").strip()
    version = re.search(r"(\d+)\.(\d+)", patchelf_version)
    if not version or tuple(map(int, version.groups())) < (0, 18):
        raise ValueError("patchelf >= 0.18 is required (older releases corrupt newer ELF layouts)")
    architecture = elf(binaries[0])["architecture"]
    if args.relocate_manifest:
        manifest = json.loads(args.relocate_manifest.read_text())
        if architecture != manifest["architecture"]:
            raise ValueError("relocated binary architecture differs from source bundle")
        for record in manifest["libraries"]:
            path = destination / record["soname"]
            info = check_library(path, record["soname"], architecture)
            if (not record["build_id"] or info["build_id"] != record["build_id"]
                    or info["needed"] != record["needed"] or info["symbol_versions"] != record["symbol_versions"]):
                raise ValueError(f"relocation lost or replaced a bundled library: {path}")
            patch(path, "$ORIGIN")
            record["pre_relocation_sha256"] = record["sha256"]
            record["sha256"] = sha256(path)
        (destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    else:
        root = args.sysroot.resolve(strict=True)
        if not root.is_dir():
            raise ValueError(f"invalid sysroot: {root}")
        roots = args.search_root or os.environ.get("MC_GPU_LIB_ROOTS", "").split(":")
        if roots == [""]:
            machine = architecture[2]
            multiarch = {"Advanced Micro Devices X86-64": "x86_64-linux-gnu", "AArch64": "aarch64-linux-gnu", "Intel 80386": "i386-linux-gnu"}.get(machine)
            roots = ([f"/usr/lib/{multiarch}", f"/lib/{multiarch}"] if multiarch else []) + ["/usr/lib64", "/lib64", "/usr/lib", "/lib"]
        overrides = json.loads(args.metadata.read_text()) if args.metadata else {}
        extra_licenses = json.loads(os.environ.get("MC_GPU_LICENSES", "{}"))
        for value in args.license:
            soname, notice = value.split("=", 1)
            extra_licenses.setdefault(soname, []).append(notice)
        libraries = {}
        pending = list(REQUIRED)
        while pending:
            name = pending.pop(0)
            if name in libraries or EXTERNAL.fullmatch(name):
                continue
            if Path(name).name != name:
                raise ValueError(f"non-SONAME dependency: {name}")
            path = None
            for search in roots:
                candidate = in_root(root, Path(search) / name)
                if candidate.is_file():
                    check_library(candidate, name, architecture)
                    path = candidate
                    break
            if path is None:
                raise ValueError(f"missing mandatory library {name} in sysroot {root}, roots {roots}")
            info = elf(path)
            metadata = overrides.get(name, {})
            if not metadata.get("version") or not metadata.get("source"):
                metadata = {**package_metadata(root, path), **metadata}
            if not metadata.get("version") or not metadata.get("source"):
                raise ValueError(f"missing actual source/version for {name}")
            licenses = metadata.get("licenses", []) + extra_licenses.get(name, [])
            notices = []
            for notice in dict.fromkeys(licenses):
                source = in_root(root, notice)
                if not source.is_file() or source.stat().st_size == 0:
                    raise ValueError(f"missing mandatory license for {name}: {source}")
                notices.append(source)
            if not notices:
                raise ValueError(f"no license notices for {name}; supply --license from matching source release")
            # Debian copyright notices can reference rather than embed license
            # texts. Include those texts from the same sysroot as well.
            for notice in notices:
                for referenced in re.findall(r"/usr/share/common-licenses/[\w.+-]+", notice.read_text(errors="replace")):
                    source = in_root(root, referenced)
                    if not source.is_file() or source.stat().st_size == 0:
                        raise ValueError(f"missing referenced license text for {name}: {source}")
                    if source not in notices:
                        notices.append(source)
            libraries[name] = (path, info, metadata, notices)
            pending.extend(info["needed"])
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="gpu-stage-", dir=destination.parent) as temporary:
            stage = Path(temporary)
            records = []
            for name, (path, info, metadata, notices) in sorted(libraries.items()):
                output = stage / name
                shutil.copy2(path, output)
                output.chmod(0o755)
                patch(output, "$ORIGIN")
                license_records = []
                for index, notice in enumerate(notices):
                    relative = Path("licenses") / name / f"{index}-{notice.name}"
                    (stage / relative).parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(notice, stage / relative)
                    license_records.append({"file": str(relative), "source": str(notice.relative_to(root)), "sha256": sha256(notice)})
                records.append({
                    "soname": name, "package": metadata.get("package"),
                    "version": metadata["version"], "source": metadata["source"],
                    "source_file": "/" + str(path.relative_to(root)), "source_sha256": sha256(path),
                    "sha256": sha256(output), "build_id": info["build_id"], "needed": info["needed"],
                    "symbol_versions": info["symbol_versions"],
                    "license_expression": metadata.get("license_expression", "see notices"), "licenses": license_records,
                    **({"source_archive": metadata["source_archive"]} if "source_archive" in metadata else {}),
                })
            manifest = {"schema": 1, "sysroot": str(root), "search_roots": roots,
                        "tools": {"patchelf": patchelf_version,
                                  "readelf": run(os.environ.get("READELF", "readelf"), "--version").splitlines()[0]},
                        "architecture": architecture, "required": list(REQUIRED), "libraries": records}
            (stage / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
            # Filter only the exact bundled SONAMEs, including RPM's ABI suffixes.
            (stage / "rpm-requires-filter.txt").write_text(rpm_filter(libraries) + "\n")
            verify(stage, [])
            if destination.exists():
                shutil.rmtree(destination)
            shutil.copytree(stage, destination)
    for binary in binaries:
        if elf(binary)["architecture"] != architecture:
            raise ValueError(f"binary architecture mismatch: {binary}")
        patch(binary, binary_runpath(binary, destination))
    verify(destination, binaries)
    print(f"staged {len(manifest['libraries'])} private GPU libraries: {destination}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        sys.exit(f"GPU bundle failed: {error}")
