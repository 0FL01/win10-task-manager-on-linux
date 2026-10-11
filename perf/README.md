# Performance measurements

`cargo xtask bench-idle` measures the current egui application with the wgpu and glow renderers.
It uses the Cargo-built magpie daemon and generated hardware database from the `xtask::dist`
helpers. No GTK baseline or Meson build tree is required by the current benchmark.

## Running

Build the release UI, then run from a Linux session with `kwin_wayland` and `XDG_RUNTIME_DIR`:

```sh
cargo build --release -p taskmanager-app
cargo xtask bench-idle --warmup 8 --seconds 40 --repeats 3
```

Native backend build dependencies are listed in the root README. The benchmark prepares the
daemon and hardware database through the same Cargo/Python distribution helpers used for
development and installation. `--out FILE` selects another report path.

## Method

- Every run starts a fresh nested compositor (`kwin_wayland --virtual`, 1280x800) so the user's
  desktop is not touched and an idle compositor cannot stop frame callbacks mid-run.
- Both renderers use the same magpie daemon with a 1 s refresh interval and the CPU page selected;
  settings are injected through temporary `taskmanager/config.toml` directories. A separate
  Processes-page scenario uses 1000 synthetic processes and no daemon.
- After a warm-up the CPU time of the UI process and of the daemon (`/proc/<pid>/stat`) is sampled
  over the measurement window; PSS comes from `/proc/<pid>/smaps_rollup`, GPU memory from the DRM
  `fdinfo` entries of the UI process.
- The egui binary reports rendered frames, applied readings and the UI pass p95 itself
  (`--bench-seconds`, `--stats-file`).
- Failed runs (including early exits and no rendered frames) are discarded. `valid` is the number
  of kept runs out of the requested repeats, and table values are medians of kept samples.
  `0/N` and `NaN` indicate no valid measurements, not zero resource usage. Frames/readings and
  UI-pass p95 are application-wide statistics, including warm-up; CPU is sampled only during
  the measurement window. DRM memory depends on driver reporting; missing entries are not
  proof of zero GPU memory usage.

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
