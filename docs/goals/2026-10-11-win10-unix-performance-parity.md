# Goal: Win10 Performance parity with Unix memory semantics

Status: complete
Source: user screenshots, audited plan, “Реализовать и коммит пуш; делай копию плана в цель и итеративно реализовать”, and the rejected tooltip/footer renders followed by “реализовать и коммит пуш”.
Last updated: 2026-10-12

## Objective
Restore logical-CPU hover labels, Unix memory composition and graph explanations; make the
lower disk graph a thin strip while preserving the compact baseline and saved window geometry.
Implement, commit, push and verify CI on the pushed SHA.

## Execution Directive
Complete the frozen Required Outcomes using the listed Change Envelope and Primary Evidence.
Work on the smallest unresolved outcome. Do not add requirements from reviews, tests, tools,
speculative risks, or optional source text. Finish when every required outcome is resolved and
affected constraints remain satisfied.

## Frozen Contract
### Required Outcomes
- R1: Restore Memory composition with Used / Cache / Shared / Buff / Free and region tooltips.
  - Source: user Memory screenshots and audited plan A.
  - Acceptance: disjoint segments total MemTotal; honest definitions, empty unknown total,
    no active Standby/Modified terminology; unchanged existing RAM metrics.
  - Primary evidence: composition and synthetic tests; actual Memory render/hover assertions.
  - Status: verified
  - Evidence: composition unit tests and synthetic consistency passed; default/HiDPI region,
    volume, frame and RAM-tooltip render assertions passed.
- R2: CPU ordinal hover labels and RAM/Disk contextual graph explanations.
  - Source: user CPU/Disk screenshots and audited plan B.
  - Acceptance: CPU N labels match logical-mode graph positions; ordinary CPU/RAM/Disk,
    summary and composition tooltips anchor near the pointer, not the graph/bar corner.
    Preserve click, context-menu and summary behavior without new repaint/cache invalidation.
  - Primary evidence: CPU model and real tooltip-position/input tests.
  - Status: verified
  - Evidence: tooltip rectangle assertions passed for two points in one CPU cell, summary,
    RAM, composition and both Disk graphs; the long RAM tooltip stays inside the right edge.
    Existing label, clipping, menu and double-click assertions passed; real renders reviewed.
- R3: Thin disk transfer graph and usable Memory vertical stack at the existing minimum.
  - Source: circled disk comparison, compact-window request, audited plan C.
  - Acceptance: disk lower graph 66 logical points; composition 52; Memory-only overflow
    scroll; default 1030×705, minimum 760×540 and sidebar 211 preserved along with saved sizes.
  - Primary evidence: actual default/minimum/tall/HiDPI render assertions and existing probes.
  - Status: verified
  - Evidence: Disk default/minimum/tall 66-point frame assertions passed; Memory minimum
    Swap+compression scroll, resize/device/summary reset and fixed sidebar/footer passed.
