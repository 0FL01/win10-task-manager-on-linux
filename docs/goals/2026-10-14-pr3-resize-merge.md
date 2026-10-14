# Goal: Merge PR #3 with correct native resize initiation

Status: active
Source: https://github.com/0FL01/win10-task-manager-on-linux/pull/3; approved review/plan below; user: «делай копию плана в цель и итеративно реализовать», «работа считается выполненой когда форкфлоу успешно завершился».
Last updated: 2026-10-14

## Objective
Integrate PR #3 after the agreed local resize-initiation correction and regression tests; finish only after the workflow on the merged main SHA succeeds and its expected packages, snapshots and release are verified.

## Execution Directive
Complete the frozen Required Outcomes using the listed Change Envelope and Primary Evidence. Work on the smallest unresolved outcome. Do not add requirements from reviews, tests, tools, speculative risks, or optional source text. Finish when every required outcome is resolved and affected constraints remain satisfied. Stop substantive work at a proven external blocker, recording the exact evidence and smallest unlock.

## Frozen Contract

### Required Outcomes
- R1: Preserve the approved plan and durable execution state.
  - Source: latest user instruction.
  - Acceptance: this goal contains the approved plan, evidence and next checkpoint.
  - Primary evidence: this document and git history.
  - Status: verified.
  - Evidence: plan copied below; work branch `fix/pr3-resize-handoff` starts at reviewed head `77be81983ca179b09b96253293613255d68f4b0b`.
- R2: Preserve PR #3's resize recovery, L-shaped corners and exact-border cursor behavior; correct initiation using a real current-frame Primary press without bypassing egui interaction arbitration.
  - Source: approved plan Goal, done criteria and changes 1–3.
  - Acceptance: existing four tests plus the agreed batching/button/seam/modal cases pass; one native command uses the last Primary press's zone, and handoff clears drag/pointer only when native resize starts.
  - Primary evidence: `cargo +1.96.1 test --locked -p mc-ui --test window_resize`.
  - Status: verified.
  - Evidence: original PR failed four new tests (press+leave, moved press direction, interior press+move, Secondary with Primary held); corrected `window_resize` passes all 12 tests. Existing modal, release and seam contracts remain green.
- R3: Verify the changed path and native behavior before integration.
  - Source: approved plan Checks; AGENTS.md Verify.
  - Acceptance: Rust 1.96.1 fmt, exact-toolchain workspace Clippy and workspace tests pass; native Wayland/X11 smoke observations and limitations recorded honestly.
  - Primary evidence: commands below, current offscreen outputs and native smoke observations.
  - Status: verified.
  - Evidence: Rust 1.96.1 fmt/check, targeted resize test, exact-toolchain workspace Clippy `-D warnings`, and single-thread locked workspace tests pass. Workspace output: `target/pr3-workspace-tests.log`; 9 snapshot and 27 Win10-probe tests passed (2 existing ignored probes). Actual KWin 6.7.5 Wayland native smoke passed with DISPLAY removed; X11-client smoke passed with WAYLAND_DISPLAY removed (XWayland, not a standalone X11 compositor). Temporary XTEST input/KWin geometry observations verified repeated East/NW/North resizing, interior non-resize, caption maximize/restore/close after resize, maximized guard, compact and performance-summary resizing. Native logs: `target/pr3-native-smoke.log`; real active-window captures: `target/ui-snapshots/pr3-native-{wayland,x11}.png`. This was automated native input, not a human manual smoke. No host settings or user configuration changed (`--synthetic 30 --no-config`).
