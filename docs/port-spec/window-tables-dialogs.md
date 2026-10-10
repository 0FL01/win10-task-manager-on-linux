> Frozen historical GTK specification. `src/`, `resources/`, schema and Meson paths below refer to the
> [root tree at `671bc544ac4f7a3149c53db6df8a453252547d0a`](https://github.com/0FL01/win10-task-manager-on-linux/tree/671bc544ac4f7a3149c53db6df8a453252547d0a),
> not the active egui workspace. Graph-widget paths refer to
> [upstream `8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc`](https://gitlab.com/mission-center-devs/graph-widget/-/tree/8aadf1cd9df0dbaa75a909ca9e3af43d4aac47dc).
> Its GUI-free core is now local at `crates/mc-graph-core/`; SVGs used by egui are in
> `crates/mc-ui/assets/icons/`. See [provenance](../PROVENANCE.md).

Below is the functional spec. All strings are quoted verbatim from source (they are gettext msgids); `_()` in `.blp` = translatable.

---

# 1. Main window

## 1.1 Template / metrics — `resources/ui/window.blp`, `src/window.rs`
- `Adw.ApplicationWindow`, `width-request: 360`, `height-request: 400`, `default-width: 800`, `default-height: 480`, `title: "Mission Center"` (blp:23-28). `collapse_threshold: 1000` (blp:30).
- Window props: `summary_mode: bind performance_page.summary-mode` (blp:29).
- Runtime overrides: `set_default_size(settings "window-width"/"window-height")` (application.rs:151-152; schema defaults 600×400), `set_maximized(settings "is-maximized")` (application.rs:169).
- Content = `Adw.OverlaySplitView split_view`, `show-sidebar: bind toggle_sidebar_button.active bidirectional` (blp:34-35).
- `content: Box(vertical)` containing `Adw.ViewStack stack` + `[bottom] Adw.ViewSwitcherBar bottom_bar`.

## 1.2 Sidebar (`split_view.sidebar`) — blp:37-101
- `Adw.HeaderBar`: `[start]` Box `visible: bind toggle_sidebar_edit_mode_button.active`, buttons:
  - `sidebar_edit_mode_enable_all` icon `list-enable-all-symbolic`, tooltip "Enable All"
  - `sidebar_edit_mode_disable_all` icon `list-disable-all-symbolic`, tooltip "Disable All"
  - `sidebar_edit_mode_reset` icon `arrow-circular-top-right-symbolic`, tooltip "Reset to Default"
  - Clicked → `performance_page.sidebar_enable_all()/sidebar_disable_all()/sidebar_reset_to_default()` (window.rs:810-838).
- `title-widget: Adw.WindowTitle { title: "Devices" }`; `[end]` `ToggleButton toggle_sidebar_edit_mode_button` icon `edit-symbolic`, tooltip "Edit Sidebar", `active: false`.
- `Stack sidebar_stack` (crossfade) pages: `"placeholders"` → `$Placeholder { profile: "sidebar" }`; `"content"` → ScrolledWindow(hscroll never) → `ListBox sidebar` style `navigation-sidebar`. `set_initial_readings` selects `"content"` (window.rs:1169).
- Row activated → switch main stack to `"performance-page"` (window.rs:797-808).

## 1.3 Header bar (`header_bar`) — blp:105-180
- `[start]` `ToggleButton toggle_sidebar_button`: `action-name: "win.toggle-sidebar"`, icon `sidebar-toggle-left-symbolic`, tooltip "Toggle Sidebar".
- `title-widget: Stack header_stack` (crossfade, `hhomogeneous: false`), pages:
  - `"view-switcher"` → `Adw.ViewSwitcher header_tabs { halign: center; policy: wide; stack: stack }`
  - `"search-entry"` → `Adw.Clamp(horizontal, hexpand, maximum-size: 400)` → `SearchEntry header_search_entry { placeholder-text: "Type a name or PID to search" }`.
- `[end]` Box:
  - `ToggleButton info_button`: styles `flat`, `flipped`, icon `sidebar-toggle-left-symbolic`, `visible: bind template.info-button-visible`, `active: false`, tooltip "Toggle Device Information".
  - `ToggleButton search_button`: style `flat`, `action-name: "win.toggle-search"`, icon `system-search-symbolic`, `visible: bind template.search-button-visible`, tooltip "Search".
  - `Button system_about`: `action-name: "app.system-about"`, tooltip "System Information", icon `info-outline-symbolic`.
  - `MenuButton menu_button`: icon `open-menu-symbolic`, tooltip "Main Menu", `menu-model: primary_menu`, `primary: true`.

**Visibility properties** (window.rs:386-396): `info-button-visible` = performance page active **and** `performance_page.info_button_visible()` (= `page_content.is_collapsed()`, performance_page/mod.rs:716-718); `search-button-visible` = apps page active **or** services page active. `notify_info_button_visible`/`notify_search_button_visible` fired in `update_active_page` (window.rs:447-448).

**Search behavior**
- `toggle-search` action (window.rs:488-519): stateful bool. Turning on: `search_button.set_active(true)`, header_stack child = `"search-entry"`, `header_search_entry.grab_focus()`, `select_region(-1,-1)`, `header_stack.set_visible(true)`. Turning off: if `window_width_below_threshold()` → `header_stack.set_visible(false)`; clear entry text; child = `"view-switcher"`.
- Entry `key_capture_widget = itself` (window.rs:907-908). `search-changed` → refilters both table views (`configure_search`, table_view/mod.rs:1029-1050, retried up to 5× every 100 ms from `realize`).
- `search-started` (Enter) and `stop-search` (Esc) → activate `win.toggle-search` (i.e. close search) when on apps/services page (window.rs:910-941).
- Page switch: if search button active, `visible-child-notify` activates `win.toggle-search` to close it first (window.rs:840-858).

## 1.4 Primary menu (`primary_menu`) — blp:228-266
```
section { item { custom: 'theme-selector' } }                       // ThemeSelector widget injected as popover child id "theme-selector" (window.rs:711-717), action "win.interface-style"
section { "_Preferences"            → app.preferences
          "_Keyboard Shortcuts"     → app.keyboard-shortcuts }
section { "Open _Wiki"  (verb-icon "external-link-symbolic") → app.open-wiki
          "Show _First Run Dialog" → app.first-run-dialog }
section { "_About Mission Center"   → app.about }
```
`app.open-wiki` opens `https://gitlab.com/mission-center-devs/mission-center/-/wikis/home` (application.rs:510-523). Note mnemonics are literal underscores in the msgids.

## 1.5 Page switcher / navigation
`Adw.ViewStack stack` pages (blp:185-217):

| name | title msgid | icon-name | child |
|---|---|---|---|
| `performance-page` | "Performance" | `speedometer-symbolic` | `$PerformancePage` (binds `infobar-visible: bind info_button.active bidirectional`, `summary-mode: bind header_bar.visible bidirectional inverted`, `sidebar`, `sidebar-edit-mode: bind toggle_sidebar_edit_mode_button.active bidirectional`) |
| `apps-page` | "Apps" | `overlapping-windows-symbolic` | `$AppsPage` |
| `services-page` (id `services_stack_page`) | "Services" | `system-run-symbolic` | `$ServicesPage` |

- `update_active_page()` (window.rs:400-458): sets the three boolean props from `stack.visible_child_name()`, notifies, then writes setting `window-selected-page` (string).
- Page persistence: `realize()` sets `stack.set_visible_child_name(settings "window-selected-page")` (window.rs:1042-1048); schema default `"performance-page"`.
- Actions / accels (window.rs:558-607): `win.select-tab-performance` `<Control>1 <Alt>1`; `win.select-tab-apps` `<Control>2 <Alt>2`; `win.select-tab-services` `<Control>3 <Alt>3`. Apps/Services activation is a **no-op while `summary_mode` is true**.
- Services page is hidden entirely when there are no services: `services_stack_page.set_visible(false)` unless `!system_services.is_empty() || !user_services.is_empty() || services_refresh_counter < 5` (window.rs:460-481, constant `SERVICES_REFRESH_COUNTER_MAX_FOR_EMPTY_LIST = 5`).
- `bottom_bar` (`Adw.ViewSwitcherBar`) visible bound to `summary-mode` inverted (window.rs:1213-1215).

## 1.6 Breakpoints / collapse / summary mode
- Breakpoint condition parsed as `format!("max-width: {}sp", collapse_threshold)` = `max-width: 1000sp` (window.rs:943-949). `window_width_below_threshold()` converts window px→`adw::LengthUnit::Sp` and compares to threshold (window.rs:720-727).
- **apply** (width ≤ 1000sp, window.rs:950-971): `bottom_bar.set_reveal(true)`; unless search is active `header_stack.set_visible(false)`; `apps_page.collapse()`; `services_page.collapse()`; if performance page active → `split_view.set_collapsed(should_hide_sidebar())`.
- **unapply** (window.rs:972-989): `header_stack.set_visible(true)`; `bottom_bar.set_reveal(false)`; `apps_page.expand()`; `services_page.expand()`; `split_view.set_collapsed(should_hide_sidebar())`.
- `should_hide_sidebar()` (window.rs:729-739) = `!performance_page_active || user_hid_sidebar || summary_mode || window_width_below_threshold()`.
- `performance_page_active` notify (window.rs:991-1009): if active → `set_collapsed(should_hide_sidebar)`, `set_show_sidebar(!should_hide_sidebar)`; else `set_collapsed(true)`, `set_show_sidebar(false)`.
- `summary_mode` notify (window.rs:1011-1028): summary on → `split_view.set_show_sidebar(false)`; summary off and width ≥ threshold → `set_collapsed(false)`, `set_show_sidebar(!user_hid_sidebar)`.
- Summary mode itself is the PerformancePage state bound to `header_bar.visible` inverted (blp:196) — i.e. summary mode ⇔ header bar hidden. Toggled via `graph.summary` / Ctrl+M (performance_page/mod.rs:731-747).
- `toggle-sidebar` action (window.rs:609-646), stateful, accel `<Control>T F9`: no-op while `summary_mode`; flips `split_view.shows_sidebar()`; inverts `user_hid_sidebar` only if the old state differed from it, and only "intentional" when on performance page (if not performance page and the new state hides the sidebar, it returns early without recording).

## 1.7 Window size persistence (`src/application.rs`)
- `connect_default_height_notify` → setting `window-height` (int); `connect_default_width_notify` → `window-width`; `connect_maximized_notify` → `is-maximized` (bool) (application.rs:122-169). Defaults: 600 / 400 / false.

## 1.8 Update interval & refresh
- Constants (application.rs:41-42): `INTERVAL_STEP = 0.05` (50 ms), `BASE_INTERVAL = 1.0`.
- Setting `app-update-interval-u64` (type `t`, range 10..200, default 20 → 1.00 s) is in 50 ms ticks (gschema:61-65). Deprecated `app-update-interval` (int) is unused ("DEPRECATED! NO LONGER IN USE!").
- On window creation (window.rs:1076-1082): `sys_info.set_update_speed(interval)`; `sys_info.set_core_count_affects_percentages(settings "apps-page-core-count-affects-percentages")`; `cached_refresh_ticks = interval`.
- `connect_changed("app-update-interval-u64")` (window.rs:1084-1111): re-`set_update_speed` on the live client and update the window's `cached_refresh_ticks`.
- Core-count setting also wired in application.rs:171-194 (`connect_changed` → `sys_info.set_core_count_affects_percentages`).

## 1.9 Reduced activity
- `reduced_activity()` = `self.is_suspended() && !self.is_active()` (window.rs:1272-1274).
- `set_reduced_activity(...)` is called (a) once at construction: `sys_info.set_reduced_activity(this.reduced_activity())` (window.rs:1113), and (b) from `sync_reduced_activity()` (window.rs:1276-1291, logs `"Reduced activity: {}"`), which is invoked from `connect_suspended_notify` (window.rs:1115-1117) and from `connect_is_active_notify` whenever the window becomes inactive (window.rs:900-905 — also calls `continue_refreshing()` there to avoid a stuck Ctrl-pause).
- Pause-on-Ctrl: key-pressed with `Control_L`/`Control_R` → `pause_refreshing()` → `is_refreshing_paused = true`; key-released → `continue_refreshing()` which, if it was paused, re-applies the single buffered `stored_readings` (window.rs:860-895, 1293-1305). While paused, `update_readings` stashes the incoming `Readings` and returns `true` (window.rs:1246-1250).

## 1.10 Animation timer
- `setup_animations()` (window.rs:1132-1154): `glib::timeout_add_local(Duration::from_millis(50), …)` (always `ControlFlow::Continue`), each tick builds
  `AnimationFrame { progress: clamp( (now_us − last_refresh) / 1e6 / (cached_refresh_ticks * 0.05), 0.0, 1.0 ) as f32, grid_offset: global_scroll_ticks }`
  (`now_us` = `SystemTime::now()` micros since epoch, window.rs:1122-1130).
- Per refresh (`update_readings`, window.rs:1241-1270): `last_refresh = now`; `grid_offset = global_scroll_ticks.wrapping_add(1)`; `global_scroll_ticks = grid_offset`; then `update_animations(AnimationFrame { progress: 0.0, grid_offset })`. `progress == 0.0` signals "new cycle" to graphs (graph-widget/core/src/animation.rs:23-29); graph widget stores `scroll_offset = grid_offset` only when `progress == 0.0` (graph-widget/src/widget.rs:513-531). `update_animations` forwards to `performance_page.update_animations`.
- `readings.includes_app_data` gates apps+services updates; otherwise only the performance page is updated.

## 1.11 `app.about` dialog (application.rs:595-706)
`adw::AboutDialog` with: `application_name "Mission Center"`, `application_icon "io.missioncenter.MissionCenter"`, `developer_name "Mission Center Developers"`, `developers ["Romeo Calota", "QwertyChouskie", "jojo2357", "Jan Luca"]`, `translator_credits` = `i18n("translator-credits")`, `version` = build `VERSION` (1.2.0, Cargo.toml), `issue_url "https://gitlab.com/mission-center-devs/mission-center/-/issues"`, `copyright "© 2023-2025 Mission Center Developers"`, `license_type Gpl30`, `website "https://missioncenter.io"`, and a long HTML `release_notes` string (application.rs:615-690) with sections "Noteworthy changes:", "Minor features:", "Bug fixes:", "Translation updates" (30 bullet entries). Additional credit section title `"Standing on the shoulders of giants"` with entries: `"GTK https://www.gtk.org/"`, `"GNOME https://www.gnome.org/"`, `"Libadwaita https://gitlab.gnome.org/GNOME/libadwaita"`, `"Blueprint Compiler https://jwestman.pages.gitlab.gnome.org/blueprint-compiler/"`, `"NVTOP https://github.com/Syllo/nvtop"`, `"Workbench https://github.com/sonnyp/Workbench"`, `"And many more... Thank you all!"`.

## 1.12 Keyboard shortcuts (`resources/ui/keyboard_shortcuts.blp` + `src/window.rs:40-258`)
Dialog title "Keyboard Shortcuts". Sections and items (title → accelerator):

**"Global"**: "Open Menu" `F10`; "Close Window" `<Control>Q <Control>W`; "Open Preferences" `<Control>comma`; "Show Keyboard Shortcuts" `<Control>question`; "Show Performance Page" `<Alt>1 <Control>1`; "Show Apps Page" `<Alt>2 <Control>2`; "Show Services Page" `<Alt>3 <Control>3`; "Show System Information" `<Control>A`; "Toggle Sidebar" `F9 <Control>T`; "Pause Updates" (subtitle "Hold to pause") `<Control>`.

**"Performance Page"**: "Copy Device Information" `<Control>C`; "Toggle Summary Mode" `<Control>M`; "Select a Device" `F1...F8`.

**"Apps Page"**: "Search" `<Control>F`; "Collapse All" `<Control>L`; "Stop App or Process" `<Control>E`; "Force Stop App or Process" `<Control>X`; "Show Details Dialog" `<Control>I`.

**"Services Page"**: "Search" `<Control>F`; "Collapse All" `<Control>L`; "Start Service" `<Control>S`; "Stop Service" `<Control>E`; "Restart Service" `<Control>R`; "Show Details Dialog" `<Control>I`.

Action wiring (window.rs:218-254; handler table keyed by modifier mask then key, returns bool "handled" → `Propagation::Stop`):
- `NO_MODIFIER`: F1…F8 → `select_device_1..8` → `performance_page.select_nth_shown_device(0..7)`; returns true only when performance page active.
- `CONTROL`: F/f → toggle search (only if search button visible and inactive); A/a → `app.activate_action("system-about")`; M/m → `performance_page` action `graph.summary`; C/c → `graph.copy` on the visible performance sub-page; L/l → `apps-page.collapse-all` or `services-page.collapse-all`; E/e → apps: `process.stop`; services: `process.stop` **and** `service.stop`; X/x → apps: `process.force-stop`; services: `process.force-stop` + `service.stop`; I/i → apps: `process.details`; services: `process.details` + `service.details`; S/s → services: `service.start`; R/r → services: `service.restart`.
- App-level accels: `app.quit` `<primary>q` (application.rs:101), `app.preferences` `<Control>comma`, `app.keyboard-shortcuts` `<Control>question` (application.rs:538-539); `win.close` `<Control>W` (window.rs:660).
- Arrow keys on rows (Right/Left) are `ShortcutController` in the table view (see §2).

---

# 2. Apps page

## 2.1 Layout / headers
`Adw.Clamp` margins 20, `maximum-size: 3000`; `Stack content_stack` (crossfade) with `"placeholders"` (`$Placeholder { profile: "apps-page" }`) → `"content"` (blp:27-113). Switch happens on first `update_readings` via idle callback (`has_loaded`, apps_page/mod.rs:252-265).

- `h1`: style `title-1`, ellipsize end, halign start — text `ni18n_f("{} Running App", "{} Running Apps", n)` (apps_page/mod.rs:292-297).
- `h2`: style `faint-text` — `ni18n_f("{} Running Process", "{} Running Processes", n)` (apps_page/mod.rs:302-307).
- Toolbar row: Button `action-name: "apps-page.collapse-all"`, tooltip "Collapse all apps and processes", child Box with Image `list-collapse-symbolic` + Label `collapse_label` "Collapse All"; then `$ProcessActionBar process_action_bar`.
- `collapse()` hides `collapse_label` and `h2` and collapses the action bar; `expand()` shows both (apps_page/mod.rs:110-124).

## 2.2 Sections
`RowModel` section headers: `"Apps"` (`ContentType::SectionHeader`, `SectionType::FirstSection`) and `"Processes"` (SecondSection) (apps_page/mod.rs:85-94). Root process = min PID in `running_processes`; its children populate the Processes section (apps_page/mod.rs:311-326). `update_apps` fills the Apps section from `readings.running_apps`.

## 2.3 Table columns — `resources/ui/table_view/table_view.blp`
`Frame` → ScrolledWindow → `ColumnView column_view` (styles `rich-list`, `reorderable: true`, `show-row-separators: false`, `show-column-separators: bind template.show-column-separators`). Columns (id / title / attrs):

| id | title | notes |
|---|---|---|
| `name` | "Name" | `expand: true`, `resizable: false` |
| `pid` | "PID" | resizable |
| `cpu` | "CPU" | resizable |
| `memory` | "Memory" | resizable |
| `swap` | "Swap" | resizable |
| `drive` | "Drive" | resizable |
| `network` | "Network" | resizable |
| `gpu` | "GPU" | resizable |
| `gpu_memory` | "GPU Memory" | resizable |

- Header alignment (`adjust_view_header_alignment`, columns/mod.rs:190-229): the "Name" header gets `margin_start = 10`; every other header container gets `hexpand`, `width_request = 75`, label `halign End` + `justify Right`, and the sort arrow is reordered after the label with `halign Start`.
- **Dynamic second lines** (`update_column_titles`, table_view/mod.rs:852-948), format `"<Title>\n<value>"`: CPU → `round(total_usage_percent)%`; Memory → `round(used/total*100)%` where used = `mem_total − mem_available` (fallback `mem_free` when available > total); Drive → `round(max busy_percent of disks with capacity>0)%`; Swap → `"{:.0}%"` of `100 − swap_free*100/swap_total` (or `0%` when `swap_total == 0`); Network → `0` when no processes else `to_human_readable_nice(sum of rounded per-process network usage, NetworkBytesPerSecond)`; GPU → `round(avg utilization)%` (0% when no GPUs); GPU Memory → `round(used/total*100)%`. Name and PID titles are static.
- **Visibility rules** (apps_page/mod.rs:271-282, same in services_page/mod.rs:504-514): `network` column hidden when `readings.network_stats_error.is_some()`; `swap` column hidden when `readings.mem_info.swap_total == 0`.
- Per-column cell widget: `LabelCell` = `adw::Bin` wrapping a `gtk::Label` with `ellipsize Middle`, `hexpand`, `halign End` (columns/label_cell.rs:95-104). Each label cell is wrapped in a `TreeExpander` with `hide_expander(true)`, `indent_for_icon(false)`, `indent_for_depth(false)`, `halign End` (columns/mod.rs:83-103) — i.e. no expanders in numeric columns.
- **PID formatting**: `pid.to_string()`, but skipped (empty label) for `ContentType::SectionHeader | Service | App` (columns/pid.rs:29-54).
- **Formatters**: CPU `"{n}%"` (`cpu.round() as u32`) (cpu.rs:52-58); Memory `to_human_readable_nice(v, MemoryBytes)` (memory.rs:50-54); Swap uses the memory formatter (`swap.rs:25-31`); Drive `to_human_readable_nice(v, DriveBytesPerSecond)` (drive.rs:50-57); Network `to_human_readable_nice(v, NetworkBytesPerSecond)` (network.rs:50-56); GPU uses the CPU formatter; GPU Memory uses the memory formatter.
- **Hide-is-zero**: `set_label_zero()` sets `label.set_opacity(0.3)` when `get_hide_is_zero()` and `usage.abs() == 0.0`, else `1.0` (columns/mod.rs:68 `IS_ZERO_OPACITY: f64 = 0.3`, 287-293). Applied to cpu, memory, swap, drive, network, gpu, gpu_memory (not pid/name).
- **Sorting**: each column has a `CustomSorter`; all go through `compare_column_entries_by` which orders first by `section_type`, then by `content_type`, then by the column comparator, honouring the primary sort order (columns/mod.rs:252-285). Name sort is case-insensitive; memory/swap/disk/gpu-memory numeric.

## 2.4 Name column / NameCell (columns/name.rs, columns/name_cell.rs)
- Cell = `TreeExpander` (`hide_expander(false)`, `indent_for_icon(true)`, `indent_for_depth(true)`, `halign Start`, `width_request 218`) → `ListCell("column-view.show-context-menu")` with `is_tree_view(true)` → `NameCell` = Box with `gtk::Image` + `gtk::Label` (label `ellipsize Middle`).
- Per content type (name_cell.rs:211-300):
  - `SectionHeader`: icon hidden; name gets CSS class `heading`; margins start/top/bottom 6; expander `indent_for_icon(false)`.
  - `Service`: icon visible, `margin_end 10`, `pixel_size 16`; margins 0/3/3; posts a high-priority idle that activates `listitem.collapse` (rows start collapsed); `indent_for_icon(true)`.
  - `Process`: icon visible, margin_end 10, **pixel_size 16**; margins 0; `indent_for_icon(true)`.
  - `App`: icon visible, margin_end 10, **pixel_size 24**; margins 0; idle `listitem.collapse`; `indent_for_icon(true)`.
- Expander visibility: `set_hide_expander(children.n_items() == 0)`, updated on `items-changed`.
- Icons: `LightCachedIcon::Empty` → blank; `StringPayload` → themed icon by name (services: `service-running`/`service-failed`/`service-stopped`/`service-disabled`, models.rs:441-453); `AppCachedKey(id, 24)` for apps and `(id, 16)` for child processes → app icon cache; re-decoded on display scale-factor change (name_cell.rs:103-121).
- **No tooltip is set on the name cell**, and the command line is *not* shown in the table: `command_line` is stored on the `RowModel` (models.rs:134, 176) and displayed only in the Process Details dialog.

## 2.5 Context menus — table_view.blp:115-204
Shown by right-click (button 3) / long-press (touch) via `ListCell` gestures that activate `column-view.show-context-menu` with `(item_id, x, y, w, h)`; the row is selected by id first, then the menu model is chosen by `ContentType` (App/Process → `app_menu_model`, Service → `service_menu_model`) (table_view/mod.rs:271-321; widgets/list_cell.rs:94-189).

`app_menu_model`:
```
"Stop"                → process.stop
"Force Stop"          → process.force-stop
submenu "Send Signal":
  "Suspend (STOP)"    → process.suspend
  "Continue (CONT)"   → process.continue
  "Hangup (HUP)"      → process.hangup
  "Interrupt (INT)"   → process.interrupt
  "Terminate (TERM)"  → process.stop
  "Kill (KILL)"       → process.force-stop
  "User 1 (USR1)"     → process.user-one
  "User 2 (USR2)"     → process.user-two
"Details"             → process.details
```
`service_menu_model`: `"Start" → service.start`, `"Stop" → service.stop`, `"Restart" → service.restart`; `"Details" → service.details`.

## 2.6 Row expansion
- `ShortcutController` on the ColumnView (`propagation-phase: capture`): `Right` → `action(column-view.tableview-expand-row)`, `Left` → `action(column-view.tableview-collapse-row)` (table_view.blp:93-106).
- Expand action sets `row.set_expanded(true)`; collapse sets expanded false if expanded **and** it has children, otherwise (depth > 0) scrolls to and focuses the parent (table_view/mod.rs:323-429).
- Clicking/activating a row toggles `set_expanded(!is_expanded())` (table_view/mod.rs:431-470) — this is the double-click expand/collapse.
- `collapse-all` (`apps-page`/`services-page`): iterate the selection model, find `ContentType::SectionHeader` rows, `set_expanded(false)`, stop after 2 (apps_page/mod.rs:151-201, services_page/mod.rs:296-347). Not in the row context menu.

## 2.7 Process actions (apps_page/actions.rs)
Group `process`, actions: `stop`→`terminate_processes`, `force-stop`→`kill_processes`, `suspend`→`suspend_processes`, `continue`→`continue_processes`, `hangup`→`hangup_processes`, `interrupt`→`interrupt_processes`, `user-one`→`user_signal_one_processes`, `user-two`→`user_signal_two_processes` (PIDs sent as `Vec<u32>`), `details` → opens `ProcessDetailsDialog`.

- Each action's `enabled` = selection content type is `Process` or `App`; recomputed on `selected-item` notify (actions.rs:28-85).
- For `App` type the PID list comes from `app_pids()` = for each direct child row, `find_stoppable_child()` which descends past `bwrap` wrappers (actions.rs:162-196).
- **There are no confirmation dialogs** for stop/force-stop/signal actions; they are dispatched immediately over IPC.

## 2.8 Action bar (table_view/process_action_bar.blp, process_action_bar.rs)
Box, spacing 5: linked Box with
- Button `process.stop`, tooltip "Stop the selected app", Image `media-playback-stop-symbolic` + Label "Stop";
- Button `process.force-stop`, tooltip "Force stop the selected app", Image `cross-small-circle-filled-symbolic` + Label "Force Stop";
then Button `process.details`, tooltip "Show details of the selected app", Image `details-symbolic` + Label "Details".

Visibility (process_action_bar.rs:100-125): visible when selection is `Process` or `App`; unchanged for `SectionHeader`; hidden for anything else (`Service`). `collapse()`/`expand()` hide/show the three labels only (icons remain).

## 2.9 Process details dialog (table_view/process_details_dialog.blp / .rs)
`Adw.Dialog`, `width-request: 360`. Header title widget = Box(spacing 10) with Image `pixel-size: 24` + Label style `heading`; the dialog icon is set at **48 px** via `apply_to_image_custom_size(&icon, 48)` and the title is the row's name (process_details_dialog.rs:118-122).
Group 1 rows (`Adw.PreferencesRow`, `activatable: false`, height-request 50, margins 12, spacing 12; label left, value right `dim-label` `ellipsize: middle`):
- Label "PID" (visible for `Process`) / "App ID" (visible for `App`), value `id_value` default "N/A" — set from `model.id()`.
- "Type" → `ContentType` string: "Section Header" | "Service" | "App" | "Process" (row_model.rs:276-285).
- "Command Line" → `model.command_line()`; the whole row is hidden when the command line is empty (process_details_dialog.rs:132); value wraps (`wrap: true`, `wrap-mode: char`, `natural-wrap-mode: none`).
Group 2 title "Resource Usage": "CPU", "Memory", "Swap", "Drives", "Network", "GPU", "GPU Memory".
Bindings: cpu→`cpu-usage`, memory→`memory-usage`, swap→**`shared-memory-usage`** property formatted with the swap (= memory) formatter, drives→`disk-usage`, network→`network-usage`, gpu→`gpu-usage`, gpu_memory→`gpu-memory-usage`; all unbound on dialog close.

## 2.10 Merged stats, core count, hide-is-zero, separators, sorting persistence
- `use_merged_stats` (setting `apps-page-merged-process-stats`, default false): read once at table setup and on change (table_view/settings.rs:24-34); when true per-process stats use `process.merged_usage_stats(&process_map)` instead of `process.usage_stats` (models.rs:340-344).
- `apps-page-core-count-affects-percentages` (default false): pushed to the magpie client (application.rs:171-194, window.rs:1078-1080) — percentages then scale by core count.
- `apps-page-hide-is-zero` → static atomic `HIDE_IS_ZERO`, applied as opacity 0.3 (§2.3).
- `apps-page-show-column-separators` is `settings.bind(...)`-ed directly to the `ColumnView.show-column-separators` (table_view/settings.rs:16-22).
- **Sorting persistence** (`table_view/settings.rs:55-102`): gated by `apps-page-remember-sorting`. If false → keys `apps-page-sorting-column-name` = `""` and `apps-page-sorting-order` = Ascending are written and the function returns (no restore). If true → the column with id == saved string is looked up and `column_view.sort_by_column(col, order)`; saved order `255`/None → return without sorting; unknown value → critical log + Ascending fallback. Writing happens from `ColumnViewSorter::changed`: column id → `…-sorting-column-name` (string), order → `…-sorting-order` enum (Ascending/Descending; anything else → Ascending) (table_view/mod.rs:732-762).
- **Column order persistence** (`table_view/mod.rs:950-1022`): gated by `apps-page-remember-column-order`; if false the `…-column-order` key is reset to `""` and order is left alone. When true: all columns are removed and re-appended following the saved `;`-separated id list (unknown ids skipped; leftovers appended at the end). `items-changed` always writes the current order ids joined by `;` (trailing `;` popped).
- Keys are namespaced: `apps-page-*` vs `services-page-*` (table_view/mod.rs:54-92).
- Section/type ordering is enforced by the comparator, so section headers ("Apps" before "Processes") always stay grouped.

---

# 3. Services page

## 3.1 Layout / headers (`resources/ui/services_page/page.blp`, `src/services_page/mod.rs`)
Same Clamp/Stack structure; placeholder profile `"services-page"`; content Box(vertical, spacing 10) with `Box top_legend`, then `$TableView table_view { margin-top: 10 }`.

- `h1` (`title-1`, ellipsize end): `ni18n_f("{} Total Service", "{} Total Services", total)` (services_page/mod.rs:98-103).
- `h2` (`faint-text`, `visible: bind template.collapsed inverted`): if the filtered count is 0 → "No services match the current filters" (if any toggle active) else "No filters applied"; otherwise `ni18n_f("{} {} Service", "{} {} Services", filtered, [count, types])` where `types` is a `", "`-joined list of the active toggles in order Running, Failed, Stopped, Disabled (services_page/mod.rs:105-158; translator comment at line 151).
- Counts (services_page/mod.rs:449-473): a service counts as running if `running`; else failed if `failed`; else stopped if `enabled`; else disabled.
- Collapse button: tooltip "Collapse all expanded services", Image `list-collapse-symbolic`, Label "Collapse All" (label visible bound to `collapsed` inverted). `collapsed` is a real property; `collapse()`/`expand()` also collapse/expand both action bars and refresh headers (services_page/mod.rs:206-223).

## 3.2 Filter toggles (blp:125-212)
Linked, homogeneous `ToggleButton`s inside a Box(spacing 5, halign end); each has `name:` used by the filter and a Label that is only visible when the page is not collapsed. All start inactive (`active` not set):

| widget name | Label | tooltip | icon |
|---|---|---|---|
| `toggle_running` | "Running" | "Filter running processes" | `service-running` |
| `toggle_failed` | "Failed" | "Filter failed processes" | `service-failed` |
| `toggle_stopped` | "Stopped" | "Filter stopped processes" | `service-stopped` |
| `toggle_disabled` | "Disabled" | "Filter disabled processes" | `service-disabled` |

Note the labels in the `.blp` are plain (not `_()`) — "Running"/"Failed"/"Stopped"/"Disabled" are used untranslated in the toggles, while the header `h2` uses the translated `i18n("Running")` etc. Preserve this distinction for msgid parity.

Filter semantics (table_view/mod.rs:581-720): if no toggle is active → everything visible. Otherwise per row: `toggle_running` → `service_running()`; `toggle_failed` → `service_failed()`; `toggle_stopped` → `service_stopped()`; `toggle_disabled` → `!enabled && !running && !failed`. Section headers are always visible. Unknown toggle names log a warning.

"Remove Filters" button: tooltip "Remove all filters", icon `edit-clear-all-symbolic`, Label "Remove Filters" (visible when not collapsed) → action `services-page.remove-filters` deactivates all four toggles (services_page/mod.rs:349-364).

## 3.3 Action bars
- `$ProcessActionBar process_action_bar { visible: false; }` and `$ServiceActionBar service_action_bar { visible: true; }` are placed in the header row; their visibility is then driven by the selected row type: process bar visible for App/Process, service bar visible for Service (see §2.8, service_action_bar.rs:145-170).
- `service_action_bar.blp`: Box(spacing 5, valign center); linked Box with Button `service.start` (tooltip "Start the selected service", icon `media-playback-start-symbolic`, Label "Start"), Button `service.stop` (tooltip "Stop the selected service", icon `media-playback-stop-symbolic`, Label "Stop") — all with `visible: bind template.is_snap inverted`; Button `service.restart` (tooltip "Restart the selected service", icon `media-playlist-repeat-symbolic`, Label "Restart", also hidden on snap); Button `service_status` with `action-name: "service.details"`, tooltip "Show details of the selected service", icon `details-symbolic`, Label "Details" (always visible).
- `is_snap` = env var `SNAP_CONTEXT` present (service_action_bar.rs:95-98): hides Start/Stop/Restart.
- Enabled-state rules (services_page/actions.rs:29-103): action enabled iff selection is `ContentType::Service` **and** —
  - `service.start`: `!service_running()`
  - `service.stop`: `service_running()`
  - `service.restart`: `service_running()`
  - `service.details`: always (when a Service is selected)
  Re-evaluated on `selected-item` and `selected-item-running` notify. Activation calls `start_service/stop_service/restart_service(service_id)`; details opens `ServiceDetailsDialog`.
- Process actions from the services page reuse `apps_page::actions` (services_page/mod.rs:377-388) and are only meaningful for service rows that have a process.

## 3.4 Service details dialog (`service_details_dialog.blp` / `.rs`)
`Adw.Dialog`, `width-request: 360`; header title Label style `heading` "Service Details".
- Group `group_state`: title "State", description "The current state of the service". Header suffix: linked homogeneous Box with icon-only buttons `service.start` (tooltip "Start service", icon `media-playback-start-symbolic`) and `service.stop` (tooltip "Stop service", icon `media-playback-stop-symbolic`), plus Button `restart` (tooltip "Restart service", icon `media-playlist-repeat-symbolic`).
  Rows: "Name" → `label_name`; "Description" → `label_description`; "Status" → `label_running` = "Running" / "Failed" / "Stopped" (also live-updated on running-notify); `Adw.SwitchRow switch_enabled` title "Enabled" (active = `service_enabled()`; toggling calls `enable_service`/`disable_service`; a guard flag suppresses programmatic changes).
- Group `group_process`: title "Process", description "Process and user information". Rows: "Process ID" (pid or "N/A"), "User" (or "N/A"), "Group" (or "N/A"), "File Location" (or "Unknown"). The whole group is hidden if all four are empty; `group_empty` is true while all values are missing.
- Logs: `Expander logs_expander` labelled with a `heading` Label "Logs" containing `Overlay logs_overlay` → `TextView` (style `card`, margins 12, `monospace: true`, `editable: false`, `wrap-mode: word`). Logs fetched at realize via `sys_info.service_logs(service_id, NonZeroU32::new(pid))`; buffer set only when non-empty and the expander becomes visible only then; expander starts hidden and collapsed.
- Copy logs button: created in code — style `flat`, icon `edit-copy-symbolic`, tooltip **"Copy logs to clipboard"**, margins top 14 / end 2, valign Start, halign End, overlaid on `logs_overlay`; click copies the whole buffer text to the clipboard (service_details_dialog.rs:202-225).
- Snap behaviour: `SNAP_CONTEXT` present → `switch_enabled` insensitive, `box_buttons` and `restart` hidden (service_details_dialog.rs:162-166).
- **No error toasts** on the services page. The only toast in the app is on the Performance → Memory page: "Getting additional memory information failed" (performance_page/memory.rs:380).

---

# 4. Preferences dialog

`adw::PreferencesDialog` (empty template), pages added in order: General, Performance Page, Apps & Services, Units (preferences/mod.rs:120-131). No page-specific group titles on General/Apps & Services/Units (single untitled `Adw.PreferencesGroup`).

## 4.1 General — title "General", icon `general-properties-symbolic`
| Row | Type | Title | Subtitle | Range / step | Setting |
|---|---|---|---|---|---|
| `update_interval` | `Adw.SpinRow` inside an `Adw.ActionRow` | "Update Interval" | "In seconds" | lower 0.5, upper 10, step 0.05, `digits: 2`, hexpand false | `app-update-interval-u64` |

Stored value = ticks = `round(value / 0.05)` (i.e. 0.5 s → 10 … 10 s → 200); out-of-range writes log "Update interval out of bounds" and are dropped (general_page.rs:43-65). Load: `value = ticks * 0.05` (general_page.rs:118-119).

## 4.2 Performance Page — title "Performance Page", icon `speedometer-symbolic`
Group "Graph Settings":
- Row "Data Points" → `Gtk.Scale` lower 10, upper 600, step 1, `digits: 0`, `draw-value: true`, `value-pos: left`, hexpand. Setting `performance-page-data-points` (int).
- SwitchRow `smooth_graphs` "Smooth Graphs" / "Draw graphs as smooth line instead of jagged line" → `performance-smooth-graphs`.
- SwitchRow `sliding_graphs` "Sliding Graphs" / "Animate graphs (may negatively impact performance)" → `performance-sliding-graphs`.
- SwitchRow `network_dynamic_scaling` "Scale Network Graphs Dynamically" / "Set network graph to scale dynamically with actual usage or with channel max speed" → `performance-page-network-dynamic-scaling`.

Group "Devices":
- SwitchRow `show_disks` "Show Disk Information" / "Show disk information by default" → `performance-show-disks`.
- **ExpanderRow** `show_network` "Show Network Information" / "Show network information by default", `show-enable-switch: true`; its enable state binds to `performance-show-network` via `connect_enable_expansion_notify`; children: SwitchRow `show_network_wired` "Wired" → `performance-show-network-wired`; `show_network_wireless` "Wireless" → `performance-show-network-wireless`; `show_network_vpn` "VPN" → `performance-show-network-vpn`; `show_network_virtual` "Virtual" / "Docker, Multipass, Bridge" → `performance-show-network-virtual`; `show_network_other` "Other" / "Bluetooth, InfiniBand, WWAN" → `performance-show-network-other`.
- SwitchRow `show_gpus` "Show GPU Information" / "Show GPU information by default" → `performance-show-gpus`.
- SwitchRow `show_fans` "Show Fan Information" / "Show fan information by default" → `performance-show-fans`.
- SwitchRow `show_batteries` "Show Battery Information" / "Show battery information by default" → `performance-show-batteries`.

(NB: `performance-show-cpu` / `performance-show-memory` exist in the schema but have no preferences row.)

## 4.3 Apps & Services — title "Apps & Services", icon `overlapping-windows-symbolic`
All `Adw.SwitchRow`:
| widget | Title | Subtitle | Setting |
|---|---|---|---|
| `merged_process_stats` | "Merge Process Stats" | "Parent and child process stats are shown individually or merged upwards" | `apps-page-merged-process-stats` |
| `remember_sorting` | "Remember Sorting" | "Persist sorting by column across app restarts" | `apps-page-remember-sorting` |
| `remember_column_order` | "Remember Column Order" | "Persist the order of the columns, across app restarts" | `apps-page-remember-column-order` |
| `core_count_affects_percentages` | "Scale CPU Usage to Core Count" | "Show CPU usage scaled to the number of available cores" | `apps-page-core-count-affects-percentages` |
| `show_column_separators` | "Show Column Separators" | "Show a separator between each column" | `apps-page-show-column-separators` |
| `hide_is_zero` | "Grey Out Zeroes" | "Gray out values that equal zero" | `apps-page-hide-is-zero` |

Dependent behaviour: toggling `remember_sorting` off does **not** immediately clear stored values; the reset (`…-sorting-column-name` → `""`, `…-sorting-order` → Ascending, and no restore) happens the next time a `TableView` is set up (`settings.rs:61-65`). Likewise `remember_column_order` off only resets `…-column-order` to `""` at the next `update_column_order()` (table_view/mod.rs:995-997).

## 4.4 Units — title "Units", icon `ruler-angled-symbolic`
Each `Adw.ActionRow` contains an `Adw.ToggleGroup` (valign center, homogeneous) with two toggles (`label`, `tooltip`):
- "Memory Usage Unit" → `toggle_group_memory_unit`: "Bytes" (tooltip "Use Bytes as a Unit of Measurement for Memory") / "Bits" ("Use Bits as a Unit of Measurement for Memory"). Setting `performance-page-memory2-use-bytes`; group `active = !use_bytes` so index 0 = Bytes, index 1 = Bits (units_page.rs:143-144).
- "Memory Usage Base" → `toggle_group_memory_base`: "Base 10" ("Use Base 10 for Memory") / "Base 2" ("Use Base 2 for Memory"). Setting `performance-page-memory2-use-base2`; `active = use_base2`.
- "Drive Usage Unit" → "Bytes"/"Bits" ("…for Disk Drives") — `performance-page-drive-use-bytes`.
- "Drive Usage Base" → "Base 10"/"Base 2" ("…for Disk Drives") — `performance-page-drive-use-base2`.
- "Network Usage Unit" → "Bytes"/"Bits" ("…for Network Interfaces") — `performance-page-network-use-bytes` (default false → Bits).
- "Network Usage Base" → "Base 10"/"Base 2" ("…for Network Interfaces") — `performance-page-network-use-base2` (default false → Base 10).

Mapping detail: the "truthy" toggle for the *unit* groups is the **Bytes** toggle, for the *base* groups the **Base 2** toggle; `connect_toggle_pair_to_setting!` sets the boolean to `active_toggle == truthy_toggle` (preferences/mod.rs:48-72).

---

# 5. About system dialog

`src/about_system_dialog.rs` + `resources/ui/about_system_dialog.blp`. `Adw.Dialog` with `follows-content-size: true`, `Adw.Clamp maximum-size: 1000`, top margin 20/20/20, spacing 32.
- Header: title Label style `heading` "System Information"; `[end]` Button `action-name: "app.info-page-copy-all"`, tooltip "Copy All", icon `edit-copy`.
- Optional OS logo `Image` `pixel-size: 192`, `margin-top: 10`, visible only if an icon could be applied at 192 px.
- Group "Operating System Information": rows "Name", "Version", "Package Manager", "Package Manager Version"
- Group "Kernel Information": rows "Kernel Release", "Version"
- Group "Desktop Environment Information": rows "Name", "Version", "Windowing System", "Virtual Terminal"

Each row is an activatable `Adw.ActionRow` with label left and a right `dim-label` `ellipsize: middle`, `selectable: true` value; all rows have tooltip "copy"; row `visible` is bound to the value label's visibility. Height-request 36, margins 10, spacing 10. Values come from the magpie `About` proto (about_system_dialog.rs:160-195):
- `os_info.pretty_name` else `os_info.name`; `os_info.version_id` else `os_info.version`; `package_manager`; `package_manager_version`; kernel release = `"{os_type} {kernel_release}"` (if type missing → `"Unknown {release}"`); `kernel_version`; `de_info.desktop_environment`, `.version`, `.windowing_system`, `.virtual_terminal`. Missing values hide the row and the group whenever none of its rows is visible.
- Copy behaviour: activating a row copies `"{label}: {value}"` (about_system_dialog.rs:135-149). The header button copies the full block from `get_copy_all()` (about_system_dialog.rs:268-301):
```
System Information:
    Operating System Information
    Name:                    {}
    Version:                 {}
    Package Manager:         {}
    Package Manager Version: {}

    Kernel Information
    Kernel Release:          {}
    Version:                 {}

    Desktop Environment Information
    Name:                    {}
    Version:                 {}
    Windowing System:        {}
    Virtual Terminal:        {}
```
The `app.info-page-copy-all` action is registered on the application each time the dialog is built (about_system_dialog.rs:257-265).

---

# 6. First run dialog

`Adw.Dialog` with `follows-content-size: true`, `default-widget: run_script`; shown when setting `first-time-running` (default true) is true during `set_initial_readings`, which then immediately sets it to false (window.rs:1228-1238); also reachable from the main menu "Show _First Run Dialog" → `app.first-run-dialog`.

Content (`resources/ui/first_run_dialog.blp`), a vertical Box(margins 12, spacing 12, valign/halign start):
- Label style `title-1`, `wrap: true`, `wrap-mode: word_char`: **"Enabling Additional Values"**
- Label `main_text` (`use-markup: true`, wrap): **"CPU power draw, per-process network usage and fans may require additional setup"**
- `Adw.WrapBox` (child-spacing 12, line-spacing 12):
  - Button with tooltip `https://gitlab.com/mission-center-devs/mission-center/-/wikis/home`, `action-name: "app.open-wiki"`, Label "Open Wiki" + Image `external-link-symbolic`
  - `Adw.SplitButton run_script`: `action-name: "app.run_script"`, style `suggested-action`, `receives-default: true`, Label **"Run Additional Setup"** + Image `padlock2-symbolic`; menu `button_run_menu`:
    - "Revert Additional Setup" → `app.run_revert_script`
    - "View Script" → `app.view_script`
    - "Copy Script Location" → `app.copy_script_location`
    - "Copy Command" → `app.copy_script_command`
    - "Copy Revert Command" → `app.copy_revert_script_command`
  - Button `close`, label **"Close"** (closes the dialog).

Flow (`src/first_run_dialog.rs:104-281`), gated on `magpie.setup_script_name()` returning `Some((file, elevation_command))` (from magpie IPC `ScriptResponse::Name`):
- `app.view_script` → `magpie.setup_script_open()`; on `Err(output)` show a modal `AlertDialog`: message **"Failed to Open Script"**, detail `"{file}:\n{output}"`, buttons `["Copy File Location", "Close"]`, default button 1; response 0 copies `output` to the clipboard.
- `app.run_script` → `magpie.setup_script_run()`; on `Err(message)` show modal AlertDialog message **"Setup Script Failed"**, detail `message`, buttons `["Copy Output", "Retry", "Close"]`, default 1; response 0 copies output, response 1 re-activates `run_script`. On success show AlertDialog message **"Success!"**, detail **"Restart Mission Center to apply changes?"**, buttons `["Restart", "Close"]`; response 0 calls `app.restart()` (exec's the current binary); the first-run dialog is then closed.
- `app.run_revert_script` → `magpie.setup_script_run_revert()`; on error AlertDialog **"Revert Setup Script Failed"** + detail, buttons `["Copy Output", "Retry", "Close"]`, default 1; on success just closes the dialog.
- Clipboard actions: `copy_script_location` → `{file}`; `copy_script_command` → `"{elevation_command} {file}"`; `copy_revert_script_command` → `"{elevation_command} {file} --revert"`.
- On `map`, focus is set to the dialog's default widget (`run_script`).
- If `setup_script_name()` returns `None`, no script actions are registered and only the static dialog is presented.

---

# 7. Theme selector + style.css

## 7.1 Theme selector (`src/widgets/theme_selector.rs`, `resources/ui/widgets/theme_selector.blp`)
Box(horizontal, hexpand, spacing 12) containing three `CheckButton`s with CSS classes `theme-selector` plus `follow`/`light`/`dark`; `focus-on-click: false`; `light` and `dark` are grouped (`group: light`).
- `follow`: `action-target: "\"default\""`, tooltip and a11y label "Follow system style"
- `light`: `action-target: "\"force-light\""`, tooltip/a11y "Light style"
- `dark`: `action-target: "\"force-dark\""`, tooltip/a11y "Dark style"

Wired to the window action `win.interface-style` (aria name set via `set_action_name` on all three when the `action-name` property is set; window.rs:711-717 injects `ThemeSelector::new("win.interface-style")` into the popover with id `theme-selector`, matching the menu's `custom: 'theme-selector'`). The whole widget is hidden when `style_manager.system_supports_color_schemes()` is false, and gets/loses the CSS class `dark` with the style manager (theme_selector.rs:81-95, 126-157).

Interface-style mapping (window.rs:521-556 / 663-718): action values `"default"` → `adw::ColorScheme::Default` + gschema enum value `ADW_COLOR_SCHEME_DEFAULT` (0); `"force-light"` → `ForceLight` (1); `"force-dark"` → `ForceDark` (4). Setting key `window-interface-style`, gschema enum `io.missioncenter.InterfaceStyle` nicks `system`=0 / `light`=1 / `dark`=4; an unknown action value logs "Invalid value for window-interface-style setting: {}" and returns; `settings::changed` re-applies the scheme and updates the action state.

## 7.2 style.css (`resources/ui/style.css`, 95 lines; loaded via `include_str`/`include_bytes!` in application.rs:198-207 at `STYLE_PROVIDER_PRIORITY_APPLICATION`)
- `window columnview header { padding-left: 0; padding-right: 0 }`
- `window listview { margin-left: -2px }`; `window listview row { border-radius: 8px; margin: 0 0 0 2px }`
- `themeselector { margin: 9px }`; `themeselector checkbutton { padding: 0; min-height: 44px; min-width: 44px; padding: 1px; background-clip: content-box; border-radius: 9999px; box-shadow: inset 0 0 0 1px @borders }`
- Checked ring: `box-shadow: inset 0 0 0 2px var(--accent-bg-color)` for `.follow/.light/.dark:checked`
- Swatch colors: `follow` = `linear-gradient(to bottom right, #fff 49.99%, #202020 50.01%)`; `light` = `#fff`; `dark` = `#202020`
- Hidden radio: `-gtk-icon-source: none; border: none; background: none; box-shadow: none; min-width/height: 12px; transform: translate(27px, 14px); padding: 2px`; checked radio shows `-gtk-icontheme("object-select-symbolic")` with `background-color: var(--accent-bg-color); color: var(--accent-fg-color)`
- `.flipped { transform: rotate(180deg) }` (used by the info button), `.transparent { background-color: transparent }`
- `grid.layout-grid picture { padding-right: 0.5em }`
- Loading shimmer: `@keyframes mc-loading-shimmer { 0% {background-position: 200% 0} 100% {background-position: -200% 0} }` and `.placeholder-shimmer { background-color: alpha(@theme_fg_color, 0.09); border-radius: 6px; background-image: linear-gradient(90deg, alpha(@theme_fg_color,0.00) 0%, alpha(@theme_fg_color,0.06) 50%, alpha(@theme_fg_color,0.00) 100%); background-size: 200% 100%; animation: mc-loading-shimmer 5s linear infinite }` — used by `Placeholder`/`ShimmerBlock` (placeholder.rs:69-75 and 197-212 for the fake table header/rows).

**No tabular-number / `font-feature-settings` / `numeric` styling exists in style.css and no `.blp` uses a `numeric` style class** (only `title-1`, `title-3`, `title-4`, `heading`, `dim-label`, `faint-text`, `caption`, `card`, `flat`, `flipped`, `linked`, `suggested-action`, `rich-list`, `navigation-sidebar`, `drag-handle`, `theme-selector`, `dark`/`light`/`follow`). Numeric alignment is achieved structurally: `LabelCell` labels are `halign End` + `ellipsize Middle`, and every non-Name column header is right-aligned with `width_request 75` (§2.3). The only monospace styling is the service logs `TextView` (`monospace: true`).

## 7.3 Placeholders (`src/widgets/placeholder.rs`)
`Placeholder` profiles: `"sidebar"` (5 rows of 80×55 + 80/110/44×9 lines), `"performance-page-graph"`, `"performance-page-info"`, `"apps-page"` (200×28 + 110×9 counts; right-aligned action row of 120×34, 182×34, 87×34; then table = 32-px shimmer header + vexpand shimmer body), `"services-page"` (counts; action row 120×34, 145×34, 90×34, 90×34; filter row 392×34 + 138×34; table). `ShimmerBlock` measures: width = max, height fixed = the given thickness.

---

## Cross-cutting notes for the port
- Data types / human-readable formatting (`src/main.rs:224-439`): `DataType` variants MemoryBytes, DriveBytes, DriveBytesPerSecond, NetworkBytes, NetworkBytesPerSecond, Hertz, Watts, Volts, WattHours; `to_human_readable_nice` uses `to_human_readable_adv_str(value, use_bytes, use_binary, per_second, unit, min_exponent=1)` with units `["", "K", "M", "G", "T", "P", "E", "Z", "Y"]`, divisor 1024/1000, bit values ×8, suffix `"i"` when binary and `"B"`/`"b"`, plus `"/s"` (bytes) or `"ps"` (bits); decimals 2 / 1 / 0 by magnitude and 0 at/below the minimum exponent. Memory uses settings keys `performance-page-memory2-use-bytes` / `-use-base2`; drive `performance-page-drive-use-bytes` / `-use-base2`; network `performance-page-network-use-bytes` / `-use-base2` (cached in a global boolean dict).
- `ContentType` → display string: "Section Header", "Service", "App", "Process" (row_model.rs:276-285).
- App desktop entry: Name `Mission Center`, Exec `missioncenter`, Icon `io.missioncenter.MissionCenter`, Categories `GTK;System;Monitor;`, StartupNotify true, Keywords list includes `Task manager;Resource monitor;System monitor;…;systemd;systemctl`. AppStream summary: "Monitor system resource usage" (app name "Mission Center").
- Custom symbolic icons shipped in the gresource (`resources/missioncenter.gresource.xml`): `list-collapse-symbolic`, `list-enable-all-symbolic`, `list-disable-all-symbolic`, `arrow-circular-top-right-symbolic`, `edit-symbolic`, `sidebar-toggle-left-symbolic`, `details-symbolic`, `cross-small-circle-filled-symbolic`, `external-link-symbolic`, `info-outline-symbolic`, `speedometer-symbolic`, `overlapping-windows-symbolic`, `general-properties-symbolic`, `ruler-angled-symbolic`, `padlock2-symbolic`, `service-running/-failed/-stopped/-disabled`, plus standard Adwaita ones used by name (`system-search-symbolic`, `open-menu-symbolic`, `system-run-symbolic`, `media-playback-*`, `media-playlist-repeat-symbolic`, `edit-clear-all-symbolic`, `edit-copy*`, `object-select-symbolic`).
