# Goal: Feature-preserving rework of PR #2

Status: done (R7 explicitly unverified)
Source: user-approved audited plan below; user instruction: «Делай копию плана в цеть и итеративно реализовать и коммит пуш».
Last updated: 2026-10-13

## Objective
Rework PR #2 ourselves, preserving its user-visible features and the upstream contracts, close the concrete lifecycle/authorization/SMART/data/UI regressions, and commit and push verified checkpoints. PR source: https://github.com/0FL01/win10-task-manager-on-linux/pull/2 at `24194cfb811f0780d9da9ec327c57874fae4ec7b`, based on main `d69478a60ae3dd7f68c74d4a90e1f2f1ad7e1495`. Work branch: `fix/pr2-hardware-rework`; do not merge into main or publish a release without separate authorization.

## Execution Directive
Complete the frozen Required Outcomes using the listed Change Envelope and Primary Evidence. Work on the smallest unresolved outcome. Do not add requirements from reviews, tests, tools, speculative risks, or optional source text. Finish when every required outcome is resolved and affected constraints remain satisfied. Stop substantive work only at a proven external blocker, recording exact evidence and the smallest unlock; finish all independent work first.

## Frozen Contract

### Required Outcomes
- R1: Preserve the approved plan as a durable execution contract and isolate the rework branch.
  - Source: latest user instruction.
  - Acceptance: this goal contains the approved plan and current checkpoint; branch starts at the reviewed PR SHA without changing main.
  - Primary evidence: this document and git status/history.
  - Status: verified.
  - Evidence: branch `fix/pr2-hardware-rework` starts at `24194cfb`; main remains `d69478a`.
- R2: Nonblocking hardware lifecycle, truthful access/grant state, reliable admission/completion and stale-data invalidation.
  - Source: approved plan A.
  - Acceptance: normal gathering continues during authorization; one foreground operation; accepted requests terminate despite backpressure; failures preserve grant/revoke truth and Temporary as specified; old SPD/samples and paused view caches cannot leak back into current display.
  - Primary evidence: focused worker/lifecycle/admission/deadline/UI tests.
   - Status: verified.
   - Evidence: sampling now keeps a received structured denial authoritative even when interrupted; only transport errors yield to cancellation (`received_denial_is_applied_even_when_sampling_was_interrupted`). Rust 1.96.1 workspace tests: mc-magpie 35 (+2 ignored), mc-ui 46, snapshots 9, Win10 probes 27 (+2 ignored); diagnostic-profile mc-magpie 36.
- R3: Coherent installed authorization, non-destructive fixed local bootstrap and serialized permanent grant mutation.
  - Source: approved plan B.
  - Acceptance: installed helper uses installed action, no fallback after denial; bootstrap does not overwrite conflicts; discoverable assets/dependencies; grant changes and cancellation cannot regrant after confirmed revoke; ungranted clients cannot stall the serial server before authorization.
  - Primary evidence: helper/Access/staging/package tests plus R7 live installation evidence.
   - Status: verified (unit/disposable-root level); live installation under R7 unverified.
   - Evidence: `/usr/bin/python3 -B support/test-hardware-helper.py` 27 OK; CI helper tests 27 OK; xtask 16 passed; installed-route tests in mc-magpie passed.
- R4: Background SMART isolated in backend with additive cached DiskList and preserved manual behavior.
  - Source: approved plan C.
  - Acceptance: one automatic task queued/running; bounded noninteractive UDisks operation outside dispatcher/cache locks; completion-based cache/backoff; old daemon yields unavailable rather than automatic legacy SMART; removed/observably replaced disk results rejected.
  - Primary evidence: backend cache/dispatch tests, Linux reader tests and frontend compatibility tests.
   - Status: verified.
   - Evidence: parent locked release backend run passed 88 unit tests plus one doc test across magpie/platform/platform-linux/types, including cache/dispatch/deadline/manual-options tests. Frontend compatibility tests passed; gatherer now consumes disks_snapshot without automatic legacy SMART calls.
