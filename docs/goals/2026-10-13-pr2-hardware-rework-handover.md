# Handover: переработка PR #2

> Завершено следующим агентом 2026-10-13; актуальный статус — в goal-файле.

Дата: 2026-10-13. Реализация передаётся другому агенту **по явному запросу пользователя**.
Это snapshot передачи, а не новый план и не заявление DONE. Авторитетный execution contract:
[`2026-10-13-pr2-hardware-rework.md`](2026-10-13-pr2-hardware-rework.md).

## 1. Задача и разрешения

Пользователь сначала попросил проверить PR https://github.com/0FL01/win10-task-manager-on-linux/pull/2,
затем предложить сохраняющие функции исправления и провести параллельный аудит плана.
После аудита разрешил: «Делай копию плана в цеть и итеративно реализовать и коммит пуш».
Последняя инструкция: сохранить handover, чтобы другой агент доделал работу.

Нужно довести скопированный план A/B/C/D до проверенного результата, коммитить и пушить
проверенные checkpoints, после каждого push проверять CI/artifacts именно его SHA.
Merge в main, публикация release, host-root установка и платная инфраструктура **не разрешены**.
Число коммитов не фиксировано: не дробить взаимозависимые изменения на некомпилируемые состояния.

Сохранять функции PR: Light/Dark/System, Win10 layout, System/copy/topology, Memory slots/SPD,
voltage/EDAC/endurance, KDE shortcuts/single-instance, временный source-mode и optional permanent helper.
KISS/YAGNI: один специализированный worker, без общих actor/job/NNG/reliable-event frameworks,
журналов rollback, новых adapters и disk-incarnation framework.

## 2. Git: точное состояние

- Workspace: `/home/stfu/ai/trash-can/task-manager`.
- Ветка: `fix/pr2-hardware-rework`.
- HEAD: `24194cfb811f0780d9da9ec327c57874fae4ec7b` — **исходный reviewed PR**, не новый rework commit.
- Main остаётся `d69478a60ae3dd7f68c74d4a90e1f2f1ad7e1495`.
- Origin: `git@github.com:0FL01/win10-task-manager-on-linux.git`.
- Ветка создана от `origin pull/2/head`; HTTPS fetch ранее упал с TLS EOF, существующий origin fetch сработал.
- **Ни одного нового коммита и push ещё нет. Изменения не staged.**
- До handover было 38 изменённых tracked files, плюс untracked worker/tests/SPD fixtures/goal.
  `git diff --stat` не включает untracked; нельзя забыть их при staging.
- Работавшие четыре implementation subagent завершены, больше никто файлы не редактирует.

Не сбрасывать рабочее дерево и не checkout main поверх него. Не force-push, не amend, не создавать PR
или release без отдельного запроса. Перед commit читать status/diff/log, stage только intended files.

## 3. Что уже реализовано

### A. Hardware worker и UI lifecycle

Основные файлы:

- `crates/mc-magpie/src/hardware_access.rs` — transport, trusted installed helper selection,
  embedded command/payload construction, child/lifetime ownership, публичные status types.
- `crates/mc-magpie/src/hardware_worker.rs` — новый private специализированный worker.
- `crates/mc-magpie/src/hardware_worker_tests.rs` — новый файл тестов, добавлен **последней правкой**.
- `crates/mc-magpie/src/{gatherer,lib,spd}.rs`.
- `crates/mc-ui/src/{app,cx,dialogs}.rs`, `perf/{mod,cpu,memory,system,mem_slots}.rs`.

Контракт текущей реализации:

- Hardware requests направляются `MagpieClient::send` прямо в hardware handle, не в обычную gatherer queue.
  Actor не использует NNG `Client`. Gatherer больше не вызывает polkit/helper/SPD inline.
- Одна foreground operation, `sync_channel(1)`, atomic admission gate; `send -> bool` сообщает отказ сразу.
  Foreground gate освобождается после успешного enqueue terminal completion, не после повторного root action.
- Worker опрашивает pending child короткими тиками; во время установки может продолжать Temporary sampling.
  Один latest-slot для telemetry/tombstone; completion и unsolicited status удерживаются bounded при full event queue.
- IDs сопоставляют acknowledgements; отдельный access epoch инвалидирует telemetry/SPD/readings.
- `Readings.hardware_epoch` имеет serde-default 0 для legacy fixtures.
- `HardwareStatus`: epoch, mode (`Off/Temporary/Persistent`), sampling bool, grant (`Absent/Present/Unconfirmed`).
  Sampling error не означает успешного revoke и не стирает сохранённую preference.
