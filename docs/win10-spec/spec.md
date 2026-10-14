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
- Texts that do not fit are shortened with "…" (own); overflowing Performance key/value rows
  remain available through a scrolling "More details…" popup.
- Scaling: all values are logical points; at other DPI multiply by `pixels_per_point` and snap
  1 px lines to physical pixels.

## Themes and additional metadata (own design, not measured)

- View → Theme: Light (default), Dark, System; System follows live desktop changes. Light keeps
  every measured colour. Dark keeps geometry/fonts, uses `#202020` surfaces, `#f0f0f0` text,
  `#aaaaaa` labels, `#293e4c` hover, `#284c66` selection, `#65b6ef` links and `#a9c8ef` headings.
  Neutral borders/surfaces map to `#292929`, `#3d3d3d`, `#555555`, `#858585`, `#b0b0b0` by original
  brightness. Heat cells use brown/gold shades (`palette.rs`); device hues are preserved.
  Dark graphs use a `#202020` background, 12% device-hue fill and 30% grid against that background,
  with line/border colour 75% original hue and 25% white.
  Close hover/pressed keeps its red surface and `#ffffff` glyph in both themes: the glyph is ink,
  not the white surface role that becomes `#202020` in Dark.
- System is last by default in Performance. Its sidebar mini-graph slot contains an own-drawn
  monitor outline, without invented history. The main page reuses title origin, 12 px label/value
  text, 18 px row pitch, graph-top origin and standard scrollbar; group headings use heading colour.
  Label widths are capped at half the available width; values start 12 px after the label column.
  The monitor uses the existing 60 px sidebar graph slot: screen inset 8 px horizontally,
  top inset 6 px, bottom inset 10 px; the base is inset 20 px, 4 px above the slot bottom.
- Left-click a metadata value copies the full value. A 1 px hover-border-colour outline lasts
  650 ms; the tooltip says "Copied". Hover otherwise shows full text and "Click to copy".
- The Performance overflow link is 100×17 px, 1 px above the page bottom. Its metadata popup
  scrolls at a maximum height of 360 px and uses the existing menu frame/typography.
- Options contains Keyboard Shortcuts (Ctrl+Shift+Esc; optional Ctrl+Alt+Delete) and Extended
  hardware monitoring. Pending items are disabled. Hardware access opens a status modal with
  progress/result; errors remain visible. "Install hardware monitoring service…" invokes permanent
  installation through authorization; the status modal also offers "Install service…". It explains
  temporary versus persistent access, without terminal setup instructions.
- Memory places compact vertical slot outlines beside Speed/Slots used/Form factor/Type,
  or directly below that metadata column when horizontal room is limited, without moving graphs.
  Own geometry: 52 px cells, 10 px gap, 18 px section heading above the stats label line.
  Module outlines are 28 px wide; their height spans from In use through the bottom stat value
  (compression details remain in the separate Swap block). DIMM/SO-DIMM use eight/four schematic chips at this shared
  display height, not physical scale. Labels use 12 px body text and occupy 38 px below each drawing,
  with name/volume cap tops at +7/+23 px. Drawings reserve 14 px of their previous height for the
  larger labels, preserving graph/statistics geometry and avoiding clipping. The area scrolls for large
  inventories. Short slot suffixes (A1, B2) are shown; hover retains full firmware names.
  Drawings resemble parallel physical slots but do not claim measured motherboard placement.
  Empty slots use label-colour outlines and neutral `#e1e1e1` fill; installed modules use Memory colours. Clicking an
  occupied slot automatically reads SPD and opens the module metadata modal (500 px wide),
  including unmatched EEPROM choices. There is no separate Read SPD button. Modal metadata
  scrolls at a maximum height of 400 px.

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
- Resize hit targets (own): 4 px border strips; each corner extends 16 px along both adjacent
  borders as an L-shaped target, without covering interior caption buttons. Native resize starts
  on primary press and releases egui's local drag immediately; the compositor owns the gesture
  and may consume its mouse release. Cursor feedback follows the actual border under the pointer.
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
  chevron at right − 12, shortcuts right-aligned at right − 24. Width = widest item (text at
  + 33, then 40 px, or 32 px + shortcut + 24 px), minimum 200. Submenus open to the right of the
  parent (to the left near the window edge), first item level with the parent item.
