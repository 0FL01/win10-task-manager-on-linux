#!/usr/bin/env python3
"""Collect validated package artifacts into a flat GitHub Release payload."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil


PACKAGE_SUFFIXES = (".rpm", ".deb", ".pkg.tar.zst")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--downloads", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--run-number", type=int, required=True)
    parser.add_argument("--source-archive", type=Path, required=True)
    args = parser.parse_args()
    downloads = args.downloads.resolve()
    output = args.output.resolve()
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)

    packages = [path for path in downloads.rglob("*") if path.is_file() and path.name.endswith(PACKAGE_SUFFIXES)]
    if len(packages) != 5:
        raise SystemExit(f"expected five native packages, found {len(packages)}")
    package_names = set()
    for package in sorted(packages):
        if package.name in package_names:
            raise SystemExit(f"duplicate package asset name: {package.name}")
        package_names.add(package.name)
        shutil.copy2(package, output / package.name)

    builds = []
    for manifest_path in sorted(downloads.rglob("build-manifest.json")):
        manifest = json.loads(manifest_path.read_text())
        builds.append(manifest)
    if len(builds) != 5:
        raise SystemExit(f"expected five build manifests, found {len(builds)}")
    distros = {build["distro"] for build in builds}
    if len(distros) != 5:
        raise SystemExit(f"expected five distinct native targets, found {sorted(distros)}")
    for build in sorted(builds, key=lambda item: item["distro"]):
        shutil.copy2(
            next(path for path in downloads.rglob("build-manifest.json")
                 if json.loads(path.read_text())["distro"] == build["distro"]),
            output / f"build-{build['distro']}.json",
        )

    shutil.copy2(args.source_archive, output / args.source_archive.name)
    source_inputs = [args.source_archive.name]
    for archive in sorted(downloads.rglob("libdrm-*.tar.xz")):
        name = archive.name
        if name in {path.name for path in output.iterdir()}:
            raise SystemExit(f"duplicate source archive asset name: {name}")
        shutil.copy2(archive, output / name)
        source_inputs.append(name)
        signature = archive.with_name(archive.name + ".sig")
        if signature.is_file():
            shutil.copy2(signature, output / signature.name)
            source_inputs.append(signature.name)

    manifest = {
        "version": args.version,
        "commit": args.commit,
        "run_number": args.run_number,
        "architecture": "x86_64",
        "packages": sorted(package_names),
        "builds": sorted(builds, key=lambda item: item["distro"]),
        "source_inputs": source_inputs,
    }
    (output / "BUILD-MANIFEST.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    with (output / "SHA256SUMS").open("w") as checksums:
        for path in sorted(output.iterdir()):
            if path.is_file() and path.name != "SHA256SUMS":
                digest = hashlib.sha256(path.read_bytes()).hexdigest()
                checksums.write(f"{digest}  {path.name}\n")
    print(f"collected {len(packages)} packages into {output}")


if __name__ == "__main__":
    main()