- Temporary read failure закрывает lifetime pipe; Persistent transport failure сохраняет revoke obligation.
  Structured permanent denial может подтвердить отсутствие только через `revoked: true`.
- Failed install/verification сохраняет Temporary. Успех проверяет **явный permanent endpoint** перед закрытием Temporary.
- Cancel/timeout после root launch не объявляет child остановленным: terminal result только после наблюдаемого exit.
  Root mutation lock обеспечивает cross-restart порядок revoke. Нет обещания мгновенной отмены kernel D-state.
- Unix transport: nonblocking connect/write/read + единый 3-секундный deadline и cancellation polling;
  newline JSON frame, лимит 8192 для telemetry / 131072 для SPD.
  `call_json` получает `Option<u64>`: None — periodic sampling, Some(id) — foreground,
  Some(0) — reconciliation probe без cancellation конкретного запроса.
  Periodic I/O уступает disable/cancel; это interruption не уничтожает работающую Temporary при отмене install.
- Local SPD reader проверяет общий deadline/cancel между bounded reads. Uninterruptible kernel I/O не маскируется
  как гарантированно прерываемое; shutdown не делает бесконечный join на hardware worker.
- Fixture/synthetic hardware operations возвращают unsupported без host elevation.
- UI pending задаётся после admission и снимается только matching completion. Sampling notification не ack install.
  SPD request стал typed `Command::ReadSpd`; отмена — `Command::CancelHardware`.
- Disable немедленно очищает helper-derived CPU/Memory/System metadata, privileged SPD и stashed readings,
  даже paused, без replay `DevicePage::update` и без дополнительного graph sample.
  CPU/Memory хранят ordinary fallback rows; System identity отделена от ordinary rows.
- UI сверяет atomic hardware epoch до drain событий: full event queue не задерживает privacy invalidation.
  Старые snapshots/SPD отсеиваются по epoch, old completion — по ID.
- Disable во время pending operation запрашивает cancel и после matching completion выполняет deferred revoke.
  Status dialog содержит separate access/grant/sampling, retry access, retry revoke и cancel; Close не отменяет.

Новые hardware request/event signatures:

```rust
Request::ReadSpd { id: u64 }
Request::InstallHardwareService { id: u64 }
Request::SetHardwareAccess { id: u64, enabled: bool, interactive: bool }
Request::CancelHardware { id: u64 }

Event::HardwareAccess {
    id: Option<u64>, // None = unsolicited status, не ack
    status: HardwareStatus,
    result: Result<(), String>,
}
Event::Spd {
    id: u64, epoch: u64, privileged: bool,
    result: Result<Vec<SpdEeprom>, String>,
}
```

**Конкретный незакрытый R2 follow-up по последнему diff:** в `hardware_worker.rs::sample`
сейчас `and_then` сводит валидный structured negative reply и transport error к одному `Err(String)`;
затем `sampling_interrupted()` может вернуть early return. Если cancel флаг появился после получения
валидного permanent `enabled:false, revoked:true`, можно обновить grant до Absent, но пропустить
sampling=false/epoch invalidation и оставить paused metadata. Это обнаруженная по control flow гонка,
ещё **не воспроизведённая тестом**. Следующему агенту проверить минимальным deterministic test и
сохранить различие `Ok(negative Reply)` / transport interruption: cancellation не должна игнорировать
уже полученный authoritative denial. Не решать через regex/keywords в error string или общий framework.

### B. Helper, installation, grant и packages

Файлы: `support/taskmanager-hardware`, `support/test-hardware-helper.py`, `xtask/src/dist.rs`,
`packaging/rpm/taskmanager.spec`, `support/ci/verify-native.sh`, `support/ci/tests/test_verify_native.py`,
`docs/hardware-monitoring.md`.

- Installed helper в `/usr` или `/usr/local` проверяется на root ownership, non-symlink protected ancestors,
  отсутствие group/world write и executable mode. Unsafe existing candidate — error, не embedded fallback.
- Installed route: `/usr/bin/pkexec <trusted-helper> enable <GUI_PID>`.
  GUI install с уже установленным helper тоже authorizes `enable`, не переписывает package assets.
  Denial/failed child — terminal, второго embedded prompt нет.
- Missing-helper source route сохранён: `/usr/bin/pkexec /usr/bin/python3 -I -c <compiled-code>`.
  Честно документировано: это root code данной сборки, не независимая read-only privilege boundary.
- Embedded API: `session <GUI_PID> <32hexTOKEN>` и `install <GUI_PID>`.
  stdin piped и открыт всю операцию; EOF/pidfd отменяют helper lifetime. PID принадлежит `PKEXEC_UID`.