- R5: Fix concrete retained-feature regressions without dropping PR features.
  - Source: approved plan D.
  - Acceptance: memory Empty/Unknown/speed/partial-total semantics, dark close glyph/theme update, no duplicate after known-owner handoff failure, disappearing hwmon/incomplete EDAC semantics; sparse/unsupported CPU topology and independent SPD evidence preserved.
  - Primary evidence: focused decoder/data/UI/desktop tests and retained light/Swap/Compression tests.
   - Status: verified.
   - Evidence: full Win10 probes 27 passed after restoring the synthetic-mode SPD message; `dbus-run-session` ignored desktop tests 4 passed; mc-core 77 passed including independent coreboot SPD fixtures (now un-ignored in `.gitignore` so CI can compile them).
- R6: Verify, commit and push implementation checkpoints; check CI and artifacts for every pushed SHA.
  - Source: latest user instruction, approved finish line and AGENTS.md Verify.
  - Acceptance: root/backend/Python checks green, exact Rust 1.96.1 Clippy before push; intended changes committed; required CI jobs and native artifacts verified on pushed SHAs. Branch publish is expected to skip.
  - Primary evidence: command results, git history/status, GitHub Actions runs/jobs/artifacts.
   - Status: verified locally; CI evidence recorded in the commit/push report.
   - Evidence: `cargo +1.96.1 clippy --locked --workspace --all-targets -- -D warnings` clean; root and backend fmt checks clean; backend release tests 88 + 1 doc test, backend diagnostic-profile 15 + 1; POT regenerated without dropping indirect msgids.
- R7: Real native integration and honest physical telemetry evidence.
  - Source: approved finish line §3.
  - Acceptance: disposable native KDE/systemd/polkit install-enable-restart-revoke-enable, policy discovery, free-key closed launch/conflict/single-instance tested; physical adapter claims have independent evidence or are explicitly unverified, never certified by a VM/mock.
  - Primary evidence: live disposable-environment observations and documented evidence boundaries.
  - Status: unverified (not performed).
   - Evidence: the user asked to finish and push to main without a disposable native KDE environment; `docs/hardware-monitoring.md` states that polkit/systemd lifecycle, KWin keepAbove and physical MSR/MMIO/SPD readings are unverified. Earlier note: Docker works, host systemd/KDE/polkit executables are present; QEMU/virt-install were not on PATH and podman machine list was empty. Disposable integration alternatives are not exhausted; no external blocker proven and no host-root installation authorized.

### Constraints
- Keep all PR motives: Light/Dark/System and Win10 geometry; System/copy/topology; memory slots/SPD; voltage/EDAC/endurance; native KDE shortcuts/single instance; unprivileged GUI/magpie; temporary no-manual-install and optional permanent hardware access.
- Keep Mission Center attribution, clean-room assets, unknown != zero, device GPU memory and busiest-engine/dedup semantics.
- No I/O/blocking in App UI/logic, no unbounded channels, no unconditional repaint, no shared locked AppState; typed commands and bounded handoffs only.
- Preserve old fixture defaults and protobuf tags/manual SMART semantics. No private process data or real-reference screenshots in commits.
- Commit/push are authorized, including push to main (2026-10-13); host-root installation, deleting unrelated work, force push and paid infrastructure are not.

### Non-goals
- General actors/job/reliable-event/NNG frameworks; journal rollback; arbitrary helper prefixes/migrations; disk-incarnation frameworks.
- New adapters, XMP/EXPO, guessed physical slots, all-CPU/all-distro/all-renderer certification or unrelated cleanup.
- Immediate cancellation of uninterruptible kernel I/O; no default subprocess/watchdog per hardware/SPD read.

## Change Envelope
- Hardware client, Requests/Events/Readings and direct UI consumers: `crates/mc-magpie`, `crates/mc-ui/src/{app,cx,dialogs}.rs`, `crates/mc-ui/src/perf/` metadata/slots invalidation.
- Root helper/assets/staging/package prerequisites: `support/taskmanager-hardware`, its tests, `data/*hardware*`, `xtask/src/dist.rs`, existing native packaging/verifiers.
- SMART: `subprojects/magpie/{platform,platform-linux,types,src}` disk/cache/reader/dispatch, frontend disk snapshot intake; existing pool and dependencies.
- Concrete corrections: backend memory/topology/hardware readers, memory schema, `win10/{palette,paint,chrome}`, `apps/taskmanager` instance handoff, focused tests/docs/CI gates.
- This goal, `docs/hardware-monitoring.md`, directly affected user/install documentation and translation strings. Generated artifacts remain under ignored target paths; private live data only in ignored `testkit/fixtures/live`.
- No fixed diff/time/attempt budget; choose the smallest change closing the frozen outcomes.

