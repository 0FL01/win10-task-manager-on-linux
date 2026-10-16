# Goal: One Performance battery page

Status: complete
Source: User's unified-battery mockup, icon clarification, approved audited plan, and implementation/commit/push instruction of 2026-10-16.
Last updated: 2026-10-16

## Objective
Show every collected battery under one Performance sidebar item, with independent charge histories, readable device cards and original Task-Manager-style icons. Preserve existing battery details and saved sidebar preferences. Commit and push; finish only after successful Actions and verification of the pushed SHA's expected artifacts/release.

## Execution Directive
Complete the frozen Required Outcomes using the listed Change Envelope and Primary Evidence. Work on the smallest unresolved outcome. Do not add requirements from reviews, tests, tools, speculative risks, or optional source text. Finish when every required outcome is resolved and affected constraints remain satisfied. Stop substantive work at a proven external blocker, an approved budget boundary, or when no remaining in-scope action has a falsifiable expected result; record the exact evidence and smallest unlock.

## Frozen Contract

### Required Outcomes
- R1: One battery page with independent device data.
  - Source: User asks to combine all batteries into one item; approved audit step 1.
  - Acceptance: Exactly one `battery` page while batteries exist; stable per-device histories/colors, genuine zero versus unknown, no aggregate percentage, and correct partial/total removal behavior.
  - Primary evidence: Focused aggregate/lifecycle unit tests.
  - Status: verified
  - Evidence: `taskset -c 0,1,2 cargo +1.96.1 test --locked -p mc-ui --lib batter`: three tests passed (aggregate identity/history/copy, host-only Powering System, late insertion/replacement/removal/return and startup selection).
- R2: Task-Manager-style presentation and preserved details.
  - Source: User's mockup, rejection of crude/composite icons, icon clarification, approved audit step 2.
  - Acceptance: One multicolored 0–100% graph, named charge/status cards, original neutral battery/mouse/headphone glyphs, readable scrolling layout, keyboard/pointer controls, graph summary and all-child Copy. Existing optional details remain available; Powering System is only for Bat/Ups, including valid No.
  - Primary evidence: Focused real-render/interaction checks using the existing UI harness.
  - Status: verified
  - Evidence: Both `performance_batteries` real-render probes pass: default three cards, 2× neutral glyph, paused Light/Dark/System line/indicator correspondence, primary/double/keyboard/context-menu input, graph summary, all-child Copy, minimum-width wrapping, overflow focus and metadata Copy. Actual Light/Dark and minimum-detail renders visually inspected under `target/ui-snapshots/`.
- R3: Preserve saved navigation and existing graph behavior.
  - Source: Approved audit's selection/migration and optional-color contracts; repository layer/theme rules.
  - Acceptance: Migrate saved and live legacy selection/order/visibility idempotently; existing new-key override wins. Non-battery graph colors/layout remain unchanged, including theme-only repaint.
  - Primary evidence: Migration tests, graph-color regression tests and existing workspace probes.
  - Status: verified
  - Evidence: Four migration tests, ten graph tests and the complete workspace suite pass, including the existing non-battery theme, geometry and snapshot probes.
