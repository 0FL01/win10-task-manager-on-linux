#!/usr/bin/env python3
"""Warm native Wayland startup comparison; diagnostic-profile binaries, no private logs saved.

Each UI must have its matching missioncenter-magpie sibling. Output contains counts/timings,
not process names, addresses or raw Wayland traffic. No cache dropping or system configuration.
"""
# Copyright 2026 Mission Center Developers
# SPDX-License-Identifier: GPL-3.0-or-later

import argparse
import json
import os
import re
import selectors
import statistics
import subprocess
import time


def measure(executable):
    env = {
        **os.environ,
        "WAYLAND_DEBUG": "1",
        "RUST_LOG": "warn,TaskManager::Perf=debug,TaskManager::Startup=debug",
    }
    start = time.monotonic()
    process = subprocess.Popen(
        [executable, "--no-config", "--bench-seconds", "1"],
        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
    )
    result = {"first_buffer_ms": None, "first_sample_ms": None, "first_applied_ms": None,
              "complete_inventory_ms": None}
    surface = None
    pending = b""
    selector = selectors.DefaultSelector()
    selector.register(process.stderr, selectors.EVENT_READ)
    deadline = start + 15
    try:
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise RuntimeError(f"startup did not terminate: {executable}")
            for key, _ in selector.select(min(remaining, 0.1)):
                chunk = os.read(key.fileobj.fileno(), 65536)
                elapsed = round((time.monotonic() - start) * 1000, 2)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                pending += chunk
                while b"\n" in pending:
                    line, pending = pending.split(b"\n", 1)
                    line = line.decode(errors="replace")
                    match = re.search(r"get_xdg_surface\(new id xdg_surface#\d+, wl_surface#(\d+)\)", line)
                    if match:
                        surface = match[1]
                    if surface and f"wl_surface#{surface}.attach(wl_buffer" in line:
                        result["first_buffer_ms"] = result["first_buffer_ms"] or elapsed
                    if "Read cycle took" in line:
                        result["first_sample_ms"] = result["first_sample_ms"] or elapsed
                    if "first snapshot applied" in line:
                        result["first_applied_ms"] = result["first_applied_ms"] or elapsed
                    if "sample processes=" in line and "pending=false" in line:
                        result["complete_inventory_ms"] = result["complete_inventory_ms"] or elapsed
        code = process.wait(timeout=max(0.01, deadline - time.monotonic()))
        if code or result["first_buffer_ms"] is None or result["first_sample_ms"] is None:
            raise RuntimeError(f"invalid startup measurement: exit={code}, {result}")
        return result
    finally:
        selector.close()
        if process.poll() is None:
            process.kill()
            process.wait()
        process.stderr.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before")
    parser.add_argument("after")
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()
    if not 1 <= args.repeats <= 20:
        parser.error("repeats must be between 1 and 20")
    rows = []
    for _ in range(args.repeats):
        for label, executable in [("before", args.before), ("after", args.after)]:
            row = {"build": label, **measure(executable)}
            rows.append(row)
            print(json.dumps(row), flush=True)
    for label in ["before", "after"]:
        selected = [row for row in rows if row["build"] == label]
        medians = {}
        for field in selected[0]:
            if field == "build":
                continue
            values = [row[field] for row in selected if row[field] is not None]
            medians[field] = round(statistics.median(values), 2) if len(values) == args.repeats else None
        print(json.dumps({"build": label, "medians": medians}))


if __name__ == "__main__":
    main()
