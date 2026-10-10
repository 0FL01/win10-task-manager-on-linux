#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Clean-room measurement of the Windows 10 Task Manager reference screenshots.

Reads the local, git-ignored screenshots (directory from $MC_REFS, default ./references) and
prints *facts only*: coordinates, sizes, colours and text bounding boxes. Nothing from the
images is written into the repository; docs/win10-spec/spec.md is authored from this output.

Usage: MC_REFS=references python3 tools/measure_refs.py [--json]
"""

import json
import os
import sys

import numpy as np
from PIL import Image

REFS = os.environ.get("MC_REFS", "references")
FILES = {
    "processes": "ksnip_20261010-181150.png",
    "performance": "ksnip_20261010-181200.png",
    "details": "ksnip_20261010-181218.png",
    "services": "ksnip_20261010-181226.png",
}


def load(tab):
    return np.array(Image.open(os.path.join(REFS, FILES[tab])).convert("RGB")).astype(int)


def hx(c):
    return "#%02x%02x%02x" % tuple(int(v) for v in c)


def runs(line, min_len=1):
    out, prev, start = [], None, 0
    for i, c in enumerate(line):
        c = hx(c)
        if c != prev:
            if prev is not None and i - start >= min_len:
                out.append((start, i - 1, prev))
            prev, start = c, i
    out.append((start, len(line) - 1, prev))
    return out


def text_boxes(im, x0, y0, x1, y1, thresh=170, gap=6):
    """Bounding boxes of dark text clusters inside a region (column-projection split)."""
    region = im[y0:y1, x0:x1]
    lum = region.mean(axis=2)
    mask = lum < thresh
    cols = mask.any(axis=0)
    boxes, start, blank = [], None, 0
    for x, on in enumerate(list(cols) + [False] * (gap + 1)):
        if on:
            if start is None:
                start = x
            blank = 0
            last = x
        elif start is not None:
            blank += 1
            if blank > gap:
                sub = mask[:, start : last + 1]
                ys = np.where(sub.any(axis=1))[0]
                boxes.append(
                    {
                        "x": x0 + start,
                        "y": y0 + int(ys[0]),
                        "w": last - start + 1,
                        "h": int(ys[-1] - ys[0] + 1),
                        "ink": hx(region[:, start : last + 1][sub].min(axis=0)) if sub.any() else None,
                    }
                )
                start = None
    return boxes


def lines_in_column(im, x, y0, y1, ignore=("#ffffff",)):
    return [(a, b, c) for a, b, c in runs(im[y0:y1, x]) if c not in ignore for a, b in [(a + y0, b + y0)]]


def measure():
    r = {}

    # ---- Processes -------------------------------------------------------
    im = load("processes")
    h, w, _ = im.shape
    p = {"size": [w, h]}
    p["vertical_structure_x700"] = [(a, b, c) for a, b, c in runs(im[:, 700])]
    p["tab_row_y58"] = [(a, b, c) for a, b, c in runs(im[58]) if c != "#ffffff" or b - a > 3]
    p["tab_labels"] = text_boxes(im, 2, 56, 420, 76, gap=5)
    p["title_text"] = text_boxes(im, 2, 2, 400, 30)
    p["caption_glyphs"] = text_boxes(im, 880, 2, 1029, 30, thresh=140, gap=4)
    p["menu_labels"] = text_boxes(im, 2, 31, 300, 50, gap=5)
    p["header_separators_y110"] = [a for a, b, c in runs(im[110]) if c != "#ffffff" and b == a and a > 200 and a < 600]
    p["header_separator_gradient_x308"] = [(y, hx(im[y, 308])) for y in range(77, 134)]
    p["header_summary_text"] = text_boxes(im, 236, 85, 528, 112, gap=4)
    p["header_name_text"] = text_boxes(im, 2, 112, 528, 132, gap=4)
    p["heat_samples"] = [
        {"row": k, "y": 140 + 28 * k, **{n: hx(im[140 + 28 * k, x]) for n, x in [("cpu", 237), ("mem", 310), ("disk", 383), ("net", 456)]}}
        for k in range(19)
    ]
    p["row_text_first_rows"] = [text_boxes(im, 2, 134 + 28 * k, 528, 162 + 28 * k, gap=5) for k in range(3)]
    p["expander_column"] = text_boxes(im, 2, 162, 24, 190, thresh=200)
    p["scrollbar_y300"] = [(a, b, c) for a, b, c in runs(im[300]) if a > 1000]
    p["scrollbar_x1022"] = [(a, b, c) for a, b, c in runs(im[130:660, 1022]) for a, b in [(a + 130, b + 130)]]
    p["footer_text"] = text_boxes(im, 2, 660, 300, 700)
    p["end_task_button_y679"] = [(a, b, c) for a, b, c in runs(im[679]) if a > 900]
    p["end_task_button_x980"] = [(a, b, c) for a, b, c in runs(im[655:705, 980]) for a, b in [(a + 655, b + 655)]]
    r["processes"] = p

    # ---- Performance -----------------------------------------------------
    im = load("performance")
    q = {"size": [im.shape[1], im.shape[0]]}
    q["list_column_x16"] = [(a, b, c) for a, b, c in runs(im[80:420, 16]) for a, b in [(a + 80, b + 80)]]
    q["list_row_y120"] = [(a, b, c) for a, b, c in runs(im[120, :220]) if b - a >= 1]
    q["list_separator_y600"] = [x for x in range(0, 300) if hx(im[600, x]) == "#d9d9d9"]
    q["list_texts"] = [text_boxes(im, 80, y0, 212, y0 + 60, gap=4) for y0 in (92, 152, 212, 278, 344)]
    q["title_texts"] = text_boxes(im, 230, 90, 1030, 135, gap=8)
    q["caption_texts"] = text_boxes(im, 230, 134, 1030, 150, gap=6)
    q["graph_border_y230"] = [x for x in range(230, 1031) if hx(im[230, x]) == "#117dbb"]
    q["graph_border_x250"] = [y for y in range(140, 480) if hx(im[y, 250]) == "#117dbb"]
    q["graph_vgrid_y200"] = [(x, hx(im[200, x])) for x in range(243, 621) if hx(im[200, x]) != "#ffffff"]
    q["graph_hgrid_x300"] = [(y, hx(im[y, 300])) for y in range(152, 305) if hx(im[y, 300]) != "#ffffff"]
    q["stats_texts"] = text_boxes(im, 230, 485, 440, 640, gap=10)
    q["stats_rows"] = [text_boxes(im, 230, y0, 440, y1, gap=10) for y0, y1 in ((488, 506), (508, 535), (540, 558), (560, 587), (592, 610), (612, 640))]
    q["kv_rows"] = [text_boxes(im, 440, y0, 700, y0 + 17, gap=10) for y0 in (490, 508, 526, 544, 562)]
    q["footer_texts"] = text_boxes(im, 2, 662, 400, 700, gap=6)
    r["performance"] = q

    # ---- Details / Services ---------------------------------------------
    for tab, xs in (("details", 300), ("services", 600)):
        im = load(tab)
        d = {"size": [im.shape[1], im.shape[0]]}
        d["vertical_structure"] = [(a, b, c) for a, b, c in runs(im[60:140, xs]) for a, b in [(a + 60, b + 60)]]
        d["header_separators_y100"] = [x for x in range(0, 1030) if hx(im[100, x]) in ("#e5e5e5", "#e4e4e4", "#d9d9d9", "#dcdcdc", "#e3e3e3", "#e6e6e6")]
        d["header_texts"] = text_boxes(im, 2, 92, 1012, 112, gap=6)
        d["row_texts"] = [text_boxes(im, 2, y0, 1012, y0 + 19, gap=6) for y0 in range(115, 115 + 19 * 3, 19)]
        d["footer_texts"] = text_boxes(im, 2, 662, 400, 702, gap=6)
        r[tab] = d
    return r


def main():
    data = measure()
    if "--json" in sys.argv:
        json.dump(data, sys.stdout, indent=1)
        return
    for tab, values in data.items():
        print(f"== {tab}")
        for k, v in values.items():
            print(f"  {k}: {v}")


if __name__ == "__main__":
    main()
