> Frozen historical GTK specification. `src/`, `resources/`, schema and Meson paths below refer to the
> [root tree at `671bc544ac4f7a3149c53db6df8a453252547d0a`](https://github.com/0FL01/win10-task-manager-on-linux/tree/671bc544ac4f7a3149c53db6df8a453252547d0a),
> not the active egui workspace. Graph-widget paths refer to
> [upstream `8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc`](https://gitlab.com/mission-center-devs/graph-widget/-/tree/8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc).
> Its GUI-free core is now local at `crates/mc-graph-core/`; SVGs used by egui are in
> `crates/mc-ui/assets/icons/`. See [provenance](../PROVENANCE.md).

I have everything I need. Here is the spec.

---

# Mission Center — GTK Performance Page functional spec (for egui port)

## 0. Cross-cutting facts

### 0.1 i18n
`src/i18n.rs`: `i18n(s)` = `gettext(s)`; `i18n_f(fmt, args)` = gettext then literal `{}` substitution in order; `ni18n_f(sing, plur, n, args)` = `ngettext` + substitution; `pi18n*` = `pgettext` w/ context. **No string on any of these pages uses a `pi18n` context** — `po/*.po` contain zero `msgctxt`. `_("...")` in `.blp` == plain gettext. Mnemonics in blp labels (`_T`, `_P`) are not part of the msgid text but the underscore is: e.g. msgid is literally `"Change _Top Graph To"` (verified in `po/missioncenter.pot:403`).

### 0.2 Number formatting (`src/main.rs`)
`to_human_readable_nice(v: f32, ty) -> String` (`src/main.rs:387`):
- `Hertz` → label `"Hz"`, `min_dec=0`; `Watts`→`"W"`, `min_dec=-1`; `Volts`→`"V"`, `-1`; `WattHours`→`"Wh"`, `-1`; everything else → `to_human_readable_adv_str(v, use_bytes, use_binary, per_second, "B"|"b", 1)`.
- read booleans from GSettings: `MemoryBytes`→`performance-page-memory2-use-bytes` / `…-use-base2` (defaults true/true); `DriveBytes[PerSecond]`→`…drive-use-bytes`/`…drive-use-base2` (true/true); `NetworkBytes[PerSecond]`→`…network-use-bytes`/`…network-use-base2` (false/false). Cached in a global mutex map (`src/main.rs:358`).
- `to_human_readable_adv_str` (`src/main.rs:317`): units `["","K","M","G","T","P","E","Z","Y"]`; divisor 1024 if binary else 1000 if bits (`use_bytes==false`) multiply by 8 and suffix `"ps"`/`""`; divide while `exponent < min_dec`, then while `value >= divisor`; decimals = `2` if exponent>min_dec && |v|<10, `1` if <100, else `0`; when exponent<=min_dec → 0 decimals. Output `"{value:.{decimals}} {UNITS[e]}{'i' if binary}{unit_label}{'/s' if per_second && bytes}"`. So `8 GiB` prints `"8.00 GiB"`, `3000 MHz` prints `"3.00 GHz"`, `15.4 W` prints `"15.4 W"`.
- `to_short_human_readable_time(seconds: u32)` (`src/main.rs:201`): `mins = s/60`, `secs = s%60`; `ni18n_f("{} second","{} seconds", secs, [secs])`, `ni18n_f("{} minute","{} minutes", mins, [mins])`; if `mins>0` → `"<mins_string> <seconds_string>"` (secs omitted when 0), else just seconds string.

### 0.3 Graph widget (`subprojects/graph-widget`)
`GraphWidget` (`src/widget.rs`) properties/defaults: `data_points` 0 (bound to `performance-page-data-points`), `base_color` black a=1, `horizontal_line_count` 9, `vertical_line_count` 6, `animation_progress` 0, `do_animation` false, `smooth_graphs` false, `scroll` **true**, `grid_visible` **true**, `force_horizontal_line_count_divisible_by` 2, `force_vertical_line_count_divisible_by` 1.
- Constants: `GRAPH_RADIUS=7.`, `ANIMATION_LOCKOUT=0.005`, `GRID_LINE_ALPHA=51./255.` (20%), `GRID_VERTICAL_LINE_IDEAL_PIXEL_WIDTH=60.`, `GRID_HORIZONTAL_LINE_IDEAL_PIXEL_WIDTH=50.` (`widget.rs:43-52`).
- Rendering order (`imp::render`, `widget.rs:222`): rounded-clip to `GRAPH_RADIUS`; grid (if `grid_visible`) drawn in `base_color` at alpha 51/255, stroke width 1; then each dataset group via `render::plot_group`; then whole-widget 1px border in `base_color` alpha 1. Grid line counts recomputed on resize as `round(px/ideal)` (if `force_divisible>1`: `round(raw/force)*force-1`), min `force-1` (`widget.rs:394-417`).
- Dataset drawing (`src/render.rs:31`): per dataset group skipped entirely when `visible == false`. Stroke = base_color alpha 1, width 1; `dashed` → `set_dash(&[5.,5.])`. Fill = base_color with alpha `dataset_settings.opacity` (default `100./255.`). Path is split on NaN y. `smooth_graphs` → cubic interpolation with control points at midpoint x. `vertical_dropoff_lines` (default true) closes the path down/up to an edge and **strokes the closed path**; when false it strokes only the open polyline. Fill skipped when `fill == None`; in `Stacking` mode only the last series is filled.
- Geometry (`core/src/dataset.rs:442 plot`): `val_min = low_watermark`, `val_max = max(high_watermark, val_min+1)`; `spacing = width * point_spacing_factor`; for index `i` = `x_points[i]` (=i): `x = width - i*spacing`, `y = (1 - (v-val_min)/(val_max-val_min)) * height`. Newest sample (`i=0`) is drawn at the **right edge**; NaN or non-normal (∞) values are replaced by `low_watermark` (`get_data_sanitized`). `point_spacing_factor = 1/max(data_points - (2 if do_animation else 1), 1)` (`widget.rs:690`).
- `add_data` rotates the ring buffer right (`data[0]` = newest), `MAX_DATA_POINTS=600`, `MIN_DATA_POINTS=10` (`core/src/lib.rs:34-37`).
- Scaling (`core/src/dataset.rs:164`, `:233`; enum `core/src/scaling.rs:21`): `Fixed` (no auto), `ScaleUp` (high = up-round(max over window × multiplier)/multiplier, expensively each sample), `ScaleDown`, `ScaleUpDown`, `StickyUp` (high only grows, cheap per sample), `StickyDown`, `StickyUpDown`, `StickyUpDownEqualMagnitude`, `Stacking` (series stacked upward, only last filled). Rounding: `NoRounding`, `Pow2`, `Pow2Base10`, `Integer` (ceil up / floor down), `Pow10`. Followers: `connect_datasets(a,b)` sets `a.following=b`, `b.followed=a`; on each `add_data_point` follower takes max(high) and min(low) of the pair (`apply_followings`, `widget.rs:489`).
- Animation (`widget.rs:513`): on `progress == 0` → reset progress, take `grid_offset`, force full redraw (new cycle); else if `do_animation` and `progress > animation_progress + 0.005` → set progress & queue redraw. When animating, the cached snapshot is translated by `spacing * (1 - progress)` (`widget.rs:274`). If widget not visible and `progress == 0`, just marks dirty.
- `AnimationFrame` (`core/src/animation.rs`): `progress` ∈ [0,1] fraction of the refresh interval elapsed; `grid_offset` a monotonically increasing counter used as grid scroll phase only when `progress == 0`.
- Tick production (`src/window.rs:1131` 50 ms timer): `progress = clamp((now - last_refresh)/1e6 / (refresh_ticks * 0.05), 0, 1)`; and after every `update_readings` (`window.rs:1261`): `grid_offset += 1; AnimationFrame{progress: 0., grid_offset}`.

### 0.4 Settings keys (defaults from `data/io.missioncenter.MissionCenter.gschema.xml`)
`performance-page-data-points` i, 10..600, **60** (line 67); `app-update-interval-u64` t, 10..200, **20** (in 50 ms units, line 61); `performance-page-cpu-graph` i, 1..4, **1** (168); `performance-page-cpu-graph-bottom` i, 0..3, **1** (174); `performance-page-kernel-times` b **false** (180); `performance-selected-page` s `"cpu"` (185); `performance-smooth-graphs` b false (191); `performance-sliding-graphs` b false (196); `performance-page-memory-composition-visible` b true (261); `performance-page-memory-swap-visible` b true (266); `performance-page-network-dynamic-scaling` b true (103); `performance-show-cpu`/`-memory` b true (201/206) — **not read by these pages**; `performance-show-disks` (211), `performance-show-network` (216), `…-network-wired/-wireless/-vpn/-virtual/-other` (221..241), `performance-show-gpus` (246), `performance-show-fans` (251), `performance-show-batteries` (256) — all true; `performance-sidebar-order` s "" (284); `performance-sidebar-hidden-graphs` s "" **deprecated/migrated** (290); `performance-sidebar-device-overrides` s "" (295); `performance-page-memory2-use-bytes/-use-base2` b true.
`src/application.rs:41`: `INTERVAL_STEP = 0.05`.

`widgets/graph_widget_settings.rs:32-48`: `connect_to_settings` binds `performance-page-data-points`→`set_data_points`, `performance-smooth-graphs`→`set_smooth_graphs`, `performance-sliding-graphs`→`set_do_animation`, applying immediately and on change. `connect_to_smooth_settings` binds only `performance-sliding-graphs`.

---

## 1. CPU page — `src/performance_page/cpu.rs` + `cpu.blp` + `cpu_details.blp`

### 1.1 Layout (`cpu.blp`)
`Box` vertical, `margin-bottom: 10`. Inner `Grid row-spacing: 10`.
- Header (`Box top`, col 0 row 0 row-span 7) inside `WindowHandle` → `Box description` vertical, `margin-top 2`, `spacing 7`:
  - Row 1: `Label "title-1"` hexpand, halign start, msgid **`"CPU"`**; `Label cpu_name` `"title-3"`, halign end, ellipsize middle, selectable (text set from code).
  - Row 2: `Label "caption"` **`"Utilization over "`** (trailing space, msgid verified pot:393) + `Label graph_max_duration "caption"` hexpand/halign start + `Label "caption"` **`"100%"`** (pot:399).
- `Grid usage_graphs` (col 0, row 0, span 7): `row-spacing 7`, `column-spacing 7`, vexpand/hexpand, height-request 20, width-request 100, row-homogeneous, column-homogeneous.
- `Box bottom_graph_box` (col 0, row 7, row-span 3) → vertical `Box`:
  - caption row: `Label bottom_graph_label "caption"` ellipsize middle; `Label "caption"` hexpand/halign start **bound to `graph_max_duration.label`**; `Label bottom_graph_total "caption"`.
  - `McGraphWidget bottom_graph` vexpand/hexpand, width-request 50, height-request 30, `base-color: bind template.base-color`, `scroll: true`.
- `PopoverMenu context_menu { has-arrow:false; menu-model: context_menu_model }`, popped up by a `GestureClick` button 3 on the page, pointing at the click point (`cpu.rs:461-481`).

### 1.2 Header/duration computation
`PerformancePageCpu::new` (`cpu.rs:1313-1351`) and the identical helper in memory: `data_points = settings.int("performance-page-data-points"); delay = settings.uint64("app-update-interval-u64"); graph_max_duration = round(delay as f64 * INTERVAL_STEP * data_points as f64) as u32;` then `graph_max_duration.set_text(to_short_human_readable_time(dur))`. Defaults → `20 * 0.05 * 60 = 60` s → **`"60 seconds"`**. Re-applied on change of either key.

### 1.3 Graphs
Graph index model (`cpu.rs:82`, `:914`): `graph_widgets[0]` = *top graph* (one of 4 modes), `graph_widgets[1]` = thread-wise overlay graph, `graph_widgets[2..]` = one graph per logical processor. All are children of `usage_graphs`, all attached at (0,0) for 0/1 (both overlaying the same cell) and at `(i%cols, i/cols)` for the per-core ones.

`populate_usage_graphs(cpu_count)` (`cpu.rs:897-990`):
- Clears the grid; sets `virt_proc` label to `cpu_count`.
- `col_count = compute_column_count(cpu_count)` (`cpu.rs:992`): if `<=3` return count; `sqrt = round(sqrt(n))`; first `i` in `sqrt..min(n, sqrt*2)` dividing `n`; else `sqrt`.
- `graph_selection = settings.int("performance-page-cpu-graph")`, `show_kernel_times = settings.boolean("performance-page-kernel-times")`.
- Top graph `overall`: dataset 0 = usage (`ScalingSettings::Fixed`, `high_watermark 100.`, `set_datasets(cpu_count)`); dataset 1 = kernel (`fill = None`, `dashed = true`, `visible = show_kernel_times`, `high_watermark 100.`). Visible iff selection == 1.
- Thread-wise `thread_wise` (1 dataset group, `set_datasets(cpu_count)`): if selection == 4 (stacked): `Stacking`, `high_watermark = 100. * cpu_count`; else `Fixed`, `high_watermark 100.`, `opacity = 100./255./cpu_count`. Visible iff selection ∈ {3,4}.
- Per-core graph *i* (2 dataset groups): usage (`high_watermark 100.`) + kernel (`fill None`, `dashed`, `visible=show_kernel_times`, `high_watermark 100.`). Visible iff selection == 2.
- Every widget: `connect_to_settings(settings)`, `set_base_color(page base color)`.
- No explicit axis text is drawn by the graph widget; the only axis text is the blp caption row (`"Utilization over "` + duration + `"100%"`). Label `max_graph_ram`-style value labels exist only on the memory page. (There is no `"% Utilization"` msgid anywhere in the repo.)

Dataset indices for the bottom graph (`cpu.rs:38-44`): `GRAPH_NONE=0`, `GRAPH_POWER=1`, `GRAPH_CLOCK=2`, `GRAPH_TEMPERATURE=3`; datasets `GRAPH_POWER_DATASET=0`, `GRAPH_CLOCK_DATASET=1`, `GRAPH_TEMPERATURE_DATASET=2`.
Bottom graph datasets (`cpu.rs:1126-1145`):
| # | name | scaling | rounding | high | low |
|---|---|---|---|---|---|
| 0 | power | `StickyUp` | `Integer` | 0. | – |
| 1 | clock | `StickyUp` | `NoRounding` | 0. | – |
| 2 | temperature | `StickyUpDown` | `Integer` | `TEMPERATURE_HIGH_WATERMARK` = **45.** | `TEMPERATURE_LOW_WATERMARK` = **35.** (`fan.rs:39-40`) |
plus `connect_to_settings`.

### 1.4 Context menu (`cpu.blp:164-272`, group `graph` on the CPU page; actions `cpu.rs:267-459`)
Section 1:
- Submenu **`"Change _Top Graph To"`**:
  - **`"Overall U_tilization"`** → `graph.overall` (state GRAPH_SELECTION_OVERALL=1)
  - **`"Logical _Processors"`** → `graph.all-processors` (2)
  - **`"_All Threads"`** → `graph.all-threads` (3)
  - **`"All Threads _Stacked"`** → `graph.all-threads-stacked` (4)
- Submenu **`"Change B_ottom Graph To"`**:
  - **`"_Power Draw"`** → `graph.cpu_power` (=1)
  - **`"_Temperature"`** → `graph.cpu_temperature` (=3)
  - **`"_Clock Speed"`** → `graph.cpu_clocks` (=2)
  - **`"_None"`** → `graph.cpu_none` (=0)
- **`"Show Kernel Times"`** → `graph.kernel_times`
Section 2: **`"Graph _Summary View"`** → `graph.summary`; submenu **`"_View"`**: **`"CP_U"`**→`graph.cpu`, **`"_Memory"`**→`graph.memory`, **`"_Drive"`**→`graph.disk`, **`"_Network"`**→`graph.network`, **`"_GPU"`**→`graph.gpu`, **`"_Fan"`**→`graph.fan`, **`"_Battery"`**→`graph.battery`.
Section 3: **`"_Copy"`** → `graph.copy`.

Top-graph selection (`update_selection_callback!`, `cpu.rs:124-169`): sets `graph_widgets[0].visible = (idx==1)`; for idx==3: `graph_widgets[1].set_dataset_max_scale(0,100.)`, `Fixed`, `opacity = 100./255./(len-2)` (== per-core count), visible; for idx==4: `max_scale = 100.*(len-2)`, `Stacking`, `opacity 100./255.`, `visible`, `set_filled(0, FillToBottom)`; otherwise `graph_widgets[1].visible=false`; per-core graphs `visible = (idx==2)`; all four stateful actions updated; **persists `set_int("performance-page-cpu-graph", idx)`**.
Kernel times action (`cpu.rs:365-404`): toggles `graph_widgets[0].set_data_visible(1, v)` and for every `graph_widgets[2..]` the same; persists `set_boolean("performance-page-kernel-times", v)`.
Copy action: `clipboard.set_text(data_summary())`.
Bottom-graph actions are stateful and enabled/disabled at runtime: `cpu_power` disabled unless `power_draw_w.is_some()`; `cpu_temperature` disabled unless `temperature_celsius.is_some()`; `cpu_clocks`/`cpu_none` always enabled. Each sets `set_int("performance-page-cpu-graph-bottom", num)`. A `settings.connect_changed("performance-page-cpu-graph-bottom")` re-applies state + graph (`cpu.rs:441-454`).

### 1.5 Bottom graph state/labels (`cpu.rs:1009-1086`)
`set_graph_states(n)` sets the four stateful action states. `set_graph(n)`: `set_data_visible(0, n==1)`, `(1, n==2)`, `(2, n==3)`; if `n==0` hide `bottom_graph_box`; else show and set `bottom_graph_label` to **`"Power draw over "`** / **`"Clock speed over "`** / **`"Temperature over "`**, then `force_redraw()` and `update_graph_total(n)`.
`update_graph_total(n)`: clock+bogomips → `format!("{:.2} BogoMIPS", max_scale[clock])`; power → `to_human_readable_nice(max_scale[power], Watts)`; clock → `…(…, Hertz)`; temperature → `format!("{:.0} °C", max_scale[temp])`; else `""`.

### 1.6 `set_static_information` (`cpu.rs:485-632`)
- `cpu_name` = `readings.cpu.name` else `i18n("Unknown")`; then `populate_usage_graphs(core_usage_percent.len())`.
- `is_bogomips = frequency_driver == Some("bogomips")`.
- `base_speed`: if `base_freq_khz` present → bogomips: `"{:.2} BogoMIPS"` with `freq = khz/1000.`; else `to_human_readable_nice(khz*1000., Hertz)`; and `bottom_graph.set_dataset_max_scale(CLOCK, freq)` (raw Hz or BogoMIPS). If absent → `i18n("Unknown")`.
- If bogomips: tooltip on `speed` label, on `speed_indicator` (makes `"(?)"` visible) and on `speed_label` = `i18n_f("Real CPU frequency data is unavailable (common in VMs, containers, or cloud instances). Displaying BogoMIPS value as a fallback. BogoMIPS is a delay loop calibration metric, not a true frequency measurement.", &[])`.
- `virtualization` = `virtualization_technology` else `i18n("Unsupported")`; `virt_machine` = `i18n("Yes")`/`i18n("No")`/`i18n("Unknown")`; `sockets` = `"{}"` count or `i18n("Unknown")`; `l1/l2/l3_cache` = `to_human_readable_nice(bytes, MemoryBytes)` or `i18n("N/A")`; l4 read into `_` (unused).
- Enables/disables bottom-graph power/temperature actions; if the stored bottom graph is unavailable it switches to `GRAPH_TEMPERATURE` (power missing) or `GRAPH_CLOCK` (temperature missing) and persists it. Then `set_graph_states(n)` + `set_graph(n)`.

### 1.7 `update_readings` (`cpu.rs:634-781`)
- If `graph_widgets.len() != core_count + 2` → repopulate.
- `graph_widgets[0].add_data_point(vec![vec![total_usage_percent], vec![kernel_usage_percent]])`.
- `graph_widgets[1].add_data_point(vec![core_usage_percent.clone()])`.
- each `graph_widgets[i>=2]`: `add_data_point(vec![vec![usage_i], vec![kernel_i]])` zipping `core_usage_percent` with `core_kernel_usage_percent`.
- `utilization` = `format!("{}%", total_usage_percent.round())`.
- speed: bogomips → `bottom_graph.add_single_data_point(CLOCK, vec![current_frequency_mhz as f32])` and text `"{:.2} BogoMIPS"`; else `add_single_data_point(CLOCK, vec![mhz*1e6])` and `to_human_readable_nice(…, Hertz)`.
- `power_draw`: if present → `add_single_data_point(POWER, vec![w])` and `to_human_readable_nice(w, Watts)`; else hide `power_row`.
- `processes`/`threads`/`handles` = `"{}"` of `total_process_count` / `total_thread_count` / `total_handle_count`.
- `uptime` = `format!("{:02}:{:02}:{:02}:{:02}", uptime/86400, (uptime%86400)/3600, (uptime%3600)/60, uptime%60)` (days:hours:minutes:seconds).
- `cpufreq_driver` = `frequency_driver` else hide label+value; `cpufreq_governor` = `frequency_governor` else hide both; `energy_performance_preference` = `power_preference` else hide both.
- temperature: if `temperature_celsius` → `add_single_data_point(TEMPERATURE, vec![t])` and `format!("{:.0} °C", t)`.

### 1.8 `update_animations` (`cpu.rs:783-797`)
Calls `update_animation(ticks)` on every `graph_widgets[i]` and on `bottom_graph`, then `update_graph_total(bottom_graph_selection)`.

### 1.9 Details / infobar panel (`cpu_details.blp`, root is `gtk::Box`, loaded at `cpu.rs:1147`)
`Box root`: `margin-top 65`, `margin-bottom 10`, vertical, spacing 20.
- `Box dynamic_data` vertical spacing 10:
  - `top_row` spacing 15: (a) caption **`"Utilization"`** + `utilization` "title-4"; (b) `Box speed_label_box` spacing 3 halign start with `Label speed_label "caption"` **`"Speed"`** and `Label speed_indicator "caption"` text `"(?)"` `visible:false`, then `Label speed "title-4"`.
  - `power_row` vertical spacing 3: caption **`"Power Draw"`** + `power_draw` "title-4".
  - `mid_row` spacing 15 width-request 200: three vertical boxes with captions **`"Processes"`**, **`"Threads"`**, **`"Handles"`** and values `processes`/`threads`/`handles` "title-4".
  - caption **`"Up time"`** + `uptime`.
  - `Box { visible: bind temperature.visible; … }` caption **`"Temperature"`** + `temperature`.
- `Grid system_info` row-spacing 5 column-spacing 10, caption-styled labels, col0 = name (`ellipsize: end`), col1 = value (selectable):
  row0 **`"Base Speed:"`**/`base_speed`; row1 **`"Sockets:"`**/`sockets`; row2 **`"Virtual processors:"`**/`virt_proc`; row3 **`"Virtualization:"`**/`virtualization` (blp hardcodes the untranslated literal `label: "Unknown"` as initial text); row4 **`"Virtual machine:"`**/`virt_machine`; row5 **`"L1 cache:"`**/`l1_cache`; row6 **`"L2 cache:"`**/`l2_cache`; row7 **`"L3 cache:"`**/`l3_cache`; row8 `cpufreq_driver_label` **`"Cpufreq driver:"`**/`cpufreq_driver`; row9 `cpufreq_governor_label` **`"Cpufreq governor:"`**/`cpufreq_governor`; row10 `energy_performance_preference_label` **`"Power preference:"`**/`energy_performance_preference`.
- Infobar margins: `PageExt::infobar_collapsed` → `set_margin_top(10)`; `infobar_uncollapsed` → `set_margin_top(65)` (`cpu.rs:1297-1311`).

### 1.10 `data_summary()` (`cpu.rs:799-895`) — exact copy text
Reads each label's `.label()` (missing → `""`): base_speed, sockets, virt_proc, virtualization, virt_machine, l1_cache, l2_cache, l3_cache, cpufreq_driver, energy_performance_preference, cpufreq_governor, utilization, speed, processes, threads, handles, uptime, temperature; first arg is `cpu_name.label()`. Format string (verbatim, 4-space indents, blank line after header):

```
CPU

    {}

    Base speed:         {}
    Sockets:            {}
    Virtual processors: {}
    Virtualization:     {}
    Virtual machine:    {}
    L1 cache:           {}
    L2 cache:           {}
    L3 cache:           {}
    Cpufreq driver:     {}
    Cpufreq governor:   {}
    Power preference:   {}

    Utilization: {}
    Speed:       {}
    Processes:   {}
    Threads:     {}
    Handles:     {}
    Up time:     {}
    Temperature: {}
```
Argument order is: cpu_name, base_speed, sockets, virt_proc, virtualization, virt_machine, l1, l2, l3, cpufreq_driver, cpufreq_governor, energy_performance_preference, utilization, speed, processes, threads, handles, uptime, temperature.

---

## 2. Memory page — `src/performance_page/memory.rs` + `memory.blp` + `memory_details.blp` + `memory_info_tooltip.blp`

### 2.1 Layout (`memory.blp`)
`Box` vertical. `WindowHandle` → `Box description` hexpand, margin-top 2, margin-bottom 7: `Label "title-1"` **`"Memory"`** + `Label total_ram "title-3"` halign end ellipsize middle selectable.
`Adw.ToastOverlay toast_overlay` → `Grid grid_graphs` margin-bottom 10 row-spacing 0:
- `Box box_system_memory` (col 0, row 0, **row-span 12**), vertical, `has-tooltip: true`:
  - caption row: **`"Memory usage over "`** + `graph_max_duration` (hexpand, halign start, valign start) + `max_graph_ram`.
  - `McGraphWidget usage_graph` vexpand/hexpand width-request 100, `base-color: bind template.base-color`, `scroll: true`.
- `Box box_swap_space` (col 0, **row 12, row-span 8**), margin-top 10, vertical:
  - caption row: **`"Swap usage over "`** + `Label` bound to `graph_max_duration.label` + `total_swap`.
  - `McGraphWidget swap_usage_graph` vexpand/hexpand width-request 100, **`base-color: bind template.memory-color`**, `scroll: true`.
- `Box box_mem_composition` (col 0, **row 20, row-span 1**), vertical: caption **`"Memory composition"`** + `$MemoryCompositionWidget mem_composition` width-request 100 height-request 25 vexpand, `base-color: bind template.base-color`.
- `PopoverMenu context_menu` (right-click, `memory.rs:276-299`).
Swap-visibility helper (`memory.rs:167-182`): toggling swap sets `box_system_memory`'s `GridLayoutChild.row_span` to **12** when visible, **20** when hidden, then `box_swap_space.set_visible(visible)`.
Toast: if `readings.mem_devices` is empty → `toast_overlay.add_toast(adw::Toast::new(&i18n("Getting additional memory information failed")))` (`memory.rs:380`, pot:2047).

### 2.2 Graphs
Creation (`memory.rs:692-714`): `usage_graph` gets 3 dataset groups — 0: `fill=None, dashed=true` (dataset_a); 1: `fill=None` (dataset_b); 2: defaults (solid, `FillToBottom`, opacity 100/255) (dataset_c); then `connect_datasets(0,1)`, `(1,2)`, `(2,0)`; `connect_to_settings`. `swap_usage_graph` gets 1 default group + `connect_to_settings`.
Static (`memory.rs:302-386`): `usage_graph.set_all_datasets_max_scale(mem_total as f32)`; `swap_usage_graph.set_all_datasets_max_scale(swap_total as f32)`; hide `box_mem_composition` if `performance-page-memory-composition-visible` false; set legend pictures: `legend_commited` → resource `/io/missioncenter/MissionCenter/line-dashed-mem.svg`, `legend_used` → `/io/missioncenter/MissionCenter/line-solid-mem.svg` (both SVGs are hardcoded `#62A0EA`, dashed array `0.8,0.8`); `total_ram` = `max_graph_ram` = `to_human_readable_nice(mem_total, MemoryBytes)`; `total_swap` = same for swap_total; hide swap graph if `performance-page-memory-swap-visible` false.
Clock/DIMM info (only if `mem_devices.len() > 0`): `speed` = `format!("{} MT/s", mem_devices[0].speed)` when `speed != 0`; `slots_used` = `i18n_f("{} of {}", [module_count, max_devices])` when `max_devices > 0` else `format!("{}", module_count)`; `form_factor` = first device's form factor if non-empty; `ram_type` = first device's RAM type if non-empty. Otherwise the toast above.

### 2.3 Update logic (`memory.rs:388-543`)
- `mem_avail = if mem_available > mem_total { mem_free } else { mem_available }`; `used = mem_total.saturating_sub(mem_avail)`; `standby = mem_total.saturating_sub(used + mem_free)` (procps meminfo semantics, comment links procps `meminfo.c#L736`).
- `usage_graph.add_data_point(vec![vec![committed], vec![dirty], vec![used]])` → dataset0=committed, 1=dirty(modified), 2=used.
- `total_ram` = human total; `max_graph_ram` = `to_human_readable_nice(usage_graph.get_dataset_max_scale(0), MemoryBytes)`.
- `swap_used = swap_total - swap_free`; `swap_usage_graph.add_data_point(vec![vec![swap_used]])`.
- `mem_composition.update_memory_information(mem_info)`.
- Labels (all `to_human_readable_nice(_, MemoryBytes)`): `in_use`=used, `available`=**`mem_available`** (not the clamped value), `committed`=committed, `cached`=cached.
- Swap: if `swap_total == 0` → disable the swap action, hide swap graph, hide `swap_available` and `swap_used`; else show and set `swap_available` = human(swap_total), `swap_used` = human(swap_total-swap_free).
- Compression (zswap + zram): `total_original = zswapped + zram_orig_size`, `total_compressed = zswap + zram_compr_size`; threshold `MIN_COMPRESSION_THRESHOLD = 1024*1024`; if `total_original >= 1 MiB` → show `compressed` = human(total_compressed) and `compression_savings` = human(total_original - total_compressed); else hide both.
- Tooltip labels (`tt_label_in_use` = used, `tt_label_modified` = dirty, `tt_label_standby` = standby, `tt_label_free` = free) all human-readable.
- `total_swap` label = human(swap_total).
`update_animations` (`memory.rs:545-555`): `usage_graph.update_animation(ticks)`, `swap_usage_graph.update_animation(ticks)`.

### 2.4 Context menu / actions (`memory.blp:192-256`, `memory.rs:184-274`)
Section 1: **`"Show Memory Composition"`** → `graph.memory_composition` (stateful, `performance-page-memory-composition-visible`, toggles `box_mem_composition` visibility); **`"Show Swap Usage"`** → `graph.swap_usage` (stateful, `performance-page-memory-swap-visible`, toggles `set_swap_space_graph_visible`).
Section 2: **`"Graph _Summary View"`** → `graph.summary` + **`"_View"`** submenu with the same 7 items as CPU.
Section 3: **`"_Copy"`** → `graph.copy`.
`new()` also connects `settings.connect_changed("performance-page-memory-swap-visible")` which activates `graph.swap_usage` when the stored value differs from action state (`memory.rs:929-948`).

### 2.5 `data_summary()` (`memory.rs:557-655`) — exact format
`unknown = i18n("Unknown")`, `na = i18n("N/A")`. `compressed`/`compression_savings` print `"N/A"` when hidden. `speed`/`slots_used`/`form_factor`/`ram_type` print `"Unknown"` if the label `uses_markup()`, else the label text (label text defaults to `"Unknown"` — set at `memory.rs:831-856`).

```
Memory

    {}

    In use:         {}
    Available:      {}
    Committed:      {}
    Cached:         {}
    Swap available: {}
    Swap used:      {}
    Compressed:     {}
    Savings:        {}

    Speed:       {}
    Slots used:  {}
    Form factor: {}
    Type:        {}
```
Args: total_ram, in_use, available, committed, cached, swap_available, swap_used, compressed, savings, speed, slots_used, form_factor, ram_type.

### 2.6 Details panel (`memory_details.blp`, `Grid root` style `"layout-grid"`, margin-top 65, margin-bottom 10, row-spacing 5, column-spacing 0)
- (col0,row0) `Picture legend_used` (`can-shrink:false`, `content-fit: scale_down`); (col1,row0) caption **`"In use"`** + `in_use` "title-4"; (col3,row0) width-request 100 caption **`"Available"`** + `available`.
- (col0,row1) `Picture legend_commited` margin-top 7; (col1,row1) **`"Committed"`** + `committed`; (col3,row1) **`"Cached"`** + `cached`.
- (col1,row2) **`"Swap used"`** (its caption `visible: bind swap_used.visible`) + `swap_used`; (col3,row2) **`"Swap available"`** (`visible: bind swap_available.visible`) + `swap_available`.
- (col1,row3) **`"Compressed"`** (`visible: bind compressed.visible`) + `compressed`; (col3,row3) **`"Savings"`** (`visible: bind compression_savings.visible`) + `compression_savings`.
- Static rows: (col1,row4) margin-top 20 caption **`"Speed:"`** / (col3,row4) `speed` margin-top 20; row5 **`"Slots used:"`**/`slots_used`; row6 **`"Form factor:"`**/`form_factor`; row7 **`"Type:"`**/`ram_type`. All caption-styled, value labels selectable; `speed`/`slots_used`/`form_factor`/`ram_type` initialize to `"Unknown"`.
Infobar margins same as CPU (10 collapsed / 65 expanded, `memory.rs:875-889`).

### 2.7 Tooltip (`memory_info_tooltip.blp`, registered on `box_system_memory` `connect_query_tooltip`, `memory.rs:719-735`)
Two-column `Box root` spacing 5: left vertical with labels **`"In Use:"`**, **`"Modified:"`**, **`"Standby:"`**, **`"Free:"`**; right vertical width-request 110 with right-aligned values `label_in_use`, `label_modified`, `label_standby`, `label_free`.

---

## 3. Memory composition widget — `src/performance_page/widgets/mem_composition_widget.rs`

Properties: `base_color` (default black), `range_min` (0.0), `range_max` (100.0) — the latter two are unused by the renderer. Constants: `GRAPH_RADIUS = 7.`, `FILL_ALPHA = 50./255.`.
Rendering (`render`, lines 190-329):
1. Build a scale-factor-sized checkerboard texture from `base_color` (pixel_size = `scale_factor.trunc()`, 2×pixel_size square, opaque pixels `[r,g,b,50]` interleaved with `[0,0,0,0]`) — used to "hatch" the modified and standby segments.
2. Rounded-clip to `GRAPH_RADIUS`; `total = mem_total` (or 1 if 0).
3. `stroke = Stroke::new(1.)`, stroke colour = base colour alpha 1, fill colour = base colour alpha `FILL_ALPHA`. Whole bar outline drawn afterwards: 1px border, alpha 1.
4. Segments, left → right, each width `trunc(width * value/total)`:
   - **used** = `total - mem_avail` (same clamp as memory page) → filled; tooltip anchored at `x` (right edge of the bar): `i18n_f("In use ({})\n\nMemory used by the operating system and running applications", [human(used)])`.
   - **modified** = `mem_info.dirty` → filled **plus** hatched via `push_repeat`+texture; tooltip `i18n_f("Modified ({})\n\nMemory whose contents must be written to disk before it can be used by another process", [human(modified)])`.
   - **standby** = `total - used - mem_free` → no fill, hatched only; tooltip `i18n_f("Standby ({})\n\nMemory that contains cached data and code that is not actively in use", [human(standby)])`.
   - **free** = drawn as a 1-px wide unfilled bar at the current x (i.e. just a vertical stroke); tooltip anchored at `width + 1.`: `i18n_f("Free ({})\n\nMemory that is not currently in use, and that will be repurposed first when the operating system, drivers, or applications need more memory", [human(free)])`.
5. Tooltips: on hover, pick the last segment whose anchor x is `>= pointer x` (iterating in reverse), else index 0. Default tooltip text is the untranslated literal `"Memory Composition"` (also used for keyboard-driven tooltips).
(Exact msgids verified in `po/missioncenter.pot:1878-1914`.)

---

## 4. `SummaryGraph` widget — `src/performance_page/summary_graph.rs` + `summary_graph.blp`

Layout: vertical `Box` margin-top 10, margin-bottom 10, margin-end 10; inner `Box` spacing 10:
`Image drag_handle_icon` (styles `"drag-handle"`, margin-start 3, icon `list-drag-handle-symbolic`, hidden) → `McGraphWidget graph_widget` (width-request 80, height-request 50, **`data-points: 60`**, **`grid-visible: false`**, `scroll` default true) → vertical `Box` spacing 3 width-request 145 with `label_heading` (plain, ellipsize end), `label_info1` ("caption", ellipsize end), `label_info2` ("caption", ellipsize end) → `Switch enabled_switch` (hidden, halign end, valign center, active true).
Properties: `base_color` (delegates to `graph_widget`), `heading`→`label_heading.text`, `info1`/`info2`→labels with `visible = !text.is_empty()` (`summary_graph.rs:239-272`). `device_type: DeviceType`, `user_activated: Cell<bool>` (default true).
Switch handler (`summary_graph.rs:305-340`): ignored while `user_activated == false` (programmatic updates); otherwise reads `performance-sidebar-device-overrides`, parses, inserts `widget_name() → Show|Hide` for the switch state, serializes and stores.
Edit mode (`summary_graph.rs:362-375`): `set_edit_mode(edit_mode, switch_enabled)` — drag handle + switch visible iff edit_mode; `set_switch_active(x)` toggles `user_activated` around the set to avoid feedback.
Drop hint API: `show_drop_hint_top()` (margin-bottom 20, `prepend`), `show_drop_hint_bottom()` (margin-top 20, `append`), `hide_drop_hint()`, `is_drop_hint_visible/top/bottom`.
Override persistence model (`summary_graph.rs:87-165`): `encode_device_name` escapes `%`→`%25`, `;`→`%3B`, `:`→`%3A`; `decode_device_name` is lenient with legacy values; `parse_device_overrides` splits on `;`, `rsplit_once(':')`, actions `"show"`/`"hide"`; `serialize_device_overrides` sorts entries and joins with `;` (order-independent, test `serialized_overrides_do_not_depend_on_insertion_order` expects `"cpu:show;disk-sda:hide;gpu-0:show;net-eth0:hide"`); `resolve_device_visibility` = override if present else `category_visible`.
`NetworkGroup` maps `ConnectionKind`: Wired/Wireless/Vpn/Virtual(Docker|Multipass|Bridge|Virtual)/Other(Bluetooth|InfiniBand|Wwan|Other) to settings keys `performance-show-network-wired|-wireless|-vpn|-virtual|-other` (`summary_graph.rs:40-67`).

### 4.1 `SidebarDropHint` — `widgets/sidebar_drop_hint.rs`
3-px tall, 2-px rounded filled bar; colour defaults to `gdk::RGBA(0.2, 0.51, 0.89, 1.)` but is immediately set from `adw::StyleManager::accent_color_rgba()` and updated on accent change; widget is set insensitive.

---

## 5. PerformancePage shell — `src/performance_page/mod.rs` + `page.blp`

### 5.1 Layout (`page.blp`)
Root `Adw.BreakpointBin` width-request 300 height-request 200. `Adw.Breakpoint breakpoint` (condition set in code: `max-width: 570sp`, `mod.rs:2289`). `Adw.OverlaySplitView page_content` with **`sidebar-position: end`**, min-sidebar-width 240, max-sidebar-width 270:
- sidebar = `ScrolledWindow` width-request 270, margin 5 → `Stack info_stack` (crossfade) with pages `"placeholders"` (`$Placeholder` profile `performance-page-info`) and `"content"` (`Adw.Bin info_bar`, margin 5).
- content = `Stack content_stack` (crossfade) with `"placeholders"` (`$Placeholder` profile `performance-page-graph`) and `"content"` = `Stack page_stack` (margin-top/start/end 10, crossfade).
The real sidebar `gtk::ListBox` lives in `window.blp:93` (style `"navigation-sidebar"`) and is injected via the `sidebar` property; `window.blp:194-199` binds `infobar-visible` to the info button, `summary-mode` = `!header_bar.visible`, and `sidebar-edit-mode` to the "Edit Sidebar" toggle.
On `breakpoint.apply` (`mod.rs:2292`): `breakpoint_applied = true`, `page_content.collapsed = true`, `show_sidebar = false`. On `unapply`: `collapsed = false`, `show_sidebar = !summary_mode`.
`info_bar.set_child(visible page's "infobar-content" property)`; re-set whenever `page_stack` visible child changes (`mod.rs:2363-2381`).
`set_initial_readings` builds all pages and then, on idle, switches `content_stack` and `info_stack` to `"content"` (`mod.rs:2637-2657`).

### 5.2 Base colours (`mod.rs:103-116`)
| constant | hex | comment |
|---|---|---|
| `CPU_BASE_COLOR` | `#1c71d8` | GNOME Blue 4 |
| `MEMORY_BASE_COLOR` | `#62a0ea` | GNOME Blue 2 |
| `DISK_BASE_COLOR` | `#26a269` | comment says "Orange 2" but value is green |
| `NETWORK_BASE_COLOR` | `#dc8a-dd`→`#dc8add` | Purple 1 |
| `FAN_BASE_COLOR` | `#813d9c` | Purple 4 |
| `GPU_BASE_COLOR` | `#f66151` | Red 1 |
| `BATTERY_BASE_COLOR` | `#57e389` | Green 2 |
Used as `gdk::RGBA::new(c[0]/255., c[1]/255., c[2]/255., 1.)` for both the summary graph and the page. Quirk: the **memory page's swap graph** (`memory-color`) is set to `DISK_BASE_COLOR` (`mod.rs:1140-1145`), so the swap graph is green while everything else memory-related is blue.

### 5.3 Page names / widget names / `Pages` enum
`Pages` (`mod.rs:118-126`): `Cpu`, `Memory`, `Disk`/`Network`/`Gpu`/`Fan`/`Battery` as `HashMap<String,(SummaryGraph, Page)>`.
Page name = sidebar `widget_name()`:
- cpu `"cpu"`, memory `"memory"`
- disk `"disk-{disk.id}"` (`mod.rs:1207`)
- net `"net-{connection.id}"` (`mod.rs:1287`)
- gpu `"gpu-{gpu.id}"` (`mod.rs:1383`)
- fan `"fan-{hwmon_index}-{fan_index}"` (`mod.rs:1485`)
- battery `"battery-{power_supply.map(|x| !x as u8).unwrap_or(2)}-{name}"` (`mod.rs:1556`)
`canonical_key` (`mod.rs:204`): category order `["cpu","memory","disk","net","gpu","fan","battery"]` by `starts_with` prefix, unknown = 7, tie-broken by name.
`view_action_name` (`mod.rs:215`): prefix before first `-` mapped `cpu|memory|disk|net→network|gpu|fan|battery`, else `None`.
Device index suppression: index shown only when there is more than one device of that kind (`hide_index = len == 1`; disks additionally skip `capacity_bytes == 0`, `mod.rs:1953`).

### 5.4 Page setup
CPU (`mod.rs:1044-1088`): summary `DeviceType::Cpu`, name `"cpu"`, heading `i18n("CPU")`, info1 initial `"0% 0.00 GHz"`, info2 = `format!("{:.0} °C", temperature_celsius)` if present; one default dataset (`Fixed`, high 100) + `connect_to_settings`; page base colour + `set_static_information(readings)`; `page_stack.add_named(&page, Some("cpu"))`.
Memory (`mod.rs:1090-1154`): summary `DeviceType::Memory`, heading `i18n("Memory")`, info1 initial `"0/0 GiB"`, info2 `"0%"`; 2 datasets both `Fixed` with `high_watermark = mem_total`: dataset 0 `fill=None, dashed=true` (committed), dataset 1 default fill (used); the `connect_datasets` calls are **commented out** (`mod.rs:1116-1117`); the page's `memory_color` is set to `DISK_BASE_COLOR`.
Disk (`mod.rs:1211-1271`): heading via `update_disk_heading`; info1 = model if present; info2 = `"{:.0}%"` of `busy_percent` plus, if `temperature_milli_k` present, `" ({:.0} °C)"` computed as `(temp_mk as i32 + MK_TO_0_C) as f64 / 1000.` with `MK_TO_0_C = -273150` (`mod.rs:82`); 1 default dataset (`Fixed`, high 100).
Network (`mod.rs:1291-1381`): heading = `format!("{} ({})", ConnectionKind::as_str_name(), if_name)` — e.g. `"Wired (enp3s0)"`, `"VPN (wg0)"` (proto names: Bluetooth, Bridge, Docker, InfiniBand, Multipass, Virtual, VPN, Wired, Wireless, WWAN, Other); 2 datasets: both `ScaleUp` + `RoundingSettings::Pow2`, dataset 0 also `fill=None, dashed=true`; `connect_datasets(0,1)` and `(1,0)`; if `max_speed_bytes_ps` is present and `performance-page-network-dynamic-scaling` is false → dataset 0 `Fixed` with `max_scale = max_speed`; a settings listener flips dataset 0 between `ScaleUp` and `Fixed(max_speed)` (only registered when `max_speed` exists). `set_static_information(connection)`.
GPU (`mod.rs:1387-1449`): heading `i18n_f("GPU {}", [index])` when index known else `i18n_f("GPU", &[])`; info1 = `device_name` else `i18n("Unknown")`; info2 built with `ArrayString<256>`: `"{v}%"` for `utilization_percent`, then `" ({v:.2}°C)"` for `temperature_c` (2 decimals, no space before °C); 1 default dataset.
Fan (`mod.rs:1489-1537`): heading `i18n_f("Fan {}", [index])` else `i18n("Fan")`; 1 dataset with `ScalingSettings::StickyUp`; no info1/info2 at creation.
Battery (`mod.rs:1564-1610`): heading `i18n_f("Battery {}", [index])` else `i18n("Battery")`; 1 default dataset.

### 5.5 Sidebar row content per device type (set in `set_up_pages`/`update_readings`)
| device | heading | info1 | info2 | graph series (default dataset settings) |
|---|---|---|---|---|
| CPU | `i18n("CPU")` | cpu `name`, else the info2 string (`mod.rs:1878-1883`) | `"{round(total_usage)}%"` + optional `" ({temp:.0} °C)"` (`write!("{}%", readings.cpu.total_usage_percent.round())`, `" ({:.0} °C)"`) | `[total_usage_percent]`, Fixed 0..100 |
| Memory | `i18n("Memory")` | `"{used} {total}"` (both human-readable, space-separated) | `"{round(used/total*100)}%"` | `[committed, used]` (0 dashed/no-fill, 1 filled), Fixed, high = mem_total |
| Disk | `update_disk_heading`: kind (HDD/SSD/NVMe/eMMC/SD/iSCSI/Optical/Floppy/Thumb Drive/Drive) or `i18n_f("{} {} ({})", [kind, index, disk_id])` when index known | model (once) | `"{busy:.0}%"` + optional `" ({temp:.0} °C)"` | `[busy_percent]`, Fixed 0..100 |
| Network | `"{kind} ({if_name})"` | not connected: `i18n_f("{}", [state.as_str_name()])`; connected: `i18n_f("{}: {}", ["S", sent_human])` | not connected: `""`; connected: `i18n_f("{}: {}", ["R", recv_human])` (human via `NetworkBytesPerSecond`) | `[tx_rate_bytes_ps, rx_rate_bytes_ps]`, ScaleUp + Pow2 |
| GPU | `i18n_f("GPU {}",[i])` / `i18n("GPU")` | device_name or `i18n("Unknown")` (creation only) | creation: `"{util}%"` + `" ({temp:.2}°C)"`; update: `"{util}%"` + `" ({round(temp)} °C)"` (note differing decimals/space) | `[utilization_percent]` (skipped when None) |
| Fan | `i18n_f("Fan {}",[i])` / `i18n("Fan")` | `fan_label` else `temp_name` | `"{pwm*100:.0}%"` + temp if `pwm_percent` present, else `"{rpm} RPM"` + temp; temp = `" ({:.0} °C)"` from `(temp_amount as i32 + MK_TO_0_C) as f32 / 1000.0` | `[rpm]`, StickyUp |
| Battery | `i18n_f("Battery {}",[i])` / `i18n("Battery")` | model or `""` | `"{percentage*100:.0}%"` + optional `" ({} °C)"` (raw temp, unformatted) | `[percentage*100]`, Fixed 0..100 |
Network additionally: `summary.set_opacity(if state == Unavailable { 0.6 } else { 1. })` (`mod.rs:1981-1983`).

### 5.6 Per-device visibility (`device_visible`, `mod.rs:228-247`)
Category flag: Disk → `performance-show-disks`; Network(group) → `performance-show-network && performance-show-network-<group>`; Gpu → `performance-show-gpus`; Fan → `performance-show-fans`; Battery → `performance-show-batteries`; Cpu/Memory/Unspecified → always `true`. Then `resolve_device_visibility(widget_name, overrides, category)`. `graph_shown = sidebar_edit_mode || device_visible`. `row_shown` returns `true` for non-SummaryGraph rows. `refresh_device_visibility` (`mod.rs:1614`) re-applies `set_switch_active(device_visible)` to every row and invalidates the list filter; bound to changes of `performance-sidebar-device-overrides` and the 9 `performance-show-*` keys (`mod.rs:1673-1711`).

### 5.7 Sidebar ordering & drag/drop
- `sidebar_rank: HashMap<name, usize>`; the ListBox uses a sort func ordering by `(0, rank, 0, "")` for ranked rows and `(1, 0, category, name)` for unknown rows (`mod.rs:430-457`).
- `load_sidebar_order`/`store_sidebar_order` (`mod.rs:263-291`): split/join on `;`, names run through `encode/decode_device_name`; write is skipped when unchanged.
- `merge_new_devices` (`mod.rs:293-322`): sorts new names by `canonical_key`, inserts each unknown name after the last entry with a matching category that sorts before it, else after the last entry of that category, else after the last entry that is canonically smaller, else at index 0.
- `prune_sidebar_order` (`mod.rs:86-98`): drops absent names from the tail while `len > MAX_REMEMBERED_DEVICES = 256`, never dropping present ones.
- `rebuild_sidebar_rank` (`mod.rs:324-355`) guarded by `rebuilding`, ends with `invalidate_sort()`.
- `update_readings` re-runs `rebuild_sidebar_rank` if any current graph name is unranked (`mod.rs:2162-2173`).
- Drag source (`mod.rs:919-1042`): created per row with `actions = empty`; set to `MOVE` only in edit mode. `prepare` refuses when actions lack MOVE, sets a row-paintable drag icon, provides the graph's `widget_name()` as a string value. `drag_begin` unselects all rows, sets every *other* row insensitive and hides the dragged row. `drag_end` restores sensitivity/visibility and clears hints/icon/content.
- Drop target on the ListBox (`mod.rs:467-598`): `preload`, type STRING. `motion(y)`: hides all hints; locates the `SummaryGraph` row under `y`; if none and `y < 10` → no action; if none at all → the hint goes **bottom of the last visible graph**; otherwise hint goes **top** of the hovered graph. Returns `gdk::DragAction::MOVE` / `empty()`. `leave` hides all hints. `drop` reads the dragged name, finds the graph with a visible hint, and calls `move_in_saved_order(dragged, target, after_target)`.
- `move_in_saved_order` (`mod.rs:357-383`): no-op if dragged == target; loads the saved order, removes the dragged name, inserts it before/after the target's index (warning when the target isn't in the saved order), stores.
- Selection helpers: `select_nearest_shown_row(index)` searches forward then backward for a shown row (`mod.rs:604-638`); `select_page(name)` selects the row or the nearest shown one, falling back to row 0 (`mod.rs:641-660`); `select_first_shown_in_category(action_name)` (`mod.rs:663-684`); `select_nth_shown_device(nth)` (`mod.rs:2667-2684`). Row selection persists `performance-selected-page` and switches `page_stack`, syncing the `graph.<view>` action states (`mod.rs:388-428`).
- Removal (`remove_pages`, `mod.rs:1724-1765`): when a device disappears, removes the summary graph from the map, removes the page from the stack, removes the row, and if it was selected selects the nearest shown row.

### 5.8 Edit mode (`mod.rs:686-705`, `2686-2776`; UI `window.blp:39-69`)
Entering/leaving edit mode calls `graph.set_edit_mode(edit_mode, device_visible(graph))` for all rows and sets drag actions to `MOVE`/`empty`, then `sidebar().invalidate_filter()` (hidden rows become visible) and re-selects the active page. Buttons (in the sidebar header bar, visible only in edit mode):
- `list-enable-all-symbolic`, tooltip **`"Enable All"`** → `sidebar_enable_all()` → `set_all_enabled(true)`
- `list-disable-all-symbolic`, tooltip **`"Disable All"`** → `sidebar_disable_all()` → `set_all_enabled(false)`
- `arrow-circular-top-right-symbolic`, tooltip **`"Reset to Default"`** → `sidebar_reset_to_default()`
- Toggle `edit-symbolic`, tooltip **`"Edit Sidebar"`**; window title widget is `Adw.WindowTitle` with title **`"Devices"`**.
`set_all_enabled(enabled)`: no-op outside edit mode; inserts `Show`/`Hide` for **every** row into the overrides map and calls `set_switch_active`, skipping the write when nothing changed.
`sidebar_reset_to_default()`: no-op outside edit mode; sets `performance-sidebar-order` and `performance-sidebar-device-overrides` to `""` and `settings.reset(...)` on `performance-show-disks`, `-network`, `-network-wired/-wireless/-vpn/-virtual/-other`, `-gpus`, `-fans`, `-batteries` (note: not `performance-show-cpu`/`-memory`).

### 5.9 Summary mode
Action `graph.summary` (stateful, `mod.rs:728-749`): toggles `summary_mode` property and, if the narrow breakpoint is not applied, sets `page_content.show_sidebar = !new_state`. The property propagates: `page.blp`/window binds `summary-mode` from the window (`window.blp:29`), the window's `summary-mode` is `!header_bar.visible` (`window.blp:196`), and the window hides the sidebar and shows the bottom bar (`window.rs:1011-1027`, `1208-1212`). Each page's `summary_mode` property is bound in `configure_page` (`mod.rs:912-917`) but is otherwise **unused** inside the CPU/memory/other page implementations — summary mode is purely window-chrome behaviour (hide header bar, show bottom view switcher, hide sidebar, block tab/sidebar toggling actions).

### 5.10 Action groups
The PerformancePage inserts group **`graph`** (`mod.rs:2286-2287`) with: stateful `summary`, and stateful view actions `cpu` (initial true = current), `memory`, `disk`, `network`, `gpu`, `fan`, `battery`; each view action selects the first shown row of its category and only then flips states. Each child page also inserts its own group **`graph`** (cpu: `overall`, `all-processors`, `all-threads`, `all-threads-stacked`, `kernel_times`, `copy`, `cpu_power`, `cpu_clocks`, `cpu_temperature`, `cpu_none`; memory: `copy`, `memory_composition`, `swap_usage`). Menu items such as `graph.summary`, `graph.cpu`, `graph.disk`, `graph.network` therefore resolve to the parent page's group by widget-hierarchy action lookup — a port must replicate this fallback (child group first, then parent).
Keyboard shortcuts in `window.rs:39+` call `select_nth_shown_device(0/1/2/...)` (device 1/2/3 …) when the performance page is active.

### 5.11 Migration
On `set_up_pages` (`mod.rs:1642-1660`): if `performance-sidebar-hidden-graphs` is non-empty, each of its `;`-separated names is inserted into `performance-sidebar-device-overrides` as `Hide` **only if not already present**, then the old key is cleared.

---

## 6. Details worth flagging for parity

1. **No `"% Utilization"` / graph-internal axis labels exist.** The only axis/scale texts are the blp caption rows: CPU `"Utilization over "` + duration + `"100%"`; Memory `"Memory usage over "` / `"Swap usage over "` + duration + human total; CPU bottom graph `"Power draw over "` / `"Clock speed over "` / `"Temperature over "` + duration + formatted max. Duration with defaults is `"60 seconds"`.
2. **Opaque strokes, translucent fills:** dataset opacity affects only the fill, never the line.
3. `performance-show-cpu` / `performance-show-memory` keys exist but are never read by the performance page.
4. Memory page's swap graph uses `DISK_BASE_COLOR`; the memory legend SVGs are hardcoded `#62A0EA` regardless of the runtime base colour.
5. CPU sidebar `info1` fallback: when `cpu.name` is `None`, the computed info2 string is written into **info1** and info2 is left unchanged (`mod.rs:1878-1883`).
6. GPU info2 formatting differs between creation (`{v:.2}°C`, no space) and refresh (`{round(v)} °C`) — reproduce as-is or unify deliberately.
7. `"Getting additional memory information failed"`, `"Memory Composition"` (tooltip), and the `"(?)"` indicator are the only user-visible strings that are *not* translated from these files (`"Memory Composition"` is a bare string literal).
8. The `cpu_details.blp` `virtualization` label ships with the hardcoded untranslated text `"Unknown"`.
9. `po/missioncenter.pot` has zero `msgctxt` entries: every `_()`/`i18n()` on these pages is plain gettext, and no `pi18n` context needs reproducing.
