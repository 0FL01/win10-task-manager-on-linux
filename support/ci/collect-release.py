#!/usr/bin/env python3
"""Collect validated package artifacts into a flat GitHub Release payload."""

import argparse
import json
from pathlib import Path
import shutil


PACKAGE_SUFFIXES = (".rpm", ".deb", ".pkg.tar.zst")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--downloads", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
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

    shutil.copy2(args.source_archive, output / args.source_archive.name)
    print(f"collected {len(packages)} packages into {output}")


if __name__ == "__main__":
    main()
