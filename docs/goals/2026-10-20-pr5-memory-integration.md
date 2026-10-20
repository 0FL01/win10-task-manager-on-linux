# Goal: integrate PR #5 memory accounting and device identities

Status: complete
Source: https://github.com/0FL01/win10-task-manager-on-linux/pull/5; user-approved integration plan and request to implement, commit, push and verify GitHub Actions.
Last updated: 2026-10-20

## Objective
Merge original commit `b5ef1603d015a09e38caee3a1612bcd72071ec04`, apply the agreed narrow corrections, and deliver a tested main-branch push with successful native-package CI and release evidence.

## Execution Directive
Complete the frozen Required Outcomes using the listed Change Envelope and Primary Evidence. Work on the smallest unresolved outcome. Do not add requirements from reviews, tests, tools, speculative risks, or optional source text. Finish when every required outcome is resolved and affected constraints remain satisfied.

## Frozen Contract

### Required Outcomes
- R1: preserve the original PR commit and its complete feature set: occupied RAM including cache, truthful capacity/type heading, sidebar identities, explanatory hover/copy, translations and config-free Memory launch preset.
  - Source: PR #5 and approved plan, steps 1–2, 4–6.
  - Acceptance: original commit is an ancestor; all four RAM consumers agree; new interactions and preview work without persistent settings.
  - Primary evidence: focused core/UI/CLI regression tests and offscreen renders.
  - Status: verified
  - Evidence: integration branch fast-forwarded from exact PR base `20749093` to original PR commit. Core/UI/CLI regressions and serialized workspace suite pass, including graph samples/visibility, full model copy and explanatory-hover recovery. Real light/dark/minimum/2x renders inspected in `target/ui-snapshots/`. Owned-session live/synthetic preview smoke passed on both wgpu/glow; `strace` confirms saved GUI config was never opened, and bytes/mtime remained unchanged.
- R2: correct incomplete/contradictory inventory handling, uncertain RAM identity, capacity formatting/explanation, translated network line and documentation sizing claims.
  - Source: approved plan, steps 3–5, 7.
  - Acceptance: no partial inventory masquerades as installed capacity/common model; legacy positive capacities remain usable; existing 60/66 px rule is documented.
  - Primary evidence: inventory/model/heading/network tests, gettext checks and updated spec.
  - Status: verified
  - Evidence: exact-count/index completeness shared by Memory/System; unknown occupancy retained for identity. Inventory, late-delivery, heading and translated-network catalog tests pass. Updated spec/preset documentation; regenerated POT and `msgfmt --check` pass.
- R3: verify backend ARC changes leave raw MemFree untouched and retain the existing availability adjustment.
  - Source: approved plan, step 7.
  - Acceptance: deterministic reader-based ARC tests and separate backend release tests pass.
  - Primary evidence: magpie-platform-linux and magpie-types release tests.
  - Status: verified
  - Evidence: Rust 1.96.1 backend release suite: 90 passed, one existing hardware-only ignore; ARC reader regressions cover unchanged free, availability correction, missing/below-minimum values and saturation. Backend binary builds and `magpie --test` succeeds.