- R4: Commit and push the implementation; verify the required CI and expected outputs.
  - Source: latest user instruction and AGENTS.md Verify.
  - Acceptance: intended clean commit on origin; required jobs and expected artifacts/releases
    successful for that SHA.
  - Primary evidence: Git SHA/ref, GitHub Actions jobs and artifacts/release.
  - Status: verified
  - Evidence: correction f4073e29af23074af0c9fc185e067953bfbb03de pushed to origin/main;
    [Actions 38212353483](https://github.com/0FL01/win10-task-manager-on-linux/actions/runs/38212353483)
    succeeded (checks, metadata, five native jobs, publish). Six nonempty, unexpired artifacts
    verified on that SHA: ui-snapshots and native packages for Arch, Debian 13, Fedora 44,
    Tumbleweed and Ubuntu 26.04. [v1.2.14](https://github.com/0FL01/win10-task-manager-on-linux/releases/tag/v1.2.14)
    is published for that SHA with five uploaded packages and one source archive.
- R5: Remove the separator above the full Performance footer without changing geometry.
  - Source: the user's crossed-out footer line and approved correction plan.
  - Acceptance: no separator in expanded Performance; preserve footer/content coordinates,
    other tabs, compact mode, summary mode, graph sizes and Memory scrolling.
  - Primary evidence: Performance white-footer probes and unchanged other-tab/compact probes.
  - Status: verified
  - Evidence: full Performance footer is white across its interior at y=655 and at the
    minimum-size anchor y=490; all other-tab and compact footer probes still pass.

### Constraints
- Preserve Mission Center attribution, clean-room assets, i18n and bounded/event-driven UI.
- No backend/IPC changes, I/O in UI, new settings, dependencies, cache layer or public test hooks.
- Keep CPU graph geometry/probes, GPU behavior, page-wide click ownership and graph/sidebar/stat data.
- Never commit reference screenshots, private readings, build artifacts or credentials.

### Non-goals
- Resetting saved windows/sidebar, kernel CPU ID guarantees, horizontal stats/info reflow.
- Windows-only memory entities, historic samples under cursor, universal tooltip/layout systems.
- New settings/migrations, GPU fixes, unrelated cleanup, screenshot/test matrices or benchmarks.

## Change Envelope
- `mc-ui/perf`: MemComposition, CPU/Memory/Disk tooltip caches, GraphPanel/PageView and their
  seven direct producers; `win10/performance.rs`, metrics, nearest model/probe tests.
- `mc-magpie/src/synthetic.rs`: consistent example memory counters and its existing test.
- `docs/win10-spec/spec.md`, this goal and `po/taskmanager.pot`.
- Correction: `mc-ui/src/app.rs`, `win10/chrome.rs`, the existing performance tooltip calls,
  `tests/win10_probes.rs` and `docs/win10-spec/probes.json`. No text/POT changes are needed.
- No collector/schema, configuration migration, packaging or dependency changes expected.

## Copy of the audited execution plan

### A. Unix composition and consistent synthetic data
In `MemComposition::update`, raw Free = mem_free, Shared = sh_mem, Buff = buffers,
Cache = cached.saturating_sub(sh_mem).saturating_add(s_reclaimable). Clamp Free → Shared →
Buff → Cache to remaining MemTotal; Used is the remainder. Explain adjustments; total zero
is unavailable, not a fictitious byte. Cache differs from the existing Cached card; Used
differs from the MemAvailable-based RAM graph. Free is collector-reported and may include
reclaimable ZFS ARC. Leave collector/IPC and existing RAM calculations unchanged.

Draw Used / Cache / Shared / Buff / Free with existing MEMORY.fill/line and WHITE,
square mesh borders, cumulative byte boundaries shared by paint/hit detection, no artificial
minimum widths. Cache a selected-category explanation plus all-five summary on update.
Use an explicit Rect for the widget. Delete dead Memory `tt_*` and `standby` calculation.

Keep synthetic Cached and existing RAM wave U/MemAvailable. Add nonzero Shared and Buff;
Free = MemTotal − U − Buff − Cache, hence Used_bar = U − Shared. SReclaimable stays default;
its nonzero contribution belongs in a model example, not extra synthetic fields.
Test known partition/overlap/distinct metrics, zero/inconsistent counters and hit boundaries;
extend the nearest synthetic consistency test with a few wave samples.

### B. One connected UI batch
Add borrowed `GraphPanel::tooltips: &[String]` (constructors default empty) and optional
`PageView::composition: Option<&MemComposition>`, updating all producers/consumers together.
CPU caches CPU {index} alongside per_core and recreates on count changes; logical mode only.
These are array ordinals, not promised kernel CPU IDs. RAM caches its usage explanation;
Disk caches two static descriptions. No historic cursor values.

Retain normal page_ui paint-loop and page-wide click handling; add clipped Sense::hover
regions for graphs with tooltips, keyed by selected device/panel/slot. Add a single fallback
tooltip to existing summary helper; GPU tile tooltip takes priority. Do not convert normal
graphs to clickable cells or change GraphResponse/page_ui return type. Composition is hover-only.
Leave mc-graph, cache and repaint policy unchanged.

### C. Only necessary geometry
Keep default 1030×705, minimum 760×540, sidebar 211 and user saved sizes; no reset/migration.
Disk lower graph = 66 logical points, upper = remaining height, current three statistic rows
and metadata unchanged. Expected upper/lower: default 211/66, minimum 46/66, tall 291/66.
One-row panel height is its outer graph Rect height; no extra GRAPH_GAP correction.

Composition = 52 after RAM/optional Swap, before stats; RAM/Swap minimum = 52.
For q graphs, minimum stack = q×52 + (q−1)×37 + 37 + 52. Reuse existing ListState/wheel/scrollbar
for Memory-only whole-page vertical overflow, clipping title/graphs/composition/stats/info;
sidebar/footer fixed, reset on device/summary transition and clamp on resizing/extras changes.
Minimum with Swap + compression: virtual 545, viewport 413, scroll 132. Preserve horizontal
stats/info policy, CPU geometry and GPU scroll. Summary has no composition.
Document heights and Unix accounting as own adaptations, not historical Windows probes.

### D. Focused acceptance and localization
Reuse win10_probes harness: CPU0/CPU1 hover plus right/double-click and summary;
default Memory composition boundaries and selected category/value (not just summary names),
RAM tooltip and one ppp=2 bar render; Disk default/minimum/tall plus both tips;
minimum Memory Swap+compression scroll, resize and device/summary reset. Use simulated tooltip
time and actual pixel/label assertions, not sleep-only or image-width-only checks.
No full dimensions×DPI×extras×device matrix. Keep existing probes/regressions unchanged.

Extract POT once after texts stabilize; do not mass-update PO translations.
Run narrow changed-path tests, then one final workspace gate because shared views and fixtures
change; no duplicate full snapshot/graph runs. Run fmt and exact Rust 1.96.1 strict Clippy.
One fresh `--no-config --synthetic 300` smoke; real ZFS readings optional if available.
Commit, push and verify required GitHub Actions jobs and expected artifacts/releases.

## Current Checkpoint
- Closed: R1–R5. Local gates, correction-SHA CI, native/UI artifacts and release assets verified.
  No runtime work remains.

## Current State
- Resolved: R1–R5 verified; original implementation 6669726, accepted correction f4073e2.
- Current repair evidence: `cargo +1.96.1 test --locked -p mc-ui --test win10_probes --
  --test-threads=1` (18 passed, 2 existing ignored), `cargo +1.96.1 fmt --all --check`,
  `cargo +1.96.1 test --locked --workspace -- --test-threads=1` (155 passed, 5 existing ignored)
  and `cargo +1.96.1 clippy --locked --workspace --all-targets -- -D warnings` all passed.
  Real CPU, RAM, composition and right-edge tooltip renders reviewed in target/ui-snapshots.
  No counter, layout-size, tooltip-text, localization-template or GPU behavior changes.
- Blocker: none.
- Delivery evidence: Actions 38212353483 succeeded; six CI artifacts and v1.2.14 assets verified
  against f4073e29af23074af0c9fc185e067953bfbb03de.
- Next: none; objective complete.

## Accepted correction plan
- Use egui 0.36.2 `on_hover_text_at_pointer` for ordinary graph cells, the summary fallback
  and composition tooltips. Keep clipping, IDs, delay, text and GPU tile behavior unchanged.
- Add `draw_separator` to `footer_line`; keep its coordinate return unchanged. The only caller
  passes `compact || self.page != MainPage::Performance`; summary continues to bypass it.
- Document the full-Performance exception. Move the grey common footer probe to the other
  three tab groups and add the white Performance probe; compact retains its grey probe.
- Extend existing CPU/RAM/Disk/composition tests with tooltip rectangle assertions, including
  two positions in one CPU cell and a long tooltip near the right edge. Update the minimum
  Performance footer assertion. No new framework, tooltip renderer, counters or settings.

## Checkpoint History
- 2026-10-11: frozen user-approved contract and copied corrected plan; no implementation yet.
- 2026-10-11: A/B/C and focused D implemented; measured assertions confirm target dimensions,
  cached Unix tooltips and preserved click behavior. R1–R3 targeted evidence successful.
- 2026-10-12: final gates and synthetic smoke successful; closure diff stays within the envelope.
  No backend/config/dependency changes or generated images included. GitHub Actions tools can
  access the native workflow; local `gh` is absent, so use those tools to verify CI after push.
- 2026-10-12: 6669726 pushed, all eight CI jobs succeeded, six artifacts and release v1.2.12
  verified against its exact SHA. Frozen outcomes and closure check complete.
- 2026-10-12: user rejected tooltip location and Performance footer separator. Reopened R2/R4,
  added explicit R5 and copied the approved narrow correction plan; no adjacent redesign.
- 2026-10-12: R2/R5 correction and regression assertions passed, including real pointer-adjacent
  renders and unchanged other-tab/compact separators. Final fmt/workspace/1.96.1 Clippy gates
  passed; only R4 delivery verification remains.
- 2026-10-12: f4073e2 pushed, all eight CI jobs succeeded, six CI artifacts and published
  v1.2.14 with six assets verified against the exact correction SHA. Closure check complete.

## Completion
- Resolved outcomes: R1–R5 verified with current local and correction-SHA CI evidence.
- Commands and artifacts: fmt, 155 workspace tests (5 existing ignored), strict Rust 1.96.1
  Clippy, Actions 38212353483, six CI artifacts and v1.2.14 native packages/source archive.
- Constraint and diff-scope check: only approved tooltip anchoring, Performance separator,
  spec/probes/tests and goal state changed; other tabs, compact/GPU behavior, graph geometry,
  memory accounting, text, settings, dependencies and backend remain unchanged.
- Final status: complete.
