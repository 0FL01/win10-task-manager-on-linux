#!/usr/bin/env python3
"""Archive tracked project sources, including the pinned app-rummage submodule."""

import argparse
import json
from pathlib import Path
import subprocess
import tarfile


SUBMODULE = Path("subprojects/magpie/platform-linux/crates/app-rummage")


def tracked(root, path=Path(".")):
    output = subprocess.check_output(["git", "ls-files", "-z"], cwd=root / path)
    return [item for item in output.decode().split("\0") if item]


def commit(root, path=Path(".")):
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root / path, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    output = args.output.resolve()
    name = f"taskmanager-{args.version}-source"
    submodule_revision = commit(root, SUBMODULE)
    manifest = {
        "version": args.version,
        "commit": commit(root),
        "submodules": {str(SUBMODULE): submodule_revision},
        "cargo_lockfiles": ["Cargo.lock", "subprojects/magpie/Cargo.lock"],
        "build_dependencies": "Rust 1.96.1 and distro-native development dependencies; resolve Cargo.lock files with --locked.",
    }
    files = [(Path(item), Path(item)) for item in tracked(root) if Path(item) != SUBMODULE]
    files.extend((SUBMODULE / item, SUBMODULE / item) for item in tracked(root, SUBMODULE))
    output.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(output, "w:gz", format=tarfile.PAX_FORMAT) as archive:
        for source, archive_name in files:
            path = root / source
            if path.is_file() or path.is_symlink():
                archive.add(path, arcname=str(Path(name) / archive_name), recursive=False)
        data = json.dumps(manifest, indent=2, sort_keys=True).encode() + b"\n"
        import io

        info = tarfile.TarInfo(str(Path(name) / "SOURCE-MANIFEST.json"))
        info.size = len(data)
        info.mode = 0o644
        archive.addfile(info, io.BytesIO(data))
    print(output)


if __name__ == "__main__":
    main()