- Unselected tab hover (own design): fill `#e5f3ff`.
- Always on top (own adaptation): while waiting for the compositor, or when unsupported,
  its check item is disabled using the existing menu colours; a tooltip explains pending
  or failed requests. The check mark reflects the last confirmed state, not the request.

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
  Full Performance omits this stroke (user-approved correction); the footer/content anchor
  stays at `window.bottom() − 50`. Other tabs and compact mode retain the separator.
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
- Network column (own design, not a verified Windows multi-adapter algorithm): the summary is
  `100 × Σ(RX + TX) / Σ(link speed)` over connected or traffic-carrying physical interfaces
  (Wired, Wireless, WWAN, Bluetooth, InfiniBand). Idle disconnected interfaces and virtual
  duplicates are excluded. Link speed comes from sysfs Mbps for non-Wi-Fi devices and the current
  NetworkManager `Device.Wireless.Bitrate` in Kbit/s for Wi-Fi, not `AccessPoint.MaxBitrate`.
  Every participating interface must have a known positive speed and finite nonnegative rates;
  otherwise, or when there are no participating interfaces, the summary is "—", without alert
  colour. Known zero traffic is 0%. Round to an integer and clamp the displayed percentage to
  0–100% (own design: full-duplex RX + TX can exceed the one-direction link speed). The tooltip
  keeps the unclamped throughput, explains that utilisation is relative to the local link rather
  than the Internet plan, and explains missing speed data. Summaries too wide for 16 px fall back
  to 12 px. Per-process values remain rates from the gatherer's TCP socket counters
  (`mc-magpie::sockets`, no privileges: netlink sock_diag counters + `/proc/<pid>/fd`; loopback
  counted for the client side only; UDP and other users' processes not visible); the tooltip
  says so. If sock_diag is unavailable cells show "—" and the tooltip explains why.
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
  Own: virtual network adapters (Docker, bridges, veth, Multipass) are hidden by default;
  View → "Show virtual network adapters" (`performance-show-network-virtual`) shows them.
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
  - Several graph panels (CPU bottom graph, swap) share the height; each
    further caption has its cap top 24 px below the previous graph, its graph 13 px lower.
  - Graph area ends 24 px above the statistics; statistics end 24 px above the footer boundary.
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
  No horizontal separator in full Performance; footer controls and content retain their anchors.

### Unix Performance parity (own adaptation, not historical pixel probes)

- Keep the 1030×705 default, 760×540 minimum and 211-point default device list; restored
  user window/list sizes are not reset. All sizes below are logical points and scale with DPI.
- Disk transfer rate is a 66-point strip; Active time receives the remaining graph height.
  Retain the three Unix statistic rows and existing metadata. At the default/minimum/tall
  (1030×785) sizes the upper graph is respectively 211/46/291 points high.
- Ordinary graph cells are hover-only, preserving page context menu and double-click ownership.
  Logical CPU labels use kernel CPU IDs and reported core type/package/core topology when
  available; historical fixtures without topology use array ordinals (CPU 0, CPU 1, ...).
  RAM/Disk graphs explain their metrics; graph summary retains these tooltips.
  These graph tooltips and composition tooltips anchor at the pointer, not the graph/bar corner,
  using egui's pointer-tooltip gap and edge fitting (own adaptation). Delay and click ownership
  are unchanged; existing GPU tile tooltips retain their behavior.
- Memory composition is always shown in the full page, after RAM and optional Swap graph and
  before the swap block, with a caption and a 52-point square bar. Segments use existing memory fill/line,
  Free stays white, boundaries are cumulative and zero-width segments have no artificial width.
- The disjoint draw order is Used / Cache / Shared / Buff / Free: Shared = Shmem, Buff = Buffers,
  Cache = max(Cached − Shmem, 0) + SReclaimable, Free = collector-reported MemFree, Used = remainder.
  Free may include reclaimable ZFS ARC. Free → Shared → Buff → Cache are limited to remaining
  MemTotal for inconsistent counters, with a tooltip note; zero total is unavailable.
  Used differs from existing MemAvailable-based In use, Cache from the raw Cached card.
  Every region tooltip explains its volume and includes all five values, including invisible ones.
- Swap block (own design): after composition, caption "Swap usage" with
  "used / total (percent%)" on the right (graph-caption style), a 20-point bar drawn like
  composition (memory fill for SwapTotal − SwapFree, white rest, 1 px memory line) and one
  18-point key/value row per active compression layer (label `#707070`, ratio and
  "stored in RAM-used RAM" black, cap 6 points below the row top; value column at
  max(109, longest label + 12), detail 12 points after the widest ratio).
  - Swap is logical slot usage and includes pages held in zram/zswap; it is not a third pool.
  - zram row: zram devices that are active swap areas in /proc/swaps; ratio =
    orig_data_size / mem_used_total, saved = orig_data_size − mem_used_total.
  - zswap row: ratio = Zswapped / Zswap; when zswap is disabled but the pool still holds
    pages, the row stays and its tooltip says the pool is draining.
  - A zero denominator shows "—". Rows need at least 1 MiB stored, so they do not flicker.
  - Visibility follows data, not flags: no swap (SwapTotal 0) → no caption, bar, rows or
    "Show Swap Usage" item; zram not used as swap and empty/disabled zswap take no row.
  - The statistics stay at the two Windows rows (In use/Available, Committed/Cached); swap and
    compression values left the statistics. Tooltips anchor at the pointer: the bar lists
    every swap area with used/size/priority, rows give stored/compressed/RAM used/saved.
- RAM/Swap graphs have a 52-point minimum. Only Memory scrolls its entire vertical page when
  its graphs/composition/swap block/statistics cannot fit, using the existing scrollbar.
  Sidebar/footer remain fixed; horizontal metadata rules are unchanged. At 760×540, the Swap
  graph plus a swap block with zram and zswap rows needs a 536-point virtual page and 123
  points of scroll. Device/summary transitions reset it. Graph-only summary omits composition
  and the swap block. CPU probes and GPU layout remain unchanged.

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
- Engine labels use the Windows names (3D, Copy, Compute, Video Encode, Video Decode). amdgpu
  engines that exist in the hardware (IP discovery) are listed before first use; from VCN 4 the
  driver runs encode and decode on one queue, shown as one "Video Encode/Decode" engine with that
  reason in its tooltip (own adaptation; Linux exposes no separate counters there).
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
| list frame / retained footer separator (not full Performance) | `#a0a0a0` |
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
