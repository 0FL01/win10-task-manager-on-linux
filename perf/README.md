# Performance measurements

`cargo xtask bench-idle` measures the current egui application with the wgpu and glow renderers.
It uses a supplied UI binary's sibling `missioncenter-magpie` when present; otherwise it prepares
the normal Cargo-built daemon through `xtask::dist`. No GTK baseline or Meson build tree is
required by the current benchmark.

## Running

Build the release UI, then run from a Linux session with `kwin_wayland` and `XDG_RUNTIME_DIR`:

```sh
cargo +1.96.1 build --locked --release -p taskmanager-app
cargo xtask bench-idle --warmup 8 --seconds 40 --repeats 3
```

Native backend build dependencies are listed in the root README. The benchmark prepares the
daemon and hardware database through the same Cargo/Python distribution helpers used for
development and installation when bundled resources are absent. `--egui FILE` uses that UI for
both renderers; `--egui-glow FILE` overrides only glow. `--out FILE` selects the report path.
The ordinary default is `perf/bench-latest.md`; a binary override without `--out` writes a uniquely
named `target/diagnostic/bench-<timestamp>.md` instead, preserving the ordinary report.

### Local diagnostic bundle

```sh
cargo +1.96.1 xtask diagnostic
```

This command takes no options. It builds both independent workspaces with Rust 1.96.1,
`--locked --release --no-default-features --features diagnostic-profile` (plus both UI renderers), `CARGO_PROFILE_RELEASE_DEBUG=2` and
`CARGO_PROFILE_RELEASE_STRIP=none`. The existing release optimization level 3 and single codegen
unit remain in effect. UI and daemon build caches are isolated at `target/diagnostic/ui/` and
`target/diagnostic/magpie/`. The completed local bundle is published at:

- `target/diagnostic/bundle/bin/taskmanager` — directly runnable ELF UI;
- `target/diagnostic/bundle/bin/missioncenter-magpie` — matching daemon;
- `target/diagnostic/bundle/lib/taskmanager/gpu/` — private GPU ABI libraries, `licenses/` and
  verified `manifest.json` from `support/bundle-gpu-libs.py`;
- `target/diagnostic/bundle/share/missioncenter/hw.db` and `share/locale/` — runtime data.

Ordinary release/install/package builds default to `production-logging`: the `log` crate
compiles out DEBUG/TRACE (and INFO), keeping WARN/ERROR, regardless of `RUST_LOG`.
Aggregate profiling code is not compiled unless `diagnostic-profile` is selected. The two
binary build modes are mutually exclusive; diagnostic commands disable production defaults
explicitly. Changing `RUST_LOG` cannot turn a production package into a diagnostic build.

The command prints an absolute-path `env` invocation selecting its hardware database and locales.
For example, from the repository root:

```sh
env MC_MAGPIE_HW_DB="$PWD/target/diagnostic/bundle/share/missioncenter/hw.db" \
    MC_LOCALEDIR="$PWD/target/diagnostic/bundle/share/locale" \
    RUST_LOG='warn,TaskManager::Profile=trace' \
    target/diagnostic/bundle/bin/taskmanager
```

The UI discovers its sibling daemon. GPU RUNPATH is relative and daemon-only; no global
`LD_LIBRARY_PATH`, system installation, root or package-manager step is needed. Existing builder
overrides (`PATCHELF`, `READELF`, `MC_GPU_*`) are honored. If `PATCHELF` is unset and patchelf is
absent from PATH, the diagnostic command checks `target/gpu-bundle-tools/bin/patchelf`, then
`target/gpu-bundle-tools/gpu-patchelf`; the bundler requires version 0.18 or newer. Ordinary
`magpie`, `install`, `rpm` and `run-dev` retain their normal targets and behavior.

Use the same bundle for quiet and opt-in profile runs, keeping reports separate:

```sh
RUST_LOG=warn cargo +1.96.1 xtask bench-idle \
    --egui target/diagnostic/bundle/bin/taskmanager \
    --warmup 8 --seconds 40 --repeats 3 --out target/diagnostic/quiet.md
RUST_LOG='warn,TaskManager::Profile=trace' cargo +1.96.1 xtask bench-idle \
    --egui target/diagnostic/bundle/bin/taskmanager \
    --warmup 8 --seconds 40 --repeats 3 --out target/diagnostic/profile.md
```

The benchmark uses the sibling daemon directly without rebuilding the default backend, selects
bundled hardware data/locales when available, and honors explicit `MC_MAGPIE_HW_DB` /
`MC_LOCALEDIR` overrides. Its report records supplied binary paths, daemon paths, logging filter,
workloads, renderers and valid-run counts. The profile log filter enables the fixed aggregate
profile target without enabling noisy application-wide trace logging; stderr stays visible and
can be redirected to a separate file under `target/diagnostic/`.

## Method

Each run uses a fresh virtual KWin with `--no-lockscreen`: this isolates benchmark frame
callbacks from the host session's screen locker without unlocking or reconfiguring that session.

