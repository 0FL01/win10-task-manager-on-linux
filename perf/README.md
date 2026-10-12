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
`--locked --release`, `CARGO_PROFILE_RELEASE_DEBUG=2` and
`CARGO_PROFILE_RELEASE_STRIP=none`. The existing release optimization level 3 and single codegen
unit remain in effect. UI and daemon build caches are isolated at `target/diagnostic/ui/` and
`target/diagnostic/magpie/`. The completed local bundle is published at:

- `target/diagnostic/bundle/bin/taskmanager` — directly runnable ELF UI;
- `target/diagnostic/bundle/bin/missioncenter-magpie` — matching daemon;
- `target/diagnostic/bundle/lib/taskmanager/gpu/` — private GPU ABI libraries, `licenses/` and
  verified `manifest.json` from `support/bundle-gpu-libs.py`;
- `target/diagnostic/bundle/share/missioncenter/hw.db` and `share/locale/` — runtime data.

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