- R4: Integrate the corrected PR and verify successful workflow completion on final main SHA.
  - Source: latest user finish line; approved plan Integration and Checks; AGENTS.md after-push rule.
  - Acceptance: reviewed correction is included in main with contributor attribution; all required jobs succeed, five native artifacts and ui-snapshots exist, and expected release targets main SHA with five packages plus source archive.
  - Primary evidence: GitHub Actions run/jobs/artifacts and release/tag API for the final SHA.
  - Status: in_progress.
  - Evidence: corrected commit `2053e5b36e544d8432a95da17f18c7d5993563c4` was fast-forward pushed to the PR branch. [Fork run 38548411445](https://github.com/DurkaEbanaya/win10-task-manager-on-linux/actions/runs/38548411445) passed checks and four native builds; openSUSE failed during downloading with `curl: (35) Recv failure: Connection reset by peer`. Main/base remains `906d46f706d05669a1174122d23525ea506d1fe0`, release `v1.2.26`; PR remains open with maintainer modification enabled. Integration and final workflow evidence are pending.

### Constraints
- Keep existing egui response gate, maximized guard and modal/layer arbitration; retain all 12 zone registrations each frame.
- Keep Mission Center attribution, own-design metrics/spec, event-driven repaint and existing public APIs/dependencies.
- No unrelated runtime, backend, workflow or packaging changes; no force pushes or host-root changes.
- Correct and verify the PR branch before merging: main Rust changes automatically publish a release.
- No assertion of manual verification from synthetic events or CI snapshots alone.

### Non-goals
- General hit-testing/StartDrag redesign, new persistent input state, touch guarantees, compositor-specific fallbacks, egui upgrades, generic window-chrome abstraction.
- Preexisting border-press followed by movement into caption/content before frame processing; broader press arbitration stays unchanged.

## Change Envelope
- Runtime: `crates/mc-ui/src/win10/chrome.rs::resize_edges`; direct consumer `App::ui` unchanged.
- Tests: `crates/mc-ui/tests/window_resize.rs`, reusing its local input harness.
- Preserve PR changes in `win10/metrics.rs` and `docs/win10-spec/spec.md`; this goal tracks execution.
- Generated logs, renders and temporary native-smoke scripts remain ignored under `target/` or the approved external temporary directory. No private dumps or reference images committed.
- Commit/push and merge/publish are authorized by the instruction to execute the approved integration plan. No new dependency or CI architecture.

## Approved Plan (copy)

### Цель

Принять исправление залипающего resize и расширенные угловые зоны, устранив зависимость начала жеста от последней hover-позиции и ложное срабатывание от Secondary при удерживаемой Primary.

### Критерии done

- Существующие четыре regression-теста сохраняются.
- Нажатие на NW с последующим `PointerGone` в том же кадре запускает ровно один NW resize.
- Направление определяется местом реального Primary press, а не последующим перемещением между resize-зонами.
- Secondary без нового Primary press не запускает native resize и не сбрасывает pointer state.
- Сохраняются ограничения egui для modal/перекрывающих слоёв и существующий запрет resize в maximized-режиме.
- Проверки проходят на исправленном head; после merge проверены CI и релиз на итоговом SHA.

### Текущее устройство

- `App::ui` рисует содержимое, затем вызывает `chrome::resize_edges` только для немаксимизированного окна; диалоги рисуются после него.
- `resize_edges` регистрирует зоны через `ui.interact(..., Sense::drag())` и отправляет `ViewportCommand::BeginResize`.
- Compact и performance-summary используют тот же механизм. Их минимальные размеры не требуют ограничения 16-пиксельных углов.
- egui вычисляет interaction snapshot по итоговой позиции кадра. Поэтому **нельзя просто заменить interaction-проверку чтением raw events**: это обойдёт существующее распределение взаимодействий.

### Изменения

#### 1. `crates/mc-ui/src/win10/chrome.rs` — `resize_edges`

Сохранить геометрию и cursor-gating из PR, изменить только выбор начала native resize:

1. Из `InputState.events` получить позицию **последнего Primary press текущего кадра**. Сначала выбрать событие, затем проверить его позицию — не искать «последнее подходящее нажатие на рамке».
2. Найти направление по существующему массиву `zones`.
   - Искать с конца: `Rect::contains` включает границы, а на общих швах egui отдаёт приоритет последней зарегистрированной зоне.
3. Зарегистрировать все 12 зон как сейчас; накопить, сообщила ли **хотя бы одна** resize-response `drag_started_by(Primary)`.
4. После цикла отправить один `BeginResize`, только когда есть и этот egui-сигнал, и направление реального Primary press.
5. После команды сохранить `stop_dragging()` и сброс `PointerState`.
6. Для курсора оставить `response.hovered()` плюс точное попадание **текущей** hover-позиции в прямоугольник.

Это два локальных значения на кадр — `Option<ResizeDirection>` и флаг. Постоянное состояние или новая абстракция не нужны. Все зоны должны продолжать регистрироваться: ранний выход из цикла здесь не нужен.

**Проверка:** сценарии ниже, включая отсутствие сброса input при отклонённом старте.

#### 2. `crates/mc-ui/tests/window_resize.rs` — существующий input harness

Добавить следующие случаи, переиспользуя текущие `frame`, `press` и сбор `BeginResize`:

| События | Ожидание |
|---|---|
| NW press → `PointerGone` одним кадром | Один NW; drag и pointer очищены |
| NW press → move к East одним кадром | Один NW, не East |
| Press внутри caption → move на рамку одним кадром | Нет native resize |
| NW press → release → East press одним кадром | Один East: учитывается последнее нажатие |
| Primary удерживается с предыдущего кадра на caption-кнопке, затем Secondary на NW | Нет resize; состояния кнопок не сброшены |
| Primary press → release одним кадром | Нет resize и передачи жеста |
| Press на шве NW/North, например `(16, 2)` | North, согласно текущему порядку зон |
| Активный modal поверх основного UI | Нет native resize через modal |

Для modal-сценария показать `egui::Modal` после chrome, как в приложении, и предварительно прогреть кадры с открытым modal.

#### 3. Остальные файлы PR

- `metrics.rs`: оставить `RESIZE_CORNER = 16`.
- `docs/win10-spec/spec.md`: сохранить описание собственного дизайна.
- `app.rs`, зависимости, backend и workflow не менять.

#### 4. Интеграция

Внести поправку **в ветку PR до merge**. Не мержить известную проблемную версию с намерением немедленно исправить её в `main`: merge запускает автоматический релиз.

После проверок исправленного head выполнить squash merge с сохранением авторства и привязкой к проверенному SHA.

### Проверки

После изменения Rust выполнить форматирование, затем:

```bash
cargo +1.96.1 fmt --all --check
cargo +1.96.1 test --locked -p mc-ui --test window_resize
cargo +1.96.1 clippy --locked --workspace --all-targets -- -D warnings
RUST_TEST_THREADS=1 cargo +1.96.1 test --locked --workspace -- --test-threads=1
```

Полный suite включает существующие Win10 probes, snapshots, compact и HiDPI; для rendering-тестов нужен рабочий wgpu adapter. Обновлять эталоны изображений не предполагается.

Ручной smoke до merge:

- На KDE/Wayland повторить resize разных углов и сторон без дополнительного «разблокирующего» жеста.
- Проверить обычный release и клики caption-кнопок после resize.
- Проверить отсутствие resize из внутренней области рядом с рамкой.
- Проверить compact, performance-summary и maximized.
- На X11 выполнить короткую проверку повторного resize и последующего клика.

После merge дождаться CI **на итоговом merge SHA**: checks, пять native-сборок, `ui-snapshots` и `publish`. При текущем состоянии релизов изменения Rust должны породить новый релиз; проверить целевой SHA и шесть assets — пять пакетов и source archive.

### Допущения и риски

- Unit-тесты проверяют egui-события, но не реальную передачу жеста композитору; ручной smoke нужен отдельно.
- Сброс всего `PointerState` намеренный и допустим только после отправки native resize.
- Если head PR или база изменятся, проверки должны относиться к окончательному дереву, а не к уже зелёному `77be819`.

### Вне scope

- Общая переделка egui hit-testing и обработки `StartDrag`.
- Уже существующий случай «press на рамке → move внутрь caption/content до обработки кадра»: он требует более широкого изменения распределения взаимодействий, не исправления этого PR.
- Новые гарантии touch-resize, compositor-specific fallback, обновление egui.
- Переписывание прямоугольников в generic window-chrome abstraction.
- Изменения workflow, упаковки или backend.

**По KISS/YAGNI/Pareto:** сохранить почти весь PR, исправить один участок выбора начала жеста и закрепить его точными тестами. Не превращать локальный баг resize в проект по переписыванию управления окнами.

## Current Checkpoint
- Closes: R4.
- Smallest next action: push this execution-state update to retry fork CI on unchanged runtime code, verify that head, then perform guarded squash merge and main workflow/release verification.
- Expected evidence: corrected PR head checks and five native jobs succeed; publish skips on the feature branch; merged-main workflow and release then succeed.
- Stop or replan if: PR head/base changes, branch update is rejected, or a mandatory workflow gate fails.

## Current State
- Resolved: R1, R2, R3; all runtime edits remain inside `resize_edges`, tests reuse the existing harness, no dependency/backend/workflow change.
- Last relevant evidence: corrected runtime passed fork checks and four native builds; the fifth build stopped on a download connection reset before compilation. Local gates and automated native smoke pass.
- Blocker: fork Actions rerun API returns 403; repository permissions confirm read-only access outside the modifiable PR branch. A docs-only execution-state push can trigger a fresh workflow without altering runtime or CI.
- Next: R4 branch CI, guarded merge, main workflow/artifacts/release verification.

## Material Decisions
- 2026-10-14: execute the approved narrow plan; successful final workflow is the user's finish line, not merely a local green test.
- 2026-10-14: retry the observed transient download failure through this necessary execution-state commit because fork Actions reruns are forbidden; do not change packaging/workflow code for an isolated network reset.

## Checkpoint History
- 2026-10-14: R1 verified; original PR and main SHAs pinned, plan frozen before runtime edits.
- 2026-10-14: R2 verified by red→green reproduction (4 failures→12 passes); R3 verified by exact-toolchain gates and actual Wayland/XWayland smoke. The first combined workspace command hit the shell's 120-second timeout without a test failure; the standalone background workspace run completed successfully. Temporary smoke navigation needed frame warm-up before summary double-click; production code did not change.
- 2026-10-14: correction pushed as `2053e5b`; fork run `38548411445` verified checks and four native builds, with an openSUSE download connection reset. Rerun API was denied (403); next checkpoint retries via the execution-state update.

## Completion
- Resolved outcomes: R1, R2, R3.
- Commands and artifacts: local gates/native evidence above; final workflow pending.
- Constraint and diff-scope check: implementation reviewed against the approved local runtime/test envelope; integration closure pending.
- Final status: active.
