#!/usr/bin/env python3
"""Generate GitHub Release notes listing the commits since the previous release."""

import argparse
from pathlib import Path
import subprocess
import sys

DISTROS = (
    "Automated native packages for Fedora 44, openSUSE Tumbleweed, Arch Linux, "
    "Debian 13 and Ubuntu 26.04 (x86_64)."
)


def git(root, *args):
    return subprocess.run(
        ["git", "-C", str(root), *args], check=True, capture_output=True, text=True,
    ).stdout


def commits(root, latest_tag, head):
    """Returns the resolved head sha and the commits since the previous release tag."""
    resolved = git(root, "rev-parse", f"{head}^{{commit}}").strip()
    if latest_tag:
        git(root, "cat-file", "-e", f"{latest_tag}^{{commit}}")
        rev_range = f"{latest_tag}..{resolved}"
    else:
        rev_range = resolved
    log = git(root, "log", "--reverse", "--format=%H%x00%s", rev_range)
    return resolved, [tuple(line.split("\x00", 1)) for line in log.splitlines()]


def notes(root, latest_tag, head, repo_url):
    resolved, entries = commits(root, latest_tag, head)
    lines = [DISTROS, ""]
    if latest_tag:
        lines.append(f"**Previous release:** `{latest_tag}`")
    else:
        lines.append("**Range:** entire commit history")
    lines += ["", f"**New commits:** {len(entries)}", "", "### Changes", ""]
    for sha, subject in entries:
        short = sha[:7]
        if repo_url:
            lines.append(f"- [`{short}`]({repo_url}/commit/{sha}) {subject}")
        else:
            lines.append(f"- `{short}` {subject}")
    lines.append("")
    if latest_tag and repo_url:
        lines.append(f"[Full changelog]({repo_url}/compare/{latest_tag}...{resolved})")
        lines.append("")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--latest-tag", default="")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repo-url", default=None)
    args = parser.parse_args()

    import os

    repo_url = args.repo_url
    if repo_url is None:
        server = os.environ.get("GITHUB_SERVER_URL")
        repository = os.environ.get("GITHUB_REPOSITORY")
        repo_url = f"{server}/{repository}" if server and repository else ""

    try:
        content = notes(args.root, args.latest_tag, args.head, repo_url)
    except subprocess.CalledProcessError as error:
        parser.exit(1, f"release notes failed: {error}\n")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(content)
    print(f"wrote release notes for {args.version} to {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
