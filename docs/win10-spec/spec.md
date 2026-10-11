# Win10 Task Manager look — clean-room visual specification

Authored from measurements of local reference screenshots (`tools/measure_refs.py`, references are
**not** in the repository). This document contains only facts — sizes, positions, colours, font
sizes — and our own design decisions where the references show nothing. Implementation must use
this document, never the screenshots.

## Coordinate system
- Canonical frame: the whole window **1030×705 px at 96 DPI**. All coordinates below are in this
  frame (the Processes reference crop). Column 0, rows 0–1 and row 704 are window frame/desktop in
  the reference; we paint a 1 px own-colour border on all four edges and never probe the edges.
- Reference crops differ: Performance is shifted +1 px in x, Services +1 px in x and +2 px in y
  (`measure_refs.py` prints raw values); every value below is already normalised to the canonical
  frame.
- Window size: 1030×705 by default; minimum 760×540 (own: the smallest size at which the
  Performance page lays out without overlaps; compact mode 300×240). Settings from older versions
  are migrated once (`Config::migrate_layout`): smaller stored sizes reset to 1030×705, CPU graph
  per logical processor without the bottom graph.
- Interface state is saved and restored (own): column widths and order (header drag; the Name
  column stays first; drop marker 3 px `#0078d7`), sorting of all three lists, collapsed groups
  and expanded apps, Always on top, Fewer details, Paused, device list width, selected tab and
  device. "Remember sorting" / "Remember column order" are on by default (settings migration v2).
- Texts that do not fit are shortened with "…" (own); key/value columns that do not fit are hidden.
- Scaling: all values are logical points; at other DPI multiply by `pixels_per_point` and snap
  1 px lines to physical pixels.

## Typography (Segoe UI metrics; we ship Selawik, prefer Segoe UI if installed)
| role | size px | weight | colour |
|---|---|---|---|
| body / table cells / menu / tabs / title / footer / key-value | 12 (cap height 9) | Regular | `#000000` |
| Processes header column names | 12 | Regular | `#4c607a` |
| Processes header summary % | 16 (digit height 11) | Regular | `#000000` |
| Performance page title ("CPU") | 28 (cap 20) | Light | `#000000` |
| Performance device model (right) | 18 (cap 13) | Regular | `#000000` |
| Performance graph caption / scale | 11 (cap 8) | Regular | `#707070` |
| Performance stat labels, key labels | 12 | Regular | `#707070` |
| Performance stat values | 20 (digit height 14) | Regular | `#000000` |
| Performance device list name | 15 | Regular | `#000000` |
| Performance device list sub-lines | 12 | Regular | `#000000` |
| links | 12 | Regular | `#0066cc` |
| disabled button text | 12 | Regular | `#838383` (own value) |

Text placement rule: `cap top` values below; for a 12 px font the cap top is ≈ 3 px below the line top.

## Window chrome
- Outer border 1 px. Own design: `#7a7a7a` (the reference shows only the desktop behind a
  translucent border).
- Title bar: y 1–30 (30 px), background `#ffffff`.
  - No icon (none in the reference). Title text "Task Manager" (`mc_ui::APP_NAME`) at x 9, cap top y 11.
  - Caption buttons 46×30 each, from the right edge: close x 983–1028, maximize x 937–982,
    minimize x 891–936.
  - Glyphs black 1 px: minimize = horizontal line 10 px at y 16 (x 912–921); maximize = 10×10
    square outline (x 957–966, y 11–20); close = X 10×10 (x 1003–1012, y 11–20).
  - Hover (own design, Win10 behaviour): buttons `#e5e5e5`, close `#e81123` with white glyph;
    pressed `#cccccc` / `#f1707a`.
- Menu bar: y 31–49, background `#ffffff`.
  - Items "File", "Options", "View" with text at x 9, 41, 96, cap top y 37; each item is
    text width + 7 px padding left/right.
  - Hover/open (own design): `#e5f3ff` fill, `#cce8ff` border.
- Lines: `#f2f2f2` at y 50, `#f0f0f0` at y 51.
- Drop-down menus (own design): background `#f2f2f2`, 1 px border `#cccccc`, 2 px inner margin,
  shadow offset 2/2 blur 4 (black 19 %), item height 22, text at item left + 33 (check/radio column
  centred at + 13), hover fill `#91c9f7`, separators `#d7d7d7` (7 px tall, from + 28), submenu
  chevron at right − 12, shortcuts right-aligned at right − 24, minimum width 200.
- Unselected tab hover (own design): fill `#e5f3ff`.

