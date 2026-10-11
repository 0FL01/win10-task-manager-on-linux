#!/usr/bin/env python3
"""Determine release eligibility and compute/stamp the CI upstream version."""

import argparse
from datetime import date, datetime, timezone
import json
import os
import re
from pathlib import Path
import subprocess
import sys
from urllib.error import HTTPError
from urllib.request import Request, urlopen


PACKAGES = {
    "mc-core",
    "mc-graph",
    "mc-magpie",
    "mc-ui",
    "taskmanager-app",
    "xtask",
}
VERSION = re.compile(r"^\d+\.\d+\.\d+$")


def workspace_version(root):
    manifest = (root / "Cargo.toml").read_text()
    section = re.search(r"(?ms)^\[workspace\.package\]\s*$(.*?)(?=^\[|\Z)", manifest)
    if not section:
        raise ValueError("[workspace.package] is missing from Cargo.toml")
    match = re.search(r'^version\s*=\s*"([^"]+)"\s*$', section[1], re.M)
    if not match or not VERSION.fullmatch(match[1]):
        raise ValueError("workspace package version must be MAJOR.MINOR.PATCH")
    return match[1]


def release_version(base, run_number):
    if not VERSION.fullmatch(base) or run_number < 1:
        raise ValueError("invalid base version or GitHub workflow run number")
    major, minor, patch = map(int, base.split("."))
    return f"{major}.{minor}.{patch + run_number}"


def compute(root, run_number):
    version = release_version(workspace_version(root), run_number)
    return version, datetime.now(timezone.utc).date().isoformat()


def latest_release_tag(repository, api_url, token):
    request = Request(
        f"{api_url}/repos/{repository}/releases/latest",
        headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json"},
    )
    try:
        with urlopen(request, timeout=30) as response:
            release = json.load(response)
    except HTTPError as error:
        if error.code == 404:
            return ""
        raise
    tag = release.get("tag_name")
    if not isinstance(tag, str) or not tag:
        raise ValueError("latest release has no tag_name")
    return tag


def rust_changes(root, base, head):
    if base:
        args = ["diff", "--no-renames", "--name-only", "-z", f"{base}^{{commit}}", f"{head}^{{commit}}", "--"]
    else:
        args = ["ls-tree", "-r", "--name-only", "-z", f"{head}^{{commit}}"]
    paths = subprocess.run(
        ["git", "-C", str(root), *args], check=True, capture_output=True,
    ).stdout.split(b"\0")
    return any(
        path.endswith(b".rs")
        or path.rsplit(b"/", 1)[-1] in (b"Cargo.toml", b"Cargo.lock")
        or path == b"subprojects/magpie/platform-linux/crates/app-rummage"
        for path in paths
    )


def eligibility(root, head, version):
    if not VERSION.fullmatch(version):
        raise ValueError(f"invalid version: {version}")
    repository = os.environ.get("GITHUB_REPOSITORY")
    api_url = os.environ.get("GITHUB_API_URL")
    token = os.environ.get("GH_TOKEN")
    if not repository or not api_url or not token:
        raise ValueError("eligibility requires GITHUB_REPOSITORY, GITHUB_API_URL and GH_TOKEN")
    latest = latest_release_tag(repository, api_url, token)
    if latest.startswith("v") and VERSION.fullmatch(latest[1:]):
        if tuple(map(int, version.split("."))) <= tuple(map(int, latest[1:].split("."))):
            return False, latest
    base = f"refs/tags/{latest}" if latest else None
    return rust_changes(root, base, head), latest


