# Task Manager

Task Manager: system monitor (processes, CPU, memory, disk, network, GPU, services) in Rust with an egui/eframe UI (pure-Rust GUI stack) in the style of the Windows 10 Task Manager, built clean-room from measurements (`docs/win10-spec/`). Based on Mission Center: its statistics backend (magpie) and ported logic; keep the attribution (README "Based on Mission Center", About dialog, `Mission Center Developers` copyright headers) intact. Goals: `docs/goals/2026-10-10-egui-gui-port.md` (GTK → egui port), `docs/goals/2026-10-10-win10-skin.md` (Win10 look).

## Map
- `crates/mc-magpie/` — GUI-free client of the magpie daemon (NNG IPC, gatherer thread, `LatestSlot` hand-off, bounded event channel, fixtures, synthetic data); gatherer-side extras without privileges: `users` (process owners), `sockets` (per-process TCP traffic when magpie/nethogs has none).
- `crates/mc-core/` — GUI-free logic: settings (`config.rs`, keys = former GSettings keys), formatting, i18n (pure-Rust `.mo` reader), sidebar ordering, process/service tree model; `gpu.rs` owns GPU identity, availability and aggregation for every view.
- `crates/mc-graph-core/` — local, GUI-free graph data/scaling core carried from upstream graph-widget; provenance in `docs/PROVENANCE.md`.
- `crates/mc-graph/` — egui time-series graph on top of `mc-graph-core`; `GraphStyle::Win10`.
- `crates/mc-ui/` — egui UI, depends on egui only:
  - `app.rs` shell (state, data intake, commands);
  - `win10/` the Win10 look: `palette`/`metrics` (all numbers from the spec), `fonts` (Selawik/Segoe UI), `paint` (pixel-exact helpers), `chrome` (CSD title bar, menu bar, tabs, footer), `menu`, `listview`/`flatlist`, tabs `processes`, `performance`, `details`, `services`, and `style`/`dialog` for standard egui widgets;
  - `perf/` device models (`DevicePage::win10_view`), `table.rs` (process/service tree view state), `dialogs.rs`, `smart.rs`. There is no settings window: options live in the View/Options menus and context menus; everything else is fixed.
- `crates/mc-ui/assets/fonts/` — bundled Selawik (SIL OFL 1.1, `OFL.txt`).
- `crates/mc-ui/assets/icons/` — SVGs carried from Mission Center, embedded by the egui UI.
- `apps/taskmanager/` — eframe binary `taskmanager` (app id `io.github.taskmanager.TaskManager`, settings `~/.config/taskmanager/config.toml`, copied once from `~/.config/missioncenter/`) (`--fixture`, `--synthetic`, `--dump-readings`, `--renderer wgpu|glow`, `--perf-overlay`, `--bench-seconds`).
- `xtask/` — dev/packaging tasks (`cargo xtask help`).
- `docs/port-spec/` — frozen historical GTK specs (msgids, formats, behaviours); source-history links are in their headers.
- `docs/win10-spec/` — clean-room visual spec (`spec.md`) and pixel probes (`probes.json`); `tools/measure_refs.py` reproduces the measurements from local screenshots in `references/` (git-ignored, never committed).
- `subprojects/magpie` — tracked backend sources, separate Cargo workspace; GPU collectors are in `platform-linux/src/gpus/`. Only its nested `platform-linux/crates/app-rummage` is a required submodule.
- `support/bundle-gpu-libs.py` — private GPU ABI staging/verification; daemon-only relative RUNPATH, no redistributed NVIDIA driver or global loader override.
- `po/` — gettext translations (Weblate); `data/` — icons, desktop/metainfo.

## Rules (Win10 look, clean room)
- Implement only from `docs/win10-spec/spec.md`: no Microsoft pixels, icons, fonts or code in the repository; draw icons ourselves; never commit `references/`.
- Geometry and colours live in `win10/metrics.rs` and `win10/palette.rs`; mark own design (not measured) as such in the spec.
- Draw 1 px lines and fills with `win10::paint` (meshes; egui's rect tessellation loses single pixels). Place text by cap top (`paint::text`).
- Names: `mc_ui::APP_NAME` ("Task Manager"), binary/package `taskmanager`, app id `io.github.taskmanager.TaskManager`, gettext domain `taskmanager`. The Mission Center backend keeps its names (`missioncenter-magpie`, `share/missioncenter/hw.db`, flatpak `missioncenter-spawner`). Internal crate names (`mc-*`) are unchanged.
- The application icon (`data/icons/hicolor/*/apps/io.github.taskmanager.TaskManager.png`) was provided by the user (fan redraw) and is outside the clean-room rule.

## Rules (egui port)
- Layers: `mc-magpie` → `mc-core` → `mc-graph`/`mc-ui` → app; `mc-graph-core` is GUI-free. `mc-core`/`mc-magpie`/`mc-graph-core` must never depend on egui/eframe/GTK.
- No I/O, IPC or blocking calls in `App::ui`/`App::logic`; talk to magpie only via `Request`s and `Event`s; persist settings via `ConfigWriter`.
- No unbounded channels. Snapshots go through `LatestSlot` (overwrite), events through bounded `sync_channel`.
- GPU percentages use the busiest engine; groups deduplicate PIDs/DRM clients before summing each engine. Unknown is not zero; device memory never comes from process sums. Collect counters on the host-side backend, not the Flatpak GUI.
- Repaint only on events (new readings, input, icon decoded). Never call `request_repaint()` unconditionally. Hidden windows run `logic` only.
- Rebuild table rows / format strings / graph geometry only when data, sort, filter, size or expansion change.
- Translatable strings use `mc_core::i18n::{i18n, i18n_f, ni18n_f, ...}` with msgids identical to the GTK app (see `docs/port-spec/`); mnemonic underscores are part of the msgid, strip them for display with `strip_mnemonic`.
- No `Arc<Mutex<AppState>>`; views mutate state through `Cx` and typed `Command`s.
- Recorded fixtures (`--dump-readings`) contain private process data: keep them under `testkit/fixtures/live/` (git-ignored). Tests use `mc_magpie::synthetic_series`.
- Report only measured numbers (method and historical evidence in `perf/README.md`); screenshots only from real renders (`target/ui-snapshots/`).
- Run `cargo fmt` after touching Rust code.

## Verify
- `cargo fmt --all --check` — nonmutating formatting check.
- `cargo test --workspace` — includes local graph core and xtask, Win10 pixel probes (`tests/win10_probes.rs`) and offscreen snapshots (needs a wgpu adapter); renders go to `target/ui-snapshots/`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- Backend edits: `cargo fmt --manifest-path subprojects/magpie/Cargo.toml --all --check`; `cargo test --manifest-path subprojects/magpie/Cargo.toml -p magpie-platform-linux -p magpie-types --locked --release` (not included in root `--workspace`).
- `cargo run -p taskmanager-app -- --synthetic 300` — run without the daemon (glow renderer by default, `--renderer wgpu` optional); `cargo xtask run-dev` — run with the real magpie.
- `cargo xtask bench-idle` — egui wgpu/glow idle and synthetic-table benchmarks (see `perf/README.md`).

## Docs
- `README.md` — dependencies, packaging, env vars.
- `docs/PROVENANCE.md` — Mission Center authors, upstream graph revision and original Silver notice.
- `docs/goals/2026-10-10-egui-gui-port.md` — port contract and current state; `docs/goals/2026-10-10-egui-gui-port-plan.md` — approved plan.