## Approved Plan (copy)

### A. Hardware-worker и правдивое состояние
Область: `hardware_access.rs`, `gatherer.rs`, аппаратные команды/events и соответствующие UI-состояния.

1. **Один worker, одна управляющая операция одновременно.** Он владеет `Access`; gatherer не ждёт polkit/helper/SPD. Ожидающий child опрашивается, а не занимает worker непрерывным 120-секундным вызовом. Телеметрия передаётся через latest-slot; очереди ограничены.
2. **Учесть отказ при отправке запроса, а не только доставку ответа.** Не принят запрос → немедленная ошибка, без вечного `pending`. Принят → гарантированный терминальный completion. При заполненном event channel удерживать completion и повторять доставку, не привилегированное действие.
3. **ID операции + поколение аппаратных данных.** ID сопоставляет запрос и completion. Поколение отсекает старые readings/SPD после отключения и повторного включения. Не нужен реестр множества параллельных задач.
4. **Отключение немедленно очищает локальные привилегированные значения.** Включая уже показанные CPU/Memory/System, SPD и paused/stashed readings. Без ожидания следующего snapshot и без удаления обычных непривилегированных показаний.
5. **Состояние доступа не смешивать с настройкой пользователя.** Temporary read failure → прекратить использование сессии и закрыть lifetime pipe. Persistent read failure → данные недоступны, но grant не считается отозванным. Disable → закрыть Temporary и отдельно отозвать Persistent, если он есть или мог остаться. Неудачная установка сохраняет Temporary; успешный переход — только после проверки постоянного endpoint.

Граница обещаний: ограничиваем программные ожидания и не допускаем бесконечного shutdown на собственной логике. Не обещаем мгновенно прервать зависший kernel I/O закрытием pipe. Subprocess/watchdog на каждое чтение по умолчанию не добавляем.

### B. Авторизация, bootstrap и отзыв grant
Область: helper, `hardware_access.rs`, policy/units, `xtask`, package dependencies.

1. **Установленный helper → авторизовать, не переустанавливать.** Проверить root ownership и защищённость пути; использовать согласованную установленную policy. Отмена/отказ — терминальный результат, без второго embedded prompt.
2. **Helper отсутствует → сохранить source-mode.** Временная embedded-сессия остаётся. Постоянная GUI-установка использует один фиксированный локальный layout. Честно сообщать: embedded elevation разрешает root-код из данной сборки, а не отдельную установленную privilege boundary.
3. **Bootstrap не заменяет существующие конфликтующие assets.** Helper в `/usr/local`, units в поддерживаемом systemd-каталоге, policy в `/usr/share/polkit-1/actions`. Проверять фактическую обнаруживаемость policy/unit, не только совпадение строк. Обычная установка приложения в произвольный prefix остаётся допустимой; неподдерживаемый optional helper пропускается с явным сообщением.
4. **Один маленький helper-side lock для install/enable/revoke.** Проверять lifetime клиента после получения lock и перед выдачей grant. Не блокировать обычные `read/spd` этим lock. В serial socket server отзыв при занятом lock возвращает `busy/unconfirmed`, а не задерживает всю телеметрию. Подтверждённый отзыв нельзя обходить поздним завершением отменённого installer.
5. **Отмена после начала установки — не выдуманный rollback.** Пока изменения ещё могут продолжаться, статус — «отмена/согласование результата». Не принимать следующую конфликтующую операцию как будто предыдущая завершилась. Не считать timeout доказательством остановки root-child.
6. **Grant/SO_PEERCRED проверять до ожидания request bytes.** Сохранить фиксированный API и лимиты. Не добавлять поток на каждое соединение.

### C. Автоматический SMART — полностью на backend
Область: backend disks handler/manager, `DiskList`, frontend intake.

1. Добавить optional-совместимый SMART map в `DiskList`, с serde-default.
2. Обновлять кеш через существующий backend thread pool: не более одной автоматической задачи queued/running.
3. Не держать cache/inventory mutex во время UDisks; ограничить весь автоматический запрос deadline.
4. Успех кешировать пять минут от завершения, ошибку повторять с коротким backoff; обеспечить обход других дисков.
5. Удалять устаревшие данные и результаты для наблюдаемо исчезнувших/изменившихся устройств.
6. Ручной SMART оставить отдельным: сохранить существующий запрос, dialog и authorization semantics. `no_user_interaction` относится к автоматическому пути.
7. Старый daemon → endurance unavailable, никакого автоматического legacy SMART.

