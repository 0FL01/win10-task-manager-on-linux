# Goal: Runnable local diagnostics

Status: complete
Source: user-approved profiling plan; latest instruction replaces Fedora RPM with a local binary runnable without dnf/root.
Last updated: 2026-10-12

## Objective
Build and run an optimized, symbol-bearing Task Manager bundle without installation; identify resource costs with bounded, low-noise profiling before changing collection behavior.

## Execution Directive
Complete the frozen outcomes using the smallest relevant changes. Measurements may select one targeted optimization, but do not authorize a collector rewrite or unrelated cleanup. Stop after verification, or record the concrete environmental blocker.

## Frozen Contract
- R1: A local diagnostic command builds both UI and matching daemon with release optimization and debug symbols, stages private GPU libraries/notices and hw.db, and prints the directly runnable binary path.
  - Acceptance: bundle runs without root, dnf, or writes to system installation paths; ordinary release/packaging commands retain their behavior.
  - Status: verified.
  - Evidence: `cargo +1.96.1 xtask diagnostic`; both staged ELFs retain `.debug_info`/`.debug_line`, daemon has `$ORIGIN/../lib/taskmanager/gpu` RUNPATH, bundle verification/daemon --test passed; 24/24 quiet/profile benchmark runs rendered successfully without installation.
- R2: Opt-in profiling distinguishes wall time from thread CPU across UI application/drawing, gatherer/IPC, daemon requests and actual collectors.
  - Acceptance: fixed-size aggregates, one bounded summary per process per 10 seconds plus final summary; no per-PID logs, UI I/O or extra repaint; disabled mode does not start a reporter or read CPU clocks.
  - Status: verified.
  - Evidence: bounded/disabled/concurrent aggregate tests passed; profile logs contain fixed CPU/wall phases and aggregated process/optional metric errors. Live 25-second `perf record` run emitted both daemon and UI final summaries after bounded native TERM shutdown (ordinary shutdown unchanged).
- R3: Run the local application and record quiet/profile resource observations with the existing benchmark mechanism, including separately labeled UI and daemon memory.
  - Acceptance: report actual build/workload/renderer and valid runs; unavailable values are not zero; do not claim per-function memory or GPU execution time from these measurements.
  - Status: verified.
  - Evidence: `target/diagnostic/{quiet,profile}.md` — 8 s warm-up, 40 s measurement, 3 repeats, four scenarios/renderer combinations, all 3/3 valid. Live daemon CPU quiet 35.35%/36.16% of one core; UI 2.07%/2.00%, daemon final PSS 107.6/108.4 MiB (wgpu/glow). Profiling is observational, not a resource improvement claim.
- R4: Select an optimization only from measured evidence and verify it against the same workload; otherwise state why no optimization is justified yet.
  - Status: verified.
  - Evidence: reusable fdinfo text buffer passed all 10 existing fdinfo tests, then the same 8/40/3 quiet benchmark (`target/diagnostic/candidate-quiet.md`, all 3/3 valid). Daemon CPU changed from 35.35/36.16% to 35.33/33.86% (wgpu/glow), not a consistent improvement across renderers; memory did not improve. Reverted the candidate rather than claiming an unproven optimization. The measured fdinfo hotspot remains a justified target, but skipping/caching observations without further evidence could change GPU availability/deduplication semantics.

### Constraints and Non-goals
- Preserve magpie, Readings/fixtures, metric semantics, bounded channels, GPU availability/deduplication rules and Mission Center attribution.
- Preserve daemon-only private GPU RUNPATH and host driver loading; no global loader/security override.
- No RPM/system installation, CI/release change, new dependency/service or speculative backend migration. No commits or pushes requested.
- Logs contain fixed phase names and aggregate counts, not private process/service/network identities.

## Change Envelope
- xtask local build and existing external benchmark; app/client/UI and backend profiling boundaries; nearest regression tests; performance usage documentation.
- Build artifacts and raw diagnostics remain under git-ignored target/.
- Toolchain: Rust 1.96.1, independent root/backend Cargo workspaces.

## Current Checkpoint
- Complete. No retained collector optimization and no next implementation checkpoint.

## Current State
- Clean initial worktree at fce5c3c; Rust 1.96.1, kwin_wayland, perf and heaptrack available; Wayland session available.
- Bundle built at `target/diagnostic/bundle/bin/taskmanager`, matching daemon and GPU closure verified by existing bundler/daemon --test; both builds report optimized + debuginfo.
- Root mc-magpie tests: 20 passed/2 ignored; mc-ui unit/snapshot/probe tests: 55 passed/2 ignored; app tests: 2 passed/1 ignored. Root Clippy with Rust 1.96.1 passed. Backend Linux/types tests: 57 passed, profile tests: 9 passed; xtask: 14 passed (delegated workstreams).
- Local build used existing offline Cargo/native-development caches. NVTOP archive SHA256 matched the tracked package metadata before reuse; host libdrm 2.4.134 needed matching existing source notices via MC_GPU_LICENSES. No downloads/installation or security overrides.
- Quiet/profile measurements completed, all 3/3 valid. GPU fdinfo dominates daemon CPU (~150–170 ms per update); process collection and apps follow. Service startup wall time is mainly waiting. Nested timings are inclusive and not summed.
- A separate native glow run recorded 323 user-space CPU samples with no lost samples; libc reads/statx and allocator work appear, but this is not kernel CPU attribution or a function-level allocation profile. Stats: 65 rendered frames/26 readings; both final profile reports present.
- Observed repeated unavailable virtual-interface speed/type warnings are counted in profiling mode, with one sanitized warning per fixed category; ordinary logging/metric fallback behavior retained.
- Build dependency logs contain existing NVTOP C warnings, not new Rust diagnostics.

## Completion
- All R1–R4 verified. Ordinary metric collection, sample baselines, Readings/fixtures, GPU semantics and packaging commands retained; no new dependency, system installation, commit or push.
- Final source passed both formatting checks and `cargo +1.96.1 clippy --locked --offline --workspace --all-targets -- -D warnings`.
- Rebuilt the final bundle after reverting the candidate; existing GPU verification and daemon --test succeeded. Final native glow run: 25 seconds, 255 rendered frames, 25 applied readings, UI-pass p95 0.592 ms; `target/diagnostic/final.log` contains both daemon/UI final summaries, zero overwritten readings/overruns/full channels, and only nine diagnostic records (three sanitized warnings plus periodic/final summaries). This native smoke is not the controlled benchmark baseline.
- Reports: `target/diagnostic/{quiet,profile,candidate-quiet}.md`; CPU sample/report and final smoke artifacts stay in ignored `target/diagnostic/`. Allocation attribution, GPU execution time, minimized and input-driven scrolling scenarios were not measured; PSS/RSS are whole-process observations, not per-function memory.
- Final diff reviewed against this contract. Final status: complete.