## Tab strip
- y 52–76; bottom line `#d9d9d9` at y 76 across the window except under the selected tab.
- Natural (unselected) tab boundaries in the reference: 3, 65, 145, …, 308, 355, 407 — i.e.
  widths Processes 62, Performance 80, Details 47, Services 52. Rule: width = label advance + 12,
  label origin at left + 7; adjacent tabs share a 1 px border.
- Unselected tab: top border `#d9d9d9` at y 56, fill `#f0f0f0` y 57–75, 1 px `#d9d9d9`
  left/right borders, label cap top y 62.
- Selected tab: top border at y 54, fill `#ffffff` y 55–76 (covers the bottom line), borders
  extend 2 px left and right over the neighbours (first tab: left border at x 1; Details measured
  −1/+2), label cap top y 60.
- Our tabs: Processes, Performance, Details, Services (in this order) → natural boundaries
  3, 65, 145, 192, 244.

## Content frame
- Content starts at y 77. List views have a 1 px `#a0a0a0` frame line on top (Details/Services
  y 89) or under the header (Processes y 133).
- Footer: top line `#a0a0a0` at y 655, area y 656–703 (48 px), background `#ffffff`.
  - "Fewer details": circle 19×19 at x 9, y 670 (stroke `#999999`, our own glyph: circle with a
    chevron); text x 36, cap top y 675.
  - Right button "End task": x 939–1021, y 669–689 (83×21).
    - Enabled (own design, Win32 button): border `#adadad`, fill `#e1e1e1`, hover border
      `#0078d7` fill `#e5f1fb`.
    - Disabled: border `#bfbfbf`, fill `#cccccc`.
  - Links (Performance, Services): separator "|" after "Fewer details", 16 px icon, link text
    `#0066cc`; link text x ≈ 143.

## Scroll bar (vertical, all list views)
- Width 15 px (x 1015–1029 incl. border column → track x 1015–1028).
- Track `#f0f0f0`; arrow buttons 17 px tall at each end with a `#606060` chevron (3 px tall,
  centred); thumb `#cdcdcd`, flush with the track width, minimum 17 px; hover thumb `#a6a6a6`
  (own design).

## Processes tab
- Header: y 77–132 (56 px), background `#ffffff`, bottom line `#a0a0a0` at y 133.
  - Column separators are 1 px vertical lines from y 92 to y 132 with a linear gradient `#e8e8e8`
    (y 92) → `#bbbbbb` (y 132).
  - Column boundaries (separator x): Name | 235 | CPU | 308 | Memory | 381 | Disk | 454 |
    Network | 527. Name 234 px wide (x 1–234), numeric columns 72 px.
  - Summary row (16 px, black, right-aligned, right padding 5 px): cap top y 96.
  - Name row (12 px, `#4c607a`, right-aligned in numeric columns, "Name" left at x 9): cap top
    y 117.
  - Sort indicator: 8×4 chevron `#6e6091` at column left + 9, top y 97 (Memory: x 318).
- Rows: height 28 px, first row y 134; no horizontal lines.
  - Expander chevron (right-pointing, 5×9, `#a6a6a6`) at x 11, row top + 9.
  - Process icon 16×16 at x 30, row top + 6.
  - Name text x 53, cap top row top + 9.
  - Numeric cells: right-aligned, right padding 5 px, cap top row top + 9.
- Heat cells (CPU, Memory, Disk, Network, GPU): fill the full cell (row height × column width
  without the separator), separator lines between heat cells `#ead5a0`.
  - value == 0 → `#fff4c4`; value > 0 (observed up to 1.4 % CPU, 108.9 MB) → `#f9eca8`.
  - Own design, higher levels by share of capacity: ≥ 5 % `#ffe48a`, ≥ 20 % `#ffd25a`,
    ≥ 50 % `#ffb63e`, ≥ 80 % `#ff9a3c`. Capacity: CPU % as is; memory/swap/GPU memory of their
    totals; disk relative to 100 MB/s; network relative to 100 Mbit/s.
  - Separators `#ead5a0` also left of the first heat column (x 235) and after the last (x 527).
- Columns: Name + visible numeric columns (default CPU, Memory, Disk, Network; header context
  menu adds PID, Swap, GPU, GPU Memory); widths 234 / 72, resizable by dragging separators.
  Clicking a header sorts (numeric columns start descending, Name ascending).