- GUI bootstrap fixed layout: `/usr/local/libexec/taskmanager-hardware`,
  `/usr/local/lib/systemd/system/taskmanager-hardware.{service,socket}`,
  policy **в `/usr/share/polkit-1/actions`**, не `/usr/local/share`.
- Nonreplacement publication, refusal conflicts/symlinks/masks/overrides, actual unit-path/FragmentPath/pkaction discovery.
  Partial assets после отмены не откатываются и не переписываются повторным bootstrap.
- Один stable-inode root `.operation.lock` в `/var/lib/taskmanager-hardware` для install/enable/revoke.
  Lifetime проверяется после lock и перед grant; root helper сам cancels/reaps фиксированные subprocesses.
  Read/SPD не берут mutation lock; serial-server revoke берёт `LOCK_NB`, возвращает busy/unconfirmed.
- SO_PEERCRED/grant до recv: unauthorized silent client не занимает сервер на двухсекундном чтении.
  Даже pre-receive denial подтверждает absence только под свободным общим lock.
- **Только `revoked: true` подтверждает permanent absence.** `enabled:false`, busy, omitted revoked,
  errors и missing socket не доказывают revoke. Temporary disable не подтверждает permanent absence.
- CLI success exit 0, errors exit 1; worker верифицирует endpoint, не разбирает произвольный stdout.
- `xtask` поддерживает helper `/usr` и `/usr/local` с discoverable policy; произвольный app prefix допустим,
  optional helper там явно skipped. openSUSE отдельно recommends `pkexec`; native verifier проверяет prerequisites
  до установки дополнительных smoke-test dependencies.
- Fixed sensor/register allowlists и Mission Center attribution сохранены.

### C. SMART backend cache — реализован и проверен

Файлы: `subprojects/magpie/platform/src/{disks,lib}.rs`,
`platform-linux/src/disks/{manager,smart_data}.rs`, `types/src/disks.proto`, `types/build.rs`,
`subprojects/magpie/src/disks.rs`, `crates/mc-magpie/src/{client,gatherer}.rs`.

- `DiskList.smart_data`: protobuf map tag **2**, serde-default; прежние теги не менялись.
- `Client::disks_snapshot() -> magpie_types::disks::disks_response::DiskList`.
  Gatherer получает disks и SMART одним обычным запросом; frontend automatic `smart_data()` polling удалён.
  Private unused legacy `Client::disks_info` alias удалён; manual `Request::SmartData` сохранён.
- Existing backend pool, один automatic job queued/running globally, reservation до queueing.
  Coherent snapshot/cache mutex не держится во время UDisks.
- Полный async lookup/SMART/properties deadline **10 секунд**; это engineering default, не измерение.
  Успех TTL 300 секунд от completion, failure backoff 30 секунд; deterministic round-robin без starvation.
- Expired/missing data omitted, unknown не превращается в zero. `last_update_time` UDisks сохраняется.
- Нужен observed nonblank serial или WWN. Prune changed/absent identities, generation rejects stale completion.
  Это только **наблюдаемые** inventory changes, не гарантия обнаружения физической замены между refreshes.
- `auth.no_user_interaction=true` только automatic path. Manual оставляет прежние empty options/response/dialog semantics.
- Old daemon даёт empty map; **никакого automatic legacy SMART fallback**.

### D. Concrete retained-feature fixes

- `types/src/memory.proto`: optional `present = 13`, `speed_configured = 14`; старые tags/JSON сохранены.
- `platform-linux/src/memory.rs`: PRESENT=0 => Empty; missing/invalid/zero size не делает слот Empty;
  configured/rated speed provenance. UI unknown module сохраняет metadata/SPD.
- `perf/{mem_slots,memory,system}.rs`: unknown speed не `0 MT/s`, нейтральная SMBIOS speed label при fallback,
  неполная сумма названа partial, occupied filter учитывает presence.
- `platform-linux/src/hardware.rs`: пропуск исчезнувшего hwmon, EDAC iterator error => unknown totals.
- `platform-linux/src/cpu/topology.rs`: sparse/replaced same-count IDs, unsupported CPUID cases.
- `crates/mc-core/src/spd.rs`: tests only, decoder production logic не меняли.
  Untracked `crates/mc-core/tests/fixtures/spd/{README.md,coreboot-ddr3-base.hex,coreboot-ddr4-base.hex}`:
  независимые публичные anonymized coreboot base blocks с pinned provenance/hash/CRC.