- R4: Verified commit/push delivery within the CPU limit.
  - Source: User: "приступай к реализации, сделать комит пуш, проверить экшенс" and compile/tests on two–three CPU cores; AGENTS.md Verify.
  - Acceptance: Local required gates pass, changes committed/pushed, Actions tools verify successful jobs on the pushed SHA plus expected artifacts/release. Local build/test commands and their children use CPU affinity 0,1,2, CARGO_BUILD_JOBS=2 and RUST_TEST_THREADS=2.
  - Primary evidence: Local gate results and GitHub Actions/API evidence for the pushed SHA.
  - Status: verified
  - Evidence: POT, formatting, exact Rust 1.96.1 workspace Clippy with -D warnings and the complete locked workspace tests pass, using CPUs 0,1,2, two Cargo jobs and serial tests. Commit `a31dc78d3c0c004e763ad449cc26f59e4c0e837f` was pushed to main. Actions tools verified all eight jobs successful in run [38946645907](https://github.com/0FL01/win10-task-manager-on-linux/actions/runs/38946645907); API checks verified six nonempty, unexpired artifacts on that SHA and published [v1.2.34](https://github.com/0FL01/win10-task-manager-on-linux/releases/tag/v1.2.34), with the release target/tag matching that commit and all five packages plus source uploaded.

### Constraints
- Keep UPower/HID++ collection, battery schema, IPC, dependencies and host permissions unchanged.
- Preserve Mission Center attribution and clean-room rules. New glyphs/geometry are explicitly own design; no Microsoft assets or references committed.
- One optional renderer-local group-color API, one battery-specific typed view hook, and a pure sidebar migration helper are the approved shared extensions. No generic card/history/icon framework.
- Use existing data intake and grid clock; rendering, expansion and theme changes do not append samples.
- Clamp the new page's scrolling and use stable, named keyboard/pointer controls. Keep other pages' layout and behavior unchanged.
- All local compilation/tests, including delegated checks, are restricted to the same three CPUs with taskset; at most two Cargo build jobs and two test threads.

### Non-goals
- Fixing preexisting unavailable-power/weekly-history semantics, UPower limitations, Flatpak packaging, or other monitoring paths.
- New device discovery/transports, settings, services, channels, persistent histories, identity registries, child visibility controls or hidden-window selection synchronization.
- Benchmarks, additional renderers, a theme/DPI cross-product or historical pixel-probe changes.

## Change Envelope
- `crates/mc-ui/src/perf/{mod,battery,batteries}.rs`: aggregate model, child detail reuse, identity/lifecycle and tests.
- `crates/mc-core/src/sidebar.rs`: pure saved-key migration and tests; UI updates live selection at its owner.
- `crates/mc-graph/src/lib.rs`: optional `[Light, Dark]` group stroke colors and narrow tests, without graph-core changes.
- `crates/mc-ui/src/win10/{performance,metrics,palette}.rs`: specialized layout, controls, scrolling and original glyphs.
- `crates/mc-ui/tests/win10_probes.rs`: focused real renders/interactions using existing fixtures/harness.
- `docs/win10-spec/spec.md`, `po/taskmanager.pot`, this goal.
- No manifests, lockfiles, backend sources, CI configuration or unrelated refactoring.

## Approved Execution Plan
1. Implement the aggregate model and compatibility migration: one page; keyed histories; NaN initialization; one complete sample per reading; normalized current percentage; existing details/all-child Copy; saved and live selection migration.
2. Integrate graph colors and the specialized UI. Keep the existing single-color fallback. Cards use 224-point minimum width, 104-point height and 12-point gaps; charge graph is half the viewport height clamped to 160–320 points; optional detail graphs are 120 points. Sidebar glyph: 36×22 body plus centered 4×8 terminal, overall 40×22; neutral LABEL ink. Summary remains graph-only. Card toggles, keyboard focus, clipping, context-menu and Copy ownership are explicit.
3. Record own-design geometry, update POT, run narrow proofs then required workspace gates under the CPU budget, commit/push and verify Actions/artifacts/release. Do not repeat unchanged gates or add unrelated fixes.

## Current Checkpoint
- Closes: All implementation outcomes are verified; only delivery of this completion record remains.
- Smallest next action: Commit/push this documentation-only closure and verify its Actions run and artifacts. Do not create another documentation commit merely to embed its own SHA or run ID.
- Expected evidence: Successful final-push jobs and six expected artifacts; no new release is expected without unpublished Rust changes.
- Stop or replan if: Final delivery fails; diagnose only the confirmed failure, without reopening completed implementation work.

## Current State
- Resolved: R1–R4; implementation, original-design spec and POT are delivered in `a31dc78`, with successful CI and release v1.2.34. Prior mouse/Powering System deliveries remain preserved.
- Last relevant evidence: Complete workspace gate passes: 81 core tests, 10 graph tests, 49 UI units, nine snapshots, 29 Win10 probes and 12 resize tests, plus other workspace suites. Exact Rust 1.96.1 Clippy and formatting pass. Every compilation/test has used CPUs 0,1,2 with two Cargo jobs and at most two test threads.
- Blocker: None.
- Next: Deliver this completion record and verify its push, then stop. No more runtime edits, backend/CI changes or repeated local gates.

## Material Decisions
- 2026-10-16: Details selection is transient UI state, validated against current cards before display; no App intake synchronizer or generation counter.
- 2026-10-16: Legacy Hide folds into group Hide only for a nonempty known-key set with every member explicitly hidden; visible group shows all current children.
- 2026-10-16: Unknown current charge is an em dash/NaN, not zero; a newly arrived series need not draw a stroke before its second valid sample.

## Checkpoint History
- 2026-10-16: Preflight completed; contract and implementation scope frozen. No tests run yet.
- 2026-10-16: Model/compatibility checkpoint passed: mc-core migration 4/4, mc-graph 10/10, battery UI model 3/3. No backend/schema/dependency/host changes.
- 2026-10-16: Presentation checkpoint passed: both original-design UI probes, actual image inspection, keyboard overflow, copied summaries/metadata and summary/menu ownership verified. Harness checks inspect transient Copy on key-down and separate double-click sequences from earlier menu clicks.
- 2026-10-16: Integration checkpoint passed: POT generation, formatting, exact-toolchain Clippy and complete workspace tests. Final scope review confirms the approved paths only. Retained unrelated existing POT entries that the current extractor does not discover; no extractor refactor or translation loss.
- 2026-10-16: Delivery checkpoint passed: main commit `a31dc78`, Actions run 38946645907 (first attempt), all eight jobs successful. Verified six workflow artifacts and release v1.2.34 with six nonempty uploaded assets and matching target/tag SHA. Closure check passed within the original twelve-path envelope and CPU budget.

## Completion
- Resolved outcomes: R1–R4 verified.
- Commands and artifacts: Resource-bounded focused tests, POT/format checks, exact Rust 1.96.1 Clippy and complete workspace tests; successful Actions run 38946645907, six verified artifacts and release v1.2.34.
- Constraint and diff-scope check: Approved twelve paths only; no backend, schema, IPC, dependency, lockfile, CI or host-permission changes. Original glyphs/spec, existing attribution, details and host-only Powering System behavior preserved. All local compilation/tests used the requested CPU limit.
- Final status: complete. The documentation-only delivery of this record is verified separately, without an additional self-referential closure commit.