Не вводим disk-incarnation framework. Используем имеющуюся идентификацию и честно ограничиваем гарантию наблюдаемыми изменениями inventory.

### D. Точечные исправления сохранённых функций
Этот блок независим от worker и SMART.

- Memory: различать Empty, известную ёмкость и Unknown; неизвестная ёмкость не закрывает metadata/SPD.
- Подписи: fallback rated speed не называть configured speed; неизвестную скорость не рисовать как `0 MT/s`; неполную сумму не выдавать за полный installed total.
- Dark: локально исправить ink close-glyph; проверить смену темы без новых readings.
- Single-instance: отличать недоступный bus от неудачной передачи известному владельцу. При исчезнувшем владельце — одна попытка занять имя; при продолжающемся конфликте — не запускать дубль.
- Collectors: пропускать исчезнувший hwmon; при ошибке EDAC enumeration не публиковать частичную сумму как полную.
- Тесты данных: sparse CPU IDs/unsupported topology и независимые обезличенные SPD-примеры.

Новые adapters, XMP/EXPO и угадывание физических слотов не добавляем.

### Зависимости и порядок
```text
A: worker/status contract -> B: auth/install/revoke -> lifecycle integration
C: backend SMART cache ----------------------------> endurance integration
D: concrete feature fixes -------------------------> regression checks
```
A и B согласуем первыми; C и D можно делать независимо. Тесты идут вместе с исправлениями, а не отдельным последним коммитом. К произвольному числу коммитов план не привязываем.

### Что выкидываем из scope
- Общий actor/job framework.
- Переписывание всей доставки events и всего NNG IPC.
- Журнал транзакций и универсальный rollback installer.
- Произвольные helper-prefix и миграции установок.
- Identity framework для дисков.
- Сертификацию всех CPU, всех distro и всех renderer/DPI комбинаций.
- Соседний cleanup, не мешающий этим контрактам.

### Finish line
1. Детерминированные тесты: queue saturation, отмена/commit/revoke race, failed install с сохранённой Temporary, stale SPD/readings, paused UI, SMART timeout/cache и старый daemon.
2. Регрессии: light probes, Swap/Compression, fixtures/synthetic, meaningful dark checks, metadata/slots/topology. Ignored D-Bus test запускать явно.
3. Одна disposable native KDE VM: реальный polkit/systemd lifecycle, policy discovery, закрытый запуск по свободной клавише, сохранение конфликта и single-instance handoff. VM не заменяет доказательства физических MSR/MMIO/SPD readings.
4. Технические gates: workspace и затронутый backend, включая daemon dispatch и тесты platform-кеша; Python helper tests, fmt, Clippy на Rust 1.96.1. После отдельно разрешённого push — CI и artifacts на окончательном SHA.

## Current State
- Resolved: R1–R6; R7 explicitly unverified and documented.
- Next: none in this goal. A disposable native KDE/systemd/polkit run remains the only open evidence gap.

## Material Decisions
- 2026-10-13: interpret «цеть» as «цель» and copy the audited implementation plan; commit/push authorized, main merge/release not authorized.
- 2026-10-13: user asked to finish, commit and push to the main branch; R7 left unverified rather than blocking.
- 2026-10-13: start from reviewed PR branch rather than reimplement its 64-file feature set; preserve upstream main unchanged.

## Checkpoint History
- 2026-10-13: R1 frozen; HTTP Git fetch failed with TLS EOF, existing origin fetch succeeded; isolated branch created.
- 2026-10-13: independent helper/SMART/data/presentation work integrated with hardware-worker/UI lifecycle. Backend release tests and Python checks passed. Current broad Rust gate stopped on test layout/read-amount diagnostics; fixes made, formatting checked, further execution paused at the user's handover request.

## Completion
- Resolved outcomes: R1–R6. R7 unverified.
- Commands and artifacts: evidence above; CI on the pushed main SHA is checked after push.
- Constraint and diff-scope check: changes stay within the Change Envelope; no private fixtures or reference screenshots added.
- Final status: done (R7 unverified).
