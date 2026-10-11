# Goal: Windows-style GPU monitoring

Status: active (R4 sandbox artifacts unverified)
Source: user-approved GPU plan and request “Реализовать и коммит пуш билд”.
Last updated: 2026-10-11

## Objective
Deliver Processes, Details and per-adapter Performance GPU monitoring with packaged GPU
dependencies, build the RPM, commit and push to the existing SSH origin.

## Execution Directive
Complete the frozen Required Outcomes using the listed Change Envelope and Primary Evidence.
Do not add requirements from reviews, tests, speculative risks or optional source text.
Finish when every required outcome is resolved and affected constraints remain satisfied.

## Frozen Contract
- R1: Reproducible host-side telemetry for GPU engines, process identity, dedicated/shared memory,
  capabilities and freshness. Include the current magpie sources in this repository, preserving
  upstream attribution and the pinned app-rummage dependency.
  - Source: approved plan, sections 1–4.
  - Acceptance: structured, backward-compatible IPC; no guessed engine or missing-data zero.
  - Primary evidence: backend parser/collector tests and live IPC smoke.
  - Status: done
- R2: Shared GPU numbering and aggregation; Processes GPU/engine defaults, optional memory
  columns, Details GPU columns and usable overflow/persistence.
  - Source: approved plan, sections 5–7.
  - Acceptance: correct max-engine values, deduplicated groups, visible missing-data reasons.
  - Primary evidence: core/table/config tests and actual UI renders.
  - Status: done
- R3: Per-GPU selectable engine graphs, dedicated/shared memory, metadata, copy and graph summary.
  - Source: approved plan, section 8.
  - Acceptance: capabilities update dynamically; missing/stale history is a gap, not fake idle.
  - Primary evidence: GPU page tests and actual UI renders.
  - Status: done
- R4: GPU monitoring needs no extra host utilities or monitoring libraries; open native libraries
  are private packaged dependencies. Installed graphics driver/NVML remain driver components.
  - Source: user-confirmed dependency contract and approved plan, section 9.
  - Acceptance: correct native/portable, RPM, AppImage, Flatpak ABI and Snap staging paths.
  - Primary evidence: bundle/staging tests, ELF/artifact inspection and packaged launch.
  - Status: partial — native/RPM verified; full Flatpak, Snap and AppImage builds not run
- R5: Preserve existing contracts, build the updated RPM, commit and push.
  - Source: latest user instruction and approved verification plan.
  - Acceptance: relevant workspace gates, RPM artifact, clean intended commit and remote match.
  - Primary evidence: Cargo gates, RPM queries, Git commit/push/ref verification.
  - Status: done

### Constraints
- No GPU I/O in the UI; keep existing gatherer, bounded hand-offs and event-driven repaint.
- Preserve old protobuf fields, JSON fixtures, settings, non-GPU UI and process commands.
- Keep Mission Center/NVTOP/app-rummage notices and clean-room rules; no reference screenshots,
  private fixtures, native binaries, build caches or credentials in Git.
- Unsupported driver counters are explicitly unavailable; never substitute system RAM, BAR1,
  process sums or process kind for measurements of another metric.

### Non-goals
- Win11 shell redesign, driver installation, overclocking, root/debugfs mode, unrelated cleanup.
- Claiming untested physical hardware or complete Windows-only WDDM/DirectX telemetry on Linux.

## Change Envelope
- Backend GPU/types/process identity; mc-core GPU/table/config; mc-ui GPU tables/performance/shell;
  mc-magpie synthetic/fixture/freshness intake; native bundling and existing packaging; affected docs.
- No new runtime service, worker/channel architecture or GPU utility dependency.

## Current Checkpoint
- Closes: R1–R3, R5; R4 for native and RPM.
- Next: full Flatpak, Snap and AppImage builds with a packaged launch of each artifact.
- Expected evidence: built artifacts, private ABI `--test` inside each sandbox and a GPU smoke.

## Material Decisions
- 2026-10-11: magpie sources from upstream 49573fffefd0c1dc9c2ebcc6a24b0619b3403725 remain at
  subprojects/magpie; app-rummage remains pinned at 26b93b8c6f0de6912f0a17022f7ece76bad87612.
- 2026-10-11: all three GPU views, honest driver limits and private open libraries are approved.

## Evidence
- Baseline main: 7d2240d; worktree and both submodules clean; backend formatting check passed.
- R1: backend `magpie-platform-linux` + `magpie-types` release tests 53 passed; backend
  formatting clean. Backend `Cargo.lock` and root `Cargo.lock` unchanged; vendored sources differ
  from upstream 49573ff only in GPU/process/IPC files plus the moved `.gitmodules`; the sole
  gitlink is app-rummage 26b93b8. Backend strict Clippy still reports 12 inherited warnings in
  files byte-identical to upstream (about, battery, cpu/power_draw, disks/disk_wrapper, fan).
- R1 live IPC (packaged RPM stage, nested KWin, AMD Radeon 780M, amdgpu/RADV): first reading has
  no GPU (cold start), then one device with telemetry; 5 distinct sequences in one epoch; engines
  gfx/compute/enc, 16 DRM clients, dedicated/shared memory from the driver, temperature/power/clock
  present; all 575 processes carry `start_time_ticks`.
- R2/R3: root `cargo test --workspace --locked -- --test-threads=1` 138 passed, 4 existing
  ignored; `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo check` and
  `cargo fmt --all --check` clean. Renders: `target/ui-snapshots/gpu-{processes,details,
  performance,summary}.png`. A parallel offscreen run once hit SIGSEGV; the serial run is the
  evidence.
- R4: `taskmanager-1.2.0-9.fc44.x86_64.rpm` ships libdrm, libdrm_amdgpu, libudev and libgcc_s under
  `/usr/lib/taskmanager/gpu` with manifest and notices; no private Provides, no host libdrm/libudev
  Requires, GUI RUNPATH unchanged, daemon RUNPATH relative. Packaged GUI smoke: 31 frames, 11
  readings applied in 10 s (clipboard warning only, DISPLAY unset on purpose). Bundle negative
  tests, matching-rootfs bootstrap (glibc 7 libs, musl 3 libs) and AppImage relocation simulation
  passed; full Flatpak/Snap/AppImage builds were not run.
- Limitations: no NVIDIA, Intel or multi-GPU hardware and no hotplug were exercised physically;
  those paths are covered by synthetic and parser tests only.
- Follow-up: partial GPU values drop the `~` prefix (reason stays in the tooltip); RPM
  1.2.0-10 rebuilt, root gates (138 passed, 4 ignored) and packaged GPU smoke re-passed.