- Name cell indent: + 22 px per tree level (expander, icon and text move together).
- Processes without an app icon get our own generic window glyph (16×16).
- Network column (own): the summary is the link utilisation of the physical interfaces (rate /
  summed link speed; the rate when no speed is known), its tooltip shows the rate. Without
  per-process data (magpie `network_stats_error`: nethogs missing or without capture
  permissions) cells show "—" and the tooltip explains Options → Enable per-process network
  usage… (magpie setup script). Summaries too wide for 16 px fall back to 12 px.
  Without nethogs the gatherer measures TCP traffic per process itself (`mc-magpie::sockets`,
  no privileges: netlink sock_diag counters + `/proc/<pid>/fd`; loopback counted for the client
  side only; UDP and other users' processes not visible); the tooltip says so.
- Header summary background (own design): none at normal load; the summary text turns `#c42b1c`
  when the column total ≥ 90 %.
- Selection (own design, Win32 list): fill `#cce8ff`, hover `#e5f3ff`.
- Group headers ("Apps (n)", "Background processes (n)"), own design: 12 px `#1e3287` at x 9, row
  height 28 px, no heat fill. View → Group by type off hides them (top-level rows flatten).
- "Fewer details" (own design): window 400×380, no menu bar and tabs, list of apps from y 31
  (rows 24 px, icon at x 9, text at x 33), footer "More details" + End task.

## Performance tab
- Device list: x 1–211, separator `#d9d9d9` at x 212. Own: the separator can be dragged (hover/drag
  colour `#0078d7`, resize cursor) to widen the list; the page keeps ≥ 547 px and shifts right;
  the width is saved (`performance-sidebar-width`) when the drag ends.
  - Items: 2 text lines → 60 px tall, 3 lines → 66 px tall; first item top y 89, no gaps
    (CPU 89, Memory 149, Disk 0 209, Disk 1 275, Ethernet 341).
  - Selected item fill `#d9d9d9` from x 5 to x 208, full item height.
  - Mini graph x 15–74 (60 px incl. border); height 40 (2-line items) or 46 (3-line items);
    top at item top + 10; 1 px border in the device colour, background `#ffffff`, fill/line as
    the big graph.
  - Text x 86: name 15 px cap top at item top + 11; sub-lines 12 px, line pitch 15 px.
- Device colours (border / line, fill):
  - CPU `#117dbb` / `#f1f6fa`;
  - Memory `#8b12ae` / `#f4f2f4`;
  - Disk `#4da60c` / `#eff7e9` (fill own value);
  - Network `#a54d05` / `#fcf3eb` (fill own value);
  - GPU `#117dbb` / `#f1f6fa` (own: same as CPU);
  - Fan `#813d9c`, Battery `#3b8f4e` (own), fills at 7 % alpha over white.
- Page:
  - Title "CPU": x 238, cap top 103 (28 px Light).
  - Device model right-aligned to x 1004, cap top 111 (18 px).
  - Caption "% Utilization over 60 seconds" at x 241, cap top 138 (11 px `#707070`); "100%"
    right-aligned to x 1004.
- Graph area (CPU per-core grid, 2×2 example): cells x 241–620 and 625–1004, y 151–305 and
  310–465; gap 4 px between borders.
  - Each graph: 1 px border in the device colour, background `#ffffff`, fill device fill colour,
    line 1 px device colour (anti-aliased, blends into the fill).
  - Grid: device colour mixed into white at 16 % (CPU → `#d9eaf4`). Rows at
    `ceil(border_top + k·(border_bottom − border_top)/10)` (2×2 cell: y 167, 182, 198, 213, 228,
    244, 259, 275, 290). Vertical lines: own approximation, every interior width / 6 (10 s of
    60 s; the reference shows ≈ 64 px at 378 px), scrolling left with the data.
  - Fill is opaque and drawn under the grid; the line on top.
  - Grid of cells: cell k starts at `floor(k·(total + 4)/n)` and ends 4 px before the next one.
  - Single-graph pages use one cell x 241–1004, y 151–465.
  - Several graph panels (CPU bottom graph, swap, disk throughput, GPU) share the height; each
    further caption has its cap top 24 px below the previous graph, its graph 13 px lower.
  - Graph area ends 24 px above the statistics; statistics end 24 px above the footer line.
- Stat block (below graphs):
  - labels 12 px `#707070` cap top 494 / 545 / 596;
  - values 20 px black cap top 515 / 566 / 617;
  - columns x 240 / 313 (row 1), 240 / 310 / 372 (row 2), 240 (row 3).
- Statistic columns: next column x = previous x + max(label, value width) + 21.
- Key/value block: labels x 442 (12 px `#707070`; or 24 px right of the statistics), values at
  label x + max(109, longest label + 12) (12 px black), rows cap top 494 + 18·n; rows that do not
  fit above the footer continue in a further column.
- Device list texts (own adaptation): CPU "percent speed (temperature)", memory
  "used/total (percent)" on one line, so these items are 60 px like the reference.
- Footer: "Fewer details" as Processes, then a 16 px own icon at x 120 and link
  "Open Resource Monitor"-style text (ours: none — no external tool; the slot stays empty).

## GPU monitoring (own adaptation, not measured)

No GPU-page reference measurement is available. Reuse the existing CPU/GPU palette,
typefaces, grid and one-pixel paint helpers; the following geometry is our own adaptation:
- Processes default to GPU and GPU engine after Network. GPU engine width 140 px;
  optional dedicated/shared memory columns 180 px. Details keeps its measured six-column
  default; GPU, GPU engine and both memory columns are available through its header menu.
- Overflow scrolls header and rows together horizontally with the existing 15 px scrollbar
  style; keyboard/mouse horizontal step 72 px. Hidden columns retain widths and order.
- GPU Performance has one selected engine or a selectable 2×2 grid; caption band 18 px,
  minimum engine height 84 px, memory/auxiliary graph height 52 px, panel/info gaps 24 px.
  Engine selectors have an original painted 7×4 downward triangle, 4 px after the caption,
  7 px below the caption-band top; no font-dependent glyph or copied icon.
- Dedicated/shared memory have separate graphs. Statistics/metadata scroll vertically;
  compact graph summary has 12 px inset and no shell chrome. Ctrl+M, context menu or
  double-click toggles summary; its background can drag the window.
- GPU numbering is session-stable hardware identity, not sidebar order. Utilization is the
  busiest measured engine; group rows deduplicate shared clients before per-engine sums.
  Unknown/stale values are `—`, graph gaps are NaN, and partial counters show the plain
  value with the reason in the tooltip. NVML overall utilization is never mislabeled as 3D/Compute.
- Frozen pixel probes keep their measured four-resource-column configuration. New GPU
  views are verified separately by real offscreen renders, not invented reference probes.

## Details tab
- Frame line `#a0a0a0` at y 89.
- Header y 90–113: single line, 12 px black, cap top y 97; sort chevron 7×4 `#595959` (darkest
  2 rows measured in Details, 4 in Services) centred over the sorted column at top y 90–92;
  separators 1 px `#e5e5e5` at x 146, 196, 713, 793, 833, 913, 1013.
- Columns: Name 1–146, PID 147–196, Status 197–713, User name 714–793, CPU 794–833, Memory
  834–913; 914–1013 empty (UAC is not implemented).
- Header labels at column left + 7 (Name: + 12); CPU header right-aligned with 6 px padding.
- Rows 19 px from y 114 (pitch 19); icon 16×16 at x 6, row top + 2; name text x 24; text cap top
  row top + 5; PID/Status/User at column left + 7; CPU and Memory right-aligned with 7 px
  padding; CPU as two digits ("06"), memory as "25,364 K" (KiB).
- Status (own mapping of Linux states): Running, Sleeping, Waiting (uninterruptible), Suspended
  (stopped/traced), Zombie, Unknown. User name from `/proc/<pid>/status` (real uid) and
  `/etc/passwd`, read by the gatherer thread; "N/A" when unavailable (e.g. Flatpak).

## Services tab
- Frame line `#a0a0a0` at y 89 (tab bottom line y 76 as everywhere).
- Header as Details (cap top y 97, chevron centred over the sorted column), separators at
  x 181, 231, 813, 913, 1013.
- Columns: Name 1–181, PID 182–231, Description 232–813, Status 814–913, Group 914–1013.
- Header labels at column left + 7 (Name: x 9).
- Rows 19 px from y 114; own gear icon 16×16 at x 9, row top + 2; name text x 27; Description
  at column left + 6, Status left + 7, Group left + 6; cap top row top + 5.
- Footer: "Fewer details" only (own decision: no external service manager link).
- Status: Running / Failed / Stopped / Disabled; Group: the unit's Group= value.
- Context menu: Start, Stop, Restart, Details.

## Palette
| role | hex |
|---|---|
| window / content background | `#ffffff` |
| chrome lines under menu | `#f2f2f2`, `#f0f0f0` |
| tab border / list separators | `#d9d9d9` |
| unselected tab | `#f0f0f0` |
| list frame / footer line | `#a0a0a0` |
| header separator gradient | `#e8e8e8` → `#bbbbbb` |
| heat zero / low | `#fff4c4` / `#f9eca8` |
| heat cell separator | `#ead5a0` |
| scroll track / thumb / arrow | `#f0f0f0` / `#cdcdcd` / `#606060` |
| caption, labels | `#707070` |
| header names | `#4c607a` |
| links | `#0066cc` |
| graph grid | `#d9eaf4` |

## Probes
Machine-readable probes (canonical coordinates, expected colour, description) are in
`probes.json` next to this file. They cover only structural, data-independent pixels plus a few
data-dependent ones whose state is fixed by the synthetic test fixture (documented per probe).