- R4: commit and push the integration, verify successful build/CI on the pushed SHA and expected artifacts/release.
  - Source: user request; AGENTS.md Verify.
  - Acceptance: Rust 1.96.1 formatting, strict Clippy, workspace/backend tests pass; GitHub checks and five native builds succeed; expected artifacts and eligible release exist.
  - Primary evidence: command results, GitHub Actions run/jobs and artifact/release metadata for the pushed SHA.
  - Status: verified
  - Evidence: Rust 1.96.1 root/backend formatting, strict workspace/all-targets Clippy, serialized workspace tests, backend release tests, both diagnostic-profile suites, 30 CI-helper and 27 hardware-helper Python tests, shell syntax and gettext checks pass. Main push `1eb852cde3526831ba6341b02927cdc97f09a887` passed [Actions #44](https://github.com/0FL01/win10-task-manager-on-linux/actions/runs/39403622910): checks, metadata, all five native builds and publish. The run exposes five native artifacts plus ui-snapshots with SHA-256 digests. Published [v1.2.44](https://github.com/0FL01/win10-task-manager-on-linux/releases/tag/v1.2.44) has five packages and the project source archive; remote tag resolves to that exact SHA.

### Constraints
- Preserve attribution, clean-room assets, public IPC contracts, GUI-free core layers, asynchronous data intake and existing settings/single-instance behavior.
- No new dependencies or privileged data sources; no unconditional repaint; no private dumps/reference images committed.
- Run Rust formatting after edits; strict Clippy uses exactly 1.96.1.

### Non-goals
- Redesign, configurable RAM formulas, SPD capacity fallback, graph-core/startup/packaging redesign, unrelated ARC parser changes or cleanup.

## Change Envelope
- Original PR's 23 paths; corrective changes in mc-ui memory/mem_slots/system/network and tests, mc-core memory tests, app CLI/tests, backend memory reader/tests, po and directly relevant docs.
- No Cargo dependency, workflow, daemon protocol, service or worker changes.
- R4 gate-driven expansion: `support/ci/verify-native.sh` and its existing argument tests may explicitly install Debian/Ubuntu optional hardware prerequisites. Run #43 built/installed Ubuntu successfully but failed `test -x /usr/bin/pkexec`; its minimal image did not install package recommendations. Keep all prerequisite probes intact.
- Generated renders/build outputs stay in ignored target directories; no private fixtures.

## Current Checkpoint
- Closed: R1–R4 and the constraint/diff-scope check.
- No further implementation work. Commit this closure record and verify its documentation-only push; a second release is not expected without unpublished Rust changes.

## Current State
- Resolved: R1–R4; original commit preserved, PR #5 merged, main integration and verifier correction published as v1.2.44. No dependency/protocol/workflow changes.
- Last relevant evidence: exact-SHA Actions run #44 succeeded, including installed-package verification on Fedora 44, openSUSE Tumbleweed, Arch, Debian 13 and Ubuntu 26.04. Six CI artifacts and six uploaded release assets independently confirmed from public metadata; tag SHA confirmed with `git ls-remote`.
- Blocker: none.
- Next: none after publishing/verifying this closure record.

## Material Decisions
- 2026-10-20: include original commit without rewriting authorship; changes above it remain separate. Apply agreed occupied-RAM semantics to ordinary launches as well as preview.
- 2026-10-20: complete inventory requires exact count, valid unique indices when present, all-absent indices for legacy compatibility; mixed-index inventory is not authoritative.

## Checkpoint History
- 2026-10-20: fetched and fast-forwarded original PR commit; no upstream divergence.
- 2026-10-20: corrected inventory/identity/format/i18n/docs, added focused graph/CLI/ARC/copy regressions; all local gates passed. Confirmed egui intentionally suppresses post-click tooltips until pointer movement, so the copy regression models that interaction instead of changing production behavior.
- 2026-10-20: `cargo xtask bench-idle` with an ignored Memory-preview smoke adapter passed all four live/synthetic × wgpu/glow runs with matching backend, real frames/readings and no saved-config access/change. This was a launch smoke, not a performance measurement; no adapter-generated benchmark numbers are claimed. The private session intentionally lacks host user-systemd/clipboard services.
- 2026-10-20: run #43 (`39402714767`, SHA `03224fe`) passed checks, Debian and Fedora; Ubuntu built/installed its package but failed the retained `pkexec` prerequisite probe because the image omits recommended packages. Minimal R4 expansion: explicitly install the already-recommended Debian hardware prerequisites in the verifier, matching existing RPM/Arch policy.
- 2026-10-20: corrective push `1eb852c` passed all eight jobs in run #44 (`39403622910`); five native packages and source archive published as v1.2.44. All original runtime/layout/data invariants remain covered by the green local/remote gates. Closure check passed; no additional implementation scope opened.

## Completion
- Resolved outcomes: R1–R4 verified.
- Commands and artifacts: exact Rust 1.96.1 gates listed above; real isolated preview smoke; successful Actions #44; native-arch, native-debian13, native-fedora44, native-opensuse-tumbleweed, native-ubuntu26.04 and ui-snapshots; release v1.2.44 at `1eb852cde3526831ba6341b02927cdc97f09a887` with five native packages and one project source archive (in addition to GitHub's automatic source links).
- Constraint and diff-scope check: original author/history retained; no dependencies, IPC, workers, workflows, private data or clean-room assets changed. Only the observed CI prerequisite installation required the recorded two-path expansion. No tests or validation checks disabled; existing environment-dependent ignores unchanged.
- Final status: complete.