- Every run starts a fresh nested compositor (`kwin_wayland --virtual`, 1280x800) so the user's
  desktop is not touched and an idle compositor cannot stop frame callbacks mid-run.
- Both renderers use the same magpie daemon with a 1 s refresh interval and the CPU page selected;
  settings are injected through temporary `taskmanager/config.toml` directories. A separate
  Processes-page scenario uses 1000 synthetic processes and no daemon.
- After warm-up, CPU ticks from `/proc/<pid>/stat` are differenced over the measurement window
  and divided by clock ticks and elapsed wall time; CPU percentages are relative to one core.
  The daemon must be a descendant with the expected executable, unique among matching
  descendants, and have the same PID/start time at each checkpoint. Missing, exited or restarted
  daemons have unavailable CPU rather than zero; synthetic runs have no daemon.
- External RSS/PSS sampling covers both the UI and actual verified live daemon at measurement
  start, every 10 s, and measurement end (also for windows not divisible by 10 s). Values come
  from `/proc/<pid>/smaps_rollup`; RSS alone falls back to `VmRSS` in `/proc/<pid>/status`.
  Missing/unreadable values are `n/a`. Reports include individual observations, final memory
  and the **maximum observed** RSS/PSS separately; sampled maxima are not absolute peaks.
  Each summary component uses only available values across kept runs. RSS counts resident
  pages; PSS proportionally accounts for shared pages. Neither attributes memory to functions.
- GPU memory comes from UI DRM `fdinfo` counters de-duplicated by client ID; absent counters are
  `n/a`, not evidence of zero GPU memory usage or a measurement of GPU execution time.
- The egui binary reports rendered frames, applied readings and the UI pass p95 itself
  (`--bench-seconds`, `--stats-file`).
- Failed runs (including early exits and no rendered frames) are discarded. `valid` is the number
  of kept runs out of the requested repeats, and table values are medians of kept samples.
  `0/N` and `n/a` indicate no valid measurements, not zero resource usage. Frames/readings and
  UI-pass p95 are application-wide statistics, including warm-up; CPU is sampled only during
  the measurement window. Historical reports retain their original `NaN` notation.

Not covered automatically: input-driven scrolling (needs input injection) and the minimized state
(eframe skips the UI pass while hidden by design; `logic` still drains data).

The UI is single-instance on the session bus: while Task Manager already runs in the session, a
launched UI only activates that window and exits. Run measurements under `dbus-run-session` in
that case; the system bus (system services) stays the real one.

## Memory footprint (2026-10-13)

Release builds of `10562c3` (before) and the memory work (after), one host (16 threads, ~620
processes, ~530 system units, AMD radeonsi/glow), nested `kwin_wayland --virtual` 1280x800,
private session bus, live daemon, 1 s updates. `RssAnon`/`RssFile` from `/proc/<pid>/status`
after ≥80 s; CPU from `/proc/<pid>/stat` over the window after a 10 s warm-up, relative to one
core. Peak heap from `heaptrack --record-only` (120 s, Processes page).

| Processes page | before | after |
|---|---|---|
| daemon `RssAnon` (5 runs) | 83.5–85.1 MiB | 8.2–8.7 MiB |
| daemon peak heap | 71.1 MB | 6.4 MB |
| daemon threads | 37 | 13 |
| daemon CPU (runs 3–5) | 30.5 / 31.8 / 32.0 % | 30.9 / 30.9 / 31.7 % |
| UI `RssAnon` (5 runs) | 34.9–35.1 MiB | 30.0–30.6 MiB |
| UI peak heap | 23.4 MB | 22.2 MB |
| UI threads | 52–53 | 28–29 |
| UI CPU (runs 3–5) | 1.93 / 2.00 / 2.01 % | 2.00 / 2.02 / 2.01 % |

The Performance page gives the same picture (daemon 84.2 → 8.3 MiB, UI 35.2 → 30.6 MiB). UI
`RssFile` (~67 MiB) is unchanged: mostly Mesa's `libLLVM`/`libgallium`, clean pages shared with
other GL clients. Causes found with heaptrack, in order of size: zbus property caches of two
proxies per systemd unit (57.7 MB), glibc arena retention and NNG pools sized by core count,
per-process file mapping lists for the shared-memory estimate, and system font bytes held twice.

## Memory footprint, second pass (2026-10-17)

Release builds (with debug info, not stripped) of `743ef7c` (before) and this work (after),
same host and setup as above: nested `kwin_wayland --virtual` 1280x800, private session bus,
live daemon, 1 s updates, glow unless noted. `RssAnon`/`RssFile` from `/proc/<pid>/status`
and PSS from `smaps_rollup` after 80 s, 3 runs per row (wgpu: 2), shown as ranges. Peak heap from
`heaptrack --record-only` (120 s, Processes page).