- `apps/taskmanager/src/{desktop,main}.rs`: bus unavailable отдельно от known-owner failed handoff;
  одна nonqueued reclaim попытка после disappearance, continuing conflict => exit 1, не duplicate.
- `win10/{chrome,paint}.rs`: close hover/pressed white ink не remap в dark surface; geometry сохранена.
- `mc-ui/tests/win10_probes.rs`, `docs/win10-spec/spec.md`: meaningful dark/theme/no-new-reading tests,
  memory Unknown/SPD access probes и локальная own-design ink annotation.

Не «чинить» старые неподтверждённые review claims: Desktop Entry Exec escaping здесь корректно двухэтапное;
DDR5 Mini-UDIMM mapping bug не был доказан authoritative source и не включён в эту переработку.

## 4. Проверки: что действительно запускалось

### Parent, последний актуальный backend/Python run — GREEN

```sh
/usr/bin/python3 -B support/test-hardware-helper.py
/usr/bin/python3 -B -m unittest discover -s support/ci/tests
cargo +1.96.1 fmt --manifest-path subprojects/magpie/Cargo.toml --all --check
CARGO_HOME="$TMPDIR/taskmanager-network-cargo" \
PKG_CONFIG_PATH="$TMPDIR/taskmanager-network-cargo:$PWD/target/cleanup-native-dev/usr/lib64/pkgconfig" \
cargo +1.96.1 test --manifest-path subprojects/magpie/Cargo.toml \
  -p magpie-platform -p magpie-platform-linux -p magpie-types -p magpie --locked --release
```

Результаты: helper **27**, Python CI suite **27**, backend **88 unit + 1 doc** passed.
Native nvtop C warnings были в generated dependencies, Rust test failures не было.
GBM metadata workaround выше **проверен**, не считать прежний missing `gbm.pc` нерешённым blocker.
При новом TMPDIR проверить существование cache; также есть `target/cleanup-cargo-home`.

### Parent, предыдущая green версия focused Rust

`cargo +1.96.1 test --locked -p mc-magpie -p mc-ui --lib`: **31 mc-magpie passed / 2 ignored + 46 mc-ui passed**.
Включены backpressure, child cleanup, failed verify preservation, revocation ack,
stale SPD и paused metadata/stash invalidation.
**После этого добавлены/изменены transport cancellation, command-route/integration tests и test layout.
Этот результат НЕ является green gate окончательного текущего diff.**

### Последний broad Root gate — остановился на Clippy, исправления пока не проверены

Запускался chain:
`cargo +1.96.1 fmt --all && cargo +1.96.1 clippy --locked --workspace --all-targets -- -D warnings && cargo +1.96.1 test --locked --workspace`.
Clippy сообщил ровно три test-only diagnostics:

1. `hardware_access.rs`: не обработан count `stream.read` в fake server.
2. `hardware_worker.rs`: items after test module.
3. `hardware_worker.rs`: ещё один необработанный read count.

Последняя implementation patch:
- test module вынесен в `hardware_worker_tests.rs`, declaration поставлена в конец worker;
- read count проверяется;
- strengthened tests: Temporary child ownership after failed verify, distinct permanent/temporary endpoints,
  failed authorization terminal/no fallback.

После неё при подготовке handover **успешно** выполнены:

```sh
cargo +1.96.1 fmt --all
cargo +1.96.1 fmt --all --check
git diff --check
```

**Clippy и tests после этой последней правки НЕ запускались. Workspace tests из broad chain вообще
не дошли до выполнения, поскольку Clippy остановил `&&`. Первый шаг следующего агента — проверить это.**

### Delegated evidence, реально сообщённое завершёнными агентами

- Helper/staging: xtask **16 passed**, xtask exact-toolchain Clippy, Python helper **27**,
  native verifier tests **7**, shell syntax/owned formatting passed.
- SMART: backend release build/tests и changed-package Clippy passed; frontend compatibility **2** passed.
- Data: core **77** + UI **44** passed, focused memory probes/core/UI Clippy passed.
  Изначальная agent Linux build проблема GBM закрыта parent backend command выше.
- Presentation: full Win10 probes **27 passed / 2 ignored**, desktop ordinary **1 passed / 5 ignored**;
  explicitly isolated D-Bus **4 passed** (success/rejection/disappearance/race), app/UI Clippy passed.
  Это evidence до поздних parent integration changes, не заменяет fresh final gates.

## 5. Что делать дальше — минимальный critical path