def stamp(root, version, release_date):
    if not VERSION.fullmatch(version):
        raise ValueError(f"invalid version: {version}")
    try:
        date.fromisoformat(release_date)
    except ValueError as error:
        raise ValueError(f"invalid release date: {release_date}") from error

    original_version = workspace_version(root)
    manifest_path = root / "Cargo.toml"
    manifest = manifest_path.read_text()
    section = re.search(r"(?ms)^\[workspace\.package\]\s*$(.*?)(?=^\[|\Z)", manifest)
    if not section:
        raise ValueError("[workspace.package] is missing from Cargo.toml")
    replacement, count = re.subn(
        r'(?m)^(version\s*=\s*)"[^"]+"\s*$',
        rf'\g<1>"{version}"',
        section[1],
        count=1,
    )
    if count != 1:
        raise ValueError("workspace version entry is missing")
    manifest_path.write_text(manifest[: section.start(1)] + replacement + manifest[section.end(1) :])

    lock_path = root / "Cargo.lock"
    lock = lock_path.read_text()
    blocks = re.split(r"(?=^\[\[package\]\]\s*$)", lock, flags=re.M)
    changed = set()
    stamped = []
    for block in blocks:
        name = re.search(r'(?m)^name\s*=\s*"([^"]+)"\s*$', block)
        lock_version = re.search(r'(?m)^version\s*=\s*"([^"]+)"\s*$', block)
        if name and name[1] in PACKAGES:
            if not lock_version:
                raise ValueError(f"Cargo.lock package has no version: {name[1]}")
            if lock_version[1] not in (original_version, version):
                raise ValueError(f"unexpected Cargo.lock version for {name[1]}: {lock_version[1]}")
            if original_version == version and lock_version[1] != version:
                raise ValueError(f"stamped Cargo.lock version differs for {name[1]}")
            if lock_version[1] != version:
                block, count = re.subn(
                    r'(?m)^(version[ \t]*=[ \t]*)"[^"]+"[ \t]*$',
                    rf'\g<1>"{version}"',
                    block,
                    count=1,
                )
                if count != 1:
                    raise ValueError(f"cannot stamp Cargo.lock package {name[1]}")
            changed.add(name[1])
        stamped.append(block)
    if changed != PACKAGES:
        raise ValueError(f"expected workspace lock records {sorted(PACKAGES)}, got {sorted(changed)}")
    lock_path.write_text("".join(stamped))

    metainfo_path = root / "data/io.github.taskmanager.TaskManager.metainfo.xml.in"
    metainfo = metainfo_path.read_text()
    release = f'        <release version="{version}" date="{release_date}" />\n'
    marker = "    <releases>\n"
    if marker not in metainfo:
        raise ValueError("AppStream releases section is missing")
    existing = re.search(r'<release\s+version="([^"]+)"\s+date="([^"]+)"\s*/>', metainfo)
    if existing and existing[1] == version:
        if existing[2] != release_date:
            raise ValueError(f"AppStream already has {version} dated {existing[2]}")
    else:
        metainfo_path.write_text(metainfo.replace(marker, marker + release, 1))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    compute_parser = subparsers.add_parser("compute")
    compute_parser.add_argument("--run-number", type=int, required=True)
    compute_parser.add_argument("--root", type=Path, default=Path.cwd())
    compute_parser.add_argument("--output", type=Path)
    eligibility_parser = subparsers.add_parser("eligibility")
    eligibility_parser.add_argument("--head", required=True)
    eligibility_parser.add_argument("--version", required=True)
    eligibility_parser.add_argument("--root", type=Path, default=Path.cwd())
    eligibility_parser.add_argument("--output", type=Path)
    stamp_parser = subparsers.add_parser("stamp")
    stamp_parser.add_argument("--version", required=True)
    stamp_parser.add_argument("--date", required=True)
    stamp_parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()

    try:
        if args.command in ("compute", "eligibility"):
            if args.command == "compute":
                version, release_date = compute(args.root, args.run_number)
                values = f"version={version}\nrelease_date={release_date}\n"
            else:
                needed, latest = eligibility(args.root, args.head, args.version)
                values = f"release_needed={str(needed).lower()}\nlatest_tag={latest}\n"
                print("Unpublished Rust changes found" if needed else "No release needed: Rust is unchanged or a newer run is already published", file=sys.stderr)
            output = args.output or (Path(os.environ["GITHUB_OUTPUT"]) if "GITHUB_OUTPUT" in os.environ else None)
            if output:
                with output.open("a") as stream:
                    stream.write(values)
            else:
                print(values, end="")
        else:
            stamp(args.root, args.version, args.date)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release metadata failed: {error}\n")


if __name__ == "__main__":
    main()