| UI `RssAnon` | before | after |
|---|---|---|
| Processes | 30.3–30.4 MiB | 21.1–21.4 MiB |
| Performance (CPU) | 30.5–30.7 MiB | 21.2–21.3 MiB |
| Details | 29.9–30.1 MiB | 20.7–20.8 MiB |
| Services | 30.3–30.4 MiB | 20.9–21.1 MiB |
| Processes, wgpu | 23.6–23.8 MiB | 20.9–21.0 MiB |
| UI PSS, Processes | 56.2–56.3 MiB | 47.1–47.7 MiB |
| UI peak heap | 22.0 MB | 12.2 MB |
| daemon `RssAnon` (all pages) | 7.6–8.3 MiB | 7.0–7.3 MiB |
| daemon peak heap | 5.6 MB | 4.9 MB |

PSS depends on what else maps the same libraries, so compare it only within one session.
Threads are unchanged (UI 28, daemon 14), and so is CPU: in two alternating 60 s windows,
UI 2.03/2.15 % before and 2.02/1.80 % after, daemon 30.3/31.2 % before and 32.0/27.7 % after
(one core). UI `RssFile` is still ~67 MiB of shared, clean library pages.

The causes were found with heaptrack (live allocations at exit and at the peak, by call stack)
and `/proc/<pid>/smaps`, largest first:

- Mesa keeps its GLSL built-in function library (5.8 MB on radeonsi) for the context's
  lifetime. egui_glow compiles its shaders only while it creates the painter, so the app
  calls `glReleaseShaderCompiler` once at startup; another compilation would load the library again.
- egui sizes its font atlas to the GPU's largest texture side: 16384×32 RGBA, a 2 MiB image
  kept for the whole run. The glyphs used fill ~20 rows of 2048 px, so the app limits
  `max_texture_side` to 2048 (atlas 256 KiB, still able to grow to 2048×2048).
- System fonts (Segoe UI, sans and monospace fallbacks) were read into the heap (1.2 MiB);
  they are now read-only file mappings that share clean pages with other programs.
- Every live graph series allocated 600 values and 600 x positions, though it shows 60;
  series now store only the values they show and use the index as their x position.
- The daemon's 5 s maps walk held one 16-byte entry per file mapping of every process (one
  process's list alone reached 1.1 MB). Adjacent segments of the same file are now one entry
  of 8 bytes; the walk's share of the daemon's heap peak fell from 1.68 MB to 0.46 MB.
- The Details tab stored four GPU tooltip strings per process; rows now share the few
  distinct texts.

Not changed: Mesa's buffer slabs and context state (~5 MB heap on radeonsi), and the
`--synthetic` scenario, whose 60 pre-generated snapshots stay in memory (UI ~82 MiB at 1000
processes) because they are the data source itself. Neither turning off Mesa glthread nor a
64 KiB `MALLOC_MMAP_THRESHOLD_` saved more than ~0.8 MiB.

Diagnostic builds log resident memory and allocator state around every heap trim:
`RUST_LOG=warn,TaskManager::Memory=debug,magpie::footprint=debug`. Production builds compile
these messages out.

## Tab switching (2026-10-17)

A tab used to switch on mouse release, and only in the frame after the click: the click's pass
still drew the old page and queued `Command::SetPage`. Tabs (and Ctrl/Alt+1..4) now switch on
the primary press, in the same pass, before the footer and content are drawn.

`cargo test -p mc-ui --lib --release -- --ignored --nocapture rapid_tab_switching` alternates
Processes and Performance 40 times on 600 synthetic processes with the system fonts, timing
the UI pass plus tessellation (no GPU upload). Three runs: a steady frame takes 0.40–0.42 ms
(p50), a switch frame 1.13–1.20 ms (p50) and 1.40–2.97 ms (p95). The only slower frame is the
first switch to Performance (6.5–7.6 ms), which rasterises that page's glyph sizes once. Every
switch fits well inside one 60 Hz frame, so no text or geometry caches were added. Built
with the diagnostic profile, `RUST_LOG=warn,TaskManager::Latency=debug` logs each switch pass.

## Reports and historical evidence

- `bench-latest.md` — generated current-run output, git-ignored; it is not reviewed release evidence.
- `history/bench-latest.md` — frozen original port comparison (8 s warm-up, 40 s measurement,
  3 repeats); all rows have 3/3 valid runs.
- `history/bench-win10.md` — frozen shorter Win10 comparison (5 s warm-up, 20 s measurement,
  2 repeats); all rows have 2/2 valid runs.
- `history/bench-win10-full.md` — frozen longer Win10 comparison: only sliding-off rows contain
  measurements (GTK and wgpu 3/3, glow 2/3). Sliding-on and synthetic rows have 0/3 valid runs
  and cannot support performance claims.

These three reports were moved byte-for-byte from `perf/` to `perf/history/`. Their GTK/Meson
references describe the historical measurement setup before cleanup. Source revisions are in
`docs/PROVENANCE.md`. They do not establish current performance, complete functional parity or
all criteria of the original decision gate (startup, minimized state and input-driven scrolling
are not covered). `RESULTS.md` and `budgets.toml` were proposed by the original plan but do not
exist; there is no recorded regression-budget file or automated budget gate.