1. Прочитать AGENTS.md и authoritative goal; inspect status/diff, включая untracked worker/tests/fixtures.
2. Fresh exact-toolchain Root Clippy; исправить реальные diagnostics, не suppress ошибки и не ослаблять тесты.
   Потом `cargo +1.96.1 test --locked --workspace -- --test-threads=1` (wgpu adapter нужен для probes).
   Фокус при failure: последние worker/test layout changes, admission/cancel/event retention,
   paused/stashed epoch invalidation и helper preservation.
   До признания R2 закрытым проверить/устранить negative-reply/cancel race, описанную в §3A.
3. Воспроизвести явно ignored isolated D-Bus tests:
   ```sh
   dbus-run-session -- cargo +1.96.1 test --locked -p taskmanager-app \
     desktop::tests:: -- --ignored --test-threads=1 --skip existing_desktop_shortcut_is_not_stolen
   ```
4. Выполнить remaining CI gates, включая diagnostic-profile:
   ```sh
   cargo +1.96.1 test --locked -p mc-magpie --features diagnostic-profile
   CARGO_HOME="$TMPDIR/taskmanager-network-cargo" \
   PKG_CONFIG_PATH="$TMPDIR/taskmanager-network-cargo:$PWD/target/cleanup-native-dev/usr/lib64/pkgconfig" \
   cargo +1.96.1 test --manifest-path subprojects/magpie/Cargo.toml \
     -p magpie-platform --features diagnostic-profile --locked --release
   ```
   Backend tests above покрывают dispatcher/platform cache сверх минимального AGENTS command.
5. Обновить directly affected docs/POT и evidence после проверки, не начинать соседний cleanup.
   **Известные stale docs:** `docs/hardware-monitoring.md` SMART paragraph ещё описывает frontend per-cycle polling
   и blanket noninteractive SMART; нужно описать backend automatic cache/manual separation.
   Там же старое обещание clear «on refreshed snapshot» вместо immediate invalidation.
   Старые native KDE claims/ограничения в Verification не выдавать за выполненный этим rework live gate.
6. Проверить R7 в disposable native KDE/systemd/polkit среде: install/enable/restart/revoke/re-enable,
   pkaction и unit discovery, free-key closed launch, conflict preservation и handoff.
   VM/mock не сертифицирует MSR/MMIO/SPD; physical adapters честно отметить unverified, где нет независимого evidence.
7. Коммитить coherent verified snapshot/checkpoints, push **эту work branch**, ждать CI каждого pushed SHA.
   Перед push обязательно exact Rust **1.96.1** Clippy. Обновлять goal только evidence, не делать status complete до closure.

## 6. CI, environment и boundaries

- `.github/workflows/native-release.yml` запускается push на любую ветку.
- Required jobs: `metadata`, `checks`, native Fedora44/openSUSE-Tumbleweed/Arch/Debian13/Ubuntu26.04.
  Проверить `native-*` package artifacts/build manifest; `ui-snapshots` загружается при наличии renders.
  `publish` на этой не-main ветке **ожидаемо skipped**, release не создавать.
- После push использовать GitHub Actions tools и проверить exact head SHA, все jobs и ожидаемые artifacts.
  GitHub CLI `gh` сейчас не найден; read-only API/tools уже доступны. Push permission пока не проверен.
- Rust 1.96.1 установлен. Root Cargo cache работает обычным запуском; backend cache/GBM путь приведены выше.
- Helper/tests запускать `/usr/bin/python3`: PATH-selected Conda Python не имеет `os.pidfd_open`.
- Root/actual polkit/systemd installation **не выполнялась**. Нельзя выполнять её на host вместо disposable environment.
- Host: user UID 1000, systemd running, `plasma_session` и `pkaction` доступны.
  Docker info успешно: **29.8.1**, свободно около **102 GiB** на workspace filesystem при проверке.
  QEMU/virt-install не найдены в PATH, `podman machine list` пуст.
  Docker/software-VM альтернативы **ещё не исследованы**; это не доказанный внешний blocker.
- Не воровать Ctrl+Shift+Esc у пользовательского Mission Center. Для native shortcut gate нужна изолированная среда.
- Real renders под `target/ui-snapshots/`, не добавлять их/`references/` в commit.
  Public SPD test fixtures можно commit; приватные process dumps — только ignored `testkit/fixtures/live`.

## 7. Статус передачи

R1/R4 verified; R2/R3/R5/R6 in progress; R7 pending. Goal остаётся **active**.
Остановка вызвана последней инструкцией пользователя о передаче, а не DONE или внешним BLOCKED.
Все implementation changes доступны в текущем working tree. Следующий агент может продолжить без повторного
полного PR-аудита: сначала fresh Clippy/tests, затем незакрытые evidence/commit/push/CI пункты.
